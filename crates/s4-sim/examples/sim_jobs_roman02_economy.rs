//! roman02 left to itself: the map's own buildings order their workers (carriers fetch tools and
//! change type), cut trees, carry logs to the sawmill and boards on. Prints the state every
//! 2000 ticks: settlers by type, goods on piles (logs 22, boards 7), unported calls.
use std::collections::BTreeMap;
use s4_sim::entity::EntityData;
use s4_sim::event::SimEvent;
fn main() {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map");
    let map = s4_formats::map::MapFile::open(&p).unwrap();
    let rules = s4_sim::Rules::load_default().unwrap();
    let (mut w, _) = s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).unwrap();
    let towers: Vec<(i32, i32)> = w.entities.iter().filter_map(|(id, e)| match &e.data {
        EntityData::Building(_) if s4_sim::buildings::have_inhabitant(&w, id) => Some((e.x(), e.y())),
        _ => None,
    }).collect();
    for (x, y) in towers {
        s4_sim::territory::set_owner(&mut w, x, y);
    }
    for (id, r) in &w.buildings.roles {
        let e = w.entities.by_index(*id as u32).unwrap();
        let ensign = match &e.data { EntityData::Building(d) => d.ensign, _ => continue };
        let eco = w.grid.checked_index(ensign.x(), ensign.y()).map_or(0, |n| s4_sim::eco::eco_of(&w, n));
        println!("building {id} kind {} owner {} flags {:#x} eco {eco} role {:?}", e.kind, e.owner, e.flags, std::mem::discriminant(&r.kind));
    }
    if let Some(e) = w.economy.get(1) {
        println!("eco 1: owner {} carriers {} min {} with order {} offer carrier {} axe offer {} saw offer {}", e.owner, e.settlers[1], e.min_carrier, e.carriers_with_job_order, e.settler_offer[1], e.good_offer[4], e.good_offer[28]);
    }
    let mut unported = BTreeMap::<&'static str, usize>::new();
    for t in 0..20000 {
        s4_sim::tick(&mut w, &[]);
        for e in w.events.drain(..) {
            if let SimEvent::Unported { what, .. } = e {
                *unported.entry(what).or_default() += 1;
            }
        }
        // With MANUAL_TRANSPORT set, one log is sent from the woodcutter's pile to the sawmill by
        // hand at tick 3000 (pile ids of roman02 player 1), to separate the roles from InitTransport.
        if t == 3000 && std::env::var_os("MANUAL_TRANSPORT").is_some() {
            let ok = s4_sim::eco::transport_good(&mut w, 1, 448, 418);
            println!("manual transport_good(448 -> 418): {ok}");
        }
        if (t + 1) % 2000 == 0 {
            let (mut settlers, mut goods) = (BTreeMap::new(), BTreeMap::new());
            for (_, e) in w.entities.iter() {
                match &e.data {
                    EntityData::Settler(d) if e.owner == 1 => {
                        *settlers.entry(e.kind).or_insert(0) += 1;
                        if d.job.list_id == 152 {
                            *settlers.entry(1000).or_insert(0) += 1;
                        }
                    }
                    EntityData::Pile(d) if d.good == 22 || d.good == 7 => *goods.entry(d.good).or_insert(0u32) += d.amount as u32,
                    _ => {}
                }
            }
            println!("t{} settlers {settlers:?} logs/boards {goods:?}", t + 1);
        }
    }
    println!("unported: {unported:?}");
    for (id, e) in w.entities.iter() {
        if let EntityData::Settler(d) = &e.data
            && e.owner == 1
            && matches!(e.kind, 4 | 5 | 12)
        {
            let j = &d.job;
            println!("worker {id} type {} at ({},{}) role {} list {} cursor {} task {} home {} events {:?} registered {}", e.kind, e.x(), e.y(), j.role, j.list_id, j.cursor, j.task, j.home, j.events, e.flags & 0x2000_0000 != 0);
        }
    }
    for (id, r) in &w.buildings.roles {
        if matches!(w.entities.by_index(*id as u32).map(|e| (e.owner, e.kind)), Some((1, 1 | 3 | 4))) {
            println!("hut {id}: inhabitant {} in house {} kind {:?}", r.base.inhabitant, r.base.worker_in_house, r.kind);
        }
    }
    if let Some(e) = w.economy.get(1) {
        println!("eco 1: log offer head {} requests for logs {:?}", e.good_offer[22], e.requests.get(22));
    }
    for (id, e) in w.entities.iter() {
        if let EntityData::Pile(d) = &e.data
            && (d.good == 22 || d.building != 0)
        {
            let eco = w.grid.checked_index(e.x(), e.y()).map_or(0, |n| s4_sim::eco::eco_of(&w, n));
            println!("pile {id} at ({},{}) good {} amount {} role {} building {} coming {} leaving {} offer {} eco {eco} registered {}", e.x(), e.y(), d.good, d.amount, d.role, d.building, d.coming, d.leaving, d.offer, e.flags & 0x2000_0000 != 0);
        }
    }
}
