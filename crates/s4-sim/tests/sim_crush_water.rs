//! Ports and shipyards on water (`CBuilding::SetToWorld` 0x536470 / `RemoveFromWorld` 0x536d00):
//! the info +0x314 cells (`waterPosLines | waterBlockPosLines`) become blocked water when the
//! building is placed and are freed when it is crushed. Singleplayer/Harbour.map has a player-2
//! shipyard (type 77) at (231, 107). Skipped without the game data.

use s4_sim::entity::{EntityData, ObjType};
use s4_sim::{Rules, World};

fn harbour() -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Singleplayer").join("Harbour.map");
    if !p.exists() {
        return None;
    }
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    let rules = Rules::load_default().ok()?;
    let (w, _) = s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).ok()?;
    Some(w)
}

fn blocked_water(w: &World, cells: &[(i32, i32)]) -> usize {
    cells.iter().filter(|&&(x, y)| w.tiling.water_flags[w.grid.index(x as u32, y as u32)] & 0x200 != 0).count()
}

#[test]
fn a_shipyard_blocks_its_water_until_crushed() {
    let Some(mut w) = harbour() else { return };
    let (id, e) = w.entities.iter().find(|(_, e)| e.obj_type == ObjType::Building && (e.x(), e.y()) == (231, 107)).expect("shipyard");
    let EntityData::Building(d) = &e.data else { panic!() };
    let rule = w.rules.building(e.race, d.type_ex).clone();
    assert!(rule.has_water_masks && !rule.water_block_cells.is_empty(), "type {} has no water masks", d.type_ex);
    let cells: Vec<(i32, i32)> = rule.water_block_cells.iter().map(|&(dx, dy)| (231 + dx as i32, 107 + dy as i32)).collect();
    // Only water nodes that are not already blocked water for another reason can take the bit
    // (`IsBlockedWater`: `(flags & 0xc300) == 0xc000`).
    let eligible = cells.iter().filter(|&&(x, y)| w.tiling.water_flags[w.grid.index(x as u32, y as u32)] >= 0xc000).count();
    let before = blocked_water(&w, &cells);
    eprintln!("shipyard {id} type {}: {} water cells, {eligible} on water, {before} blocked", d.type_ex, cells.len());
    assert!(before > 0, "SetToWorld blocked no water");
    s4_sim::buildings::crush::crush_building(&mut w, id);
    assert_eq!(blocked_water(&w, &cells), 0, "RemoveFromWorld left blocked water");
}
