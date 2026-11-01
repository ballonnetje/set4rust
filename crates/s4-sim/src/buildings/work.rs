//! What the worker-driving building roles (`gather`, `farm`, `workup`, `production`) share: the
//! `IBuildingRole` members they use (state, delay counter, inhabitant, the work target and the
//! working-area search cursors), the buildingInfo fields they read, and
//! `IBuildingRole::SearchInWorkingArea`.

use super::search;
use crate::hash::StateHasher;
use crate::rules::{BUILDING_MAX, RACES};
use crate::world::World;
use s4_formats::gamedata::GameData;

/// The buildingInfo.xml values the worker roles read (`CBuildingInfoMgr::GetBuildingInfo`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct WorkInfo {
    /// +478 `inhabitant` settler type (0 = none).
    pub inhabitant: u16,
    /// +480 `productionDelay`: `LogicUpdate`s of 14 ticks between two work trips.
    pub production_delay: u8,
    /// +492 `workingAreaRadius`.
    pub working_radius: i32,
    /// +57 pile count, records of 16 bytes from +60 (`AddPile` arguments).
    pub piles: Vec<WorkPile>,
    /// `vAnimListId`: the inhabitant's work lists the building orders (event 3).
    pub anim_lists: Vec<u16>,
}

/// One buildingInfo pile record (+60 + 16·i).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct WorkPile {
    /// +60/+61 cell offset from the building.
    pub x: i8,
    pub y: i8,
    /// +62 good.
    pub good: u8,
    /// +63 type: the `IPileRole` (0 production, 1 delivery).
    pub kind: u8,
    /// +64 appearance (`AddPile` variant), +65 patch (1 = drawn by the building, pile hidden).
    pub appearance: u8,
    pub patch: u8,
    /// +68/+72 pixel offset.
    pub pixel: (i16, i16),
}

/// `[race * BUILDING_MAX + type]`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct WorkRules {
    infos: Vec<WorkInfo>,
}

impl WorkRules {
    pub fn new(data: &GameData) -> WorkRules {
        let mut infos = vec![WorkInfo::default(); RACES * BUILDING_MAX];
        for b in &data.buildings {
            let (r, t) = (b.race as usize, b.building as usize);
            if r >= RACES || t >= BUILDING_MAX {
                continue;
            }
            infos[r * BUILDING_MAX + t] = WorkInfo {
                inhabitant: b.inhabitant.map_or(0, |s| s as u16),
                production_delay: b.production_delay,
                working_radius: b.working_area_radius,
                piles: b
                    .piles
                    .iter()
                    .map(|p| WorkPile {
                        x: p.x as i8,
                        y: p.y as i8,
                        good: p.good.map_or(0, |g| g as u8),
                        kind: p.kind.clamp(0, 255) as u8,
                        appearance: p.appearance.clamp(0, 255) as u8,
                        patch: p.patch.clamp(0, 255) as u8,
                        pixel: (p.x_pixel as i16, p.y_pixel as i16),
                    })
                    .collect(),
                anim_lists: b.anim_lists.clone(),
            };
        }
        WorkRules { infos }
    }

    pub fn info(&self, race: u8, building: u8) -> Option<&WorkInfo> {
        self.infos.get(race as usize * BUILDING_MAX + building as usize)
    }
}

/// The `IBuildingRole` members the worker roles use (besides the inhabitant and +29, which are
/// in [`super::Base`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct WorkBase {
    /// +4: the role's state.
    pub state: u8,
    /// +5: production delay counter.
    pub delay: u8,
    /// +12: entity to work on, found by the search (0 = a position only).
    pub target_entity: u16,
    /// +16: where the worker goes (packed xy, 0 = nothing found).
    pub target: u32,
    /// +20 + 4·i: per search `i` the two spiral cursors (near, far).
    pub cursors: [[u16; 2]; 2],
    /// +28: "nothing found" warning armed.
    pub warned: bool,
    /// +32: the search routine (a `SEARCH_*` type), set by the role's `Init` from one of the
    /// inhabitant's searches (gather: the first, simple: the second).
    pub routine: u16,
}

impl WorkBase {
    pub(crate) fn hash(&self, h: &mut StateHasher) {
        h.u8(self.state);
        h.u8(self.delay);
        h.u16(self.target_entity);
        h.u32(self.target);
        for c in self.cursors {
            h.u16s(&c);
        }
        h.bool(self.warned);
        h.u16(self.routine);
    }
}

/// `IBuildingRole::SearchInWorkingArea(building, i)` (release 0x540490): look for work with the
/// inhabitant's search `i`, 75 spiral entries from the near cursor and 50 from the far one,
/// around the working-area centre. A hit stores the entity and the worker's position (the node +
/// the search offset, which must lie in the building's eco-sector). The cursors make the search
/// restart from the centre after a near hit and continue outwards otherwise.
pub fn search_in_working_area(world: &mut World, building: u16, base: &mut WorkBase, i: usize) -> bool {
    let Some(e) = world.entities.by_index(building as u32) else { return false };
    let (race, owner) = (e.race, e.owner);
    let crate::entity::EntityData::Building(d) = &e.data else { return false };
    let (type_ex, centre, ensign) = (d.type_ex, d.working_area, d.ensign);
    let Some(info) = world.rules.work.info(race, type_ex) else { return false };
    let (inhabitant, radius) = (info.inhabitant, info.working_radius);
    let _ = owner; // IAIDifficultyLevels::GetDifficultyLevel(owner): result unused.
    // The routine is `this + 32` (set by `Init`); the offset is search i's.
    let kind = base.routine;
    let (dx, dy) = search::offset(world.rules.jobs.search(race, inhabitant, i).unwrap_or(0));
    // "workingAreaRadius < SPIRAL_RADIUS_MAX" asserted.
    let last = crate::spiral::last(radius.clamp(0, 255) as u8) as i32;
    let eco_at = |w: &World, x: i32, y: i32| w.grid.checked_index(x, y).map(|n| w.tiling.eco_sector_of(n));
    let eco = eco_at(world, ensign.x(), ensign.y());
    let mut found: Option<(usize, i32)> = None;
    if search::is_ported(kind) {
        let c = &mut base.cursors[i];
        if c[1] < c[0] + 75 {
            c[1] = c[0] + 75;
        }
        for pass in 0..2 {
            let start = base.cursors[i][pass] as i32;
            let mut len = if pass == 0 { 75 } else { 50 };
            if len + start >= last {
                len = last - start;
            }
            let mut j = 0;
            while j < len {
                let k = (j + start) as usize;
                let (x, y) = (centre.x() + crate::spiral::dx(k), centre.y() + crate::spiral::dy(k));
                if world.grid.in_world(x, y) && eco_at(world, x + dx, y + dy) == eco {
                    let r = search::run(world, kind, x, y);
                    if r != 0 {
                        base.target = crate::grid::Y16X16::pack(x + dx, y + dy).0;
                        base.target_entity = if r > 0 { r as u16 } else { 0 };
                        found = Some((pass, j + start));
                        break;
                    }
                }
                j += 1;
            }
            base.cursors[i][pass] = (base.cursors[i][pass] as i32 + j.max(0)) as u16;
            if found.is_some() {
                break;
            }
        }
    }
    match found {
        None => {
            let wrapped = base.cursors[0][0] as i32 >= last;
            let c = &mut base.cursors[i];
            if (c[0] as i32) < last {
                if c[1] as i32 >= last {
                    c[1] = 0;
                }
            } else {
                *c = [0, 0];
            }
            if wrapped {
                // Armed twice in a row: the "no more resources" message (0x54076d): woodcutter
                // GUI_WARN_WOODCUT_NOWOOD, stonecutter GUI_WARN_STONECUT_NOSTONE, fisher
                // GUI_WARN_FISHERHUT_NOFISH.
                if base.warned
                    && let Some(b) = world.entities.by_index(building as u32)
                    && let Some(text) = match b.kind {
                        1 => Some(0x9fa),
                        4 => Some(0x9ef),
                        6 => Some(0xefd),
                        _ => None,
                    }
                {
                    let ev = crate::event::SimEvent::warning(b.owner, text, b.pos);
                    world.events.push(ev);
                }
                base.warned = !base.warned;
            }
            false
        }
        Some((pass, k)) => {
            base.warned = false;
            if pass == 0 || k <= 150 || base.cursors[i][0] >= 150 {
                base.cursors[i][0] = 0;
            } else {
                base.cursors[i][1] = k as u16;
            }
            if i == 1 {
                base.cursors[0] = [0, 0];
            }
            true
        }
    }
}

/// The inhabitant's search `i` of building `id` (a `SEARCH_*` type), for a role's `Init`.
pub fn inhabitant_search(world: &World, id: u16, i: usize) -> u16 {
    let Some(e) = world.entities.by_index(id as u32) else { return 0 };
    let crate::entity::EntityData::Building(d) = &e.data else { return 0 };
    let inhabitant = world.rules.work.info(e.race, d.type_ex).map_or(0, |i| i.inhabitant);
    world.rules.jobs.search(e.race, inhabitant, i).unwrap_or(0)
}

/// `IBuildingRole::OrderInhabitant` (release 0x53ff40): true when the building has its inhabitant or
/// the eco-sector at its flag sent one (`CEcoSector::OrderWorker`); otherwise look again in 31
/// ticks (without an eco-sector the original just returns).
pub fn order_inhabitant(world: &mut World, id: u16) -> bool {
    if super::role(world, id).is_some_and(|r| r.base.inhabitant != 0) {
        return true;
    }
    let Some(e) = world.entities.by_index(id as u32) else { return false };
    let crate::entity::EntityData::Building(d) = &e.data else { return false };
    let (race, type_ex, ensign) = (e.race, d.type_ex, d.ensign);
    let eco = world.grid.checked_index(ensign.x(), ensign.y()).map_or(0, |n| world.tiling.eco_sector_of(n));
    if eco == 0 {
        return false;
    }
    let kind = world.rules.work.info(race, type_ex).map_or(0, |i| i.inhabitant);
    let settler = crate::eco::order_worker(world, eco, id, kind);
    if let Some(r) = super::role_mut(world, id) {
        r.base.inhabitant = settler;
    }
    if settler != 0 {
        return true;
    }
    world.register_for_logic_update(id, 31);
    false
}

/// buildingInfo `productionDelay` of building `id`.
pub fn production_delay(world: &World, id: u16) -> u8 {
    let Some(e) = world.entities.by_index(id as u32) else { return 0 };
    let crate::entity::EntityData::Building(d) = &e.data else { return 0 };
    world.rules.work.info(e.race, d.type_ex).map_or(0, |i| i.production_delay)
}

/// `IBuildingRole::Switch` (release 0x53ffd0, vtbl+32; command 5003): the building stops or starts
/// working. Flag 0x1000 is toggled and the building's observers (its piles and workers) get
/// event 8 when it starts, 7 when it stops (`CBuilding` vtbl+0x7c, data0 = the building).
pub fn switch(world: &mut World, id: u16) {
    const FLAG_WORKING: u32 = 0x1000;
    let Some(e) = world.entities.by_index_mut(id as u32) else { return };
    let start = e.flags & FLAG_WORKING == 0;
    e.flags ^= FLAG_WORKING;
    let event = if start { 8 } else { 7 };
    super::notify(world, id, crate::jobs::state::EntityEvent { event, kind: 0, data: [id as i32, 0, 0] });
}
