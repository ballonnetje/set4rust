//! User bug 2026-10-20: "soldiers I made cannot leave the player's territory". A carrier turned
//! swordsman at the barracks (`ChangeType` → `CSettler::SetRole` 0x580c20) must walk with the
//! soldier role's `CWalkingNormal` (vtbl+0x14, 0x58a9d0), not keep the carrier's
//! `CWalkingWorker` (own land only). Sent with command 5014 (`SendTo` 0x56a800, formation
//! 0x56a000 on the walking sector) into neutral and enemy land on roman02, it gets there.
//! Skipped without the game data.

use s4_sim::build::possible_sites;
use s4_sim::entity::{EntityData, ObjType};
use s4_sim::jobs::role;
use s4_sim::pathing::walking::walking_type;
use s4_sim::{Rules, World};

const BARRACKS: u8 = 24;
const SWORD: u8 = 34;
const SWORDSMAN_1: u16 = 29;

fn roman02() -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map");
    if !p.exists() {
        return None;
    }
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    let rules = Rules::load_default().ok()?;
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

fn job(w: &World, s: u16) -> &s4_sim::jobs::JobState {
    match &w.entities.by_index(s as u32).expect("settler").data {
        EntityData::Settler(d) => &d.job,
        _ => panic!("{s} is not a settler"),
    }
}

fn free_node(w: &World, x: i32, y: i32) -> bool {
    w.grid.checked_index(x, y).is_some_and(|n| w.layers.ground[n] >= 16 && w.layers.flags[n] & 0x7F == 0 && w.layers.map_object[n] == 0 && w.tiling.sector_of(n) != 0)
}

/// A swordsman trained at a new barracks next to player 1's castle (as in `sim_military`).
fn recruit(w: &mut World) -> u16 {
    let castle = w
        .entities
        .iter()
        .find(|(id, e)| e.owner == 1 && e.obj_type == ObjType::Building && s4_sim::buildings::have_inhabitant(w, *id))
        .map(|(_, e)| (e.x(), e.y()))
        .expect("castle");
    let size = w.grid.size() as i32;
    let &(x, y, _) = possible_sites(w, 1, BARRACKS, (0, 0), (size, size))
        .iter()
        .min_by_key(|s| s4_sim::spiral::grid_distance(s.0 as i32 - castle.0, s.1 as i32 - castle.1))
        .expect("a barracks site");
    let barracks = s4_sim::create::add_building(w, x as i32, y as i32, 1, BARRACKS).expect("barracks");
    let sword_pile = s4_sim::buildings::barrack::pile_with_good(w, barracks, SWORD);
    if let Some(EntityData::Pile(p)) = w.entities.by_index_mut(sword_pile as u32).map(|e| &mut e.data) {
        p.amount = 1;
    }
    let before: Vec<u16> = s4_sim::settler_lists::iter(w, 1, SWORDSMAN_1).collect();
    let cmd = s4_sim::PlayerCommand { player: 1, command: s4_sim::Command::SoldierOrder { building: barracks, kind: SWORDSMAN_1, amount: 1 } };
    s4_sim::tick(w, &[cmd]);
    for _ in 0..4000 {
        s4_sim::tick(w, &[]);
        let new = s4_sim::settler_lists::iter(w, 1, SWORDSMAN_1).find(|s| !before.contains(s));
        if let Some(s) = new {
            // Let it reach the rally point and become idle.
            for _ in 0..600 {
                s4_sim::tick(w, &[]);
            }
            return s;
        }
    }
    panic!("no swordsman was made");
}

/// The nearest free node at least `min` away in the soldier's walking sector whose land belongs
/// to `land(owner)`.
fn target(w: &World, s: u16, min: u8, land: impl Fn(u8) -> bool) -> Option<(i32, i32)> {
    let p = w.entities.by_index(s as u32).unwrap().pos;
    let sector = w.tiling.sector_of(w.grid.index(p.x() as u32, p.y() as u32));
    (s4_sim::spiral::first(min)..=s4_sim::spiral::last(60))
        .map(|k| (p.x() + s4_sim::spiral::dx(k), p.y() + s4_sim::spiral::dy(k)))
        .filter(|&(x, y)| free_node(w, x, y))
        .find(|&(x, y)| {
            let n = w.grid.index(x as u32, y as u32);
            w.tiling.sector_of(n) == sector && land(w.owner_of(n)) && {
                // A few nodes deep, so the soldier really is off its own land on arrival.
                (0..=s4_sim::spiral::last(3)).all(|k| w.grid.checked_index(x + s4_sim::spiral::dx(k), y + s4_sim::spiral::dy(k)).is_some_and(|m| land(w.owner_of(m))))
            }
        })
}

fn send_and_walk(w: &mut World, s: u16, (x, y): (i32, i32)) -> i32 {
    let cmd = s4_sim::PlayerCommand { player: 1, command: s4_sim::Command::SendTo { x: x as u16, y: y as u16, mode: 0, entities: vec![s] } };
    s4_sim::tick(w, &[cmd]);
    let mut best = i32::MAX;
    for _ in 0..3000 {
        s4_sim::tick(w, &[]);
        let p = w.entities.by_index(s as u32).unwrap().pos;
        best = best.min(s4_sim::spiral::grid_distance(p.x() - x, p.y() - y));
        if best == 0 {
            break;
        }
    }
    best
}

#[test]
fn a_recruited_soldier_walks_into_neutral_and_enemy_land() {
    let Some(mut w) = roman02() else { return };
    let s = recruit(&mut w);
    assert_eq!(job(&w, s).role, role::SOLDIER);
    let kind = job(&w, s).walking.as_ref().map(|k| k.kind);
    assert_eq!(kind, Some(walking_type::NORMAL), "the recruit kept the carrier's walking object");

    let neutral = target(&w, s, 8, |o| o == 0).expect("neutral land in the soldier's sector");
    let d = send_and_walk(&mut w, s, neutral);
    let p = w.entities.by_index(s as u32).unwrap().pos;
    eprintln!("neutral target {neutral:?}: soldier at {p:?}, closest {d}");
    assert_eq!(d, 0, "the soldier did not reach neutral land");

    // roman02 has no enemy land in reach: a player-2 small tower deep in neutral land takes some.
    let enemy = target(&w, s, 8, |o| o == 2).unwrap_or_else(|| {
        enemy_tower(&mut w, s);
        target(&w, s, 8, |o| o == 2).expect("player 2 land in the soldier's sector")
    });
    let d = send_and_walk(&mut w, s, enemy);
    let p = w.entities.by_index(s as u32).unwrap().pos;
    eprintln!("enemy target {enemy:?}: soldier at {p:?}, closest {d}");
    assert_eq!(d, 0, "the soldier did not reach enemy land");
}

/// An occupied player-2 small tower on a node with only neutral land within 24, its footprint
/// free, in the soldier's walking sector.
fn enemy_tower(w: &mut World, s: u16) {
    const SMALL_TOWER: u8 = 46;
    let p = w.entities.by_index(s as u32).unwrap().pos;
    let sector = w.tiling.sector_of(w.grid.index(p.x() as u32, p.y() as u32));
    let near = |w: &World, (x, y): (i32, i32), r: u8, f: &dyn Fn(usize) -> bool| {
        (0..=s4_sim::spiral::last(r)).all(|k| w.grid.checked_index(x + s4_sim::spiral::dx(k), y + s4_sim::spiral::dy(k)).is_some_and(f))
    };
    let (x, y) = (s4_sim::spiral::first(20)..=s4_sim::spiral::last(80))
        .map(|k| (p.x() + s4_sim::spiral::dx(k), p.y() + s4_sim::spiral::dy(k)))
        .filter(|&(x, y)| free_node(w, x, y) && w.tiling.sector_of(w.grid.index(x as u32, y as u32)) == sector)
        .find(|&q| near(w, q, 4, &|n| w.layers.ground[n] >= 16 && w.layers.flags[n] & 0x7F == 0 && w.layers.map_object[n] == 0) && near(w, q, 24, &|n| w.owner_of(n) == 0))
        .expect("a place for an enemy tower");
    let tower = s4_sim::create::add_building(w, x, y, 2, SMALL_TOWER).expect("tower");
    let k = (s4_sim::spiral::first(5)..s4_sim::spiral::last(10)).find(|&k| free_node(w, x + s4_sim::spiral::dx(k), y + s4_sim::spiral::dy(k))).expect("a free node");
    s4_sim::create::add_settler(w, x + s4_sim::spiral::dx(k), y + s4_sim::spiral::dy(k), 2, SWORDSMAN_1).expect("swordsman");
    for _ in 0..3000 {
        s4_sim::tick(w, &[]);
        if s4_sim::buildings::military::is_inhabited(w, tower) {
            eprintln!("player 2 tower {tower} at ({x}, {y})");
            return;
        }
    }
    panic!("the player 2 tower was not occupied");
}
