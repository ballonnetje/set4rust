//! Load every shipped script and every map-embedded script against a logging stub host and
//! report Lua errors.
//!
//! * `Script\**\*.txt` map scripts: load + start (`register_functions`, `new_game`), then `ticks`
//!   logic ticks (default 3000, ~3.5 game minutes) of event dispatch.
//! * `Script\Internal\StartResources.txt`: all three `CreateStartResources*` for each race.
//! * `Script\Internal\CheckInstallation.txt`: in its own state with `CheckFile`/`CheckFiles` and the
//!   flag globals only, as `CInstallationInfo::CheckWithLuaScript`.
//! * `Map\**\*.map`/`*.edm`: chunk 16 (`MapFile::script`) loaded + started + ticked the same way,
//!   and for every map the script a game would actually run (`resolve_map_script`).
//!
//! Usage: `cargo run --release -p s4-script --example script_check [ticks]`
//! (`SCRIPT_DUMP=<dir>` also writes each chunk-16 script to `<dir>`).

use s4_script::{Arg, ScriptEngine, ScriptHost, ScriptSource, resolve_map_script};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Per-entry instruction budget: the stub answers 0 to almost everything, which sends some scripts
/// into loops the real game data never triggers (e.g. MCD2_roman2's tower scan with 0 towers).
const LIMIT: u64 = 2_000_000;

#[derive(Default)]
struct Stub {
    calls: BTreeMap<&'static str, usize>,
    race: i32,
    minutes: i32,
}

impl ScriptHost for Stub {
    fn unhandled(&mut self, func: &'static str, _args: &[Arg]) {
        *self.calls.entry(func).or_default() += 1;
    }
    // A few answers that keep scripts on their normal paths.
    fn game_local_player(&mut self) -> i32 {
        *self.calls.entry("Game.LocalPlayer").or_default() += 1;
        1
    }
    fn game_player_race(&mut self, _player: i32) -> i32 {
        *self.calls.entry("Game.PlayerRace").or_default() += 1;
        self.race
    }
    fn game_time(&mut self) -> i32 {
        *self.calls.entry("Game.Time").or_default() += 1;
        self.minutes
    }
    fn game_random(&mut self, n: i32) -> i32 {
        *self.calls.entry("Game.Random").or_default() += 1;
        if n > 0 { n / 2 } else { 0 }
    }
}

fn walk(dir: &Path, exts: &[&str], out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, exts, out);
        } else if p.extension().is_some_and(|x| exts.iter().any(|e| x.eq_ignore_ascii_case(e))) {
            out.push(p);
        }
    }
}

/// `CInstallationInfo::CheckWithLuaScript`: `CheckFile`/`CheckFiles` plus the flag globals.
fn check_installation(src: &[u8], name: &str) -> mlua::Result<usize> {
    let lua = mlua::Lua::new();
    let n = std::rc::Rc::new(std::cell::Cell::new(0));
    for (flag, v) in [("MUST_NOT_EXIST", 65536), ("NOT_INTERNAL", 131072), ("OPTIONAL", 262144), ("PROD_NOT_EXTERNAL", 524288)] {
        lua.globals().set(flag, v)?;
    }
    for func in ["CheckFile", "CheckFiles"] {
        let n = n.clone();
        lua.globals().set(func, lua.create_function(move |_, _: mlua::MultiValue| {
            n.set(n.get() + 1);
            Ok(())
        })?)?;
    }
    lua.load(src).set_name(name).exec()?;
    Ok(n.get())
}

#[derive(Default)]
struct Tally {
    ok: usize,
    init: usize,
    ticks: usize,
    /// scripts that ran without Lua errors but triggered the original's API warnings
    diag: usize,
    calls: BTreeMap<&'static str, usize>,
}

/// Load + start + tick one map script; prints one line.
fn run_map_script(src: &[u8], name: &str, ticks: u32, t: &mut Tally) -> anyhow::Result<()> {
    let mut e = ScriptEngine::new(Stub::default())?;
    e.set_instruction_limit(Some(LIMIT));
    let compiled = e.load(src, name)?;
    e.start()?;
    let init_errors = e.errors();
    let registered = e.registered_count();
    for tick in 1..=ticks {
        // a logic tick is 1/14 s; the stub's clock follows it
        e.host_mut().minutes = (tick / (14 * 60)) as i32;
        e.update(tick)?;
    }
    let all = e.errors();
    let tick_errors = &all[init_errors.len()..];
    for (k, v) in &e.host().calls {
        *t.calls.entry(k).or_default() += v;
    }
    let first = |v: &[String]| v.first().map(|s| s.lines().next().unwrap_or("").to_string()).unwrap_or_default();
    let diags = e.diagnostics();
    let diag = if diags.is_empty() {
        String::new()
    } else {
        t.diag += 1;
        format!(" [{} API warnings, first: {}]", diags.len(), first(&diags))
    };
    if !compiled || !init_errors.is_empty() {
        t.init += 1;
        println!("FAIL  {name}: init: {}{diag}", first(&init_errors));
    } else if !tick_errors.is_empty() {
        t.ticks += 1;
        println!("TICK  {name}: {} errors in {ticks} ticks, first: {}{diag}", tick_errors.len(), first(tick_errors));
    } else {
        t.ok += 1;
        println!("{}  {name}: {registered} functions registered{diag}", if diag.is_empty() { "OK  " } else { "WARN" });
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let ticks: u32 = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(3000);
    let game = s4_formats::game_dir();
    let root = game.join("Script");
    let mut files = Vec::new();
    walk(&root, &["txt"], &mut files);
    files.sort();

    println!("== Script\\**\\*.txt ({} files)", files.len());
    let mut t = Tally::default();
    let mut skipped = 0;
    for f in &files {
        let rel = f.strip_prefix(&root).unwrap_or(f).display().to_string();
        let name = f.file_name().unwrap().to_string_lossy().to_lowercase();
        let src = std::fs::read(f)?;
        if name == "readme.txt" {
            skipped += 1;
            println!("SKIP  {rel} (not Lua)");
            continue;
        }
        if name == "checkinstallation.txt" {
            match check_installation(&src, &rel) {
                Ok(n) => {
                    t.ok += 1;
                    println!("OK    {rel}: {n} CheckFile/CheckFiles calls")
                }
                Err(e) => {
                    t.init += 1;
                    println!("FAIL  {rel}: {e}")
                }
            }
            continue;
        }
        if name == "startresources.txt" {
            let mut errs = Vec::new();
            for race in 0..5 {
                let mut e = ScriptEngine::new(Stub { race, ..Default::default() })?;
                e.set_instruction_limit(Some(LIMIT));
                for amount in 1..=3 {
                    e.create_start_resources(&src, amount, 100, 100, 1, 0)?;
                }
                errs.extend(e.errors());
                for (k, v) in &e.host().calls {
                    *t.calls.entry(k).or_default() += v;
                }
            }
            if errs.is_empty() {
                t.ok += 1;
                println!("OK    {rel}: 5 races x 3 amounts")
            } else {
                t.init += 1;
                println!("FAIL  {rel}: {:?}", errs)
            }
            continue;
        }
        run_map_script(&src, &rel, ticks, &mut t)?;
    }
    println!(
        "Script files: {} without Lua errors ({} of them with API warnings), {} failed load/init, {} failed only during ticks, {skipped} skipped",
        t.ok, t.diag, t.init, t.ticks
    );

    // Map-embedded scripts and per-map resolution.
    let map_root = game.join("Map");
    let mut maps = Vec::new();
    walk(&map_root, &["map", "edm"], &mut maps);
    maps.sort();
    println!("\n== Map\\**\\*.map|edm ({} files), chunk 16", maps.len());
    let mut mt = Tally::default();
    let (mut no_chunk, mut redirects, mut bad_maps) = (0, 0, 0);
    let mut resolved = BTreeMap::<&'static str, usize>::new();
    let mut missing_files = Vec::new();
    // SCRIPT_DUMP=<dir>: also write every chunk-16 script there (for offline surveys)
    let dump = std::env::var_os("SCRIPT_DUMP").map(PathBuf::from);
    if let Some(d) = &dump {
        std::fs::create_dir_all(d)?;
    }
    for m in &maps {
        let rel = m.strip_prefix(&map_root).unwrap_or(m).display().to_string();
        let map = match s4_formats::map::MapFile::open(m) {
            Ok(x) => x,
            Err(e) => {
                bad_maps += 1;
                println!("BAD   {rel}: {e}");
                continue;
            }
        };
        let script = map.script()?;
        let lower = rel.to_ascii_lowercase();
        let campaign = lower.starts_with("campaign") || lower.starts_with("tutorial");
        let source = resolve_map_script(script.as_ref().map(|s| s.as_bytes()), &rel, campaign);
        match &source {
            ScriptSource::Embedded(_) => *resolved.entry("embedded").or_default() += 1,
            ScriptSource::File(f) if game.join(f).is_file() => *resolved.entry("file (exists)").or_default() += 1,
            ScriptSource::File(f) => {
                *resolved.entry("file (missing = no script)").or_default() += 1;
                if script.is_some() {
                    missing_files.push(format!("{rel} -> {f}"));
                }
            }
            ScriptSource::None => *resolved.entry("none").or_default() += 1,
        }
        let Some(s) = script else {
            no_chunk += 1;
            continue;
        };
        if let Some(dir) = &dump {
            let name = rel.replace(['\\', '/'], "__");
            std::fs::write(dir.join(format!("{name}.lua")), s.as_bytes())?;
        }
        let trimmed = s.trim();
        if trimmed.starts_with('$') && trimmed.ends_with('$') {
            redirects += 1;
            println!("DIR   {rel}: chunk 16 = {trimmed:?} -> {source:?}");
            continue;
        }
        run_map_script(s.as_bytes(), &format!("{rel} [chunk16]"), ticks, &mut mt)?;
    }
    println!(
        "Maps: {bad_maps} unreadable, {no_chunk} without chunk 16, {redirects} `$dir$` redirects; embedded scripts: {} without Lua errors ({} with API warnings), {} failed load/init, {} failed only during ticks",
        mt.ok, mt.diag, mt.init, mt.ticks
    );
    println!("Script a game would run, by source: {resolved:?}");
    for m in &missing_files {
        println!("  embedded script shadowed by a missing campaign file: {m}");
    }

    println!("\nhost calls observed (stub defaults), shipped + embedded:");
    for (k, v) in &mt.calls {
        *t.calls.entry(k).or_default() += v;
    }
    for (k, v) in &t.calls {
        println!("  {v:7}  {k}");
    }
    Ok(())
}
