//! `CShip` (vtable 0x105c704, 0x590300..0x590b90) and the ships' side of the world: the ship
//! cells and the water-flag marks around a ship (0x5bb710..0x5bc3c6), and the `CWalkingShip`
//! walker parts (vtable 0x1060e1c). Decompiles `tools/re/vehicles/ship.c`, `shipctors.c`,
//! `shipmark*.c`, `shipwalk.c`.
//!
//! * **Ship cells** (.bss 0x1443cb8, 16-byte cells of 16 × 16 nodes, 64 per row): three slots
//!   for the packed positions of the ships in the cell and a fourth that one ship may take only
//!   while the fourth slots of the cell's six hex neighbours (in cell coordinates) are free
//!   (0x5bb960). No slot, no ship there.
//! * **Marks** (0x5bc070 / 0x5bc130): on a water node (ground type ≤ 8), the ship sets water-flag
//!   bit 0x800 on its node and keeps in the low byte of every water node (flags ≥ 0xc000) within
//!   9 (`g_sSurroundingHexPoints8`, 271 entries) the closeness `10 − distance` of the nearest
//!   ship (bits 0..3) and of the second nearest (bits 4..7). Taking a mark away recomputes them
//!   from the ships of the 3 × 3 cells around within 19 (0x5bb850 / 0x5bb7c0 / 0x5bb710).
//! * **The walker** (`CWalkingShip`): a node is open on water whose flags `& 0xc300 == 0xc000`
//!   (0x5ba7d0) and free without bit 0x800 (0x5ba800). Its fine search lifts its own mark
//!   (0x5ba820). On a fine step another ship's closeness 3..=7 at the next node makes it wait,
//!   and within 3 of the goal it counts as arrived (`Walk` 0x5b96c6, [`walk_step`]). Its
//!   `IdleWalk` (0x5ba8a0, [`idle_walk`]) drifts away from other ships and leaves a shipyard's
//!   water (bit 0x1000).
//! * **On the map** the ship holds its node in the map-object layer (0x590370, a step 0x5908d0,
//!   off the map 0x590a20).
//!
//! The original indexes the cell array without bounds (rows −1 and 64 read neighbouring .bss);
//! cells outside the array count as empty here [inf].

use super::{get, get_mut, kind, logic};
use crate::World;
use crate::entity::{ObjType, flag};
use crate::grid::Y16X16;
use crate::hash::StateHasher;
use crate::pathing::walking::WalkState;

/// Cells per row / column.
const CELLS: usize = 64;
/// `Y16X16::g_iNeighborY16X16Modifiers` (.rdata 0x101bd58).
const MODIFIER: [i32; 6] = [1, 0x10001, 0x10000, -1, -0x10001, -0x10000];
/// `g_sNeighborPoints` (.rdata 0x101bd88).
const NEIGHBOR: [(i32, i32); 6] = [(1, 0), (1, 1), (0, 1), (-1, 0), (-1, -1), (0, -1)];
/// `Y16X16::DirectionFast` (.rdata 0x101c8fc).
const DIRECTION: [usize; 8] = [0, 1, 2, 2, 5, 3, 4, 3];
/// Water-flag bits: water, the walker's open-water mask, a ship on the node, a shipyard's water.
const WATER: u16 = 0xc000;
const SHIP_HERE: u16 = 0x800;
const SHIPYARD: u16 = 0x1000;

/// The ship cells (0x1443cb8).
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ShipCells(pub Vec<[i32; 4]>);

impl Default for ShipCells {
    fn default() -> Self {
        ShipCells(vec![[0; 4]; CELLS * CELLS])
    }
}

impl ShipCells {
    pub(crate) fn hash(&self, h: &mut StateHasher) {
        for c in &self.0 {
            c.iter().for_each(|&v| h.u32(v as u32));
        }
    }

    fn cell(&self, cx: i32, cy: i32) -> Option<usize> {
        ((0..CELLS as i32).contains(&cx) && (0..CELLS as i32).contains(&cy)).then(|| cy as usize * CELLS + cx as usize)
    }

    fn get(&self, cx: i32, cy: i32, slot: usize) -> i32 {
        self.cell(cx, cy).map_or(0, |c| self.0[c][slot])
    }
}

fn cell_of(pos: i32) -> (i32, i32) {
    ((pos >> 4) & 0xfff, pos >> 20)
}

fn hexdist(a: i32, b: i32) -> i32 {
    let (dx, dy) = ((a & 0xffff) - (b & 0xffff), (a >> 16) - (b >> 16));
    crate::spiral::grid_distance(dx, dy)
}

fn hex(radius: i32) -> &'static [(i32, i32)] {
    static HEX: std::sync::OnceLock<Vec<(i32, i32)>> = std::sync::OnceLock::new();
    let all = HEX.get_or_init(|| crate::tiling::hex_points(15));
    &all[..(1 + 3 * radius * (radius + 1)) as usize]
}

fn index(world: &World, pos: i32) -> i64 {
    (pos >> 16) as i64 * world.grid.size() as i64 + (pos & 0xffff) as i64
}

fn water(world: &World, n: i64) -> u16 {
    usize::try_from(n).ok().and_then(|n| world.tiling.water_flags.get(n).copied()).unwrap_or(0)
}

/// 0x5bb960 `(pos)`: a free slot of the cell of `pos`, `(cell, slot)`.
fn free_slot(world: &World, pos: i32) -> Option<(usize, usize)> {
    let c = &world.vehicles.ship_cells;
    let (cx, cy) = cell_of(pos);
    let i = c.cell(cx, cy)?;
    if let Some(s) = (0..3).find(|&s| c.0[i][s] == 0) {
        return Some((i, s));
    }
    let taken = c.0[i][3] != 0
        || c.get(cx, cy - 1, 3) != 0
        || c.get(cx, cy + 1, 3) != 0
        || (cx != 0 && (c.get(cx - 1, cy - 1, 3) != 0 || c.get(cx - 1, cy, 3) != 0))
        || (cx < CELLS as i32 - 1 && (c.get(cx + 1, cy, 3) != 0 || c.get(cx + 1, cy + 1, 3) != 0));
    (!taken).then_some((i, 3))
}

/// 0x5bb9f0 `(pos)`: `pos` out of its cell (the later slots move up); whether it was there.
fn remove(world: &mut World, pos: i32) -> bool {
    let (cx, cy) = cell_of(pos);
    let Some(i) = world.vehicles.ship_cells.cell(cx, cy) else { return false };
    let c = &mut world.vehicles.ship_cells.0[i];
    let Some(mut s) = (0..4).find(|&s| c[s] == pos || c[s] == 0).filter(|&s| c[s] == pos) else { return false };
    while s < 3 && c[s + 1] != 0 {
        c[s] = c[s + 1];
        s += 1;
    }
    c[s] = 0;
    true
}

/// 0x5bc070 `(pos)`: the ship's marks (see the module doc).
fn mark(world: &mut World, pos: i32) {
    let n = index(world, pos);
    if !ground_water(world, n) {
        return;
    }
    let len = world.grid.len() as i64;
    let wf = &mut world.tiling.water_flags;
    wf[n as usize] |= SHIP_HERE;
    for &(dx, dy) in hex(9) {
        let m = n + dy as i64 * world.grid.size() as i64 + dx as i64;
        if !(0..len).contains(&m) {
            continue;
        }
        let v = wf[m as usize];
        if v < WATER {
            continue;
        }
        let near = 10 - crate::spiral::grid_distance(dx, dy) as u16;
        let (lo, hi) = (v & 0xf, (v >> 4) & 0xf);
        let b = if near < lo {
            if near < hi { v & 0xff } else { lo | near << 4 }
        } else {
            lo << 4 | near
        };
        wf[m as usize] = (v & 0xff00) | b;
    }
}

/// The ship node's ground type is water (≤ 8, `[0x15630d4 + n·4 + 1]`).
fn ground_water(world: &World, n: i64) -> bool {
    usize::try_from(n).ok().and_then(|n| world.layers.ground.get(n)).is_some_and(|&g| g <= 8)
}

/// 0x5bc130 `(pos)`: the marks of a ship at `pos` go (its cell entry is already gone): bit 0x800
/// off, the closeness nibbles within 9 recomputed from the ships around.
fn unmark(world: &mut World, pos: i32) {
    let n = index(world, pos);
    if !ground_water(world, n) {
        return;
    }
    let ships = near_ships(world, pos);
    let len = world.grid.len() as i64;
    let side = world.grid.size() as i64;
    let wf = &mut world.tiling.water_flags;
    wf[n as usize] &= !SHIP_HERE;
    for &(dx, dy) in hex(9) {
        let m = n + dy as i64 * side + dx as i64;
        if !(0..len).contains(&m) {
            continue;
        }
        let v = wf[m as usize];
        if v < WATER {
            continue;
        }
        let at = pos.wrapping_add(dx).wrapping_add(dy.wrapping_mul(0x10000));
        wf[m as usize] = (v & 0xff00) | closeness(&ships, at);
    }
}

/// 0x5bb850 / 0x5bb7c0: the ships of the 3 × 3 cells around `pos` within 19 (columns cx, cx−1,
/// cx+1; rows cy, cy−1, cy+1 each).
fn near_ships(world: &World, pos: i32) -> Vec<i32> {
    let c = &world.vehicles.ship_cells;
    let (cx, cy) = cell_of(pos);
    let mut cols = vec![cx];
    if cx != 0 {
        cols.push(cx - 1);
    }
    if cx < CELLS as i32 - 1 {
        cols.push(cx + 1);
    }
    let mut out = Vec::new();
    for x in cols {
        for y in [cy, cy - 1, cy + 1] {
            for s in 0..4 {
                let p = c.get(x, y, s);
                if p == 0 {
                    break;
                }
                if hexdist(pos, p) < 19 {
                    out.push(p);
                }
            }
        }
    }
    out
}

/// 0x5bb710 `(ships, at)`: `(10 − second nearest) << 4 | (10 − nearest)`, distances below 10.
fn closeness(ships: &[i32], at: i32) -> u16 {
    let (mut a, mut b) = (10, 10);
    for &p in ships {
        let d = hexdist(p, at);
        if d < 10 {
            if d < a {
                b = a;
                a = d;
            } else if d < b {
                b = d;
            }
        }
    }
    ((10 - b) << 4 | (10 - a)) as u16
}

/// 0x5bc2a0 `(pos)`: on open water with a free cell slot the ship takes it and marks.
fn place(world: &mut World, pos: i32) -> bool {
    if water(world, index(world, pos)) & 0xcb00 != WATER {
        return false;
    }
    let Some((c, s)) = free_slot(world, pos) else { return false };
    world.vehicles.ship_cells.0[c][s] = pos;
    mark(world, pos);
    true
}

/// `remove` + `unmark` (0x5bb9f0 → 0x5bc130): whether the ship was in its cell.
fn lift(world: &mut World, pos: i32) -> bool {
    if remove(world, pos) {
        unmark(world, pos);
        return true;
    }
    false
}

/// 0x5bc2f0 `(pos)`: a ship can be made at `pos` (open water, a cell slot).
pub fn can_place(world: &World, x: i32, y: i32) -> bool {
    let pos = Y16X16::pack(x, y).0 as i32;
    world.grid.checked_index(x, y).is_some() && water(world, index(world, pos)) & 0xcb00 == WATER && free_slot(world, pos).is_some()
}

/// 0x5bc350 `(to, from)`: the ship's cell entry and marks move; if `to` has no room the old ones
/// come back (0x5bc2a0).
fn move_mark(world: &mut World, to: i32, from: i32) -> bool {
    let had = lift(world, from);
    if place(world, to) {
        return true;
    }
    if had {
        place(world, from);
    }
    false
}

pub(crate) fn is_ship(world: &World, id: u16) -> bool {
    world.entities.by_index(id as u32).is_some_and(|e| e.obj_type == ObjType::Ship)
}

/// `CShip` ctor 0x590370 (after `CVehicle`): its cell slot and marks, its node in the map-object
/// layer; a finished ship joins the war map, gets a task list on its place (0x595240) and its
/// first task.
pub fn init(world: &mut World, id: u16, under_construction: bool) {
    let Some(pos) = world.entities.by_index(id as u32).map(|e| e.pos) else { return };
    place(world, pos.0 as i32);
    if let Some(n) = world.grid.checked_index(pos.x(), pos.y()) {
        world.layers.map_object[n] = id;
    }
    if !under_construction {
        crate::warmap::readd(world, id);
        logic::new_task_list(world, id, pos.x(), pos.y(), 0);
        next_task(world, id);
    }
}

/// vtbl+0xc8 (0x590a20): off the map: its marks and its node; its owner hears "ship lost"
/// (0x52a460 with text 0x9a7 at its node; the original only calls it for the local player).
pub fn off_map(world: &mut World, id: u16) {
    let Some((pos, owner)) = world.entities.by_index(id as u32).map(|e| (e.pos, e.owner)) else { return };
    lift(world, pos.0 as i32);
    if let Some(n) = world.grid.checked_index(pos.x(), pos.y())
        && world.layers.map_object[n] == id
    {
        world.layers.map_object[n] = 0;
    }
    world.events.push(crate::event::SimEvent::warning(owner, 0x9a7, pos));
}

/// vtbl+0x8c (0x590b80): a settler comes to the ship's own node.
pub fn exit_place(world: &World, id: u16) -> i32 {
    world.entities.by_index(id as u32).map_or(0, |e| e.pos.0 as i32)
}

/// vtbl+0xbc (0x5907a0): the current task (0x595150); a walk (6 / 7, a waypoint 8 at −1/−1 on its
/// own place) starts the walker with the class's walk task (info +0x24), 0x24 deletes the ship,
/// 0x11 stands, anything else (none) is standing with the idle job; then vtbl+0x90.
pub fn next_task(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let (race, k, pos) = (e.race, e.kind, e.pos);
    let info = world.rules.vehicles.get(race, k as u8);
    let t = get(world, id).and_then(|v| (v.listed != 0).then(|| v.tasks.get(v.listed as usize - 1).copied()).flatten());
    logic::apply_task(world, id, t);
    if let Some(t) = t {
        match t.task {
            logic::task::WAYPOINT | logic::task::GO | logic::task::GO_TO_POS => {
                let mut to = Y16X16::pack(t.x as i32, t.y as i32);
                if t.task == logic::task::WAYPOINT && (t.x as u16 | t.y as u16) > 0x7fff {
                    to = pos;
                    if let Some(v) = get_mut(world, id)
                        && let Some(x) = v.tasks.get_mut(v.listed as usize - 1)
                    {
                        x.x = pos.x() as i16;
                        x.y = pos.y() as i16;
                    }
                }
                let walker = get_mut(world, id).and_then(|v| {
                    v.task = info.x24 as u8;
                    v.walking.take()
                });
                if let Some(mut w) = walker {
                    w.init(world, to.0 as i32, 0);
                    if let Some(v) = get_mut(world, id) {
                        v.walking = Some(w);
                    }
                }
            }
            logic::task::IDLE => {}
            0x24 => {
                if let Some(e) = world.entities.by_index_mut(id as u32) {
                    e.flags &= !flag::VISIBLE;
                }
                super::delete(world, id);
                return;
            }
            _ => logic::idle(world, id),
        }
    } else {
        logic::idle(world, id);
    }
    update(world, id);
}

/// 0x590a90: a shipyard (building type 0x1f) on one of the ship's six neighbours.
pub(super) fn shipyard_near(world: &World, id: u16) -> bool {
    let Some(pos) = world.entities.by_index(id as u32).map(|e| e.pos) else { return false };
    NEIGHBOR.iter().any(|&(dx, dy)| {
        world.grid.checked_index(pos.x() + dx, pos.y() + dy).is_some_and(|n| {
            let b = world.layers.map_object[n];
            b != 0 && world.entities.by_index(b as u32).is_some_and(|e| e.obj_type == ObjType::Building && e.kind == 0x1f)
        })
    })
}

/// `Vehicle/ShipRepairRate` (static config var 0x11cdbf8, set by 0x424f60).
const REPAIR_RATE: u8 = 1;

/// The ships' update (vtbl+0x90): ferries, transport ships and war ships have their own
/// (0x58d2e0 `ferry::update`, 0x591c30 `transport::update`, 0x592530 `warship::update`); this
/// base (`CShip` 0x590710's moving part) runs only for a ship without class data.
pub fn update(world: &mut World, id: u16) {
    if super::ferry::is_ferry(world, id) {
        super::ferry::update(world, id);
        return;
    }
    if super::transport::is_transport(world, id) {
        super::transport::update(world, id);
        return;
    }
    if super::warship::is_warship(world, id) {
        super::warship::update(world, id);
        return;
    }
    repair(world, id);
    let Some(pos) = world.entities.by_index(id as u32).map(|e| e.pos) else { return };
    match get(world, id).map_or(logic::task::IDLE, |v| v.task) {
        logic::task::GO => walk(world, id, pos, true),
        logic::task::IDLE => walk(world, id, pos, false),
        0x1b => logic::next_task(world, id),
        _ => {}
    }
}

/// The ships' repair: every 21 ticks (+0x94 the tick) by `ShipRepairRate` next to a shipyard,
/// below the info's hit points.
pub(crate) fn repair(world: &mut World, id: u16) {
    let now = world.tick;
    let last = get(world, id).map_or(0, |v| v.x9c as u32);
    if last.wrapping_add(0x14) < now {
        if shipyard_near(world, id) {
            let Some(e) = world.entities.by_index(id as u32) else { return };
            let max = world.rules.vehicles.get(e.race, e.kind as u8).hitpoints;
            if let Some(v) = get_mut(world, id)
                && (v.health as i32) < max
            {
                v.health = v.health.wrapping_add(REPAIR_RATE);
            }
        }
        if let Some(v) = get_mut(world, id) {
            v.x9c = now as i32;
        }
    }
}

/// Walking (`go`: the walker's `Walk(pos, 1)`, the step checked) or standing (`IdleWalk(pos, 0,
/// 0)`, unchecked), then the step 0x5950d0.
pub(crate) fn walk(world: &mut World, id: u16, pos: Y16X16, go: bool) {
    let Some(mut w) = get_mut(world, id).and_then(|v| v.walking.take()) else { return };
    let s = if go { crate::pathing::walking::walk(world, &mut w, pos) } else { crate::pathing::walking::idle_walk(world, &mut w, pos.0 as i32, 0) };
    if let Some(v) = get_mut(world, id) {
        v.walking = Some(w);
    }
    logic::step(world, id, s, go);
}

/// vtbl+0xd8 (0x5908d0) `(step)`: one node in the step's direction if the walker finds it open
/// and free and the cell slot and marks can move there; the map-object layer follows, a turn,
/// the war map. Only a failed slot / mark move marks the step blocked (| 0x88); a node that is
/// not open or taken leaves the step as it is (the original).
pub(crate) fn move_step(world: &mut World, id: u16, s: u32) {
    let mut s = s;
    let d = (s & 0xf) as usize;
    if d < 6 {
        let Some(pos) = world.entities.by_index(id as u32).map(|e| e.pos) else { return };
        let to = pos.0.wrapping_add(MODIFIER[d] as u32) as i32;
        let n = index(world, to);
        let open = water(world, n) & 0xc300 == WATER && water(world, n) & SHIP_HERE == 0;
        if !open {
            // Blocked or taken: the step stays as it is (no 0x88).
        } else if move_mark(world, to, pos.0 as i32) {
            if let Some(m) = world.grid.checked_index(pos.x(), pos.y()) {
                world.layers.map_object[m] = 0;
            }
            if let Ok(m) = usize::try_from(n)
                && m < world.layers.map_object.len()
            {
                world.layers.map_object[m] = id;
            }
            if get(world, id).is_some_and(|v| d as i8 != v.dir) {
                logic::turn_to(world, id, d as i8);
            }
            if let Some(e) = world.entities.by_index_mut(id as u32) {
                e.pos = Y16X16(to as u32);
            }
            if let Some(v) = get_mut(world, id) {
                v.distance = 0xff;
            }
            crate::warmap::moved(world, id);
        } else {
            s |= 0x88;
        }
    }
    if let Some(v) = get_mut(world, id) {
        v.b5c = s as u8;
    }
}

/// `CWalkingShip::IsNotBlocked` (0x5ba7d0): open water.
pub fn not_blocked(world: &World, n: Option<usize>) -> bool {
    n.is_some_and(|n| world.tiling.water_flags[n] & 0xc300 == WATER)
}

/// `CWalkingShip::IsNotOccupied` (0x5ba800): no ship on the node.
pub fn not_occupied(world: &World, n: Option<usize>) -> bool {
    n.is_some_and(|n| world.tiling.water_flags[n] & SHIP_HERE == 0)
}

/// `CWalkingShip::FindPathAStar64` (0x5ba820) around the base search: the ship's own mark at
/// `from` is lifted, and (on open water with a slot) set again afterwards.
pub fn with_own_mark_lifted<R>(world: &mut World, from: i32, f: impl FnOnce(&mut World) -> R) -> R {
    lift(world, from);
    let r = f(world);
    place(world, from);
    r
}

/// `Walk`'s ship branch (0x5b96c6) for the fine step `dir` onto `s.fine`: not open water →
/// failed (state 1, 0x48); another ship's closeness `c` (bits 4..7) at the node: any → search
/// again next time (state 9); up to 2 → the step; within 3 of the goal → arrived (state 3,
/// 0x28); else wait (+0x38 counts), stepping anyway from the second wait on when `c` ≤ 7, else
/// no step (0x88).
pub fn walk_step(world: &World, s: &mut WalkState, cur: i32, dir: u32) -> u32 {
    let n = world.grid.checked_index(s.fine & 0xffff, s.fine >> 16);
    if !not_blocked(world, n) {
        s.state = 1;
        return 0x48;
    }
    let c = n.map_or(0, |n| (world.tiling.water_flags[n] >> 4) & 0xf) as u32;
    if c != 0 {
        s.state = 9;
    }
    if c <= 2 {
        s.ship_wait = 0;
        return dir;
    }
    if hexdist(cur, s.walk_to) <= 3 {
        s.state = 3;
        return 0x28;
    }
    s.ship_wait += 1;
    if s.ship_wait >= 2 && c <= 7 { dir } else { 0x88 }
}

/// `CWalkingShip::IdleWalk` (0x5ba8a0) `(cur)`. The six neighbours' values: another ship's
/// closeness (bits 4..7) on open water, 11 elsewhere (water flags & 0x300). Some neighbour
/// closer to other ships than another: drift. Standing in a shipyard's water (0x1000) with an
/// open side: head for the first node of the 15-ring table in the same sector, outside shipyard
/// water and at least 5 from the nearest ship (+0x1c, re-chosen every time: the kept-target test
/// reads the current node's 0x1000 and so always fails), one step towards it if open (flag
/// 0x20000). Else, drifting or leaving: a random direction among the least crowded neighbours,
/// weighted 1 (+1 when the next neighbour has no other ship, +1 while every earlier one was least
/// crowded and so is the last). 8 = no step.
pub fn idle_walk(world: &mut World, s: &mut WalkState, cur: i32) -> u32 {
    let side = world.grid.size() as i64;
    let cn = index(world, cur);
    let mut v = [0u32; 6];
    let (mut lo, mut hi, mut taken, mut solid) = (10u32, 0u32, 0u32, 0u32);
    for (i, &(dx, dy)) in NEIGHBOR.iter().enumerate() {
        let wf = water(world, cn + dy as i64 * side + dx as i64) as u32;
        if wf & 0x300 != 0 {
            v[i] = 0xb;
            solid += 1;
            continue;
        }
        v[i] = (wf >> 4) & 0xf;
        taken += (wf >> 11) & 1;
        lo = lo.min(v[i]);
        if v[i] != 0 {
            hi = hi.max(v[i]);
        }
    }
    let drift = lo < hi && hi != 0;
    let mut leave = false;
    if drift {
        s.idle_target = -1;
    } else {
        leave = water(world, cn) & SHIPYARD != 0 && taken + solid < 6;
        if !leave {
            s.idle_target = -1;
        } else {
            let (x, y) = (cur & 0xffff, cur >> 16);
            let here = sector(world, usize::try_from(cn).ok().filter(|&n| n < world.grid.len()));
            if s.flags & crate::pathing::walking::walk_flag::IDLE_KEEP == 0 {
                s.idle_target = -1;
            }
            s.flags &= !crate::pathing::walking::walk_flag::IDLE_KEEP;
            // The kept target is dropped: its test reads the current node, which is in a
            // shipyard's water here.
            s.idle_target = -1;
            let size = world.grid.size() as i32;
            let found = hex(15).iter().find_map(|&(dx, dy)| {
                let (nx, ny) = (x + dx, y + dy);
                if (nx as u32) >= size as u32 || (ny as u32) >= size as u32 {
                    return None;
                }
                let n = ny as usize * size as usize + nx as usize;
                if sector(world, Some(n)) != here {
                    return None;
                }
                let wf = world.tiling.water_flags[n];
                let near = wf & 0xf;
                (wf & SHIPYARD == 0 && (near == 0 || near == 11 || 10 - (near as i32) > 4)).then_some(ny << 16 | nx)
            });
            let Some(t) = found else { return 8 };
            s.idle_target = t;
            let (dx, dy) = ((t & 0xffff) - x, (t >> 16) - y);
            let i = (dx <= 2 * dy) as usize | if dx < -dy { 4 } else { 0 } | if 2 * dx <= dy { 2 } else { 0 };
            let dir = DIRECTION[i];
            let (ndx, ndy) = NEIGHBOR[dir];
            let next = (y + ndy) as i64 * side + (x + ndx) as i64;
            if not_blocked(world, usize::try_from(next).ok().filter(|&n| n < world.grid.len())) {
                s.flags |= crate::pathing::walking::walk_flag::IDLE_KEEP;
                return dir as u32;
            }
        }
    }
    if leave || drift {
        let mut cum = [-1i64; 6];
        let mut total = 0u32;
        let mut bonus = (v[5] == lo) as u32;
        for i in 0..6 {
            if v[i] == lo {
                total += 2 - (v[(i + 1) % 6] != 0) as u32 + bonus;
                cum[i] = total as i64;
            } else {
                bonus = 0;
            }
        }
        let r = (world.rng.rand() % total.max(1)) as i64;
        if let Some(i) = cum.iter().position(|&c| r < c) {
            return i as u32;
        }
    }
    s.idle_target = -1;
    8
}

/// `CWalkingBase::SectorId` (0x5b8d10): the sector of the node's tile.
pub fn sector(world: &World, n: Option<usize>) -> i32 {
    let t = n.and_then(|n| world.tiling.normal.get(n).copied()).unwrap_or(0);
    world.tiling.tiles.get(t as usize).map_or(0, |t| t.sector as i32)
}

/// Ships of `player` (any type) within the vehicles' lists.
pub fn ships(world: &World, player: u8) -> impl Iterator<Item = u16> + '_ {
    [kind::WARSHIP, kind::FERRY, kind::TRANSPORT_SHIP].into_iter().flat_map(move |k| super::iter(world, player, k))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closeness_keeps_the_two_nearest() {
        let at = 0x0010_0010;
        let ships = [0x0010_0013, 0x0010_0011, 0x0010_0020];
        assert_eq!(closeness(&ships, at), (10 - 3) << 4 | (10 - 1));
        assert_eq!(closeness(&[], at), 0);
    }

    #[test]
    fn cell_of_packs() {
        assert_eq!(cell_of(0x0025_0013), (1, 2));
    }
}
