//! `CDiggerRole` (MapObjects/Settler/DiggerRole.cpp, role id 2): levels the ground of a
//! construction site. The site (`build::site`, `CBuildingSiteRole`) calls a digger with event 1;
//! the digger walks to the site, checks in and then loops over list 161 (GO_TO_POS, WORK): pick
//! a node of the site's digging slot that still differs from its target height (`SearchDig`),
//! walk there and raise or lower it one step per WORK cycle (`CheckDig`). When no node is left,
//! the last digger reports `DiggingIsReady` and every digger is set free.
//!
//! Re-verified against the release (vtable 0x105ad8c, 2026-10-19; evidence in
//! `tools/re/decompiled/script/digbuild/`): every function below matches it.

use super::core::{self, job, job_mut, register};
use super::hooks;
use super::state::{DiggerData, EntityEvent};
use super::task::task_type;
use super::walking::{ActivePathing, Pathing};
use crate::World;
use crate::build::site;
use crate::entity::flag;
use crate::grid::Y16X16;

/// `JOB_DIGGER` (GO_TO_POS, WORK), taken on check-in.
pub const LIST_DIG: u16 = 161;
/// The implicit list `66 + type` of the digger (GO_TO_TARGET, CHECKIN), taken on event 1.
pub const LIST_COME_TO_WORK: u16 = 68;
/// Ground a dug node becomes (`SetGround(x, y, 28)`, the construction-site ground).
const DUG_GROUND: u8 = 28;
/// Node flag 0x20: a digger has picked the node (`SearchDig` sets it; cleared when the node is
/// done, when it cannot be changed, and by `ISettlerRole::SetFree` at the destination).
const NODE_FLAG_PICKED: u8 = 0x20;
/// Node flag 0x10 ([`crate::grid::flag::LEVELING`]): the node still has to be levelled.
const NODE_FLAG_LEVELING: u8 = crate::grid::flag::LEVELING;
/// Building entity flag 0x1000, set by `CBuildingSiteRole::Init`: the target is a building site.
const BUILDING_FLAG_SITE: u32 = 0x1000;

fn digger(world: &mut World, i: u16) -> Option<&mut DiggerData> {
    job_mut(world, i).map(|j| &mut j.digger)
}

/// `CDiggerRole::CDiggerRole`, `InitWalking` (vtbl+20, 0x5755e0: a `CWalkingWorker` of type 1,
/// then vtbl+8(-1, 0)) and `Init` (vtbl+44, 0x5755a0: war map, home 0, not working).
pub fn init_role(world: &mut World, i: u16) {
    if let Some(d) = digger(world, i) {
        *d = DiggerData::default();
    }
    ActivePathing::create(world, i, 1);
    ActivePathing::walk_to_xy(world, i, Y16X16(u32::MAX), 0);
    // Init: "!FlagBits(ENTITY_FLAG_ATTACHED)" and "!m_uHomeEntityId" asserted.
    hooks::war_map_add(world, i);
    if let Some(j) = job_mut(world, i) {
        j.home = 0;
        j.digger.working = false;
    }
}

/// The site the digger works for: its home, if that is still a construction site.
fn home_site(world: &World, i: u16) -> Option<u16> {
    let home = job(world, i)?.home;
    (home != 0 && world.sites.roles.contains_key(&home)).then_some(home)
}

/// Index into the 31×31 digging window (`32 · (x − site x + 15) + (y − site y + 15)`), if the
/// node lies in it.
fn window_index(x: i32, y: i32, site: Y16X16) -> Option<usize> {
    let (dx, dy) = (x - site.x() + 15, y - site.y() + 15);
    ((0..31).contains(&dx) && (0..31).contains(&dy)).then_some((32 * dx + dy) as usize)
}

/// `byte_409F9AD[30750 · owner + 1 + 1025 · slot + index]`: the digging slot's target height.
fn dig_target(world: &World, owner: u8, site: u16, index: usize) -> u8 {
    let Some(s) = world.sites.roles.get(&site) else { return 0 };
    if s.dig_slot < 0 {
        return 0;
    }
    world.sites.dig_slots.get(&(owner, s.dig_slot as u8)).and_then(|d| d.heights.get(index)).copied().unwrap_or(0)
}

fn clear_dig_target(world: &mut World, owner: u8, site: u16, index: usize) {
    let Some(s) = world.sites.roles.get(&site) else { return };
    if s.dig_slot < 0 {
        return;
    }
    let key = (owner, s.dig_slot as u8);
    if let Some(h) = world.sites.dig_slots.get_mut(&key).and_then(|d| d.heights.get_mut(index)) {
        *h = 0;
    }
}

/// `CWorldManager::GroundHeight(x, y)`; −1 off the map (the original reads past the map there).
fn height(world: &World, x: i32, y: i32) -> i32 {
    world.grid.checked_index(x, y).map_or(-1, |n| world.layers.height[n] as i32)
}

/// The slope rule shared by `SearchDig` and `CheckDig`: a node at height `h` may be lowered
/// unless (x−1, y−1) or (x, y−1) is at `h + 7`, or (x, y+1) or (x+1, y+1) at `h + 5`; it may be
/// raised unless those are at `h − 5` / `h − 7`. (The original compares for equality only.)
fn may_change(world: &World, x: i32, y: i32, h: i32, lower: bool) -> bool {
    let (up, down) = if lower { (h + 7, h + 5) } else { (h - 5, h - 7) };
    height(world, x - 1, y - 1) != up
        && height(world, x, y - 1) != up
        && height(world, x, y + 1) != down
        && height(world, x + 1, y + 1) != down
}

fn node_flags(world: &mut World, x: i32, y: i32) -> Option<&mut u8> {
    let n = world.grid.checked_index(x, y)?;
    Some(&mut world.layers.flags[n])
}

/// `CDiggerRole::LogicUpdateJob` (0x575640, vtbl+28).
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
            let frames = core::actual_task(world, i).map_or(1, |t| t.frames);
            if let Some(j) = job_mut(world, i) {
                j.cycle_frames = frames;
            }
            if check_dig(world, i) {
                register(world, i, frames as i32);
            }
        }
        _ => {}
    }
}

/// `CDiggerRole::GetNextJob` (0x575940): next task; at the end of the list start it again.
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

/// `CDiggerRole::TakeJob` (0x5756a0). The WORK target is read from the slot table at 0x14f1b15
/// (= the window's centre in the slot at 0x14f1926 + 1025 · (slot + 30 · owner)).
pub fn take_job(world: &mut World, i: u16) {
    let Some(t) = core::actual_task(world, i) else { return };
    core::init_common_task_values(world, i, &t);
    let Some(j) = job(world, i) else { return };
    let (counter, destination, home) = (j.counter, j.destination, j.home);
    match t.task {
        task_type::GO_TO_POS | task_type::GO_TO_TARGET => {
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
            if let Some(d) = digger(world, i) {
                d.working = true;
            }
            let Some(e) = world.entities.by_index(i as u32) else { return };
            let (pos, owner) = (e.pos, e.owner);
            let Some(site) = core::entity_pos(world, home) else { return };
            let target = window_index(pos.x(), pos.y(), site).map_or(0, |n| dig_target(world, owner, home, n));
            if let Some(d) = digger(world, i) {
                d.target = target as u16;
            }
            if check_dig(world, i) {
                if let Some(j) = job_mut(world, i) {
                    j.displacement_costs = 10;
                }
                register(world, i, counter as i32);
            }
        }
        task_type::WAIT => {
            if let Some(j) = job_mut(world, i) {
                j.displacement_costs = 0;
            }
            register(world, i, 1);
        }
        task_type::CHECKIN => {
            if let Some(j) = job_mut(world, i) {
                j.displacement_costs = 10;
            }
            // CBuilding::SettlerEnter → CBuildingSiteRole::SettlerEnter (0x542ba0), which returns
            // true and does nothing else.
            if !world.sites.roles.contains_key(&home) {
                hooks::building_settler_enter(world, home, i);
            }
            core::new_shared_list(world, i, LIST_DIG);
        }
        _ => {}
    }
}

/// `CDiggerRole::CheckDig` (0x575d00): change the node under the
/// digger one step towards its target height. Returns whether it did (the WORK cycle goes on);
/// otherwise the node is done or blocked by the slope rule and the next one is searched.
fn check_dig(world: &mut World, i: u16) -> bool {
    let Some(e) = world.entities.by_index(i as u32) else { return false };
    let (x, y, owner) = (e.x(), e.y(), e.owner);
    let Some(j) = job(world, i) else { return false };
    let (target, home) = (j.digger.target as i32, j.home);
    let Some(site) = core::entity_pos(world, home) else { return false };
    let h = height(world, x, y);
    if h == target || target == 0 {
        if let Some(n) = window_index(x, y, site) {
            clear_dig_target(world, owner, home, n);
        }
        if let Some(f) = node_flags(world, x, y) {
            *f &= !(NODE_FLAG_LEVELING | NODE_FLAG_PICKED);
        }
        search_dig(world, i);
        return false;
    }
    let lower = h > target;
    if !may_change(world, x, y, h, lower) {
        if let Some(f) = node_flags(world, x, y) {
            *f &= !NODE_FLAG_PICKED;
        }
        search_dig(world, i);
        return false;
    }
    if let Some(n) = world.grid.checked_index(x, y) {
        // Decrease/IncreaseGroundHeight, then UpdateWorldPosition (0x575ddf: the shading).
        let v = &mut world.layers.height[n];
        *v = if lower { v.wrapping_sub(1) } else { v.wrapping_add(1) };
        world.update_world_position(x, y);
    }
    if let Some(s) = world.sites.roles.get_mut(&home) {
        s.dig_effort -= 1; // `--site+392`
    }
    if site::can_change_ground(world, x, y, DUG_GROUND) {
        site::set_ground(world, x, y, DUG_GROUND);
        world.update_world_position(x, y); // 0x575e0a
    }
    true
}

/// `CDiggerRole::SearchDig` (0x575a10): from a random cell of the 31×31 window, step through
/// the window 13 cells at a time (961 steps cover every cell once) for a node that still has a
/// target height, differs from it, is not picked by another digger and passes the slope rule.
/// Found: pick it and walk there. None left: the last digger reports `DiggingIsReady`; the
/// digger is set free either way.
fn search_dig(world: &mut World, i: u16) {
    let mut cx = ((31 * world.rng.rand()) >> 16) as i32;
    let mut cy = ((31 * world.rng.rand()) >> 16) as i32;
    let Some(e) = world.entities.by_index(i as u32) else { return };
    let owner = e.owner;
    let home = job(world, i).map_or(0, |j| j.home);
    let Some(site) = core::entity_pos(world, home) else { return };
    let (sx, sy) = (site.x(), site.y());
    let mut steps = 0;
    let mut found = None;
    while found.is_none() && steps <= 961 {
        steps += 1;
        cx += 13;
        if cx >= 31 {
            cx -= 31;
            cy += 1;
            if cy >= 31 {
                cy = 0;
            }
        }
        let target = dig_target(world, owner, home, (32 * cx + cy) as usize) as i32;
        if target == 0 {
            continue;
        }
        let (x, y) = (cx + sx - 15, cy + sy - 15);
        let Some(n) = world.grid.checked_index(x, y) else { continue };
        let h = world.layers.height[n] as i32;
        if target == h || world.layers.flags[n] & NODE_FLAG_PICKED != 0 {
            continue;
        }
        if may_change(world, x, y, h, target < h) {
            found = Some((x, y, target));
        }
    }
    match found {
        Some((x, y, target)) if steps <= 961 => {
            if let Some(f) = node_flags(world, x, y) {
                *f |= NODE_FLAG_PICKED;
            }
            // "Digger trouble" is traced when the node has FLAG_BLOCKED_LAND (no effect).
            core::new_destination(world, i, Y16X16::pack(x, y));
            if let Some(d) = digger(world, i) {
                d.target = target as u16;
            }
            super::roles::get_next_job(world, i);
        }
        _ => {
            if world.sites.roles.get(&home).is_some_and(|s| s.diggers_working == 1) {
                site::digging_is_ready(world, home, owner);
            }
            super::roles::set_free(world, i, -1);
        }
    }
}

/// `CDiggerRole::SetFree(settler, eco_sector)` (0x575980, vtbl+64): leave the site
/// (`BuilderLeft` while the home is still a site, IBuildingRole vtbl+112 = 0), then the base.
pub fn set_free(world: &mut World, i: u16, eco_sector: i32) -> bool {
    if let Some(site) = home_site(world, i) {
        site::builder_left(world, site, i);
    }
    if let Some(d) = digger(world, i) {
        d.working = false;
    }
    core::set_free_base(world, i, eco_sector)
}

/// `CDiggerRole::ConvertEventIntoGoal` (0x575850).
pub fn convert_event_into_goal(world: &mut World, i: u16, ev: &EntityEvent) {
    match ev.event {
        // COME_TO_WORK from CEcoSector::CallWorker: data0 = the site.
        1 => {
            if let Some(d) = digger(world, i) {
                d.working = false;
            }
            let site = ev.data[0] as u16;
            let Some(b) = world.entities.by_index(site as u32) else { return };
            if b.flags & BUILDING_FLAG_SITE == 0 {
                return;
            }
            let pos = b.pos;
            core::new_destination(world, i, pos);
            core::new_shared_list(world, i, LIST_COME_TO_WORK);
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
