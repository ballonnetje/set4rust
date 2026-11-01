//! Lookout towers (`CLookoutTowerRole`, house kind 9) on AO_Viking3, driven by `tick()` only:
//! every tower gets its inhabitant (a carrier) and raises the alarm (trigger 16, message 0x992)
//! once per sighting of enemy soldiers within 32. Skipped when the game data is not present.

use s4_sim::World;
use s4_sim::entity::ObjType;

fn map(name: &str) -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join(name);
    if !p.exists() {
        return None;
    }
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    let rules = s4_sim::Rules::load_default().ok()?;
    s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).ok().map(|(w, _)| w)
}

#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    hash: u64,
    manned: usize,
    alarms: usize,
    messages: usize,
}

fn run(ticks: u32) -> Option<Outcome> {
    let mut w = map("AO_Viking3.map")?;
    let towers: Vec<u16> = w.entities.iter().filter(|(_, e)| e.obj_type == ObjType::Building && e.kind == 45).map(|(i, _)| i).collect();
    assert_eq!(towers.len(), 9);
    let (mut alarms, mut messages) = (0, 0);
    let mut last: std::collections::BTreeMap<u16, bool> = std::collections::BTreeMap::new();
    for _ in 0..ticks {
        w.events.clear();
        s4_sim::tick(&mut w, &[]);
        for e in &w.events {
            match *e {
                s4_sim::event::SimEvent::JobTrigger { building, trigger: 16 } if towers.contains(&building) => {
                    // Raised only when it was not up.
                    assert!(!last.get(&building).copied().unwrap_or(false));
                    alarms += 1;
                }
                s4_sim::event::SimEvent::Message { text: 0x992, player, x, y, filtered } => {
                    assert!(filtered);
                    let t = towers.iter().find(|&&t| w.entities.by_index(t as u32).is_some_and(|b| b.x() == x && b.y() == y)).expect("at a tower");
                    assert_eq!(w.entities.by_index(*t as u32).unwrap().owner, player);
                    messages += 1;
                }
                _ => {}
            }
        }
        for &t in &towers {
            if let Some(s) = s4_sim::buildings::lookout::get(&w, t) {
                last.insert(t, s.alarm);
            }
        }
    }
    let manned = towers.iter().filter(|&&t| s4_sim::buildings::have_inhabitant(&w, t)).count();
    Some(Outcome { hash: w.state_hash(), manned, alarms, messages })
}

#[test]
fn towers_watch() {
    let Some(a) = run(3000) else { return };
    eprintln!("{a:?}");
    assert_eq!(a.alarms, a.messages);
    assert!(a.manned > 0, "towers get their carrier");
    assert_eq!(Some(a), run(3000), "deterministic");
}

/// An enemy soldier put next to a manned tower raises the alarm once; when it is gone the alarm
/// is down again.
#[test]
fn alarm_once_per_sighting() {
    let Some(mut w) = map("AO_Viking3.map") else { return };
    for _ in 0..1500 {
        s4_sim::tick(&mut w, &[]);
    }
    let tower = w.entities.iter().find(|(i, e)| e.obj_type == ObjType::Building && e.kind == 45 && s4_sim::buildings::have_inhabitant(&w, *i)).map(|(i, _)| i).expect("a manned tower");
    let (tx, ty, owner) = {
        let t = w.entities.by_index(tower as u32).unwrap();
        (t.x(), t.y(), t.owner)
    };
    assert!(!s4_sim::buildings::lookout::get(&w, tower).unwrap().alarm);
    let soldier = w
        .entities
        .iter()
        .find(|(_, e)| e.obj_type == ObjType::Settler && e.owner != owner && e.owner != 0 && (2..=5).contains(&(e.flags & 0xf)))
        .map(|(i, _)| i)
        .expect("an enemy soldier");
    // Next to the tower, on a free node.
    let (sx, sy) = s4_sim::spiral::walk(w.grid, tx, ty, 12)
        .find(|&(x, y)| {
            let n = w.grid.index(x as u32, y as u32);
            w.layers.map_object[n] == 0 && w.layers.flags[n] & 1 == 0 && w.layers.ground[n] & 0xf0 == 0x10
        })
        .expect("room");
    let e = w.entities.by_index(soldier as u32).unwrap();
    let from = w.grid.index(e.x() as u32, e.y() as u32);
    w.layers.map_object[from] = 0;
    w.layers.map_object[w.grid.index(sx as u32, sy as u32)] = soldier;
    w.entities.by_index_mut(soldier as u32).unwrap().pos = s4_sim::grid::Y16X16::pack(sx, sy);
    s4_sim::warmap::readd(&mut w, soldier);
    let mut raised = 0;
    for _ in 0..62 {
        w.events.clear();
        s4_sim::tick(&mut w, &[]);
        raised += w.events.iter().filter(|e| matches!(e, s4_sim::event::SimEvent::JobTrigger { building, trigger: 16 } if *building == tower)).count();
    }
    assert_eq!(raised, 1, "the alarm goes up once");
    assert!(s4_sim::buildings::lookout::get(&w, tower).unwrap().alarm);
    w.kill(soldier, 0);
    for _ in 0..40 {
        s4_sim::tick(&mut w, &[]);
    }
    assert!(!s4_sim::buildings::lookout::get(&w, tower).unwrap().alarm, "down again");
}
