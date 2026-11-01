//! Why roman02's woodcutter (player 1, hut 447) stops after a few logs: every 1000 ticks, the
//! trees in its working area (resource 0x70 within the radius around the working-area centre),
//! how many of those are takeable by `SearchTree` (no node flag 0x24, static instance), the hut's
//! search state and the worker's job state.
use s4_sim::entity::EntityData;
fn main() {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map");
    let map = s4_formats::map::MapFile::open(&p).unwrap();
    let rules = s4_sim::Rules::load_default().unwrap();
    let (mut w, _) = s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).unwrap();
    let hut = w.entities.iter().find(|(_, e)| e.owner == 1 && e.kind == 1 && matches!(e.data, EntityData::Building(_))).map(|(i, _)| i).unwrap();
    let (centre, race, type_ex) = match &w.entities.by_index(hut as u32).unwrap().data {
        EntityData::Building(d) => (d.working_area, w.entities.by_index(hut as u32).unwrap().race, d.type_ex),
        _ => unreachable!(),
    };
    let radius = w.rules.work.info(race, type_ex).unwrap().working_radius;
    let last = s4_sim::spiral::last(radius as u8);
    println!("hut {hut} working area ({},{}) radius {radius} spiral entries {}", centre.x(), centre.y(), last + 1);
    for t in 0..20000u32 {
        s4_sim::tick(&mut w, &[]);
        if t % 1000 != 999 {
            continue;
        }
        let (mut trees, mut takeable, mut flagged) = (0, 0, 0);
        for k in 0..=last {
            let (x, y) = (centre.x() + s4_sim::spiral::dx(k), centre.y() + s4_sim::spiral::dy(k));
            let Some(n) = w.grid.checked_index(x, y) else { continue };
            if w.layers.resource_type(n) != 0x70 {
                continue;
            }
            trees += 1;
            let id = w.layers.deco_object[n];
            let st = w.entities.by_index(id as u32).is_some_and(|e| matches!(&e.data, EntityData::Deco(d) if d.static_instance));
            if t == 19999 {
                let ensign = match &w.entities.by_index(hut as u32).unwrap().data { EntityData::Building(d) => d.ensign, _ => unreachable!() };
                let eco_of = |x: i32, y: i32| w.grid.checked_index(x, y).map(|m| w.tiling.eco_sector_of(m));
                println!("  tree at ({x},{y}) spiral {k}: flags {:#x} deco {id} eco(tree+1,1) {:?} hut eco {:?} owner(tree+1,1) {:?}", w.layers.flags[n], eco_of(x + 1, y + 1), eco_of(ensign.x(), ensign.y()), w.grid.checked_index(x + 1, y + 1).map(|m| w.owner_of(m)));
            }
            if w.layers.flags[n] & 0x24 != 0 {
                flagged += 1;
            } else if st {
                takeable += 1;
            }
        }
        let r = &w.buildings.roles[&hut];
        let worker = r.base.inhabitant;
        let j = match w.entities.by_index(worker as u32).map(|e| &e.data) {
            Some(EntityData::Settler(d)) => format!("list {} cursor {} task {} registered {}", d.job.list_id, d.job.cursor, d.job.task, w.entities.by_index(worker as u32).unwrap().flags & 0x2000_0000 != 0),
            _ => "none".into(),
        };
        println!("t{} trees {trees} takeable {takeable} flagged {flagged}; hut {:?} in_house {}; worker {worker}: {j}", t + 1, r.kind, r.base.worker_in_house);
    }
}
