//! Military buildings (`CMilitaryBuildingRole` / `CCastleRole`, `buildings::military`; release
//! 0x548390 Init, 0x548ed0 LogicUpdate, 0x5488d0 SettlerEnter, 0x582ca0 soldier search,
//! 0x549390 soldiers leave) with `jobs::soldier` (`CSoldierRole`, `CTowerSoldier`) and
//! `jobs::door` (`CDoorRole`), on roman02 and driven by `tick()` only. Skipped without the game
//! data.

use s4_sim::buildings::military;
use s4_sim::build::possible_sites;
use s4_sim::entity::{EntityData, ObjType, flag};
use s4_sim::grid::Y16X16;
use s4_sim::jobs::role;
use s4_sim::{Rules, World};

const SMALL_TOWER: u8 = 46;
const SWORDSMAN_1: u16 = 29;
const BOWMAN_1: u16 = 32;

fn roman02() -> Option<World> {
    roman02_with(&s4_sim::NewGame::default())
}

fn roman02_with(setup: &s4_sim::NewGame) -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map");
    if !p.exists() {
        return None;
    }
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    let rules = Rules::load_default().ok()?;
    let (mut w, _) = s4_sim::new_game(&map, rules, setup).ok()?;
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

fn military_buildings(w: &World) -> Vec<u16> {
    w.buildings.roles.iter().filter(|(_, r)| matches!(r.kind, s4_sim::buildings::RoleKind::Military(_))).map(|(&id, _)| id).collect()
}

fn job(w: &World, s: u16) -> &s4_sim::jobs::JobState {
    match &w.entities.by_index(s as u32).expect("settler").data {
        EntityData::Settler(d) => &d.job,
        _ => panic!("{s} is not a settler"),
    }
}

/// Checks every invariant of one military building's garrison against the entities.
fn check_garrison(w: &World, id: u16) -> (usize, usize) {
    let b = w.entities.by_index(id as u32).unwrap();
    let EntityData::Building(d) = &b.data else { panic!() };
    let st = military::state(w, id).unwrap();
    let rule = w.rules.building(b.race, d.type_ex);
    // The door settler: type 69 on the door node, homed at the building, not visible.
    let door = w.entities.by_index(st.door as u32).expect("door settler");
    assert_eq!((door.obj_type, door.kind, door.pos), (ObjType::Settler, s4_sim::create::DOOR_SETTLER, d.door), "building {id}");
    assert_eq!(w.layers.map_object[w.grid.index(d.door.x() as u32, d.door.y() as u32)], st.door);
    assert_eq!(job(w, st.door).home, id);
    assert_eq!(job(w, st.door).role, role::DOOR);
    assert_eq!(door.flags & flag::VISIBLE, 0);
    assert_eq!(st.slots.len(), rule.soldier_slots.len());
    let (mut tops, mut walls) = (0, 0);
    for (k, &s) in st.slots.iter().enumerate() {
        if s == 0 {
            continue;
        }
        let e = w.entities.by_index(s as u32).unwrap();
        let j = job(w, s);
        assert_eq!((j.role, j.home, j.tower.slot as usize), (role::TOWER_SOLDIER, id, k), "soldier {s} of {id}");
        assert!(e.flags & flag::ON_BOARD != 0 && e.flags & flag::SELECTABLE == 0 && e.flags & flag::VULNERABLE_MASK == 0);
        let top = rule.soldier_slots[k].top;
        assert_eq!(e.warrior_type(), if top { 3 } else { 2 }, "slot {k} of {id}");
        if top {
            tops += 1;
        } else {
            walls += 1;
        }
    }
    assert_eq!((st.bow_in as usize, st.sword_in as usize), (tops, walls), "building {id}");
    (tops, walls)
}

#[test]
fn start_garrisons_are_complete() {
    let Some(w) = roman02() else { return };
    let ids = military_buildings(&w);
    assert!(!ids.is_empty());
    let mut soldiers = 0;
    for &id in &ids {
        let (tops, walls) = check_garrison(&w, id);
        let st = military::state(&w, id).unwrap();
        // `AddSoldierToStartTower` (0x53dab0): wish and order + 1 per soldier, entering turns the
        // order into presence; while loading, `FirstSettlerEnter` keeps the door settler.
        assert_eq!((st.bow_coming, st.sword_coming), (0, 0));
        assert_eq!((st.bow_wish as usize, st.sword_wish as usize), (tops, walls));
        assert_eq!(military::is_inhabited(&w, id), tops + walls > 0);
        soldiers += tops + walls;
    }
    eprintln!("{} military buildings, {soldiers} soldiers inside", ids.len());
    assert!(soldiers > 0);
}

/// A new small tower next to the castle, a free swordsman nearby: the tower orders him
/// (0x549c50: an empty tower wishes one swordsman), he walks to the door and enters; the first
/// soldier makes it inhabited (new door settler, territory); crushing it lets him out again.
#[test]
fn a_new_tower_calls_a_soldier_and_lets_him_out_when_crushed() {
    let Some(mut w) = roman02() else { return };
    let castle = w
        .entities
        .iter()
        .find(|(id, e)| e.owner == 1 && e.obj_type == ObjType::Building && s4_sim::buildings::have_inhabitant(&w, *id))
        .map(|(_, e)| (e.x(), e.y()))
        .expect("castle");
    let size = w.grid.size() as i32;
    let sites = possible_sites(&w, 1, SMALL_TOWER, (0, 0), (size, size));
    // The site farthest from the castle: near the border, where territory can grow.
    let &(x, y, _) = sites
        .iter()
        .max_by_key(|s| (s4_sim::spiral::grid_distance(s.0 as i32 - castle.0, s.1 as i32 - castle.1), s.1, s.0))
        .expect("a tower site");
    let owned_before = (0..w.grid.len()).filter(|&n| w.owner_of(n) == 1).count();
    let tower = s4_sim::create::add_building(&mut w, x as i32, y as i32, 1, SMALL_TOWER).expect("tower");
    let first_door = military::state(&w, tower).unwrap().door;
    assert!(!military::is_inhabited(&w, tower));
    // A free swordsman on the first free node 5..10 from the tower.
    let (tx, ty) = (x as i32, y as i32);
    let k = (s4_sim::spiral::first(5)..s4_sim::spiral::last(10))
        .find(|&k| {
            let n = w.grid.index((tx + s4_sim::spiral::dx(k)) as u32, (ty + s4_sim::spiral::dy(k)) as u32);
            w.layers.flags[n] & 0x7F == 0 && w.layers.map_object[n] == 0 && w.tiling.sector_of(n) != 0
        })
        .expect("a free node");
    let sword = s4_sim::create::add_settler(&mut w, tx + s4_sim::spiral::dx(k), ty + s4_sim::spiral::dy(k), 1, SWORDSMAN_1).expect("swordsman");
    assert_eq!(job(&w, sword).role, role::SOLDIER);
    assert!(s4_sim::settler_lists::iter(&w, 1, SWORDSMAN_1).any(|s| s == sword));

    let mut ordered_at = None;
    let mut entered_at = None;
    for t in 0..3000 {
        s4_sim::tick(&mut w, &[]);
        let st = military::state(&w, tower).unwrap();
        if ordered_at.is_none() && st.sword_coming == 1 {
            ordered_at = Some(t);
            assert_eq!(st.sword_wish, 1);
        }
        if st.sword_in == 1 {
            entered_at = Some(t);
            break;
        }
    }
    eprintln!("ordered at tick {ordered_at:?}, entered at {entered_at:?}");
    let (Some(_), Some(_)) = (ordered_at, entered_at) else { panic!("the swordsman did not come: {:?}", military::state(&w, tower)) };
    let st = military::state(&w, tower).unwrap().clone();
    assert_eq!((st.sword_in, st.sword_coming, st.sword_wish), (1, 0, 1));
    check_garrison(&w, tower);
    assert!(st.slots.contains(&sword));
    assert!(military::is_inhabited(&w, tower));
    // FirstSettlerEnter killed the first door settler and placed a new one.
    assert_ne!(st.door, first_door);
    assert!(w.entities.by_index(first_door as u32).is_none_or(|e| e.is_dead()));
    let owned_after = (0..w.grid.len()).filter(|&n| w.owner_of(n) == 1).count();
    eprintln!("player 1 nodes: {owned_before} → {owned_after}");
    assert!(owned_after > owned_before, "the new tower brought no territory");

    // Crush (command 5002): the soldier comes out next to the door as a free soldier again.
    s4_sim::buildings::crush::crush_building(&mut w, tower);
    let e = w.entities.by_index(sword as u32).unwrap();
    assert_eq!(job(&w, sword).role, role::SOLDIER);
    assert!(e.flags & flag::ON_BOARD == 0 && e.flags & flag::SELECTABLE != 0 && e.flags & flag::VISIBLE != 0);
    let n = w.grid.index(e.x() as u32, e.y() as u32);
    assert_eq!(w.layers.map_object[n], sword);
    assert!(w.entities.by_index(st.door as u32).is_none_or(|e| e.is_dead()));
    for _ in 0..200 {
        s4_sim::tick(&mut w, &[]);
    }
    let e = w.entities.by_index(sword as u32).unwrap();
    let j = job(&w, sword);
    eprintln!("after crush: dead {} flags {:#x} role {} home {} task {} pos ({}, {})", e.is_dead(), e.flags, j.role, j.home, j.task, e.x(), e.y());
    assert!(!e.is_dead());
    // Free again, or taken by another building of the player that wanted a swordsman (an empty
    // start building orders one, 0x549c50).
    if j.role == role::TOWER_SOLDIER {
        let b = j.home;
        let be = w.entities.by_index(b as u32).unwrap();
        eprintln!("taken by building {b} (type {}, owner {}): {:?}", be.kind, be.owner, military::state(&w, b));
        assert_eq!(be.owner, 1);
        check_garrison(&w, b);
        assert!(military::slots(&w, b).contains(&sword));
    } else {
        assert_eq!(j.role, role::SOLDIER);
        assert_eq!(e.flags & flag::ON_BOARD, 0);
    }
}

#[test]
fn military_runs_are_deterministic() {
    let run = || -> Option<u64> {
        let mut w = roman02()?;
        for _ in 0..1500 {
            s4_sim::tick(&mut w, &[]);
        }
        Some(w.state_hash())
    };
    let Some(a) = run() else { return };
    assert_eq!(Some(a), run());
}

/// `FirstSettlerEnter` (0x5484f0) with a Dark Tribe settler: the settler dies and the empty tower
/// takes 10000 damage (vtbl+0x20 0x5371f0 → role +0x6c 0x549520: 4997 hit points ≥ health), is
/// destroyed (0x536a80: role +0x74 0x549440, `RemoveFromWorld`, killed).
#[test]
fn a_dark_tribe_soldier_destroys_an_empty_tower() {
    let Some(mut w) = roman02() else { return };
    let ids = military_buildings(&w);
    let size = w.grid.size() as i32;
    let sites = possible_sites(&w, 1, SMALL_TOWER, (0, 0), (size, size));
    let &(x, y, _) = sites.iter().max_by_key(|s| (s.1, s.0)).expect("a tower site");
    let tower = s4_sim::create::add_building(&mut w, x as i32, y as i32, 1, SMALL_TOWER).expect("tower");
    assert!(!ids.contains(&tower));
    let door = military::state(&w, tower).unwrap().door;
    let (tx, ty) = (x as i32, y as i32);
    let k = (s4_sim::spiral::first(5)..s4_sim::spiral::last(10))
        .find(|&k| {
            let n = w.grid.index((tx + s4_sim::spiral::dx(k)) as u32, (ty + s4_sim::spiral::dy(k)) as u32);
            w.layers.flags[n] & 0x7F == 0 && w.layers.map_object[n] == 0 && w.tiling.sector_of(n) != 0
        })
        .expect("a free node");
    // A swordsman of player 1 created while player 1 counts as Dark Tribe (entity race 3).
    let race = w.players[1].as_ref().unwrap().race;
    w.players[1].as_mut().unwrap().race = 3;
    let sword = s4_sim::create::add_settler(&mut w, tx + s4_sim::spiral::dx(k), ty + s4_sim::spiral::dy(k), 1, SWORDSMAN_1).expect("swordsman");
    w.players[1].as_mut().unwrap().race = race;
    assert_eq!(w.entities.by_index(sword as u32).unwrap().race, 3);
    let mut gone = None;
    for t in 0..3000 {
        s4_sim::tick(&mut w, &[]);
        if w.entities.by_index(tower as u32).is_none_or(|e| e.is_dead()) {
            gone = Some(t);
            break;
        }
    }
    eprintln!("tower destroyed at tick {gone:?}");
    assert!(gone.is_some(), "the tower still stands: {:?}", military::state(&w, tower));
    assert!(w.entities.by_index(sword as u32).is_none_or(|e| e.is_dead()));
    assert!(w.entities.by_index(door as u32).is_none_or(|e| e.is_dead()));
    let n = w.grid.index(tx as u32, ty as u32);
    assert_eq!(w.layers.map_object[n], 0);
    assert_eq!(w.layers.flags[n] & 0x08, 0, "the footprint was not removed");
}

const BARRACKS: u8 = 24;
const SWORD: u8 = 34;

/// M2: a soldier order (command 5047 → `ChangeSoldierWish` 0x4d68a0) and a barracks
/// (`CBarrackRole` 0x534ac0): the nearest free carrier is ordered (event 26), takes the sword
/// (CHANGE_TYPE_AT_BARRACKS), becomes a swordsman with a `CSoldierRole`, joins the warrior list
/// and walks to the barracks' working area (event 0x11, kind 0xd); the order is used up.
#[test]
fn a_barracks_turns_a_carrier_into_an_ordered_swordsman() {
    let Some(mut w) = roman02() else { return };
    let castle = w
        .entities
        .iter()
        .find(|(id, e)| e.owner == 1 && e.obj_type == ObjType::Building && s4_sim::buildings::have_inhabitant(&w, *id))
        .map(|(_, e)| (e.x(), e.y()))
        .expect("castle");
    let size = w.grid.size() as i32;
    let &(x, y, _) = possible_sites(&w, 1, BARRACKS, (0, 0), (size, size))
        .iter()
        .min_by_key(|s| s4_sim::spiral::grid_distance(s.0 as i32 - castle.0, s.1 as i32 - castle.1))
        .expect("a barracks site");
    let barracks = s4_sim::create::add_building(&mut w, x as i32, y as i32, 1, BARRACKS).expect("barracks");
    let piles = s4_sim::buildings::barrack::state(&w, barracks).expect("barracks role").piles.clone();
    assert!(!piles.is_empty());
    let sword_pile = s4_sim::buildings::barrack::pile_with_good(&w, barracks, SWORD);
    assert_ne!(sword_pile, 0);
    if let Some(EntityData::Pile(p)) = w.entities.by_index_mut(sword_pile as u32).map(|e| &mut e.data) {
        p.amount = 2;
    }
    // The building works (flag 0x1000, set by `CBuilding::Switch`; map buildings start working).
    let works = w.entities.by_index(barracks as u32).unwrap().flags & 0x1000 != 0;
    assert!(works, "a new barracks should work");
    let swordsmen = |w: &World| s4_sim::settler_lists::iter(w, 1, SWORDSMAN_1).collect::<Vec<_>>();
    let before = swordsmen(&w);
    let cmd = s4_sim::PlayerCommand { player: 1, command: s4_sim::Command::SoldierOrder { building: barracks, kind: SWORDSMAN_1, amount: 1 } };
    s4_sim::tick(&mut w, &[cmd]);
    let eco = s4_sim::buildings::storage::eco_of_building(&w, barracks);
    assert_eq!(w.economy.get(eco).unwrap().soldier_orders[0], 1, "the order did not arrive");
    let mut new = None;
    for t in 0..4000 {
        s4_sim::tick(&mut w, &[]);
        if let Some(&s) = swordsmen(&w).iter().find(|s| !before.contains(s)) {
            new = Some((t, s));
            break;
        }
    }
    let Some((t, s)) = new else { panic!("no swordsman was made; order {:?}", w.economy.get(eco).map(|e| e.soldier_orders)) };
    eprintln!("swordsman {s} made at tick {t}");
    assert_eq!(w.economy.get(eco).unwrap().soldier_orders[0], 0);
    assert_eq!(job(&w, s).role, role::SOLDIER);
    assert_eq!(s4_sim::pile::get(&w, sword_pile).unwrap().amount, 1, "the sword was not taken");
    let area = match &w.entities.by_index(barracks as u32).unwrap().data {
        EntityData::Building(d) => d.working_area,
        _ => unreachable!(),
    };
    // It walks to the working area; idle there, walkers and animals may push it about.
    let mut nearest = i32::MAX;
    for _ in 0..600 {
        s4_sim::tick(&mut w, &[]);
        let e = w.entities.by_index(s as u32).unwrap();
        nearest = nearest.min(s4_sim::spiral::grid_distance(e.x() - area.x(), e.y() - area.y()));
    }
    let e = w.entities.by_index(s as u32).unwrap();
    eprintln!("swordsman at ({}, {}), working area ({}, {}), nearest {nearest}", e.x(), e.y(), area.x(), area.y());
    assert!(nearest <= 3, "the new swordsman did not walk to the working area");
    assert_eq!(job(&w, s).task, s4_sim::jobs::soldier::TASK_BEHAVIOUR);
}

/// M2: commands 5032 (`EmptyTower`, building event 0x13) and 5031 (`FillTower`, event 0x12),
/// handled by `CBuilding::LogicUpdate`'s event processing and the role (0x54a560): a lowered wish
/// sends a soldier out (0x549680), filling the tower calls them back (0x549c50).
#[test]
fn tower_commands_change_the_garrison() {
    let Some(mut w) = roman02() else { return };
    // A player-1 tower with bowmen and swordsmen inside.
    let tower = military_buildings(&w)
        .into_iter()
        .find(|&id| w.entities.by_index(id as u32).unwrap().owner == 1 && military::state(&w, id).is_some_and(|s| s.sword_in == 1 && s.bow_in == 2))
        .expect("a tower with 2 bowmen and a swordsman");
    let cmd = |c| s4_sim::PlayerCommand { player: 1, command: c };
    let empty = |kind, amount| cmd(s4_sim::Command::EmptyTower { building: tower, kind, amount });
    s4_sim::tick(&mut w, &[empty(0, 1)]);
    for _ in 0..40 {
        s4_sim::tick(&mut w, &[]);
    }
    let st = military::state(&w, tower).unwrap().clone();
    eprintln!("after EmptyTower(swordsmen): {st:?}");
    // The swordsman wish may drop to 0 because bowmen are wished and present.
    assert_eq!((st.sword_wish, st.sword_in), (0, 0));
    check_garrison(&w, tower);
    s4_sim::tick(&mut w, &[empty(1, 1)]);
    for _ in 0..40 {
        s4_sim::tick(&mut w, &[]);
    }
    let st = military::state(&w, tower).unwrap().clone();
    eprintln!("after EmptyTower(bowmen): {st:?}");
    assert_eq!((st.bow_wish, st.bow_in), (1, 1));
    check_garrison(&w, tower);
    // A foreign player's command is ignored.
    s4_sim::tick(&mut w, &[s4_sim::PlayerCommand { player: 2, command: s4_sim::Command::FillTower { building: tower as u32, value: u32::MAX } }]);
    for _ in 0..20 {
        s4_sim::tick(&mut w, &[]);
    }
    assert_eq!(military::state(&w, tower).unwrap().bow_wish, 1);
    s4_sim::tick(&mut w, &[cmd(s4_sim::Command::FillTower { building: tower as u32, value: u32::MAX })]);
    for _ in 0..800 {
        s4_sim::tick(&mut w, &[]);
    }
    let st = military::state(&w, tower).unwrap().clone();
    eprintln!("after FillTower: {st:?}");
    assert_eq!((st.bow_wish, st.sword_wish), (2, 1));
    assert_eq!((st.bow_in, st.sword_in), (2, 1), "the soldiers did not come back");
    check_garrison(&w, tower);
}

/// Places a finished small tower for `player` at `(x, y)` and a free swordsman of that player
/// 5..10 nodes away, then ticks until the tower is inhabited. Returns the tower.
fn occupied_tower(w: &mut World, player: u8, x: i32, y: i32) -> u16 {
    let tower = s4_sim::create::add_building(w, x, y, player, SMALL_TOWER).expect("tower");
    let k = (s4_sim::spiral::first(5)..s4_sim::spiral::last(10))
        .find(|&k| {
            let n = w.grid.index((x + s4_sim::spiral::dx(k)) as u32, (y + s4_sim::spiral::dy(k)) as u32);
            w.layers.flags[n] & 0x7F == 0 && w.layers.map_object[n] == 0 && w.tiling.sector_of(n) != 0
        })
        .expect("a free node");
    s4_sim::create::add_settler(w, x + s4_sim::spiral::dx(k), y + s4_sim::spiral::dy(k), player, SWORDSMAN_1).expect("swordsman");
    for _ in 0..3000 {
        s4_sim::tick(w, &[]);
        if military::is_inhabited(w, tower) {
            return tower;
        }
    }
    panic!("tower {tower} of player {player} was not occupied: {:?}", military::state(w, tower));
}

/// M3: land that changes hands destroys the buildings on it (`SetOwner` 0x60cd30 →
/// `DestroyBuilding(x, y, 0)` 0x53ce10 → 0x536a80). A player-1 woodcutter's hut B near the border;
/// a player-2 tower C reaches B but cannot take land player 1 still influences; when player 1's
/// military buildings that reach B are crushed, the last one's +0x2c (0x549370) recomputes the
/// territory, C's player takes B's land and B is destroyed.
#[test]
fn losing_land_destroys_the_buildings_on_it() {
    let Some(mut w) = roman02() else { return };
    let castle = w
        .entities
        .iter()
        .find(|(id, e)| e.owner == 1 && e.obj_type == ObjType::Building && s4_sim::buildings::have_inhabitant(&w, *id))
        .map(|(_, e)| (e.x(), e.y()))
        .expect("castle");
    let size = w.grid.size() as i32;
    let d = |a: (i32, i32), b: (i32, i32)| s4_sim::spiral::grid_distance(a.0 - b.0, a.1 - b.1);
    let land = |w: &World, (x, y): (i32, i32)| {
        w.grid.checked_index(x, y).is_some_and(|n| w.layers.ground[n] >= 16 && w.owner_of(n) == 0 && w.layers.flags[n] & 0x0F == 0 && w.layers.map_object[n] == 0)
    };
    // B: a woodcutter's hut site of player 1 with free land 20..26 away (for C), farthest from
    // the castle first.
    let woodcutter = 1u8;
    let mut sites = possible_sites(&w, 1, woodcutter, (0, 0), (size, size));
    sites.sort_by_key(|s| std::cmp::Reverse((d((s.0 as i32, s.1 as i32), castle), s.1, s.0)));
    let (b, c) = sites
        .iter()
        .find_map(|s| {
            let b = (s.0 as i32, s.1 as i32);
            let c = (0..s4_sim::spiral::last(30))
                .map(|k| (b.0 + s4_sim::spiral::dx(k), b.1 + s4_sim::spiral::dy(k)))
                .find(|&p| (20..=26).contains(&d(p, b)) && land(&w, p))?;
            Some((b, c))
        })
        .expect("sites B and C");
    let hut = s4_sim::create::add_building(&mut w, b.0, b.1, 1, woodcutter).expect("hut");
    let tower_c = occupied_tower(&mut w, 2, c.0, c.1);
    let hut_node = w.grid.index(b.0 as u32, b.1 as u32);
    assert_eq!(w.owner_of(hut_node), 1, "player 1 still influences B's land");
    // Player 1's military buildings whose influence spiral reaches B.
    let reaching: Vec<u16> = military_buildings(&w)
        .into_iter()
        .filter(|&id| {
            let e = w.entities.by_index(id as u32).unwrap();
            let (last, _) = s4_sim::territory::influence_info(e.kind);
            e.owner == 1 && military::is_inhabited(&w, id) && (0..=last).any(|k| (e.x() + s4_sim::spiral::dx(k), e.y() + s4_sim::spiral::dy(k)) == b)
        })
        .collect();
    eprintln!("B {b:?} (hut {hut}), C {c:?} (tower {tower_c}), player-1 buildings reaching B: {reaching:?}");
    assert!(!reaching.is_empty());
    for &t in &reaching {
        assert!(!w.entities.by_index(hut as u32).unwrap().is_dead(), "the hut died before the last crush");
        s4_sim::buildings::crush::crush_building(&mut w, t);
    }
    eprintln!("B's node owner after the crushes: {}", w.owner_of(hut_node));
    assert_eq!(w.owner_of(hut_node), 2, "C's player should take B's land");
    assert!(w.entities.by_index(hut as u32).is_none_or(|e| e.is_dead()), "the hut on lost land still stands");
    assert_eq!(w.layers.flags[hut_node] & 0x08, 0);
}

/// M4: the war map (`CWarMap`) stays consistent while soldiers walk, enter towers, leave them and
/// change type: lists, counts and the strength grid recomputed from the listed entities agree.
#[test]
fn the_war_map_stays_consistent() {
    let Some(mut w) = roman02() else { return };
    s4_sim::warmap::check(&w).unwrap();
    let walkers = s4_sim::settler_lists::iter(&w, 1, SWORDSMAN_1).count() + s4_sim::settler_lists::iter(&w, 2, SWORDSMAN_1).count();
    assert!(walkers > 0);
    for t in 0..1500 {
        s4_sim::tick(&mut w, &[]);
        if t % 100 == 0 {
            s4_sim::warmap::check(&w).unwrap_or_else(|e| panic!("tick {t}: {e}"));
        }
    }
    // Every free warrior on the map is listed; soldiers inside buildings are not.
    for (id, e) in w.entities.iter() {
        if e.obj_type != ObjType::Settler || e.is_dead() || e.warrior_type() == 0 {
            continue;
        }
        let inside = e.flags & flag::ON_BOARD != 0;
        let role = job(&w, id).role;
        if role == role::SOLDIER || role == role::DOOR {
            assert_eq!(w.warmap.contains(id), !inside, "settler {id} (role {role}) on board {inside}");
        }
    }
}

fn free_node(w: &World, x: i32, y: i32) -> bool {
    w.grid.checked_index(x, y).is_some_and(|n| w.layers.ground[n] >= 16 && w.layers.flags[n] & 0x7F == 0 && w.layers.map_object[n] == 0 && w.tiling.sector_of(n) != 0)
}

fn health(w: &World, s: u16) -> Option<u8> {
    match &w.entities.by_index(s as u32)?.data {
        EntityData::Settler(d) if !w.entities.by_index(s as u32)?.is_dead() => Some(d.health),
        _ => None,
    }
}

/// M4: two swordsmen of hostile players a few nodes apart. The war map shows each the other's
/// strength in its cell (0x5983f0), the approach search (0x599550) walks them together, the
/// melee search (0x598eb0) finds the neighbour and `Attack` (0x58a750) takes the fight task;
/// each task's end (0x589250) deals `(strength · damage + 127) >> 8` until one dies.
#[test]
fn hostile_swordsmen_fight_until_one_dies() {
    let Some(mut w) = roman02() else { return };
    let (s1, s2, h1, h2) = two_swordsmen(&mut w);
    let mut end = None;
    for t in 0..3000 {
        s4_sim::tick(&mut w, &[]);
        if t % 50 == 0 {
            s4_sim::warmap::check(&w).unwrap_or_else(|e| panic!("tick {t}: {e}"));
        }
        if health(&w, s1).is_none() || health(&w, s2).is_none() {
            end = Some(t);
            break;
        }
    }
    eprintln!("fight over at tick {end:?}: {:?} / {:?}", health(&w, s1), health(&w, s2));
    assert!(end.is_some(), "nobody died: health {:?} / {:?}", health(&w, s1), health(&w, s2));
    let survivor = if health(&w, s1).is_some() { s1 } else { s2 };
    let full = if survivor == s1 { h1 } else { h2 };
    assert!(health(&w, survivor).unwrap() < full, "the survivor was never hit");
    s4_sim::warmap::check(&w).unwrap();
    // The dead one's death effect (0x580bff): its owner's tribe effect 1..=3 with sound 0x29.
    let dead_owner = if survivor == s1 { 2 } else { 1 };
    assert!(
        w.events.iter().any(|e| matches!(*e, s4_sim::event::SimEvent::TribeEffect { effect: 1..=3, sound: 0x29, player, .. } if player == dead_owner)),
        "no death effect"
    );
}

/// Hostile bowmen shoot (`Attack` 0x58a750 → `CEffects` vtbl+0x18): each shot is a flying arrow
/// (kind 0) from the shooter's node to the target's, 15 ticks after the attack starts, with the
/// flight `projectile::frames` gives; the castle's tower bowmen shoot from their slots.
#[test]
fn hostile_bowmen_shoot_arrows() {
    let Some(mut w) = roman02() else { return };
    let (s1, s2, _, _) = two_swordsmen(&mut w);
    // The same places, as bowmen.
    let at = |w: &World, s: u16| w.entities.by_index(s as u32).map(|e| (e.x(), e.y())).unwrap();
    let (a, b) = (at(&w, s1), at(&w, s2));
    s4_sim::create::settler_delete(&mut w, s1);
    s4_sim::create::settler_delete(&mut w, s2);
    s4_sim::tick(&mut w, &[]);
    let b1 = s4_sim::create::add_settler(&mut w, a.0, a.1, 1, BOWMAN_1).expect("bowman 1");
    let b2 = s4_sim::create::add_settler(&mut w, b.0, b.1, 2, BOWMAN_1).expect("bowman 2");
    let (mut shots, mut towers) = (0, 0);
    for _ in 0..1500 {
        w.events.clear();
        s4_sim::tick(&mut w, &[]);
        for e in &w.events {
            if let s4_sim::event::SimEvent::Projectile { kind, from, to, delay, offset, frames } = *e {
                let f = s4_sim::projectile::frames(&w, kind, Y16X16::pack(from.0, from.1), Y16X16::pack(to.0, to.1), offset);
                assert_eq!(frames, f);
                if offset != (0, 0) {
                    // A tower bowman's arrow from its slot (0x58c46f).
                    assert_eq!((kind, delay), (0, 15));
                    towers += 1;
                    continue;
                }
                // Soldiers from the castles join in; the two bowmen move.
                assert_eq!((kind, delay), (0, 15));
                assert_ne!(from, to);
                shots += 1;
            }
        }
        if health(&w, b1).is_none() || health(&w, b2).is_none() {
            break;
        }
    }
    eprintln!("{shots} bowman arrows, {towers} tower arrows");
    assert!(shots > 0, "no arrow flew");
}

/// `CAlliances` (0x4fe510 / 0x5ab700): players 1 and 2 in one team are allies — their swordsmen
/// side by side never fight (enemy masks 0x12dcd54), and the war map counts both in one team.
#[test]
fn allied_swordsmen_do_not_fight() {
    let mut teams = [None; 8];
    teams[0] = Some(0);
    teams[1] = Some(0);
    let Some(mut w) = roman02_with(&s4_sim::NewGame { teams, ..Default::default() }) else { return };
    assert_eq!((w.alliances.team(1), w.alliances.team(2)), (1, 1));
    assert!(w.alliances.allied(1, 2) && !w.alliances.hostile(1, 2));
    let (s1, s2, h1, h2) = two_swordsmen(&mut w);
    for t in 0..1500 {
        s4_sim::tick(&mut w, &[]);
        if t % 100 == 0 {
            s4_sim::warmap::check(&w).unwrap_or_else(|e| panic!("tick {t}: {e}"));
        }
    }
    assert_eq!((health(&w, s1), health(&w, s2)), (Some(h1), Some(h2)), "allies fought");
}

/// Two free swordsmen of players 1 and 2 about 5 nodes apart in one sector, 12..40 from player
/// 1's castle: their ids and health.
fn two_swordsmen(w: &mut World) -> (u16, u16, u8, u8) {
    let castle = w
        .entities
        .iter()
        .find(|(id, e)| e.owner == 1 && e.obj_type == ObjType::Building && s4_sim::buildings::have_inhabitant(w, *id))
        .map(|(_, e)| (e.x(), e.y()))
        .expect("castle");
    let ring = |c: (i32, i32), r: u8| (s4_sim::spiral::first(r)..=s4_sim::spiral::last(r)).map(move |k| (c.0 + s4_sim::spiral::dx(k), c.1 + s4_sim::spiral::dy(k)));
    let (a, b) = (12..40)
        .flat_map(|r| ring(castle, r))
        .filter(|&p| free_node(w, p.0, p.1))
        .find_map(|p| {
            let sector = w.tiling.sector_of(w.grid.index(p.0 as u32, p.1 as u32));
            let q = ring(p, 5).find(|&q| free_node(w, q.0, q.1) && w.tiling.sector_of(w.grid.index(q.0 as u32, q.1 as u32)) == sector)?;
            Some((p, q))
        })
        .expect("two free nodes");
    let s1 = s4_sim::create::add_settler(w, a.0, a.1, 1, SWORDSMAN_1).expect("swordsman 1");
    let s2 = s4_sim::create::add_settler(w, b.0, b.1, 2, SWORDSMAN_1).expect("swordsman 2");
    let (h1, h2) = (health(w, s1).unwrap(), health(w, s2).unwrap());
    eprintln!("{s1} at {a:?} (health {h1}), {s2} at {b:?} (health {h2})");
    (s1, s2, h1, h2)
}

/// M4: soldiers take an enemy tower. Player-1 swordsmen next to a player-2 small tower kill the
/// door settler (`CDoorRole`), the soldier that then guards the door (0x54a120, tower state 3)
/// and, once the building is empty, one of them is sent in and captures it (`FirstSettlerEnter`
/// 0x5484f0: owner change 0x53f050, RemoveFromWorld / SetOwner / SetToWorld).
#[test]
fn swordsmen_capture_an_enemy_tower() {
    let Some(mut w) = roman02() else { return };
    let castle = w
        .entities
        .iter()
        .find(|(id, e)| e.owner == 1 && e.obj_type == ObjType::Building && s4_sim::buildings::have_inhabitant(&w, *id))
        .map(|(_, e)| (e.x(), e.y()))
        .expect("castle");
    let size = w.grid.size() as i32;
    let d = |a: (i32, i32), b: (i32, i32)| s4_sim::spiral::grid_distance(a.0 - b.0, a.1 - b.1);
    let mut sites = possible_sites(&w, 1, SMALL_TOWER, (0, 0), (size, size));
    sites.sort_by_key(|s| (d((s.0 as i32, s.1 as i32), castle), s.1, s.0));
    let &(x, y, _) = sites.iter().find(|s| d((s.0 as i32, s.1 as i32), castle) >= 25).expect("a tower site");
    let tower = occupied_tower(&mut w, 2, x as i32, y as i32);
    let door = military::state(&w, tower).unwrap().door;
    let dpos = w.entities.by_index(door as u32).unwrap().pos;
    let ring = |c: Y16X16, r: u8| (s4_sim::spiral::first(r)..=s4_sim::spiral::last(r)).map(move |k| (c.x() + s4_sim::spiral::dx(k), c.y() + s4_sim::spiral::dy(k)));
    let mut attackers = Vec::new();
    for p in (3..8).flat_map(|r| ring(dpos, r)) {
        if attackers.len() == 3 {
            break;
        }
        if free_node(&w, p.0, p.1) {
            attackers.push(s4_sim::create::add_settler(&mut w, p.0, p.1, 1, SWORDSMAN_1).expect("swordsman"));
        }
    }
    eprintln!("tower {tower} of player 2 at ({x}, {y}), door {door} at {dpos:?}, attackers {attackers:?}");
    let mut captured = None;
    for t in 0..6000 {
        s4_sim::tick(&mut w, &[]);
        if t % 100 == 0 {
            s4_sim::warmap::check(&w).unwrap_or_else(|e| panic!("tick {t}: {e}"));
        }
        if w.entities.by_index(tower as u32).is_some_and(|e| e.owner == 1) && military::is_inhabited(&w, tower) {
            captured = Some(t);
            break;
        }
    }
    let alive: Vec<_> = attackers.iter().map(|&a| health(&w, a)).collect();
    eprintln!("captured at {captured:?}; attackers {alive:?}; state {:?}", military::state(&w, tower));
    assert!(captured.is_some(), "the tower was not captured");
    check_garrison(&w, tower);
    let n = w.grid.index(x as u32, y as u32);
    assert_eq!(w.owner_of(n), 1, "the tower's land changed hands");
    s4_sim::warmap::check(&w).unwrap();
}

/// The right-click move: command 5014 (`CGroupMgr::SendTo` 0x56a800) gives each selected soldier
/// event 0x11 / 0xd with its own place of the formation (0x56a000: the target, then every second
/// spiral node in its sector); the soldiers walk there (0x589810: GO_TO_POS, then WAIT) and become
/// idle there (`TakeJob` WAIT 0x589b30: `SetMode(-1, 0)`, no target position). An idle soldier
/// keeps no place: a later walker may push it aside (only the squad leader walks back to its
/// anchor, 0x58ad30). With land animals on (they draw the game RNG and occupy nodes), the third
/// soldier's path crosses the first one's place and pushes idle ones off by a few nodes (how
/// far depends on the game RNG) — so: every soldier reaches its place, and all end idle within 3
/// of it.
#[test]
fn send_to_moves_selected_soldiers() {
    let Some(mut w) = roman02() else { return };
    let free: Vec<u16> = s4_sim::settler_lists::iter(&w, 1, SWORDSMAN_1)
        .filter(|&s| job(&w, s).role == role::SOLDIER && w.entities.by_index(s as u32).unwrap().flags & flag::ON_BOARD == 0)
        .take(3)
        .collect();
    assert_eq!(free.len(), 3, "three free swordsmen of player 1");
    let start = w.entities.by_index(free[0] as u32).unwrap().pos;
    let sector = |w: &World, x: i32, y: i32| w.tiling.sector_of(w.grid.index(x as u32, y as u32));
    let s0 = sector(&w, start.x(), start.y());
    let target = (s4_sim::spiral::first(12)..=s4_sim::spiral::last(14))
        .map(|k| (start.x() + s4_sim::spiral::dx(k), start.y() + s4_sim::spiral::dy(k)))
        .find(|&(x, y)| free_node(&w, x, y) && sector(&w, x, y) == s0)
        .expect("a target");
    let places = s4_sim::group::formation(&w, target.0, target.1, 3, 1, start.0 as i32).positions;
    eprintln!("soldiers {free:?} from {start:?} to {target:?}, places {places:?}");
    let cmd = s4_sim::PlayerCommand { player: 1, command: s4_sim::Command::SendTo { x: target.0 as u16, y: target.1 as u16, mode: 0, entities: free.clone() } };
    s4_sim::tick(&mut w, &[cmd]);
    let dist = |w: &World, k: usize| {
        let p = w.entities.by_index(free[k] as u32).unwrap().pos;
        s4_sim::spiral::grid_distance(p.x() - places[k].x(), p.y() - places[k].y())
    };
    let mut reached = [false; 3];
    for _ in 0..600 {
        s4_sim::tick(&mut w, &[]);
        for (k, r) in reached.iter_mut().enumerate() {
            *r |= dist(&w, k) == 0;
        }
    }
    for (k, &s) in free.iter().enumerate() {
        let d = dist(&w, k);
        eprintln!("soldier {s} at {:?}, place {:?}, distance {d}, reached {}", w.entities.by_index(s as u32).unwrap().pos, places[k], reached[k]);
        assert!(reached[k], "soldier {s} never reached its place");
        assert!(d <= 3 && job(&w, s).soldier.state == 0, "soldier {s} not idle near its place");
    }
}

fn set_health(w: &mut World, s: u16, h: u8) {
    if let Some(EntityData::Settler(d)) = w.entities.by_index_mut(s as u32).map(|e| &mut e.data) {
        d.health = h;
    }
}

/// A free settler of `player` (race forced to `race` while created) on a free node near `at`.
fn settler_near(w: &mut World, at: (i32, i32), player: u8, race: u8, kind: u16, r: std::ops::Range<u8>) -> u16 {
    let k = (s4_sim::spiral::first(r.start)..s4_sim::spiral::last(r.end))
        .find(|&k| free_node(w, at.0 + s4_sim::spiral::dx(k), at.1 + s4_sim::spiral::dy(k)))
        .expect("a free node");
    let old = w.players[player as usize].as_ref().unwrap().race;
    w.players[player as usize].as_mut().unwrap().race = race;
    let s = s4_sim::create::add_settler(w, at.0 + s4_sim::spiral::dx(k), at.1 + s4_sim::spiral::dy(k), player, kind).expect("settler");
    w.players[player as usize].as_mut().unwrap().race = old;
    s
}

/// Medics (behaviour row 15, 0x5992c0 / 0x599650): a hurt swordsman of the medic's player nearby
/// is approached and healed (task 0x22 → 0x589250: `(strength · info+7 + 127) >> 8`, at least 1).
#[test]
fn a_medic_heals_a_hurt_swordsman() {
    let Some(mut w) = roman02() else { return };
    let free: Vec<u16> = s4_sim::settler_lists::iter(&w, 1, SWORDSMAN_1)
        .filter(|&s| job(&w, s).role == role::SOLDIER && w.entities.by_index(s as u32).unwrap().flags & flag::ON_BOARD == 0)
        .collect();
    let sword = free[0];
    let p = w.entities.by_index(sword as u32).unwrap().pos;
    set_health(&mut w, sword, 10);
    let medic = settler_near(&mut w, (p.x(), p.y()), 1, 0, 35, 3..6);
    eprintln!("swordsman {sword} at {p:?} (health 10), medic {medic}, misc {}", w.rules.settler_misc(0, 35));
    let mut healed = None;
    for t in 0..600 {
        s4_sim::tick(&mut w, &[]);
        if health(&w, sword).is_some_and(|h| h > 10) {
            healed = Some(t);
            break;
        }
    }
    eprintln!("healed at tick {healed:?}: health {:?}", health(&w, sword));
    assert!(healed.is_some(), "the medic never healed");
}

/// Axe warriors (Viking, row 16, 0x5997a0 / 0x5993a0 → 0x597fd0 / 0x597e10): an enemy tower's
/// builder spot is approached and the tower is hit from there.
#[test]
fn axe_warriors_attack_an_enemy_tower() {
    let Some(mut w) = roman02() else { return };
    let size = w.grid.size() as i32;
    let castle = w
        .entities
        .iter()
        .find(|(id, e)| e.owner == 1 && e.obj_type == ObjType::Building && s4_sim::buildings::have_inhabitant(&w, *id))
        .map(|(_, e)| (e.x(), e.y()))
        .expect("castle");
    let d = |a: (i32, i32), b: (i32, i32)| s4_sim::spiral::grid_distance(a.0 - b.0, a.1 - b.1);
    let mut sites = possible_sites(&w, 1, SMALL_TOWER, (0, 0), (size, size));
    sites.sort_by_key(|s| (d((s.0 as i32, s.1 as i32), castle), s.1, s.0));
    let &(x, y, _) = sites.iter().find(|s| d((s.0 as i32, s.1 as i32), castle) >= 25).expect("a tower site");
    let tower = occupied_tower(&mut w, 2, x as i32, y as i32);
    let full = match &w.entities.by_index(tower as u32).unwrap().data {
        EntityData::Building(b) => b.health,
        _ => unreachable!(),
    };
    let axes: Vec<u16> = (0..3).map(|_| settler_near(&mut w, (x as i32, y as i32), 1, 1, 38, 6..9)).collect();
    eprintln!("tower {tower} at ({x}, {y}) health {full}, axe warriors {axes:?}");
    let mut hit = None;
    for t in 0..2000 {
        s4_sim::tick(&mut w, &[]);
        let h = w.entities.by_index(tower as u32).filter(|e| !e.is_dead()).map(|e| match &e.data {
            EntityData::Building(b) => b.health,
            _ => 0,
        });
        if h.is_none_or(|h| h < full) {
            hit = Some((t, h));
            break;
        }
    }
    eprintln!("tower hit: {hit:?}");
    assert!(hit.is_some(), "the axe warriors never hit the tower");
}

/// Saboteurs (`CSaboteurRole`, row 19, 0x5997e0 / 0x5993d0 → 0x597fd0; hit 0x57fd80): an enemy
/// tower's builder spot is approached and the tower is hit from there with the saboteur's own
/// chance and damage (info +3, no fighting strength).
#[test]
fn saboteurs_attack_an_enemy_building() {
    let Some(mut w) = roman02() else { return };
    let size = w.grid.size() as i32;
    let castle = w
        .entities
        .iter()
        .find(|(id, e)| e.owner == 1 && e.obj_type == ObjType::Building && s4_sim::buildings::have_inhabitant(&w, *id))
        .map(|(_, e)| (e.x(), e.y()))
        .expect("castle");
    let d = |a: (i32, i32), b: (i32, i32)| s4_sim::spiral::grid_distance(a.0 - b.0, a.1 - b.1);
    let mut sites = possible_sites(&w, 1, SMALL_TOWER, (0, 0), (size, size));
    sites.sort_by_key(|s| (d((s.0 as i32, s.1 as i32), castle), s.1, s.0));
    let &(x, y, _) = sites.iter().find(|s| d((s.0 as i32, s.1 as i32), castle) >= 25).expect("a tower site");
    let tower = occupied_tower(&mut w, 2, x as i32, y as i32);
    let full = match &w.entities.by_index(tower as u32).unwrap().data {
        EntityData::Building(b) => b.health,
        _ => unreachable!(),
    };
    let sab = settler_near(&mut w, (x as i32, y as i32), 1, 0, 46, 6..9);
    assert_eq!(job(&w, sab).role, role::SABOTEUR);
    eprintln!("tower {tower} at ({x}, {y}) health {full}, saboteur {sab}");
    let mut hit = None;
    for t in 0..2000 {
        s4_sim::tick(&mut w, &[]);
        let h = w.entities.by_index(tower as u32).filter(|e| !e.is_dead()).map(|e| match &e.data {
            EntityData::Building(b) => b.health,
            _ => 0,
        });
        if h.is_none_or(|h| h < full) {
            hit = Some((t, h));
            break;
        }
    }
    eprintln!("tower hit: {hit:?}");
    // The hit is the building's damage law (`max(1, (n − 5) / 2)` for n = 6).
    assert_eq!(hit.map(|h| h.1), Some(Some(full - 1)), "the saboteur never hit the tower");
    let e = w.entities.by_index(sab as u32).unwrap();
    assert_eq!(e.flags & s4_sim::jobs::soldier::FLAG_HIDDEN, 0, "an attacking saboteur shows itself");
}

/// A saboteur on its own land with no enemy warrior within 15 hides (0x5801f0: vulnerable bits
/// cleared, flag 0x10000000), checking again every 15 ticks.
#[test]
fn a_saboteur_hides_on_own_land() {
    let Some(mut w) = roman02() else { return };
    let castle = w
        .entities
        .iter()
        .find(|(id, e)| e.owner == 1 && e.obj_type == ObjType::Building && s4_sim::buildings::have_inhabitant(&w, *id))
        .map(|(_, e)| (e.x(), e.y()))
        .expect("castle");
    let sab = settler_near(&mut w, castle, 1, 0, 46, 6..9);
    let flags = w.entities.by_index(sab as u32).unwrap().flags;
    assert_eq!(flags & flag::VULNERABLE_MASK, flag::VULNERABLE_MASK);
    for _ in 0..5 {
        s4_sim::tick(&mut w, &[]);
    }
    let flags = w.entities.by_index(sab as u32).unwrap().flags;
    assert_eq!(flags & (flag::VULNERABLE_MASK | s4_sim::jobs::soldier::FLAG_HIDDEN), s4_sim::jobs::soldier::FLAG_HIDDEN);
}

/// End to end in a normal game: a barracks near player 1's castle, swords lying on a free pile
/// (map pile role 2) — nothing seeded into the barracks. Order 3 swordsmen (5047): carriers bring
/// the swords to the barracks' input pile (transport), the barracks turns carriers into
/// swordsmen (0x534ac0) until the order is used up.
#[test]
fn a_barracks_trains_ordered_swordsmen_from_delivered_swords() {
    let Some(mut w) = roman02() else { return };
    let castle = w
        .entities
        .iter()
        .find(|(id, e)| e.owner == 1 && e.obj_type == ObjType::Building && s4_sim::buildings::have_inhabitant(&w, *id))
        .map(|(_, e)| (e.x(), e.y()))
        .expect("castle");
    let size = w.grid.size() as i32;
    let &(x, y, _) = possible_sites(&w, 1, BARRACKS, (0, 0), (size, size))
        .iter()
        .min_by_key(|s| s4_sim::spiral::grid_distance(s.0 as i32 - castle.0, s.1 as i32 - castle.1))
        .expect("a barracks site");
    let barracks = s4_sim::create::add_building(&mut w, x as i32, y as i32, 1, BARRACKS).expect("barracks");
    let sword_pile = s4_sim::buildings::barrack::pile_with_good(&w, barracks, SWORD);
    assert_eq!(s4_sim::pile::get(&w, sword_pile).unwrap().amount, 0);
    let eco = s4_sim::buildings::storage::eco_of_building(&w, barracks);
    let swords_before = w.economy.get(eco).unwrap().goods_on_piles[SWORD as usize];
    // Four swords on a free node 4..8 from the castle, in the barracks' eco-sector.
    let spot = (s4_sim::spiral::first(4)..s4_sim::spiral::last(8))
        .map(|k| (castle.0 + s4_sim::spiral::dx(k), castle.1 + s4_sim::spiral::dy(k)))
        .find(|&(px, py)| free_node(&w, px, py) && s4_sim::eco::eco_of(&w, w.grid.index(px as u32, py as u32)) == eco)
        .expect("a free node");
    s4_sim::pile::add_pile(&mut w, spot.0, spot.1, SWORD, 4, 2).expect("sword pile");
    eprintln!("barracks {barracks} at ({x}, {y}), swords at {spot:?}, swords in the eco-sector before {swords_before}");
    let swordsmen = |w: &World| s4_sim::settler_lists::iter(w, 1, SWORDSMAN_1).collect::<Vec<_>>();
    let before = swordsmen(&w);
    let cmd = s4_sim::PlayerCommand { player: 1, command: s4_sim::Command::SoldierOrder { building: barracks, kind: SWORDSMAN_1, amount: 3 } };
    s4_sim::tick(&mut w, &[cmd]);
    let mut made = Vec::new();
    for t in 0..8000 {
        s4_sim::tick(&mut w, &[]);
        for s in swordsmen(&w) {
            if !before.contains(&s) && !made.iter().any(|&(_, m)| m == s) {
                made.push((t, s));
            }
        }
        if made.len() >= 3 {
            break;
        }
    }
    eprintln!("swordsmen made {made:?}; order {:?}; barracks sword pile {:?}", w.economy.get(eco).map(|e| e.soldier_orders), s4_sim::pile::get(&w, sword_pile).map(|p| p.amount));
    assert_eq!(made.len(), 3, "the barracks did not train three swordsmen");
    assert_eq!(w.economy.get(eco).unwrap().soldier_orders[0], 0, "the order was not used up");
    for &(_, s) in &made {
        assert_eq!(job(&w, s).role, role::SOLDIER);
    }
}

/// Team setup 0 of User/Mousetrap (chunk 3: player 1 team 0, players 2..5 team 1) → players 2..5
/// are one alliance against player 1 (0x5069b0 → 0x4fe510 / 0x5ab700).
#[test]
fn map_team_setup_makes_alliances() {
    let p = s4_formats::game_dir().join("Map").join("User").join("Mousetrap.map");
    if !p.exists() {
        return;
    }
    let map = s4_formats::map::MapFile::open(&p).unwrap();
    let teams = s4_sim::NewGame::map_teams(&map, 0).unwrap();
    assert_eq!(&teams[..5], &[Some(0), Some(1), Some(1), Some(1), Some(1)]);
    let (w, _) = s4_sim::new_game(&map, Rules::load_default().unwrap(), &s4_sim::NewGame { teams, ..Default::default() }).unwrap();
    assert_eq!((1..=5).map(|p| w.alliances.team(p)).collect::<Vec<_>>(), [1, 2, 2, 2, 2]);
    assert!(w.alliances.hostile(1, 3) && w.alliances.allied(2, 5) && !w.alliances.hostile(4, 2));
}

/// Diagnostic (ignored): `N` player-1 soldiers of `KIND` next to a player-2 small tower held by one
/// swordsman; prints every change of health and every death until the tower falls or the
/// attackers are gone. `FIGHT_N` / `FIGHT_KIND` / `FIGHT_DEF` (extra defenders) from the
/// environment.
#[test]
#[ignore]
fn fight_report() {
    let Some(mut w) = roman02() else { return };
    let env = |k: &str, d: u32| std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d);
    let (n, kind, def) = (env("FIGHT_N", 3) as usize, env("FIGHT_KIND", SWORDSMAN_1 as u32) as u16, env("FIGHT_DEF", 0));
    let castle = w
        .entities
        .iter()
        .find(|(id, e)| e.owner == 1 && e.obj_type == ObjType::Building && s4_sim::buildings::have_inhabitant(&w, *id))
        .map(|(_, e)| (e.x(), e.y()))
        .expect("castle");
    let size = w.grid.size() as i32;
    let d = |a: (i32, i32), b: (i32, i32)| s4_sim::spiral::grid_distance(a.0 - b.0, a.1 - b.1);
    let mut sites = possible_sites(&w, 1, SMALL_TOWER, (0, 0), (size, size));
    sites.sort_by_key(|s| (d((s.0 as i32, s.1 as i32), castle), s.1, s.0));
    let &(x, y, _) = sites.iter().find(|s| d((s.0 as i32, s.1 as i32), castle) >= 25).expect("a tower site");
    let tower = occupied_tower(&mut w, 2, x as i32, y as i32);
    for _ in 0..def {
        let k = (s4_sim::spiral::first(5)..s4_sim::spiral::last(10)).find(|&k| free_node(&w, x as i32 + s4_sim::spiral::dx(k), y as i32 + s4_sim::spiral::dy(k))).unwrap();
        s4_sim::create::add_settler(&mut w, x as i32 + s4_sim::spiral::dx(k), y as i32 + s4_sim::spiral::dy(k), 2, SWORDSMAN_1).unwrap();
    }
    for _ in 0..600 {
        s4_sim::tick(&mut w, &[]);
    }
    let door = military::state(&w, tower).unwrap().door;
    let dpos = w.entities.by_index(door as u32).unwrap().pos;
    let ring = |c: Y16X16, r: u8| (s4_sim::spiral::first(r)..=s4_sim::spiral::last(r)).map(move |k| (c.x() + s4_sim::spiral::dx(k), c.y() + s4_sim::spiral::dy(k)));
    let mut attackers = Vec::new();
    for p in (3..12).flat_map(|r| ring(dpos, r)) {
        if attackers.len() == n {
            break;
        }
        if free_node(&w, p.0, p.1) {
            attackers.push(s4_sim::create::add_settler(&mut w, p.0, p.1, 1, kind).expect("attacker"));
        }
    }
    let st = &w.statistic.players;
    eprintln!("tower {tower} state {:?}; strengths p1 own {} foreign {}, p2 own {} foreign {}", military::state(&w, tower), st[1].own, st[1].foreign, st[2].own, st[2].foreign);
    let mut hp: std::collections::BTreeMap<u16, (u8, u8, u16)> = std::collections::BTreeMap::new();
    for t in 0..20000 {
        s4_sim::tick(&mut w, &[]);
        for (id, e) in w.entities.iter() {
            if let EntityData::Settler(s) = &e.data
                && (e.flags & 0xf != 0 || e.kind == 69)
            {
                let h = if e.is_dead() { 0 } else { s.health };
                let prev = hp.insert(id, (h, e.owner, e.kind));
                if let Some((ph, _, _)) = prev
                    && ph != h
                {
                    eprintln!("t{t} p{} k{} #{id} role {} health {ph} -> {h}", e.owner, e.kind, s.job.role);
                }
            }
        }
        if w.entities.by_index(tower as u32).is_some_and(|e| e.owner == 1) {
            eprintln!("t{t}: captured");
            break;
        }
        if attackers.iter().all(|&a| health(&w, a).is_none()) {
            eprintln!("t{t}: attackers gone; tower {:?}", military::state(&w, tower));
            break;
        }
    }
}

/// Playtest "towerbug": a soldier on his way to a tower (attached: home = the tower, ATTACHED)
/// whose tower is taken while he walks. The capture (0x54a120 → 0x54a38d) and the cancelled
/// orders (0x54aaf0) send event 9 with `NotifyAndDetachAll` (0x537430): every observer's target
/// goes to 0 (0x56e130), so he is free and no longer ATTACHED. Kept attached, he could never
/// take an empty enemy tower himself (0x54a26c wants SELECTABLE without ATTACHED) and hit its
/// door settler for ever.
#[test]
fn a_taken_tower_detaches_the_soldiers_on_their_way() {
    let Some(mut w) = roman02() else { return };
    let castle = w
        .entities
        .iter()
        .find(|(id, e)| e.owner == 1 && e.obj_type == ObjType::Building && s4_sim::buildings::have_inhabitant(&w, *id))
        .map(|(_, e)| (e.x(), e.y()))
        .expect("castle");
    let size = w.grid.size() as i32;
    let d = |a: (i32, i32), b: (i32, i32)| s4_sim::spiral::grid_distance(a.0 - b.0, a.1 - b.1);
    let mut sites = possible_sites(&w, 1, SMALL_TOWER, (0, 0), (size, size));
    sites.sort_by_key(|s| (d((s.0 as i32, s.1 as i32), castle), s.1, s.0));
    let &(x, y, _) = sites.iter().find(|s| d((s.0 as i32, s.1 as i32), castle) >= 25).expect("a tower site");
    // A player-2 tower without soldiers (its door settler stands), and a player-2 swordsman far
    // away that it orders.
    let tower = s4_sim::create::add_building(&mut w, x as i32, y as i32, 2, SMALL_TOWER).expect("tower");
    let dpos = w.entities.by_index(military::state(&w, tower).unwrap().door as u32).unwrap().pos;
    let sector = w.tiling.sector_of(w.grid.index(dpos.x() as u32, dpos.y() as u32));
    let far = (s4_sim::spiral::first(30)..s4_sim::spiral::last(40))
        .map(|k| (x as i32 + s4_sim::spiral::dx(k), y as i32 + s4_sim::spiral::dy(k)))
        .find(|&(a, b)| free_node(&w, a, b) && d((a, b), castle) > 45 && w.tiling.sector_of(w.grid.index(a as u32, b as u32)) == sector)
        .expect("a far node");
    let walker = s4_sim::create::add_settler(&mut w, far.0, far.1, 2, SWORDSMAN_1).expect("walker");
    let mut ordered = false;
    for _ in 0..200 {
        s4_sim::tick(&mut w, &[]);
        if w.entities.by_index(walker as u32).is_some_and(|e| e.flags & flag::ATTACHED != 0) {
            ordered = true;
            break;
        }
    }
    assert!(ordered, "the tower ordered the walker");
    assert_eq!(job(&w, walker).home, tower);
    // Player-1 swordsmen at the door kill the door settler; nobody is inside, so one of them
    // takes the tower.
    let ring = |c: Y16X16, r: u8| (s4_sim::spiral::first(r)..=s4_sim::spiral::last(r)).map(move |k| (c.x() + s4_sim::spiral::dx(k), c.y() + s4_sim::spiral::dy(k)));
    let mut attackers = 0;
    for p in (2..6).flat_map(|r| ring(dpos, r)) {
        if attackers == 2 {
            break;
        }
        if free_node(&w, p.0, p.1) {
            s4_sim::create::add_settler(&mut w, p.0, p.1, 1, SWORDSMAN_1).expect("attacker");
            attackers += 1;
        }
    }
    let mut taken = None;
    for t in 0..3000 {
        s4_sim::tick(&mut w, &[]);
        if w.entities.by_index(walker as u32).is_some_and(|e| e.flags & flag::ATTACHED == 0) {
            taken = Some(t);
            break;
        }
    }
    let e = w.entities.by_index(walker as u32).expect("walker");
    assert!(!e.is_dead(), "the walker died");
    eprintln!("walker detached at {taken:?}: flags {:#x}, home {}, tower {:?}", e.flags, job(&w, walker).home, military::state(&w, tower));
    assert!(taken.is_some(), "the walker stayed attached to the taken tower");
    assert_eq!(job(&w, walker).home, 0, "the walker's home is cleared");
}

/// Playtest P1: the bowmen in a tower shoot (`CTowerSoldier` state 2, 0x58c9d0: an enemy warrior
/// within 20 on the tower team's or nobody's land; the arrow from the slot, 0x58c46f; the WORK
/// task's end deals `(info damage + TowerBowmanExtraDmg) · own strength`). Enemy swordsmen 15 to
/// 18 nodes from a castle's door on its land — out of their own approach radius (13), so nothing
/// else fights them — are shot and die.
#[test]
fn tower_bowmen_shoot_enemies_dead() {
    let Some(mut w) = roman02() else { return };
    let (castle, door, owner) = w
        .buildings
        .roles
        .iter()
        .filter_map(|(&id, _)| military::state(&w, id).filter(|s| s.bow_in > 0).map(|s| (id, s.door, w.entities.by_index(id as u32).unwrap().owner)))
        .min()
        .expect("a building with bowmen");
    let enemy = 3 - owner;
    let dpos = w.entities.by_index(door as u32).unwrap().pos;
    let ring = |c: Y16X16, r: u8| (s4_sim::spiral::first(r)..=s4_sim::spiral::last(r)).map(move |k| (c.x() + s4_sim::spiral::dx(k), c.y() + s4_sim::spiral::dy(k)));
    let mut targets = Vec::new();
    for p in (15..19).flat_map(|r| ring(dpos, r)) {
        if targets.len() == 3 {
            break;
        }
        if free_node(&w, p.0, p.1) && w.owner_of(w.grid.index(p.0 as u32, p.1 as u32)) == owner && targets.iter().all(|&(_, q): &(u16, (i32, i32))| (q.0 - p.0).abs() + (q.1 - p.1).abs() > 3) {
            targets.push((s4_sim::create::add_settler(&mut w, p.0, p.1, enemy, SWORDSMAN_1).expect("target"), p));
        }
    }
    assert_eq!(targets.len(), 3, "three places");
    let full: Vec<u8> = targets.iter().map(|&(s, _)| health(&w, s).unwrap()).collect();
    let (mut arrows, mut dead) = (0, 0);
    for _ in 0..4000 {
        w.events.clear();
        s4_sim::tick(&mut w, &[]);
        arrows += w.events.iter().filter(|e| matches!(e, s4_sim::event::SimEvent::Projectile { offset, .. } if *offset != (0, 0))).count();
        dead = targets.iter().filter(|&&(s, _)| health(&w, s).is_none()).count();
        if dead == targets.len() {
            break;
        }
    }
    let hp: Vec<Option<u8>> = targets.iter().map(|&(s, _)| health(&w, s)).collect();
    eprintln!("castle {castle}: {arrows} tower arrows, targets {targets:?} health {full:?} -> {hp:?}");
    assert!(arrows > 0, "no tower arrow");
    assert_eq!(dead, targets.len(), "every target shot dead: {hp:?}");
}

/// Playtest P5: a player-2 construction site on the land of a player-2 tower that player 1 takes
/// is destroyed (`FirstSettlerEnter` 0x5484f0 → `SetOwner` 0x60cd30: a node with flag 0x08 that
/// changes hands → `DestroyBuilding(x, y, 0)` 0x53ce10, which finds the site through the
/// deco-object / map-object layer). A mine site where the tower's land reaches a mountain,
/// otherwise a woodcutter's.
#[test]
fn a_taken_tower_destroys_the_enemy_sites_on_its_land() {
    let Some(mut w) = roman02() else { return };
    let castle = w
        .entities
        .iter()
        .find(|(id, e)| e.owner == 1 && e.obj_type == ObjType::Building && s4_sim::buildings::have_inhabitant(&w, *id))
        .map(|(_, e)| (e.x(), e.y()))
        .expect("castle");
    let size = w.grid.size() as i32;
    let d = |a: (i32, i32), b: (i32, i32)| s4_sim::spiral::grid_distance(a.0 - b.0, a.1 - b.1);
    let mut sites = possible_sites(&w, 1, SMALL_TOWER, (0, 0), (size, size));
    sites.sort_by_key(|s| (d((s.0 as i32, s.1 as i32), castle), s.1, s.0));
    let &(x, y, _) = sites.iter().find(|s| d((s.0 as i32, s.1 as i32), castle) >= 40 && possible_sites(&w, 1, 1, (s.0 as i32 - 10, s.1 as i32 - 10), (s.0 as i32 + 10, s.1 as i32 + 10)).is_empty()).expect("a tower site");
    let tower = occupied_tower(&mut w, 2, x as i32, y as i32);
    // A player-2 site on the tower's new land.
    let (lo, hi) = ((x as i32 - 12, y as i32 - 12), (x as i32 + 12, y as i32 + 12));
    let mut site = None;
    for t in [16u8, 17, 18, 19, 20, 21, 1] {
        let c = possible_sites(&w, 2, t, lo, hi).into_iter().find(|s| d((s.0 as i32, s.1 as i32), (x as i32, y as i32)) >= 4);
        if let Some((sx, sy, _)) = c
            && let Some(id) = s4_sim::build::site::add_building_site(&mut w, sx as i32, sy as i32, 2, t)
        {
            site = Some((id, t, sx, sy));
            break;
        }
    }
    let owned = s4_sim::spiral::walk(w.grid, x as i32, y as i32, 12).filter(|&(a, b)| w.owner_of(w.grid.index(a as u32, b as u32)) == 2).count();
    let (site, t, sx, sy) = site.unwrap_or_else(|| panic!("a player-2 site near the tower ({x}, {y}); player-2 nodes within 12: {owned}"));
    let door = military::state(&w, tower).unwrap().door;
    let dpos = w.entities.by_index(door as u32).unwrap().pos;
    let ring = |c: Y16X16, r: u8| (s4_sim::spiral::first(r)..=s4_sim::spiral::last(r)).map(move |k| (c.x() + s4_sim::spiral::dx(k), c.y() + s4_sim::spiral::dy(k)));
    let mut n = 0;
    for p in (3..8).flat_map(|r| ring(dpos, r)) {
        if n == 3 {
            break;
        }
        if free_node(&w, p.0, p.1) {
            s4_sim::create::add_settler(&mut w, p.0, p.1, 1, SWORDSMAN_1).expect("swordsman");
            n += 1;
        }
    }
    let uid = w.entities.by_index(site as u32).unwrap().uid;
    let mut captured = false;
    for _ in 0..6000 {
        s4_sim::tick(&mut w, &[]);
        if w.entities.by_index(tower as u32).is_some_and(|e| e.owner == 1) && military::is_inhabited(&w, tower) {
            captured = true;
            break;
        }
    }
    assert!(captured, "tower taken");
    let node = w.grid.index(sx as u32, sy as u32);
    let alive = w.entities.by_index(site as u32).is_some_and(|e| e.uid == uid && !e.is_dead());
    eprintln!("site {site} type {t} at ({sx}, {sy}): land now p{}, alive {alive}", w.owner_of(node));
    assert_eq!(w.owner_of(node), 1, "the site's land changed hands");
    assert!(!alive, "the enemy site on the taken land still stands");
}

/// A3, the forced move (Alt): SendTo's mode bit 2 (`(keys >> 1 & 4 | keys & 0x10) >> 1 | keys &
/// 4) >> 1`, 0x4ec960) reaches the soldier as event 0x11's walk flags; 0x589810 makes it task 8
/// instead of GO_TO_POS 7, whose `TakeJob` (0x589b30) sets the behaviour mode 0x200000 (state 4:
/// walk, no search, 0x598610) instead of 0x100000 (state 1: walk and attack whatever comes into
/// range). Two hostile swordsmen in a fight, each sent away from the other: forced, the pending
/// hit is dropped (0x589921: +0x50 = 0 before `TakeJob`) and player 1's walks off untouched;
/// without the bit they fight on.
#[test]
fn a_forced_move_walks_away_from_a_fight() {
    for (mode, forced) in [(4u32, true), (0, false)] {
        let Some(mut w) = roman02() else { return };
        let (s1, s2, h1, h2) = two_swordsmen(&mut w);
        let pos = |w: &World, s: u16| w.entities.by_index(s as u32).map(|e| (e.x(), e.y())).unwrap();
        let (a, b) = (pos(&w, s1), pos(&w, s2));
        // Each one's target: 15 nodes on, straight away from the other, in its sector.
        let away = |w: &World, from: (i32, i32), other: (i32, i32)| {
            let sector = w.tiling.sector_of(w.grid.index(from.0 as u32, from.1 as u32));
            (s4_sim::spiral::first(12)..=s4_sim::spiral::last(16))
                .map(|k| (from.0 + s4_sim::spiral::dx(k), from.1 + s4_sim::spiral::dy(k)))
                .filter(|&(x, y)| free_node(w, x, y) && w.tiling.sector_of(w.grid.index(x as u32, y as u32)) == sector)
                .max_by_key(|&(x, y)| s4_sim::spiral::grid_distance(x - other.0, y - other.1))
                .expect("a place away")
        };
        let (ta, tb) = (away(&w, a, b), away(&w, b, a));
        // Let them see each other first (the search runs every 30 ticks).
        for _ in 0..31 {
            s4_sim::tick(&mut w, &[]);
        }
        let send = |s: u16, p: (i32, i32), player: u8| s4_sim::PlayerCommand { player, command: s4_sim::Command::SendTo { x: p.0 as u16, y: p.1 as u16, mode, entities: vec![s] } };
        s4_sim::tick(&mut w, &[send(s1, ta, 1), send(s2, tb, 2)]);
        let mut closest = i32::MAX;
        for t in 0..1500 {
            if std::env::var_os("A3_DEBUG").is_some() && t < 120 && t % 5 == 0 {
                for s in [s1, s2] {
                    if let Some(e) = w.entities.by_index(s as u32) {
                        let j = job(&w, s);
                        eprintln!("t{t} s{s} ({},{}) flags {:#x} hp {:?} task {} st {} tgt {} fight {:?} events {}", e.x(), e.y(), e.flags, health(&w, s), j.task, j.soldier.state, j.soldier.target, j.soldier.fight_task.map(|f| f.task), j.events.len());
                    }
                }
            }
            s4_sim::tick(&mut w, &[]);
            if health(&w, s1).is_none() {
                break;
            }
            if health(&w, s2).is_some() {
                let (p, q) = (pos(&w, s1), pos(&w, s2));
                closest = closest.min(s4_sim::spiral::grid_distance(p.0 - q.0, p.1 - q.1));
            }
        }
        let (e1, e2) = (health(&w, s1), health(&w, s2));
        eprintln!("mode {mode}: health {h1}/{h2} -> {e1:?}/{e2:?}, closest {closest}, at {:?} / {:?}, targets {ta:?} / {tb:?}", e1.map(|_| pos(&w, s1)), e2.map(|_| pos(&w, s2)));
        // Player 2's swordsman stands near player 1's castle, whose tower bowmen shoot him either
        // way: only player 1's is checked.
        if forced {
            assert_eq!(e1, Some(h1), "the forced walker was hit");
            let p = pos(&w, s1);
            assert!(s4_sim::spiral::grid_distance(p.0 - ta.0, p.1 - ta.1) <= 3, "soldier {s1} at {p:?}, not at {ta:?}");
        } else {
            assert_ne!(e1, Some(h1), "without the bit player 1's swordsman stays in the fight");
        }
    }
}
