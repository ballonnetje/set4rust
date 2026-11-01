//! Survey: a construction site on roman02 driven by `tick()` only (the site orders diggers,
//! builders and material itself). Prints the site's progress.
//!   cargo run --release -p s4-sim --example sim_jobs_site_build [building type] [ticks]

use s4_sim::build::possible_sites;
use s4_sim::entity::EntityData;
use s4_sim::{Command, PlayerCommand, World};

fn roman02() -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map");
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

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let btype: u8 = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(1);
    let ticks: u32 = args.get(2).and_then(|a| a.parse().ok()).unwrap_or(20_000);
    let mut w = roman02().expect("roman02");
    let count = |w: &World, k: u16| w.entities.iter().filter(|(_, e)| e.owner == 1 && e.kind == k && matches!(e.data, EntityData::Settler(_))).count();
    println!("player 1: diggers {}, builders {}, carriers {}", count(&w, 2), count(&w, 3), count(&w, 1));
    let goods = |w: &World, g: u8| -> u32 { w.entities.iter().filter_map(|(_, e)| match &e.data { EntityData::Pile(d) if d.good == g => Some(d.amount as u32), _ => None }).sum() };
    println!("boards {}, stones {}", goods(&w, 7), goods(&w, 32));
    let mut all = std::collections::BTreeMap::new();
    for (_, e) in w.entities.iter() {
        if let EntityData::Pile(d) = &e.data { *all.entry(d.good).or_insert(0u32) += d.amount as u32; }
    }
    println!("piles by good: {all:?}");
    // NEAR=<building type>: the site goes next to the player's first building of that type
    // (default: the castle).
    let near: Option<u16> = std::env::var("NEAR").ok().and_then(|v| v.parse().ok());
    let castle = w
        .entities
        .iter()
        .find(|(id, e)| e.owner == 1 && matches!(&e.data, EntityData::Building(_)) && near.map_or(s4_sim::buildings::have_inhabitant(&w, *id), |k| e.kind == k))
        .map(|(_, e)| (e.x(), e.y()))
        .unwrap();
    for t in 1..83u8 {
        if let Some(i) = w.rules.build.info(0, t) {
            print!("{t}:{}/{} ", i.boards, i.stone);
        }
    }
    println!();
    let size = w.grid.size() as i32;
    let sites = possible_sites(&w, 1, btype, (0, 0), (size, size));
    // Away from the map's soldiers (their roles are not ported, so they never step aside).
    let soldiers: Vec<(i32, i32)> = w.entities.iter().filter(|(_, e)| matches!(e.data, EntityData::Settler(_)) && e.kind >= 29).map(|(_, e)| (e.x(), e.y())).collect();
    let &(x, y, _) = sites
        .iter()
        .filter(|s| soldiers.iter().all(|&(a, b)| (a - s.0 as i32).abs() + (b - s.1 as i32).abs() > 8))
        .min_by_key(|s| (s.0 as i32 - castle.0).abs() + (s.1 as i32 - castle.1).abs())
        .expect("no site");
    s4_sim::tick(&mut w, &[PlayerCommand { player: 1, command: Command::Build { building: btype as u32, x, y } }]);
    let site = *w.sites.roles.keys().max().expect("site");
    // Recruit diggers and builders the player's way (5027 UnitReservation), as roman02 starts
    // without any (OrderDiggerAndBuilderAtStartUp is not ported).
    let eco = match &w.entities.by_index(site as u32).unwrap().data {
        EntityData::Building(d) => s4_sim::eco::eco_of(&w, w.grid.index(d.ensign.x() as u32, d.ensign.y() as u32)),
        _ => 0,
    };
    let recruit: u32 = std::env::var("RECRUIT").ok().and_then(|v| v.parse().ok()).unwrap_or(4);
    let cmds: Vec<PlayerCommand> = [2u16, 3].iter().map(|&kind| PlayerCommand { player: 1, command: Command::SetUnitReservation { eco_sector: eco, kind, value: recruit } }).collect();
    s4_sim::tick(&mut w, &cmds);
    if let Some(e) = w.economy.get(eco) {
        println!("eco {eco}: worker_delta {:?} min_carrier {} carriers {} offered carrier head {} shovel/hammer offers {:?}", e.worker_delta, e.min_carrier, e.settlers[1], e.settler_offer[1], (e.good_offer[27], e.good_offer[25], e.good_offer[26], e.good_offer[28]));
    }
    println!("site {site} type {btype} at ({x},{y})");
    let mut last = String::new();
    let mut sources = std::collections::BTreeSet::new();
    for _ in 0..ticks {
        s4_sim::tick(&mut w, &[]);
        let line = match w.sites.roles.get(&site) {
            Some(s) => {
                let pa = |p: u16| s4_sim::pile::get(&w, p).map_or((0, 0), |d| (d.amount, d.coming));
                format!(
                    "d{} b{} state {} effort {} diggers {}/{} builders {} work {}/{} used b{}/{} s{}/{} piles {:?}",
                    count(&w, 2), count(&w, 3), s.state, s.dig_effort, s.diggers_working, s.diggers, s.builders_working, s.work_done, s.work,
                    s.boards_used, s.boards, s.stones_used, s.stones, s.piles.map(pa)
                )
            }
            None => "done".into(),
        };
        if let Some(sp) = w.sites.roles.get(&site).map(|s| s.piles[0]) {
            for (_, e) in w.entities.iter() {
                if let EntityData::Settler(d) = &e.data
                    && d.job.carrier.dest_pile == sp
                    && d.job.source_pile != 0
                    && sources.insert(d.job.source_pile)
                {
                    println!("t{} board for the site from pile {} (kind {})", w.tick, d.job.source_pile, w.entities.by_index(d.job.source_pile as u32).and_then(|p| match &p.data { EntityData::Pile(x) => Some(x.role), _ => None }).unwrap_or(255));
                }
            }
        }
        if line != last {
            println!("t{} {line}", w.tick);
            if line == "done" {
                break;
            }
            last = line;
        }
    }
    for (i, e) in w.entities.iter() {
        if let EntityData::Settler(d) = &e.data
            && (e.kind == 2 || e.kind == 3)
        {
            println!("settler {i} kind {} at ({},{}) home {} list {} job {:?}", e.kind, e.x(), e.y(), d.job.home, d.job.list_id, if e.kind == 2 && d.job.home != 0 { format!("{:?}", d.job) } else { String::new() });
        }
    }
    if let Ok(v) = std::env::var("NODE") {
        let (nx, ny) = v.split_once(',').map(|(a, b)| (a.parse::<i32>().unwrap(), b.parse::<i32>().unwrap())).unwrap();
        for dy in -1..=1 {
            for dx in -1..=1 {
                let n = w.grid.index((nx + dx) as u32, (ny + dy) as u32);
                println!("({},{}) h {} flags {:#x} move {} mapobj {} deco {}", nx + dx, ny + dy, w.layers.height[n], w.layers.flags[n], w.layers.move_cost(n), w.layers.map_object[n], w.layers.deco_object[n]);
            }
        }
    }
}
