//! The Dark Tribe's shaman (`CShamanRole`, role 16, magic stage C) on dark03, driven by `tick()`:
//! its enslaving attack (0x587b60 → future event 8, 0x4e5460), the rot (0x4f4c70) and teleport
//! (0x4f4d10) casts its `LogicUpdateJob` (0x587820) makes on an order (event 3). Skipped without
//! the game data.

use s4_sim::World;
use s4_sim::entity::{EntityData, ObjType};
use s4_sim::jobs::role;

const SHAMAN: u16 = 0x36;
const SLAVE: u16 = 0x37;

fn map(name: &str) -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join(name);
    if !p.exists() {
        return None;
    }
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    let rules = s4_sim::Rules::load_default().ok()?;
    s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).ok().map(|(w, _)| w)
}

fn job(w: &World, i: u16) -> &s4_sim::jobs::JobState {
    match &w.entities.by_index(i as u32).unwrap().data {
        EntityData::Settler(s) => &s.job,
        _ => panic!("not a settler"),
    }
}

fn dark_player(w: &World) -> u8 {
    (1..=8).find(|&p| w.players.get(p as usize).and_then(|p| p.as_ref()).is_some_and(|p| p.race == 3)).expect("a Dark Tribe player")
}

/// Free land (dark land, node flag 0x04, allowed).
fn free_node(w: &World, x: i32, y: i32) -> bool {
    w.grid.checked_index(x, y).is_some_and(|n| w.layers.ground[n] >= 16 && w.layers.flags[n] & 0x7B == 0 && w.layers.map_object[n] == 0 && w.tiling.sector_of(n) != 0)
}

fn count(w: &World, player: u8, kind: u16) -> usize {
    w.entities.iter().filter(|(_, e)| e.obj_type == ObjType::Settler && e.owner == player && e.kind == kind && !e.is_dead()).count()
}

#[test]
fn a_shaman_enslaves_an_enemy_carrier() {
    let Some(mut w) = map("dark03.map") else { return };
    let dt = dark_player(&w);
    assert_eq!(w.rules.settler_role(3, SHAMAN), role::SHAMAN);
    // An idle carrier of another player standing on its own land, and a free node 2 away.
    let (c, cp, owner) = w
        .entities
        .iter()
        .filter(|(_, e)| e.obj_type == ObjType::Settler && e.kind == 1 && e.owner != dt)
        .find_map(|(i, e)| {
            let p = (e.x(), e.y());
            let spot = (s4_sim::spiral::first(2)..=s4_sim::spiral::last(2)).map(|k| (p.0 + s4_sim::spiral::dx(k), p.1 + s4_sim::spiral::dy(k))).find(|&(x, y)| free_node(&w, x, y))?;
            Some((i, spot, e.owner))
        })
        .expect("an enemy carrier");
    let s = s4_sim::create::add_settler(&mut w, cp.0, cp.1, dt, SHAMAN).expect("shaman");
    assert_eq!(job(&w, s).role, role::SHAMAN);
    let pool = w.rules.magic.dark.shaman_mana_pool;
    assert_eq!(job(&w, s).soldier.shaman_mana, pool);
    let slaves = count(&w, dt, SLAVE);
    let mut enslaved = None;
    for t in 0..600 {
        s4_sim::tick(&mut w, &[]);
        if w.entities.by_index(c as u32).is_none_or(|e| e.is_dead()) {
            enslaved = Some(t);
            break;
        }
    }
    let mana = job(&w, s).soldier.shaman_mana;
    eprintln!("carrier {c} of player {owner}: gone after {enslaved:?} ticks; shaman mana {mana} (pool {pool})");
    assert!(enslaved.is_some(), "the carrier was not enslaved");
    // One slave per enslaving hit (each paid from the shaman's mana, landing 5 ticks later).
    let cost = w.rules.magic.dark.mana_cost_enslave;
    let before = ((pool - mana) / cost) as usize;
    for _ in 0..6 {
        s4_sim::tick(&mut w, &[]);
    }
    let after = ((pool - job(&w, s).soldier.shaman_mana) / cost) as usize;
    let n = count(&w, dt, SLAVE) - slaves;
    eprintln!("{n} slaves for {before}..={after} hits");
    assert!(before >= 1 && (before..=after).contains(&n), "a slave per hit");
}

#[test]
fn a_shaman_rots_a_field_and_teleports_boards() {
    let Some(mut w) = map("dark03.map") else { return };
    let dt = dark_player(&w);
    // The Dark Tribe's castle; a free stretch of land 6..12 away for a young grain field.
    let castle = w
        .entities
        .iter()
        .find(|(id, e)| e.owner == dt && e.obj_type == ObjType::Building && s4_sim::buildings::have_inhabitant(&w, *id))
        .map(|(_, e)| (e.x(), e.y()))
        .expect("a Dark Tribe castle");
    let ring = |c: (i32, i32), a: u8, b: u8| (s4_sim::spiral::first(a)..=s4_sim::spiral::last(b)).map(move |k| (c.0 + s4_sim::spiral::dx(k), c.1 + s4_sim::spiral::dy(k)));
    let (fx, fy) = ring(castle, 6, 20).find(|&(x, y)| free_node(&w, x, y) && w.layers.deco_object[w.grid.index(x as u32, y as u32)] == 0).expect("a field node");
    let grain = s4_sim::deco::plant::plant(&mut w, fx, fy, 209).expect("grain");
    let spot = ring((fx, fy), 2, 3).find(|&(x, y)| free_node(&w, x, y)).expect("a node near the field");
    let s = s4_sim::create::add_settler(&mut w, spot.0, spot.1, dt, SHAMAN).expect("shaman");
    let pool = w.rules.magic.dark.shaman_mana_pool;
    let order = |w: &mut World, x: i32, y: i32| {
        let at = s4_sim::grid::Y16X16::pack(x, y).0 as i32;
        s4_sim::jobs::core::set_event(w, s, s4_sim::jobs::state::EntityEvent { event: 3, kind: 0, data: [at, 0, 0] });
    };
    order(&mut w, fx, fy);
    let stage = |w: &World| match w.entities.by_index(grain as u32).map(|e| &e.data) {
        Some(EntityData::Deco(d)) => d.stage,
        _ => 0xff,
    };
    let mut t_rot = None;
    for t in 0..400 {
        s4_sim::tick(&mut w, &[]);
        if stage(&w) == 5 {
            t_rot = Some(t);
            break;
        }
    }
    let mana = job(&w, s).soldier.shaman_mana;
    eprintln!("field ({fx}, {fy}): rotten after {t_rot:?} ticks, shaman mana {mana} (pool {pool})");
    assert!(t_rot.is_some(), "the young field did not rot");
    assert_eq!(job(&w, s).soldier.shaman_cast_at, 0);
    // Five boards on a free node near the shaman, then a teleport order there.
    let here = w.entities.by_index(s as u32).unwrap().pos;
    let p = ring((here.x(), here.y()), 2, 4).find(|&(x, y)| free_node(&w, x, y) && w.layers.deco_object[w.grid.index(x as u32, y as u32)] == 0).expect("a pile node");
    s4_sim::pile::search_space_for_goods(&mut w, p.0, p.1, 7, 5);
    let pile = w.layers.deco_object[w.grid.index(p.0 as u32, p.1 as u32)];
    let amount = |w: &World| match w.entities.by_index(pile as u32).filter(|e| !e.is_dead()).map(|e| &e.data) {
        Some(EntityData::Pile(d)) => d.amount,
        _ => 0,
    };
    assert_eq!(amount(&w), 5);
    order(&mut w, p.0, p.1);
    for _ in 0..400 {
        s4_sim::tick(&mut w, &[]);
        if amount(&w) == 0 {
            break;
        }
    }
    let mana2 = job(&w, s).soldier.shaman_mana;
    eprintln!("boards at {p:?}: {} left, shaman mana {mana2}", amount(&w));
    assert_eq!(amount(&w), 0, "the boards were not teleported");
}
