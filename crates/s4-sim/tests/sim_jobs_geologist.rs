//! Geologists (`CGeologistRole`, role 12) on MCD2_roman1, driven by `tick()` and a SendTo
//! command only: player 1's geologists, sent into the mountains, search, reserve spots and put up
//! signs that match the resource under them. Skipped when the game data is not present.

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

const GEOLOGIST: u16 = 49;

/// The sign for resource byte `r` (0x57b1b0).
fn sign_for(r: u8) -> u16 {
    let amount = (r & 0xf) as u16;
    let level = amount.saturating_sub(1) / 5;
    match r & 0xf0 {
        0x10 => 194 + level,
        0x20 => 200 + level,
        0x30 => 197 + level,
        0x40 => 206 + level,
        0x50 => 203 + level,
        _ => 193,
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    hash: u64,
    signs: usize,
    kinds: std::collections::BTreeMap<u16, usize>,
    messages: usize,
}

fn run(ticks: u32) -> Option<Outcome> {
    let mut w = map("MCD2_roman1.map")?;
    let gs: Vec<u16> = w.entities.iter().filter(|(_, e)| e.obj_type == ObjType::Settler && e.kind == GEOLOGIST && e.owner == 1).map(|(i, _)| i).take(3).collect();
    assert!(!gs.is_empty(), "MCD2_roman1 has geologists of player 1");
    let g = w.entities.by_index(gs[0] as u32).unwrap();
    let (gx, gy) = (g.x(), g.y());
    let sector = w.tiling.sector_of(w.grid.index(gx as u32, gy as u32));
    // The nearest mountain node with a resource, on his sector.
    let (tx, ty) = (0..s4_sim::spiral::last(60))
        .map(|k| (gx + s4_sim::spiral::dx(k), gy + s4_sim::spiral::dy(k)))
        .find(|&(x, y)| {
            w.grid.checked_index(x, y).is_some_and(|n| w.layers.ground[n] & 0xf0 == 0x20 && w.layers.resource[n] & 0xf0 != 0 && w.tiling.sector_of(n) == sector)
        })
        .expect("mountains nearby");
    let before = s4_sim::jobs::geologist::signs(&w).len();
    let cmd = s4_sim::PlayerCommand { player: 1, command: s4_sim::Command::SendTo { x: tx as u16, y: ty as u16, mode: 0, entities: gs.clone() } };
    s4_sim::tick(&mut w, &[cmd]);
    let mut messages = 0;
    for _ in 0..ticks {
        w.events.clear();
        s4_sim::tick(&mut w, &[]);
        for e in &w.events {
            match e {
                s4_sim::event::SimEvent::Unported { what, .. } => assert!(!what.contains("Geologist"), "{what}"),
                // Resource found (0x77f..=0x783) and many of it (0x78f..=0x793).
                s4_sim::event::SimEvent::Message { player, text, .. } if (0x77f..=0x783).contains(text) || (0x78f..=0x793).contains(text) => {
                    assert_eq!(*player, 1);
                    messages += 1;
                }
                _ => {}
            }
        }
    }
    let signs = s4_sim::jobs::geologist::signs(&w);
    let mut kinds = std::collections::BTreeMap::new();
    for &(p, kind) in &signs {
        let n = w.grid.index(p.x() as u32, p.y() as u32);
        assert_eq!(w.layers.ground[n] & 0xf0, 0x20, "signs stand in the mountains");
        assert_eq!(kind, sign_for(w.layers.resource[n]), "the sign at ({}, {}) shows its resource", p.x(), p.y());
        *kinds.entry(kind).or_insert(0) += 1;
    }
    assert!(signs.len() > before, "geologists put up signs");
    // (Geologists are warriors to the enemy: one working near player 2's soldiers can be killed.)
    for &i in &gs {
        if let Some(e) = w.entities.by_index(i as u32).filter(|e| !e.is_dead() && e.kind == GEOLOGIST)
            && let s4_sim::entity::EntityData::Settler(s) = &e.data
        {
            assert_eq!(s.job.list_id, 177, "geologist {i} works");
        }
    }
    Some(Outcome { hash: w.state_hash(), signs: signs.len() - before, kinds, messages })
}

#[test]
fn geologists_put_up_signs() {
    let Some(a) = run(3000) else { return };
    eprintln!("{a:?}");
    assert!(a.signs >= 10);
    assert_eq!(Some(a), run(3000), "deterministic");
}


