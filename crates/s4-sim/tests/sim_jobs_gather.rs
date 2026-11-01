//! The gather buildings end to end on a real map: a hut (`CGatherBuildingRole`) with its worker
//! (`CFreeWorkerRole`) next to roman02's resources. The worker checks in; the hut searches its
//! working area and sends the worker out. A woodcutter chops a tree down (`CTree` stages 3 → 10)
//! and carries the log home; a stonecutter hacks one stone off a heap (`CStone::Take`) per trip.
//! Skipped when the game data is not present.

use s4_sim::build::possible_sites;
use s4_sim::buildings::{Role, RoleKind, gather};
use s4_sim::entity::EntityData;

use s4_sim::World;

const WOODCUTTERHUT: u8 = 1;
const WOODCUTTER: u16 = 4;
const STONECUTTERHUT: u8 = 4;
const STONECUTTER: u16 = 5;

fn roman02() -> Option<World> {
    roman02_as(None)
}

/// roman02 with player 1 of race `race` (the map's when `None`).
fn roman02_as(race: Option<u8>) -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map");
    if !p.exists() {
        return None;
    }
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    let rules = s4_sim::Rules::load_default().ok()?;
    let mut setup = s4_sim::NewGame::default();
    setup.races[0] = race;
    let (mut w, _) = s4_sim::new_game(&map, rules, &setup).ok()?;
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

/// Units of resource `kind` (0x70 trees: 1 each; 0x60 stones: the heap's amount) around (x, y).
fn resource_near(w: &World, x: i32, y: i32, r: i32, kind: u8) -> usize {
    let mut n = 0;
    for dy in -r..=r {
        for dx in -r..=r {
            if let Some(i) = w.grid.checked_index(x + dx, y + dy)
                && w.layers.resource_type(i) == kind
            {
                n += (w.layers.resource[i] & 0xF) as usize;
            }
        }
    }
    n
}

fn gather_state(w: &World, id: u16) -> &gather::State {
    match &w.buildings.roles[&id] {
        Role { kind: RoleKind::Gather(s), .. } => s,
        _ => panic!("not a gather role"),
    }
}

struct Run {
    hash: u64,
    trace: Vec<(u32, i16, u16, u8)>,
    /// Goods on the hut's pile at the end.
    goods: u8,
    /// Resource units gone from the working area.
    used: usize,
}

fn run(hut_type: u8, worker: u16, resource: u8) -> Option<Run> {
    let mut w = roman02()?;
    let size = w.grid.size() as i32;
    let sites = possible_sites(&w, 1, hut_type, (0, 0), (size, size));
    let &(x, y, _) = sites.iter().max_by_key(|s| resource_near(&w, s.0 as i32, s.1 as i32, 8, resource))?;
    let (x, y) = (x as i32, y as i32);
    let before = resource_near(&w, x, y, 20, resource);
    let hut = s4_sim::create::add_building(&mut w, x, y, 1, hut_type).ok()?;
    // A worker next to the hut; the hut orders it (`OrderInhabitant` → `CEcoSector::OrderWorker`).
    let door = match &w.entities.by_index(hut as u32)?.data {
        EntityData::Building(d) => d.door,
        _ => return None,
    };
    let s = s4_sim::create::add_settler(&mut w, door.x() + 2, door.y() + 2, 1, worker)?;
    eprintln!("hut {hut} at ({x},{y}), {before} resource units within 20, worker {s}");
    let mut trace = Vec::new();
    let mut last = (i16::MIN, u16::MAX, u8::MAX);
    let pile = gather_state(&w, hut).pile;
    let amount = |w: &World| s4_sim::pile::get(w, pile).map_or(0, |p| p.amount);
    // Goods put on the pile (carriers may take them away again: count arrivals).
    let (mut goods, mut on_pile) = (0u8, amount(&w));
    for _ in 0..6000 {
        s4_sim::tick(&mut w, &[]);
        let a = amount(&w);
        goods += a.saturating_sub(on_pile);
        on_pile = a;
        let j = match &w.entities.by_index(s as u32)?.data {
            EntityData::Settler(st) => &st.job,
            _ => return None,
        };
        let now = (j.list_id, j.cursor, j.task);
        if now != last {
            trace.push((w.tick, now.0, now.1, now.2));
            last = now;
        }
    }
    let used = before - resource_near(&w, x, y, 20, resource);
    Some(Run { hash: w.state_hash(), trace, goods, used })
}

#[test]
fn woodcutter_fells_trees_and_stocks_logs() {
    let Some(r) = run(WOODCUTTERHUT, WOODCUTTER, 0x70) else { return };
    for t in r.trace.iter().take(20) {
        eprintln!("{t:?}");
    }
    eprintln!("{} logs on the pile, {} trees felled", r.goods, r.used);
    assert!(r.used >= 1, "no tree was cut");
    assert_eq!(r.goods as usize, r.used, "every felled tree is a log on the pile");
    let r2 = run(WOODCUTTERHUT, WOODCUTTER, 0x70).unwrap();
    assert_eq!((r.hash, r.trace.len()), (r2.hash, r2.trace.len()), "two runs differ");
}

#[test]
fn stonecutter_hacks_stones_and_stocks_them() {
    let Some(r) = run(STONECUTTERHUT, STONECUTTER, 0x60) else { return };
    for t in r.trace.iter().take(20) {
        eprintln!("{t:?}");
    }
    eprintln!("{} stones on the pile, {} stones taken", r.goods, r.used);
    assert!(r.used >= 1, "no stone was taken");
    // The last trip's stone may still be on the way.
    assert!(r.goods as usize == r.used || r.goods as usize + 1 == r.used, "stones taken vs on the pile");
    let r2 = run(STONECUTTERHUT, STONECUTTER, 0x60).unwrap();
    assert_eq!((r.hash, r.trace.len()), (r2.hash, r2.trace.len()), "two runs differ");
}

/// The forester (`CSimpleBuildingRole` + `CFreeWorkerRole` PLANT): every trip plants a tree
/// (`SearchTreeSeedPos`, `GetObjectType`), a dynamic `CTree` at stage 0 that grows a stage per
/// 40 updates of 31 ticks and, grown, becomes a static instance with resource 0x70 | 1.
#[test]
fn forester_plants_trees_that_grow() {
    forester(None);
}

/// The Maya's forester searches with type 7 (0x54e260: green land or ground 0x4?).
#[test]
fn maya_forester_plants_trees_that_grow() {
    forester(Some(2));
}

fn forester(race: Option<u8>) {
    const FORESTERHUT: u8 = 2;
    const FORESTER: u16 = 6;
    let run = || -> Option<(u64, usize, usize, usize)> {
        let mut w = roman02_as(race)?;
        let size = w.grid.size() as i32;
        let sites = possible_sites(&w, 1, FORESTERHUT, (0, 0), (size, size));
        // Open land: the site with the fewest trees around.
        let &(x, y, _) = sites.iter().min_by_key(|s| resource_near(&w, s.0 as i32, s.1 as i32, 8, 0x70))?;
        let (x, y) = (x as i32, y as i32);
        let before = resource_near(&w, x, y, 20, 0x70);
        let hut = s4_sim::create::add_building(&mut w, x, y, 1, FORESTERHUT).ok()?;
        let door = match &w.entities.by_index(hut as u32)?.data {
            EntityData::Building(d) => d.door,
            _ => return None,
        };
        s4_sim::create::add_settler(&mut w, door.x() + 2, door.y() + 2, 1, FORESTER)?;
        let growing = |w: &World| {
            w.entities
                .iter()
                .filter(|(_, e)| matches!(&e.data, EntityData::Deco(d) if !d.static_instance && d.stage < 3 && (1..=22).contains(&e.kind)))
                .count()
        };
        let mut planted_max = 0;
        for _ in 0..8000 {
            s4_sim::tick(&mut w, &[]);
            planted_max = planted_max.max(growing(&w));
        }
        let grown = resource_near(&w, x, y, 20, 0x70) - before;
        eprintln!("forester at ({x},{y}): {before} trees before, {grown} grown since, {} growing now, max {planted_max} growing", growing(&w));
        Some((w.state_hash(), grown, growing(&w), planted_max))
    };
    let Some(r) = run() else { return };
    assert!(r.3 >= 1, "nothing was planted");
    assert!(r.1 >= 1, "no planted tree grew up in 8000 ticks");
    assert_eq!(run().unwrap(), r, "two runs differ");
}

/// The sawmill (`CWorkUpBuildingRole` + `CHouseWorkerRole`): logs on its delivery pile become
/// boards on its production pile, one per work cycle ordered with event 3.
#[test]
fn sawmill_turns_logs_into_boards() {
    const SAWMILL: u8 = 3;
    const SAWMILLWORKER: u16 = 12;
    const LOG: u8 = 22;
    let run = || -> Option<(u64, u8, u8)> {
        let mut w = roman02()?;
        let size = w.grid.size() as i32;
        let sites = possible_sites(&w, 1, SAWMILL, (0, 0), (size, size));
        let &(x, y, _) = sites.first()?;
        let mill = s4_sim::create::add_building(&mut w, x as i32, y as i32, 1, SAWMILL).ok()?;
        let (product, delivery) = match &w.buildings.roles[&mill] {
            Role { kind: RoleKind::WorkUp(s), .. } => (s.product, s.delivery.clone()),
            _ => panic!("the sawmill is not a work-up building"),
        };
        let logs = *delivery.iter().find(|&&p| s4_sim::pile::get(&w, p).is_some_and(|d| d.good == LOG))?;
        // Five logs delivered (as carriers would).
        for _ in 0..5 {
            s4_sim::pile::increase_unforeseen(&mut w, logs, 1);
        }
        let door = match &w.entities.by_index(mill as u32)?.data {
            EntityData::Building(d) => d.door,
            _ => return None,
        };
        s4_sim::create::add_settler(&mut w, door.x() + 2, door.y() + 2, 1, SAWMILLWORKER)?;
        // Count the logs the worker takes and the boards he puts down (carriers may also bring
        // logs or take boards: only the decreases / increases are the worker's).
        let amount = |w: &World, p: u16| s4_sim::pile::get(w, p).map_or(0, |d| d.amount);
        let (mut used, mut boards, mut l0, mut b0) = (0u8, 0u8, amount(&w, logs), amount(&w, product));
        for _ in 0..6000 {
            s4_sim::tick(&mut w, &[]);
            let (l, b) = (amount(&w, logs), amount(&w, product));
            used += l0.saturating_sub(l);
            boards += b.saturating_sub(b0);
            (l0, b0) = (l, b);
        }
        eprintln!("sawmill {mill} at ({x},{y}): {used} logs used, {boards} boards");
        Some((w.state_hash(), used, boards))
    };
    let Some(r) = run() else { return };
    assert!(r.2 >= 5, "the sawmill made {} boards from its 5 logs", r.2);
    assert_eq!(r.1, r.2, "every log used became a board");
    assert_eq!(run().unwrap(), r, "two runs differ");
}

/// The grain farm (`CFarmBuildingRole` + `CFreeWorkerRole`, all from the shipped exe): the farmer
/// sows grain where `SearchGrainSeedPos` allows (`CPlant` stage 0, growing 30 updates of 31 ticks
/// per stage), and harvests it once ripe (`SearchGrain` → event 5 → `Take` → grain on the pile).
#[test]
fn farmer_sows_and_harvests_grain() {
    const GRAINFARM: u8 = 11;
    const FARMERGRAIN: u16 = 7;
    let run = || -> Option<(u64, u32, u32)> {
        let mut w = roman02()?;
        let size = w.grid.size() as i32;
        let sites = possible_sites(&w, 1, GRAINFARM, (0, 0), (size, size));
        let &(x, y, _) = sites.first()?;
        let farm = s4_sim::create::add_building(&mut w, x as i32, y as i32, 1, GRAINFARM).ok()?;
        let pile = match &w.buildings.roles[&farm].kind {
            RoleKind::Farm(s) => s.pile,
            _ => panic!("the grain farm is not a farm role"),
        };
        assert_eq!(s4_sim::pile::get(&w, pile).map(|p| p.good), Some(16), "the farm's pile holds grain");
        let door = match &w.entities.by_index(farm as u32)?.data {
            EntityData::Building(d) => d.door,
            _ => return None,
        };
        s4_sim::create::add_settler(&mut w, door.x() + 2, door.y() + 2, 1, FARMERGRAIN)?;
        let sown = |w: &World| -> u32 {
            w.entities.iter().filter(|(_, e)| matches!(&e.data, EntityData::Deco(d) if (e.kind == 209 || e.kind == 210) && !d.static_instance && d.stage < 3)).count() as u32
        };
        let amount = |w: &World| s4_sim::pile::get(w, pile).map_or(0, |p| p.amount as u32);
        let (mut max_sown, mut harvested, mut last) = (0, 0, amount(&w));
        for _ in 0..12_000 {
            s4_sim::tick(&mut w, &[]);
            max_sown = max_sown.max(sown(&w));
            let a = amount(&w);
            harvested += a.saturating_sub(last);
            last = a;
        }
        eprintln!("grain farm {farm} at ({x},{y}): up to {max_sown} fields growing, {harvested} grain harvested");
        Some((w.state_hash(), max_sown, harvested))
    };
    let Some(r) = run() else { return };
    assert!(r.1 >= 1, "nothing was sown");
    assert!(r.2 >= 1, "no grain was harvested");
    assert_eq!(run().unwrap(), r, "two runs differ");
}
