//! `CSlaveRole` (role 17) on dark03, driven by `tick()`: a new slave is sent by the dark temple
//! (0x545e10, free slaves only) to a mushroom farm (event 1), walks to its working area and works
//! next to ripe mushrooms on a node it reserves (0x588730: node flag 0x20). Skipped without the
//! game data.

use s4_sim::World;
use s4_sim::entity::{EntityData, ObjType};
use s4_sim::jobs::role;

const SLAVE: u16 = 0x37;
const WORK: u8 = 16;

fn map(name: &str) -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join(name);
    if !p.exists() {
        return None;
    }
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    let rules = s4_sim::Rules::load_default().ok()?;
    s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).ok().map(|(w, _)| w)
}

fn job(w: &World, i: u16) -> &s4_sim::jobs::JobState {
    match &w.entities.by_index(i as u32).unwrap().data {
        EntityData::Settler(s) => &s.job,
        _ => panic!("not a settler"),
    }
}

#[test]
fn a_slave_goes_to_a_farm_and_works() {
    let Some(mut w) = map("dark03.map") else { return };
    let (temple, dt) = w
        .buildings
        .roles
        .iter()
        .find(|(_, r)| matches!(r.kind, s4_sim::buildings::RoleKind::DarkTemple(_)))
        .map(|(&id, _)| (id, w.entities.by_index(id as u32).unwrap().owner))
        .expect("a dark temple");
    let exit = s4_sim::buildings::dark_temple::get(&w, temple).unwrap().exit;
    // Ripe mushrooms (shared static instances of type 212) on free dark land around every farm's
    // working area, as a grown farm has them.
    let farms: Vec<s4_sim::grid::Y16X16> = w
        .entities
        .iter()
        .filter(|(_, e)| e.obj_type == ObjType::Building && e.owner == dt && e.kind == 49)
        .filter_map(|(_, e)| match &e.data {
            EntityData::Building(d) => Some(d.working_area),
            _ => None,
        })
        .collect();
    let mut planted = 0;
    for a in farms {
        for (x, y) in s4_sim::spiral::walk(w.grid, a.x(), a.y(), 6).step_by(5) {
            let n = w.grid.index(x as u32, y as u32);
            let free = w.layers.flags[n] & 0x7f == 4 && w.layers.map_object[n] == 0 && w.layers.deco_object[n] == 0;
            if free && s4_sim::create::add_deco_obj_without_flags(&mut w, x, y, 212, 0).is_ok() && s4_sim::buildings::search::run(&mut w, 14, x, y) != 0 {
                planted += 1;
            }
        }
    }
    assert!(planted > 0, "no ripe mushroom could be placed");
    let s = s4_sim::buildings::dark_temple::make_settler(&mut w, temple, dt, SLAVE, 0);
    assert!(s > 0, "no slave");
    assert_eq!(job(&w, s).role, role::SLAVE);
    assert!(s4_sim::jobs::slave::is_free(&w, s));
    let (mut farm, mut worked, mut reserved) = (0, None, false);
    for t in 0..3000 {
        s4_sim::tick(&mut w, &[]);
        let j = job(&w, s);
        if farm == 0 && j.home != 0 {
            farm = j.home;
        }
        if j.slave.spot.0 >= 0 {
            let n = w.grid.index(j.slave.spot.0 as u32, j.slave.spot.1 as u32);
            reserved |= w.layers.flags[n] & 0x20 != 0;
        }
        if j.task == WORK && worked.is_none() {
            worked = Some(t);
        }
        if worked.is_some() && t > worked.unwrap() + 200 {
            break;
        }
    }
    if farm != 0 {
        let (area, kind) = match &w.entities.by_index(farm as u32).unwrap().data {
            EntityData::Building(d) => (d.working_area, w.entities.by_index(farm as u32).unwrap().kind),
            _ => unreachable!(),
        };
        let mut ripe = 0;
        let mut any = 0;
        for (x, y) in s4_sim::spiral::walk(w.grid, area.x(), area.y(), 20) {
            let id = w.layers.deco_object[w.grid.index(x as u32, y as u32)];
            if let Some(e) = w.entities.by_index(id as u32) && (212..=214).contains(&e.kind) {
                any += 1;
                if e.flags & 0x200_0000 != 0 && matches!(&e.data, EntityData::Deco(d) if d.static_instance) { ripe += 1; }
            }
        }
        let j = job(&w, s);
        eprintln!("farm kind {kind} area {area:?}: mushrooms {any}, ripe static {ripe}; slave task {} cursor {} list {} spot {:?}", j.task, j.slave.cursor, j.list_id, j.slave.spot);
    }
    let e = w.entities.by_index(s as u32).unwrap();
    eprintln!("slave {s} from {exit:#x}: farm {farm}, first WORK at {worked:?}, reserved {reserved}, now at {:?}", e.pos);
    assert!(!e.is_dead());
    assert_ne!(farm, 0, "the temple sent the slave to no farm");
    assert_eq!(w.entities.by_index(farm as u32).unwrap().obj_type, ObjType::Building);
    assert!(!s4_sim::jobs::slave::is_free(&w, s));
    assert!(worked.is_some(), "the slave never worked");
    assert!(reserved, "no work node was reserved");
}
