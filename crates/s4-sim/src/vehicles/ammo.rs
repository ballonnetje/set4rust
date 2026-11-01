//! Ammunition brought to a catapult (command 5010 `CatapultLoad` → `RequestAmmoDonkey` 0x560d80;
//! decompiles `tools/re/vehicles/catapult.c`, `catmisc.c`, `foundation.c`).
//!
//! The nearest pile of the ammunition good (info +0x50) offered in an eco-sector of the owner on
//! the catapult's sector (0x4dfc80); a transporter for it (0x4fa280): the nearest free donkey of
//! the owner (0x4fa050, carries up to 8 goods), else — the pile and the catapult in one
//! eco-sector — the nearest offered carrier there (0x4d4af0, out of the offer; one good, the
//! pile reserves it and the catapult attaches it). The transporter gets event 11 `[goods,
//! pile, catapult]` and becomes +0xbc. A carrier runs list 153 (`jobs::carrier`), a donkey list
//! 164 (`jobs::donkey`); both unload with vtbl+0x84 (0x560f50).
//!
//! While it comes, the catapult's moves move it too: a move order points a donkey at the new
//! place (0x576eb0) and sends a carrier away (event 9, 0x560f80); the end of a behaviour state
//! points either at the catapult's new place behind it (0x561400).

use super::{catapult, get};
use crate::World;
use crate::entity::ObjType;
use crate::grid::Y16X16;
use crate::jobs::state::EntityEvent;

fn pos_of(world: &World, id: u16) -> Y16X16 {
    world.entities.by_index(id as u32).map_or(Y16X16(0), |e| e.pos)
}

fn eco_at(world: &World, p: Y16X16) -> u16 {
    world.grid.checked_index(p.x(), p.y()).map_or(0, |n| crate::eco::eco_of(world, n))
}

/// 0x4dfc80 `(player, x, y, good)`: over the eco-sectors of `player` (in id order), the piles
/// offering `good` on the sector of (x, y), the nearest (first on ties) by hex distance.
fn nearest_pile(world: &World, player: u8, x: i32, y: i32, good: i32) -> u16 {
    let Some(n) = world.grid.checked_index(x, y) else { return 0 };
    let sector = world.tiling.sector_of(n);
    let (mut best, mut best_d) = (0u16, 0xffff);
    for (eco, s) in world.economy.sectors.iter().enumerate() {
        let Some(s) = s else { continue };
        if s.owner != player || eco == 0 {
            continue;
        }
        let mut p = s.good_offer.get(good as usize).copied().unwrap_or(0);
        while p != 0 {
            let Some(e) = world.entities.by_index(p as u32) else { break };
            if e.obj_type != ObjType::Pile {
                break;
            }
            if world.grid.checked_index(e.x(), e.y()).is_some_and(|m| world.tiling.sector_of(m) == sector) {
                let d = crate::spiral::grid_distance(x - e.x(), y - e.y());
                if d < best_d {
                    best_d = d;
                    best = p;
                }
            }
            p = e.next;
        }
    }
    best
}

/// `RequestAmmoDonkey` (0x560d80): returns the transporter (0 = none). See the module doc.
pub fn request(world: &mut World, id: u16) -> u16 {
    let Some(e) = world.entities.by_index(id as u32) else { return 0 };
    let (race, k, owner, cp) = (e.race, e.kind, e.owner, e.pos);
    let info = world.rules.vehicles.get(race, k as u8);
    let pile = nearest_pile(world, owner, cp.x(), cp.y(), info.x50);
    if pile == 0 {
        return 0;
    }
    let per = if race == 2 { 8 } else { 4 };
    let amount = crate::pile::get(world, pile).map_or(0, |d| d.amount as i32);
    let cargo = get(world, id).map_or(0, |v| v.cargo) as i32;
    let room = info.max_ammo - cargo;
    let want = if amount * per <= room { amount * per } else { room };
    let pp = pos_of(world, pile);
    let (t, cap) = match crate::jobs::donkey::nearest_free(world, owner, pp.x(), pp.y()) {
        0 => {
            let eco = eco_at(world, pp);
            if eco == 0 || eco != eco_at(world, cp) {
                return 0;
            }
            let c = crate::eco::get_nearest_settler(world, eco, 1, pp.x(), pp.y());
            if c == 0 {
                return 0;
            }
            crate::eco::get_settler_out_of_offer(world, eco, c);
            (c, 1)
        }
        d => (d, 8),
    };
    let n = want.min(per * cap);
    if n <= 0 {
        return 0;
    }
    if world.entities.by_index(t as u32).is_some_and(|e| e.kind == 1) {
        crate::pile::attach_and_inc_amount_leaving(world, pile, t, 1, crate::pile::target::PILE_SRC);
        super::attach(world, id, t);
    }
    crate::jobs::core::set_event(world, t, EntityEvent { event: 11, kind: 0, data: [n / per, pile as i32, id as i32] });
    t
}

/// Command 5010 `CatapultLoad`: a READY catapult without a transporter and below `MaxAmmo` asks
/// for one (+0xbc).
pub fn catapult_load(world: &mut World, id: u16) {
    let ready = world.entities.by_index(id as u32).is_some_and(|e| e.flags & crate::entity::flag::READY != 0);
    if !ready || !catapult::is_catapult(world, id) || catapult::data(world, id).is_none_or(|c| c.carrier != 0) {
        return;
    }
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let max = world.rules.vehicles.get(e.race, e.kind as u8).max_ammo;
    if get(world, id).is_none_or(|v| v.cargo as i32 >= max) {
        return;
    }
    let t = request(world, id);
    catapult::set_carrier(world, id, t as i32);
}

/// 0x560f80's part: a move order to `to` while transporter `t` comes: a donkey heads for `to`
/// (0x576eb0); a carrier gets event 9 `[catapult, its eco-sector]` and is forgotten.
pub fn move_ordered(world: &mut World, id: u16, t: u16, to: Y16X16) {
    let Some(e) = world.entities.by_index(t as u32) else { return };
    match e.kind {
        crate::buildings::trading::DONKEY_SETTLER => crate::jobs::donkey::vehicle_moved(world, t, to),
        1 => {
            let eco = eco_at(world, pos_of(world, id));
            crate::jobs::core::set_event(world, t, EntityEvent { event: 9, kind: 0, data: [id as i32, eco as i32, 0] });
            catapult::set_carrier(world, id, 0);
        }
        _ => {}
    }
}

/// 0x561630's part: the catapult goes onto a ferry; its transporter `t` gets event 9
/// `[catapult, its eco-sector]` and is forgotten.
pub(super) fn release(world: &mut World, id: u16, t: u16) {
    let eco = eco_at(world, pos_of(world, id));
    crate::jobs::core::set_event(world, t, EntityEvent { event: 9, kind: 0, data: [id as i32, eco as i32, 0] });
    catapult::set_carrier(world, id, 0);
}

/// 0x561400's part: the transporter is pointed at the catapult's place behind it: a donkey (role
/// 20) through 0x576eb0, a carrier (role 1) gets it as its destination (and its start position
/// from its own id at role +0x12, as the original).
pub fn retarget(world: &mut World, id: u16) {
    let t = catapult::data(world, id).map_or(0, |c| c.carrier);
    if t <= 0 {
        return;
    }
    let t = t as u16;
    let Some(e) = world.entities.by_index(t as u32) else { return };
    if e.obj_type != ObjType::Settler {
        return;
    }
    let role = crate::jobs::core::job(world, t).map_or(0, |j| j.role);
    let place = Y16X16(catapult::exit_place(world, id) as u32);
    if role == crate::jobs::role::DONKEY {
        crate::jobs::donkey::vehicle_moved(world, t, place);
    }
    if role == crate::jobs::role::CARRIER {
        let me = crate::jobs::core::job(world, t).map_or(0, |j| j.w18);
        let start = pos_of(world, me);
        if let Some(j) = crate::jobs::core::job_mut(world, t) {
            j.start = start;
            j.destination = place;
        }
    }
}
