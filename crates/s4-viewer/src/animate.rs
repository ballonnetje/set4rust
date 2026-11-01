//! Settlers between logic ticks: what the original does at draw time.
//!
//! `CSettler::GetGfxInfo` (release 0x5810a0) first calls the role's `Update` (0x586a00, ported as
//! `s4_sim::jobs::core::update`). That `Update` advances the frame and `IMovingEntity`'s walk
//! distance (+0x41) by the ticks since the entity's last update. It then reports job `+0x26`,
//! frame `+0x24`, direction `+0x40` and the distance. The object renderer (0x660070, gfx-info
//! types 2/4) draws a settler at
//! `cur − (cur − prev) · distance / 256`, where `prev` is the node it came from
//! (`cur − NEIGHBOR[dir]`, both with their heights). So a walking settler glides from the
//! previous node (distance 0xFF right after `WalkDir`) to its node (0) over the walk speed's ticks.
//! A settler on a virtual task (flag 0x800: inside a building, or a free worker beside its
//! target) reports direction 0xFF and its role's float offset instead, in 1/256 of a node: the
//! renderer adds `(off_x · column >> 8, off_y · row >> 8)` px (0x660b24; column 24 and row 12
//! px at zoom 1, `cdq`/`sar` division truncating towards zero).
//! A house worker inside its workup or production building is not drawn by `GetGfxInfo` (it has
//! MAGIC_INVISIBLE) but by the building's `FillGfxInfo` (0x55a3a0 / 0x54d0e0), which runs the same
//! `Update` and hands the role floats to `BlitBuilding` as a **pixel** offset from the building's
//! anchor (0x6626dc: `(dx << 16) · zoom`); its pose is that offset, unscaled.
//!
//! Our simulation runs `Update` only where a role's logic does (`LogicUpdateJob` of house and
//! free workers), so the snapshot's frame and distance of a walking carrier are those of its
//! last step, and a virtual walk's offset that of the role's last wake-up. [`Animator`] keeps
//! the draw-time copy: it takes the snapshot's values whenever the simulation changed them, and
//! otherwise advances them as `Update` (and the role's `UpdateJob`) would, once per tick. The
//! distance runs continuously between ticks, for smooth motion at any frame rate.
//!
//! Vehicles: every `GetGfxInfo` runs `CVehicle`'s update 0x594db0 ([`crate::vehicle_gfx::update`])
//! for the ticks since `+0x2c`, which the simulation's `LogicUpdate` also runs. So the turn
//! (+0x5d / +0x5e) and the distance follow from the snapshot (the update's state at `+0x2c`) and
//! the ticks since; the distance goes down by `256 / +0x62` per tick once the turn is done, and
//! the renderer (types 4 and 2) glides from the previous node like a settler. The frame needs
//! the gfx frame count (+0x61), which the simulation does not have: [`Animator::vehicle_poses`]
//! keeps the frame of each vehicle at its `+0x2c` and plays it on.

use std::collections::HashMap;

use s4_sim::jobs::state::{OFFSET_ONE, role};
use s4_sim::jobs::walking::NEIGHBOR;

use crate::population::{Population, Unit};
use crate::terrain::{ROW_H, TRI_W, node_pos};
use crate::vehicle_gfx::{self, Anim};

/// How a unit is drawn now: displacement from its node anchor (world px) and the frame of its
/// gfx job (`None`: the frame the sprite scene placed).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pose {
    pub dx: f32,
    pub dy: f32,
    pub frame: Option<u8>,
    /// A turning vehicle: its 16-way direction (`+0x5e & 0x7f`), the index of the turn sprite
    /// ([`crate::vehicle_gfx::TURN`]) drawn instead of its job.
    pub turn: Option<u8>,
}

/// The draw-time state of one settler.
#[derive(Clone, Copy, Debug)]
struct Track {
    /// The unit as the last snapshot showed it (to tell what the simulation changed).
    seen: Unit,
    frame: u8,
    /// The virtual-task offset (fixed point, as [`Unit::offset`]).
    offset: (i32, i32),
    /// Tick the frame and offset were advanced to.
    tick: u32,
    /// Distance (1/256 of a node, 0xFF counted as 256) at tick `dist_at`.
    dist: f32,
    dist_at: f64,
}

#[derive(Default)]
pub struct Animator {
    tracks: HashMap<u16, Track>,
    /// Snapshot tick the tracks were last synced with.
    synced: Option<u32>,
    /// Vehicles: the unit as last seen and its frame / half frame at that unit's `+0x2c`.
    vehicles: HashMap<u16, (Unit, Anim)>,
    vehicles_synced: Option<u32>,
}

/// `IMovingEntity::DecDistance` counts 0xFF as 256.
fn distance(d: u8) -> f32 {
    if d == 0xFF { 256.0 } else { d as f32 }
}

/// One tick (elapsed 1) of `ISettlerRole::Update` (0x586a00): animated tasks play their cycle
/// forwards or backwards; GO (6) and the WAIT step (17) loop the walk cycle without frame 0, or
/// stand on frame 0 when the last walk made no step (`walk & 8`). Other tasks go to the role's
/// `UpdateJob`: the virtual walk (task 0, house and free workers) loops its cycle without frame 0
/// and moves the offset by its step; putting a good down plays its frames back to 0, picking one
/// up plays them forwards to the last (house worker tasks 4 / 5, free worker 4, 21 / 5, 22,
/// carrier 21 / 20, 22, 23).
fn step(u: &Unit, f: u8, off: &mut (i32, i32)) -> u8 {
    let c = u.cycle as u32;
    let f = f as u32;
    let (house, free, carrier) = (u.role == role::HOUSE_WORKER, u.role == role::FREE_WORKER, u.role == role::CARRIER);
    let soldier = soldier_job(u.role);
    let n = match u.task {
        // `CSoldierRole::UpdateJob` (0x589750): the behaviour task walks the walk cycle without
        // frame 0, or stands on frame 0 (step word 0x08); tasks 0x10 / 0x22 loop their cycle.
        BEHAVIOUR if soldier && u.walk & 8 != 0 => 0,
        BEHAVIOUR if soldier && c > 0 => match (f + 1) % c {
            0 => 1,
            n => n,
        },
        0x10 | 0x22 if soldier && c > 0 => (f + 1) % c,
        1 | 3 | 13 | 16 | 25 | 28..=31 if c > 0 => {
            if u.forward {
                (f + 1) % c
            } else if f == 0 {
                c - 1
            } else {
                f - 1
            }
        }
        6 | 17 if u.walk & 8 != 0 => 0,
        6 | 17 if c > 0 => match (f + 1) % c {
            0 => 1,
            n => n,
        },
        // The house worker returns before moving when the cycle is empty.
        0 if house && c == 0 => f,
        0 if house || free => {
            *off = (off.0 + u.step.0, off.1 + u.step.1);
            match c {
                0 => 0,
                _ => match (f + 1) % c {
                    0 if c > 1 => 1,
                    n => n,
                },
            }
        }
        4 if house || free => f.saturating_sub(1),
        21 if free || carrier => f.saturating_sub(1),
        5 if house || free => (f + 1).min(c.saturating_sub(1)),
        22 if free || carrier => (f + 1).min(c.saturating_sub(1)),
        20 | 23 if carrier => (f + 1).min(c.saturating_sub(1)),
        _ => f,
    };
    n as u8
}

/// Ticks per node of a step: the walk speed for GO, `cycle − 1` (at least 1) for the WAIT step.
fn step_ticks(u: &Unit) -> Option<f32> {
    match u.task {
        6 if u.counter > 0 => Some(u.counter as f32),
        17 => Some((u.cycle as f32 - 1.0).max(1.0)),
        // 0x589750: `DecDistance(elapsed · 256 / counter)` while not standing.
        BEHAVIOUR if soldier_job(u.role) && u.walk & 8 == 0 && u.counter > 0 => Some(u.counter as f32),
        _ => None,
    }
}

/// The soldiers' behaviour-driven task (`ISettlerRole +4` = 0x1b, `s4_sim::jobs::soldier`).
const BEHAVIOUR: u8 = 0x1b;

/// Roles whose `UpdateJob` (vtable +0x20) is `CSoldierRole`'s 0x589750: soldier (7), saboteur
/// (10, vtable 0x105bdbc), role 16 (vtable 0x105c14c) and squad leader (23, 0x105b734 /
/// 0x105b7f0). It is the only `UpdateJob` that moves the walk distance; the others (carrier
/// 0x573560, donkey 0x576610, hunter 0x57d4a0, mushroom farmer 0x57dc90, slave 0x587ff0, thief
/// 0x58b750, flee and the empty 0x572cd0) only change frames.
fn soldier_job(r: u8) -> bool {
    matches!(r, role::SOLDIER | role::SABOTEUR | 16 | role::SQUAD_LEADER)
}

impl Animator {
    /// The poses of every drawn settler of `pop` at game time `t` (ticks, fractional: the
    /// snapshot's tick plus the fraction of the next one that has passed).
    pub fn poses(&mut self, pop: &Population, t: f64) -> HashMap<u16, Pose> {
        let tick = pop.tick;
        let resync = self.synced != Some(tick);
        // Settlers drawn on their own, house workers drawn by their building, and military
        // buildings' soldiers (at their slot: frame only).
        let garrison = || pop.buildings.iter().flat_map(|b| b.inhabitants.iter().filter_map(|i| i.unit.as_ref()));
        let garrisoned: std::collections::HashSet<u16> = garrison().map(|u| u.id).collect();
        let units = || {
            pop.settlers
                .iter()
                .filter(|u| u.id != 0 && u.drawn())
                .map(|u| (u, false))
                .chain(pop.buildings.iter().filter_map(|b| b.worker.as_ref()).map(|u| (u, true)))
                .chain(garrison().map(|u| (u, true)))
        };
        if resync {
            self.synced = Some(tick);
            let ids: std::collections::HashSet<u16> = units().map(|(u, _)| u.id).collect();
            self.tracks.retain(|id, _| ids.contains(id));
        }
        let now = (t.floor() as u32).max(tick);
        let h = |x: i32, y: i32| {
            (x >= 0 && y >= 0 && (x as usize) < pop.size).then(|| pop.height.get(y as usize * pop.size + x as usize).copied()).flatten()
        };
        let mut out = HashMap::with_capacity(pop.settlers.len());
        for (u, in_building) in units() {
            // First seen standing (no draw-time decrement) with the step's 0xFF: the simulation
            // never decrements, the release's +0x41 would be down to 0 by now (a loaded game's
            // soldiers stood one node behind until they walked on).
            let first = if step_ticks(u).is_none() && u.distance == 0xFF { 0.0 } else { distance(u.distance) };
            let tr = self.tracks.entry(u.id).or_insert_with(|| Track { seen: *u, frame: u.frame, offset: u.offset, tick, dist: first, dist_at: tick as f64 });
            if resync {
                let s = tr.seen;
                if (s.task, s.job, s.cycle, s.frame, s.offset, s.step) != (u.task, u.job, u.cycle, u.frame, u.offset, u.step) {
                    tr.frame = u.frame;
                    tr.offset = u.offset;
                    tr.tick = tick;
                }
                if (s.x, s.y, s.distance) != (u.x, u.y, u.distance) {
                    tr.dist = distance(u.distance);
                    tr.dist_at = tick as f64;
                } else if step_ticks(&s) != step_ticks(u) {
                    // The walk stopped (a soldier's walk & 8: 0x589750 no longer decrements) or
                    // changed speed on the same step: the distance covered so far stays, as the
                    // release's +0x41 does. (Taking the snapshot's 0xFF again drew a soldier that
                    // stopped at its node back at the previous one: playtest P7.)
                    if let Some(n) = step_ticks(&s) {
                        tr.dist = (tr.dist - ((tick as f64 - tr.dist_at) as f32) * 256.0 / n).clamp(0.0, 256.0);
                    }
                    tr.dist_at = tick as f64;
                }
                tr.seen = *u;
            }
            // At most one cycle's worth of catching up matters; skip whole cycles.
            if now > tr.tick + 512 {
                tr.tick = now - 512;
            }
            while tr.tick < now {
                tr.frame = step(u, tr.frame, &mut tr.offset);
                tr.tick += 1;
            }
            let mut pose = Pose { frame: Some(tr.frame), ..Pose::default() };
            if garrisoned.contains(&u.id) {
                // At its slot's offset (the building part's), no displacement of its own.
            } else if in_building {
                // `(int)` of the float offset: pixels from the building's anchor.
                pose.dx = (tr.offset.0 / OFFSET_ONE) as f32;
                pose.dy = (tr.offset.1 / OFFSET_ONE) as f32;
            } else if u.flags & 0x800 != 0 {
                // `(int)` of the float offset, then the renderer's column / row scaling.
                let (ox, oy) = (tr.offset.0 / OFFSET_ONE, tr.offset.1 / OFFSET_ONE);
                pose.dx = (ox * TRI_W / 256) as f32;
                pose.dy = (oy * ROW_H / 256) as f32;
            } else if let Some(&(ddx, ddy)) = NEIGHBOR.get(u.dir as usize) {
                let d = match step_ticks(u) {
                    Some(n) => (tr.dist - ((t - tr.dist_at) as f32) * 256.0 / n).clamp(0.0, 256.0),
                    None => tr.dist,
                };
                let (x, y) = (u.x as i32, u.y as i32);
                let (px, py) = (x - ddx, y - ddy);
                if let (Some(hc), Some(hp)) = (h(x, y), h(px, py))
                    && d > 0.0
                {
                    let c = node_pos(x as usize, y as usize, hc);
                    let p = node_pos(px as usize, py as usize, hp);
                    pose.dx = (p[0] - c[0]) * d / 256.0;
                    pose.dy = (p[1] - c[1]) * d / 256.0;
                }
            }
            out.insert(u.id, pose);
        }
        for a in pop.animals.iter().filter(|a| a.id != 0 && a.flags & 0x80100 == 0x100) {
            out.insert(a.id, animal_pose(a, t, &h));
        }
        out
    }
}

impl Animator {
    /// The poses of the drawn vehicles of `pop` at game time `t`. `frames` gives each vehicle's
    /// `+0x61` (the direction-2 frame count of its job +0x26, see
    /// [`crate::sprites::SpriteScene::vehicle_frames`]).
    pub fn vehicle_poses(&mut self, pop: &Population, t: f64, frames: &HashMap<u16, u8>) -> HashMap<u16, Pose> {
        let tick = pop.tick;
        let resync = self.vehicles_synced != Some(tick);
        let drawn = || pop.vehicles.iter().filter(|u| u.id != 0 && u.flags & 0x80100 == 0x100).filter_map(|u| Some((u, u.vehicle?)));
        if resync {
            self.vehicles_synced = Some(tick);
            let ids: std::collections::HashSet<u16> = drawn().map(|(u, _)| u.id).collect();
            self.vehicles.retain(|id, _| ids.contains(id));
            for (u, g) in drawn() {
                let n = frames.get(&u.id).copied().unwrap_or(0);
                let fresh = (*u, Anim { turn: g.turn, dir16: g.dir16, frame: u.frame, half: g.half_frame });
                let tr = self.vehicles.entry(u.id).or_insert(fresh);
                let (s, sg) = (tr.0, tr.0.vehicle.unwrap_or_default());
                if (s.job, sg.task, s.frame, sg.half_frame) != (u.job, g.task, u.frame, g.half_frame) {
                    // The simulation took a job (0x595150 / 0x596280: frame 0).
                    tr.1 = fresh.1;
                } else if sg.anim_tick != g.anim_tick {
                    // Its LogicUpdate ran the update to the new +0x2c: play the frames of the
                    // ticks in between as the old state would have.
                    let mut a = Anim { turn: sg.turn, dir16: sg.dir16, ..tr.1 };
                    vehicle_gfx::update(&mut a, sg.task, sg.step, s.flags, n, g.anim_tick.wrapping_sub(sg.anim_tick));
                    tr.1.frame = a.frame;
                    tr.1.half = a.half;
                }
                tr.0 = *u;
            }
        }
        let now = (t.floor() as u32).max(tick);
        let h = |x: i32, y: i32| {
            (x >= 0 && y >= 0 && (x as usize) < pop.size).then(|| pop.height.get(y as usize * pop.size + x as usize).copied()).flatten()
        };
        let mut out = HashMap::new();
        for (u, g) in drawn() {
            let Some(&(_, base)) = self.vehicles.get(&u.id) else { continue };
            let mut a = Anim { turn: g.turn, dir16: g.dir16, ..base };
            let n = frames.get(&u.id).copied().unwrap_or(0);
            vehicle_gfx::update(&mut a, g.task, g.step, u.flags, n, now.saturating_sub(g.anim_tick));
            let mut pose = Pose { frame: Some(a.frame), turn: (a.turn != 0).then_some(a.dir16 & 0x7f), ..Pose::default() };
            let mut d = distance(u.distance);
            if vehicle_gfx::moves(g.task, g.step) {
                let ticks = (t - g.anim_tick as f64 - g.turn as f64).max(0.0) as f32;
                d = (d - ticks * 256.0 / g.speed.max(1) as f32).clamp(0.0, 256.0);
            }
            if let Some(&(ddx, ddy)) = NEIGHBOR.get(u.dir as usize) {
                let (x, y) = (u.x as i32, u.y as i32);
                let (px, py) = (x - ddx, y - ddy);
                if let (Some(hc), Some(hp)) = (h(x, y), h(px, py))
                    && d > 0.0
                {
                    let c = node_pos(x as usize, y as usize, hc);
                    let p = node_pos(px as usize, py as usize, hp);
                    pose.dx = (p[0] - c[0]) * d / 256.0;
                    pose.dy = (p[1] - c[1]) * d / 256.0;
                }
            }
            out.insert(u.id, pose);
        }
        out
    }
}

/// A land animal at game time `t`, as `CAnimal::GetGfxInfo` (0x52bb30) draws it after its
/// draw-time update (0x52bb90). That update counts the ticks since the animal's last update; the
/// dead (+0x58 = 5) keep frame and distance. Walking (task 6) not blocked (+0x68 & 8): frame
/// `(frame + ticks) % frames` and `DecDistance(ticks · 256 / speed)`; blocked walking and
/// standing (0x11): frame 0; idle and dying animations (0x10, 0x1b): `(frame + ticks) %
/// frames`. `TakeJob` (0x52bd40) starts at frame 0 with distance 0 and a step sets distance 0xFF,
/// so both follow from the ticks since `TakeJob` and since the step. The renderer's type 0x80
/// (0x6609f0) glides from the previous node like a settler's.
fn animal_pose(a: &crate::population::Animal, t: f64, h: &impl Fn(i32, i32) -> Option<u8>) -> Pose {
    if a.state == 5 {
        return Pose { frame: Some(a.frame), ..Pose::default() };
    }
    let n = a.frames.max(1) as u32;
    let since_job = (t.floor() as u32).wrapping_sub(a.job_tick);
    let frame = match a.task_type {
        6 if a.moved & 8 == 0 => since_job % n,
        0x10 | 0x1b => since_job % n,
        _ => 0,
    };
    let mut pose = Pose { frame: Some(frame as u8), ..Pose::default() };
    if a.task_type != 6 || a.moved & 8 != 0 || a.step_tick < a.job_tick {
        return pose;
    }
    let d = (256.0 - (t - a.step_tick as f64) as f32 * 256.0 / a.speed.max(1) as f32).clamp(0.0, 256.0);
    if let Some(&(ddx, ddy)) = NEIGHBOR.get(a.dir as usize) {
        let (x, y) = (a.x as i32, a.y as i32);
        let (px, py) = (x - ddx, y - ddy);
        if let (Some(hc), Some(hp)) = (h(x, y), h(px, py))
            && d > 0.0
        {
            let c = node_pos(x as usize, y as usize, hc);
            let p = node_pos(px as usize, py as usize, hp);
            pose.dx = (p[0] - c[0]) * d / 256.0;
            pose.dy = (p[1] - c[1]) * d / 256.0;
        }
    }
    pose
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pop(tick: u32, u: Unit) -> Population {
        Population { tick, size: 64, height: vec![0; 64 * 64], settlers: vec![u], ..Default::default() }
    }

    fn walker(x: u16, distance: u8) -> Unit {
        // A carrier walking +x (direction 0) at speed 9 on its 8-frame walk cycle.
        Unit { id: 7, obj: 1, flags: 0x100, x, y: 10, kind: 1, dir: 0, task: 6, counter: 9, cycle: 8, walk: 0, frame: 3, distance, ..Default::default() }
    }

    #[test]
    fn walking_settler_glides_from_the_previous_node() {
        let mut a = Animator::default();
        // Just stepped onto (11, 10) at tick 100: drawn on (10, 10), one node (24 px) left.
        let p = pop(100, walker(11, 0xFF));
        assert_eq!(a.poses(&p, 100.0)[&7].dx, -24.0);
        // Halfway after 4.5 of the 9 ticks, on the node after 9.
        assert_eq!(a.poses(&p, 104.5)[&7].dx, -12.0);
        assert_eq!(a.poses(&p, 109.0)[&7].dx, 0.0);
        // The next step (snapshot of tick 109) starts over from the new previous node.
        let q = pop(109, walker(12, 0xFF));
        assert_eq!(a.poses(&q, 109.0)[&7].dx, -24.0);
    }

    /// User report: soldiers walked in jumps. Their walk is the behaviour task 0x1b, whose
    /// `UpdateJob` (0x589750) moves the distance like GO: they glide too.
    #[test]
    fn soldier_behaviour_walk_glides() {
        let mut a = Animator::default();
        let s = Unit { role: role::SOLDIER, task: 0x1b, counter: 7, frame: 0, ..walker(11, 0xFF) };
        let p = pop(100, s);
        assert_eq!(a.poses(&p, 100.0)[&7].dx, -24.0);
        assert_eq!(a.poses(&p, 103.5)[&7].dx, -12.0);
        let q = a.poses(&p, 105.0)[&7];
        assert!((q.dx + 24.0 * 2.0 / 7.0).abs() < 1e-4 && q.frame == Some(5), "{q:?}");
        assert_eq!(a.poses(&p, 107.0)[&7].dx, 0.0);
        // Standing (step word 0x08, no step made): frame 0 on its node.
        let mut b = Animator::default();
        let st = pop(100, Unit { walk: 8, distance: 0, frame: 3, ..s });
        let r = b.poses(&st, 104.0)[&7];
        assert_eq!((r.dx, r.frame), (0.0, Some(0)));
    }

    /// Playtest P7 ("soldiers walk towards a node, then are teleported back"): the simulation
    /// leaves a soldier's distance at the step's 0xFF; when the soldier stops on that step (the
    /// step word's 0x08, same node and distance), the distance it covered stays (the release's
    /// +0x41, which `UpdateJob` 0x589750 no longer decrements), not 0xFF again. A soldier first
    /// seen standing with 0xFF (a loaded game) is on its node.
    #[test]
    fn a_stopped_soldier_stays_where_its_walk_got_to() {
        let mut a = Animator::default();
        let s = Unit { role: role::SOLDIER, task: 0x1b, counter: 8, frame: 0, ..walker(11, 0xFF) };
        assert_eq!(a.poses(&pop(100, s), 100.0)[&7].dx, -24.0);
        assert_eq!(a.poses(&pop(100, s), 106.0)[&7].dx, -6.0);
        // Tick 108: on its node; it stops there (walk & 8) and stays.
        let stop = Unit { walk: 0x18, ..s };
        for t in [108.0, 108.5, 112.0] {
            assert_eq!(a.poses(&pop(108, stop), t)[&7].dx, 0.0, "t {t}");
        }
        // Stopped after 4 of the 8 ticks: halfway, and it stays halfway.
        let mut b = Animator::default();
        assert_eq!(b.poses(&pop(200, s), 200.0)[&7].dx, -24.0);
        b.poses(&pop(203, s), 203.5);
        for t in [204.0, 206.0, 210.0] {
            assert_eq!(b.poses(&pop(204, stop), t)[&7].dx, -12.0, "t {t}");
        }
        // A loaded game: standing with 0xFF on its node.
        let mut c = Animator::default();
        assert_eq!(c.poses(&pop(300, stop), 300.0)[&7].dx, 0.0);
    }

    #[test]
    fn animal_walks_from_the_previous_node() {
        use crate::population::Animal;
        // A deer walking +x (direction 0), 10 ticks per step, 8 frames; job taken at 90, the step
        // onto (11, 10) at 100.
        let a = Animal { id: 9, uid: 0, x: 11, y: 10, kind: 2, state: 2, task_type: 6, job: 5, dir: 0, flags: 0x100, frames: 8, moved: 0, speed: 10, frame: 0, job_tick: 90, step_tick: 100 };
        let p = Population { tick: 100, size: 64, height: vec![0; 64 * 64], animals: vec![a], ..Default::default() };
        let mut an = Animator::default();
        assert_eq!(an.poses(&p, 100.0)[&9], Pose { dx: -24.0, dy: 0.0, frame: Some(2), turn: None });
        assert_eq!(an.poses(&p, 105.0)[&9], Pose { dx: -12.0, dy: 0.0, frame: Some(7), turn: None });
        assert_eq!(an.poses(&p, 110.0)[&9], Pose { dx: 0.0, dy: 0.0, frame: Some(4), turn: None });
        // Blocked: frame 0 on its node; hidden: not drawn.
        let b = Population { animals: vec![Animal { moved: 0x88, ..a }], ..p.clone() };
        assert_eq!(an.poses(&b, 105.0)[&9], Pose { dx: 0.0, dy: 0.0, frame: Some(0), turn: None });
        let c = Population { animals: vec![Animal { flags: 0, ..a }], ..p };
        assert!(!an.poses(&c, 105.0).contains_key(&9));
    }

    /// A cart that stepped onto (11, 10) going +x at tick 100 with 3 sixteenths still to turn:
    /// the turn sprite for 3 ticks (on the previous node), then the glide at 256 / 8 per tick
    /// with its 12-frame walk cycle; a logic update at 108 (no step: +0x5c = 8) keeps the
    /// frame the draw-time update reached.
    #[test]
    fn vehicle_turns_then_glides_and_keeps_its_frame() {
        use crate::population::VehicleGfx;
        let g = VehicleGfx { anim_tick: 100, task: 6, step: 0, speed: 8, turn: 3, dir16: 0x80 | 13, ..Default::default() };
        let u = Unit { id: 5, obj: 4, flags: 0x100, x: 11, y: 10, kind: 5, dir: 0, job: 38, frame: 2, distance: 0xFF, vehicle: Some(g), ..Default::default() };
        let p = Population { tick: 100, size: 64, height: vec![0; 64 * 64], vehicles: vec![u], ..Default::default() };
        let frames = HashMap::from([(5, 12)]);
        let mut a = Animator::default();
        let at = |a: &mut Animator, p: &Population, t: f64| a.vehicle_poses(p, t, &frames)[&5];
        assert_eq!(at(&mut a, &p, 101.0), Pose { dx: -24.0, dy: 0.0, frame: Some(2), turn: Some(14) });
        assert_eq!(at(&mut a, &p, 103.0), Pose { dx: -24.0, dy: 0.0, frame: Some(2), turn: None });
        assert_eq!(at(&mut a, &p, 107.0), Pose { dx: -12.0, dy: 0.0, frame: Some(6), turn: None });
        // The simulation's update at 108: no step, the frame it holds unchanged (it has no +0x61).
        let g2 = VehicleGfx { anim_tick: 108, step: 8, turn: 0, dir16: 0x80, ..g };
        let q = Population { tick: 108, vehicles: vec![Unit { distance: 0, vehicle: Some(g2), ..u }], ..p.clone() };
        assert_eq!(at(&mut a, &q, 110.0), Pose { dx: 0.0, dy: 0.0, frame: Some(7), turn: None });
    }

    #[test]
    fn walk_cycle_skips_frame_zero_and_continues_across_steps() {
        let mut a = Animator::default();
        let p = pop(100, walker(11, 0xFF));
        let frames: Vec<u8> = (100..106).map(|t| a.poses(&p, t as f64)[&7].frame.unwrap()).collect();
        assert_eq!(frames, [3, 4, 5, 6, 7, 1]);
        // A step changes neither the task nor the frame the simulation holds: the cycle goes on.
        let q = pop(109, walker(12, 0xFF));
        assert_eq!(a.poses(&q, 109.0)[&7].frame, Some(5));
    }

    #[test]
    fn blocked_walker_stands_and_hidden_settlers_are_not_drawn() {
        let mut a = Animator::default();
        let p = pop(100, Unit { walk: 0x88, ..walker(11, 0) });
        assert_eq!(a.poses(&p, 101.0)[&7].frame, Some(0));
        let hidden = pop(101, Unit { flags: 0, ..walker(11, 0) });
        assert!(a.poses(&hidden, 101.0).is_empty());
    }

    #[test]
    fn virtual_task_uses_the_role_offset() {
        let mut a = Animator::default();
        // A woodcutter beside its tree: half a column right, one row up (127·24/256 = 11.9 → 11).
        let u = Unit { flags: 0x900, task: 29, cycle: 12, offset: (127 * OFFSET_ONE, -255 * OFFSET_ONE), ..walker(11, 0) };
        let pose = a.poses(&pop(100, u), 100.5)[&7];
        assert_eq!((pose.dx, pose.dy), (11.0, -11.0));
    }

    #[test]
    fn house_worker_in_its_building_uses_the_pixel_offset() {
        use crate::population::Building;
        let mut a = Animator::default();
        // A sawmill worker at cell (4, 3) of its building: (24·4 − 12·3, 12·3) = (60, 36) px,
        // walking on by (1, −1.5) px per tick. Hidden as a settler (MAGIC_INVISIBLE), drawn by
        // the building at the offset as is.
        let w = Unit { flags: 0x80900, role: role::HOUSE_WORKER, task: 0, cycle: 8, frame: 1, offset: (60 * OFFSET_ONE, 36 * OFFSET_ONE), step: (OFFSET_ONE, -3 * OFFSET_ONE / 2), ..walker(11, 0) };
        let b = Building { id: 3, x: 11, y: 10, kind: 3, owner: 1, race: 0, inhabitants: vec![], site: None, worker: Some(w), door: (11, 10), land: 0, ready: true, generic: 3, flag: None, pen: None, mushroom: None };
        let p = Population { tick: 100, size: 64, height: vec![0; 64 * 64], buildings: vec![b], ..Default::default() };
        let pose = a.poses(&p, 100.0)[&7];
        assert_eq!((pose.dx, pose.dy), (60.0, 36.0));
        let pose = a.poses(&p, 102.0)[&7];
        assert_eq!((pose.dx, pose.dy, pose.frame), (62.0, 33.0, Some(3)));
    }

    #[test]
    fn virtual_walk_moves_every_tick() {
        let mut a = Animator::default();
        // A free worker's virtual walk: 32 units of a row per tick (1.5 px), cycle of 8 frames.
        let u = Unit { flags: 0x900, role: role::FREE_WORKER, task: 0, cycle: 8, frame: 7, step: (0, 32 * OFFSET_ONE), ..walker(11, 0) };
        let p = pop(100, u);
        let at = |a: &mut Animator, t: f64| {
            let q = a.poses(&p, t)[&7];
            (q.dy, q.frame.unwrap())
        };
        assert_eq!(at(&mut a, 100.0), (0.0, 7));
        // 32·12/256 = 1.5 → 1 px; the cycle skips frame 0.
        assert_eq!(at(&mut a, 101.0), (1.0, 1));
        assert_eq!(at(&mut a, 104.0), (6.0, 4));
    }
}
