//! `CBuilderRole` (MapObjects/Settler/BuilderRole.cpp, role id 3): builds a construction site.
//! The site (`build::site`, `CBuildingSiteRole`) calls a builder with event 1 once it has
//! material; the builder takes one of the building's builder positions (`GetBuilderPos`), walks
//! there and loops over list 149 (GO_TO_POS, WORK). Every WORK cycle adds its frame count to the
//! site's work (`AddWork`) while material is there; without material it is set free and steps
//! aside.
//!
//! Re-verified against the release (vtable 0x105a9d8, 2026-10-19; evidence in
//! `tools/re/decompiled/script/digbuild/`): every function below matches it.

use super::core::{self, job, job_mut, register};
use super::hooks;
use super::state::{BuilderData, EntityEvent};
use super::task::task_type;
use super::walking::{ActivePathing, Pathing};
use crate::World;
use crate::build::site;
use crate::entity::flag;
use crate::grid::Y16X16;
use crate::spiral;

/// `JOB_BUILDER` (GO_TO_POS, WORK).
pub const LIST_BUILD: u16 = 149;
/// Building entity flag 0x1000, set by `CBuildingSiteRole::Init`: the target is a building site.
const BUILDING_FLAG_SITE: u32 = 0x1000;
/// `WalkToXY` flag 0x2000 (GO_TO_TARGET_ROUGHLY).
const WALK_ROUGHLY: i32 = 0x2000;
/// Node flags that make a node unsuitable to step aside to (release 0x587377: `flags & 0x77`).
const NODE_FLAGS_OCCUPIED: u8 = 0x77;

fn builder(world: &mut World, i: u16) -> Option<&mut BuilderData> {
    job_mut(world, i).map(|j| &mut j.builder)
}

/// `CBuilderRole::CBuilderRole`, `InitWalking` (vtbl+20, 0x572850: a `CWalkingWorker` of type 1,
/// then vtbl+8(-1, 0)) and `Init` (vtbl+44, 0x572810: war map, not at work).
pub fn init_role(world: &mut World, i: u16) {
    if let Some(b) = builder(world, i) {
        *b = BuilderData::default();
    }
    ActivePathing::create(world, i, 1);
    ActivePathing::walk_to_xy(world, i, Y16X16(u32::MAX), 0);
    // Init: "!FlagBits(ENTITY_FLAG_ATTACHED)" and "!m_uHomeEntityId" asserted.
    hooks::war_map_add(world, i);
}

/// The builder's home, if that is still a construction site (IBuildingRole vtbl+112 = 0).
fn home_site(world: &World, i: u16) -> Option<u16> {
    let home = job(world, i)?.home;
    (home != 0 && world.sites.roles.contains_key(&home)).then_some(home)
}

/// `CBuilderRole::LogicUpdateJob` (0x5728b0, vtbl+28): a WORK cycle is over. Re-register for
/// the next one and add its frame count to the site's work; without material, give up.
pub fn logic_update_job(world: &mut World, i: u16) {
    let Some(j) = job(world, i) else { return };
    match j.task {
        task_type::GO => {
            if let Some(j) = job_mut(world, i) {
                j.distance = 0;
            }
            core::go(world, i);
        }
        task_type::WORK => {
            if !core::check_home(world, i) {
                return;
            }
            let frames = core::actual_task(world, i).map_or(1, |t| t.frames);
            register(world, i, frames as i32);
            if let Some(j) = job_mut(world, i) {
                j.cycle_frames = 1;
            }
            let home = job(world, i).map_or(0, |j| j.home);
            if site::have_building_material(world, home) {
                if let Some(j) = job_mut(world, i) {
                    j.cycle_frames = frames;
                }
                site::add_work(world, home, frames as i32);
            } else {
                super::roles::set_free(world, i, -1);
            }
        }
        _ => {}
    }
}

/// `CBuilderRole::GetNextJob` (0x572a10): next task; at the end of the list start it again.
pub fn get_next_job(world: &mut World, i: u16) {
    if let Some(j) = job_mut(world, i) {
        j.cursor += 1;
    }
    if core::is_end_iter(world, i)
        && let Some(j) = job_mut(world, i)
    {
        j.cursor = 0; // ResetToDoList
    }
    take_job(world, i);
}

/// `CBuilderRole::TakeJob` (0x572950).
pub fn take_job(world: &mut World, i: u16) {
    let Some(t) = core::actual_task(world, i) else { return };
    core::init_common_task_values(world, i, &t);
    let Some(j) = job(world, i) else { return };
    let (destination, cycle, dir) = (j.destination, j.cycle_frames, j.builder.dir);
    match t.task {
        task_type::GO_TO_POS => {
            if let Some(j) = job_mut(world, i) {
                j.frame = 1;
            }
            ActivePathing::walk_to_xy(world, i, destination, 0);
            if let Some(j) = job_mut(world, i) {
                j.task = task_type::GO;
                j.displacement_costs = 5;
            }
            core::go(world, i);
        }
        task_type::WORK => {
            if !core::check_home(world, i) {
                return;
            }
            let home = job(world, i).map_or(0, |j| j.home);
            // CBuilding::SettlerEnter → CBuildingSiteRole::SettlerEnter (0x542ba0), which returns
            // true and does nothing else.
            if !world.sites.roles.contains_key(&home) {
                hooks::building_settler_enter(world, home, i);
            }
            if let Some(j) = job_mut(world, i) {
                j.displacement_costs = 10;
            }
            register(world, i, cycle as i32);
            if let Some(s) = core::settler_mut(world, i) {
                s.direction = dir as u8; // IMovingEntity::SetDirection
                s.job.builder.at_work = 1;
            }
        }
        task_type::WAIT => {
            if let Some(j) = job_mut(world, i) {
                j.displacement_costs = 0;
            }
            register(world, i, 1);
        }
        _ => {}
    }
}

/// `ISettlerRole` vtbl+120 (release 0x5872b0; not in the debug decompilation): look for a node
/// to step aside to, on the spiral rings 2..`radius` around the settler from the outermost
/// entry inwards: in the world, a tile of the same type bits (`(type ^ own type) & 0xFFFF0000`)
/// and none of node flags 0x77. Found: it becomes the start position (+28).
fn find_free_position(world: &mut World, i: u16, radius: u8) -> bool {
    let Some(e) = world.entities.by_index(i as u32) else { return false };
    let (x, y) = (e.x(), e.y());
    let Some(own) = world.grid.checked_index(x, y) else { return false };
    // Release: indices `First(radius − 1) ..= Last(radius)` of the `{first, last}` table
    // 0x117cf38 (`[r*4 + 0x117cf34]` and `[r*4 + 0x117cf3a]`).
    let lo = if radius >= 1 { spiral::first(radius - 1) } else { 0 };
    let hi = spiral::last(radius);
    let ids = world.tiling.ids(false);
    let own_ty = world.tiling.tile(ids[own]).ty;
    for k in (lo..=hi).rev() {
        let (nx, ny) = (x + spiral::dx(k), y + spiral::dy(k));
        let Some(n) = world.grid.checked_index(nx, ny) else { continue };
        let ty = world.tiling.tile(ids[n]).ty;
        if (ty ^ own_ty) & 0xFFFF_0000 != 0 || world.layers.flags[n] & NODE_FLAGS_OCCUPIED != 0 {
            continue;
        }
        if let Some(j) = job_mut(world, i) {
            j.start = Y16X16::pack(nx, ny);
        }
        return true;
    }
    false
}

/// `CBuilderRole::SetFree(settler, eco_sector)` (0x572a50, vtbl+64): leave the site
/// (`BuilderLeft`), then the base. A builder that was at work steps aside (walk roughly to a
/// free node within radius 3); otherwise it waits.
pub fn set_free(world: &mut World, i: u16, eco_sector: i32) -> bool {
    if let Some(site) = home_site(world, i) {
        site::builder_left(world, site, i);
    }
    let was_at_work = job(world, i).is_some_and(|j| j.builder.at_work == 1);
    if core::set_free_base(world, i, eco_sector) {
        return true;
    }
    if was_at_work && find_free_position(world, i, 3) {
        let start = job(world, i).map_or(Y16X16(0), |j| j.start);
        if let Some(j) = job_mut(world, i) {
            j.frame = 1;
        }
        ActivePathing::walk_to_xy(world, i, start, WALK_ROUGHLY);
        if let Some(j) = job_mut(world, i) {
            j.task = task_type::GO;
            j.displacement_costs = 5;
        }
    } else if let Some(j) = job_mut(world, i) {
        j.frame = 1;
        j.task = task_type::WAIT;
    }
    false
}

/// `CBuilderRole::ConvertEventIntoGoal` (0x572b20).
pub fn convert_event_into_goal(world: &mut World, i: u16, ev: &EntityEvent) {
    match ev.event {
        // COME_TO_WORK from CEcoSector::CallWorker: data0 = the site (already the home).
        1 => {
            if let Some(b) = builder(world, i) {
                b.at_work = 0;
            }
            let site = ev.data[0] as u16;
            let is_site = world.entities.by_index(site as u32).is_some_and(|b| b.flags & BUILDING_FLAG_SITE != 0);
            let home = job(world, i).map_or(0, |j| j.home);
            if !is_site || home == 0 {
                super::roles::set_free(world, i, -1);
                return;
            }
            let pos = world.entities.by_index(i as u32).map_or(Y16X16(0), |e| e.pos);
            if let Some(j) = job_mut(world, i) {
                j.start = pos;
            }
            let (x, y, dir) = site::get_builder_pos(world, site);
            core::new_destination(world, i, Y16X16::pack(x, y));
            core::new_shared_list(world, i, LIST_BUILD);
            if let Some(b) = builder(world, i) {
                b.dir = dir; // SetDir
            }
        }
        // Order cancelled.
        7 | 9 => {
            super::roles::set_free(world, i, ev.data[2]);
        }
        _ => {
            let registered = world.entities.by_index(i as u32).is_some_and(|e| e.flags & flag::REGISTERED != 0);
            if !registered {
                register(world, i, 1); // "unknown event"
            }
        }
    }
}
