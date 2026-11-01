//! A soldier standing on a building site's footprint does not block it: the site (state 4,
//! `LogicUpdate` 0x541cc0) gives its `blockPosLines` cells move cost 7 and waits until
//! `CheckBlockable` holds for every cell; the idle soldier (behaviour `IdleWalk`, vtbl+16 of the
//! walking: on a cost-7 node it heads for the nearest cost < 7 node of its sector) walks off, and
//! the site goes on to state 5. On roman02, driven by `tick()` only; skipped when the game data
//! is not present.

use s4_sim::entity::EntityData;
use s4_sim::{Command, PlayerCommand, World};

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

/// (tick the site reached state 5, the soldier's node then, state hash).
fn run() -> Option<(u32, (i32, i32), u64)> {
    let mut w = roman02()?;
    let size = w.grid.size() as i32;
    let sites = s4_sim::build::possible_sites(&w, 1, 1, (0, 0), (size, size));
    let &(x, y, _) = sites.iter().min_by_key(|s| s.2)?;
    s4_sim::tick(&mut w, &[PlayerCommand { player: 1, command: Command::Build { building: 1, x, y } }]);
    let (&id, _) = w.sites.roles.iter().next().expect("a site");
    // The diggers are done; a swordsman stands on the building's own node.
    w.sites.roles.get_mut(&id).unwrap().dig_effort = 0;
    let soldier = s4_sim::create::add_settler(&mut w, x as i32, y as i32, 1, SWORDSMAN).expect("a swordsman");
    let start = w.tick;
    for _ in 0..2000 {
        s4_sim::tick(&mut w, &[]);
        if w.sites.roles.get(&id).is_some_and(|s| s.state >= 5) {
            let e = w.entities.by_index(soldier as u32)?;
            return Some((w.tick - start, (e.x(), e.y()), w.state_hash()));
        }
    }
    let e = w.entities.by_index(soldier as u32)?;
    panic!("the site is still blocked (state {:?}); the soldier stands at {},{}", w.sites.roles.get(&id).map(|s| s.state), e.x(), e.y());
}

/// The same with soldiers the map placed: a woodcutter site whose footprint holds one of player
/// 1's free map soldiers. (tick the site reached state 5, state hash).
fn run_map_soldier() -> Option<(u32, u64)> {
    let mut w = roman02()?;
    let size = w.grid.size() as i32;
    let soldiers: Vec<(i32, i32)> = w
        .entities
        .iter()
        .filter(|(_, e)| e.owner == 1 && e.warrior_type() != 0 && matches!(e.data, EntityData::Settler(_)))
        .filter(|(id, e)| w.grid.checked_index(e.x(), e.y()).is_some_and(|n| w.layers.map_object[n] == *id))
        .map(|(_, e)| (e.x(), e.y()))
        .collect();
    assert!(!soldiers.is_empty(), "roman02 has free soldiers of player 1");
    let info = w.rules.build.info(w.race_of(1), 1)?.clone();
    let sites = s4_sim::build::possible_sites(&w, 1, 1, (0, 0), (size, size));
    let &(x, y, _) = sites.iter().find(|&&(x, y, _)| info.cells(&info.block_lines, x as i32, y as i32).any(|c| soldiers.contains(&c)))?;
    s4_sim::tick(&mut w, &[PlayerCommand { player: 1, command: Command::Build { building: 1, x, y } }]);
    let (&id, _) = w.sites.roles.iter().next().expect("a site");
    w.sites.roles.get_mut(&id).unwrap().dig_effort = 0;
    let start = w.tick;
    for _ in 0..2000 {
        s4_sim::tick(&mut w, &[]);
        if w.sites.roles.get(&id).is_some_and(|s| s.state >= 5) {
            return Some((w.tick - start, w.state_hash()));
        }
    }
    panic!("the site at {x},{y} is still blocked (state {:?})", w.sites.roles.get(&id).map(|s| s.state));
}

#[test]
fn a_map_soldier_on_a_site_walks_off_it() {
    if roman02().is_none() {
        return;
    }
    let a = run_map_soldier().expect("a woodcutter site over a map soldier");
    eprintln!("{a:?}");
    assert_eq!(Some(a), run_map_soldier(), "not deterministic");
}

#[test]
fn a_soldier_on_a_site_walks_off_it() {
    let Some(a) = run() else { return };
    eprintln!("{a:?}");
    assert_eq!(Some(a), run(), "not deterministic");
}
