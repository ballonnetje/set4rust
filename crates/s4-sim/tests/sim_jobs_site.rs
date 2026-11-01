//! A construction site on roman02 from command to finished building, driven by `tick()` only:
//! command 5025 places a woodcutter's hut, command 5027 recruits diggers and builders (roman02
//! starts without any; carriers fetch shovels and hammers), and the site does the rest itself:
//! `OrderDigger`, `OrderMaterial` (carriers bring boards and stones from the storage), then
//! `OrderBuilder`, until `BuildingDone` gives the hut its real role. The second test builds next
//! to the map's sawmill, so the chain tree → log → sawmill → board → site runs end to end.
//! Skipped when the game data is not present.

use s4_sim::build::possible_sites;
use s4_sim::entity::EntityData;
use s4_sim::{Command, PlayerCommand, World};

const WOODCUTTER: u8 = 1;
const DIGGER: u16 = 2;
const BUILDER: u16 = 3;

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
    /// Tick the site was finished.
    done: u32,
    /// Ticks the site spent in states 2/3 (digging), 4 (footprint) and 5 (building).
    states: [u32; 3],
    /// Most diggers and builders at work at once.
    peak: (u8, u8),
    /// Goods the carriers brought to the site's piles (boards, stones).
    delivered: (u32, u32),
    /// The script's GOODARRIVE events for the site's building type (0x56ec10).
    arrived: u32,
}

/// Places a woodcutter's hut near player 1's castle, away from the map's soldiers: the site
/// and the commands that recruit its diggers and builders.
fn place(w: &mut World) -> Option<(u16, Vec<PlayerCommand>)> {
    let castle = w.entities.iter().find(|(id, e)| e.owner == 1 && matches!(&e.data, EntityData::Building(_)) && s4_sim::buildings::have_inhabitant(w, *id)).map(|(_, e)| (e.x(), e.y()))?;
    // Away from the map's soldiers: their roles are not ported, so they never step aside.
    let soldiers: Vec<(i32, i32)> = w.entities.iter().filter(|(_, e)| matches!(e.data, EntityData::Settler(_)) && e.kind >= 29).map(|(_, e)| (e.x(), e.y())).collect();
    let size = w.grid.size() as i32;
    let &(x, y, _) = possible_sites(w, 1, WOODCUTTER, (0, 0), (size, size))
        .iter()
        .filter(|s| soldiers.iter().all(|&(a, b)| (a - s.0 as i32).abs() + (b - s.1 as i32).abs() > 8))
        .min_by_key(|s| (s.0 as i32 - castle.0).abs() + (s.1 as i32 - castle.1).abs())?;
    s4_sim::tick(w, &[PlayerCommand { player: 1, command: Command::Build { building: WOODCUTTER as u32, x, y } }]);
    let site = *w.sites.roles.keys().max()?;
    let eco = match &w.entities.by_index(site as u32)?.data {
        EntityData::Building(d) => s4_sim::eco::eco_of(w, w.grid.index(d.ensign.x() as u32, d.ensign.y() as u32)),
        _ => return None,
    };
    let recruit: Vec<PlayerCommand> = [DIGGER, BUILDER]
        .iter()
        .map(|&kind| PlayerCommand { player: 1, command: Command::SetUnitReservation { eco_sector: eco, kind, value: 4 } })
        .collect();
    // PrepareGround (0x543680) attaches each material pile to the site (vtbl+0x74).
    for p in w.sites.roles[&site].piles.into_iter().filter(|&p| p != 0) {
        assert_eq!(s4_sim::pile::get(w, p).map(|d| d.building), Some(site), "pile {p} not the site's");
    }
    Some((site, recruit))
}

fn run() -> Option<Outcome> {
    let mut w = roman02()?;
    let (site, recruit) = place(&mut w)?;
    w.script_events.wants |= 1 << s4_sim::event::ScriptEvents::GOODARRIVE;
    let mut arrived = 0;
    let (boards, stones) = { let s = &w.sites.roles[&site]; (s.boards, s.stones) };
    let piles = w.sites.roles[&site].piles;
    let amount = |w: &World, p: u16| s4_sim::pile::get(w, p).map_or(0, |d| d.amount as u32);
    let mut last = [amount(&w, piles[0]), amount(&w, piles[1])];
    let (mut states, mut peak, mut delivered) = ([0u32; 3], (0u8, 0u8), (0u32, 0u32));
    let mut first = true;
    while let Some(s) = w.sites.roles.get(&site) {
        match s.state {
            2 | 3 => states[0] += 1,
            4 => states[1] += 1,
            _ => states[2] += 1,
        }
        peak = (peak.0.max(s.diggers_working), peak.1.max(s.builders_working));
        assert!(w.tick < 20_000, "site {site} not finished: state {}, work {}/{}", s.state, s.work_done, s.work);
        let now = [amount(&w, piles[0]), amount(&w, piles[1])];
        delivered.0 += now[0].saturating_sub(last[0]);
        delivered.1 += now[1].saturating_sub(last[1]);
        last = now;
        s4_sim::tick(&mut w, if first { &recruit } else { &[] });
        first = false;
        let kind = w.entities.by_index(site as u32).map_or(0, |e| e.kind as i32);
        arrived += w.script_events.queue.drain(..).filter(|&(ev, a)| ev == s4_sim::event::ScriptEvents::GOODARRIVE && a[0] == kind && a[2] == 1).count() as u32;
    }
    let done = w.tick;
    // The hut has its real role and goes on to order its woodcutter.
    assert!(w.buildings.roles.contains_key(&site), "the finished hut has no role");
    let e = w.entities.by_index(site as u32)?;
    assert_eq!(e.flags & s4_sim::entity::flag::BIRTH, 0);
    assert_ne!(e.flags & s4_sim::entity::flag::READY, 0);
    // Everything the hut cost was brought by carriers (none was on the site's piles before).
    assert_eq!(delivered, (boards as u32, stones as u32), "goods delivered to the site");
    // The builders, told by event 9, leave; then the diggers and builders are all free again.
    for _ in 0..200 {
        s4_sim::tick(&mut w, &[]);
    }
    for (_, e) in w.entities.iter() {
        if let EntityData::Settler(d) = &e.data
            && (e.kind == DIGGER || e.kind == BUILDER)
        {
            assert_ne!(d.job.home, site, "a worker still belongs to the finished site");
        }
    }
    Some(Outcome { hash: w.state_hash(), done, states, peak, delivered, arrived })
}

#[test]
fn woodcutter_hut_is_built_from_command_to_role() {
    let Some(r) = run() else { return };
    eprintln!("{r:?}");
    assert!(r.peak.0 >= 1 && r.peak.1 >= 1, "no digger or builder came: {:?}", r.peak);
    assert_eq!(r.arrived, r.delivered.0 + r.delivered.1, "GOODARRIVE per good delivered to the site");
    assert_eq!(run().unwrap(), r, "two runs differ");
}

/// A site crushed after its first delivered good: its material piles become free (anywhere)
/// piles and keep their goods (the destroyed site's observers get target 0, 0x56fd10).
#[test]
fn crushed_site_leaves_free_piles() {
    let Some(mut w) = roman02() else { return };
    let Some((site, recruit)) = place(&mut w) else { return };
    let piles = w.sites.roles[&site].piles;
    let amount = |w: &World, p: u16| s4_sim::pile::get(w, p).map_or(0, |d| d.amount);
    s4_sim::tick(&mut w, &recruit);
    while !piles.iter().any(|&p| p != 0 && amount(&w, p) > 0) {
        assert!(w.tick < 20_000, "nothing delivered");
        s4_sim::tick(&mut w, &[]);
    }
    s4_sim::tick(&mut w, &[PlayerCommand { player: 1, command: Command::CrushBuilding { building: site as u32 } }]);
    for _ in 0..10 {
        s4_sim::tick(&mut w, &[]);
    }
    assert!(w.entities.by_index(site as u32).is_none_or(|e| e.is_dead()), "the site is still there");
    let full: Vec<u16> = piles.into_iter().filter(|&p| p != 0 && amount(&w, p) > 0).collect();
    assert!(!full.is_empty(), "the delivered goods are gone");
    for p in full {
        let d = s4_sim::pile::get(&w, p).unwrap();
        assert_eq!((d.role, d.building), (s4_sim::pile::role::ANYWHERE, 0), "pile {p}");
    }
}

const SAWMILL: u16 = 3;
const LOG: u8 = 22;
/// A building of 10 boards and 12 stones: its site wants boards long enough for the sawmill.
const BIG: u8 = 42;

#[derive(Debug, PartialEq, Eq)]
struct Chain {
    hash: u64,
    done: u32,
    /// Logs on the map at the start (all piles) and logs the sawmill received.
    logs_at_start: u32,
    logs_delivered: u32,
    /// Boards the sawmill made, and those of them carried to the site.
    boards_made: u32,
    sawmill_boards_to_site: u32,
}

fn chain() -> Option<Chain> {
    let mut w = roman02()?;
    let mill = w.entities.iter().find(|(_, e)| e.owner == 1 && e.kind == SAWMILL && matches!(e.data, EntityData::Building(_))).map(|(i, _)| i)?;
    let (mx, my) = { let e = w.entities.by_index(mill as u32)?; (e.x(), e.y()) };
    let (delivery, product) = match &w.buildings.roles[&mill].kind {
        s4_sim::buildings::RoleKind::WorkUp(s) => (s.delivery.clone(), s.product),
        _ => return None,
    };
    let logs_pile = *delivery.iter().find(|&&p| s4_sim::pile::get(&w, p).is_some_and(|d| d.good == LOG))?;
    let amount = |w: &World, p: u16| s4_sim::pile::get(w, p).map_or(0, |d| d.amount as u32);
    // Every board on the sawmill's production pile is one it made.
    assert_eq!(amount(&w, product), 0);
    let logs_at_start: u32 = w.entities.iter().filter_map(|(_, e)| match &e.data { EntityData::Pile(d) if d.good == LOG => Some(d.amount as u32), _ => None }).sum();
    let soldiers: Vec<(i32, i32)> = w.entities.iter().filter(|(_, e)| matches!(e.data, EntityData::Settler(_)) && e.kind >= 29).map(|(_, e)| (e.x(), e.y())).collect();
    let size = w.grid.size() as i32;
    let &(x, y, _) = possible_sites(&w, 1, BIG, (0, 0), (size, size))
        .iter()
        .filter(|s| soldiers.iter().all(|&(a, b)| (a - s.0 as i32).abs() + (b - s.1 as i32).abs() > 8))
        .min_by_key(|s| (s.0 as i32 - mx).abs() + (s.1 as i32 - my).abs())?;
    s4_sim::tick(&mut w, &[PlayerCommand { player: 1, command: Command::Build { building: BIG as u32, x, y } }]);
    let site = *w.sites.roles.keys().max()?;
    let eco = match &w.entities.by_index(site as u32)?.data {
        EntityData::Building(d) => s4_sim::eco::eco_of(&w, w.grid.index(d.ensign.x() as u32, d.ensign.y() as u32)),
        _ => return None,
    };
    let recruit: Vec<PlayerCommand> = [DIGGER, BUILDER]
        .iter()
        .map(|&kind| PlayerCommand { player: 1, command: Command::SetUnitReservation { eco_sector: eco, kind, value: 4 } })
        .collect();
    let site_boards = w.sites.roles[&site].piles[0];
    let (mut logs_delivered, mut boards_made) = (0, 0);
    let (mut last_logs, mut last_boards) = (amount(&w, logs_pile), amount(&w, product));
    // Carriers sent from the sawmill's pile to the site's board pile (event 10 sets both piles).
    let (mut sent, mut assigned) = (0u32, std::collections::BTreeSet::new());
    let mut first = true;
    while w.sites.roles.contains_key(&site) {
        assert!(w.tick < 20_000, "site not finished: {:?}", w.sites.roles[&site].state);
        s4_sim::tick(&mut w, if first { &recruit } else { &[] });
        first = false;
        let (l, b) = (amount(&w, logs_pile), amount(&w, product));
        logs_delivered += l.saturating_sub(last_logs);
        boards_made += b.saturating_sub(last_boards);
        (last_logs, last_boards) = (l, b);
        let now: std::collections::BTreeSet<u16> = w
            .entities
            .iter()
            .filter(|(_, e)| matches!(&e.data, EntityData::Settler(d) if d.job.source_pile == product && d.job.carrier.dest_pile == site_boards))
            .map(|(i, _)| i)
            .collect();
        sent += now.difference(&assigned).count() as u32;
        assigned = now;
    }
    let done = w.tick;
    Some(Chain { hash: w.state_hash(), done, logs_at_start, logs_delivered, boards_made, sawmill_boards_to_site: sent })
}

#[test]
fn boards_from_the_sawmill_build_a_site() {
    let Some(r) = chain() else { return };
    eprintln!("{r:?}");
    // No log on the map at the start: each one the sawmill got was felled by the woodcutter.
    assert_eq!(r.logs_at_start, 0);
    assert!(r.logs_delivered >= 1 && r.boards_made >= 1, "{r:?}");
    assert!(r.sawmill_boards_to_site >= 1, "no board went from the sawmill to the site");
    assert_eq!(chain().unwrap(), r, "two runs differ");
}
