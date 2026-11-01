//! The monitor's read-only debug channel (`s4_sim::debug`): on, every tick leaves a timing
//! record and the world is the same as with it off (same state hash); the economy views read the
//! world as it is. Skipped without the game data. One test: the switch is process-wide.

use s4_sim::{Rules, World};

fn roman02() -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map");
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    // Player 2 a computer player (the AI view).
    let mut setup = s4_sim::NewGame::default();
    setup.control[1] = 3;
    s4_sim::new_game(&map, Rules::load_default().ok()?, &setup).ok().map(|(w, _)| w)
}

#[test]
fn the_debug_channel_reads_and_changes_nothing() {
    let (Some(mut on), Some(mut off)) = (roman02(), roman02()) else { return };
    s4_sim::debug::enable(true);
    for _ in 0..600 {
        s4_sim::tick(&mut on, &[]);
    }
    s4_sim::debug::enable(false);
    for _ in 0..600 {
        s4_sim::tick(&mut off, &[]);
    }
    assert_eq!(on.state_hash(), off.state_hash(), "the timings changed the world");
    // Collected while on only, then dropped by switching off: on again, one record a tick.
    assert!(s4_sim::debug::take_ticks().is_empty());
    s4_sim::debug::enable(true);
    for _ in 0..20 {
        s4_sim::tick(&mut on, &[]);
    }
    let ticks = s4_sim::debug::take_ticks();
    s4_sim::debug::enable(false);
    assert_eq!(ticks.len(), 20);
    assert_eq!(ticks[0].tick + 19, ticks[19].tick);
    let names: Vec<&str> = ticks[19].parts.iter().map(|p| p.0).collect();
    eprintln!("tick {}: {:?} in {:?}", ticks[19].tick, ticks[19].parts, ticks[19].total);
    for n in ["input_process", "settlers", "ai_update", "tiling_update", "statistic_update"] {
        assert!(names.contains(&n), "part {n} in {names:?}");
    }
    let sum: std::time::Duration = ticks[19].parts.iter().map(|p| p.1).sum();
    assert!(sum <= ticks[19].total);
    // Path searches are counted, and a walker's path reads back.
    s4_sim::debug::enable(true);
    let mut fine = 0;
    let mut walker = None;
    for _ in 0..300 {
        s4_sim::tick(&mut on, &[]);
        if walker.is_none() {
            walker = on.entities.iter().map(|(i, _)| i).find(|&i| s4_sim::debug::path_of(&on, i).len() > 2);
        }
    }
    for t in s4_sim::debug::take_ticks() {
        fine += t.paths.fine;
    }
    s4_sim::debug::enable(false);
    let walker = walker.expect("a walker with a path");
    eprintln!("{fine} fine searches; walker {walker}: {:?}", s4_sim::debug::path_of(&on, walker));
    assert!(fine > 0, "no fine search counted");
    // The views.
    let prod = s4_sim::debug::production(&on, 1);
    let car = s4_sim::debug::carriers(&on, 1);
    let sites = s4_sim::debug::sites(&on, 1);
    eprintln!("player 1: {} production buildings, carriers {car:?}, {} sites", prod.len(), sites.len());
    assert!(!prod.is_empty() && car.carriers > 0 && car.idle + car.busy == car.carriers);
    // The decision log of a watched settler, the settler and AI views.
    let watched = walker;
    s4_sim::debug::watch(&[watched]);
    s4_sim::debug::enable(true);
    for _ in 0..400 {
        s4_sim::tick(&mut on, &[]);
    }
    let log = s4_sim::debug::take_log();
    s4_sim::debug::enable(false);
    s4_sim::debug::watch(&[]);
    eprintln!("log of {watched}: {} entries, first {:?}", log.len(), log.first());
    assert!(!log.is_empty() && log.iter().all(|l| l.entity == watched));
    let v = s4_sim::debug::settler(&on, watched).expect("settler view");
    eprintln!("settler {watched}: {v:?}");
    let ai = s4_sim::debug::ai(&on, 2).expect("player 2's AI");
    eprintln!("AI 2: {} agents, {} task forces, sectors {:?}", ai.agents.len(), ai.task_forces.len(), ai.sectors);
    assert!(!ai.agents.is_empty());
    assert!(s4_sim::debug::ai(&on, 1).is_none());
    // The queues: with the channel on, requests made since carry their tick. A site of player
    // 1's asks for its material.
    let size = on.grid.size() as i32;
    let spot = s4_sim::build::possible_sites(&on, 1, 1, (0, 0), (size, size)).into_iter().next().expect("a site");
    s4_sim::debug::enable(true);
    let site = s4_sim::build::site::add_building_site(&mut on, spot.0 as i32, spot.1 as i32, 1, 1).expect("site");
    for _ in 0..600 {
        s4_sim::tick(&mut on, &[]);
    }
    // A request queued now carries this tick.
    let eco = on.economy.player_ecos[1][0];
    s4_sim::eco::request_good(&mut on, eco, site, 30);
    let tr = s4_sim::debug::transport(&on, 1);
    let r = tr.iter().flat_map(|t| &t.requests).find(|r| r.building == site && r.good == 30).expect("the request");
    assert_eq!(r.since, Some(on.tick));
    let bq = s4_sim::debug::build_queue(&on, 1);
    let (eco_orders, bld_orders) = s4_sim::debug::orders(&on, 1);
    let jobs = s4_sim::debug::job_states(&on, 1);
    let aq = s4_sim::debug::ai_queues(&on, 2);
    s4_sim::debug::enable(false);
    let requests: usize = tr.iter().map(|t| t.requests.len()).sum();
    let stamped = tr.iter().flat_map(|t| &t.requests).filter(|r| r.since.is_some()).count();
    let idle: usize = tr.iter().map(|t| t.idle_carriers.len()).sum();
    eprintln!(
        "transport: {} eco-sectors, {requests} requests ({stamped} stamped), {} offers, {idle} idle carriers, {} jobs; {} queued sites; orders {:?} / {}; job states {:?}; AI attacks {aq:?}",
        tr.len(),
        tr.iter().map(|t| t.offers.len()).sum::<usize>(),
        tr.iter().map(|t| t.jobs.len()).sum::<usize>(),
        bq.len(),
        eco_orders.first().map(|o| o.soldier_orders),
        bld_orders.len(),
        jobs.iter().find(|j| j.kind == 1)
    );
    assert!(!tr.is_empty() && !jobs.is_empty() && !aq.is_empty());
    assert!(bq.iter().any(|q| q.site.id == site), "the site in the build queue");
    let carriers = jobs.iter().find(|j| j.kind == 1).expect("carriers");
    assert_eq!(carriers.idle as usize, idle, "the idle carriers are the offered ones");
}
