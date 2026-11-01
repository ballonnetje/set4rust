//! Light land under a Dark Tribe building destroys it (`CDarkTribe` 0x4cbb60 → 0x4cbc15
//! `DestroyBuilding(x, y, player)` 0x53ce10 → 0x536a80 with the player as the cause); a mushroom
//! farm destroyed so counts for that player (`CDarkMushroomFarmRole` vtbl+0x2c 0x545050,
//! `Statistic.MushroomFarmsDestroyed`). dark04's farms of player 3, lightened by player 1.
//! Skipped without the game data.

use s4_sim::entity::ObjType;

#[test]
fn lightened_mushroom_farm_is_destroyed_and_counted() {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("dark04.map");
    let (Ok(map), Ok(rules)) = (s4_formats::map::MapFile::open(&p), s4_sim::Rules::load_default()) else { return };
    let (mut w, _) = s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).unwrap();
    let farms: Vec<(u16, i32, i32)> = w.entities.iter().filter(|(_, e)| e.obj_type == ObjType::Building && e.kind == 49 && e.race == 3).map(|(i, e)| (i, e.x(), e.y())).collect();
    assert!(!farms.is_empty(), "dark04 has mushroom farms");
    let (id, x, y) = farms[0];
    // Every dark node within 6, lightened by player 1 (a gardener's WORK end).
    let nodes: Vec<(i32, i32)> = s4_sim::spiral::walk(w.grid, x, y, 6).collect();
    for (px, py) in nodes {
        if w.grid.checked_index(px, py).is_some_and(|n| w.layers.flags[n] & 4 != 0) {
            s4_sim::dark::lighten(&mut w, px, py, 1);
        }
    }
    assert!(w.entities.by_index(id as u32).is_none_or(|e| e.is_dead()), "the farm is destroyed");
    assert_eq!(w.statistic.players[1].mushroom_farms_destroyed, 1);
    assert_eq!(w.statistic.players[2].mushroom_farms_destroyed + w.statistic.players[3].mushroom_farms_destroyed, 0);
}
