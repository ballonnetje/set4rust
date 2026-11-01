//! Map/campaign scripting: the original's Lua API on a modern Lua.
//!
//! Mirrors `CGameScriptManager` / `CScriptManager` / `CLua` (decompiled debug build,
//! `Script\*.cpp`) and the export tables of the shipped `S4_Main.exe` (`docs/SCRIPT_API.md`).
//!
//! The original runs Lua 3.2.1. We run Lua 5.2 (via `mlua`) with the 3.2 global library rebuilt
//! in `compat32.lua`; see `Cargo.toml` for why 5.2. Game behaviour is delegated to a
//! [`ScriptHost`], which has one method per exported C function.
//!
//! Life cycle (as `CGameScriptManager`):
//! 1. [`ScriptEngine::new`]: create the state, export `request_event(s)`, `unrequest_event(s)`,
//!    `_ERRORMESSAGE`, `reg_func`, the `Vars` table, all constants and all API tables, plus the
//!    `AI` table that `CAIScripting::Init` adds to the same state.
//! 2. [`ScriptEngine::load`] (`NewGame`/`LoadMapScript`): keep the chunk and execute it once.
//! 3. [`ScriptEngine::start`] (`StartScript`): execute the chunk **again**, call
//!    `register_functions` (the only time `reg_func` is allowed), then `new_game` for a new game.
//! 4. [`ScriptEngine::update`] every logic tick (`Update`): events 1+2 (new game) or 2 (loaded) on
//!    the first tick, `TICK` every tick, `FIVE_TICKS` every 5th, the victory check every 8th
//!    after tick 140.
//!
//! Script errors never abort the game: like the original's `_ERRORMESSAGE` handler they are
//! reported through [`ScriptHost::trace`] and collected in [`ScriptEngine::errors`]. The
//! original's warnings about API misuse (`reg_func`/`request_event` with bad arguments) go to
//! [`ScriptHost::trace`] and [`ScriptEngine::diagnostics`]; they are not Lua errors and the
//! script carries on.

mod generated;
mod host;

pub use generated::{AI_CONSTANTS, AI_PLAYER_VARS, CONSTANTS, FUNCS, Ret};
pub use host::ScriptHost;

/// The script text keys, index = string id: the names in the player's exe.
pub static TEXT_KEYS: std::sync::LazyLock<Vec<String>> = std::sync::LazyLock::new(|| s4_formats::text::installed_string_names().unwrap_or_default());

/// The string id of a script text key (`Tutorial.ShowText`, `Game.ShowText`): 0x5bfcd0 compares
/// the key with each of the exe's 0xefe names in order (case-sensitive) and returns that entry's
/// id, which is its index; no key (or an empty one) gives 0xefe, i.e. `None`.
pub fn text_key_id(key: &str) -> Option<u16> {
    if key.is_empty() {
        return None;
    }
    TEXT_KEYS.iter().position(|k| k == key).map(|i| i as u16)
}

use mlua::{Function, Lua, MultiValue, RegistryKey, Table, Value};
use std::cell::RefCell;
use std::rc::Rc;

/// `SCRIPT_EVENTS_MAX`: event ids 1..=22 are valid (0 is rejected by `request_event`).
pub const SCRIPT_EVENTS_MAX: usize = 23;
/// `CScriptManager::REGISTERED_FUNCTION_MAX`.
pub const REGISTERED_FUNCTION_MAX: usize = 64;

/// `Events.*` values exported to scripts.
pub mod events {
    pub const FIRST_TICK_OF_NEW_GAME: u32 = 1;
    pub const FIRST_TICK_OF_NEW_OR_LOADED_GAME: u32 = 2;
    pub const VICTORY_CONDITION_CHECK: u32 = 3;
    pub const TICK: u32 = 4;
    pub const FIVE_TICKS: u32 = 5;
    pub const SPACE: u32 = 7;
    pub const DRAG_BUILDING: u32 = 8;
    pub const WARRIOR_SENT: u32 = 9;
    pub const CRUSH_BUILDING: u32 = 10;
    pub const BUILD_PRIO: u32 = 11;
    pub const WORK_STATUS: u32 = 12;
    pub const WORK_AREA: u32 = 13;
    pub const SHOW_WORK_AREA: u32 = 14;
    pub const ZOOM_FACTOR_CHANGED: u32 = 15;
    pub const COMMAND: u32 = 16;
    pub const GOODARRIVE: u32 = 17;
    pub const SETTLER_CHANGE_TYPE: u32 = 18;
    pub const MENUCLICK: u32 = 19;
    pub const CREATE_FOUNDATION_CART: u32 = 20;
    pub const PRODUCTION: u32 = 21;
    pub const MAGIC_SPELL_CAST: u32 = 22;
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A Lua-side failure, kept as text: `mlua::Error` holds a non-`Send` `dyn Error`, which
    /// would stop callers from boxing ours into `anyhow::Error`.
    #[error("lua: {0}")]
    Lua(String),
    #[error("{0}")]
    State(&'static str),
}

impl From<mlua::Error> for Error {
    fn from(e: mlua::Error) -> Self {
        Error::Lua(e.to_string())
    }
}
pub type Result<T> = std::result::Result<T, Error>;

/// An argument as the host saw it (for logging / [`ScriptHost::unhandled`]).
#[derive(Clone, Debug, PartialEq)]
pub enum Arg {
    Int(i32),
    Str(String),
    Ints(Vec<i32>),
}

impl std::fmt::Display for Arg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Arg::Int(v) => write!(f, "{v}"),
            Arg::Str(s) => write!(f, "{s:?}"),
            Arg::Ints(v) => write!(f, "{v:?}"),
        }
    }
}

/// Arguments of one Lua → host call, converted like Lua 3.2's `lua_getnumber` / `lua_getstring`.
pub struct Args(Vec<Value>);

impl Args {
    fn get(&self, k: usize) -> Option<&Value> {
        self.0.get(k.checked_sub(1)?)
    }
    /// Parameter `k` as [`Args::int`] when at least `k` arguments were passed (an explicit nil
    /// counts and reads 0), else `missing`: the exe's argument-count guarded reads.
    pub fn int_or(&self, k: usize, missing: i32) -> i32 {
        if k > self.0.len() { missing } else { self.int(k) }
    }
    /// Parameter `k` when at least `k` arguments were passed (an explicit nil counts), else None.
    pub fn int_opt(&self, k: usize) -> Option<i32> {
        (k <= self.0.len()).then(|| self.int(k))
    }
    /// Parameter `k` (1-based) as the original's `(int)lua_getnumber(lua_getparam(k))`:
    /// numbers truncate toward zero, numeric strings convert, everything else (nil, missing,
    /// tables) is 0. Booleans (which 3.2 did not have; 5.1 comparisons produce them) read as 1/0.
    pub fn int(&self, k: usize) -> i32 {
        match self.get(k) {
            Some(Value::Integer(i)) => cvttsd2si(*i as f64),
            Some(Value::Number(n)) => cvttsd2si(*n),
            Some(Value::String(s)) => {
                s.to_str().ok().and_then(|s| parse_lua_number(&s)).map_or(0, cvttsd2si)
            }
            Some(Value::Boolean(b)) => *b as i32,
            _ => 0,
        }
    }
    /// Parameter `k` as `lua_getstring`: strings as-is, numbers formatted like Lua, anything else
    /// "" (the original would pass NULL). The shipped scripts are Windows-1252, so bytes that are
    /// not valid UTF-8 are decoded as 1252.
    pub fn str(&self, k: usize) -> String {
        match self.get(k) {
            Some(Value::String(s)) => decode_script_text(&s.as_bytes()),
            Some(Value::Integer(i)) => i.to_string(),
            Some(Value::Number(n)) => fmt_lua_number(*n),
            _ => String::new(),
        }
    }
    /// Parameter `k` as `AI.SetPlayerVar` reads its level values (release 0x43caa0): missing or
    /// not convertible to a number -1, negative -2, otherwise the truncated value.
    pub fn ai_level(&self, k: usize) -> i32 {
        let n = match self.get(k) {
            Some(Value::Integer(i)) => Some(*i as f64),
            Some(Value::Number(n)) => Some(*n),
            Some(Value::String(s)) => s.to_str().ok().and_then(|s| parse_lua_number(&s)),
            _ => None,
        };
        match n {
            None => -1,
            Some(n) if cvttsd2si(n) < 0 => -2,
            Some(n) => cvttsd2si(n),
        }
    }
    /// Parameters `k..` as ints (variadic functions such as `Tutorial.DisableExcept`).
    pub fn ints_from(&self, k: usize) -> Vec<i32> {
        (k..=self.0.len()).map(|i| self.int(i)).collect()
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// `(int)double` as the shipped exe does it (SSE2 `cvttsd2si`): truncation toward zero, and
/// i32::MIN for NaN or anything outside the i32 range (Rust's `as` would saturate instead).
pub fn cvttsd2si(n: f64) -> i32 {
    if n > -2_147_483_649.0 && n < 2_147_483_648.0 { n as i32 } else { i32::MIN }
}

/// Script text as Rust text: UTF-8 if it is valid UTF-8, otherwise Windows-1252 (the encoding of
/// every shipped `Script\*.txt`).
pub fn decode_script_text(b: &[u8]) -> String {
    match std::str::from_utf8(b) {
        Ok(s) => s.to_owned(),
        Err(_) => b.iter().map(|&c| cp1252(c)).collect(),
    }
}

fn cp1252(c: u8) -> char {
    const HI: [u16; 32] = [
        0x20AC, 0x81, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039,
        0x0152, 0x8D, 0x017D, 0x8F, 0x90, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014,
        0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x9D, 0x017E, 0x0178,
    ];
    match c {
        0x80..=0x9F => char::from_u32(HI[(c - 0x80) as usize] as u32).unwrap_or('?'),
        _ => c as char,
    }
}

/// Where a game's script comes from (`CGameScriptManager::NewGameEx`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScriptSource {
    /// The map's own chunk-16 script.
    Embedded(Vec<u8>),
    /// A file relative to the install directory, e.g. `Script\AO_maya1.txt`. A missing file means
    /// no script (the original opens it unconditionally and gets 0 bytes; inferred, not traced).
    File(String),
    /// No script at all.
    None,
}

/// `CGameScriptManager::NewGameEx(src, size, map_name, prefer_file)`.
///
/// * `chunk16`: the map's script chunk (`None` if absent or shorter than 4 bytes, as
///   `CGameRun::LoadEditorMap` drops those).
/// * `map_name`: the map path or file name; only the file name is used.
/// * `prefer_file`: `CGameData::IsCampaign() || IsTutorial()` (computed in `CGameRun` before
///   `LoadEditorMap`).
///
/// A chunk that, trimmed of bytes <= ' ', reads `$dir\$` (under 64 bytes between the `$`s, ending
/// in a backslash) is not a script but a subdirectory of `Script\` to look in; any other `$...$`
/// chunk is discarded. Then: no preference and an embedded script -> embedded; otherwise
/// `Script\[dir]<map name>.txt` if the map file ends in `.map`/`.edm`; otherwise the embedded
/// script if any, else none.
pub fn resolve_map_script(chunk16: Option<&[u8]>, map_name: &str, prefer_file: bool) -> ScriptSource {
    let mut dir = String::new();
    let mut src = chunk16.filter(|s| s.len() >= 2 && s[0] != 0);
    if let Some(s) = src {
        let end = s.iter().rposition(|&c| c > b' ').map_or(0, |i| i + 1);
        let start = s[..end].iter().position(|&c| c > b' ').unwrap_or(end);
        let t = &s[start..end];
        if t.len() > 2 && t[0] == b'$' && t[t.len() - 1] == b'$' {
            let inner = &t[1..t.len() - 1];
            if inner.len() < 64 && inner.last() == Some(&b'\\') {
                dir = inner.iter().map(|&c| c as char).collect();
            }
            src = None;
        }
    }
    if let (false, Some(s)) = (prefer_file, src) {
        return ScriptSource::Embedded(s.to_vec());
    }
    let file = map_name.rsplit(['\\', '/']).next().unwrap_or("");
    let lower = file.to_ascii_lowercase();
    if file.len() > 4 && (lower.ends_with(".map") || lower.ends_with(".edm")) {
        return ScriptSource::File(format!("Script\\{dir}{}.txt", &file[..file.len() - 4]));
    }
    match src {
        Some(s) => ScriptSource::Embedded(s.to_vec()),
        None => ScriptSource::None,
    }
}

fn parse_lua_number(s: &str) -> Option<f64> {
    let t = s.trim();
    if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        return i64::from_str_radix(h, 16).ok().map(|v| v as f64);
    }
    t.parse().ok()
}

fn fmt_lua_number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 { format!("{}", n as i64) } else { format!("{n}") }
}

/// `CScriptEventRequests`: up to 64 registered-function slots, 255 = free.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EventRequests {
    pub slots: Vec<u8>,
}

impl EventRequests {
    fn add(&mut self, f: u8) {
        if self.slots.contains(&f) {
            return;
        }
        match self.slots.iter().position(|&s| s == 255) {
            Some(i) => self.slots[i] = f,
            None if self.slots.len() < REGISTERED_FUNCTION_MAX => self.slots.push(f),
            None => {}
        }
    }
    fn remove(&mut self, f: u8) {
        if let Some(i) = self.slots.iter().position(|&s| s == f) {
            self.slots[i] = 255;
            // the original trims trailing free slots but never below size 1
            let mut n = self.slots.len() - 1;
            while n > 0 && self.slots[n] == 255 {
                n -= 1;
            }
            self.slots.truncate(n + 1);
        }
    }
}

/// The script part of a savegame (`CGameScriptManager::Save`): the map script source (name kept
/// for error messages), `Vars.Save1..9`, and the 23 event request lists. Nothing else survives.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SavedState {
    pub source: Option<(Vec<u8>, String)>,
    pub vars: [f64; 9],
    pub requests: Vec<EventRequests>,
}

#[derive(Default)]
struct State {
    registered: Vec<RegistryKey>,
    allow_register: bool,
    requests: [EventRequests; SCRIPT_EVENTS_MAX],
    errors: Vec<String>,
    diagnostics: Vec<String>,
    /// Instructions run since the current entry from Rust, in units of [`HOOK_STEP`].
    steps: u64,
    /// `None` = unlimited (the original has no limit: a runaway loop freezes the game).
    step_limit: Option<u64>,
}

/// Granularity of the instruction budget (see [`ScriptEngine::set_instruction_limit`]).
const HOOK_STEP: u32 = 10_000;

/// Script state for one game. `H` is the simulation side.
pub struct ScriptEngine<H: ScriptHost + 'static> {
    lua: Lua,
    host: Rc<RefCell<H>>,
    state: Rc<RefCell<State>>,
    chunk: Option<(Vec<u8>, String)>,
    /// 0 none, 1 new game loaded, 2 saved game loaded, 3 running (`m_uGameScriptState`).
    script_state: u8,
    start_of_game: u8,
}

/// A Lua error (compile or runtime): traced and kept in `errors`.
fn trace_host<H: ScriptHost>(host: &Rc<RefCell<H>>, state: &Rc<RefCell<State>>, msg: String) {
    host.borrow_mut().trace(&msg);
    state.borrow_mut().errors.push(msg);
}

/// An API-misuse warning the original prints with `ScriptTracePrintF`: traced and kept in
/// `diagnostics`.
fn trace_diag<H: ScriptHost>(host: &Rc<RefCell<H>>, state: &Rc<RefCell<State>>, msg: String) {
    host.borrow_mut().trace(&msg);
    state.borrow_mut().diagnostics.push(msg);
}

impl<H: ScriptHost + 'static> ScriptEngine<H> {
    /// `CGameScriptManager::InitScriptEnvironmentAndManager`.
    pub fn new(host: H) -> Result<Self> {
        let lua = Lua::new();
        let host = Rc::new(RefCell::new(host));
        let state = Rc::new(RefCell::new(State::default()));
        let g = lua.globals();

        let h = host.clone();
        g.set("__host_trace", lua.create_function(move |_, s: mlua::String| {
            h.borrow_mut().trace(&String::from_utf8_lossy(&s.as_bytes()));
            Ok(())
        })?)?;
        lua.load(include_str!("compat32.lua")).set_name("compat32").exec()?;

        // reg_func (CScriptManager::LuaRegisterFunction)
        let (h, st) = (host.clone(), state.clone());
        g.set("reg_func", lua.create_function(move |lua, v: Value| {
            let Value::Function(f) = v else {
                trace_diag(&h, &st, "SCRIPT: reg_func: parameter not a function".into());
                return Ok(());
            };
            let msg = {
                let mut s = st.borrow_mut();
                if !s.allow_register {
                    Some("SCRIPT: reg_func must only be used in register_functions")
                } else if find_func(lua, &s, &f).is_some() {
                    Some("SCRIPT: reg_func: function already registered")
                } else if s.registered.len() >= REGISTERED_FUNCTION_MAX {
                    Some("SCRIPT: reg_func: too many registered functions")
                } else {
                    s.registered.push(lua.create_registry_value(f)?);
                    None
                }
            };
            if let Some(m) = msg {
                trace_diag(&h, &st, m.into());
            }
            Ok(())
        })?)?;

        for (name, add, multi) in [
            ("request_event", true, false),
            ("request_events", true, true),
            ("unrequest_event", false, false),
            ("unrequest_events", false, true),
        ] {
            let (h, st) = (host.clone(), state.clone());
            g.set(name, lua.create_function(move |lua, args: MultiValue| {
                let args: Vec<Value> = args.into_iter().collect();
                let func = match args.first() {
                    Some(Value::Function(f)) => find_func(lua, &st.borrow(), f),
                    _ => None,
                };
                let Some(fi) = func else {
                    trace_diag(&h, &st, format!("SCRIPT: {name}: Parameter 1 is not a registered function!"));
                    return Ok(());
                };
                let a = Args(args);
                // request_events loops while CheckParam(i); request_event reads exactly arg 2
                let last = if multi { a.len() } else { 2 };
                for i in 2..=last {
                    let ev = a.int(i);
                    if ev <= 0 || ev as usize >= SCRIPT_EVENTS_MAX {
                        trace_diag(&h, &st, format!("SCRIPT: {name}: Invalid event type {ev}!"));
                        continue;
                    }
                    let r = &mut st.borrow_mut().requests[ev as usize];
                    if add { r.add(fi as u8) } else { r.remove(fi as u8) }
                }
                Ok(())
            })?)?;
        }

        let (h, st) = (host.clone(), state.clone());
        g.set("_ERRORMESSAGE", lua.create_function(move |_, s: Option<mlua::String>| {
            let s = s.map(|s| String::from_utf8_lossy(&s.as_bytes()).into_owned()).unwrap_or_default();
            trace_host(&h, &st, format!("SCRIPT: Error: {s}"));
            Ok(())
        })?)?;

        // `CreateTable("Vars")`: Save1..Save9 are what a savegame keeps.
        g.set("Vars", lua.create_table()?)?;

        // ScriptExportVars, then ScriptExportFunctions (same order as the original).
        for &(table, key, value) in CONSTANTS {
            table_of(&lua, table)?.set(key, value)?;
        }
        // CAIScripting::Init (AI\AI_Script.cpp): the `AI` table, constants first, then its functions
        // (which are in FUNCS under "AI").
        for &(key, value) in AI_CONSTANTS {
            table_of(&lua, "AI")?.set(key, value)?;
        }
        for &(table, name, method) in FUNCS {
            let h = host.clone();
            let f = lua.create_function(move |_, args: MultiValue| {
                let a = Args(args.into_iter().collect());
                let r = generated::dispatch(&mut *h.borrow_mut(), method, &a);
                Ok(match r {
                    Ret::None => MultiValue::new(),
                    Ret::One(v) => MultiValue::from_iter([Value::Number(v as f64)]),
                    Ret::Six(v) => MultiValue::from_iter(v.map(|x| Value::Number(x as f64))),
                })
            })?;
            table_of(&lua, table)?.set(name, f)?;
        }
        let st = state.clone();
        lua.set_hook(mlua::HookTriggers::new().every_nth_instruction(HOOK_STEP), move |_, _| {
            let mut s = st.borrow_mut();
            s.steps += 1;
            match s.step_limit {
                Some(limit) if s.steps > limit => {
                    Err(mlua::Error::runtime("SCRIPT: instruction limit exceeded (runaway loop?)"))
                }
                _ => Ok(mlua::VmState::Continue),
            }
        });
        drop(g);
        Ok(Self { lua, host, state, chunk: None, script_state: 0, start_of_game: 0 })
    }

    pub fn host(&self) -> std::cell::Ref<'_, H> {
        self.host.borrow()
    }
    pub fn host_mut(&self) -> std::cell::RefMut<'_, H> {
        self.host.borrow_mut()
    }
    pub fn lua(&self) -> &Lua {
        &self.lua
    }
    /// Every Lua error (compile/runtime) traced so far.
    pub fn errors(&self) -> Vec<String> {
        self.state.borrow().errors.clone()
    }
    /// Every API-misuse warning traced so far (not Lua errors; see the crate docs).
    pub fn diagnostics(&self) -> Vec<String> {
        self.state.borrow().diagnostics.clone()
    }
    pub fn requests(&self, event: u32) -> EventRequests {
        self.state.borrow().requests.get(event as usize).cloned().unwrap_or_default()
    }
    pub fn registered_count(&self) -> usize {
        self.state.borrow().registered.len()
    }

    /// Abort any single entry into Lua (chunk execution, `register_functions`/`new_game`, one event
    /// handler) after roughly `limit` VM instructions, reporting it like a script error. Not in the
    /// original, where such a loop hangs the game; `None` (the default) keeps that behaviour.
    pub fn set_instruction_limit(&mut self, limit: Option<u64>) {
        self.state.borrow_mut().step_limit = limit.map(|l| l.div_ceil(HOOK_STEP as u64).max(1));
    }

    /// Start a new instruction budget (each entry from Rust into Lua gets its own).
    fn enter(&self) {
        self.state.borrow_mut().steps = 0;
    }

    /// The in-game cheat console (`EventManagement` 0x26 → 0x515420 → 0x67f2a0): a typed line
    /// run as a chunk in the game script's state. Returns false on an error (reported as the
    /// chunk's).
    pub fn console(&self, code: &[u8]) -> bool {
        self.exec(code, "console")
    }

    fn exec(&self, chunk: &[u8], name: &str) -> bool {
        self.enter();
        match self.lua.load(chunk).set_name(name).exec() {
            Ok(()) => true,
            Err(e) => {
                trace_host(&self.host, &self.state, format!("SCRIPT: Error: {e}"));
                false
            }
        }
    }

    /// New game, `NewGame(Src, Size)` -> `LoadMapScript`: keep the map script, execute it once,
    /// drop all event requests. Returns false if the chunk failed to compile or run (as the
    /// original's `ExecuteScript` result, which `NewGame` ignores).
    pub fn load(&mut self, chunk: &[u8], name: &str) -> Result<bool> {
        if self.script_state != 0 {
            return Err(Error::State("script already loaded"));
        }
        self.chunk = Some((chunk.to_vec(), name.to_string()));
        let ok = self.exec(chunk, name);
        self.state.borrow_mut().requests = Default::default();
        self.script_state = 1;
        Ok(ok)
    }

    /// A new game without a map script (`NewGame()` with no source).
    pub fn load_none(&mut self) {
        self.chunk = None;
        self.script_state = 1;
    }

    /// Saved game, `CGameScriptManager::Load` + `LoadState`: restore the script source,
    /// `Vars.Save1..9` and the event requests. The chunk is **not** executed here; [`start`] runs
    /// it, calls `register_functions` (registration order must match the save, as in the original,
    /// since requests store function indices) and skips `new_game`. Every other global the script
    /// had is gone, exactly as in the original.
    ///
    /// [`start`]: ScriptEngine::start
    pub fn load_saved(&mut self, saved: &SavedState) -> Result<()> {
        if self.script_state != 0 {
            return Err(Error::State("script already loaded"));
        }
        self.chunk = saved.source.clone();
        self.set_save_vars(&saved.vars)?;
        let mut s = self.state.borrow_mut();
        s.requests = Default::default();
        for (r, l) in s.requests.iter_mut().zip(&saved.requests) {
            *r = l.clone();
        }
        drop(s);
        self.script_state = 2;
        Ok(())
    }

    /// `CGameScriptManager::Save` + `SaveState`: what a savegame keeps of the script.
    pub fn save_state(&self) -> Result<SavedState> {
        Ok(SavedState {
            source: self.chunk.clone(),
            vars: self.save_vars()?,
            requests: self.state.borrow().requests.to_vec(),
        })
    }

    /// `StartScript`: execute the chunk again, `register_functions`, then `new_game` (new games only).
    pub fn start(&mut self) -> Result<()> {
        if !matches!(self.script_state, 1 | 2) {
            return Err(Error::State("start() needs a loaded script"));
        }
        if let Some((chunk, name)) = self.chunk.clone() {
            self.exec(&chunk, &name);
            self.call_global("register_functions", true);
            if self.script_state == 1 {
                self.call_global("new_game", false);
            }
        }
        self.start_of_game = self.script_state;
        self.script_state = 3;
        Ok(())
    }

    /// `CallRegisterFunctions` / `CallNewGame`: missing functions are a warning, not an error.
    fn call_global(&mut self, name: &str, register: bool) {
        let f: Value = self.lua.globals().get(name).unwrap_or(Value::Nil);
        let Value::Function(f) = f else {
            self.host.borrow_mut().trace(&format!("SCRIPT: Warning: {name} not found."));
            return;
        };
        if register {
            self.state.borrow_mut().allow_register = true;
        }
        self.enter();
        if let Err(e) = f.call::<()>(()) {
            trace_host(&self.host, &self.state, format!("SCRIPT: Error in {name}: {e}"));
        }
        if register {
            self.state.borrow_mut().allow_register = false;
        }
    }

    /// `SendGameEventCall`: call every function requested for `event` with up to 4 int args.
    pub fn send_event(&mut self, event: u32, args: &[i32]) -> Result<()> {
        if self.script_state != 3 {
            return Err(Error::State("send_event() before start()"));
        }
        let ev = event as usize;
        if ev >= SCRIPT_EVENTS_MAX {
            return Err(Error::State("invalid event"));
        }
        let size = self.state.borrow().requests[ev].slots.len();
        for i in 0..size {
            let (fi, func) = {
                let s = self.state.borrow();
                let Some(&fi) = s.requests[ev].slots.get(i) else { break };
                if fi == 255 {
                    continue;
                }
                let Some(key) = s.registered.get(fi as usize) else { continue };
                (fi, self.lua.registry_value::<Function>(key)?)
            };
            let argv = MultiValue::from_iter(args.iter().map(|&a| Value::Number(a as f64)));
            self.enter();
            if let Err(e) = func.call::<()>(argv) {
                trace_host(&self.host, &self.state, format!("SCRIPT: Error in event {event} handler #{fi}: {e}"));
            }
        }
        Ok(())
    }

    /// `CGameScriptManager::Update(tick)`, once per logic tick.
    pub fn update(&mut self, tick: u32) -> Result<()> {
        match self.start_of_game {
            1 => {
                self.send_event(events::FIRST_TICK_OF_NEW_GAME, &[])?;
                self.send_event(events::FIRST_TICK_OF_NEW_OR_LOADED_GAME, &[])?;
            }
            2 => self.send_event(events::FIRST_TICK_OF_NEW_OR_LOADED_GAME, &[])?,
            _ => {}
        }
        self.start_of_game = 3;
        self.send_event(events::TICK, &[])?;
        if tick.is_multiple_of(5) {
            self.send_event(events::FIVE_TICKS, &[])?;
        }
        if tick & 7 == 0 && tick > 140 && !self.host.borrow_mut().is_game_won() {
            let hooked = self.host.borrow_mut().victory_hook();
            if !hooked {
                let requested = !self.requests(events::VICTORY_CONDITION_CHECK).slots.is_empty();
                if requested {
                    self.send_event(events::VICTORY_CONDITION_CHECK, &[])?;
                } else {
                    self.host.borrow_mut().default_victory_check();
                }
            }
        }
        Ok(())
    }

    /// `CGameScriptManager::SetGlobal(name, int)`: the tutorial UI sets `gActiveMenu` this way.
    pub fn set_global(&self, name: &str, value: i32) -> Result<()> {
        self.lua.globals().set(name, value as f64)?;
        Ok(())
    }

    /// `GetSaveVarValues`: `Vars.Save1..Save9` (non-numbers read as 0).
    pub fn save_vars(&self) -> Result<[f64; 9]> {
        let vars: Table = self.lua.globals().get("Vars")?;
        let mut out = [0.0; 9];
        for (i, o) in out.iter_mut().enumerate() {
            *o = match vars.raw_get::<Value>(format!("Save{}", i + 1))? {
                Value::Number(n) => n,
                Value::Integer(n) => n as f64,
                _ => 0.0,
            };
        }
        Ok(out)
    }

    /// `SetSaveVarValues`.
    pub fn set_save_vars(&self, v: &[f64; 9]) -> Result<()> {
        let vars: Table = self.lua.globals().get("Vars")?;
        for (i, x) in v.iter().enumerate() {
            vars.raw_set(format!("Save{}", i + 1), *x)?;
        }
        Ok(())
    }

    /// `CreateStartResources`: run `Script\Internal\StartResources.txt` and call
    /// `CreateStartResourcesFew/Medium/Many(x, y, a5, a6)` for amount 1/2/3.
    pub fn create_start_resources(&mut self, script: &[u8], amount: u32, x: i32, y: i32, a5: i32, a6: i32) -> Result<()> {
        self.exec(script, "StartResources.txt");
        let name = match amount {
            1 => "CreateStartResourcesFew",
            2 => "CreateStartResourcesMedium",
            3 => "CreateStartResourcesMany",
            _ => return Err(Error::State("wrong amount of start resources")),
        };
        self.enter();
        if let Value::Function(f) = self.lua.globals().get::<Value>(name)?
            && let Err(e) = f.call::<()>((x, y, a5, a6))
        {
            trace_host(&self.host, &self.state, format!("SCRIPT: Error in {name}: {e}"));
        }
        Ok(())
    }
}

fn table_of(lua: &Lua, name: &str) -> mlua::Result<Table> {
    let g = lua.globals();
    match g.raw_get::<Value>(name)? {
        Value::Table(t) => Ok(t),
        _ => {
            let t = lua.create_table()?;
            g.raw_set(name, t.clone())?;
            Ok(t)
        }
    }
}

/// `CScriptManager::FindFunc`: index of a registered function, by identity.
fn find_func(lua: &Lua, s: &State, f: &Function) -> Option<usize> {
    s.registered.iter().position(|k| {
        lua.registry_value::<Function>(k).map(|g| g.to_pointer() == f.to_pointer()).unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Log(Vec<String>);
    impl ScriptHost for Log {
        fn unhandled(&mut self, func: &'static str, args: &[Arg]) {
            self.0.push(format!("{func}({})", args.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(",")));
        }
        fn game_time(&mut self) -> i32 {
            7
        }
        fn game_set_config_var(&mut self, _section: &str, _key: &str, _value: i32, a4: Option<i32>) {
            self.0.push(format!("SetConfigVar a4={a4:?}"));
        }
    }

    const SCRIPT: &str = r#"
        function tick() Vars.Save1 = Vars.Save1 + 1 end
        function first() Settlers.AddSettlers(1, 2.9, nil, Settlers.CARRIER, "3") end
        function register_functions() reg_func(tick) reg_func(first) end
        function new_game()
            Vars.Save1 = Game.Time()
            request_event(tick, Events.TICK)
            request_events(first, Events.FIRST_TICK_OF_NEW_GAME, 99)
            local t = {} tinsert(t, 5) tinsert(t, 1, 4)
            Vars.Save2 = getn(t) * 10 + t[1] + strlen(strsub("abcdef", 2, 3))
        end
    "#;

    #[test]
    fn lifecycle() {
        let mut e = ScriptEngine::new(Log::default()).unwrap();
        assert!(e.load(SCRIPT.as_bytes(), "t").unwrap());
        e.start().unwrap();
        e.update(1).unwrap();
        e.update(2).unwrap();
        let v = e.save_vars().unwrap();
        assert_eq!(v[0], 9.0); // 7 + two ticks
        assert_eq!(v[1], 20.0 + 4.0 + 2.0);
        assert_eq!(e.host().0, vec!["Settlers.AddSettlers(1,2,0,1,3)".to_string()]);
        assert_eq!(e.requests(events::TICK).slots, vec![0]);
        assert!(e.diagnostics().iter().any(|m| m.contains("Invalid event type 99")));
    }

    /// The exe reads `ExistsBuildingInArea`'s status only when 6 arguments were passed (else 1,
    /// READY) and `SetConfigVar`'s arg 4 only when 4 were (else "not given"); an explicit nil is
    /// an argument and reads as 0. roman02's victory check passes 5 arguments.
    #[test]
    fn argument_count_guarded_defaults() {
        let mut e = ScriptEngine::new(Log::default()).unwrap();
        e.load(
            br#"function new_game()
                Buildings.ExistsBuildingInArea(2, Buildings.SMALLTEMPLE, 397, 368, 4)
                Buildings.ExistsNearPt(2, 1, 3, 4, 5, nil)
                Buildings.ExistsBuildingInArea(2, 1, 3, 4, 5, Buildings.ALL)
                Game.SetConfigVar("s", "k", 3)
                Game.SetConfigVar("s", "k", 3, nil)
            end"#,
            "t",
        )
        .unwrap();
        e.start().unwrap();
        assert_eq!(e.host().0, vec![
            "Buildings.ExistsBuildingInArea(2,43,397,368,4,1)".to_string(),
            "Buildings.ExistsBuildingInArea(2,1,3,4,5,0)".to_string(),
            "Buildings.ExistsBuildingInArea(2,1,3,4,5,2)".to_string(),
            "SetConfigVar a4=None".to_string(),
            "SetConfigVar a4=Some(0)".to_string(),
        ]);
    }

    #[test]
    fn save_and_load() {
        let mut e = ScriptEngine::new(Log::default()).unwrap();
        e.load(SCRIPT.as_bytes(), "t").unwrap();
        e.start().unwrap();
        e.update(1).unwrap();
        let saved = e.save_state().unwrap();
        assert_eq!(saved.vars[0], 8.0);

        let mut f = ScriptEngine::new(Log::default()).unwrap();
        f.load_saved(&saved).unwrap();
        f.start().unwrap(); // no new_game: Save1 keeps its saved value
        f.update(2).unwrap(); // event 2 only (nobody requested it), then TICK
        assert_eq!(f.save_vars().unwrap()[0], 9.0);
        assert!(f.host().0.is_empty(), "FIRST_TICK_OF_NEW_GAME must not fire: {:?}", f.host().0);
    }

    #[test]
    fn reg_func_outside_register_functions_is_refused() {
        let mut e = ScriptEngine::new(Log::default()).unwrap();
        e.load(b"function new_game() reg_func(new_game) end", "t").unwrap();
        e.start().unwrap();
        assert_eq!(e.registered_count(), 0);
        assert!(e.diagnostics().iter().any(|m| m.contains("register_functions")));
    }

    #[test]
    fn ai_set_player_var_levels() {
        let mut e = ScriptEngine::new(Log::default()).unwrap();
        e.load(b"function new_game() AI.SetPlayerVar(2, 'AttackMode', 1, -1) AI.SetPlayerVar(0, 'X', '7', {}, 2.9) end", "t").unwrap();
        e.start().unwrap();
        assert_eq!(e.host().0, vec![
            r#"AI.SetPlayerVar(2,"AttackMode",1,-2,-1)"#.to_string(),
            r#"AI.SetPlayerVar(0,"X",7,-1,2)"#.to_string(),
        ]);
    }

    #[test]
    fn instruction_limit() {
        let mut e = ScriptEngine::new(Log::default()).unwrap();
        e.set_instruction_limit(Some(100_000));
        e.load(b"function new_game() while 1 do end end", "t").unwrap();
        e.start().unwrap();
        assert!(e.errors().iter().any(|m| m.contains("instruction limit")));
    }

    #[test]
    fn reg_func_non_function() {
        let mut e = ScriptEngine::new(Log::default()).unwrap();
        e.load(b"function register_functions() reg_func(3) end", "t").unwrap();
        e.start().unwrap();
        assert!(e.diagnostics().iter().any(|m| m.contains("not a function")));
    }

    /// Lua 3.2 (and 5.2) read a `(` at the start of a line as the call's argument list; 5.1 refuses
    /// it ("ambiguous syntax"). Eleven tutorial scripts are written like this.
    #[test]
    fn call_args_on_next_line() {
        let mut e = ScriptEngine::new(Log::default()).unwrap();
        e.load(b"function new_game() Tutorial.DisableExcept
(
1, 2
) end", "t").unwrap();
        e.start().unwrap();
        assert!(e.errors().is_empty(), "{:?}", e.errors());
        assert_eq!(e.host().0, vec!["Tutorial.DisableExcept([1, 2])".to_string()]);
    }

    #[test]
    fn int_conversion_like_cvttsd2si() {
        assert_eq!(cvttsd2si(2.9), 2);
        assert_eq!(cvttsd2si(-2.9), -2);
        assert_eq!(cvttsd2si(f64::NAN), i32::MIN);
        assert_eq!(cvttsd2si(f64::INFINITY), i32::MIN);
        assert_eq!(cvttsd2si(3e9), i32::MIN);
        assert_eq!(cvttsd2si(-2147483648.5), i32::MIN); // in range: truncates to MIN itself
    }

    #[test]
    fn script_resolution() {
        use ScriptSource::{Embedded, File};
        let r = resolve_map_script;
        assert_eq!(r(Some(b"x=1"), r"Map\User\a.map", false), Embedded(b"x=1".to_vec()));
        assert_eq!(r(Some(b"x=1"), r"Map\Campaign\AO_maya1.map", true), File(r"Script\AO_maya1.txt".into()));
        assert_eq!(r(Some(b" $MissionCD\\$\n"), "m.map", false), File(r"Script\MissionCD\m.txt".into()));
        assert_eq!(r(Some(b"$nodir$"), "m.map", false), File(r"Script\m.txt".into()));
        assert_eq!(r(None, "m.map", false), File(r"Script\m.txt".into()));
        assert_eq!(r(None, "m", false), ScriptSource::None);
        assert_eq!(decode_script_text(b"M\xe4dchen \x80"), "M\u{e4}dchen \u{20ac}");
    }

    #[test]
    fn requests_slots() {
        let mut r = EventRequests::default();
        r.add(3);
        r.add(5);
        r.add(3);
        r.remove(3);
        assert_eq!(r.slots, vec![255, 5]);
        r.add(7);
        assert_eq!(r.slots, vec![7, 5]);
        r.remove(5);
        r.remove(7);
        assert_eq!(r.slots, vec![255]);
    }
}
