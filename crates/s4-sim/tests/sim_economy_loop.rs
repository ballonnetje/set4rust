//! The first economy loop on roman02, driven by `tick()` only: the map's woodcutter hut and
//! sawmill order their workers (`OrderInhabitant` → `CEcoSector::OrderWorker`: carriers fetch an
//! axe and a saw and change type), the woodcutter fells trees onto its pile, `InitTransport`
//! sends carriers with the logs to the sawmill's delivery pile, the sawmill worker saws boards,
//! and carriers take the boards to a storage area (placed like a map building; boards switched
//! on by the player command). Skipped when the game data is not present.

use s4_sim::build::possible_sites;
use s4_sim::entity::EntityData;
use s4_sim::{Command, PlayerCommand, World, pile};

const SAWMILL: u16 = 3;
const LOG: u8 = 22;
const BOARD: u8 = 7;

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

#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    hash: u64,
    /// Logs that reached the sawmill's delivery pile (counted as they arrive).
    logs_delivered: u32,
    /// Boards the sawmill made (its production pile's arrivals).
    boards_made: u32,
    /// Boards carried away from the sawmill's production pile.
    boards_taken: u32,
    /// Boards on the storage area's piles at the end (the map's own boards are stored too).
    boards_stored: u32,
    /// All boards on piles, at the start and at the end (carried ones are on their way).
    boards_total: (u32, u32),
    /// Tick the first board reached the storage area.
    first_stored: Option<u32>,
}

fn run(ticks: u32) -> Option<Outcome> {
    let mut w = roman02()?;
    let mill = w.entities.iter().find(|(_, e)| e.owner == 1 && e.kind == SAWMILL && matches!(e.data, EntityData::Building(_))).map(|(i, _)| i)?;
    let (mx, my) = { let e = w.entities.by_index(mill as u32)?; (e.x(), e.y()) };
    // A storage area on the nearest possible place to the sawmill, set up as a map building.
    let storage_type = s4_sim::buildings::storage::STORAGE_AREA as u8;
    let size = w.grid.size() as i32;
    let sites = possible_sites(&w, 1, storage_type, (0, 0), (size, size));
    let &(sx, sy, _) = sites.iter().min_by_key(|s| (s.0 as i32 - mx).abs() + (s.1 as i32 - my).abs())?;
    let store = s4_sim::create::add_building(&mut w, sx as i32, sy as i32, 1, storage_type).ok()?;
    let switch = PlayerCommand { player: 1, command: Command::StorageGoodSwitch { building: store, good: BOARD as u16, value: 1 } };
    let (delivery, product) = match &w.buildings.roles[&mill].kind {
        s4_sim::buildings::RoleKind::WorkUp(s) => (s.delivery.clone(), s.product),
        _ => return None,
    };
    let logs_pile = *delivery.iter().find(|&&p| pile::get(&w, p).is_some_and(|d| d.good == LOG))?;
    let amount = |w: &World, p: u16| pile::get(w, p).map_or(0, |d| d.amount as u32);
    let stored = |w: &World| -> u32 {
        let eco = s4_sim::buildings::storage::eco_of_building(w, store);
        w.economy.get(eco).map_or(0, |e| {
            e.good_storage_piles.iter().filter_map(|&(b, p, _)| (b == store).then(|| pile::get(w, p)).flatten()).filter(|d| d.good == BOARD).map(|d| d.amount as u32).sum()
        })
    };
    let all_boards = |w: &World| -> u32 {
        w.entities.iter().filter_map(|(_, e)| match &e.data {
            EntityData::Pile(d) if d.good == BOARD => Some(d.amount as u32),
            EntityData::Settler(d) if d.job.carrier.good == BOARD => Some(1),
            _ => None,
        }).sum()
    };
    let boards_start = all_boards(&w);
    let (mut logs_delivered, mut boards_made, mut boards_taken, mut first_stored) = (0, 0, 0, None);
    let (mut last_logs, mut last_boards) = (amount(&w, logs_pile), amount(&w, product));
    for t in 0..ticks {
        let cmds = if t == 0 { vec![switch.clone()] } else { vec![] };
        s4_sim::tick(&mut w, &cmds);
        let (l, b) = (amount(&w, logs_pile), amount(&w, product));
        logs_delivered += l.saturating_sub(last_logs);
        boards_made += b.saturating_sub(last_boards);
        boards_taken += last_boards.saturating_sub(b);
        (last_logs, last_boards) = (l, b);
        if first_stored.is_none() && stored(&w) > 0 {
            first_stored = Some(w.tick);
        }
    }
    let boards_total = (boards_start, all_boards(&w));
    Some(Outcome { hash: w.state_hash(), logs_delivered, boards_made, boards_taken, boards_stored: stored(&w), boards_total, first_stored })
}

#[test]
fn logs_to_boards_to_storage() {
    let Some(r) = run(12_000) else { return };
    eprintln!("{r:?}");
    assert!(r.logs_delivered >= 2, "carriers brought {} logs to the sawmill", r.logs_delivered);
    assert!(r.boards_made >= 2, "the sawmill made {} boards", r.boards_made);
    assert!(r.boards_taken >= 1, "no board was carried away from the sawmill");
    assert!(r.boards_stored >= 1, "no board reached the storage area");
    // Boards are only made, never lost: the end total is the start plus the sawmill's output.
    assert_eq!(r.boards_total.1, r.boards_total.0 + r.boards_made, "board count");
    assert_eq!(run(12_000).unwrap(), r, "two runs differ");
}
