//! Finished buildings a script places: `Buildings.AddBuilding` (Lua 0x5c4ae0) and
//! `Buildings.AddBuildingEx` (0x5c4d70) both call 0x53c580 `(x, y, player, type, -, r)`.
//! Decompile in `tools/re/decompiled/script/tutorial/addbuilding.c` / `buildbuilding.c`.

use crate::World;

/// 0x53c580: along the spiral around `(x, y)` (entries `0..last(r)`, `r` −1 → 0x18, at most
/// 0x4a) the first node on the map where `CheckForBuild(x, y, player, type, mode)` (0x53e4e0)
/// returns anything but 0 gets the finished building (`AddBuilding(…, 0)` 0x53c350). As in the
/// release, an impossible place (−1) counts as well: only "cannot level" (0) moves on.
///
/// `mode` is 2 for buildings whose kind (buildingInfo `kind`, info +0x1dd of race 0) is
/// `HOUSE_KIND_MILITARY` (10) or `HOUSE_KIND_CASTLE` (15): the check then wants unowned land
/// (owner 0). Those also get a level-1 swordsman (`AddSoldierToStartTower(b, 0)`, 0x53dab0).
/// Returns the building, 0 if none.
pub fn add_building_scripted(world: &mut World, x: i32, y: i32, player: u8, building: u8, radius: i32) -> u16 {
    let r = if radius < 0 { 0x18 } else { radius.min(0x4a) } as u8;
    let count = crate::spiral::last(r);
    let military = matches!(world.rules.building(0, building).house_kind, 10 | 15);
    let owner = if military { 0 } else { player };
    if count == 0 {
        return 0;
    }
    let size = world.grid.size() as i32;
    let at = (0..count).map(|k| (x + crate::spiral::dx(k), y + crate::spiral::dy(k))).find(|&(nx, ny)| {
        (0..size).contains(&nx) && (0..size).contains(&ny) && super::check::check(world, nx, ny, player, owner, building).0.code() != 0
    });
    let Some((nx, ny)) = at else { return 0 };
    let Ok(id) = crate::create::add_building(world, nx, ny, player, building) else { return 0 };
    if military {
        crate::create::add_soldier_to_start_tower(world, id, 0);
    }
    id
}
