//! Lists a map's buildings with their entity ids, owners and types, and the military
//! buildings' garrison (`buildings::military`): `cargo run -p s4-sim --example
//! sim_military_probe -- [MAP]` (default Campaign/roman02).
use s4_sim::entity::{EntityData, ObjType};

fn main() {
    let name = std::env::args().nth(1).unwrap_or_else(|| "Campaign/roman02".into());
    let p = s4_formats::game_dir().join("Map").join(format!("{name}.map"));
    let map = s4_formats::map::MapFile::open(&p).expect("map");
    let rules = s4_sim::Rules::load_default().expect("rules");
    let (w, _) = s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).expect("load");
    for (id, e) in w.entities.iter() {
        if e.obj_type != ObjType::Building {
            continue;
        }
        let EntityData::Building(d) = &e.data else { continue };
        let garrison = s4_sim::buildings::military::state(&w, id).map(|s| format!("door {} in {}/{} wish {}/{} slots {:?}", s.door, s.bow_in, s.sword_in, s.bow_wish, s.sword_wish, s.slots));
        println!("{id}: type {} owner {} at ({}, {}) health {} {}", d.type_ex, e.owner, e.x(), e.y(), d.health, garrison.unwrap_or_default());
    }
}
