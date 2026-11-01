//! `CMineRole` (release 0x54b630 and helpers) on MCD2_roman1, driven by `tick()` only: the map's
//! mines order their miners, buy work units with the food carriers bring, dig their resource
//! from the nodes around them and fill their production piles. Skipped when the game data is not
//! present.

use s4_sim::World;
use s4_sim::buildings::RoleKind;
use s4_sim::entity::EntityData;

fn map() -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("MCD2_roman1.map");
    if !p.exists() {
        return None;
    }
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    let rules = s4_sim::Rules::load_default().ok()?;
    let (mut w, _) = s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).ok()?;
    let towers: Vec<(i32, i32)> = w
        .entities
        .iter()
        .filter_map(|(id, e)| match &e.data {
            EntityData::Building(_) if s4_sim::buildings::have_inhabitant(&w, id) => Some((e.x(), e.y())),
            _ => None,
        })
        .collect();
    for (x, y) in towers {
        s4_sim::territory::set_owner(&mut w, x, y);
    }
    Some(w)
}

/// Per mine: (id, resource type, resource units in reach (the first 64 spiral nodes)).
fn mines(w: &World) -> Vec<(u16, u8, u32)> {
    let mut out = Vec::new();
    for (&id, r) in &w.buildings.roles {
        let RoleKind::Mine(s) = &r.kind else { continue };
        let e = w.entities.by_index(id as u32).unwrap();
        let mut units = 0;
        for k in 0..64 {
            if let Some(n) = w.grid.checked_index(e.x() + s4_sim::spiral::dx(k), e.y() + s4_sim::spiral::dy(k))
                && w.layers.resource[n] & 0xF0 == s.resource
            {
                units += (w.layers.resource[n] & 0x0F) as u32;
            }
        }
        out.push((id, s.resource, units));
    }
    out
}

#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    hash: u64,
    /// Resource units dug, summed over all mines.
    dug: u32,
    /// Attempts that found the resource in the last 15 of every mine.
    found: u32,
}

fn run(ticks: u32) -> Option<Outcome> {
    let mut w = map()?;
    let before = mines(&w);
    assert!(before.len() >= 10, "MCD2_roman1 has {} mines", before.len());
    for _ in 0..ticks {
        s4_sim::tick(&mut w, &[]);
    }
    let after = mines(&w);
    let dug: u32 = before.iter().zip(&after).map(|(b, a)| b.2 - a.2).sum();
    let mut found = 0;
    for (&id, r) in &w.buildings.roles {
        let RoleKind::Mine(s) = &r.kind else { continue };
        assert!(r.base.worker_in_house || r.base.inhabitant != 0, "mine {id} has no miner");
        found += s.history.iter().filter(|&&b| b == 100).count() as u32;
    }
    Some(Outcome { hash: w.state_hash(), dug, found })
}

#[test]
fn mines_dig_with_food() {
    let Some(a) = run(3000) else { return };
    eprintln!("{a:?}");
    // Every successful attempt takes one unit off a node in reach (two mines may share nodes,
    // which only lowers the count of distinct units). The miners carried the goods out.
    assert!(a.dug >= 60, "only {} units dug", a.dug);
    assert!(a.found > 0);
    assert_eq!(Some(a), run(3000), "not deterministic");
}
