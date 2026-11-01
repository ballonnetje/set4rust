//! The scripts' vehicle calls (`Vehicles.*`, `Settlers.AddSettlersToFerry`), decompiles in
//! `tools/re/missions/lua1.c` and `cargo.c`. The Lua bindings read their arguments as
//! `(int)lua_getnumber` (the host's `Args::int`); these are the parts behind them.

use super::{ferry, kind};
use crate::World;
use crate::entity::{ObjType, flag};

/// `CStdCargoCounter`'s level class per settler type (.bss 0x12dbc58, filled by 0x581f20 from the
/// records at 0x101a649): 1..=3 the level of swordsmen, bowmen, medics, axe / blowgun warriors
/// and backpack catapultists, 4 the squad leader, 0 the rest.
pub fn level_class(settler: u16) -> u8 {
    match settler {
        29 | 32 | 35 | 38 | 41 | 61 => 1,
        30 | 33 | 36 | 39 | 42 | 62 => 2,
        31 | 34 | 37 | 40 | 43 | 63 => 3,
        44 => 4,
        _ => 0,
    }
}

/// `CVehicle` vtbl+0x9c (0x594c30) `(n)`: `n` > 0 more ammunition (+0x63), at most `MaxAmmo`
/// (info +0x54). The Vikings' catapult (0x560840) and war ship (0x592050) shoot mana: nothing.
pub fn add_ammo(world: &mut World, id: u16, n: i32) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let (race, k) = (e.race, e.kind);
    if n <= 0 || (race == 1 && matches!(k, kind::WAR_MACHINE | kind::WARSHIP)) {
        return;
    }
    let max = world.rules.vehicles.get(race, k as u8).max_ammo;
    if let Some(v) = super::get_mut(world, id) {
        let sum = v.cargo as i32 + n;
        v.cargo = if (sum as u32) <= (max as u32) { sum as u8 } else { max as u8 };
    }
}

/// `Vehicles.AddVehicle(x, y, player, type, dir, ammo, ai)` (0x5c1280): on the map, a player 1..=8
/// with a team, type 1..=5 (0x406, the filled cart, counts as 5) that can stand there (0x597020):
/// `AddVehicle` (0x596ad0, finished), then vtbl+0x9c `(ammo)`; `ai` = 1 tells the AI (event
/// 0x19 `(player, id, 1)`). Returns the id, 0 for none.
#[allow(clippy::too_many_arguments)]
pub fn add_vehicle(world: &mut World, x: i32, y: i32, player: i32, vtype: i32, dir: i32, ammo: i32, ai: i32) -> u16 {
    let k = if vtype == kind::TRADE_CART as i32 { kind::CART as i32 } else { vtype };
    let size = world.grid.size();
    if (x as u32) >= size || (y as u32) >= size || !(1..=8).contains(&player) || world.alliances.team(player as u8) == 0 {
        return 0;
    }
    if !(1..=5).contains(&k) || !super::can_place(world, x, y, k as u16) {
        return 0;
    }
    let Some(id) = super::add_vehicle(world, x, y, player as u8, vtype as u16, dir, false) else { return 0 };
    add_ammo(world, id, ammo);
    if ai == 1 {
        crate::ai::post_event(world, 0x19, player, id as i32, 1);
    }
    id
}

/// `Vehicles.KillVehicles(player, type, x, y, radius, effects)` (0x5c22a0): a player with a team,
/// type 1..=5, the centre on the map, radius 0..=50: every READY ship / land vehicle of the player
/// and of exactly that type on a node of the circle (the map-object layer, spiral order [inf] —
/// the original walks 0x4312e0 with the count table 0x117cf3a) dies (`Kill(id, 0)`); with
/// `effects` 0 it is made invisible first.
pub fn kill_vehicles(world: &mut World, player: i32, vtype: i32, x: i32, y: i32, radius: i32, effects: i32) {
    let size = world.grid.size();
    if !(1..=8).contains(&player) || world.alliances.team(player as u8) == 0 || !(1..=5).contains(&vtype) {
        return;
    }
    if (x as u32) >= size || (y as u32) >= size || !(0..=50).contains(&radius) {
        return;
    }
    let nodes: Vec<(i32, i32)> = crate::spiral::walk(world.grid, x, y, radius as u8).collect();
    for (px, py) in nodes {
        let Some(n) = world.grid.checked_index(px, py) else { continue };
        let id = world.layers.map_object[n];
        if id == 0 {
            continue;
        }
        let Some(e) = world.entities.by_index_mut(id as u32) else { continue };
        if e.flags & flag::READY == 0 || e.owner as i32 != player || !matches!(e.obj_type, ObjType::Ship | ObjType::LandVehicle) || e.kind as i32 != vtype {
            continue;
        }
        if effects == 0 {
            e.flags &= !flag::VISIBLE;
        }
        world.kill(id, 0);
    }
}

/// `Settlers.AddSettlersToFerry(ferry, type, amount)` (0x5bffc0): a ferry, a warrior type
/// (0x12dbca0, type 1..=0x42): at least one, while there is room (0x58eb00) a new settler of the
/// ferry's owner at (1, 1) (`AddSettler` 0x582600) goes aboard (0x58e2a0); a failure ends it.
pub fn add_settlers_to_ferry(world: &mut World, ferry_id: i32, settler: i32, amount: i32) {
    let Ok(id) = u16::try_from(ferry_id) else { return };
    if !ferry::is_ferry(world, id) || !(1..=0x42).contains(&settler) || world.rules.warrior_type(settler as u16) == 0 {
        return;
    }
    let Some(owner) = world.entities.by_index(id as u32).map(|e| e.owner) else { return };
    let amount = amount.max(1);
    let mut n = 0;
    while ferry::room(world, id, ObjType::Settler as u8, settler as u16) {
        let s = crate::create::add_settler(world, 1, 1, owner, settler as u16).unwrap_or(0);
        if !ferry::load_direct(world, id, s) {
            return;
        }
        n += 1;
        if n >= amount {
            return;
        }
    }
}

/// `Vehicles.AddWheelerToFerry(ferry, type)` (0x5c1600): a ferry, type 4 or 0x406 (the filled
/// cart), five free in both weights (+0xac / +0xb0 against +0xb4): a vehicle of the ferry's owner
/// made at (−1, 0) (0x596ad0, off the map) goes aboard (0x58e2a0).
pub fn add_wheeler_to_ferry(world: &mut World, ferry_id: i32, vtype: i32) {
    let Ok(id) = u16::try_from(ferry_id) else { return };
    if !matches!(vtype, 4 | 0x406) {
        return;
    }
    let Some(f) = ferry::data(world, id) else { return };
    if f.aboard + 5 > ferry::CAPACITY || f.reserved + 5 > ferry::CAPACITY {
        return;
    }
    let Some(owner) = world.entities.by_index(id as u32).map(|e| e.owner) else { return };
    let v = super::add_vehicle(world, -1, 0, owner, vtype as u16, 0, false).unwrap_or(0);
    ferry::load_direct(world, id, v);
}

/// `Vehicles.GetFerryCargoInArea(player, x, y, radius)` (0x5c1e80): over the player's ferries
/// (list 0x597480) within `radius` (hex distance of their node), their passengers that are alive
/// (vtbl+0xb4 0x58edf0 → `CStdCargoCounter` 0x595830): the civilians (level class 0), the
/// soldiers of level 1, 2, 3, the squad leaders, and the land vehicles of type 5. A player with a
/// team, the centre on the map and `radius` > 0, else all 0.
pub fn ferry_cargo_in_area(world: &World, player: i32, x: i32, y: i32, radius: i32) -> [i32; 6] {
    let mut out = [0; 6];
    let size = world.grid.size();
    if !(1..=8).contains(&player) || world.alliances.team(player as u8) == 0 || (x as u32) >= size || (y as u32) >= size || radius <= 0 {
        return out;
    }
    for id in super::iter(world, player as u8, kind::FERRY) {
        let Some(e) = world.entities.by_index(id as u32) else { continue };
        if crate::spiral::grid_distance(e.x() - x, e.y() - y) > radius {
            continue;
        }
        let Some(f) = ferry::data(world, id) else { continue };
        for p in &f.passengers {
            let Some(pe) = world.entities.by_index(p.id as u32) else { continue };
            if pe.flags & flag::ALIVE_MASK == 0 {
                continue;
            }
            match pe.obj_type {
                ObjType::Settler => match level_class(pe.kind) {
                    0 => out[0] += 1,
                    c => out[c as usize] += 1,
                },
                ObjType::LandVehicle if pe.kind == 5 => out[5] += 1,
                _ => {}
            }
        }
    }
    out
}

/// `Vehicles.GetHealth(x, y)` (0x5c27b0): the hit points (+0x21) of the ship / land vehicle whose
/// node is exactly `(x, y)` (the map-object layer), else −1.
pub fn health_at(world: &World, x: i32, y: i32) -> i32 {
    let Some(n) = world.grid.checked_index(x, y) else { return -1 };
    let id = world.layers.map_object[n];
    match world.entities.by_index(id as u32) {
        Some(e) if id != 0 && matches!(e.obj_type, ObjType::Ship | ObjType::LandVehicle) && (e.x(), e.y()) == (x, y) => super::get(world, id).map_or(-1, |v| v.health as i32),
        _ => -1,
    }
}
