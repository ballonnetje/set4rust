//! `COwnerMapEx` (`Tiling::owner_map`): the full calculation at load (0x5aff50) against the
//! incremental updates of tile creation (0x5b00a0) and deletion (0x5b0130) while land changes
//! hands. Byte 0 must always equal a fresh calculation; byte 1 only grows, so it holds at least
//! the fresh one. roman02, skipped without the game data.

use s4_sim::World;

fn roman02() -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map");
    if !p.exists() {
        return None;
    }
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    let rules = s4_sim::Rules::load_default().ok()?;
    s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).ok().map(|(w, _)| w)
}

fn check(w: &World, exact: bool) -> usize {
    let side = (w.grid.size() / 16) as usize;
    let mut fresh = w.tiling.clone();
    fresh.calc_owner_map(side);
    let mut grown = 0;
    for sy in 0..side as i32 {
        for sx in 0..side as i32 {
            let (a, b) = (w.tiling.owner_bits(sx, sy), fresh.owner_bits(sx, sy));
            assert_eq!(a[0], b[0], "byte 0 of square ({sx}, {sy})");
            assert_eq!(a[1] & b[1], b[1], "byte 1 of square ({sx}, {sy}) lost an owner");
            if exact {
                assert_eq!(a[1], b[1], "byte 1 of square ({sx}, {sy})");
            }
            grown += (a[1] != b[1]) as usize;
        }
    }
    grown
}

#[test]
fn owner_map_follows_the_tiles() {
    let Some(mut w) = roman02() else { return };
    check(&w, false);
    // Player 1's land around its castle changes hands, then becomes neutral.
    let castle = w
        .entities
        .iter()
        .find(|(id, e)| e.owner == 1 && e.obj_type == s4_sim::entity::ObjType::Building && s4_sim::buildings::have_inhabitant(&w, *id))
        .map(|(_, e)| (e.x(), e.y()))
        .expect("castle");
    let nodes: Vec<(i32, i32)> = (0..=s4_sim::spiral::last(24)).map(|k| (castle.0 + s4_sim::spiral::dx(k), castle.1 + s4_sim::spiral::dy(k))).collect();
    for &(x, y) in &nodes {
        w.change_owner(x, y, 2);
    }
    check(&w, false);
    for &(x, y) in &nodes {
        w.change_owner(x, y, 0);
    }
    let grown = check(&w, false);
    eprintln!("{grown} squares keep an owner in byte 1 that a fresh calculation drops");
    assert!(grown > 0, "byte 1 should remember player 2 next to the squares it left");
}
