//! `CDarkMushroomFarmRole` (house kind 20, release vtable 0x1058ae8): the Dark Tribe's mushroom
//! farm. Decompiles in `tools/re/decompiled/script/dark/mushfarm.c`, `mushfarm2.c`,
//! `pull_effect.c`.
//!
//! The farm needs dark land at its door. Its farmer comes from the temple
//! ([`crate::dark::order_mushroom_farmer`]) and plants mushrooms on dark land in the working area.
//! The farm itself "pulls" from ripe mushrooms there (they stay): each pull plays patch 0 (OPEN)
//! until the pull effect has flown to the farm, then patch 1 (FLOW), and counts a sphere. When
//! the spheres reach the frame count of patch 2 (GROW, which shows them), the farmer is set free
//! and goes to the temple, where he brings the player mana ([`crate::magic::add_sphere`]); the
//! farm then orders a new farmer.
//!
//! States (+4): 1 order a farmer, 3 plant / look for a pull, 4 OPEN until the pull arrives, 5 FLOW.
//! Slaves (+0x18c, [`crate::dark::slave_to_farm`]) make the pull search look at more nodes.
//!
//! [`destroyed`] is vtbl+0x2c (0x545050). Not ported: `PostLoadInit` (0x544650, savegames). The patch display (FillGfxInfo 0x544ac0: GROW
//! shows frame `spheres − 1`) is the client's; [`State::patch`] says which patch plays.

use super::{Role, RoleKind};
use crate::World;
use crate::entity::{EntityData, ObjType, flag};
use crate::grid::Y16X16;
use crate::hash::StateHasher;
use crate::jobs::state::EntityEvent;

/// vtbl+0x2c (0x545050, from `CrushBuilding` and the destroy path 0x536a80): a farm with a cause
/// (the building byte +0x25, the destroying player) counts one more for that player's
/// `Statistic.MushroomFarmsDestroyed`.
pub fn destroyed(world: &mut World, cause: u8) {
    if cause != 0
        && let Some(p) = world.statistic.players.get_mut(cause as usize)
    {
        p.mushroom_farms_destroyed += 1;
    }
}

/// `CDarkMushroomFarmRole` +4 .. +0x198.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct State {
    /// +4.
    pub state: u8,
    /// +0x10: the node the farmer was sent to (he plants at it minus his search offset).
    pub target: u32,
    /// +0x17c: frames of patch 2 (GROW), the spheres a farmer collects.
    pub grow_frames: i32,
    /// +0x180 / +0x184: spiral cursors of the pull and the plant search.
    pub pull_cursor: i32,
    pub plant_cursor: i32,
    /// +0x188: spheres pulled for the present farmer.
    pub spheres: i32,
    /// +0x18c: slaves sent here.
    pub slaves: i32,
    /// +0x190: updates since the slaves were last told (event 5 every 8th).
    pub notify: i32,
    /// +0x194: tick of the last pull; +0x198: tick the pull effect reaches the farm.
    pub last_pull: i32,
    pub pull_end: u32,
    /// Patch playing (client): 0 none, 1 OPEN (slot 0), 2 FLOW (slot 1); from tick `patch_tick`.
    pub patch: u8,
    pub patch_tick: u32,
    /// The node of the last pull (client: the flight effect 0x15 from it to the farm).
    pub pull_from: u32,
}

impl State {
    pub(crate) fn hash(&self, h: &mut StateHasher) {
        h.u8(self.state);
        h.u32(self.target);
        for v in [self.grow_frames, self.pull_cursor, self.plant_cursor, self.spheres, self.slaves, self.notify, self.last_pull] {
            h.u32(v as u32);
        }
        h.u32(self.pull_end);
        h.u8(self.patch);
        h.u32(self.patch_tick);
        h.u32(self.pull_from);
    }
}

fn role(world: &mut World, id: u16) -> Option<(&mut super::Base, &mut State)> {
    match world.buildings.roles.get_mut(&id)? {
        Role { base, kind: RoleKind::MushroomFarm(s) } => Some((base, s)),
        _ => None,
    }
}

pub fn get(world: &World, id: u16) -> Option<&State> {
    match world.buildings.roles.get(&id)? {
        Role { kind: RoleKind::MushroomFarm(s), .. } => Some(s),
        _ => None,
    }
}

/// +0x18c of farm `id` (0 for anything else).
pub fn slaves(world: &World, id: u16) -> i32 {
    get(world, id).map_or(0, |s| s.slaves)
}

/// 0x4cb610: one more slave for farm `id`.
pub(crate) fn add_slave(world: &mut World, id: u16) {
    if let Some((_, s)) = role(world, id) {
        s.slaves += 1;
    }
}

struct Info {
    race: u8,
    pos: Y16X16,
    door: Y16X16,
    ensign: Y16X16,
    area: Y16X16,
    inhabitant: u16,
    radius: i32,
}

fn info(world: &World, id: u16) -> Option<Info> {
    let e = world.entities.by_index(id as u32)?;
    let EntityData::Building(d) = &e.data else { return None };
    let w = world.rules.work.info(e.race, d.type_ex);
    Some(Info {
        race: e.race,
        pos: e.pos,
        door: d.door,
        ensign: d.ensign,
        area: d.working_area,
        inhabitant: w.map_or(0, |w| w.inhabitant),
        radius: w.map_or(0, |w| w.working_radius),
    })
}

fn patch_frames(world: &World, race: u8, slot: usize) -> u8 {
    world.rules.dark.farm_patch_frames.get(race as usize).map_or(0, |p| p[slot])
}

/// `CDarkMushroomFarmRole::Init` (0x5446b0), after `InitCommon`: state 1, the GROW frame count,
/// the cursors and counters cleared; first update next tick.
pub fn init(world: &mut World, id: u16) {
    let Some(i) = info(world, id) else { return };
    let grow = patch_frames(world, i.race, 2) as i32;
    if let Some((_, s)) = role(world, id) {
        *s = State { state: 1, target: s.target, grow_frames: grow, ..State::default() };
    }
    world.register_for_logic_update(id, 1);
}

/// `CDarkMushroomFarmRole::SettlerEnter` (0x544750): the farmer is the inhabitant, hidden, at
/// home. Returns false (the release returns 0).
pub fn settler_enter(world: &mut World, id: u16, settler: u16) -> bool {
    if let Some((base, _)) = role(world, id) {
        base.inhabitant = settler;
        base.worker_in_house = true;
    }
    if let Some(e) = world.entities.by_index_mut(settler as u32) {
        e.flags &= !flag::VISIBLE;
    }
    false
}

/// 0x544780: whether the farm lacks dark land — none of the door's six neighbours is dark and
/// unblocked (flags & 5 == 4), and the ensign is not dark.
fn no_dark_land(world: &World, i: &Info) -> bool {
    if let Some(n) = world.grid.checked_index(i.door.x(), i.door.y()) {
        for d in 0..6 {
            if crate::dark::neighbour(world, n, d).is_some_and(|m| world.layers.flags[m] & 5 == 4) {
                return false;
            }
        }
    }
    world.grid.checked_index(i.ensign.x(), i.ensign.y()).is_none_or(|n| world.layers.flags[n] & 4 == 0)
}

/// 0x544fa0: every 8th call the observers except the farmer (filter 0x5444b0: not a settler of
/// type 53) get event 5 (`CBuilding` vtbl+0x78).
fn notify_slaves(world: &mut World, id: u16) {
    let Some((base, s)) = role(world, id) else { return };
    s.notify += 1;
    if s.notify <= 7 {
        return;
    }
    s.notify = 0;
    let list = base.observers.clone();
    let ev = EntityEvent { event: 5, kind: 0, data: [0, 0, 0] };
    for (e, _) in list {
        match world.entities.by_index(e as u32).map(|x| (x.obj_type, x.kind)) {
            Some((ObjType::Settler, crate::dark::MUSHROOM_FARMER)) => {}
            Some((ObjType::Settler, _)) => crate::jobs::core::set_event(world, e, ev),
            Some((ObjType::Pile, _)) => crate::pile::set_event(world, e, ev),
            _ => {}
        }
    }
}

/// 0x544cb0: send the farmer out to plant. From the plant cursor, 10 spiral entries around the
/// working area are tried, each only when a game-RNG draw passes (`Rand() >= 0x4000`): a node
/// on the working area's sector where the farmer's search 1 (`SEARCH_MUSHROOM_SEED_POS`) finds
/// room at node − its offset. The mushroom type for the ground there (`GetObjectType`) gets its
/// flags at once; the farmer stands at the door, visible, and gets event 6 (type, node).
fn plant(world: &mut World, id: u16, i: &Info) -> bool {
    let Some((base, s)) = role(world, id) else { return false };
    let (farmer, start) = (base.inhabitant, s.plant_cursor);
    let Some(f) = world.entities.by_index(farmer as u32) else { return false };
    let (frace, fkind) = (f.race, f.kind);
    let Some(t) = world.rules.jobs.search(frace, fkind, 1) else { return false };
    if !crate::buildings::search::is_ported(t) {
        return false;
    }
    let (dx, dy) = crate::buildings::search::offset(t);
    let last = crate::spiral::last(i.radius.clamp(0, 255) as u8) as i32;
    let area_sector = world.grid.checked_index(i.area.x(), i.area.y()).map_or(0, |n| world.tiling.sector_of(n));
    loop {
        let cursor = role(world, id).map_or(0, |(_, s)| s.plant_cursor);
        if cursor >= start + 10 {
            return false;
        }
        if world.rng.rand() < 0x4000 {
            continue;
        }
        let (x, y) = (i.area.x() + crate::spiral::dx(cursor as usize), i.area.y() + crate::spiral::dy(cursor as usize));
        let ok = world.grid.in_world(x, y)
            && world.tiling.sector_of(world.grid.index(x as u32, y as u32)) == area_sector
            && crate::buildings::search::run(world, t, x - dx, y - dy) != 0;
        let Some((_, s)) = role(world, id) else { return false };
        s.plant_cursor += 1;
        if last < s.plant_cursor {
            s.plant_cursor = 0;
            if !ok {
                return false;
            }
        }
        if !ok {
            continue;
        }
        let target = Y16X16::pack(x, y);
        s.target = target.0;
        let (px, py) = (x - dx, y - dy);
        let ground = world.grid.checked_index(px, py).map_or(0, |n| world.layers.ground[n]);
        let other = world.rules.jobs.other(frace, fkind);
        let kind = crate::deco::get_object_type(world, other, frace, ground);
        crate::deco::set_flags_for_object(world, px, py, kind);
        if let Some(e) = world.entities.by_index_mut(farmer as u32) {
            e.pos = i.door;
            e.flags |= flag::VISIBLE;
        }
        if let Some(n) = world.grid.checked_index(i.door.x(), i.door.y()) {
            world.layers.map_object[n] = farmer;
        }
        crate::jobs::core::set_event(world, farmer, EntityEvent { event: 6, kind: 0, data: [0, kind as i32, target.0 as i32] });
        if let Some((base, _)) = role(world, id) {
            base.worker_in_house = false;
        }
        return true;
    }
}

/// 0x544b40: look for a ripe mushroom to pull from — `slaves + 1` spiral entries from the pull
/// cursor around the working area, with the inhabitant type's search 0 (`SEARCH_MUSHROOM`; the
/// mushroom stays). A hit sets the arrival tick of the pull effect ([`pull_duration`]).
fn pull(world: &mut World, id: u16, i: &Info) -> bool {
    let Some((_, s)) = role(world, id) else { return false };
    let (start, slaves) = (s.pull_cursor, s.slaves);
    let Some(t) = world.rules.jobs.search(i.race, i.inhabitant, 0) else { return false };
    if !crate::buildings::search::is_ported(t) {
        return false;
    }
    let last = crate::spiral::last(i.radius.clamp(0, 255) as u8) as i32;
    let mut found = None;
    loop {
        let cursor = role(world, id).map_or(0, |(_, s)| s.pull_cursor);
        if cursor > start + slaves {
            break;
        }
        let (x, y) = (i.area.x() + crate::spiral::dx(cursor as usize), i.area.y() + crate::spiral::dy(cursor as usize));
        let hit = world.grid.in_world(x, y) && crate::buildings::search::run(world, t, x, y) != 0;
        let Some((_, s)) = role(world, id) else { return false };
        s.pull_cursor += 1;
        let wrapped = last < s.pull_cursor;
        if wrapped {
            s.pull_cursor = 0;
        }
        if hit {
            found = Some((x, y));
            break;
        }
        if wrapped {
            break;
        }
    }
    let Some((x, y)) = found else { return false };
    let end = world.tick.wrapping_add(pull_duration(world, x, y, i.pos) as u32);
    world.events.push(crate::event::SimEvent::Effect { effect: 0x14, sound: 0, x, y });
    if let Some((_, s)) = role(world, id) {
        s.pull_end = end;
        s.pull_from = Y16X16::pack(x, y).0;
    }
    true
}

/// 0x65b130 in whole pixels and its height term ([`crate::projectile::screen`]).
pub use crate::projectile::{height_px, screen};

/// `CEffects` vtbl+0x1c (0x4e3b90) from the mushroom at (x, y) to the farm at `to`: effect 0x14
/// at the mushroom (vtbl+0x10, lasting `GetJobFrameCount(6, 0x14, 0)` ticks), then the flight
/// effect 0x15 (0x4e25c0), which starts when the first ends and takes `max(1, (int)(√D · 0.2))`
/// ticks for the squared screen distance `D` between the mushroom − (0, 18) and the farm −
/// (48, 90); that is `isqrt(D / 25)` (exact while D stays far below 2^24, as it does inside a
/// working area). Returns both durations. [inf] The effect table always has a free slot
/// (without one the release returns 1).
pub fn pull_duration(world: &World, x: i32, y: i32, to: Y16X16) -> i32 {
    let first = world.rules.dark.pull_frames.max(1) as i32;
    let (mx, my) = screen(world, x, y);
    let (fx, fy) = screen(world, to.x(), to.y());
    let a = (fy - 90 - (my - 18)) as i64;
    let b = (fx - 48 - mx) as i64;
    let steps = ((a * a + b * b) / 25).isqrt() as i32;
    first + steps.max(1)
}

/// The farmer is done (LogicUpdate 0x544980): set free (role vtbl+0x40 with −1) and sent to the
/// temple with `full`; the farm orders the next one.
fn release_farmer(world: &mut World, id: u16, full: bool) {
    let farmer = role(world, id).map_or(0, |(b, _)| b.inhabitant);
    crate::jobs::roles::set_free(world, farmer, -1);
    crate::dark::send_to_temple(world, farmer, full);
    if let Some((base, s)) = role(world, id) {
        s.state = 1;
        base.inhabitant = 0;
        base.worker_in_house = false;
    }
    world.register_for_logic_update(id, 1);
}

fn start_patch(world: &mut World, id: u16, patch: u8) {
    let tick = world.tick;
    if let Some((_, s)) = role(world, id) {
        s.patch = patch;
        s.patch_tick = tick;
    }
}

/// `CDarkMushroomFarmRole::LogicUpdate` (0x5447f0). (A selected farm first calls vtbl+0x58,
/// 0x544b30, which does nothing.)
pub fn logic_update(world: &mut World, id: u16) {
    let Some(i) = info(world, id) else { return };
    let Some((base, s)) = role(world, id) else { return };
    let (state, home) = (s.state, base.worker_in_house);
    let now = world.tick;
    match state {
        1 => {
            if no_dark_land(world, &i) || crate::dark::order_mushroom_farmer(world, id) < 1 {
                world.register_for_logic_update(id, 31);
                return;
            }
            if let Some((_, s)) = role(world, id) {
                s.state = 3;
            }
        }
        3 => {
            notify_slaves(world, id);
            if home {
                let (spheres, grow) = role(world, id).map_or((0, 0), |(_, s)| (s.spheres, s.grow_frames));
                if grow <= spheres {
                    if let Some((_, s)) = role(world, id) {
                        s.spheres = 0;
                    }
                    release_farmer(world, id, true);
                    return;
                }
                if !plant(world, id, &i) && no_dark_land(world, &i) {
                    release_farmer(world, id, false);
                    return;
                }
            }
            let Some((_, s)) = role(world, id) else { return };
            if s.spheres != s.grow_frames && s.last_pull.wrapping_add(0x118) < now as i32 && pull(world, id, &i) {
                let open = patch_frames(world, i.race, 0) as i32;
                if let Some((_, s)) = role(world, id) {
                    s.last_pull = now as i32;
                    s.state = 4;
                }
                start_patch(world, id, 1);
                world.register_for_logic_update(id, open);
                return;
            }
        }
        4 => {
            if home {
                plant(world, id, &i);
            }
            let end = role(world, id).map_or(0, |(_, s)| s.pull_end);
            if end <= world.tick {
                notify_slaves(world, id);
                let flow = patch_frames(world, i.race, 1) as i32;
                if let Some((_, s)) = role(world, id) {
                    s.state = 5;
                }
                start_patch(world, id, 2);
                world.register_for_logic_update(id, flow - 1);
            } else {
                world.register_for_logic_update(id, patch_frames(world, i.race, 0) as i32);
            }
            return;
        }
        5 => {
            if home {
                plant(world, id, &i);
            }
            notify_slaves(world, id);
            if let Some((_, s)) = role(world, id) {
                s.spheres += 1;
                s.state = 3;
            }
            start_patch(world, id, 0);
        }
        _ => return,
    }
    world.register_for_logic_update(id, 1);
}
