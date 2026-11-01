//! Savegame chunk 150 (`CAIMain` Save 0x438900 / Load 0x438790) over all savegames:
//! * it decodes and re-encodes byte for byte;
//! * loaded into a world and saved again it is the same chunk;
//! * the player records are the defaults (0x43da20 / 0x43c8c0: level 2, flags 0, the exe's
//!   `CAIConfigIntEx` values) changed only by the `AI.SetPlayerVar` calls of the save's own map
//!   script (chunk 131) as [`s4_sim::ai::vars::PlayerVars::set_player_var`] applies them, and
//!   the level is 1 or 2 (the game start, 0x4ff5f0).
//!
//! Skipped when there are no savegames.

use std::path::{Path, PathBuf};

use s4_formats::map::container::Container;
use s4_sim::ai::chunk;
use s4_sim::ai::vars::PlayerVars;

fn saves() -> Vec<PathBuf> {
    fn walk(d: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(d) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out)
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("sav")) {
                out.push(p)
            }
        }
    }
    let home = std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_default();
    let mut v = Vec::new();
    walk(&home.join(r"Documents\TheSettlers4\Save"), &mut v);
    v.sort();
    v
}

/// The `AI.SetPlayerVar(player, "name", easy[, normal[, hard]])` calls with literal arguments in
/// a script, as the binding reads them (0x43caa0: missing −1, negative −2).
fn set_player_var_calls(script: &str) -> Vec<(i32, String, [i32; 3])> {
    let mut out = Vec::new();
    for line in script.lines() {
        let line = line.split("--").next().unwrap_or("");
        let Some(at) = line.find("AI.SetPlayerVar(") else { continue };
        let args = &line[at + "AI.SetPlayerVar(".len()..];
        let Some(end) = args.find(')') else { continue };
        let parts: Vec<&str> = args[..end].split(',').map(str::trim).collect();
        let num = |i: usize| parts.get(i).and_then(|s| s.parse::<i32>().ok()).map_or(-1, |v| if v < 0 { -2 } else { v });
        let (Ok(p), Some(name)) = (parts[0].parse::<i32>(), parts.get(1)) else { continue };
        out.push((p, name.trim_matches('"').to_string(), [num(2), num(3), num(4)]));
    }
    out
}

#[test]
fn chunk150_all_saves() {
    let list = saves();
    if list.is_empty() {
        return;
    }
    let base = s4_formats::map::MapFile::open(s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map"))
        .ok()
        .and_then(|m| s4_sim::new_game(&m, s4_sim::Rules::load_default().ok()?, &s4_sim::NewGame::default()).ok())
        .map(|(w, _)| w);
    let (mut n, mut scripted, mut ai_players) = (0, 0, 0);
    for p in &list {
        let c = Container::parse(std::fs::read(p).unwrap()).unwrap();
        let (_, bytes) = c.load_id(150, 0).unwrap().expect("chunk 150");
        let ch = chunk::parse(&bytes).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        assert_eq!(chunk::write(&ch), bytes, "{}: re-encoded", p.display());
        n += 1;

        // The expected records.
        let mut want = PlayerVars::default();
        if let Some(h) = c.chunks.iter().find(|h| h.kind == 131)
            && let Ok(b) = c.load(h)
        {
            let calls = set_player_var_calls(&String::from_utf8_lossy(&b));
            scripted += !calls.is_empty() as u32;
            for (player, name, [e, nn, hh]) in calls {
                want.set_player_var(player, &name, e, nn, hh, |_| true);
            }
        }
        for (k, saved) in ch.records.iter().enumerate() {
            let got = chunk::record_of(saved);
            // Level 1 only from the game start's control 2; the player's AI may have been removed
            // since (rom2.sav: player 2 at level 1 without a player AI).
            assert!(got.level == 1 || got.level == 2, "{}: player {k} level {}", p.display(), got.level);
            assert_eq!(got.flags, 0, "{}: player {k}", p.display());
            assert_eq!(got.columns, want.records[k].columns, "{}: player {k}", p.display());
        }
        ai_players += ch.players.len();

        // Load into a world and save again.
        if let Some(mut w) = base.clone() {
            chunk::load(&mut w, &ch).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
            // The schedule a load rebuilds (0x42e5c0 → 0x42e950): every agent once, by next tick.
            for a in w.ai.players.iter().flatten() {
                use s4_sim::ai::sched::Agents;
                let ticks: Vec<u32> = a.sched.queue.iter().map(|&r| a.body.get(r).unwrap().next).collect();
                assert!(ticks.windows(2).all(|t| t[0] <= t[1]), "{}: {ticks:?}", p.display());
                let agents: usize = 1 + a.body.sectors.iter().map(|s| match &s.kind {
                    s4_sim::ai::model::SectorKind::Normal { agents, .. } | s4_sim::ai::model::SectorKind::Dark { agents } => agents.len(),
                }).sum::<usize>();
                assert_eq!(a.sched.queue.len(), agents);
                eprintln!("{}: player {} first agents {:?}", p.display(), a.player, &a.sched.queue[..3.min(agents)]);
            }
            let mut again = chunk::save(&w);
            // (The world's player count is the map's; the chunk keeps the save's.)
            again.player_count = ch.player_count;
            assert_eq!(chunk::write(&again), bytes, "{}: load + save", p.display());
        }
    }
    eprintln!("{n} saves, {scripted} with AI.SetPlayerVar in their script, {ai_players} AI players");
    assert!(scripted >= 2 && ai_players >= 1);
}
