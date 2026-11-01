//! `CEcoManager` (vtable 0x102bc7c, 85 slots, ctor 0x44d7f0): the eco-sector AI's whole view of
//! the world and its only way to act on it. The engine calls it through [`Manager`]; [`World2`]
//! is the release's behaviour on the s4rs world. Slot inventory in docs/ECOAI_MANAGER.md.
//!
//! The manager's own data ([`State`]) is the place search: per building type a 64×64-cell
//! bitmap, a breadth-first walk over the eco-sector's cells, the accepted cells and a quadtree
//! of places (0x450fa0 and the 0x452xxx..0x457xxx helpers).

use super::places;
use crate::World;
use crate::ai::{AiEvent, EventQueue};

/// One cell record of a resource or class query (12 bytes, pushed by 0x452050 / 0x451f90).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CellValue {
    /// +0 the resource / class asked for.
    pub what: i32,
    /// +4 the value.
    pub value: u16,
    /// +6 / +8 the cell.
    pub cx: u16,
    pub cy: u16,
    /// +0xa: a cell of the own eco-sector (1) or a foreign square (0).
    pub own: u8,
}

/// The engine's interface: one method per slot the engine uses (slot offset and release
/// address in each doc). Positions are packed `y << 16 | x` where the release packs them.
pub trait Manager {
    /// 0x0c 0x44dbb0 `(t)`: drive type `t`'s place scan: 0 done with places, 1 done without, 2
    /// working.
    fn scan(&mut self, t: i32) -> i32;
    /// 0x08 0x44dc60 `(t, cx, cy)`: the 3×3 cells around `(cx, cy)` into type `t`'s scan → 2.
    fn rescan(&mut self, t: i32, cx: i32, cy: i32) -> i32;
    /// 0x10 0x44df40 `(t)`: type `t`'s scan and places cleared.
    fn clear(&mut self, t: i32) -> bool;
    /// 0x18 0x44e270 `(level, t, b1, b2, b3)`: the place near the buildings' mean cell.
    fn place_near(&mut self, level: i32, t: i32, b: [i32; 3]) -> places::Place;
    /// 0x1c 0x44e180 `(t, place, out, r)`: the places around `place`.
    fn places_around(&mut self, t: i32, place: &places::Place, out: &mut Vec<places::Place>, r: i32) -> i32;
    /// 0x20 0x44e070 `(level, t, out)`: every place of a level.
    fn places_of(&mut self, level: i32, t: i32, out: &mut Vec<places::Place>) -> i32;
    /// 0x108 0x450920 `(t, level, in, out)`: the parent or children of a place.
    fn place_family(&mut self, t: i32, level: i32, place: &places::Place, out: &mut Vec<places::Place>) -> i32;
    /// 0x14 0x44e050 `(t, place)`: a place removed.
    fn remove_place(&mut self, t: i32, place: &places::Place) -> bool;
    /// 0x24 0x44e790 `(t, x, y)`: `CheckForBuild > 0`.
    fn can_build(&mut self, t: i32, x: i32, y: i32) -> bool;
    /// 0x28 0x44e7d0 `(tA, posA, tB, posB)`: the two footprints would overlap.
    fn overlap(&mut self, ta: i32, pa: i32, tb: i32, pb: i32) -> bool;
    /// 0x2c 0x44e920 `(t, pos)`: a building of the war-map cell that `t` at `pos` would
    /// overlap, else 0.
    fn overlapping_building(&mut self, t: i32, pos: i32) -> i32;
    /// 0x30 0x44e9a0 `(x, y)`: the building on a node.
    fn building_at(&mut self, x: i32, y: i32) -> i32;
    /// 0x34 0x44e9e0 `(id)`: a live building's position.
    fn building_pos(&mut self, id: i32) -> Option<(i32, i32)>;
    /// 0x38 0x44ea30 `(id, good)`: the amount on the building's pile of `good`.
    fn pile_amount(&mut self, id: i32, good: i32) -> i32;
    /// 0x3c 0x44ea80 `(t, which)`: the race's board / stone / gold cost of `t`.
    fn cost(&mut self, t: i32, which: i32) -> i32;
    /// 0x40 0x44eb50 `(id, which)`: the site's need of boards / stone / (nothing).
    fn need(&mut self, id: i32, which: i32) -> i32;
    /// 0x44 0x44ebc0 `(id)`: build progress 0..=100.
    fn progress(&mut self, id: i32) -> i32;
    /// 0x48 0x44ec40 `(id)`: building type.
    fn kind(&mut self, id: i32) -> i32;
    /// 0x4c 0x44ebf0 `(id)`: finished and working.
    fn finished_working(&mut self, id: i32) -> bool;
    /// 0x50 0x44ec20 `(id)`: working flag.
    fn working(&mut self, id: i32) -> bool;
    /// 0x54 0x44ec60 `(id)`: the worker is home.
    fn worker_home(&mut self, id: i32) -> bool;
    /// 0x58 0x44ec80 `(t)`: beds of `t`.
    fn beds(&mut self, t: i32) -> i32;
    /// 0x5c 0x44ecb0 `(id)`: free beds of a residence.
    fn free_beds(&mut self, id: i32) -> i32;
    /// 0x60 0x44ed00 `(id)`: the eco-sector of the building's ensign.
    fn eco_of_building(&mut self, id: i32) -> i32;
    /// 0x64 0x44ed60 ConstructBuilding `(t, x, y)` → 0.
    fn construct(&mut self, t: i32, x: i32, y: i32);
    /// 0x68 0x44ee60 DestroyBuilding `(id)` → 0.
    fn destroy(&mut self, id: i32);
    /// 0x6c 0x44f0f0 `(t, x, y)`: planting nodes around.
    fn plant_nodes(&mut self, t: i32, x: i32, y: i32) -> i32;
    /// 0x74 0x44ef80 ChangeWorkingAreaTo `(id, x, y)`.
    fn set_working_area(&mut self, id: i32, x: i32, y: i32) -> bool;
    /// 0x78 0x44f0b0 `(id)`: the working area.
    fn working_area(&mut self, id: i32) -> (i32, i32);
    /// 0x7c 0x44f390 `(t)`: working radius.
    fn working_radius(&mut self, t: i32) -> i32;
    /// 0x80 / 0x84 / 0x88: player, eco-sector, race.
    fn player(&mut self) -> i32;
    fn eco(&mut self) -> i32;
    fn race(&mut self) -> i32;
    /// 0x8c 0x44f3e0 `(t)`: influence radius.
    fn influence_radius(&mut self, t: i32) -> i32;
    /// 0x90 0x44f410 `(good)`: the goods the eco-sector offers.
    fn offered(&mut self, good: i32) -> i32;
    /// 0x94 0x44f430 `(type)`: worker delta.
    fn worker_delta(&mut self, kind: i32) -> i32;
    /// 0x98 0x44f480 `(type)`: settlers of the type in the eco-sector.
    fn settlers(&mut self, kind: i32) -> i32;
    /// 0x9c 0x44f4a0 `(t)`: the player's buildings of `t` in this eco-sector.
    fn count_buildings(&mut self, t: i32) -> i32;
    /// 0xa0 0x44f530 `(t, out)`: their ids (sites included).
    fn buildings(&mut self, t: i32, out: &mut Vec<i32>) -> i32;
    /// 0xa4 0x44f640 / 0xa8 0x44f680 `(res, cx, cy)`: a cell's resource sum / 3×3 sum.
    fn cell_resource(&mut self, res: i32, cx: i32, cy: i32) -> i32;
    fn cell_resource_around(&mut self, res: i32, cx: i32, cy: i32) -> i32;
    /// 0xac 0x44f6c0 `(res, sx, sy)`: the mean resource node of a square.
    fn resource_centre(&mut self, res: i32, sx: i32, sy: i32) -> Option<(i32, i32)>;
    /// 0xb0 0x44f810 `(res, sx, sy)`: the resource sum of a square.
    fn resource_in_square(&mut self, res: i32, sx: i32, sy: i32) -> i32;
    /// 0xb4 / 0xb8 / 0xbc / 0xc0 `(res, out)`: cells with a resource (own walk / foreign
    /// squares; sum / 3×3).
    fn own_resource_cells(&mut self, res: i32, out: &mut Vec<CellValue>);
    fn own_resource_cells_around(&mut self, res: i32, out: &mut Vec<CellValue>);
    fn foreign_resource_cells(&mut self, res: i32, out: &mut Vec<CellValue>);
    fn foreign_resource_cells_around(&mut self, res: i32, out: &mut Vec<CellValue>);
    /// 0xc4 0x44fe60 `(x, y)`: a node of this eco-sector.
    fn own_node(&mut self, x: i32, y: i32) -> bool;
    /// 0xc8 0x44feb0 `(cx, cy)`: a border cell.
    fn border_cell(&mut self, cx: i32, cy: i32) -> bool;
    /// 0xcc 0x44ff30 / 0xd0 0x4500d0 `(res or class, cx, cy, r)`: in the disc.
    fn resource_near(&mut self, res: i32, cx: i32, cy: i32, r: i32) -> bool;
    fn class_near(&mut self, class: i32, cx: i32, cy: i32, r: i32) -> bool;
    /// 0xd4..0xe8 `(cx, cy)`: cell bits.
    fn cell_bit(&mut self, slot: i32, cx: i32, cy: i32) -> bool;
    /// 0xec 0x450380 `(res, x, y)`: a node's resource amount.
    fn resource_at(&mut self, res: i32, x: i32, y: i32) -> i32;
    /// 0xf0 0x4503d0 `(n, x, y)`: ground class test.
    fn ground_is(&mut self, n: i32, x: i32, y: i32) -> bool;
    /// 0xf4 0x4504b0 `(class, cx, cy)`: cell class count.
    fn cell_class(&mut self, class: i32, cx: i32, cy: i32) -> i32;
    /// 0xf8 / 0xfc `(class, out)`: cells with a class (own walk / foreign squares).
    fn own_class_cells(&mut self, class: i32, out: &mut Vec<CellValue>);
    fn foreign_class_cells(&mut self, class: i32, out: &mut Vec<CellValue>);
    /// 0x100 0x450770 `(out)`: the border cells, `cy << 16 | cx`.
    fn border_cells(&mut self, out: &mut Vec<i32>);
    /// 0x104 0x450890 `(cx, cy)`: a ground-0x6X node of the square, packed, else 0.
    fn water_node(&mut self, cx: i32, cy: i32) -> i32;
    /// 0x10c SendProduceOrder `(id, good, amount)`.
    fn produce(&mut self, id: i32, good: i32, amount: i32);
    /// 0x110 RecruteSpecialist `(id, type, amount)`.
    fn recruit(&mut self, id: i32, kind: i32, amount: i32);
    /// 0x114 SwitchBuildingStatus `(id)`.
    fn switch(&mut self, id: i32);
    /// 0x118 SendNetMessage `(id, a, b, player, c)`.
    fn net_message(&mut self, id: i32, a: i32, b: i32, player: i32, c: i32);
    /// 0x11c / 0x120 / 0x124: the player's carriers, total beds, the tick.
    fn player_carriers(&mut self) -> i32;
    fn total_beds(&mut self) -> i32;
    fn tick(&mut self) -> i32;
    /// 0x128 0x450f00 `(r)`: spiral `first(r + 1) − 1`.
    fn spiral_end(&mut self, r: i32) -> i32;
    /// 0x12c 0x450f20 `(i)`: spiral offsets.
    fn spiral_offset(&mut self, i: i32) -> (i32, i32);
    /// 0x130 0x451b80 `(id)`: a live building of this eco-sector.
    fn own_building(&mut self, id: i32) -> bool;
    /// 0x134 / 0x138 / 0x13c / 0x140: min carriers, extra carriers.
    fn min_carriers(&mut self) -> i32;
    fn set_min_carriers(&mut self, n: i32);
    fn extra_carriers(&mut self) -> i32;
    fn set_extra_carriers(&mut self, n: i32);
    /// 0x144 0x450f50 `(id, good)`: a storage's free space for `good`.
    fn storage_space(&mut self, id: i32, good: i32) -> i32;
    /// 0x150 0x451d60 + `CAIEventQueue::Get(rec, 1)` (0x436ec0): the next event.
    fn next_event(&mut self) -> Option<AiEvent>;
}

/// `CEcoManager`'s own data (+4.. of the 0x3fbc8 bytes).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct State {
    /// +4 eco-sector, +8 player.
    pub eco: u16,
    pub player: u8,
    /// +0x3fbbc / +0x3fbbd: MapSize >> 4.
    pub side: i32,
    /// The per-type place search (+0x10, +0x15c.., +0x2fc.., +0x3462c.., +0x34a10..,
    /// +0x34df4.., +0x3f3f4..).
    pub scans: places::Scans,
}

impl State {
    /// The ctor (0x44d7f0): everything zero, +4 eco-sector, +8 its owner, the cell bounds.
    pub fn new(world: &World, eco: u16, player: u8) -> State {
        let owner = world.economy.owner(eco);
        let _ = player;
        State { eco, player: owner, side: world.grid.size() as i32 >> 4, scans: places::Scans::default() }
    }
}

/// [`Manager`] on the world: the release's slots.
pub struct World2<'a> {
    pub world: &'a mut World,
    pub state: &'a mut State,
    pub queue: &'a mut EventQueue,
}

fn ent(world: &World, id: i32) -> Option<&crate::Entity> {
    if id <= 0 || id > 0xffff {
        return None;
    }
    world.entities.by_index(id as u32)
}

fn building(world: &World, id: i32) -> Option<&crate::Entity> {
    ent(world, id).filter(|e| e.obj_type == crate::entity::ObjType::Building)
}

/// Building flag 0x4000000 (DIED).
const DIED: u32 = 0x400_0000;
/// Building flag 0x1000 (working).
const WORKING: u32 = 0x1000;
/// Building flags 0x1000000 BIRTH, 0x2000000 READY.
const BIRTH: u32 = 0x100_0000;
const READY: u32 = 0x200_0000;

fn ensign(e: &crate::Entity) -> (i32, i32) {
    match &e.data {
        crate::entity::EntityData::Building(b) => (b.ensign.x(), b.ensign.y()),
        _ => (0, 0),
    }
}

impl World2<'_> {
    fn eco_at(&self, x: i32, y: i32) -> u16 {
        self.world.grid.checked_index(x, y).map_or(0, |n| self.world.tiling.eco_sector_of(n))
    }

    fn race_of(&self) -> u8 {
        self.world.race_of(self.state.player)
    }

    fn cell(&self, cx: i32, cy: i32) -> crate::animals::cells::Cell {
        self.world.animals.cells.get(cx, cy)
    }

    /// 0x451930 `(tile)`: the square's tile chain from `tile` holds a tile of this eco-sector
    /// before the first catapult tile.
    fn chain_has_eco(&self, mut tile: u16) -> bool {
        while tile != 0 {
            let t = self.world.tiling.tile(tile);
            if t.catapult() {
                return false;
            }
            if t.eco_sector == self.state.eco {
                return true;
            }
            tile = t.sq_next;
        }
        false
    }

    fn first_tile(&self, cx: i32, cy: i32) -> u16 {
        if !(0..64).contains(&cx) || !(0..64).contains(&cy) || cx >= self.state.side || cy >= self.state.side {
            return 0;
        }
        self.world.tiling.first_tile_of_square(cx as usize, cy as usize, self.state.side as usize)
    }

    /// The u16 the release reads at word `k` of cell `(cx, cy)` (0x1267bc0, 21 words a cell; a
    /// negative resource reads into the cell before: word 20 is +0x28 with the pad byte +0x29,
    /// taken as 0 [inf]).
    fn cell_word(&self, cx: i32, cy: i32, k: i32) -> i32 {
        let w = (cy * 64 + cx) * 21 + k;
        let c = self.world.animals.cells.at(w.div_euclid(21));
        let j = w.rem_euclid(21) as usize;
        let pair = |a: u8, b: u8| a as i32 | (b as i32) << 8;
        match j {
            0..=7 => c.resources[j] as i32,
            8..=15 => c.around[j - 8] as i32,
            16 => pair(c.classes[0], c.classes[1]),
            17 => pair(c.classes[2], c.classes[3]),
            18 => pair(c.classes[4], c.classes[5]),
            19 => pair(c.classes[6], c.bits),
            _ => c.around_bits as i32,
        }
    }

    /// `(1 << player) >> 1` in owner byte 0 of the square.
    fn player_square(&self, cx: i32, cy: i32) -> bool {
        let bit = ((1u32 << self.state.player) >> 1) as u8;
        self.world.tiling.owner_bits(cx, cy)[0] & bit != 0
    }

    /// The cell walk of 0x44cd90 / 0x44ce20 from scratch, collecting every cell.
    fn walk_cells(&self) -> Vec<(i32, i32)> {
        let mut it = places::Bfs::default();
        it.init(self);
        let mut v = Vec::new();
        while let Some(c) = it.next(self) {
            v.push(c);
        }
        v
    }

    /// The squares of slots 0xbc / 0xc0 / 0xfc (x outer, y inner): no player bit and no own
    /// tile before the first catapult tile.
    fn foreign_squares(&self) -> Vec<(i32, i32)> {
        let side = self.state.side;
        let mut v = Vec::new();
        for x in 0..side {
            for y in 0..side {
                if self.player_square(x, y) {
                    continue;
                }
                if self.chain_has_eco(self.first_tile(x, y)) {
                    continue;
                }
                v.push((x, y));
            }
        }
        v
    }

    pub(super) fn bfs_parts(&self) -> places::BfsWorld<'_> {
        places::BfsWorld { world: self.world, eco: self.state.eco, player: self.state.player, side: self.state.side }
    }
}

impl Manager for World2<'_> {
    fn scan(&mut self, t: i32) -> i32 {
        places::scan(self, t)
    }

    fn rescan(&mut self, t: i32, cx: i32, cy: i32) -> i32 {
        places::rescan(self, t, cx, cy)
    }

    fn clear(&mut self, t: i32) -> bool {
        self.state.scans.clear(t)
    }

    fn place_near(&mut self, level: i32, t: i32, b: [i32; 3]) -> places::Place {
        places::place_near(self, level, t, b)
    }

    fn places_around(&mut self, t: i32, place: &places::Place, out: &mut Vec<places::Place>, r: i32) -> i32 {
        self.state.scans.places_around(t, place, out, r, self.state.side)
    }

    fn places_of(&mut self, level: i32, t: i32, out: &mut Vec<places::Place>) -> i32 {
        self.state.scans.places_of(level, t, out)
    }

    fn place_family(&mut self, t: i32, level: i32, place: &places::Place, out: &mut Vec<places::Place>) -> i32 {
        self.state.scans.place_family(t, level, place, out)
    }

    fn remove_place(&mut self, t: i32, place: &places::Place) -> bool {
        self.state.scans.remove_place(t, place)
    }

    fn can_build(&mut self, t: i32, x: i32, y: i32) -> bool {
        let size = self.world.grid.size() as i32;
        if !(0..size).contains(&x) || !(0..size).contains(&y) {
            return false;
        }
        crate::build::check::check_for_build(self.world, x, y, self.state.player, t as u8).code() > 0
    }

    fn overlap(&mut self, ta: i32, pa: i32, tb: i32, pb: i32) -> bool {
        overlap(self.world, self.race_of(), ta, pa, tb, pb)
    }

    fn overlapping_building(&mut self, t: i32, pos: i32) -> i32 {
        let (x, y) = (pos & 0xffff, pos >> 16);
        let race = self.race_of();
        for b in self.world.warmap.cell((x >> 4) as usize, (pos >> 0x14) as usize & 0xfff, 2).collect::<Vec<_>>() {
            let Some(e) = building(self.world, b as i32) else { continue };
            let (ex, ey) = ensign(e);
            if overlap(self.world, race, t, (y << 16) | x, e.kind as i32, (ey << 16) | ex) {
                return b as i32;
            }
        }
        0
    }

    fn building_at(&mut self, x: i32, y: i32) -> i32 {
        let size = self.world.grid.size() as i32;
        let Some(&id) = usize::try_from(y * size + x).ok().and_then(|i| self.world.layers.map_object.get(i)) else { return 0 };
        if building(self.world, id as i32).is_some() { id as i32 } else { 0 }
    }

    fn building_pos(&mut self, id: i32) -> Option<(i32, i32)> {
        let e = building(self.world, id)?;
        if e.flags & DIED != 0 {
            return None;
        }
        Some((e.x(), e.y()))
    }

    fn pile_amount(&mut self, id: i32, good: i32) -> i32 {
        if building(self.world, id).is_none() {
            return 0;
        }
        let p = crate::buildings::pile_with_good(self.world, id as u16, good as u8);
        crate::pile::get(self.world, p).map_or(0, |p| p.amount as i32)
    }

    fn cost(&mut self, t: i32, which: i32) -> i32 {
        let race = self.race_of();
        if race >= 5 || !(1..0x53).contains(&t) {
            return 0;
        }
        let r = self.world.rules.building(race, t as u8);
        match which {
            0 => r.cost_boards as i32,
            1 => r.cost_stone as i32,
            2 => r.cost_gold as i32,
            _ => 0,
        }
    }

    fn need(&mut self, id: i32, which: i32) -> i32 {
        if building(self.world, id).is_none() {
            return 0;
        }
        // Role vtbl+0x34 `GetBuildingNeed` with good 7 / 0x20 / 0xf: a site's material still
        // needed (boards, stone; gold is 0xe there, so 0xf gives 0). A finished building's role
        // answers a pile id, which the engine adds up as an amount (release quirk): production
        // 0x54d410 its delivery pile of the good, work-up / animal ranch 0x55a660 the same, the
        // workshop 0x559610 its input pile, the barracks 0x534a50 its pile; every other role 0
        // (e.g. 0x53b060 gather, 0x53b220 simple, 0x550040 storage).
        let good = match which {
            0 => 7,
            1 => 0x20,
            2 => 0xf,
            _ => return 0,
        };
        let b = id as u16;
        if let Some(s) = self.world.sites.roles.get(&b) {
            return crate::build::order::building_need(self.world, s, good);
        }
        use crate::buildings::RoleKind;
        let w = &*self.world;
        let pile = match crate::buildings::role(w, b).map(|r| &r.kind) {
            Some(RoleKind::Production(p)) => p.delivery.iter().copied().find(|&p| crate::pile::get(w, p).is_some_and(|d| d.good == good)).unwrap_or(0),
            Some(RoleKind::WorkUp(_)) => crate::buildings::workup::building_need(w, b, good),
            Some(RoleKind::Workshop(_)) => crate::buildings::workshop::pile_with_good(w, b, good),
            Some(RoleKind::Barrack(_)) => crate::buildings::barrack::pile_with_good(w, b, good),
            _ => 0,
        };
        pile as i32
    }

    fn progress(&mut self, id: i32) -> i32 {
        let Some(e) = ent(self.world, id) else { return 100 };
        if e.flags & BIRTH == 0 {
            return 100;
        }
        crate::build::site::progress(self.world, id as u16)
    }

    fn kind(&mut self, id: i32) -> i32 {
        ent(self.world, id).map_or(0, |e| e.kind as i32)
    }

    fn finished_working(&mut self, id: i32) -> bool {
        ent(self.world, id).is_some_and(|e| e.flags & BIRTH == 0 && e.flags & WORKING != 0)
    }

    fn working(&mut self, id: i32) -> bool {
        ent(self.world, id).is_some_and(|e| e.flags & WORKING != 0)
    }

    fn worker_home(&mut self, id: i32) -> bool {
        crate::buildings::have_inhabitant(self.world, id as u16)
    }

    fn beds(&mut self, t: i32) -> i32 {
        let race = self.race_of();
        self.world.rules.building(race, t as u8).beds
    }

    fn free_beds(&mut self, id: i32) -> i32 {
        let Some(e) = ent(self.world, id) else { return 0 };
        if !(0x28..=0x2a).contains(&e.kind) {
            return 0;
        }
        let beds = self.world.rules.building(e.race, e.kind as u8).beds;
        let used = crate::buildings::residence::residents(self.world, id as u16).unwrap_or(0) as i32;
        (beds - used).max(0)
    }

    fn eco_of_building(&mut self, id: i32) -> i32 {
        let Some(e) = building(self.world, id) else { return 0 };
        if e.flags & DIED != 0 {
            return 0;
        }
        let (x, y) = ensign(e);
        self.eco_at(x, y) as i32
    }

    fn construct(&mut self, t: i32, x: i32, y: i32) {
        let p = self.state.player;
        crate::ai::send(self.world, 0x13a1, p, t as u32, ((y << 16) + x) as u32);
    }

    fn destroy(&mut self, id: i32) {
        let p = self.state.player;
        crate::ai::send(self.world, 0x138a, p, id as u32, 0);
    }

    fn plant_nodes(&mut self, t: i32, x: i32, y: i32) -> i32 {
        let r = self.working_radius(t).min(7);
        let (want, steep) = match t {
            0xb => (0x10, false),
            0x23 => (0x10, true),
            0x24 => (0x40, false),
            0x52 => (0x10, true),
            _ => return 0,
        };
        let mut n = 0;
        let size = self.world.grid.size() as i32;
        for (px, py) in crate::spiral::walk(self.world.grid, x, y, r as u8) {
            let i = (py * size + px) as usize;
            if self.world.layers.ground[i] & 0xf0 != want || self.world.layers.flags[i] & 0xd != 0 {
                continue;
            }
            let s = slope(self.world, i);
            if (s > 7) == steep {
                n += 1;
            }
        }
        n
    }

    fn set_working_area(&mut self, id: i32, x: i32, y: i32) -> bool {
        let size = self.world.grid.size() as i32;
        if !(0..size).contains(&x) || !(0..size).contains(&y) {
            return false;
        }
        let p = self.state.player;
        crate::ai::send(self.world, 0x1389, p, id as u32, ((y << 16) + x) as u32);
        true
    }

    fn working_area(&mut self, id: i32) -> (i32, i32) {
        match ent(self.world, id).map(|e| &e.data) {
            Some(crate::entity::EntityData::Building(b)) => (b.working_area.x(), b.working_area.y()),
            _ => (0, 0),
        }
    }

    fn working_radius(&mut self, t: i32) -> i32 {
        let race = self.race_of();
        self.world.rules.work.info(race, t as u8).map_or(0, |i| i.working_radius)
    }

    fn player(&mut self) -> i32 {
        self.state.player as i32
    }

    fn eco(&mut self) -> i32 {
        self.state.eco as i32
    }

    fn race(&mut self) -> i32 {
        self.race_of() as i32
    }

    fn influence_radius(&mut self, t: i32) -> i32 {
        let race = self.race_of();
        self.world.rules.building(race, t as u8).influence_radius
    }

    fn offered(&mut self, good: i32) -> i32 {
        let Some(e) = self.world.economy.get(self.state.eco) else { return 0 };
        let mut id = e.good_offer.get(good as usize).copied().unwrap_or(0);
        let mut n = 0;
        while let Some(p) = crate::pile::get(self.world, id) {
            n += p.amount as i32;
            id = self.world.entities.by_index(id as u32).map_or(0, |e| e.next);
        }
        n
    }

    fn worker_delta(&mut self, kind: i32) -> i32 {
        let Some(i) = crate::eco::WORKERS.iter().position(|&t| t as i32 == kind) else { return 0 };
        self.world.economy.get(self.state.eco).map_or(0, |e| e.worker_delta[i] as i32)
    }

    fn settlers(&mut self, kind: i32) -> i32 {
        self.world.economy.get(self.state.eco).and_then(|e| e.settlers.get(kind as usize).copied()).map_or(0, |n| n as i32)
    }

    fn count_buildings(&mut self, t: i32) -> i32 {
        let list = self.world.building_list(self.state.player as i32, t);
        list.iter().filter(|&&b| building(self.world, b as i32).is_some_and(|e| {
            let (x, y) = ensign(e);
            self.eco_at(x, y) == self.state.eco
        })).count() as i32
    }

    fn buildings(&mut self, t: i32, out: &mut Vec<i32>) -> i32 {
        out.clear();
        for b in self.world.building_list(self.state.player as i32, t) {
            let Some(e) = building(self.world, b as i32) else { continue };
            let (x, y) = ensign(e);
            if e.flags & (BIRTH | READY) != 0 && self.eco_at(x, y) == self.state.eco {
                out.push(b as i32);
            }
        }
        out.len() as i32
    }

    fn cell_resource(&mut self, res: i32, cx: i32, cy: i32) -> i32 {
        if !(0..64).contains(&cx) || !(0..64).contains(&cy) || res >> 4 >= 8 {
            return 0;
        }
        self.cell_word(cx, cy, res >> 4)
    }

    fn cell_resource_around(&mut self, res: i32, cx: i32, cy: i32) -> i32 {
        if !(0..64).contains(&cx) || !(0..64).contains(&cy) || res >> 4 >= 8 {
            return 0;
        }
        self.cell_word(cx, cy, 8 + (res >> 4))
    }

    fn resource_centre(&mut self, res: i32, sx: i32, sy: i32) -> Option<(i32, i32)> {
        let (mut sum_x, mut sum_y, mut n) = (0, 0, 0);
        square_nodes(self, res, sx, sy, |m, x, y, d| {
            if m.own_node(x + d, y + d) && m.resource_at(res, x, y) != 0 {
                sum_x += x + d;
                sum_y += y + d;
                n += 1;
            }
        })?;
        if n == 0 {
            return None;
        }
        Some((sum_x / n, sum_y / n))
    }

    fn resource_in_square(&mut self, res: i32, sx: i32, sy: i32) -> i32 {
        if !(0..64).contains(&sx) || !(0..64).contains(&sy) {
            return 0;
        }
        let mut sum = 0;
        let _ = square_nodes(self, res, sx, sy, |m, x, y, d| {
            if m.own_node(x + d, y + d) {
                sum += m.resource_at(res, x, y);
            }
        });
        sum
    }

    fn own_resource_cells(&mut self, res: i32, out: &mut Vec<CellValue>) {
        out.clear();
        for (cx, cy) in self.walk_cells() {
            let v = self.cell_resource(res, cx, cy);
            if v != 0 {
                out.push(CellValue { what: res, value: v as u16, cx: cx as u16, cy: cy as u16, own: 1 });
            }
        }
    }

    fn own_resource_cells_around(&mut self, res: i32, out: &mut Vec<CellValue>) {
        out.clear();
        for (cx, cy) in self.walk_cells() {
            let v = self.cell_resource_around(res, cx, cy);
            if v != 0 {
                out.push(CellValue { what: res, value: v as u16, cx: cx as u16, cy: cy as u16, own: 1 });
            }
        }
    }

    fn foreign_resource_cells(&mut self, res: i32, out: &mut Vec<CellValue>) {
        out.clear();
        if self.world.economy.get(self.state.eco).is_none() {
            return;
        }
        for (cx, cy) in self.foreign_squares() {
            let v = self.cell_resource(res, cx, cy);
            if v != 0 {
                out.push(CellValue { what: res, value: v as u16, cx: cx as u16, cy: cy as u16, own: 0 });
            }
        }
    }

    fn foreign_resource_cells_around(&mut self, res: i32, out: &mut Vec<CellValue>) {
        out.clear();
        if self.world.economy.get(self.state.eco).is_none() {
            return;
        }
        for (cx, cy) in self.foreign_squares() {
            let v = self.cell_resource_around(res, cx, cy);
            if v != 0 {
                out.push(CellValue { what: res, value: v as u16, cx: cx as u16, cy: cy as u16, own: 0 });
            }
        }
    }

    fn own_node(&mut self, x: i32, y: i32) -> bool {
        let size = self.world.grid.size() as i32;
        (0..size).contains(&x) && (0..size).contains(&y) && self.eco_at(x, y) == self.state.eco
    }

    fn border_cell(&mut self, cx: i32, cy: i32) -> bool {
        if !(0..64).contains(&cx) || !(0..64).contains(&cy) {
            return false;
        }
        let first = self.first_tile(cx, cy);
        if !self.chain_has_eco(first) {
            return false;
        }
        let mut t = first;
        while t != 0 {
            let tile = self.world.tiling.tile(t);
            if tile.catapult() || tile.eco_sector != self.state.eco {
                return true;
            }
            t = tile.sq_next;
        }
        false
    }

    fn resource_near(&mut self, res: i32, cx: i32, cy: i32, r: i32) -> bool {
        if !(0..64).contains(&cx) || !(0..64).contains(&cy) {
            return false;
        }
        let k = ((res >> 4) & 7) as usize;
        let c = self.cell(cx, cy);
        if c.around[k] != 0 || c.resources[k] != 0 {
            return true;
        }
        let side = self.state.side;
        for dx in -r..=r {
            for dy in -r..=r {
                if dx * dx + dy * dy > r * r || (dx == 0 && dy == 0) {
                    continue;
                }
                let (x, y) = (cx + dx, cy + dy);
                if x < 0 || x > side || y < 0 || y > side {
                    continue;
                }
                if self.cell(x, y).resources[k] != 0 {
                    return true;
                }
            }
        }
        false
    }

    fn class_near(&mut self, class: i32, cx: i32, cy: i32, r: i32) -> bool {
        if !(0..64).contains(&cx) || !(0..64).contains(&cy) {
            return false;
        }
        if self.cell_class(class, cx, cy) != 0 {
            return true;
        }
        let side = self.state.side;
        for dx in -r..=r {
            for dy in -r..=r {
                if dx * dx + dy * dy > r * r || (dx == 0 && dy == 0) {
                    continue;
                }
                let (x, y) = (cx + dx, cy + dy);
                if x < 0 || x > side || y < 0 || y > side {
                    continue;
                }
                if self.cell_class(class, x, y) != 0 {
                    return true;
                }
            }
        }
        false
    }

    fn cell_bit(&mut self, slot: i32, cx: i32, cy: i32) -> bool {
        if !(0..64).contains(&cx) || !(0..64).contains(&cy) {
            return false;
        }
        let c = self.cell(cx, cy);
        match slot {
            0xd4 | 0xd8 => c.bits & 1 != 0,
            0xdc => c.bits & 2 != 0,
            0xe0 => c.around_bits & 2 != 0,
            0xe4 => c.bits & 4 != 0,
            0xe8 => c.around_bits & 4 != 0,
            _ => false,
        }
    }

    fn resource_at(&mut self, res: i32, x: i32, y: i32) -> i32 {
        let size = self.world.grid.size() as i32;
        if x < 0 || y < 0 || x >= size || y >= size {
            return 0;
        }
        let r = self.world.layers.resource[(y * size + x) as usize] as i32;
        if r & 0xf0 == res { r & 0xf } else { 0 }
    }

    fn ground_is(&mut self, n: i32, x: i32, y: i32) -> bool {
        let size = self.world.grid.size() as i32;
        // [inf] The release reads any node index unchecked; off the map nothing matches here.
        let Some(&g) = usize::try_from(y * size + x).ok().and_then(|i| self.world.layers.ground.get(i)) else { return false };
        let g = g as i32 & 0xf0;
        let want = if (2..=10).contains(&n) { 0x10 * (n - 1) } else { 0 };
        g == want
    }

    fn cell_class(&mut self, class: i32, cx: i32, cy: i32) -> i32 {
        if !(0..64).contains(&cx) || !(0..64).contains(&cy) {
            return 0;
        }
        let c = self.cell(cx, cy);
        match class {
            0..=6 => c.classes[class as usize] as i32,
            7 => c.bits as i32,
            _ => 0,
        }
    }

    fn own_class_cells(&mut self, class: i32, out: &mut Vec<CellValue>) {
        out.clear();
        for (cx, cy) in self.walk_cells() {
            let v = self.cell_class(class, cx, cy);
            if v != 0 {
                out.push(CellValue { what: class, value: v as u16, cx: cx as u16, cy: cy as u16, own: 1 });
            }
        }
    }

    fn foreign_class_cells(&mut self, class: i32, out: &mut Vec<CellValue>) {
        out.clear();
        for (cx, cy) in self.foreign_squares() {
            let v = self.cell_class(class, cx, cy);
            if v != 0 {
                out.push(CellValue { what: class, value: v as u16, cx: cx as u16, cy: cy as u16, own: 0 });
            }
        }
    }

    fn border_cells(&mut self, out: &mut Vec<i32>) {
        out.clear();
        for (cx, cy) in self.walk_cells() {
            if self.border_cell(cx, cy) {
                out.push((cy << 16) + cx);
            }
        }
    }

    fn water_node(&mut self, cx: i32, cy: i32) -> i32 {
        let size = self.world.grid.size() as i32;
        for j in (1..=16).rev() {
            let y = cy * 16 + j;
            for i in (1..=16).rev() {
                let x = cx * 16 + i;
                let n = y * size + x;
                if let Some(&g) = usize::try_from(n).ok().and_then(|n| self.world.layers.ground.get(n))
                    && g & 0xf0 == 0x60
                {
                    return (y << 16) + x;
                }
            }
        }
        0
    }

    fn produce(&mut self, id: i32, good: i32, amount: i32) {
        let p = self.state.player;
        crate::ai::send(self.world, 0x13a9, p, ((id << 16) | (good & 0xffff)) as u32, amount as u32);
    }

    fn recruit(&mut self, id: i32, kind: i32, amount: i32) {
        let p = self.state.player;
        crate::ai::send(self.world, 0x13b7, p, ((id << 16) | (kind & 0xffff)) as u32, amount as u32);
    }

    fn switch(&mut self, id: i32) {
        let p = self.state.player;
        crate::ai::send(self.world, 0x138b, p, id as u32, 0);
    }

    fn net_message(&mut self, id: i32, a: i32, b: i32, player: i32, c: i32) {
        let (w, l) = if id == 0x13a3 { (((a & 0xffff) << 16) | (b & 0xffff), c) } else { (a, b) };
        crate::ai::send(self.world, id as u32, player as u8, w as u32, l as u32);
    }

    fn player_carriers(&mut self) -> i32 {
        let p = self.state.player as usize;
        self.world.statistic.players.get(p).and_then(|s| s.settlers.get(1).copied()).unwrap_or(0)
    }

    fn total_beds(&mut self) -> i32 {
        crate::eco::current_total_beds(self.world, self.state.player)
    }

    fn tick(&mut self) -> i32 {
        self.world.tick as i32
    }

    fn spiral_end(&mut self, r: i32) -> i32 {
        crate::spiral::first((r + 1) as u8) as i32 - 1
    }

    fn spiral_offset(&mut self, i: i32) -> (i32, i32) {
        (crate::spiral::dx(i as usize), crate::spiral::dy(i as usize))
    }

    fn own_building(&mut self, id: i32) -> bool {
        if id <= 0 || id >= 0xffff {
            return false;
        }
        let Some(e) = building(self.world, id) else { return false };
        if e.flags & DIED != 0 {
            return false;
        }
        let (x, y) = ensign(e);
        self.eco_at(x, y) == self.state.eco
    }

    fn min_carriers(&mut self) -> i32 {
        self.world.economy.get(self.state.eco).map_or(0, |e| e.min_carrier as i32)
    }

    fn set_min_carriers(&mut self, n: i32) {
        let Some(e) = self.world.economy.get_mut(self.state.eco) else { return };
        e.min_carrier = if n < 6 {
            5
        } else if n > 998 {
            999
        } else {
            n.min(e.settlers[1] as i32) as u16
        };
    }

    fn extra_carriers(&mut self) -> i32 {
        self.world.economy.get(self.state.eco).map_or(0, |e| e.extra_min_carrier as i32)
    }

    fn set_extra_carriers(&mut self, n: i32) {
        let Some(e) = self.world.economy.get_mut(self.state.eco) else { return };
        let v = n - e.extra_min_carrier as i32;
        e.extra_min_carrier = v.clamp(0, 99) as u8;
    }

    fn storage_space(&mut self, id: i32, good: i32) -> i32 {
        crate::buildings::storage::free_space(self.world, id as u16, good as u8)
    }

    fn next_event(&mut self) -> Option<AiEvent> {
        let q = &mut *self.queue;
        let ev = *q.events.get(q.read as usize)?;
        q.read += 1;
        if q.read as usize == q.events.len() {
            q.events.clear();
            q.read = 0;
        }
        Some(ev)
    }
}

/// 0x54e0c0: the largest height difference of the three opposite neighbour pairs.
fn slope(world: &World, i: usize) -> i32 {
    let size = world.grid.size() as i64;
    let h = |k: i64| usize::try_from(k).ok().and_then(|k| world.layers.height.get(k)).copied().unwrap_or(0) as i32;
    let i = i as i64;
    (h(i - 1) - h(i + 1)).abs().max((h(i - size) - h(i + size)).abs()).max((h(i - size - 1) - h(i + size + 1)).abs())
}

/// The node loop of slots 0xac / 0xb0 over square `(sx, sy)`: rows j 0..16 (outer), columns i
/// 0..16; a column past the map side ends the row, a row past it ends the loop (`>`, so the
/// side itself is visited); `d` = 1 for resource 0x70, 2 for 0x60, else 0. `None` when the
/// square's origin is off the map.
fn square_nodes(m: &mut World2<'_>, res: i32, sx: i32, sy: i32, mut f: impl FnMut(&mut World2<'_>, i32, i32, i32)) -> Option<()> {
    let size = m.world.grid.size() as i32;
    if sx * 16 >= size || sy * 16 >= size {
        return None;
    }
    let d = match res {
        0x70 => 1,
        0x60 => 2,
        _ => 0,
    };
    let mut j = 0;
    while j < 16 {
        let mut i = 0;
        while i < 16 {
            let (x, y) = (sx * 16 + i, sy * 16 + j);
            if x > size {
                break;
            }
            if y > size {
                return Some(());
            }
            f(m, x, y, d);
            i += 1;
        }
        j += 1;
    }
    Some(())
}

/// 0x44e7d0 `(tA, posA, tB, posB)`: unless the two are more than 4 apart and their bounding
/// rects (race table +0, info +0x34..+0x37) are disjoint, they overlap.
pub fn overlap(world: &World, race: u8, ta: i32, pa: i32, tb: i32, pb: i32) -> bool {
    let (xa, ya, xb, yb) = (pa & 0xffff, pa >> 16, pb & 0xffff, pb >> 16);
    let d = crate::spiral::grid_distance(xb - xa, yb - ya);
    let ra = world.rules.building(race, ta as u8).bounding_rect;
    let rb = world.rules.building(race, tb as u8).bounding_rect;
    let (a0, a1, a2, a3) = (ra[0] as i32, ra[1] as i32, ra[2] as i32, ra[3] as i32);
    let (b0, b1, b2, b3) = (rb[0] as i32, rb[1] as i32, rb[2] as i32, rb[3] as i32);
    let disjoint = a0 + xa > b1 + xb || b0 + xb > a1 + xa || a2 + ya > b3 + yb || a3 + ya < b2 + yb;
    !(d > 4 && disjoint)
}
