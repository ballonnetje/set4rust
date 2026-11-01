//! What all roles share: `CSettler::LogicUpdate` / `NewToDoList` / `TakeWaitList` /
//! `TakeAnimList` / `Walk` / `WalkDir`, the `IMovingEntity` to-do iterator, and the base
//! `ISettlerRole` methods (`LogicUpdate`, `Go`, `Update`, `InitCommonTaskValues`, `CheckHome`).
//! Role-specific virtuals are dispatched in [`super::roles`].

use super::hooks;
use super::roles;
use super::state::{JobState, ToDo};
use super::task::{EntityTask, task_type};
use super::walking::{ActivePathing, NEIGHBOR, Pathing, WALK_ARRIVED, WALK_BLOCKED, WALK_FAILED, WALK_NONE};
use crate::entity::{EntityData, SettlerData, flag};
use crate::event::SimEvent;
use crate::grid::Y16X16;
use crate::World;

/// Entity flag 0x800, set by `InitCommonTaskValues` for the in-building tasks
/// (GO/WORK/WAIT/PUT/GET_GOOD_VIRTUAL, WORK_ON_ENTITY_VIRTUAL, RESOURCE_GATHERING_VIRTUAL).
/// Meaning not traced (inferred: "inside / not on the map").
pub const FLAG_VIRTUAL_TASK: u32 = 0x800;

pub(crate) fn settler_mut(world: &mut World, i: u16) -> Option<&mut SettlerData> {
    match &mut world.entities.by_index_mut(i as u32)?.data {
        EntityData::Settler(s) => Some(s),
        _ => None,
    }
}

pub(crate) fn job(world: &World, i: u16) -> Option<&JobState> {
    match &world.entities.by_index(i as u32)?.data {
        EntityData::Settler(s) => Some(&s.job),
        _ => None,
    }
}

pub(crate) fn job_mut(world: &mut World, i: u16) -> Option<&mut JobState> {
    settler_mut(world, i).map(|s| &mut s.job)
}

/// `IAnimatedEntity::RegisterForLogicUpdate(delta)`.
pub(crate) fn register(world: &mut World, i: u16, delta: i32) {
    world.register_for_logic_update(i, delta);
}

/// The tasks of the settler's current to-do list.
pub(crate) fn todo_tasks<'a>(world: &'a World, j: &'a JobState) -> &'a [EntityTask] {
    match &j.todo {
        ToDo::None => &[],
        ToDo::Shared { race, list } => world.rules.jobs.list(*race, *list).map_or(&[], |l| &l[..]),
        ToDo::Own(v) => v,
    }
}

/// `IMovingEntity::GetActualTask` (`*m_iToDoIter`).
pub fn actual_task(world: &World, i: u16) -> Option<EntityTask> {
    let j = job(world, i)?;
    todo_tasks(world, j).get(j.cursor as usize).copied()
}

/// `IMovingEntity::IsEndIter`.
pub(crate) fn is_end_iter(world: &World, i: u16) -> bool {
    job(world, i).is_none_or(|j| j.cursor as usize >= todo_tasks(world, j).len())
}

/// `CSettler::NewToDoList(list, id)` (release 0x580fb0, vtbl+112). `None` falls back to the wait list
/// when one exists for the settler's type (and otherwise does nothing).
pub fn new_to_do_list(world: &mut World, i: u16, list: Option<ToDo>, id: i16) {
    let Some(e) = world.entities.by_index(i as u32) else { return };
    if e.flags & flag::DIED != 0 {
        return; // "STOP this settler is dead"
    }
    let (race, kind) = (e.race, e.kind);
    match list {
        Some(todo) => {
            if let Some(j) = job_mut(world, i) {
                j.list_id = id;
                j.todo = todo;
                j.cursor = 0;
            }
            roles::take_job(world, i);
        }
        None => {
            if world.rules.jobs.list(race, kind).is_some() {
                take_wait_list(world, i);
            }
        }
    }
}

/// `CSettler::TakeWaitList` (release 0x581020): list `type` of the settler's race.
pub fn take_wait_list(world: &mut World, i: u16) {
    let Some(e) = world.entities.by_index(i as u32) else { return };
    let (race, kind) = (e.race, e.kind);
    let list = world.rules.jobs.list(race, kind).map(|_| ToDo::Shared { race, list: kind });
    new_to_do_list(world, i, list, kind as i16);
}

/// `CSettler::TakeAnimList(index)` (release 0x581050): the `index`-th list of the settler's
/// `SettlerInfo::m_vAnimLists` (0 = check-in list, 1 = work list, ...).
pub fn take_anim_list(world: &mut World, i: u16, index: usize) {
    let Some(e) = world.entities.by_index(i as u32) else { return };
    let (race, kind) = (e.race, e.kind);
    // "(_iAnimListIndex >= 0) && (_iAnimListIndex < iNumberOfAnimLists)" is an assert only.
    let id = world.rules.jobs.anim_list(race, kind, index).unwrap_or(0);
    let list = world.rules.jobs.list(race, id).map(|_| ToDo::Shared { race, list: id });
    new_to_do_list(world, i, list, id as i16);
}

/// `IMovingEntity::SetToDoList(list)` (release 0x56df20): a role-built list; list id −1.
pub fn set_to_do_list(world: &mut World, i: u16, tasks: Vec<EntityTask>) {
    crate::debug::log(world, i, "todo", tasks.len() as i32, 0);
    if let Some(j) = job_mut(world, i) {
        j.todo = ToDo::Own(tasks);
        j.list_id = -1;
        j.cursor = 0;
    }
}

/// `CSettler::LogicUpdate` (release 0x5814f0).
pub fn logic_update(world: &mut World, i: u16) {
    let Some(e) = world.entities.by_index_mut(i as u32) else { return };
    if e.flags & flag::DIED != 0 {
        return;
    }
    if e.flags & 0x80 != 0 {
        e.flags &= !0x80;
        // role vtbl+60 (eco-sector changed, `ISettlerRole::ESChanged`); returns whether it took over.
        if hooks::es_changed(world, i) {
            return;
        }
    }
    if process_all_events(world, i) {
        return;
    }
    roles::logic_update(world, i);
}

/// `IAnimatedEntity::SetEvent(event)` (release 0x532e60): queue an event for the settler and make sure it
/// is scheduled (registered for the next tick if it is not).
pub fn set_event(world: &mut World, i: u16, ev: super::state::EntityEvent) {
    crate::debug::log(world, i, "event", ev.event, ev.kind);
    let registered = world.entities.by_index(i as u32).is_some_and(|e| e.flags & flag::REGISTERED != 0);
    if !registered {
        register(world, i, 1);
    }
    if let Some(j) = job_mut(world, i) {
        j.events.push(ev);
    }
}

/// Entity flag bit 31: a role sets it while handling an event to ask for the normal update too.
pub const FLAG_CONTINUE_AFTER_EVENT: u32 = 0x8000_0000;

/// `IAnimatedEntity::ProcessAllEvents` (release 0x532be0): hand every queued event, oldest first, to the
/// role's `ConvertEventIntoGoal` (vtbl+108 of the settler → role). Returns true (skip the rest
/// of `LogicUpdate`) when events were processed and the last one did not set
/// [`FLAG_CONTINUE_AFTER_EVENT`].
fn process_all_events(world: &mut World, i: u16) -> bool {
    let mut done = false;
    loop {
        let Some(j) = job_mut(world, i) else { return done };
        if j.events.is_empty() {
            return done;
        }
        let ev = j.events.remove(0);
        if let Some(e) = world.entities.by_index_mut(i as u32) {
            e.flags &= !FLAG_CONTINUE_AFTER_EVENT;
        }
        roles::convert_event_into_goal(world, i, &ev);
        done = world.entities.by_index(i as u32).is_some_and(|e| e.flags & FLAG_CONTINUE_AFTER_EVENT == 0);
    }
}

/// Node flag 0x20, cleared at a free settler's old destination (`ISettlerRole::SetFree`;
/// inferred: "destination reserved").
pub const NODE_FLAG_DESTINATION: u8 = 0x20;

/// `ISettlerRole::SetFree(settler, eco_sector)` (release 0x587050, vtbl+64): detach from piles and home,
/// release the destination, then offer the settler to its eco-sector and give it the wait list
/// (warriors: just the wait list). On land of another owner a `CFleeRole` is created instead
/// (returns true then).
pub fn set_free_base(world: &mut World, i: u16, _eco_sector: i32) -> bool {
    for target in 1..=4 {
        hooks::detach_from_pile(world, i, target);
    }
    let Some(j) = job(world, i) else { return false };
    let (home, destination) = (j.home, j.destination);
    if home != 0 {
        // 0x587148: a building home's role hears `SettlerLeft` (role vtbl+0x50) first.
        if world.entities.by_index(home as u32).is_some_and(|e| e.obj_type == crate::entity::ObjType::Building) {
            crate::buildings::settler_left(world, home, i);
        }
        hooks::home_detach(world, home, i);
        if let Some(j) = job_mut(world, i) {
            j.home = 0;
        }
        if let Some(e) = world.entities.by_index_mut(i as u32) {
            e.flags &= !flag::ATTACHED;
        }
    }
    if destination.0 != 0
        && let Some(n) = world.grid.checked_index(destination.x(), destination.y())
    {
        world.layers.flags[n] &= !NODE_FLAG_DESTINATION;
    }
    let Some(e) = world.entities.by_index(i as u32) else { return false };
    let (warrior, owner, offered, pos) = (e.warrior_type(), e.owner, e.flags & flag::OFFERED != 0, e.pos);
    if warrior == 0 {
        // 0x5871a1: the owner of the tile at his node (tile +0 & 0xf, `ITiling::OwnerId`).
        let node = world.grid.checked_index(pos.x(), pos.y());
        let own_land = node.is_some_and(|n| world.owner_of(n) == owner);
        if !own_land {
            hooks::flee_role(world, i);
            return true;
        }
        if !offered {
            hooks::eco_sector_offer(world, i);
        }
    }
    take_wait_list(world, i);
    false
}

/// `ISettlerRole::LogicUpdate` (release 0x586930, vtbl+24): WAIT is handled here (idle; steps aside when
/// pushed), every other task by the role's `LogicUpdateJob` (vtbl+28).
pub fn role_logic_update(world: &mut World, i: u16) {
    let Some(j) = job(world, i) else { return };
    if j.role == super::state::role::NONE {
        return;
    }
    if j.task != task_type::WAIT {
        roles::logic_update_job(world, i);
        return;
    }
    if let Some(j) = job_mut(world, i) {
        j.distance = 0;
        j.cycle_frames = 1;
    }
    ActivePathing::walk_to_xy(world, i, Y16X16(u32::MAX), 0);
    let step = ActivePathing::next_step(world, i);
    let w = walk_dir(world, i, step);
    if let Some(j) = job_mut(world, i) {
        j.walk = w as u8;
    }
    if w & 0xF >= 6 {
        if let Some(j) = job_mut(world, i) {
            j.frame = 0;
        }
        let r = world.rng.rand();
        register(world, i, (r % 4 + 1) as i32);
    } else {
        let frames = actual_task(world, i).map_or(1, |t| t.frames);
        let delta = if frames > 1 { frames as i32 - 1 } else { 1 };
        if let Some(j) = job_mut(world, i) {
            j.cycle_frames = frames; // "m_iCycleFrames > 0" asserted
            j.frame = 0;
            j.distance = 0xFF;
        }
        register(world, i, delta);
    }
}

/// `CSettler::Walk` (inlined in `Go` 0x586c00): the next step from the walking object, applied by `WalkDir`.
pub fn settler_walk(world: &mut World, i: u16) -> u32 {
    let Some(e) = world.entities.by_index(i as u32) else { return u32::MAX };
    if e.flags & flag::DIED != 0 {
        return u32::MAX; // −1, "STOP this settler is dead"
    }
    if job(world, i).is_none_or(|j| j.walking.is_none()) {
        return WALK_NONE;
    }
    let step = ActivePathing::next_step(world, i);
    walk_dir(world, i, step)
}

/// `CSettler::WalkDir(step)` (release 0x581560): move one node in direction `step & 0xF` if the node is
/// free (`IsNotBlocked && IsNotOccupied`), else or in [`WALK_BLOCKED`]. A carrier (type 1)
/// wears the path (`IncreaseWalkCount`) unless bit 0x10 is set.
pub fn walk_dir(world: &mut World, i: u16, step: u32) -> u32 {
    let Some(e) = world.entities.by_index(i as u32) else { return u32::MAX };
    if e.flags & flag::DIED != 0 {
        return u32::MAX;
    }
    let d = (step & 0xF) as usize;
    if d >= 6 {
        return step;
    }
    let (pos, kind) = (e.pos, e.kind);
    let (dx, dy) = NEIGHBOR[d];
    let to = Y16X16::pack(pos.x() + dx, pos.y() + dy);
    let from_node = world.grid.checked_index(pos.x(), pos.y());
    let to_node = world.grid.checked_index(to.x(), to.y());
    let (Some(from_node), Some(to_node)) = (from_node, to_node) else { return step | WALK_BLOCKED };
    // The original traces "Invalid world map object id" when map_object[from] != i (no effect).
    let free = ActivePathing::is_not_blocked(world, i, to_node) && ActivePathing::is_not_occupied(world, i, to_node);
    if !free {
        // Both the turn-left and turn-right probes leave the result at −1: always blocked.
        return step | WALK_BLOCKED;
    }
    world.layers.map_object[from_node] = 0;
    world.layers.map_object[to_node] = i;
    if let Some(e) = world.entities.by_index_mut(i as u32) {
        e.pos = to;
    }
    if let Some(s) = settler_mut(world, i) {
        s.direction = d as u8;
        s.job.distance = 0xFF;
    }
    crate::warmap::moved(world, i);
    if kind == 1 && step & 0x10 == 0 {
        hooks::increase_walk_count(world, to_node);
    }
    d as u32 | (step & !0xF)
}

/// `ISettlerRole::Go` (release 0x586c00, vtbl+16): one walking step. Arrived →
/// `GetNextJob` (vtbl+36); failed → `SetFree(-1)` (vtbl+64); no step → retry next tick; a step
/// → re-register after the walk speed: 9 ticks, 8 on move cost 1, 7 on move cost 0 (the cost
/// rule only for carriers (1) and donkeys (60); everyone else always 9).
pub fn go(world: &mut World, i: u16) {
    let w = settler_walk(world, i);
    if let Some(j) = job_mut(world, i) {
        j.walk = w as u8;
    }
    if w & WALK_ARRIVED != 0 {
        roles::get_next_job(world, i);
        return;
    }
    if w & WALK_FAILED != 0 {
        roles::set_free(world, i, -1);
        return;
    }
    if w & 0xF >= 6 {
        register(world, i, 1);
        return;
    }
    let Some(e) = world.entities.by_index(i as u32) else { return };
    let (kind, pos) = (e.kind, e.pos);
    let mut speed: i8 = 9;
    if (kind == 1 || kind == 60)
        && let Some(n) = world.grid.checked_index(pos.x(), pos.y())
    {
        match world.layers.move_cost(n) {
            1 => speed = 8,
            0 => speed = 7,
            _ => {}
        }
    }
    if let Some(j) = job_mut(world, i) {
        j.distance = 0xFF; // `settler->unk_45 = -1`
        j.counter = speed;
    }
    register(world, i, speed as i32);
}

/// `IMovingEntity::DecDistance(a)`: 0xFF counts as 256.
fn dec_distance(j: &mut JobState, a: u32) {
    let d = if j.distance == 0xFF { 256 } else { j.distance as u32 };
    j.distance = if a >= j.distance as u32 { 0 } else { d.wrapping_sub(a) as u8 };
}

/// `ISettlerRole::Update` (release 0x586a00): advance the animation by the ticks since the last call.
/// Animated tasks play their gfx job forwards or backwards, fire the job sound on its frame and
/// signal war action for warriors; GO and WAIT animate the walk; other tasks go to the role's
/// `UpdateJob` (vtbl+32).
pub fn update(world: &mut World, i: u16) {
    let tick = world.tick;
    let Some(e) = world.entities.by_index(i as u32) else { return };
    let (x, y, warrior) = (e.x(), e.y(), e.warrior_type());
    let Some(j) = job_mut(world, i) else { return };
    // `(u16)GetTickCounter() - LastUpdateTick()`
    let elapsed = (tick as u16).wrapping_sub(j.last_update_tick as u16);
    j.elapsed = elapsed;
    if elapsed == 0 {
        return;
    }
    j.last_update_tick = tick;
    let cycle = j.cycle_frames as u32;
    match j.task {
        1 | 3 | 13 | 16 | 25 | 28 | 29 | 30 | 31 => {
            if cycle == 0 {
                return; // division by zero in the original ("m_iCycleFrames >= 1" asserted)
            }
            let e16 = elapsed as u32;
            j.frame = if j.forward {
                ((e16 + j.frame as u32) % cycle) as u8
            } else {
                let back = e16 % cycle;
                let f = j.frame as u32;
                (if f < back { (cycle + f - back) % cycle } else { f - back }) as u8
            };
            let (frame, job_part) = (j.frame, j.job_part);
            let snd = world.rules.jobs.sound(job_part);
            if snd.sound > 0 && snd.frame == frame {
                // `random == 100 || random >= rand() % 100`, `g_pFogging->IsVisible` and
                // `PlayEnvironmentSound` are client-side (CRT rand, local fog): the event
                // carries the chance.
                world.events.push(SimEvent::JobSound { settler: i, x, y, sound: snd.sound, chance: snd.random });
            }
            if (1u32 << warrior) & 0x3C != 0 {
                world.events.push(SimEvent::WarAction);
            }
        }
        6 | 17 => {
            if j.walk & 8 != 0 {
                j.frame = 0;
            } else {
                if cycle == 0 {
                    return;
                }
                j.frame = ((elapsed as u32 + j.frame as u32) % cycle) as u8;
                if j.frame == 0 {
                    j.frame = 1;
                }
                let div = if j.task == 6 {
                    j.counter as i32 // walk speed
                } else if cycle > 1 {
                    cycle as i32 - 1
                } else {
                    1
                };
                if div != 0 {
                    let a = ((elapsed as i32) << 8) / div;
                    dec_distance(j, a as u32);
                }
            }
        }
        _ => roles::update_job(world, i),
    }
}

/// `ISettlerRole::InitCommonTaskValues(settler, task)` (release 0x586d30).
pub fn init_common_task_values(world: &mut World, i: u16, t: &EntityTask) {
    crate::debug::log(world, i, "task", t.task as i32, t.job_part as i32);
    let virtual_task = matches!(t.task, 0 | 1 | 29 | 2 | 5 | 4 | 30);
    let Some(e) = world.entities.by_index_mut(i as u32) else { return };
    if virtual_task {
        e.flags |= FLAG_VIRTUAL_TASK;
    } else {
        e.flags &= !FLAG_VIRTUAL_TASK;
    }
    if t.visible {
        e.flags |= flag::VISIBLE;
    } else {
        e.flags &= !flag::VISIBLE;
    }
    let Some(s) = settler_mut(world, i) else { return };
    s.job.distance = 0;
    s.job.x = t.x;
    s.job.y = t.y;
    if t.dir >= 0 {
        s.direction = t.dir as u8;
    }
    let home = s.job.home;
    if t.trigger != 0 {
        // "m_uHomeEntityId != 0" asserted.
        if home != 0 {
            hooks::take_job_trigger(world, home, t.trigger);
        }
    }
    let Some(j) = job_mut(world, i) else { return };
    j.walk = 72;
    j.task = t.task;
    j.counter = t.duration;
    j.cycle_frames = t.frames;
    // "Invalid number of frames" is traced for cycle_frames <= 1; asserted >= 1.
    if t.entity != 0 {
        j.entity = t.entity;
    }
    if t.good != 0 {
        j.good = t.good;
    }
    j.job_part = t.job_part;
    j.forward = t.forward;
    j.frame = if j.forward { 0 } else { j.cycle_frames.wrapping_sub(1) };
}

/// `ISettlerRole::CheckHome` (release 0x5873b0, vtbl+124): a home building is required; without
/// one the settler is set free (`SetFree(-1)`).
pub fn check_home(world: &mut World, i: u16) -> bool {
    if job(world, i).is_some_and(|j| j.home != 0) {
        return true;
    }
    roles::set_free(world, i, -1);
    false
}

/// `CBuilding::DoorPackedXY` of building `b` (`None` if `b` is not a building).
pub fn building_door(world: &World, b: u16) -> Option<Y16X16> {
    match &world.entities.by_index(b as u32)?.data {
        EntityData::Building(d) => Some(d.door),
        _ => None,
    }
}

/// `CBuilding +0x4c`, the working area of building `b`.
pub fn building_working_area(world: &World, b: u16) -> Option<Y16X16> {
    match &world.entities.by_index(b as u32)?.data {
        EntityData::Building(d) => Some(d.working_area),
        _ => None,
    }
}

/// Position of entity `id`, if it exists (`CMapObjectMgr::EntityPtr(id)->PackedXY()`).
pub fn entity_pos(world: &World, id: u16) -> Option<Y16X16> {
    if id == 0 {
        return None;
    }
    world.entities.by_index(id as u32).map(|e| e.pos)
}

/// `CPile::GetGoodType` (pile vtbl+60) of pile `p`.
pub fn pile_good(world: &World, p: u16) -> Option<u8> {
    match &world.entities.by_index(p as u32)?.data {
        EntityData::Pile(d) => Some(d.good),
        _ => None,
    }
}

/// `ISettlerRole::NewDestination(settler, pos, 0)`: start = current position, destination = `pos`.
pub fn new_destination(world: &mut World, i: u16, pos: Y16X16) {
    let start = world.entities.by_index(i as u32).map_or(Y16X16(0), |e| e.pos);
    if let Some(j) = job_mut(world, i) {
        j.start = start;
        j.destination = pos;
    }
}

/// `CSettler::NewToDoList(SettlerJobList(race, list), list)`.
pub fn new_shared_list(world: &mut World, i: u16, list: u16) {
    let Some(e) = world.entities.by_index(i as u32) else { return };
    let race = e.race;
    let todo = world.rules.jobs.list(race, list).map(|_| ToDo::Shared { race, list });
    new_to_do_list(world, i, todo, list as i16);
}

/// `CSettler::AttachToBuilding(building)` (release 0x5816f0): `CBuilding::Attach(settler)` (release
/// vtbl+116 = 0x5370e0) puts the settler on the building's observer list, which sets the
/// settler's observer target 0 (`CSettler::SetObserverTarget`, release 0x581710: role vtbl+80,
/// the home) and entity flag ATTACHED.
pub fn attach_to_building(world: &mut World, i: u16, building: u16) {
    // "m_pBehavior->m_uHomeEntityId == 0" asserted.
    hooks::building_attach(world, building, i);
    if let Some(j) = job_mut(world, i) {
        j.home = building;
    }
    if building != 0
        && let Some(e) = world.entities.by_index_mut(i as u32)
    {
        e.flags |= flag::ATTACHED;
    }
}

/// The settler side of `CEcoSector::CallWorker(building, type)` (release 0x4d4c70): once the eco-sector
/// has picked the nearest offered settler and taken it out of the offer list, attach it to the
/// building and send it event 1 (`CEntityEvent(1, 0, building, 0, 0)`, COME_TO_WORK).
pub fn call_worker(world: &mut World, i: u16, building: u16) {
    attach_to_building(world, i, building);
    set_event(world, i, super::state::EntityEvent { event: 1, kind: 0, data: [building as i32, 0, 0] });
}

/// `CSettler::ChangeType(type, _, offer)` (release 0x580ce0): a settler becomes another type on its
/// owner's eco-sector (nothing happens elsewhere). A specialist that becomes a carrier drops its
/// tool. The eco-sector's counts move from the old type to the new, a thief (role 19) is set
/// free first, the warrior bits follow the type, the settler gets the new type's role
/// (`CreateSettlerRole` + `SetRole` 0x580c20, Init included) and its wait list (always: the
/// release never reads its second argument), an offer to the eco-sector when asked, and the new
/// type's health. Warriors leave `CSettlerMgr`'s per-type list when they become carriers and join
/// the new type's list (`DetachSettler` 0x582bb0 / `AttachSettler` 0x582b40, flag 0x400 turned
/// into 0x200), and it moves in the war map with its new strength. Then the AI hears (0x16).
///
/// Not ported, unreachable with the shipped data: 0x580e52..0x580e94, a squad leader (0x2c) of
/// the player in 0x12dbc3c while 0x1165440 is 0 gets `CHJBRole` (vtable 0x105b734, ctor
/// 0x57bc30) and becomes 0x1165440. 0x12dbc3c is set only by `CHJBMgr` (vtable 0x105b714): the
/// text "!hjb" (0x57b660 [inf: chat input]) → command 5051 → every machine answers 5052 with
/// whether it could load gfx file 25 (0x57b580, "Jodowein" 0x57b6a0); a 0 answer cancels
/// (0x57b780), and the install has no `Gfx/25.*`, so every answer is 0. `CHJBRole` vtbl+0xc
/// (0x57bdb0) sets the globals again for a loaded hero.
pub fn change_type(world: &mut World, i: u16, new_type: u16, _take_wait: bool, offer: bool) {
    let Some(e) = world.entities.by_index(i as u32) else { return };
    if e.flags & flag::DIED != 0 {
        return;
    }
    // "Type() != newSettlerType" and "!OFFERED" asserted.
    let (old, owner, race, pos, warrior) = (e.kind, e.owner, e.race, e.pos, e.warrior_type());
    let Some(n) = world.grid.checked_index(pos.x(), pos.y()) else { return };
    let eco = crate::eco::eco_of(world, n);
    if eco == 0 || world.economy.owner(eco) != owner {
        return;
    }
    if new_type == 1 {
        let tool = world.rules.settler_tool(race, old);
        if tool != 0 {
            crate::pile::search_space_for_goods(world, pos.x(), pos.y(), tool, 1);
        }
        if warrior != 0 {
            crate::settler_lists::detach(world, i);
        }
    }
    if warrior == 0 {
        crate::eco::change_nr_of_settler(world, eco, old, -1);
    }
    // 0x580db5 / 0x580dca: the manager's count and `DecNumberOfSettler(owner, old type)`.
    world.settler_lists.settler_removed(owner, old);
    crate::statistic::settler_removed(world, owner, old);
    // 0x580dec: a thief (role vtbl+0x48 = 19) is set free (vtbl+0x40, −1).
    if job(world, i).is_some_and(|j| j.role == super::state::role::THIEF) {
        roles::set_free(world, i, -1);
    }
    if world.warmap.contains(i) {
        crate::warmap::remove(world, i);
    }
    let new_warrior = world.rules.warrior_type(new_type) as u32;
    if let Some(e) = world.entities.by_index_mut(i as u32) {
        e.kind = new_type;
        e.flags = (e.flags & !(0xF | 0x1000_0000)) | new_warrior;
    }
    crate::warmap::readd(world, i);
    let new_role = world.rules.jobs.settler_role(race, new_type);
    // `CSettler::SetRole` (0x580c20, called at 0x580ec0) first takes the settler out of its
    // groups.
    crate::group::remove_all(world, i);
    if let Some(j) = job_mut(world, i) {
        // A fresh role: the old role's members go with it (the home is set again by the caller);
        // the CSettler / IMovingEntity / IAnimatedEntity parts stay.
        let old = std::mem::take(j);
        *j = JobState {
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
            ..JobState::default()
        };
    }
    super::roles::new_role(world, i, new_role, 0);
    take_wait_list(world, i);
    // 0x580efa / 0x580f01: the manager's count and `IncNumberOfSettler(owner, new type)`.
    world.settler_lists.settler_added(owner, new_type);
    crate::statistic::settler_added(world, owner, new_type);
    if new_warrior == 0 {
        crate::eco::change_nr_of_settler(world, eco, new_type, 1);
        if offer {
            crate::eco::set_settler_offer(world, eco, new_type, i);
        }
    } else {
        crate::settler_lists::attach(world, i);
        if let Some(e) = world.entities.by_index_mut(i as u32)
            && e.flags & 0x400 != 0
        {
            e.flags = (e.flags & !0x400) | flag::SELECTED;
        }
    }
    let health = world.rules.settler_health(race, new_type);
    if let Some(s) = settler_mut(world, i) {
        s.health = health;
    }
    // The AI notice (0x16, owner, settler, new type << 16 | old type), 0x580f86.
    crate::ai::post_event(world, 0x16, owner as i32, i as i32, (new_type as i32) << 16 | old as i32);
}
