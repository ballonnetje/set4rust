//! `CBuildingMgr`'s per-(player, type) lists (`World::building_list`): a new building goes to the
//! head (`AddBuilding` 0x53c350), a deleted one leaves (`CheckOutBuilding` 0x53c680). Entity ids
//! are reused, so the head is the newest building, not the highest id — the 123 savegames' chunk
//! 162 heads show it (3033 of 90774 heads are not the highest id of their list). Skipped without
//! the game data.

#[test]
fn new_buildings_at_the_head_deleted_ones_gone() {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("roman01.map");
    let Ok(map) = s4_formats::map::MapFile::open(&p) else { return };
    let (mut w, _) = s4_sim::new_game(&map, s4_sim::Rules::load_default().unwrap(), &Default::default()).unwrap();
    // Every building of the map is listed once, in the list of its owner and type.
    let all: Vec<(u16, u8, u16)> = w.entities.iter().filter(|(_, e)| e.obj_type == s4_sim::entity::ObjType::Building).map(|(i, e)| (i, e.owner, e.kind)).collect();
    for &(id, owner, kind) in &all {
        assert_eq!(w.building_list(owner as i32, kind as i32).iter().filter(|&&b| b == id).count(), 1, "building {id}");
    }
    // A list with two or more: crush its last (oldest) building, then add one of that type.
    let (owner, kind) = all.iter().map(|&(_, o, k)| (o, k)).find(|&(o, k)| w.building_list(o as i32, k as i32).len() >= 2).expect("a list of two");
    let list = w.building_list(owner as i32, kind as i32);
    let old = *list.last().unwrap();
    s4_sim::buildings::crush::crush_building(&mut w, old);
    for _ in 0..50 {
        s4_sim::tick(&mut w, &[]);
    }
    assert!(!w.building_list(owner as i32, kind as i32).contains(&old), "the crushed building left its list");
    let pos = w.entities.by_index(list[0] as u32).unwrap().pos;
    let type_ex = match &w.entities.by_index(list[0] as u32).unwrap().data {
        s4_sim::entity::EntityData::Building(b) => b.type_ex,
        _ => unreachable!(),
    };
    let new = s4_sim::create::add_building(&mut w, pos.x() + 30, pos.y() + 30, owner, type_ex).expect("placed");
    let after = w.building_list(owner as i32, kind as i32);
    assert_eq!(after[0], new, "the new building {new} is the head");
    assert_eq!(&after[1..], &list[..list.len() - 1], "the others keep their order");
}

/// The `CBuilding` ctor (0x535d24) puts a construction site into the war map's building lists
/// (category 2) like a finished building; it stays there when finished and leaves when crushed.
#[test]
fn sites_are_in_the_war_map() {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("roman01.map");
    let Ok(map) = s4_formats::map::MapFile::open(&p) else { return };
    let (mut w, _) = s4_sim::new_game(&map, s4_sim::Rules::load_default().unwrap(), &Default::default()).unwrap();
    let b = w.entities.iter().find(|(_, e)| e.obj_type == s4_sim::entity::ObjType::Building && e.owner == 1).map(|(_, e)| e.pos).unwrap();
    let site = (0..400)
        .find_map(|k| s4_sim::build::site::add_building_site(&mut w, b.x() + 10 + k % 20, b.y() + 10 + k / 20, 1, 1))
        .expect("a site");
    let pos = w.entities.by_index(site as u32).unwrap().pos;
    let listed = |w: &s4_sim::World| w.warmap.cell((pos.x() >> 4) as usize, (pos.y() >> 4) as usize, 2).any(|id| id == site);
    assert!(listed(&w), "the site {site} is in its cell's building list");
    s4_sim::warmap::check(&w).unwrap();
    s4_sim::buildings::crush::crush_building(&mut w, site);
    for _ in 0..50 {
        s4_sim::tick(&mut w, &[]);
    }
    assert!(!listed(&w), "the crushed site left the war map");
    s4_sim::warmap::check(&w).unwrap();
}
