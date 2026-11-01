//! Building commands from the dialogs, on roman02, driven by `tick()` only:
//! * 5003 `SwitchBuilding` → `IBuildingRole::Switch` (release 0x53ffd0): the sawmill stops (flag
//!   0x1000 cleared, event 7 to its piles and worker) and makes nothing until switched on again.
//! * 5004 `PrioritizeBuildingSite` → `CBuildingSiteRole` vtbl+36 (0x542e40): of two residence
//!   sites the second, prioritised, gets its diggers, builders, boards and stones first and is
//!   finished first; toggling twice leaves no trace; finished sites hold no priority.
//!
//! Skipped when the game data is not present.

use s4_sim::entity::EntityData;
use s4_sim::{Command, PlayerCommand, World};

const SAWMILL: u16 = 3;
const WORKING: u32 = 0x1000;

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

fn sawmill(w: &World) -> u16 {
    w.entities.iter().find(|(_, e)| e.owner == 1 && e.kind == SAWMILL && matches!(e.data, EntityData::Building(_))).map(|(i, _)| i).expect("sawmill")
}

/// Boards the sawmill put on its production pile per 1,000-tick window, with `switch_at` the
/// ticks a SwitchBuilding command is sent; and whether it works at the end.
fn run(switch_at: &[u32], ticks: u32) -> Option<(Vec<u32>, bool, u64)> {
    let mut w = roman02()?;
    let mill = sawmill(&w);
    let product = match &w.buildings.roles[&mill].kind {
        s4_sim::buildings::RoleKind::WorkUp(s) => s.product,
        _ => panic!("sawmill role"),
    };
    let amount = |w: &World| s4_sim::pile::get(w, product).map_or(0, |p| p.amount as u32);
    let mut last = amount(&w);
    let mut made = vec![0; (ticks / 1000) as usize];
    for t in 0..ticks {
        let cmds: Vec<PlayerCommand> =
            if switch_at.contains(&t) { vec![PlayerCommand { player: 1, command: Command::SwitchBuilding { building: mill as u32 } }] } else { vec![] };
        s4_sim::tick(&mut w, &cmds);
        let now = amount(&w);
        made[(t / 1000) as usize] += now.saturating_sub(last);
        last = now;
    }
    let working = w.entities.by_index(mill as u32).is_some_and(|e| e.flags & WORKING != 0);
    Some((made, working, w.state_hash()))
}

#[test]
fn switch_building_stops_and_starts_the_sawmill() {
    let Some(on) = run(&[], 6000) else { return };
    let off = run(&[5], 6000).unwrap();
    let again = run(&[5, 3000], 6000).unwrap();
    eprintln!("boards per 1000 ticks: on {:?}, off {:?}, off then on at 3000 {:?}", on.0, off.0, again.0);
    assert!(on.1 && !off.1 && again.1);
    assert!(on.0.iter().sum::<u32>() > 0, "the sawmill makes boards");
    assert_eq!(off.0.iter().sum::<u32>(), 0, "a stopped sawmill makes nothing");
    assert_eq!(again.0[..3].iter().sum::<u32>(), 0);
    assert!(again.0[3..].iter().sum::<u32>() > 0, "switched on again, it works");
    assert_eq!(Some(again), run(&[5, 3000], 6000), "not deterministic");
}

const RESIDENCE_SMALL: u8 = 41;

/// Two small residences placed near player 1's castle (away from the map's soldiers); the
/// second one is prioritised at `prio_at` (if any). Returns the ticks each site took, and the
/// eco-sector's priority counters right after the command and once both are finished.
#[derive(Debug, PartialEq, Eq)]
struct Prio {
    done: [u32; 2],
    /// (sites, boards, stone, digger sites, diggers needed, builder sites, builders needed)
    counters_after_command: (u8, u8, u8, i8, i16, i8, i16),
    counters_at_end: (u8, u8, u8, i8, i16, i8, i16),
    flags_after_command: (bool, bool, bool, bool, bool),
    hash: u64,
}

fn counters(w: &World, eco: u16) -> (u8, u8, u8, i8, i16, i8, i16) {
    let e = w.economy.get(eco).expect("eco");
    (e.sites_with_prio, e.prio_boards, e.prio_stone, e.digger_prio_sites, e.needed_prio_diggers, e.builder_prio_sites, e.needed_prio_builders)
}

fn prio_run(prio_at: Option<u32>, toggles: u32) -> Option<Prio> {
    let mut w = roman02()?;
    let castle = w.entities.iter().find(|(id, e)| e.owner == 1 && matches!(&e.data, EntityData::Building(_)) && s4_sim::buildings::have_inhabitant(&w, *id)).map(|(_, e)| (e.x(), e.y()))?;
    let soldiers: Vec<(i32, i32)> = w.entities.iter().filter(|(_, e)| matches!(e.data, EntityData::Settler(_)) && e.kind >= 29).map(|(_, e)| (e.x(), e.y())).collect();
    let mut sites = Vec::new();
    for _ in 0..2 {
        let size = w.grid.size() as i32;
        let &(x, y, _) = s4_sim::build::possible_sites(&w, 1, RESIDENCE_SMALL, (0, 0), (size, size))
            .iter()
            .filter(|s| soldiers.iter().all(|&(a, b)| (a - s.0 as i32).abs() + (b - s.1 as i32).abs() > 8))
            .min_by_key(|s| (s.0 as i32 - castle.0).abs() + (s.1 as i32 - castle.1).abs())?;
        s4_sim::tick(&mut w, &[PlayerCommand { player: 1, command: Command::Build { building: RESIDENCE_SMALL as u32, x, y } }]);
        sites.push(*w.sites.roles.keys().max()?);
    }
    let eco = match &w.entities.by_index(sites[1] as u32)?.data {
        EntityData::Building(d) => s4_sim::eco::eco_of(&w, w.grid.index(d.ensign.x() as u32, d.ensign.y() as u32)),
        _ => return None,
    };
    let start = w.tick;
    let mut done = [0u32; 2];
    let mut after = (0, 0, 0, 0, 0, 0, 0);
    let mut flags = (false, false, false, false, false);
    while done.contains(&0) {
        assert!(w.tick < start + 20_000, "sites not finished");
        let t = w.tick - start;
        let cmds: Vec<PlayerCommand> = match prio_at {
            Some(p) if t >= p && t < p + toggles => vec![PlayerCommand { player: 1, command: Command::PrioritizeBuildingSite { building: sites[1] as u32 } }],
            _ => vec![],
        };
        s4_sim::tick(&mut w, &cmds);
        if prio_at.is_some_and(|p| t + 1 == p + toggles) {
            after = counters(&w, eco);
            let f = w.sites.roles[&sites[1]].prio;
            flags = (f.on, f.boards, f.stone, f.diggers, f.builders);
        }
        for k in 0..2 {
            if done[k] == 0 && !w.sites.roles.contains_key(&sites[k]) {
                done[k] = w.tick - start;
            }
        }
    }
    Some(Prio { done, counters_after_command: after, counters_at_end: counters(&w, eco), flags_after_command: flags, hash: w.state_hash() })
}

#[test]
fn prioritize_building_site() {
    let Some(plain) = prio_run(None, 0) else { return };
    let prio = prio_run(Some(2), 1).unwrap();
    let twice = prio_run(Some(2), 2).unwrap();
    eprintln!("plain {plain:?}\nprio {prio:?}\ntoggled twice {twice:?}");
    // On: prioritised, with board, stone and builder priority (both piles are empty) and digger
    // priority if the site needs leveling; the eco-sector counts it once.
    let (on, boards, stone, diggers, builders) = prio.flags_after_command;
    assert!(on && boards && stone && builders);
    let c = prio.counters_after_command;
    assert_eq!((c.0, c.1, c.2, c.5), (1, 1, 1, 1));
    assert_eq!(c.3, diggers as i8);
    assert!(c.6 > 0, "builders needed");
    // Off again: everything back to 0.
    assert_eq!(twice.counters_after_command, (0, 0, 0, 0, 0, 0, 0));
    assert_eq!(twice.flags_after_command, (false, false, false, false, false));
    // Finished sites hold no priority (BuildingDone 0x5429f0).
    assert_eq!(prio.counters_at_end, (0, 0, 0, 0, 0, 0, 0));
    // The prioritised second site no longer waits for the first.
    assert!(prio.done[1] < plain.done[1], "prioritised site took {} vs {}", prio.done[1], plain.done[1]);
    assert_eq!(Some(prio), prio_run(Some(2), 1), "not deterministic");
}

/// 5001 `SetWorkingArea` (release 0x4f62f5): the woodcutter's hut gets the new centre (+0x4c)
/// and its search cursors (+0x14 / +0x18) and warning (+0x1c) start over.
#[test]
fn set_working_area_resets_the_search() {
    let Some(mut w) = roman02() else { return };
    let hut = w
        .entities
        .iter()
        .find(|(id, e)| e.owner == 1 && e.kind == 1 && matches!(e.data, EntityData::Building(_)) && w.buildings.roles.contains_key(id))
        .map(|(i, _)| i)
        .expect("woodcutter hut");
    let work = |w: &World| match &w.buildings.roles[&hut].kind {
        s4_sim::buildings::RoleKind::Gather(s) => s.work.clone(),
        _ => panic!("gather role"),
    };
    let mut t = 0;
    while work(&w).cursors == [[0; 2]; 2] {
        s4_sim::tick(&mut w, &[]);
        t += 1;
        assert!(t < 5000, "the hut never searched");
    }
    let (x, y) = { let e = w.entities.by_index(hut as u32).unwrap(); (e.x() as u16 + 3, e.y() as u16 + 3) };
    s4_sim::tick(&mut w, &[PlayerCommand { player: 1, command: Command::SetWorkingArea { building: hut as u32, x, y } }]);
    let after = work(&w);
    // The hut may already have searched again in that tick; it did not, as the cursors show.
    assert_eq!(after.cursors, [[0; 2]; 2]);
    assert!(!after.warned);
    match &w.entities.by_index(hut as u32).unwrap().data {
        EntityData::Building(d) => assert_eq!((d.working_area.x(), d.working_area.y()), (x as i32, y as i32)),
        _ => unreachable!(),
    }
}

/// 5003 on a construction site → `CBuildingSiteRole::Switch` (0x542bf0): the site stops (flag
/// 0x1000 off, its diggers / builders set free and detached, counts 0, no progress), then starts
/// again (event 8, flag on) and is finished.
#[test]
fn switch_building_stops_and_starts_a_site() {
    let Some(mut w) = roman02() else { return };
    let castle = w.entities.iter().find(|(id, e)| e.owner == 1 && matches!(&e.data, EntityData::Building(_)) && s4_sim::buildings::have_inhabitant(&w, *id)).map(|(_, e)| (e.x(), e.y())).unwrap();
    let size = w.grid.size() as i32;
    let &(x, y, _) = s4_sim::build::possible_sites(&w, 1, RESIDENCE_SMALL, (0, 0), (size, size))
        .iter()
        .min_by_key(|s| (s.0 as i32 - castle.0).abs() + (s.1 as i32 - castle.1).abs())
        .unwrap();
    s4_sim::tick(&mut w, &[PlayerCommand { player: 1, command: Command::Build { building: RESIDENCE_SMALL as u32, x, y } }]);
    let site = *w.sites.roles.keys().max().unwrap();
    let homed = |w: &World| w.entities.iter().filter(|(_, e)| matches!(&e.data, EntityData::Settler(s) if s.job.home == site)).count();
    // Until someone works there.
    let mut t = 0;
    while homed(&w) == 0 {
        s4_sim::tick(&mut w, &[]);
        t += 1;
        assert!(t < 3000, "nobody came to the site");
    }
    let switch = || vec![PlayerCommand { player: 1, command: Command::SwitchBuilding { building: site as u32 } }];
    s4_sim::tick(&mut w, &switch());
    assert_eq!(w.entities.by_index(site as u32).unwrap().flags & WORKING, 0, "stopped");
    let s = &w.sites.roles[&site];
    assert_eq!((s.diggers_working, s.builders_working, s.builder_pos), (0, 0, 0));
    assert_eq!(homed(&w), 0, "workers detached");
    let progress = |w: &World| {
        let s = &w.sites.roles[&site];
        (s.state, s.dig_effort, s.work_done)
    };
    let before = progress(&w);
    for _ in 0..1500 {
        s4_sim::tick(&mut w, &[]);
    }
    assert_eq!(progress(&w), before, "a stopped site makes no progress");
    assert_eq!(homed(&w), 0, "nobody is called to a stopped site");
    s4_sim::tick(&mut w, &switch());
    assert_ne!(w.entities.by_index(site as u32).unwrap().flags & WORKING, 0, "working again");
    let mut t = 0;
    while w.sites.roles.contains_key(&site) {
        s4_sim::tick(&mut w, &[]);
        t += 1;
        assert!(t < 20_000, "the site never finished after the restart");
    }
    s4_sim::eco::check_offer_lists(&w).unwrap();
}

/// 5003 on a storage → `CStorageBuildingRole::Switch` (0x5506b0): stopped, its goods leave the
/// eco-sector's storage list; working again, they are back.
#[test]
fn switch_building_stops_and_starts_a_storage() {
    let Some(mut w) = roman02() else { return };
    let Some(store) = w.entities.iter().find(|(id, e)| e.owner == 1 && matches!(w.buildings.roles.get(id).map(|r| &r.kind), Some(s4_sim::buildings::RoleKind::Storage(_)))).map(|(id, _)| id) else {
        return;
    };
    s4_sim::tick(&mut w, &[PlayerCommand { player: 1, command: Command::StorageGoodSwitch { building: store, good: 7, value: 1 } }]);
    let eco = s4_sim::buildings::storage::eco_of_building(&w, store);
    let listed = |w: &World| w.economy.get(eco).map_or(0, |e| e.storage_goods.iter().filter(|g| g.0 == store).count());
    assert_eq!(listed(&w), 1);
    let switch = || vec![PlayerCommand { player: 1, command: Command::SwitchBuilding { building: store as u32 } }];
    s4_sim::tick(&mut w, &switch());
    assert_eq!(w.entities.by_index(store as u32).unwrap().flags & WORKING, 0);
    assert_eq!(listed(&w), 0, "a stopped storage takes nothing");
    s4_sim::tick(&mut w, &switch());
    assert_eq!(listed(&w), 1, "working again");
}
