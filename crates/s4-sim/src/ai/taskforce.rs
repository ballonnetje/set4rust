//! The task forces at run time (`CAITaskForce`, base ctor 0x4448e0; the classes of
//! [`super::model::CLASS_OF_TYPE`]). Decompiles in `tools/re/decompiled/script/ai/dtagents.c`,
//! `taskforce2.c` .. `taskforce4.c`.
//!
//! * **The timer list** (.bss 0x15aa200 count, 0x15aa204 remainder, 0x15aa208 head, 0x15aa20c
//!   cursor): every task force joins it at the head when made (0x4448e0) and leaves it when
//!   destroyed (0x445710). Each AI update (0x4384d0, before the schedulers) visits an eighth of
//!   them from the cursor (0x445610: `count / 8`, the remainders summed, one more when they pass
//!   7); a visit counts the timer (+0x34) down by 8 and at 0 runs the task force's update
//!   (vtbl+0x28). Past the end the cursor goes back to the head. So a task force is visited about
//!   every 8 ticks and a timer of `t` waits about `t` ticks.
//! * **Orders** (vtbl+0x20 `(order, target, flags)`): the target (+0x3c), the tick (+0x44), the
//!   order in bits 4..11 of +0x38, the goal and waypoints cleared, state 0, phase 1, timer 0;
//!   then per class and order the state machine's start ([`order`]).
//! * **Updates** (vtbl+0x28, [`update`]): reservoirs nothing (0x439850); squads 0x446a00, dark
//!   gardeners 0x447cd0, shamans 0x447db0, manakopters 0x448120; the shared states of
//!   `CAITaskForceEx` (0x4464a0): 0x66 plan the way (0x446260: the tile path from the first
//!   member to the goal, its first three waypoints) and send the members (0x446370), 0x65 wait
//!   until they arrived and go on to the next waypoint, then to the next state (+0x36).
//!
//! The release keeps the order in which the task forces were made (the list) and visits them in
//! that order; s4rs does the same with [`TfRef`]s.

use super::model::{TaskForce, class_of};
use super::player::{TfRef, tf, tf_mut};
use crate::World;
use crate::grid::Y16X16;

/// The task-force classes (table 0xff07a8).
pub mod class {
    pub const RESERVOIR: u8 = 1;
    pub const SQUAD: u8 = 2;
    pub const PRIESTS: u8 = 3;
    pub const DARK_GARDENERS: u8 = 6;
    pub const SHAMANS: u8 = 7;
    pub const MANAKOPTERS: u8 = 8;
}

/// The task-force types the Dark Tribe makes: squads (2), dark gardeners (0xb), shamans (0xc),
/// manakopters (0xd).
pub mod kind {
    pub const SQUAD: i32 = 2;
    pub const DARK_GARDENERS: i32 = 0xb;
    pub const SHAMANS: i32 = 0xc;
    pub const MANAKOPTERS: i32 = 0xd;
}

/// Flag bits of +0x38.
pub mod flag {
    /// The order's target is an entity whose unique id is noted (+0x40, 0x444cf0).
    pub const ENTITY: u32 = 0x1000;
    /// Walk to the goal (+0x50) through the tile path.
    pub const WALK: u32 = 0x2000;
    /// The members stopped once on this waypoint (0x4464a0 state 0x65).
    pub const STOPPED: u32 = 0x10000;
    /// Re-target soon (the agents clear it).
    pub const RETARGET: u32 = 0x400_0000;
    /// The followed task force's waypoint changed (0x445ff0).
    pub const FOLLOW_MOVED: u32 = 0x800_0000;
    /// Reserved by the dark attack (0x42c9f0 / 0x42ca40).
    pub const ATTACK: u32 = 0x2000_0000;
}

/// The class of the task force `r` points at.
pub fn class_of_ref(world: &World, r: TfRef) -> u8 {
    match r {
        TfRef::Reservoir(..) | TfRef::Slot(..) | TfRef::Force(..) => class::RESERVOIR,
        TfRef::Squad(_) => class::SQUAD,
        TfRef::Group(p, s, uid) => group_item(world, p, s, uid).map_or(0, |i| class_of(i.kind)),
    }
}

/// The type (0..0xd) of a group task force (1 for the reservoirs, 4 for the player's squad).
pub fn kind_of_ref(world: &World, r: TfRef) -> i32 {
    match r {
        TfRef::Reservoir(..) | TfRef::Slot(..) | TfRef::Force(..) => 1,
        TfRef::Squad(_) => 4,
        TfRef::Group(p, s, uid) => group_item(world, p, s, uid).map_or(0, |i| i.kind),
    }
}

fn group_item(world: &World, p: u8, s: u8, uid: u32) -> Option<&super::model::GroupItem> {
    world.ai.players.get(p as usize)?.as_ref()?.body.sectors.get(s as usize)?.group.get(uid)
}

/// The task force linked with a group task force (+0x2c).
pub fn linked(world: &World, r: TfRef) -> Option<TfRef> {
    let TfRef::Group(p, s, uid) = r else { return None };
    let l = group_item(world, p, s, uid)?.link;
    (l != 0).then_some(TfRef::Group(p, s, l))
}

/// The members (entity ids) in list order.
pub fn members(world: &World, r: TfRef) -> Vec<u16> {
    tf(world, r).map_or(Vec::new(), |t| t.members.clone())
}

/// +0xc: the member count.
pub fn count(world: &World, r: TfRef) -> i32 {
    tf(world, r).map_or(0, |t| t.members.len() as i32)
}

/// The position of the first member (`*(Entity[*(+0x10)] + 0x18)`).
pub fn first_pos(world: &World, r: TfRef) -> Option<Y16X16> {
    let id = *tf(world, r)?.members.first()?;
    world.entities.by_index(id as u32).map(|e| e.pos)
}

/// The group's list of `class` (`sector + 0xc0 + 4·class`) from its head.
pub fn list(world: &World, player: u8, s: u8, class: u8) -> Vec<TfRef> {
    world.ai.players.get(player as usize).and_then(|a| a.as_ref()).and_then(|a| a.body.sectors.get(s as usize)).map_or(Vec::new(), |sec| {
        sec.group.list(class).into_iter().map(|u| TfRef::Group(player, s, u)).collect()
    })
}

/// The squad-like classes count their members by warrior type 2..5 (0x446770 / 0x4467e0); the
/// count of `wt` (vtbl+0x10, 0x446720), −1 for other types or classes.
pub fn warriors(world: &World, r: TfRef, wt: u8) -> i32 {
    if class_of_ref(world, r) != class::SQUAD || !(2..=5).contains(&wt) {
        return -1;
    }
    members(world, r).iter().filter(|&&m| world.entities.by_index(m as u32).is_some_and(|e| e.flags & 0xf == wt as u32)).count() as i32
}

// ---- the timer list ----------------------------------------------------------------------

/// The ctor 0x4448e0: a new task force joins the timer list at the head.
pub fn register(ai: &mut super::Ai, r: TfRef) {
    ai.timers.insert(0, r);
}

/// 0x445710: the task force leaves the timer list; a cursor on it moves to the next one (or the
/// head).
pub fn unregister(ai: &mut super::Ai, r: TfRef) {
    let Some(k) = ai.timers.iter().position(|&t| t == r) else { return };
    let next = ai.timers.get(k + 1).copied();
    ai.timers.remove(k);
    if ai.timer_cursor == Some(r) {
        ai.timer_cursor = next;
    }
    if ai.timer_cursor.is_none() {
        ai.timer_cursor = ai.timers.first().copied();
    }
}

/// The task forces a new player AI makes (ctor 0x439b20): its 5 reservoirs, then the squad.
pub fn register_player(ai: &mut super::Ai, player: u8) {
    for k in 0..5 {
        register(ai, TfRef::Reservoir(player, k));
    }
    register(ai, TfRef::Squad(player));
}

/// The task forces a new sector AI makes: the normal one (0x442180) its 7 reservoirs, then the 15
/// slots; the Dark Tribe's (0x440600) the reservoirs of slots 3, 7, 4, 9, 2.
pub fn register_sector(ai: &mut super::Ai, player: u8, s: u8, dark: bool) {
    if dark {
        for k in [3, 7, 4, 9, 2] {
            register(ai, TfRef::Slot(player, s, k));
        }
    } else {
        for k in 0..7 {
            register(ai, TfRef::Force(player, s, k));
        }
        for k in 0..15 {
            register(ai, TfRef::Slot(player, s, k));
        }
    }
}

/// The timer list as a load makes it: per player AI (in player order) its ctor's task forces,
/// then per sector AI its ctor's and its group's in load order. TODO(port): the eco-sector AIs'
/// task forces (0x4439f0) are not made.
pub fn rebuild_timers(world: &mut World) {
    let mut timers = Vec::new();
    for p in 1..super::PLAYERS as u8 {
        let Some(a) = world.ai.players[p as usize].as_ref() else { continue };
        let mut made: Vec<TfRef> = (0..5).map(|k| TfRef::Reservoir(p, k)).collect();
        made.push(TfRef::Squad(p));
        for (s, sec) in a.body.sectors.iter().enumerate() {
            let s = s as u8;
            match sec.kind {
                super::model::SectorKind::Dark { .. } => made.extend([3, 7, 4, 9, 2].map(|k| TfRef::Slot(p, s, k))),
                super::model::SectorKind::Normal { .. } => {
                    made.extend((0..7).map(|k| TfRef::Force(p, s, k)));
                    made.extend((0..15).map(|k| TfRef::Slot(p, s, k)));
                }
            }
            made.extend(sec.group.items.iter().map(|i| TfRef::Group(p, s, i.uid)));
        }
        for r in made {
            timers.insert(0, r);
        }
    }
    world.ai.timers = timers;
    world.ai.timer_cursor = None;
    world.ai.timer_acc = 0;
}

/// 0x445610: one eighth of the task forces (with the remainders carried) from the cursor.
pub fn run_timers(world: &mut World) {
    let n = world.ai.timers.len() as i32;
    let mut k = n >> 3;
    world.ai.timer_acc += n & 7;
    if world.ai.timer_acc > 7 {
        k += 1;
        world.ai.timer_acc -= 8;
    }
    if k <= 0 || world.ai.timers.is_empty() {
        return;
    }
    let mut cur = world.ai.timer_cursor.filter(|c| world.ai.timers.contains(c)).unwrap_or(world.ai.timers[0]);
    let mut done = 0;
    loop {
        let next = world.ai.timers.iter().position(|&t| t == cur).and_then(|i| world.ai.timers.get(i + 1)).copied();
        visit(world, cur);
        // [deviation] a visit that destroyed the next task force: the release would read freed
        // memory; s4rs goes back to the head.
        let Some(next) = next.filter(|n| world.ai.timers.contains(n)) else {
            world.ai.timer_cursor = world.ai.timers.first().copied();
            return;
        };
        done += 1;
        cur = next;
        if done >= k {
            world.ai.timer_cursor = Some(next);
            return;
        }
    }
}

fn visit(world: &mut World, r: TfRef) {
    let Some(t) = tf_mut(world, r) else { return };
    let v = t.timer();
    let v = if v < 9 { 0 } else { v - 8 };
    t.set_timer(v);
    if v == 0 {
        update(world, r);
    }
}

// ---- making and destroying -----------------------------------------------------------------

/// The factory 0x445470 and the head insertion into sector AI `s`'s group (0x445020): a new task
/// force of `kind`, registered with the timer list.
pub fn create(world: &mut World, player: u8, s: u8, kind: i32) -> Option<TfRef> {
    let sec = world.ai.players.get_mut(player as usize)?.as_mut()?.body.sectors.get_mut(s as usize)?;
    let uid = sec.group.create(player, kind)?;
    let r = TfRef::Group(player, s, uid);
    register(&mut world.ai, r);
    Some(r)
}

/// vtbl+0xc / the dtor (0x4449e0): the link goes, the task force leaves its group, its members
/// lose their task force (0x444c40) and it leaves the timer list.
pub fn destroy(world: &mut World, r: TfRef) {
    let TfRef::Group(p, s, uid) = r else { return };
    let Some(sec) = world.ai.players.get_mut(p as usize).and_then(|a| a.as_mut()).and_then(|a| a.body.sectors.get_mut(s as usize)) else { return };
    let Some(it) = sec.group.remove(uid) else { return };
    for m in it.tf.members {
        if world.ai.infos.get(&m) == Some(&r) {
            world.ai.infos.remove(&m);
        }
    }
    unregister(&mut world.ai, r);
}

/// Adds entity `id` (vtbl+0x14 mode 0): see [`super::player::add_to`].
pub fn add(world: &mut World, r: TfRef, id: u16) {
    super::player::add_to(world, id, r);
}

/// vtbl+0x18 (0x444ba0): the member leaves.
pub fn remove(world: &mut World, r: TfRef, id: u16) {
    if world.ai.infos.get(&id) == Some(&r) {
        world.ai.infos.remove(&id);
        if let Some(t) = tf_mut(world, r) {
            t.remove_member(id);
        }
    }
}

// ---- orders ------------------------------------------------------------------------------

/// vtbl+0x20 `(order, target, flags)` by class: squads 0x446880, dark gardeners 0x447d10,
/// shamans 0x447fd0, manakopters 0x448210; reservoirs refuse (0x4397d0). False: the class does
/// not know the order (the common part has run all the same).
pub fn order(world: &mut World, r: TfRef, cmd: u32, target: i32, flags: u32) -> bool {
    let class = class_of_ref(world, r);
    if !matches!(class, class::SQUAD | class::PRIESTS | class::DARK_GARDENERS | class::SHAMANS | class::MANAKOPTERS) {
        return false;
    }
    let now = world.ai.tick;
    let Some(t) = tf_mut(world, r) else { return false };
    // The common part.
    t.ints[1] = target;
    t.ints[2] = 0;
    t.ints[3] = now as i32;
    t.set_flags(t.flags() & 0xf3ff_000f | cmd << 4 | flags);
    t.set_goal(-1);
    t.bytes[1] = 0;
    t.bytes[2] = 0;
    t.set_phase(1, now);
    t.set_timer(0);
    match (class, cmd) {
        (_, 0) => {
            t.next_cast = 0;
            true
        }
        // The priests (0x446e50): +0x60 cleared; only order 6 besides 0.
        (class::PRIESTS, 6) => {
            t.next_cast = 0;
            t.set_states(7, 0x6a, 0, now);
            true
        }
        (class::PRIESTS, _) => {
            t.next_cast = 0;
            false
        }
        // Move to the target.
        (class::SQUAD | class::SHAMANS, 1) | (class::SHAMANS, 7) => {
            t.set_flags(t.flags() | flag::WALK);
            t.set_goal(target);
            t.set_states(2, 0x66, 0, now);
            true
        }
        (class::MANAKOPTERS, 1) => {
            t.set_flags(t.flags() | flag::WALK);
            t.set_goal(target);
            t.set_states(8, 0x6d, 0, now);
            true
        }
        // A dark gardener's work place: walk there, then work (state 2).
        (class::DARK_GARDENERS, 5) => {
            t.set_flags(t.flags() | flag::WALK);
            t.set_goal(target);
            t.set_states(2, 0x66, 2, now);
            true
        }
        // Attack a building: its ensign.
        (class::SQUAD, 2) => {
            note_entity(world, r);
            if let Some(t) = tf_mut(world, r) {
                t.set_states(2, 1, 0, now);
            }
            true
        }
        (class::SQUAD, 4) => {
            note_entity(world, r);
            if let Some(t) = tf_mut(world, r) {
                t.set_states(1, 3, 0, now);
            }
            true
        }
        // Move to the edge of the enemy land on the way to the target.
        (class::SQUAD | class::SHAMANS, 3) => {
            t.set_flags(t.flags() | flag::WALK);
            let ok = stage_point(world, r);
            if ok && let Some(t) = tf_mut(world, r) {
                t.set_states(2, 0x66, if class == class::SQUAD { 6 } else { 0 }, now);
            }
            ok
        }
        // Follow the linked task force.
        (class::SQUAD, 6) => {
            t.set_states(7, 0x6a, 0, now);
            true
        }
        (class::SHAMANS, 9 | 0xe) => {
            t.set_flags(t.flags() | flag::WALK);
            t.set_states(6, 0x70, 0, now);
            true
        }
        // Board the manakopter `target`.
        (class::SQUAD, 10) => {
            t.set_states(2, 0x6b, 0, now);
            true
        }
        (class::SHAMANS, 10) => {
            t.set_states(9, 0x6b, 0, now);
            true
        }
        (class::MANAKOPTERS, 0xb) => {
            t.set_states(1, 0x6e, 0, now);
            true
        }
        (class::MANAKOPTERS, 0xc) => {
            t.set_states(10, 0x6e, 0, now);
            true
        }
        _ => false,
    }
}

/// 0x444cf0: flag 0x1000 and the target entity's unique id (+0x40; 0 without one).
fn note_entity(world: &mut World, r: TfRef) {
    let target = tf(world, r).map_or(0, |t| t.target());
    let uid = if target > 0 { world.entities.by_index(target as u32).map_or(0, |e| e.uid) } else { 0 };
    if let Some(t) = tf_mut(world, r) {
        t.set_flags(t.flags() | flag::ENTITY);
        t.ints[2] = uid as i32;
    }
}

// ---- updates -----------------------------------------------------------------------------

/// vtbl+0x28.
pub fn update(world: &mut World, r: TfRef) {
    match class_of_ref(world, r) {
        class::SQUAD => update_squad(world, r),
        class::PRIESTS => {
            super::priests::update(world, r);
            update_ex(world, r);
        }
        class::DARK_GARDENERS => update_gardeners(world, r),
        class::SHAMANS => update_shamans(world, r),
        class::MANAKOPTERS => update_manakopters(world, r),
        _ => {}
    }
}

/// `CAITaskForceEx` update (0x4464a0), the states the classes share.
fn update_ex(world: &mut World, r: TfRef) {
    let now = world.ai.tick;
    let class = class_of_ref(world, r);
    let Some(t) = tf_mut(world, r) else { return };
    t.set_timer(8);
    match t.state() {
        0 => t.set_phase(1, now),
        0x65 => {
            let (min, need) = if t.flags() & flag::STOPPED == 0 { (3, 0) } else { (0x80, t.members.len() as i32 / 2) };
            let ms = t.members.clone();
            let mut moving = 0;
            for m in ms {
                if walk_progress(world, m) >= min {
                    moving += 1;
                    if moving >= need {
                        return;
                    }
                }
            }
            let Some(t) = tf_mut(world, r) else { return };
            t.set_flags(t.flags() | flag::STOPPED);
            let w = t.waypoints();
            if w[0] == t.goal() {
                t.set_waypoints([-1; 3]);
                t.set_state(t.next_state());
                return;
            }
            t.set_waypoints([w[1], w[2], -1]);
            if next_on_sector(world, r) || plan_way(world, r) {
                send_members(world, r, class != class::DARK_GARDENERS);
                if let Some(t) = tf_mut(world, r) {
                    t.set_timer(0x20);
                }
                return;
            }
            if let Some(t) = tf_mut(world, r) {
                t.set_state(t.next_state());
            }
        }
        100 => t.set_state(t.next_state()),
        0x66 => {
            plan_way(world, r);
            send_members(world, r, class != class::DARK_GARDENERS);
            if let Some(t) = tf_mut(world, r) {
                t.set_flags(t.flags() & !flag::STOPPED);
                t.set_timer(0x20);
                t.set_state(0x65);
            }
        }
        0x68 | 0x69 => {
            let target = t.target();
            let ready = world.entities.by_index(target as u32).is_some_and(|e| e.flags & 0x0200_0000 != 0);
            if !ready && let Some(t) = tf_mut(world, r) {
                t.set_state(0);
            }
        }
        0x6a => {
            if !follow(world, r)
                && let Some(t) = tf_mut(world, r)
            {
                t.set_state(0);
            }
        }
        _ => {}
    }
}

/// `IMovingEntity +0x4c` vtbl+0x34 (`CWalkingBase` 0x5b9ea0): how far the walk is — the state + 1
/// for states 0..5; on a fine step (6) the distance to the fine waypoint (+1, at most 15) << 4
/// when heading for the last coarse waypoint, else the distance to the coarse one << 8; 0x1000 /
/// 0x2000 / 0x4000 while searching (7 / 8 / 9). 0 for an entity without a walker.
pub fn walk_progress(world: &World, id: u16) -> i32 {
    let w = crate::jobs::core::job(world, id).and_then(|j| j.walking.as_deref()).or_else(|| {
        let e = world.entities.by_index(id as u32)?;
        if matches!(e.obj_type, crate::entity::ObjType::Ship | crate::entity::ObjType::LandVehicle) { crate::vehicles::get(world, id).and_then(|v| v.walking.as_deref()) } else { None }
    });
    let Some(w) = w else { return 0 };
    let d = |a: i32, b: i32| (crate::pathing::distance_fast(Y16X16(a as u32), Y16X16(b as u32)) + 1).min(15);
    match w.state {
        0..=5 => w.state + 1,
        6 => {
            if w.walk_to2 == w.coarse {
                d(w.walk_to, w.fine) << 4
            } else {
                d(w.walk_to, w.coarse) << 8
            }
        }
        7 => 0x1000,
        8 => 0x2000,
        9 => 0x4000,
        _ => 0,
    }
}

/// The walking sector of a packed position (tile +4), 0 outside the map.
fn sector(world: &World, p: i32) -> u16 {
    let v = Y16X16(p as u32);
    if p < 0 {
        return 0;
    }
    world.grid.checked_index(v.x(), v.y()).map_or(0, |n| world.tiling.sector_of(n))
}

/// 0x446140: the first waypoint lies on the first member's sector.
fn next_on_sector(world: &World, r: TfRef) -> bool {
    let Some(t) = tf(world, r) else { return false };
    let w = t.waypoints()[0];
    if w < 1 {
        return false;
    }
    let s = sector(world, w);
    s != 0 && first_pos(world, r).is_some_and(|p| sector(world, p.0 as i32) == s)
}

/// 0x4461c0: with [`flag::WALK`] the goal (+0x50, else the target) must be a node without flag 1;
/// a blocked one is replaced by the nearest free node of the target within 16 (0x436020), else
/// it fails (the goal becomes the target).
fn check_goal(world: &mut World, r: TfRef) -> bool {
    let Some(t) = tf_mut(world, r) else { return false };
    if t.flags() & flag::WALK == 0 {
        return true;
    }
    if t.goal() < 0 {
        let v = t.target();
        if let Some(s) = t.squad.as_mut() {
            s.0 = v;
        }
    }
    let g = t.goal();
    if g < 0 {
        return false;
    }
    let target = t.target();
    let v = Y16X16(g as u32);
    let blocked = world.grid.checked_index(v.x(), v.y()).is_none_or(|n| world.layers.flags[n] & 1 != 0);
    if !blocked {
        return true;
    }
    let tv = Y16X16(target as u32);
    let found = super::player::free_node_dist(world, tv.x(), tv.y());
    let Some(t) = tf_mut(world, r) else { return false };
    match found {
        Some((d, (x, y))) if d < 0x10 => {
            if let Some(s) = t.squad.as_mut() {
                s.0 = Y16X16::pack(x, y).0 as i32;
            }
            true
        }
        _ => {
            if let Some(s) = t.squad.as_mut() {
                s.0 = target;
            }
            false
        }
    }
}

/// 0x446260: the waypoints cleared; with a sound goal ([`check_goal`]) and members, the tile path
/// (0x5ac2f0, flags player | 0x80) from the first member to the goal gives the first three
/// waypoints from the member's side.
fn plan_way(world: &mut World, r: TfRef) -> bool {
    if let Some(t) = tf_mut(world, r) {
        t.set_waypoints([-1; 3]);
    }
    if !check_goal(world, r) {
        return false;
    }
    let Some(t) = tf(world, r) else { return false };
    let Some(from) = first_pos(world, r) else { return false };
    let goal = Y16X16(t.goal() as u32);
    let flags = (t.flags() & 0xf) | 0x80;
    let mut wp = crate::pathing::Waypoints::default();
    let side = world.grid.size() as usize / 16;
    if crate::pathing::astar_tiling(&world.tiling, &world.alliances, &world.warmap, side, from, goal, flags, &mut wp) == 0 {
        return false;
    }
    let mut out = [-1; 3];
    for o in out.iter_mut() {
        if wp.count == 0 {
            break;
        }
        *o = wp.back().0 as i32;
        wp.pop_back();
    }
    if let Some(t) = tf_mut(world, r) {
        t.set_waypoints(out);
    }
    true
}

/// .rdata 0x101bdb8: the offsets (doubled) the members are spread over around a waypoint
/// (entries 0..=0x25).
const SPREAD: [(i8, i8); 38] = [
    (0, 0), (1, 0), (1, 1), (0, 1), (-1, 0), (-1, -1), (0, -1), (2, 0), (2, 1), (2, 2), (1, 2), (0, 2), (-1, 1),
    (-2, 0), (-2, -1), (-2, -2), (-1, -2), (0, -2), (1, -1), (3, 0), (3, 1), (3, 2), (3, 3), (2, 3), (1, 3), (0, 3),
    (-1, 2), (-2, 1), (-3, 0), (-3, -1), (-3, -2), (-3, -3), (-2, -3), (-1, -3), (0, -3), (1, -2), (2, -1), (4, 0),
];

/// 0x446370 `(walk)`: each member gets a move order (event 0x11 kind 0xd, data0 0 with `walk`,
/// else 4) to its own spot around the first waypoint: the next entry of [`SPREAD`] (doubled)
/// that lies on the map on the waypoint's sector.
fn send_members(world: &mut World, r: TfRef, walk: bool) {
    let Some(t) = tf(world, r) else { return };
    let w = t.waypoints()[0];
    if w < 1 || t.members.is_empty() {
        return;
    }
    let ms = t.members.clone();
    let (wx, wy) = (w & 0xffff, w >> 16);
    let s = sector(world, w);
    let size = world.grid.size();
    let mut k = 0usize;
    for m in ms {
        let (x, y) = loop {
            let (x, y) = (wx + SPREAD[k].0 as i32 * 2, wy + SPREAD[k].1 as i32 * 2);
            if (x as u32) < size && (y as u32) < size && world.tiling.sector_of((y as u32 * size + x as u32) as usize) == s {
                break (x, y);
            }
            k = if k < 0x25 { k + 1 } else { 0 };
        };
        move_entity(world, m, x, y, walk);
        k = if k < 0x25 { k + 1 } else { 0 };
    }
}

/// 0x435540 `(entity, x, y, walk)`: event 0x11 kind 0xd to `(x, y)` (data0 0 with `walk`, else 4)
/// for an `IMovingEntity`.
pub fn move_entity(world: &mut World, id: u16, x: i32, y: i32, walk: bool) {
    let ev = crate::jobs::state::EntityEvent { event: 0x11, kind: 0xd, data: [if walk { 0 } else { 4 }, (y << 16) + x, 0] };
    send_event(world, id, ev);
}

/// `IEntity` vtbl+0x50 (`SetEvent`) for the `IMovingEntity`s the AI orders: settlers, ships and
/// land vehicles.
pub fn send_event(world: &mut World, id: u16, ev: crate::jobs::state::EntityEvent) {
    use crate::entity::ObjType;
    match world.entities.by_index(id as u32).map(|e| e.obj_type) {
        Some(ObjType::Settler) => crate::jobs::core::set_event(world, id, ev),
        Some(ObjType::Ship | ObjType::LandVehicle) => crate::vehicles::logic::set_event(world, id, ev),
        _ => {}
    }
}

/// 0x445ff0: following the linked task force. Both first members on the same sector: flag
/// 0x8000000 clears the goal; a goal on that sector within 7 of the linked one's first member
/// (or within 13 without foreign strength at that member, 0x431380) is kept; otherwise the goal
/// and first waypoint become that member's position and the members walk there. False
/// (goal cleared) without a link, members or a common sector.
fn follow(world: &mut World, r: TfRef) -> bool {
    let fail = |world: &mut World| {
        if let Some(t) = tf_mut(world, r) {
            t.set_goal(-1);
        }
        false
    };
    let Some(l) = linked(world, r) else { return fail(world) };
    let (Some(lp), Some(mp)) = (first_pos(world, l), first_pos(world, r)) else { return fail(world) };
    let s = sector(world, mp.0 as i32);
    if sector(world, lp.0 as i32) != s {
        return fail(world);
    }
    if let Some(t) = tf_mut(world, r)
        && t.flags() & flag::FOLLOW_MOVED != 0
    {
        t.set_flags(t.flags() & !flag::FOLLOW_MOVED);
        t.set_goal(-1);
    }
    let goal = tf(world, r).map_or(-1, |t| t.goal());
    if goal > 0 && sector(world, goal) == s {
        let d = crate::pathing::distance_fast(lp, Y16X16(goal as u32));
        if d < 7 {
            return true;
        }
        let player = tf(world, r).map_or(0, |t| (t.flags() & 0xf) as u8);
        if d < 0xd && crate::warmap::foreign_strength(world, lp.x(), lp.y(), player) == 0 {
            return true;
        }
    }
    if let Some(t) = tf_mut(world, r) {
        t.set_goal(lp.0 as i32);
        let g = t.goal();
        let mut w = t.waypoints();
        w[0] = g;
        t.set_waypoints(w);
    }
    send_members(world, r, true);
    true
}

/// 0x445e50 (order 3): the goal cleared; the tile path (flags player | 0xa0) from the target to
/// the first member, walked from the target's side: past the first waypoint on land that is not
/// an enemy's, the first after another 45 of path (or the last) becomes the goal. False without
/// members, a path or any length.
fn stage_point(world: &mut World, r: TfRef) -> bool {
    if let Some(t) = tf_mut(world, r) {
        t.set_goal(-1);
    }
    let Some(t) = tf(world, r) else { return false };
    let Some(to) = first_pos(world, r) else { return false };
    let player = (t.flags() & 0xf) as u8;
    let mut prev = t.target();
    let from = Y16X16(prev as u32);
    let mut wp = crate::pathing::Waypoints::default();
    let side = world.grid.size() as usize / 16;
    if crate::pathing::astar_tiling(&world.tiling, &world.alliances, &world.warmap, side, from, to, player as u32 | 0xa0, &mut wp) == 0 || wp.count == 0 {
        return false;
    }
    let mut total = 0;
    let mut seen = false;
    let mut cur;
    loop {
        cur = wp.back().0 as i32;
        if wp.count > 0 {
            wp.pop_back();
        }
        total += crate::pathing::distance_fast(Y16X16(prev as u32), Y16X16(cur as u32));
        prev = cur;
        let v = Y16X16(cur as u32);
        let owner = world.grid.checked_index(v.x(), v.y()).map_or(0, |n| world.tiling.owner_of(n));
        if !world.alliances.hostile(player, owner) {
            if seen {
                if total > 0x2d {
                    break;
                }
            } else {
                seen = true;
            }
        }
        if wp.count == 0 {
            break;
        }
    }
    if total <= 0 {
        return false;
    }
    if let Some(t) = tf_mut(world, r) {
        t.set_goal(cur);
    }
    true
}

/// Event 0x1c `(manakopter)` to every member (an `IAnimatedEntity`): board it.
fn board(world: &mut World, r: TfRef) {
    let Some(t) = tf(world, r) else { return };
    let target = t.target();
    for m in t.members.clone() {
        send_event(world, m, crate::jobs::state::EntityEvent { event: 0x1c, kind: 0, data: [target, 0, 0] });
    }
}

/// Whether every member has flag 0x8000 (on board).
fn all_aboard(world: &World, r: TfRef) -> bool {
    members(world, r).iter().all(|&m| world.entities.by_index(m as u32).is_some_and(|e| e.flags & 0x8000 != 0))
}

/// 0x435320: an entity that is ready (flags 0x3000000) and of warrior type 0xc (a building).
fn building_ok(world: &World, id: i32) -> bool {
    id > 0 && world.entities.by_index(id as u32).is_some_and(|e| e.flags & 0x0300_0000 != 0 && e.flags & 0xf == 0xc)
}

/// 0x435980: a building's ensign (+0x48), −1 otherwise.
fn ensign_of(world: &World, id: i32) -> i32 {
    match world.entities.by_index(id.max(0) as u32).map(|e| &e.data) {
        Some(crate::entity::EntityData::Building(b)) if id > 0 => b.ensign.0 as i32,
        _ => -1,
    }
}

/// `CAITaskForceSquad` update (0x446a00), timer 1 first:
/// * 1 / 3: with the target building standing its ensign is the goal (state 0x66, then 2 / 4)
///   and the shared update runs at once; else idle (phase 1);
/// * 2: phase 4, back to 1 in 40 ticks; 4: phase 3 (idle with the building gone), then 5 in 20;
///   6: phase 5 in 20;
/// * 0x6b: every member boards the target (event 0x1c), state 0x6c, phase 9; 0x6c: all aboard
///   gives phase 8;
/// * otherwise the shared states.
fn update_squad(world: &mut World, r: TfRef) {
    let now = world.ai.tick;
    let Some(t) = tf_mut(world, r) else { return };
    t.set_timer(1);
    let state = t.state();
    let target = t.target();
    match state {
        1 | 3 => {
            if !building_ok(world, target) {
                if let Some(t) = tf_mut(world, r) {
                    t.set_states(1, 0, 0, now);
                }
                return;
            }
            let e = ensign_of(world, target);
            if let Some(t) = tf_mut(world, r) {
                t.set_goal(e);
                t.set_state(0x66);
                t.bytes[2] = if state == 1 { 2 } else { 4 };
            }
            update_ex(world, r);
        }
        2 => {
            t.set_phase(4, now);
            t.set_state(1);
            t.bytes[2] = 0;
            t.set_timer(0x28);
        }
        4 | 6 => {
            if state == 4 {
                if building_ok(world, target) {
                    if let Some(t) = tf_mut(world, r) {
                        t.set_phase(3, now);
                        t.set_timer(0x14);
                    }
                } else if let Some(t) = tf_mut(world, r) {
                    t.set_states(1, 0, 0, now);
                }
            }
            if let Some(t) = tf_mut(world, r) {
                t.set_phase(5, now);
                t.set_timer(0x14);
            }
        }
        0x6b => {
            if t.members.is_empty() || target <= 0 {
                return;
            }
            board(world, r);
            if let Some(t) = tf_mut(world, r) {
                t.set_states(9, 0x6c, 0, now);
            }
        }
        0x6c => {
            if target > 0
                && !members(world, r).is_empty()
                && all_aboard(world, r)
                && let Some(t) = tf_mut(world, r)
            {
                t.set_phase(8, now);
            }
        }
        _ => update_ex(world, r),
    }
}

/// `CAITaskForceDarkGardeners` update (0x447cd0): in state 2 (at the work place) once: the way
/// planned anew and the members sent to work there (event data0 0), phase 6, timer 250;
/// otherwise the shared states.
fn update_gardeners(world: &mut World, r: TfRef) {
    let now = world.ai.tick;
    let Some(t) = tf(world, r) else { return };
    if t.state() != 2 {
        update_ex(world, r);
        return;
    }
    if t.phase() != 6 {
        plan_way(world, r);
        send_members(world, r, true);
        if let Some(t) = tf_mut(world, r) {
            t.set_phase(6, now);
            t.set_timer(0xfa);
        }
    }
}

/// `CAITaskForceShamans` update (0x447db0), timer 8:
/// * 0x6b / 0x6c: boarding as the squad's (phase 9 → 8);
/// * 0x70: every member gets the cast order (event 3 at the target, 0x435770; shamans only) —
///   then 0x6f; 0x6f: all members shamans without mana (role +0x68 < 1) gives phase 10;
/// * otherwise the shared states.
fn update_shamans(world: &mut World, r: TfRef) {
    let now = world.ai.tick;
    let Some(t) = tf_mut(world, r) else { return };
    t.set_timer(8);
    let target = t.target();
    match t.state() {
        0x6b => {
            if t.members.is_empty() || target <= 0 {
                return;
            }
            board(world, r);
            if let Some(t) = tf_mut(world, r) {
                t.set_states(9, 0x6c, 0, now);
            }
        }
        0x6c => {
            if target > 0
                && !members(world, r).is_empty()
                && all_aboard(world, r)
                && let Some(t) = tf_mut(world, r)
            {
                t.set_phase(8, now);
            }
        }
        0x6f => {
            let ms = members(world, r);
            if ms.is_empty() {
                return;
            }
            let spent = ms.iter().all(|&m| {
                world.entities.by_index(m as u32).is_some_and(|e| e.obj_type == crate::entity::ObjType::Settler && e.kind == 0x36)
                    && crate::jobs::core::job(world, m).is_some_and(|j| j.soldier.shaman_mana < 1)
            });
            if spent && let Some(t) = tf_mut(world, r) {
                t.set_phase(10, now);
            }
        }
        0x70 => {
            let ms = members(world, r);
            if ms.is_empty() {
                return;
            }
            for m in ms {
                // 0x435770: shamans only; the first other member ends the round.
                if !world.entities.by_index(m as u32).is_some_and(|e| e.obj_type == crate::entity::ObjType::Settler && e.kind == 0x36) {
                    return;
                }
                send_event(world, m, crate::jobs::state::EntityEvent { event: 3, kind: 0, data: [target, 0, 0] });
            }
            if let Some(t) = tf_mut(world, r) {
                t.set_state(0x6f);
            }
        }
        _ => update_ex(world, r),
    }
}

/// `CAITaskForceManakopters` update (0x448120), timer 8: state 0x6d sends every member that
/// is a flying object to the goal (+0x50; [`crate::flying::fly_to`]) and turns to 0x67; 0x67 (in
/// flight) waits until the first member is gone, no flying object or no longer ordered (state
/// bit 1, cleared when it starts to land), then state 0 and phase 1; 0x6e unloads every member
/// (0x56b990, [`crate::flying::unload`]) and turns to state 0.
fn update_manakopters(world: &mut World, r: TfRef) {
    let now = world.ai.tick;
    let Some(t) = tf_mut(world, r) else { return };
    t.set_timer(8);
    let (state, goal) = (t.state(), t.goal());
    let ms = members(world, r);
    match state {
        0x67 => {
            let Some(&first) = ms.first() else { return };
            let flying = crate::flying::get(world, first).is_some_and(|f| f.state & crate::flying::state::ORDERED != 0);
            if !flying && let Some(t) = tf_mut(world, r) {
                t.set_state(0);
                t.set_phase(1, now);
            }
        }
        0x6d => {
            if ms.is_empty() {
                return;
            }
            let at = crate::grid::Y16X16(goal as u32);
            for m in ms {
                if crate::flying::get(world, m).is_some() {
                    crate::flying::fly_to(world, m, at.x(), at.y());
                }
            }
            if let Some(t) = tf_mut(world, r) {
                t.set_state(0x67);
            }
        }
        0x6e => {
            if ms.is_empty() {
                return;
            }
            for m in ms {
                if crate::flying::get(world, m).is_some() {
                    crate::flying::unload(world, m);
                }
            }
            if let Some(t) = tf_mut(world, r) {
                t.set_state(0);
            }
        }
        _ => {}
    }
}

/// For tests: the task force as a whole.
pub fn get(world: &World, r: TfRef) -> Option<&TaskForce> {
    tf(world, r)
}
