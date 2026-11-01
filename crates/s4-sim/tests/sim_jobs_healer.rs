//! The healer (`CSimpleBuildingRole` 0x54ef60, building type 27) on roman02, driven by `tick()`
//! only: a healer's hut set up like a map building orders its healer; a hurt swordsman standing
//! on the hut's flag is healed (event 3 with the hut's work list on that soldier, then
//! `CSettler` vtbl+24(-1) at the end of WORK_VIRTUAL: full health). Skipped when the game data is
//! not present.
//!
//! The hut also calls hurt soldiers of its working area to the flag (event 0x11 kind 0xd), which
//! the soldier role answers by walking there.

use s4_sim::World;
use s4_sim::entity::EntityData;

const HEALERHUT: u8 = 27;
const SWORDSMAN: u16 = 29;

fn roman02() -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map");
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

/// (the hut called the soldier while it stood nearby, tick it was healed, the healer's type,
/// state hash).
fn run() -> Option<(bool, u32, u16, u64)> {
    let mut w = roman02()?;
    let castle = w.entities.iter().find(|(id, e)| e.owner == 1 && matches!(&e.data, EntityData::Building(_)) && s4_sim::buildings::have_inhabitant(&w, *id)).map(|(_, e)| (e.x(), e.y()))?;
    let size = w.grid.size() as i32;
    let &(x, y, _) = s4_sim::build::possible_sites(&w, 1, HEALERHUT, (0, 0), (size, size))
        .iter()
        .min_by_key(|s| (s.0 as i32 - castle.0).abs() + (s.1 as i32 - castle.1).abs())?;
    let hut = s4_sim::create::add_building(&mut w, x as i32, y as i32, 1, HEALERHUT).expect("healer hut");
    let ensign = match &w.entities.by_index(hut as u32)?.data {
        EntityData::Building(d) => d.ensign,
        _ => return None,
    };
    let soldier = s4_sim::create::add_settler(&mut w, ensign.x(), ensign.y(), 1, SWORDSMAN)?;
    let max = w.rules.settler_health(0, SWORDSMAN);
    if let Some(EntityData::Settler(s)) = w.entities.by_index_mut(soldier as u32).map(|e| &mut e.data) {
        s.health = 1;
    }
    let pause = |w: &World| match &s4_sim::buildings::role(w, hut)?.kind {
        s4_sim::buildings::RoleKind::Simple(s) => Some(s.pause),
        _ => None,
    };
    // The soldier steps off the flag; the hut, its healer home, calls it back (event 0x11 kind
    // 0xd, then a pause of 10); the soldier role walks it to the flag (sim's `move_order`),
    // where the healer heals it.
    let mut called = false;
    for _ in 0..6000 {
        s4_sim::tick(&mut w, &[]);
        called |= pause(&w) == Some(10);
        if let Some(EntityData::Settler(s)) = w.entities.by_index(soldier as u32).map(|e| &e.data)
            && s.health == max
        {
            let healer = s4_sim::buildings::role(&w, hut).map_or(0, |r| r.base.inhabitant);
            let kind = w.entities.by_index(healer as u32).map_or(0, |e| e.kind);
            return Some((called, w.tick, kind, w.state_hash()));
        }
    }
    panic!("the soldier was not healed");
}

#[test]
fn healer_heals_a_soldier_on_the_flag() {
    let Some(a) = run() else { return };
    eprintln!("{a:?}");
    assert!(a.0, "the hut did not call the hurt soldier");
    assert_eq!(a.2, 20, "healed by the hut's healer");
    assert_eq!(Some(a), run(), "not deterministic");
}
