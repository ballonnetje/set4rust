//! The dark temple's units (magic stage C): command 5047 → `CDarkTempleRole::TakeOrder`
//! (0x545d60) and the production from mana in its update (0x545760, `DTCost*` via `CAIMain`
//! vtbl+0x34 0x438a10). dark03, driven by `tick()`. Skipped without the game data.

use s4_sim::World;
use s4_sim::buildings::dark_temple::{self, UNIT_TYPES};
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

fn temple(w: &World) -> (u16, u8) {
    w.buildings
        .roles
        .iter()
        .find(|(_, r)| matches!(r.kind, s4_sim::buildings::RoleKind::DarkTemple(_)))
        .map(|(&id, _)| (id, w.entities.by_index(id as u32).unwrap().owner))
        .expect("a dark temple")
}

fn count(w: &World, player: u8, kind: u16) -> usize {
    w.entities.iter().filter(|(_, e)| e.obj_type == ObjType::Settler && e.owner == player && e.kind == kind && !e.is_dead()).count()
}

#[test]
fn take_order_rules() {
    let Some(mut w) = map("dark03.map") else { return };
    let (t, _) = temple(&w);
    let orders = |w: &World| dark_temple::get(w, t).unwrap().orders;
    dark_temple::take_order(&mut w, t, 29, 5);
    dark_temple::take_order(&mut w, t, 29, 120);
    dark_temple::take_order(&mut w, t, 32, 150);
    dark_temple::take_order(&mut w, t, 54, 98);
    dark_temple::take_order(&mut w, t, 54, 7);
    dark_temple::take_order(&mut w, t, 52, -3);
    dark_temple::take_order(&mut w, t, 1, 5); // not a temple unit
    assert_eq!(orders(&w), [0, 0, 0, 99, 100, 100]);
    dark_temple::take_order(&mut w, t, 32, 10003);
    dark_temple::take_order(&mut w, t, 29, -100);
    dark_temple::take_order(&mut w, t, 54, -5);
    assert_eq!(orders(&w), [0, 0, 0, 94, 0, 3]);
    assert_eq!(UNIT_TYPES, [0, 53, 52, 54, 29, 32]);
}

#[test]
fn the_temple_buys_ordered_units_with_mana() {
    let Some(mut w) = map("dark03.map") else { return };
    let (t, p) = temple(&w);
    w.magic.players[p as usize].mana = 50;
    let (sw0, bow0) = (count(&w, p, 29), count(&w, p, 32));
    let order = |b: u16, kind: u16, amount: u32| s4_sim::PlayerCommand { player: p, command: s4_sim::Command::SoldierOrder { building: b, kind, amount } };
    s4_sim::tick(&mut w, &[order(t, 29, 2), order(t, 32, 1)]);
    for _ in 0..200 {
        s4_sim::tick(&mut w, &[]);
    }
    let (sw, bow) = (count(&w, p, 29) - sw0, count(&w, p, 32) - bow0);
    let mana = s4_sim::magic::mana(&w, p);
    eprintln!("player {p}: {sw} swordsmen, {bow} bowmen, mana {mana}, orders {:?}", dark_temple::get(&w, t).unwrap().orders);
    // 50 mana: two swordsmen (15 each) and one bowman (15) leave 5 (< 10, nothing more).
    assert_eq!((sw, bow, mana), (2, 1, 5));
    assert_eq!(dark_temple::get(&w, t).unwrap().orders, [0; 6]);
}

/// The aura (0x5458c0): with mushroom farms (`CBuildingMgr` type-49 count) enemy warriors on dark
/// land near the temple are struck down (future event 2, 255 damage).
#[test]
fn the_aura_strikes_enemy_warriors_on_dark_land() {
    let Some(mut w) = map("dark03.map") else { return };
    let (t, p) = temple(&w);
    let farms = w.statistic.players[p as usize].count[49];
    w.statistic.players[p as usize].count[49] = 3;
    let tp = w.entities.by_index(t as u32).unwrap().pos;
    let ensign = match &w.entities.by_index(t as u32).unwrap().data {
        s4_sim::entity::EntityData::Building(d) => d.ensign,
        _ => unreachable!(),
    };
    let sector = w.tiling.sector_of(w.grid.index(ensign.x() as u32, ensign.y() as u32));
    let enemy = (1..=8u8).find(|&q| q != p && w.players.get(q as usize).and_then(|x| x.as_ref()).is_some()).expect("an enemy player");
    // Enemy swordsmen on free dark land within 10 of the aura's centre.
    let (cx, cy) = (tp.x() + 1, tp.y() + 2);
    let spots: Vec<(i32, i32)> = (s4_sim::spiral::first(4)..=s4_sim::spiral::last(10))
        .map(|k| (cx + s4_sim::spiral::dx(k), cy + s4_sim::spiral::dy(k)))
        .filter(|&(x, y)| {
            w.grid.checked_index(x, y).is_some_and(|n| w.layers.flags[n] & 0x7F == 4 && w.layers.map_object[n] == 0 && w.layers.ground[n] >= 16 && w.tiling.sector_of(n) == sector)
        })
        .step_by(7)
        .take(6)
        .collect();
    assert!(!spots.is_empty(), "dark land near the temple");
    let soldiers: Vec<u16> = spots.iter().filter_map(|&(x, y)| s4_sim::create::add_settler(&mut w, x, y, enemy, 29)).collect();
    // By unique id: a struck soldier's slot is reused 31 ticks after its death.
    let uids: Vec<u32> = soldiers.iter().map(|&s| w.entities.by_index(s as u32).unwrap().uid).collect();
    let mut dead = 0;
    for _ in 0..400 {
        s4_sim::tick(&mut w, &[]);
        dead = soldiers.iter().zip(&uids).filter(|&(&s, &u)| w.entities.by_index(s as u32).is_none_or(|e| e.is_dead() || e.uid != u)).count();
        if dead == soldiers.len() {
            break;
        }
    }
    eprintln!("farms on the map {farms}; {dead} of {} enemy swordsmen struck", soldiers.len());
    assert!(dead > 0, "the aura struck nobody");
}
