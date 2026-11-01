//! `CSoldierRole` (role 7; release vtable 0x105c3ac, `CWarriorBehavior` part at +0x30 with
//! vtable 0x105c394) and `CTowerSoldier` (role 22, vtable 0x105c584): soldiers outside and
//! inside military buildings. Own reverse engineering of the shipped exe (release VAs).
//!
//! A free soldier's job is its `CWarriorBehavior`: `TakeJob(WAIT)` turns the task into 0x1b
//! (behaviour-driven) and sets the behaviour idle (`SetMode(-1, 0)`, 0x598290); every update
//! (`LogicUpdateJob` 0x589250 → behaviour `Update` 0x598610) looks for enemies every 30 ticks and
//! otherwise takes one walking step (`IdleWalk` when there is no target; 9 ticks per step, 7 for
//! swordsmen; 1 tick when standing).
//!
//! Ordered to a building (event 1 from the soldier search 0x582ca0), it walks to the building's
//! door (`SetMode(door, 0x200000)`: behaviour state 4, walk flag 0x2000 = close enough within 3)
//! and takes its `*_CHECKIN` list (GO_TO_TARGET + CHECKIN); arriving (step flags 0x70), the
//! behaviour ends (state 8 → `GetNextJob`) and CHECKIN takes it off the map into the building's
//! `SettlerEnter`, where the building gives it a `CTowerSoldier` role.
//!
//! A tower soldier waits (`WAIT` of its wait list). Swordsmen at the wall (slot state 0) are not
//! updated again; bowmen on top (states 1/2) and the door guard (3) look for enemies and
//! otherwise re-register after `31 − Rand() % 4` (guard: `4 − Rand() % 4`) ticks.
//!
//! Fights: [`warrior`] (the behaviour's searches and `Attack`), the WORK task's end in
//! [`logic_update_job`], the tower soldiers' shots in [`tower_logic_update`].
//!
//! Ferries (`vehicles::ferry`): boarding on a move order (0x5800b0), events 0x18 / 0x19, CHECKIN
//! into the vehicle. Not ported: manakopters (event 0x1c, CHECKIN into one).

use super::core::{self, job, job_mut, register, settler_mut};
use super::warrior;
use super::state::{EntityEvent, SoldierData, TowerData, role};
use super::task::task_type;
use super::walking::{ActivePathing, Pathing};
use crate::World;
use crate::entity::{EntityData, ObjType, flag};
use crate::event::SimEvent;
use crate::grid::Y16X16;

/// The behaviour-driven task (`ISettlerRole +4` = 0x1b).
pub const TASK_BEHAVIOUR: u8 = 0x1b;
/// `CWarriorBehavior` mode: walk to a target position (state 4).
pub const MODE_GO_TO: u32 = 0x20_0000;

fn unported(world: &mut World, what: &'static str, entity: u16) {
    world.events.push(SimEvent::Unported { what, entity });
}

fn soldier(world: &mut World, i: u16) -> Option<&mut SoldierData> {
    job_mut(world, i).map(|j| &mut j.soldier)
}

/// `CSettler::SetRole(role)` (0x580c20) for a settler that already has one: the old role's
/// members go (the `CSettler` / `IMovingEntity` / `IAnimatedEntity` parts stay), the new role's
/// `Init` runs (it still sees the old walking object), then the role's walking object replaces
/// the old one. First the settler leaves its groups (`CGroupMgr` vtbl+0x1c).
pub fn replace_role(world: &mut World, i: u16, new_role: u8) {
    if world.entities.by_index(i as u32).is_none_or(|e| e.flags & flag::DIED != 0) {
        return;
    }
    crate::group::remove_all(world, i);
    if let Some(j) = job_mut(world, i) {
        let old = std::mem::take(j);
        *j = super::JobState {
            todo: old.todo,
            list_id: old.list_id,
            cursor: old.cursor,
            frame: old.frame,
            job_part: old.job_part,
            last_update_tick: old.last_update_tick,
            events: old.events,
            distance: old.distance,
            displacement_costs: old.displacement_costs,
            walking: old.walking,
            role: new_role,
            ..super::JobState::default()
        };
    }
    match new_role {
        role::SOLDIER => {
            init(world, i);
            create_walking(world, i);
        }
        role::TOWER_SOLDIER => {
            tower_init(world, i);
            tower_create_walking(world, i);
        }
        r if super::roles::is_ported(r) => super::roles::new_role(world, i, r, 0),
        _ => {}
    }
}

/// vtbl+0x14 (0x58a9d0): a `CWalkingNormal` of the owner.
fn create_walking(world: &mut World, i: u16) {
    ActivePathing::create(world, i, crate::pathing::walking::walking_type::NORMAL);
}

/// `CSoldierRole::Init` (0x5890f0) with the role's walking object. A new settler gets it first
/// (the `CSettler` ctor 0x580330 creates it before the role's `Init`); a settler that changes
/// role (`CSettler::SetRole` 0x580c20, e.g. a carrier turned soldier by `ChangeType`) runs `Init`
/// with its old walking object, which the role's (vtbl+0x14) then replaces: a recruit must not
/// keep the carrier's `CWalkingWorker`, which only walks on its owner's land.
pub fn init_role(world: &mut World, i: u16) {
    if job(world, i).is_some_and(|j| j.walking.is_none()) {
        create_walking(world, i);
        init(world, i);
    } else {
        init(world, i);
        create_walking(world, i);
    }
}

/// `CSoldierRole::Init` proper: +0x12 = own id, flags VULNERABLE | SELECTABLE and the warrior
/// type, war map, behaviour idle, task 0x1b.
fn init(world: &mut World, i: u16) {
    let healings = world.rules.logic.medic_number_of_healings;
    let Some(e) = world.entities.by_index_mut(i as u32) else { return };
    let kind = e.kind;
    e.flags |= flag::VULNERABLE_MASK | flag::SELECTABLE;
    let wt = world.rules.warrior_type(kind) as u32;
    if let Some(e) = world.entities.by_index_mut(i as u32) {
        e.flags |= wt;
    }
    if let Some(j) = job_mut(world, i) {
        j.w18 = i;
        j.soldier = SoldierData { healings, ..SoldierData::default() };
    }
    crate::warmap::readd(world, i);
    set_mode(world, i, -1, 0);
    if let Some(j) = job_mut(world, i) {
        j.task = TASK_BEHAVIOUR;
    }
}

/// Behaviour vtbl+0 `SetMode(settler, target, mode)`: [`set_mode_base`], for a squad leader
/// followed by 0x58acf0 (+0x60 / +0x64 / +0x68 = 0, the anchor +0x6c = the leader's position).
/// Every caller in the soldier code goes through the vtable.
pub fn set_mode(world: &mut World, i: u16, target: i32, mode: u32) {
    set_mode_base(world, i, target, mode);
    if is_leader(world, i) {
        let pos = world.entities.by_index(i as u32).map_or(0, |e| e.pos.0 as i32);
        if let Some(s) = soldier(world, i) {
            s.lead_tick = 0;
            s.lead_target = 0;
            s.anchor = pos;
        }
    }
}

/// `CWarriorBehavior::SetMode(settler, target, mode)` (0x598290): picks the search table row,
/// keeps the target only for modes 0x100000..0x700000, sets the state (0x100000 → 1, 0x200000 /
/// 0x300000 → 4, 0x500000 → 6, 0x600000 → 7, else 0; 0x700000: attack `target` directly,
/// state 5) and points the walking object at the target (`WalkToXY(target, (mode & 0x1060) |
/// 0x2000)`).
fn set_mode_base(world: &mut World, i: u16, target: i32, mode: u32) {
    let Some(e) = world.entities.by_index(i as u32) else { return };
    let (kind, owner, wt) = (e.kind, e.owner, e.warrior_type());
    let mut table = wt;
    if wt == 4 || kind == 0x2e {
        table = match kind {
            0x23..=0x25 => 0xf,
            0x26..=0x28 => 0x10,
            0x29..=0x2b => 0x11,
            0x2e => 0x13,
            0x36 => 0x12,
            0x3d..=0x3f => 0x14,
            _ => wt,
        };
    }
    let m = mode & 0x70_0000;
    let target = if m == 0 { -1 } else { target };
    let Some(s) = soldier(world, i) else { return };
    s.table = table;
    s.target_pos = target;
    s.mode = owner as u32 | (mode & !0xF);
    if m == 0x70_0000 {
        s.target = target as u16;
        s.state = 5;
        s.bflags = 0;
        s.next_search = 0;
        s.target_pos = 0;
        return;
    }
    s.state = match m {
        0x10_0000 => 1,
        0x20_0000 | 0x30_0000 => 4,
        0x50_0000 => 6,
        0x60_0000 => 7,
        _ => 0,
    };
    s.bflags = 0;
    s.target = 0;
    s.next_search = 0;
    ActivePathing::walk_to_xy(world, i, Y16X16(target as u32), ((mode & 0x1060) | 0x2000) as i32);
}

/// `CSoldierRole::TakeJob` (0x589b30).
/// The override task (+0x50, a fight set by [`warrior::attack`]) replaces the list's task.
pub fn take_job(world: &mut World, i: u16) {
    let fight = job(world, i).and_then(|j| j.soldier.fight_task);
    let Some(t) = fight.or_else(|| core::actual_task(world, i)) else { return };
    let old = job(world, i).map(|j| (j.frame, j.job_part));
    core::init_common_task_values(world, i, &t);
    match t.task {
        task_type::GO_TO_TARGET => {
            if let Some(j) = job_mut(world, i) {
                j.task = TASK_BEHAVIOUR;
                j.displacement_costs = 5;
                j.frame = 1;
                j.walk = 1;
            }
            register(world, i, 1);
        }
        task_type::WAIT => {
            if let Some(j) = job_mut(world, i) {
                j.task = TASK_BEHAVIOUR;
                j.displacement_costs = 0;
            }
            register(world, i, 1);
            set_mode(world, i, -1, 0);
        }
        task_type::CHECKIN => check_in(world, i),
        7..=9 => {
            // The animation goes on if the gfx job stays and its frame is inside the new cycle.
            if let (Some((frame, part)), Some(j)) = (old, job_mut(world, i)) {
                j.frame = if part == j.job_part && frame > 0 && frame < j.cycle_frames { frame } else { 1 };
            }
            go_to_pos(world, i, &t);
        }
        task_type::WORK | 0x22 => warrior::take_work(world, i),
        _ => {}
    }
}

/// CHECKIN (0x589b30 case 0x18): unregistered, off the map (ON_BOARD), into the home: a
/// building's `SettlerEnter`, a vehicle's vtbl+0x80, a manakopter's boarding (0x56b890); no
/// home → `SetFree(-1)`.
fn check_in(world: &mut World, i: u16) {
    if let Some(j) = job_mut(world, i) {
        j.displacement_costs = 10;
    }
    world.unregister_from_logic_update(i);
    if world.warmap.contains(i) {
        crate::warmap::remove(world, i);
    }
    let Some(e) = world.entities.by_index_mut(i as u32) else { return };
    let pos = e.pos;
    e.flags |= flag::ON_BOARD;
    if let Some(n) = world.grid.checked_index(pos.x(), pos.y()) {
        world.layers.map_object[n] = 0;
    }
    let home = job(world, i).map_or(0, |j| j.home);
    match world.entities.by_index(home as u32).map(|e| e.obj_type) {
        None => {
            set_free(world, i, -1);
        }
        Some(ObjType::Building) => {
            crate::buildings::settler_enter(world, home, i);
        }
        // A vehicle (a ferry): its vtbl+0x80.
        Some(ObjType::Ship | ObjType::LandVehicle) => crate::vehicles::settler_entered(world, home, i),
        Some(ObjType::Flying) => {
            crate::flying::board(world, home, i);
        }
        Some(_) => {}
    }
}

/// `CSoldierRole::LogicUpdateJob` (0x589250). At the end of a WORK task (0x10) the fight damage
/// ([`warrior::hit`]); 0x22 a medic's heal ([`warrior::heal`]). Then without
/// an override task the update proper (0x589170); with one, the override is cleared and the
/// list's task taken again, and a behaviour task (0x1b) updates at once (0x589170).
pub fn logic_update_job(world: &mut World, i: u16) {
    let Some(j) = job(world, i) else { return };
    let (task, fight) = (j.task, j.soldier.fight_task.is_some());
    if task == task_type::WORK {
        warrior::hit(world, i);
    } else if task == 0x22 {
        warrior::heal(world, i);
    }
    if !fight {
        update(world, i);
        return;
    }
    if let Some(s) = soldier(world, i) {
        s.fight_task = None;
    }
    take_job(world, i);
    if job(world, i).is_some_and(|j| j.task == TASK_BEHAVIOUR) {
        update(world, i);
    }
}

/// 0x589170: medics regain a healing every `Logic/MedicTicksToRegeneration` ticks, then the
/// behaviour update; entity flags 0xc00000 select 0x5891e0 instead: frozen (0x800000, a blowgun
/// hit) the soldier stands for 8 ticks (displacement costs 10, step word 8); 0x400000 (the
/// Vikings' spell 6) it idles about with `IdleWalk` flag 0x10000000 (a step even without a
/// crowd), displacement costs 0, then 0x58a580.
fn update(world: &mut World, i: u16) {
    let tick = world.tick;
    let (regen, max) = (world.rules.logic.medic_ticks_to_regeneration, world.rules.logic.medic_number_of_healings);
    let kind = world.entities.by_index(i as u32).map_or(0, |e| e.kind);
    if let Some(s) = soldier(world, i)
        && (0x23..=0x25).contains(&kind)
        && s.regen_tick.wrapping_add(regen) <= tick
    {
        s.regen_tick = tick;
        if s.healings < max {
            s.healings += 1;
        }
    }
    let flags = world.entities.by_index(i as u32).map_or(0, |e| e.flags);
    if flags & 0xc0_0000 == 0 {
        warrior::update(world, i);
    } else if flags & 0x80_0000 != 0 {
        if let Some(j) = job_mut(world, i) {
            j.displacement_costs = 10;
            j.walk = 8;
        }
        register(world, i, 8);
    } else {
        if let Some(j) = job_mut(world, i) {
            j.displacement_costs = 0;
        }
        let step = ActivePathing::idle_step(world, i, 0x1000_0000);
        let r = core::walk_dir(world, i, step);
        after_step(world, i, r);
    }
}

/// Behaviour vtbl+4 (0x58a6b0): one step (`CSettler::Walk`); a step re-registers after 9 ticks
/// (7 for swordsmen), no step after 1. Displacement costs 5, 0 while the step word has 0x10.
/// A squad leader's (0x58ad30) then holds its ground and leads its group ([`leader_step`]).
pub(super) fn walk_step(world: &mut World, i: u16) -> u32 {
    let dead = world.entities.by_index(i as u32).is_none_or(|e| e.flags & flag::DIED != 0);
    let r = if dead { u32::MAX } else { core::settler_walk(world, i) };
    after_step(world, i, r);
    if is_leader(world, i) {
        leader_step(world, i, r);
    }
    r
}

fn after_step(world: &mut World, i: u16, r: u32) {
    let wt = world.entities.by_index(i as u32).map_or(0, |e| e.warrior_type());
    if let Some(j) = job_mut(world, i) {
        j.walk = r as u8;
    }
    if r & 0xF < 6 {
        let speed: i8 = if wt == 2 { 7 } else { 9 };
        if let Some(j) = job_mut(world, i) {
            j.counter = speed;
            j.distance = 0xFF;
        }
        register(world, i, speed as i32);
    } else {
        register(world, i, 1);
    }
    if let Some(j) = job_mut(world, i) {
        j.displacement_costs = if r & 0x10 != 0 { 0 } else { 5 };
    }
}

/// Behaviour vtbl+8 (0x58a950): stand about — one `IdleWalk` step (walking vtbl+0x10 with flags
/// 0, then `CSettler::WalkDir`), otherwise as [`walk_step`]. A squad leader (0x58ae10) then
/// leads its group ([`lead`]).
pub(super) fn stand_step(world: &mut World, i: u16) -> u32 {
    let step = ActivePathing::idle_step(world, i, 0);
    let r = core::walk_dir(world, i, step);
    after_step(world, i, r);
    if is_leader(world, i) {
        lead(world, i);
    }
    r
}

/// Behaviour vtbl+0x10 (0x58a920; the squad leader's 0x58ae70 is the same code): done — in 1 tick, displacement costs 0, `GetNextJob`.
pub(super) fn behaviour_done(world: &mut World, i: u16) {
    register(world, i, 1);
    if let Some(j) = job_mut(world, i) {
        j.displacement_costs = 0;
    }
    get_next_job(world, i);
}

/// `CSoldierRole::GetNextJob` (0x589ec0): on a WAIT task only a pending fight task (+0x50) is
/// dropped and the job taken again. Otherwise the iterator moves on; when the new task is a WAIT
/// whose x offset is odd, the list has more than 2 tasks and its first task's offset lies more
/// than 3 (hex distance) from the old task's, the iterator goes back to the list's start
/// (0x56df40); then the fight task is dropped and the job taken.
pub fn get_next_job(world: &mut World, i: u16) {
    let Some(t) = core::actual_task(world, i) else { return };
    if t.task == task_type::WAIT {
        if job(world, i).is_some_and(|j| j.soldier.fight_task.is_some()) {
            if let Some(s) = soldier(world, i) {
                s.fight_task = None;
            }
            take_job(world, i);
        }
        return;
    }
    if let Some(j) = job_mut(world, i) {
        j.cursor += 1;
    }
    if let Some(n) = core::actual_task(world, i)
        && n.task == task_type::WAIT
        && n.x & 1 != 0
        && let Some(j) = job(world, i)
    {
        let tasks = core::todo_tasks(world, j);
        if tasks.len() > 2
            && crate::spiral::grid_distance(tasks[0].x as i32 - t.x as i32, tasks[0].y as i32 - t.y as i32) > 3
            && let Some(j) = job_mut(world, i)
        {
            j.cursor = 0;
        }
    }
    if let Some(s) = soldier(world, i) {
        s.fight_task = None;
    }
    take_job(world, i);
}

/// `CSoldierRole::GetNextJob`'s visible animation, `UpdateJob` (0x589750): the behaviour task
/// walks the walk cycle unless the step word has 0x08 (standing).
pub fn update_job(world: &mut World, i: u16) {
    let Some(j) = job_mut(world, i) else { return };
    if j.task != TASK_BEHAVIOUR {
        return;
    }
    if j.walk & 8 != 0 || j.cycle_frames == 0 {
        j.frame = 0;
        return;
    }
    j.frame = ((j.frame as u32 + j.elapsed as u32) % j.cycle_frames as u32) as u8;
    if j.frame == 0 {
        j.frame = 1;
    }
    if j.counter > 0 {
        let a = ((j.elapsed as u32) << 8) / j.counter as u32;
        j.distance = j.distance.saturating_sub(a.min(255) as u8);
    }
}

/// `CSoldierRole::ConvertEventIntoGoal` (0x589f70).
pub fn convert_event_into_goal(world: &mut World, i: u16, ev: &EntityEvent) {
    match ev.event {
        // Come to the building: the home set by `CBuilding::Attach`.
        1 => {
            let home = job(world, i).map_or(0, |j| j.home);
            if home != 0 {
                go_to_building(world, i, home);
            } else {
                set_free(world, i, -1);
            }
        }
        // The building is gone / the order was cancelled: free again.
        7 | 9 => {
            let selectable = world.entities.by_index(i as u32).is_some_and(|e| e.flags & flag::SELECTABLE != 0);
            if !selectable {
                if let Some(e) = world.entities.by_index_mut(i as u32) {
                    e.flags |= flag::SELECTABLE;
                }
                // 0x52c380: the role's own task list is emptied (always empty here).
                core::take_wait_list(world, i);
                set_mode(world, i, -1, 0);
                if let Some(j) = job_mut(world, i) {
                    j.task = TASK_BEHAVIOUR;
                }
                // AI notice 0x15 (0x58a00d).
                let owner = world.entities.by_index(i as u32).map_or(0, |e| e.owner);
                crate::ai::post_event(world, 0x15, owner as i32, i as i32, ev.data[0]);
            }
        }
        0x11 => move_order(world, i, ev),
        0x18 => called_to_vehicle(world, i, ev.data[0] as u16),
        0x19 => ashore(world, i, Y16X16(ev.data[1] as u32)),
        0x1c => called_to_manakopter(world, i, ev.data[0] as u16),
        _ => {
            if world.entities.by_index(i as u32).is_some_and(|e| e.flags & flag::REGISTERED == 0) {
                register(world, i, 1);
            }
        }
    }
}

/// Event 0x11 (0x589f70): a move order. Ignored (but the update continues) while the soldier is
/// not SELECTABLE; only type 0xd is handled — data0 walk flags, data1 the target, data2 (> 0) a
/// building position: there a free soldier of the player standing in the same sector is sent
/// into the first military building (entity flags nibble 0xc) of its player on the radius-5
/// spiral around it if it may order one more of its kind (0x549f00); otherwise it walks to data1
/// (0x589810). Before that a move order onto a ferry boards it (0x5800b0, `vehicles::ferry`).
fn move_order(world: &mut World, i: u16, ev: &EntityEvent) {
    let Some(e) = world.entities.by_index(i as u32) else { return };
    let (flags, owner, pos, kind) = (e.flags, e.owner, e.pos, e.kind);
    if flags & flag::SELECTABLE == 0 {
        if let Some(e) = world.entities.by_index_mut(i as u32) {
            e.flags |= core::FLAG_CONTINUE_AFTER_EVENT;
        }
        return;
    }
    if ev.kind != 0xd || crate::vehicles::ferry::settler_move_order(world, i, ev) {
        return;
    }
    let at = Y16X16(ev.data[2] as u32);
    let home = job(world, i).map_or(0, |j| j.home);
    let blocked = ev.data[2] > 0 && world.grid.checked_index(at.x(), at.y()).is_some_and(|n| world.layers.flags[n] & 1 != 0);
    if blocked && home == 0 && world.grid.checked_index(at.x(), at.y()).is_some_and(|n| world.owner_of(n) == owner) {
        let my_sector = world.grid.checked_index(pos.x(), pos.y()).map_or(0, |n| world.tiling.sector_of(n));
        for k in 0..=crate::spiral::last(5) {
            let (x, y) = (at.x() + crate::spiral::dx(k), at.y() + crate::spiral::dy(k));
            let Some(n) = world.grid.checked_index(x, y) else { continue };
            let m = world.layers.map_object[n];
            if m == 0 {
                continue;
            }
            let Some(b) = world.entities.by_index(m as u32) else { continue };
            let ensign_sector = match &b.data {
                EntityData::Building(d) => world.grid.checked_index(d.ensign.x(), d.ensign.y()).map_or(0, |n| world.tiling.sector_of(n)),
                _ => continue,
            };
            if b.flags & 0xf != 0xc || b.owner != owner || ensign_sector != my_sector {
                continue;
            }
            if crate::buildings::military::inc_wish_and_order(world, m, world.rules.warrior_type(kind), false) {
                crate::buildings::military::attach_settler(world, m, i);
                go_to_building(world, i, m);
                return;
            }
            break;
        }
    }
    walk_to(world, i, Y16X16(ev.data[1] as u32), ev.data[0] as u32);
    if let Some(e) = world.entities.by_index_mut(i as u32) {
        e.flags |= core::FLAG_CONTINUE_AFTER_EVENT;
    }
}

/// 0x589810 `(x, y, flags)`: the role's own task list — GO_TO_POS (7; 8 with flag 4, 9 with
/// flag 8) to the target, then WAIT (its x = the flags) — becomes the settler's to-do list (id
/// −1) and is started. With flag 1 a first leg to the current position is added and flag 2 is
/// dropped (both within 3); with flag 2 a running list (not on WAIT, more than one task) is
/// extended instead of replaced (its trailing WAIT removed) and not restarted; a restart drops a
/// pending fight first.
pub(super) fn walk_to(world: &mut World, i: u16, target: Y16X16, flags: u32) {
    let Some(e) = world.entities.by_index(i as u32) else { return };
    let (pos, race, kind) = (e.pos, e.race, e.kind);
    let mut flags = flags;
    if flags & 1 != 0 {
        let d = crate::spiral::grid_distance(target.x() - pos.x(), target.y() - pos.y());
        flags &= !2;
        if d < 3 {
            flags &= !3;
        }
    }
    let mut list = match job(world, i).map(|j| &j.todo) {
        Some(super::ToDo::Own(v)) => v.clone(),
        _ => Vec::new(),
    };
    let mut restart = true;
    let waiting = core::actual_task(world, i).is_some_and(|t| t.task == task_type::WAIT);
    if flags & 2 != 0 && !waiting && list.len() > 1 {
        if list.last().is_some_and(|t| t.task == task_type::WAIT) {
            list.pop();
        }
        restart = false;
    } else {
        list.clear();
    }
    let job_part = world.rules.jobs.first_job(race, kind);
    let frames = world.rules.jobs.frame_count(race, job_part, 2);
    let task = if flags & 8 != 0 { 9 } else if flags & 4 != 0 { 8 } else { 7 };
    let go = |x: i32, y: i32| super::EntityTask::new(task, job_part, x as i16, y as i16, -1, frames, -1, true, true, 0, 0, 0, 0);
    if flags & 1 != 0 {
        list.push(go(pos.x(), pos.y()));
    }
    list.push(go(target.x(), target.y()));
    list.push(super::EntityTask::new(task_type::WAIT, job_part, flags as i16, 0, -1, frames, -1, true, true, 0, 0, 0, 0));
    core::set_to_do_list(world, i, list);
    if restart {
        // 0x589921: the override task (+0x50, a fight [`warrior::attack`] set) is dropped first,
        // so the new list's walk is taken, not the fight.
        if let Some(j) = job_mut(world, i) {
            j.soldier.fight_task = None;
        }
        take_job(world, i);
    }
}

/// `TakeJob` GO_TO_POS / 8 / 9 (0x589b30 cases 7..9): task 0x1b, displacement costs 5, the frame
/// kept if the gfx job did not change, step word 0x80, and the behaviour walks to the task's
/// position (`SetMode` with 0x100000, 0x200000 or 0x500000): the role's +0xe / +0x10 position
/// (the task's) through 0x56a000 with one unit and flag 1 (a node of the soldier's sector near
/// it), which is written back.
fn go_to_pos(world: &mut World, i: u16, t: &super::EntityTask) {
    let mode = match t.task {
        9 => 0x50_0000,
        8 => 0x20_0000,
        _ => 0x10_0000,
    };
    if let Some(j) = job_mut(world, i) {
        j.task = TASK_BEHAVIOUR;
        j.displacement_costs = 5;
        j.walk = 0x80;
    }
    let pos = world.entities.by_index(i as u32).map_or(0, |e| e.pos.0 as i32);
    let (x, y) = job(world, i).map_or((t.x, t.y), |j| (j.x, j.y));
    let target = crate::group::formation(world, x as i32, y as i32, 1, 1, pos).positions[0];
    if let Some(j) = job_mut(world, i) {
        j.x = target.x() as i16;
        j.y = target.y() as i16;
    }
    set_mode(world, i, target.0 as i32, mode);
}

/// 0x58a5f0: walk to the building's door (`SetMode(door, 0x200000)`), the role's task list
/// emptied, the settler's anim list 0 (its `*_CHECKIN` list) taken, SELECTABLE / SELECTED
/// cleared. AI notice 0x12 (its player, the soldier, its home) unless the target is a barracks
/// (building type 24).
pub fn go_to_building(world: &mut World, i: u16, building: u16) {
    let Some(door) = core::building_door(world, building) else { return };
    set_mode(world, i, door.0 as i32, MODE_GO_TO);
    core::take_anim_list(world, i, 0);
    if let Some(e) = world.entities.by_index_mut(i as u32) {
        e.flags &= !(flag::SELECTABLE | flag::SELECTED);
    }
    if world.entities.by_index(building as u32).is_some_and(|b| b.kind != 24) {
        let owner = world.entities.by_index(i as u32).map_or(0, |e| e.owner);
        let home = job(world, i).map_or(0, |j| j.home);
        crate::ai::post_event(world, 0x12, owner as i32, i as i32, home as i32);
    }
}

/// Event 0x18 `[vehicle]` (0x58a2fa): with the vehicle's place for him (vtbl+0x8c) and the
/// vehicle alive, he walks there (0x589810 with flags 0; its own list is then emptied, 0x52c380,
/// and his anim list 0, GO_TO_TARGET + CHECKIN, taken), the vehicle attaches him, SELECTABLE /
/// SELECTED go, he leaves his groups and the AI hears (0x12 with his home); else SELECTABLE and
/// the wait list.
fn called_to_vehicle(world: &mut World, i: u16, vehicle: u16) {
    let place = crate::vehicles::exit_place(world, vehicle, i);
    let alive = world.entities.by_index(vehicle as u32).is_some_and(|e| e.flags & flag::ALIVE_MASK != 0);
    if place == 0 || !alive {
        if let Some(e) = world.entities.by_index_mut(i as u32) {
            e.flags |= flag::SELECTABLE;
        }
        core::take_wait_list(world, i);
        return;
    }
    walk_to(world, i, Y16X16(place as u32), 0);
    core::take_anim_list(world, i, 0);
    // (Deselected if he is the local player's, 0x4eda10: the client's.)
    crate::vehicles::attach(world, vehicle, i);
    if let Some(e) = world.entities.by_index_mut(i as u32) {
        e.flags &= !(flag::SELECTABLE | flag::SELECTED);
    }
    if crate::group::word(world, i) != 0 {
        crate::group::remove_all(world, i);
    }
    let (owner, home) = (world.entities.by_index(i as u32).map_or(0, |e| e.owner), job(world, i).map_or(0, |j| j.home));
    crate::ai::post_event(world, 0x12, owner as i32, i as i32, home as i32);
}

/// Event 0x19 `[0, at]` (0x58a4e0): on the map near `at` (0x582a40), into the war map again
/// (0x5bb560), off board, VISIBLE and SELECTABLE, the behaviour reset (`SetMode(−1, 0)`), task
/// 0x1b; the AI hears (0x15 with his home) unless he is of the Dark Tribe; a manakopter home
/// lets him go (vtbl+0x40, 0x569390).
fn ashore(world: &mut World, i: u16, at: Y16X16) {
    crate::create::put_settler_near(world, i, at.x(), at.y());
    crate::warmap::readd(world, i);
    let Some(e) = world.entities.by_index_mut(i as u32) else { return };
    e.flags = (e.flags & !flag::ON_BOARD) | 0x2100;
    let (owner, race) = (e.owner, e.race);
    set_mode(world, i, -1, 0);
    if let Some(j) = job_mut(world, i) {
        j.task = TASK_BEHAVIOUR;
    }
    let home = job(world, i).map_or(0, |j| j.home);
    if race != 3 {
        crate::ai::post_event(world, 0x15, owner as i32, i as i32, home as i32);
    }
    if world.entities.by_index(home as u32).is_some_and(|e| e.obj_type == ObjType::Flying) {
        crate::flying::detach(world, home, i);
    }
}

/// Event 0x1c `[manakopter]` (0x58a440): a living manakopter (obj type 0x40) with a position
/// is walked to (0x589810, then `SetMode(pos, 0x200000)`), anim list 0, no longer SELECTABLE
/// (nor selected), and it takes the settler on (vtbl+0x70, [`crate::flying::attach`]: its
/// observer and home). Otherwise the normal update continues (flag bit 31).
fn called_to_manakopter(world: &mut World, i: u16, mk: u16) {
    if !crate::flying::alive(world, mk) {
        if let Some(e) = world.entities.by_index_mut(i as u32) {
            e.flags |= core::FLAG_CONTINUE_AFTER_EVENT;
        }
        return;
    }
    let pos = world.entities.by_index(mk as u32).map_or(Y16X16(0), |e| e.pos);
    if pos.0 == 0 {
        return;
    }
    walk_to(world, i, pos, 0);
    set_mode(world, i, pos.0 as i32, MODE_GO_TO);
    core::take_anim_list(world, i, 0);
    if let Some(e) = world.entities.by_index_mut(i as u32) {
        e.flags &= !(flag::SELECTABLE | flag::SELECTED);
    }
    crate::flying::attach(world, mk, i);
}

/// `CSoldierRole::SetFree` (0x58aa30): the home (a building or a manakopter) `Detach`es the
/// soldier (vtbl+0x40). Returns false.
pub fn set_free(world: &mut World, i: u16, _eco_sector: i32) -> bool {
    let home = job(world, i).map_or(0, |j| j.home);
    if home != 0 && world.entities.by_index(home as u32).is_some() {
        super::hooks::home_detach(world, home, i);
    }
    false
}

// ---- CSquadLeaderRole ----------------------------------------------------------------------

/// The settler is a squad leader (`CSquadLeaderRole`, role 23: a `CSoldierRole` whose behaviour
/// vtable 0x105b7c0 overrides slots 0, 4, 8 and 0xc, and whose role vtable 0x105b7f0 overrides
/// `LogicUpdateJob` and `Init`).
pub(super) fn is_leader(world: &World, i: u16) -> bool {
    job(world, i).is_some_and(|j| j.role == role::SQUAD_LEADER)
}

/// `CSquadLeaderRole::Init` (0x58ac90): `CSoldierRole::Init`, then +0x60 / +0x64 / +0x68 = 0 and
/// the anchor +0x6c = −1.
pub fn leader_init_role(world: &mut World, i: u16) {
    init_role(world, i);
    if let Some(s) = soldier(world, i) {
        s.lead_tick = 0;
        s.lead_target = 0;
        s.anchor = -1;
    }
}

/// `CSquadLeaderRole::LogicUpdateJob` (0x58acc0): [`logic_update_job`], then the anchor becomes
/// the leader's position unless the behaviour is idle (state 0) with an anchor set.
pub fn leader_logic_update_job(world: &mut World, i: u16) {
    logic_update_job(world, i);
    let pos = world.entities.by_index(i as u32).map_or(0, |e| e.pos.0 as i32);
    if is_leader(world, i)
        && let Some(s) = soldier(world, i)
        && (s.state != 0 || s.anchor < 1)
    {
        s.anchor = pos;
    }
}

/// 0x58b0c0: the hex distance of two packed positions (x the low word, y the signed high word).
fn packed_distance(a: i32, b: i32) -> i32 {
    let (a, b) = (Y16X16(a as u32), Y16X16(b as u32));
    crate::spiral::grid_distance(b.x() - a.x(), b.y() - a.y())
}

/// 0x58ad30 after the step `r`: a leader in a group (word bits 12..15) that is SELECTABLE and
/// whose step has 0x10 walks back to its anchor when 3 or more away from it in the same sector
/// (`WalkToXY(anchor, (mode & 0x1060) | 0x2000)`); otherwise (no anchor, another sector) the
/// anchor becomes its position. Then [`lead`].
fn leader_step(world: &mut World, i: u16, r: u32) {
    let Some(e) = world.entities.by_index(i as u32) else { return };
    let (pos, flags) = (e.pos.0 as i32, e.flags);
    let Some(j) = job(world, i) else { return };
    let (groups, anchor, mode) = (j.groups, j.soldier.anchor, j.soldier.mode);
    if r & 0x10 != 0 && groups & 0xf000 != 0 && flags & flag::SELECTABLE != 0 {
        let sector = |p: i32| {
            let p = Y16X16(p as u32);
            world.grid.checked_index(p.x(), p.y()).map_or(0, |n| world.tiling.sector_of(n))
        };
        if anchor > 0 && packed_distance(pos, anchor) < 3 {
            // Close enough: the anchor stays.
        } else if anchor > 0 && sector(anchor) == sector(pos) {
            ActivePathing::walk_to_xy(world, i, Y16X16(anchor as u32), ((mode & 0x1060) | 0x2000) as i32);
        } else if let Some(s) = soldier(world, i) {
            s.anchor = pos;
        }
    }
    lead(world, i);
}

/// Walking vtbl+0x20 (0x5b9f40): the coarse waypoint (+0x2c) while the walk state is 6 or 9,
/// else −1.
fn waypoint(world: &World, i: u16) -> i32 {
    match job(world, i).and_then(|j| j.walking.as_ref()) {
        Some(w) if w.state == 6 || w.state == 9 => w.coarse,
        _ => -1,
    }
}

/// `(int)(sqrt(n) · 0.75 + 3.0)` (0x58aea0 with the doubles .rdata 0x1070e20 / 0x1070e48) in
/// integers: `3 + ⌊√(9n)⌋ / 4`. Equal for every `n` the groups allow: 0.75·√n is either an
/// integer (computed exactly) or at least 1/60 away from one.
pub fn lead_reach(n: u32) -> i32 {
    3 + ((9 * n).isqrt() / 4) as i32
}

/// 0x58aea0, the squad leader leads its group, at most every 45 ticks: with its group word's
/// group `g` (bits 12..15) of more than one member, the places of [`crate::group::formation`]
/// for all `n` members (flags 0, from the leader's position) around its coarse waypoint (else its
/// position) go in turn to the members that are soldiers (role vtbl+0x58 0x58aa60: the warrior
/// behaviour; other roles have none and take no place). A member is sent (behaviour
/// `SetMode(place, mode)`, mode 0x200000 if the leader's has it, else 0x100000) unless the group
/// was led before and the member's target position is within [`lead_reach`] of the leader or of
/// its place. Without a group (or alone in it) +0x68 = 0 and nothing else.
pub(super) fn lead(world: &mut World, i: u16) {
    let tick = world.tick;
    let Some(j) = job(world, i) else { return };
    let (groups, s) = (j.groups, j.soldier);
    if tick < s.lead_tick {
        return;
    }
    let Some(e) = world.entities.by_index(i as u32) else { return };
    let (owner, pos) = (e.owner, e.pos.0 as i32);
    let g = (groups >> 12) as usize;
    let n = if g != 0 { world.groups.count(owner, g) } else { 0 };
    if n <= 1 {
        if let Some(s) = soldier(world, i) {
            s.lead_target = 0;
        }
        return;
    }
    let mode = if s.mode & 0x20_0000 != 0 { 0x20_0000 } else { 0x10_0000 };
    let reach = lead_reach(n as u32);
    let prev = s.lead_target;
    let target = match waypoint(world, i) {
        t if t < 0 => pos,
        t => t,
    };
    let t = Y16X16(target as u32);
    let places = crate::group::formation(world, t.x(), t.y(), n, 0, pos).positions;
    let mut k = 0;
    for m in world.groups.members(owner, g).to_vec() {
        if m == i {
            continue;
        }
        let Some(at) = job(world, m).filter(|j| matches!(j.role, role::SOLDIER | role::SQUAD_LEADER | role::SABOTEUR)).map(|j| j.soldier.target_pos) else {
            continue;
        };
        let place = places[k].0 as i32;
        k = if k + 1 >= places.len() { 0 } else { k + 1 };
        if prev > 0 && (packed_distance(at, pos) <= reach || packed_distance(at, place) <= reach) {
            continue;
        }
        set_mode(world, m, place, mode);
    }
    if let Some(s) = soldier(world, i) {
        s.lead_target = target;
        s.lead_tick = tick + 45;
    }
}

// ---- CSaboteurRole -------------------------------------------------------------------------

/// Entity flag 0x10000000: a saboteur hiding (not vulnerable; [`camouflage`]). `ChangeType`
/// clears it.
pub const FLAG_HIDDEN: u32 = 0x1000_0000;

/// `CSaboteurRole::Init` (0x57fd60): `CSoldierRole::Init`, then +0x60 (the next camouflage
/// check) = 0.
pub fn saboteur_init_role(world: &mut World, i: u16) {
    init_role(world, i);
    if let Some(s) = soldier(world, i) {
        s.lead_tick = 0;
    }
}

/// `CSaboteurRole::LogicUpdate` (vtbl+0x18, 0x57fd80; the role is a `CSoldierRole` with
/// `GetSettlerRole` 10, behaviour row 19): every time +0x60 is due, [`camouflage`] sets the next
/// check. At the end of a WORK task (the attack on a building, started by the behaviour's
/// `Attack` 0x57fe40) the task becomes 0x1b, so the soldier's damage at the end of WORK
/// (0x589250) is never dealt; instead the target (role +0x22), if READY and vulnerable, takes the
/// saboteur's settler info +3 (no fighting strength) from its player with chance
/// [`crate::rules::Rules::saboteur_chance`] (one game-RNG draw). Then the base `LogicUpdate`
/// (0x586930). The target's owner gets GUI_WARN_ATTACK_BUILD (0x57fe10) before the damage.
pub fn saboteur_logic_update(world: &mut World, i: u16) {
    let tick = world.tick;
    if job(world, i).is_some_and(|j| j.soldier.lead_tick <= tick) {
        let n = camouflage(world, i);
        if let Some(s) = soldier(world, i) {
            s.lead_tick = tick + n;
        }
    }
    if let Some(j) = job_mut(world, i)
        && j.task == task_type::WORK
    {
        j.task = TASK_BEHAVIOUR;
        let target = j.entity;
        let hit = world.entities.by_index(target as u32).is_some_and(|t| t.flags & flag::READY != 0 && t.flags & flag::VULNERABLE_MASK != 0);
        if hit && world.rng.rand() < world.rules.saboteur_chance() {
            let Some(e) = world.entities.by_index(i as u32) else { return };
            let (d, owner) = (world.rules.settler_damage(e.race, e.kind) as i32, e.owner);
            if let Some(t) = world.entities.by_index(target as u32) {
                let ev = crate::event::SimEvent::warning(t.owner, 0x990, t.pos);
                world.events.push(ev);
            }
            crate::combat::take_damage(world, target, d, owner);
        }
    }
    core::role_logic_update(world, i);
}

/// 0x5801f0: a visible saboteur on land of its own team or of no team hides (vulnerable bits
/// cleared, [`FLAG_HIDDEN`]) unless an enemy warrior is within 15 (0x5b0a20); a hidden one shows
/// itself (vulnerable again) when an enemy warrior is within 6. Returns the ticks to the next
/// check: 15 while hidden, else 45.
fn camouflage(world: &mut World, i: u16) -> u32 {
    let Some(e) = world.entities.by_index(i as u32) else { return 45 };
    let (pos, owner, flags) = (e.pos, e.owner, e.flags);
    let land = world.grid.checked_index(pos.x(), pos.y()).map_or(0, |n| world.economy.owner(world.tiling.eco_sector_of(n)));
    let land_team = crate::warmap::team(world, land);
    let flags = if flags & FLAG_HIDDEN == 0 {
        if (land_team == crate::warmap::team(world, owner) || land_team == 0) && !crate::warmap::enemy_warriors_near(world, pos.x(), pos.y(), 15, owner) {
            (flags & !flag::VULNERABLE_MASK) | FLAG_HIDDEN
        } else {
            flags
        }
    } else if crate::warmap::enemy_warriors_near(world, pos.x(), pos.y(), 6, owner) {
        (flags & !FLAG_HIDDEN) | flag::VULNERABLE_MASK
    } else {
        flags
    };
    if let Some(e) = world.entities.by_index_mut(i as u32) {
        e.flags = flags;
    }
    if flags & FLAG_HIDDEN != 0 { 15 } else { 45 }
}

/// Behaviour vtbl+0xc of the saboteur (0x57fe40), before `Attack` (0x58a750): +0x60 (the next
/// camouflage check) = tick + 45, and a hidden saboteur shows itself (vulnerable again).
pub(super) fn saboteur_attack(world: &mut World, i: u16) {
    let tick = world.tick;
    if let Some(s) = soldier(world, i) {
        s.lead_tick = tick + 0x2d;
    }
    if let Some(e) = world.entities.by_index_mut(i as u32)
        && e.flags & FLAG_HIDDEN != 0
    {
        e.flags = (e.flags & !FLAG_HIDDEN) | flag::VULNERABLE_MASK;
    }
}

pub(super) fn is_saboteur(world: &World, i: u16) -> bool {
    job(world, i).is_some_and(|j| j.role == role::SABOTEUR)
}

// ---- CTowerSoldier -------------------------------------------------------------------------

/// `CTowerSoldier::Init` (0x58c120): +0x12 = own id, displacement costs 10, slot / state /
/// direction 0.
fn tower_init(world: &mut World, i: u16) {
    if let Some(j) = job_mut(world, i) {
        j.w18 = i;
        j.displacement_costs = 10;
        j.tower = TowerData::default();
    }
}

/// vtbl+0x14 (0x58c140): a `CWalkingNormal` of the owner, `Init(-1, 0)`.
fn tower_create_walking(world: &mut World, i: u16) {
    ActivePathing::create(world, i, crate::pathing::walking::walking_type::NORMAL);
    ActivePathing::walk_to_xy(world, i, Y16X16(u32::MAX), 0);
}

/// `CTowerSoldier::TakeJob` (0x58c1a0): `InitCommonTaskValues`; WORK re-registers after its
/// duration, WAIT after 1.
pub fn tower_take_job(world: &mut World, i: u16) {
    let Some(t) = core::actual_task(world, i) else { return };
    core::init_common_task_values(world, i, &t);
    let Some(j) = job(world, i) else { return };
    match j.task {
        task_type::WORK => {
            let d = j.counter as i32;
            register(world, i, d);
        }
        task_type::WAIT => register(world, i, 1),
        _ => {}
    }
}

/// `CTowerSoldier::LogicUpdate` (0x58c200), vtbl+0x18 (it replaces the base); only WORK and WAIT
/// are handled. At the end of a WORK task (a shot) the target (+0x22), if READY, takes
/// `(shot strength · own-land fighting strength + 127) >> 8` from the soldier's player (no
/// minimum), and +0x30 notes the tick. Then by slot state: 0 — nothing, the soldier is not
/// updated again; 1 — an enemy next to the door (0x58c8e0) gets a stone (anim list 2, damage
/// info +3 plus `TowerStoneThrowerExtraDmg`, the slot's direction kept), else as 2; 2 — an enemy
/// within 20 (0x58c9d0), damage plus `TowerBowmanExtraDmg`; 3 (door guard) — an enemy next to
/// it, damage plus `TowerDoorGuardExtraDmg`, else (45 ticks after the last hit or start) back
/// into the building (0x58c5f0). States 2 and 3 turn to the target and take anim list 1 (3 for
/// a bowman guard). Without an enemy: target 0, the slot's direction, then after a shot the wait
/// list again, while waiting a new update after `31 − Rand() % 4` ticks (state 3: `4 − Rand() %
/// 4`). Bowmen start a flying stone (state 1) or arrow ([`crate::projectile`], drawn only).
pub fn tower_logic_update(world: &mut World, i: u16) {
    let Some(j) = job(world, i) else { return };
    let (task, state, home, target, strength) = (j.task, j.tower.state, j.home, j.entity, j.tower.strength);
    let Some(e) = world.entities.by_index(i as u32) else { return };
    let (owner, pos, race, kind) = (e.owner, e.pos, e.race, e.kind);
    if task == task_type::WORK {
        if world.entities.by_index(target as u32).is_some_and(|t| t.flags & flag::READY != 0) {
            let own = world.statistic.players.get(owner as usize).map_or(0x100, |p| p.own);
            if let Some(ev) = crate::event::SimEvent::attacked(world, target) {
                world.events.push(ev);
            }
            crate::combat::take_damage(world, target, (strength as i32 * own + 0x7f) >> 8, owner);
            let tick = world.tick;
            if let Some(j) = job_mut(world, i) {
                j.tower.tick = tick;
            }
        }
    } else if task != task_type::WAIT {
        return;
    }
    let door = core::building_door(world, home).unwrap_or(Y16X16(0));
    let bowner = world.entities.by_index(home as u32).map_or(owner, |b| b.owner);
    let logic = &world.rules.logic;
    let (stone, bow, guard) = (logic.tower_stone_thrower_extra_dmg, logic.tower_bowman_extra_dmg, logic.tower_door_guard_extra_dmg);
    let damage = world.rules.settler_damage(race, kind) as i32;
    let (enemy, extra) = match state {
        0 => return,
        1 => {
            let e = door_enemy(world, bowner, door);
            if e != 0 {
                if let Some(j) = job_mut(world, i) {
                    j.entity = e;
                    j.tower.strength = (damage + stone) as u8;
                    let dir = j.tower.dir;
                    if let Some(s) = settler_mut(world, i) {
                        s.direction = dir;
                    }
                }
                core::take_anim_list(world, i, 2);
                // A bowman throws a stone (0x58c38a): kind 5 from its slot.
                if world.rules.warrior_type(kind) == 3 {
                    tower_shot(world, i, home, 5);
                }
                return;
            }
            (far_enemy(world, owner, pos), bow)
        }
        2 => (far_enemy(world, owner, pos), bow),
        3 => {
            let e = door_enemy(world, owner, pos);
            let gtick = job(world, i).map_or(0, |j| j.tower.tick);
            if e == 0 && gtick.wrapping_add(0x2d) <= world.tick {
                // 0x58c5f0: target cleared, the building's SettlerEnter takes the guard back.
                if let Some(j) = job_mut(world, i) {
                    j.entity = 0;
                }
                crate::buildings::settler_enter(world, home, i);
                return;
            }
            (e, guard)
        }
        _ => (0, 0),
    };
    if enemy != 0 {
        let to = world.entities.by_index(enemy as u32).map_or(pos, |t| t.pos);
        let dir = crate::pathing::walking::direction(pos.0 as i32, to.0 as i32) as u8;
        if let Some(j) = job_mut(world, i) {
            j.entity = enemy;
            j.tower.strength = (damage + extra) as u8;
        }
        if let Some(s) = settler_mut(world, i) {
            s.direction = dir;
        }
        let bow_guard = state == 3 && world.rules.warrior_type(kind) == 3;
        core::take_anim_list(world, i, if bow_guard { 3 } else { 1 });
        // A bowman shoots an arrow from its slot (0x58c46f).
        if !bow_guard && world.rules.warrior_type(kind) == 3 {
            tower_shot(world, i, home, 0);
        }
        return;
    }
    if let Some(j) = job_mut(world, i) {
        j.entity = 0;
        let dir = j.tower.dir;
        if let Some(s) = settler_mut(world, i) {
            s.direction = dir;
        }
    }
    if task != task_type::WAIT {
        // vtbl+0x70: the wait list again (its TakeJob registers).
        core::take_wait_list(world, i);
        return;
    }
    let base = if state == 3 { 4 } else { 31 };
    let r = world.rng.rand();
    register(world, i, base - (r & 3) as i32);
}

/// A tower soldier's flying effect (`CEffects` vtbl+0x18): `kind` from its node, drawn from the
/// slot's pixel offset (building info +0xe0 + 0xc · slot: the settler `xOffset` / `yOffset` of
/// the soldier's race and the building's type), to its target, 15 ticks from now.
fn tower_shot(world: &mut World, i: u16, home: u16, kind: u8) {
    let Some(j) = job(world, i) else { return };
    let (slot, target) = (j.tower.slot as usize, j.entity);
    let race = world.entities.by_index(i as u32).map_or(0, |e| e.race & 0xf);
    let btype = world.entities.by_index(home as u32).map_or(0, |b| b.kind as u8);
    let offset = world.rules.building(race, btype).soldier_slots.get(slot).map_or((0, 0), |s| (s.x, s.y));
    crate::projectile::launch_at(world, kind, i, target, 15, offset);
}

/// Whether `e` is a target of the tower searches: a warrior of another team, READY and
/// vulnerable to fights (0x20000) but not BIRTH (flags & 0x3020000 == 0x2020000), on land whose
/// eco-sector owner is of the searching team or nobody.
fn tower_target(world: &World, player: u8, n: usize, m: u16) -> bool {
    let Some(e) = world.entities.by_index(m as u32) else { return false };
    let land = crate::warmap::team(world, world.economy.owner(world.tiling.eco_sector_of(n)));
    e.warrior_type() != 0
        && crate::warmap::team(world, e.owner) != crate::warmap::team(world, player)
        && e.flags & 0x302_0000 == 0x202_0000
        && (land == crate::warmap::team(world, player) || land == 0)
}

/// 0x58c8e0 `(player, pos)`: the first [`tower_target`] on the 6 neighbours of `pos` (spiral
/// entries 1..=6) whose node is not blocked (flags bit 0).
fn door_enemy(world: &World, player: u8, pos: Y16X16) -> u16 {
    for k in 1..=6 {
        let (x, y) = (pos.x() + crate::spiral::dx(k), pos.y() + crate::spiral::dy(k));
        let Some(n) = world.grid.checked_index(x, y) else { continue };
        let m = world.layers.map_object[n];
        if world.layers.flags[n] & 1 == 0 && m != 0 && tower_target(world, player, n, m) {
            return m;
        }
    }
    0
}

/// 0x58c9d0 `(player, pos)`: a bowman's search. Nothing unless the war map has an enemy cell
/// within 20 (0x5b06d0); then the nearest READY manakopter nearer than 20 of a Dark Tribe player
/// (race 3) of another team (0x569e60 per player); without one the first [`tower_target`]
/// without flag 0x8000000 on spiral entries `First(1)..=Last(20)` closer than 21.
fn far_enemy(world: &World, player: u8, pos: Y16X16) -> u16 {
    if !crate::warmap::enemy_near(world, pos.x(), pos.y(), player) {
        return 0;
    }
    let team = crate::warmap::team(world, player);
    let mut best = (0x14, 0u16);
    for p in 1..=world.player_count() {
        if world.race_of(p) != 3 || crate::warmap::team(world, p) == team {
            continue;
        }
        let m = crate::flying::nearest(world, p, pos.x(), pos.y(), 0);
        if let Some(e) = world.entities.by_index(m as u32).filter(|e| m != 0 && e.flags & flag::READY != 0) {
            let d = crate::spiral::grid_distance(e.x() - pos.x(), e.y() - pos.y());
            if d < best.0 {
                best = (d, m);
            }
        }
    }
    if best.1 != 0 {
        return best.1;
    }
    for k in crate::spiral::first(1)..=crate::spiral::last(20) {
        let (dx, dy) = (crate::spiral::dx(k), crate::spiral::dy(k));
        if crate::spiral::grid_distance(dx, dy) > 20 {
            continue;
        }
        let Some(n) = world.grid.checked_index(pos.x() + dx, pos.y() + dy) else { continue };
        let m = world.layers.map_object[n];
        if world.layers.flags[n] & 1 == 0
            && m != 0
            && world.entities.by_index(m as u32).is_some_and(|e| e.flags & 0x800_0000 == 0)
            && tower_target(world, player, n, m)
        {
            return m;
        }
    }
    0
}

/// `CTowerSoldier::GetNextJob` (0x58c570): the next task, from the start again after the last.
pub fn tower_get_next_job(world: &mut World, i: u16) {
    if let Some(j) = job_mut(world, i) {
        j.cursor += 1;
    }
    if core::is_end_iter(world, i)
        && let Some(j) = job_mut(world, i)
    {
        j.cursor = 0;
    }
    tower_take_job(world, i);
}

/// `CTowerSoldier::ConvertEventIntoGoal` (0x58c6f0).
pub fn tower_convert_event(world: &mut World, i: u16, ev: &EntityEvent) {
    match ev.event {
        // The building is gone: out, selectable, its wait list, no home (AI notice 0x15).
        7 | 9 => {
            if job(world, i).is_none_or(|j| j.home == 0) {
                return;
            }
            if let Some(e) = world.entities.by_index_mut(i as u32) {
                e.flags = (e.flags & !flag::ON_BOARD) | flag::SELECTABLE;
            }
            core::take_wait_list(world, i);
            let owner = world.entities.by_index(i as u32).map_or(0, |e| e.owner);
            let home = job(world, i).map_or(0, |j| j.home);
            crate::ai::post_event(world, 0x15, owner as i32, i as i32, home as i32);
            if let Some(j) = job_mut(world, i) {
                j.home = 0;
            }
        }
        0xf | 0x19 => unported(world, "CTowerSoldier::ConvertEventIntoGoal (0xf / 0x19)", i),
        _ => {
            if world.entities.by_index(i as u32).is_some_and(|e| e.flags & flag::REGISTERED == 0) {
                register(world, i, 1);
            }
        }
    }
}

/// `CTowerSoldier::SetFree` (0x58c5b0): the home building's `SettlerLeft` (role +0x50), no
/// home. Returns false.
pub fn tower_set_free(world: &mut World, i: u16, _eco_sector: i32) -> bool {
    let home = job(world, i).map_or(0, |j| j.home);
    if home != 0 {
        crate::buildings::settler_left(world, home, i);
        if let Some(j) = job_mut(world, i) {
            j.home = 0;
        }
    }
    false
}

/// Whether `i` is a soldier inside a building (`CTowerSoldier`).
pub fn is_tower_soldier(world: &World, i: u16) -> bool {
    matches!(world.entities.by_index(i as u32).map(|e| &e.data), Some(EntityData::Settler(s)) if s.job.role == role::TOWER_SOLDIER)
}
