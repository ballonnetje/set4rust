//! `CFleeRole` (role 18, release vtable 0x105afc0): a non-warrior on land that is not its
//! owner's. It gets the role when it is set free there (`ISettlerRole::SetFree` 0x587245: `new
//! CFleeRole` + `CSettler::NewRole` 0x580c20) or when a carrier is made there (`AddSettler`
//! 0x582693). Decompiles in `tools/re/decompiled/script/flee/`.
//!
//! Each update (`LogicUpdate` 0x577ba0, which replaces the base one):
//! * back on its owner's land, it counts into the eco-sector's settlers of its type again, gets
//!   its type's normal role (`CreateSettlerRole`), its queued events dropped, is offered to the
//!   eco-sector (unless OFFERED) and takes its wait list;
//! * while walking it steps (`CSettler::WalkDir` of the walker's next step): 1 tick to retry a
//!   blocked step, else 9;
//! * otherwise it picks a new target, at most 5 times (+6, from 1): the nearest node of its
//!   owner on its sector (spiral up to radius 50, 0x5b1b70), else one of 64 tries at a random
//!   spiral entry 0..255 on its sector (game RNG, walk flag 0x2000). With no target, or after
//!   the fifth, it is killed.

use super::core::{self, job, job_mut, register};
use super::walking::{ActivePathing, Pathing};
use crate::World;
use crate::entity::flag;
use crate::grid::Y16X16;

/// `CFleeRole::Init` (0x577ad0) with `InitWalking` (0x577b40, a `CWalkingNormal`): the war map,
/// the wait list, the first update next tick, try counter 1. (Init also calls `InitWalking` and
/// drops the walker it makes.)
pub fn init_role(world: &mut World, i: u16) {
    ActivePathing::create(world, i, crate::pathing::walking::walking_type::NORMAL);
    ActivePathing::walk_to_xy(world, i, Y16X16(u32::MAX), 0);
    super::hooks::war_map_add(world, i);
    core::take_wait_list(world, i);
    register(world, i, 1);
    if let Some(j) = job_mut(world, i) {
        j.counter = 1;
    }
}

/// `CFleeRole::TakeJob` (0x577e90): the common task values; WAIT has displacement costs 0.
pub fn take_job(world: &mut World, i: u16) {
    let Some(t) = core::actual_task(world, i) else { return };
    core::init_common_task_values(world, i, &t);
    if t.task == super::task::task_type::WAIT
        && let Some(j) = job_mut(world, i)
    {
        j.displacement_costs = 0;
    }
}

/// `CFleeRole::ConvertEventIntoGoal` (0x577ef0): only makes sure the settler is updated.
pub fn convert_event_into_goal(world: &mut World, i: u16) {
    if world.entities.by_index(i as u32).is_some_and(|e| e.flags & flag::REGISTERED == 0) {
        register(world, i, 1);
    }
}

/// 0x5b1b70: the first node in the spiral up to radius 50 around (x, y) on the same sector that
/// belongs to `owner` (sector 0: none).
fn own_land_near(world: &World, owner: u8, x: i32, y: i32) -> Option<(i32, i32)> {
    let sector = world.tiling.sector_of(world.grid.checked_index(x, y)?);
    if sector == 0 {
        return None;
    }
    crate::spiral::walk(world.grid, x, y, 50).find(|&(px, py)| {
        let n = world.grid.index(px as u32, py as u32);
        world.tiling.sector_of(n) == sector && world.owner_of(n) == owner
    })
}

/// `CFleeRole::LogicUpdate` (0x577ba0).
pub fn logic_update(world: &mut World, i: u16) {
    let Some(e) = world.entities.by_index(i as u32) else { return };
    let (pos, owner, race, kind) = (e.pos, e.owner, e.race, e.kind);
    let Some(n) = world.grid.checked_index(pos.x(), pos.y()) else { return };
    if world.owner_of(n) == owner {
        let eco = crate::eco::eco_of(world, n);
        crate::eco::change_nr_of_settler(world, eco, kind, 1);
        let r = super::roles::create_settler_role(world, world.rules.jobs.settler_role(race, kind));
        if let Some(j) = job_mut(world, i) {
            j.events.clear();
        }
        set_role(world, i, r);
        // 0x577c78 tests OFFERED after `NewRole` (0x577c73): the new role's `Init` may have
        // offered it already (a carrier's does); offering it twice links it to itself.
        if world.entities.by_index(i as u32).is_some_and(|e| e.flags & flag::OFFERED == 0) {
            crate::eco::set_settler_offer(world, eco, kind, i);
        }
        core::take_wait_list(world, i);
        return;
    }
    // Dead: −1; no walker: 0x48 (`CSettler::Walk`).
    let w = core::settler_walk(world, i) as u8;
    if let Some(j) = job_mut(world, i) {
        j.walk = w;
    }
    if w & 0x70 == 0 {
        register(world, i, if w & 8 != 0 { 1 } else { 9 });
        return;
    }
    let tries = job(world, i).map_or(5, |j| j.counter);
    if tries < 5 {
        let target = match own_land_near(world, owner, pos.x(), pos.y()) {
            Some(t) => Some((t, 0)),
            None => {
                let sector = world.tiling.sector_of(n);
                let mut found = None;
                for _ in 0..64 {
                    let k = (world.rng.rand().wrapping_sub(0x80) & 0xff) as usize;
                    let (x, y) = (pos.x() + crate::spiral::dx(k), pos.y() + crate::spiral::dy(k));
                    if world.grid.in_world(x, y) && world.tiling.sector_of(world.grid.index(x as u32, y as u32)) == sector {
                        found = Some(((x, y), 0x2000));
                        break;
                    }
                }
                found
            }
        };
        if let Some(((x, y), flags)) = target {
            if let Some(j) = job_mut(world, i) {
                j.frame = 1;
            }
            ActivePathing::walk_to_xy(world, i, Y16X16::pack(x, y), flags);
            if let Some(j) = job_mut(world, i) {
                j.displacement_costs = 5;
            }
            register(world, i, 1);
            if let Some(j) = job_mut(world, i) {
                j.counter += 1;
            }
            return;
        }
    }
    world.kill(i, 0);
}

/// `CSettler::NewRole(role)` (0x580c20) for a living settler: out of all groups, the new role's
/// `Init` and walker.
fn set_role(world: &mut World, i: u16, r: super::roles::NewRole) {
    if world.entities.by_index(i as u32).is_some_and(|e| e.flags & flag::DIED != 0) {
        return;
    }
    crate::group::remove_all(world, i);
    super::roles::set_role(world, i, r, 0);
}

/// `ISettlerRole::SetFree` on foreign land (0x587245): the settler becomes a fleeing one.
pub fn start(world: &mut World, i: u16) {
    let r = super::roles::create_settler_role(world, super::state::role::FLEE);
    set_role(world, i, r);
}
