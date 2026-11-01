//! Unit selection, as `CInputProcessor` does it (Logic/InputProcessor.cpp):
//!
//! * **Who can be selected**: the local player's entities with `EntityFlag_Selectable` (0x2000,
//!   set by the soldier / specialist role `Init`s, cleared while in a building or carrying),
//!   or with flag 0x4000 (selected as "other", marker 91), and buildings (obj type 8).
//! * **Selection type** (`GetEntitySelectionType`): building 0x10000; by warrior type
//!   1..=5 → 0x30000 + wt, 6 → 0x30000, 7 (specialist) → 0x20000 + type << 8, 8/9 → 0x40000,
//!   10/11 → 0x50000, else 0. A selection only ever holds one category (`& 0xFFFF00`), at most
//!   100 entities, and at most one building.
//! * **Click** (`DotSelection` → `SelectOne`): the local player's selectable map objects within
//!   spiral radius 2 of the clicked node whose bounding box (`GetBoundingBox`: settlers 25 px
//!   wide centred on the anchor, the height and top of frame 1 of direction 0 of their current
//!   gfx job `+0x26`; none while that is 0), anchored at the entity's node (`+0x18`, not its drawn
//!   walking position), contains the clicked node's screen point, the one furthest down (largest
//!   y) wins (`GetTheBestEntity`);
//!   else the first owned selectable map object within radius 5, unless it is a settler beyond
//!   spiral index 1. With the append modifier (`wParam & 8`) a same-type entity is added or, if
//!   selected, removed; anything else replaces the selection. A click on nothing clears it.
//! * **Box** (`PrepareBoxSelection`, `IAnimatedEntity::BoxSelection`, `BoxSelection`): every
//!   drawn entity whose anchor (screen point of its node) lies in the box is a candidate if its
//!   category beats the type so far (or equals it with the same type byte); the type follows
//!   the candidates, and at the end the candidates of the final type (`& 0xFFFF00`) are taken in
//!   draw order. Strict mode (`wParam < 0`, a selection exists) keeps the selection and its type
//!   and only adds entities of exactly that type (mask 0xFFFF00).
//! * **Markers** (`CSettler::GetGfx` → `SGfxObjectInfo +22 / +20`, drawn by `BlitSettler` in the
//!   icon pass with `g_pIconGfx` = `Gfx.gfx` images): the role's `GetKindOfSelection`
//!   (`CSoldierRole`: rank 2 → 92, rank 3 → 93, type 44 → 95, type 45 → 94, else 1;
//!   `ISettlerRole` → 1; flag 0x400 → 91) and, for types with live points, the health icon
//!   `9 − 7·hp / max` (2 = full).
//!
//! Verified in the shipped exe: `GetEntitySelectionType` (release 0x4e69a0, the jump table at
//! 0x4e69ec; the entity overload 0x4e6a20 takes the warrior type from flags & 0xF),
//! `BoxSelection` (0x4ed2c0: the type from the candidates by category, `& 0xFFFF00`, at most 100
//! (`cmp 0x64`), one building, the Selected flag 0x200), `IAnimatedEntity::BoxSelection`
//! (0x532d20: the local player, Selectable 0x2000 or obj type 8, masks 0xFF0000 / 0xFFFF00 /
//! 0xFFFFFF by strictness, the node's screen point in the box) and `CSoldierRole::
//! GetKindOfSelection` (0x58aa70: types 30..63 through the byte table at 0x58aad4: 92 / 93 for
//! ranks 2 / 3, 95 for 44, 94 for 45, else 1) and `SelectOne` (0x4ed630: spiral `Last(2)` /
//! `Last(5)` from the word table at 0x117cf3e, flags & 0x6000, the local player, a settler beyond
//! spiral index 1 rejected, the building fallback through the object layer), `DotSelection`
//! (0x4eb3b0: append = `wParam >> 3 & 1`; nothing hit clears only without append and sends event
//! 0x25B; append replaces unless the selection's type is the entity's and not a building, then a
//! selected entity (0x200) is removed; flag 0x4000 marks with 0x400 instead of 0x200),
//! `GetTheBestEntity` (0x4ed490: entities with a job frame whose box (0x4626d0: width 25.0, left
//! 12.5 from the anchor, the frame's height and top, times the zoom) holds the point; the largest
//! y wins) and the markers (`CSettler` gfx info 0x5810a0: 0x200 → the role's `GetKindOfSelection`
//! (vtable +0x6c) and, when the type's max live points (table 0x1546932) are not 0, the health
//! icon `9 − 7·hp / max`; else 0x400 → 91).

use std::collections::HashMap;

use crate::population::{Population, Unit};

/// The selection's capacity: 1000 on the user's request (2026-10-23); the release's
/// `m_vSelection` holds 100 (`AddToSelection`, `DotSelection`, `BoxSelection` and 0x4ed810:
/// `< 100`), and its commands carry at most 100 ids.
pub const MAX: usize = 1000;
/// Shift + click's spiral (0x4ed810): its first 3000 entries (radius ~31), with Alt (key bit
/// 0x10) 19823 (radius ~74, the whole table).
pub const SAME_TYPE_NEAR: usize = 3000;
pub const SAME_TYPE_FAR: usize = 19823;
const SELECTABLE: u32 = 0x2000;
/// "Selectable as other" (`DotSelection`: marked with 0x400 instead of `Selected`).
const OTHER: u32 = 0x4000;
const BUILDING_TYPE: u32 = 0x10000;

/// `CInputProcessor::GetEntitySelectionType(objType, type, warriorType)`.
pub fn entity_type(obj: u8, kind: u16, warrior: u8) -> u32 {
    if obj == 8 {
        return BUILDING_TYPE;
    }
    match warrior {
        1..=5 => 0x30000 + warrior as u32,
        6 => 0x30000,
        7 => ((kind as u32) << 8) + 0x20000,
        8 | 9 => 0x40000,
        10 | 11 => 0x50000,
        _ => 0,
    }
}

/// An icon over a selected settler: `(x, y, entity, Gfx.gfx image)`.
pub type Icon = (u16, u16, u16, u8);

/// The selected building's corner brackets: `CBuilding::GetGfxInfo` (release 0x5367d0) sets gfx
/// info `+0x16` to 0x1c while the entity has `Selected` (0x200), and the building blitter
/// (0x6707c0 / 0x662e80) draws that `Gfx.gfx` image right after the building, at the same anchor
/// minus the icon's own offsets (times the zoom), with the building's shading.
pub const BUILDING_ICON: u8 = 0x1c;

/// A bounding box `(width, height, left, top)` around an anchor, px at zoom 1.
pub type BBox = (f32, f32, f32, f32);

/// A selectable thing of the population.
#[derive(Clone, Copy, Debug)]
pub struct Candidate {
    pub id: u16,
    pub x: u16,
    pub y: u16,
    pub obj: u8,
    pub kind: u16,
    pub flags: u32,
    pub owner: u8,
}

impl Candidate {
    fn of_unit(u: &Unit) -> Self {
        Candidate { id: u.id, x: u.x, y: u.y, obj: u.obj, kind: u.kind, flags: u.flags, owner: u.owner }
    }
    pub fn selection_type(&self) -> u32 {
        entity_type(self.obj, self.kind, (self.flags & 0xF) as u8)
    }
    fn selectable(&self) -> bool {
        self.flags & (SELECTABLE | OTHER) != 0
    }
}

/// What the selection changed, for the GUI (`RequestDialog` / `Clear`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    None,
    /// The selection is new or different: show its dialog.
    Selected,
    /// A click on nothing without Ctrl (`DotSelection` 0x4eb3b0: `sub_EE690`, UI event 0x191 to
    /// the leave handler 0x4edb10, GUI event 0x25b), also when nothing was selected; a box that
    /// selected nothing (0x4ed2c0 → `sub_EDC90`).
    Cleared,
}

/// How a selection change reaches the GUI ([`crate::gui::Gui::show_selection`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
    /// `RequestDialog` 0x4ee730 (after a click or box selection, the "next building" button):
    /// the sidebar closed (0x260), a building's dialog created anew (0x25a).
    Request,
    /// Emptied by `sub_EDC90` (right click with a building, crush, an empty click or box
    /// selection, the last entity gone) or a click on nothing: GUI event 0x25b
    /// (`ResetMainMenu`) and the leave handler's 0x260.
    Reset,
    /// `sub_EE690` alone (the main and submenu tabs' 0x192, the search page) or a refill: the
    /// dialogs as they are.
    Keep,
}

#[derive(Clone, Debug, Default)]
pub struct Selection {
    /// Entity ids in selection order.
    pub ids: Vec<u16>,
    /// Entities selected as "other" (flag 0x400 instead of `Selected`).
    pub other: Vec<u16>,
    /// What each selected id was when it was last seen ([`Selection::stamp`]): an id whose entity
    /// died and was reused by another between two snapshots is not the selected one.
    seen: HashMap<u16, Identity>,
}

/// What tells an entity at an id from a later one at the same id: a unit's unique id; a
/// building's type and position (its snapshot carries no unique id).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Identity {
    Unit(u32),
    Building(u8, u16, u16),
}

fn identity(pop: &Population, id: u16) -> Option<Identity> {
    pop.settlers.iter().chain(&pop.vehicles).find(|u| u.id == id).map(|u| Identity::Unit(u.uid)).or_else(|| {
        pop.buildings.iter().find(|b| b.id == id).map(|b| Identity::Building(b.kind, b.x, b.y))
    })
}

/// The map objects by node: settlers outside buildings and vehicles (`m_cMapObjectMap`
/// 0x15630dc), else a building at its position (finished: the same layer; a construction site:
/// `m_cDecoObjectMap` 0x15630e0, which `SelectOne` reads only where the first layer is empty).
struct Objects {
    by_node: HashMap<(u16, u16), Candidate>,
}

impl Objects {
    fn new(pop: &Population) -> Self {
        let mut by_node = HashMap::new();
        for u in pop.settlers.iter().chain(&pop.vehicles) {
            by_node.insert((u.x, u.y), Candidate::of_unit(u));
        }
        for b in pop.buildings.iter().filter(|b| b.id != 0) {
            by_node.entry((b.x, b.y)).or_insert(Candidate { id: b.id, x: b.x, y: b.y, obj: 8, kind: b.kind as u16, flags: 0, owner: b.owner });
        }
        Objects { by_node }
    }
}

/// Every candidate the population has (units and buildings), in draw order (rows, then x).
pub fn candidates(pop: &Population) -> Vec<Candidate> {
    let mut v: Vec<Candidate> = pop.settlers.iter().chain(&pop.vehicles).map(Candidate::of_unit).collect();
    v.extend(pop.buildings.iter().filter(|b| b.id != 0).map(|b| Candidate { id: b.id, x: b.x, y: b.y, obj: 8, kind: b.kind as u16, flags: 0, owner: b.owner }));
    v.sort_by_key(|c| (c.y, c.x));
    v
}

impl Selection {
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    fn clear(&mut self) {
        self.ids.clear();
        self.other.clear();
    }

    /// `GetSelectionTypeOfCurrentSelection`.
    pub fn current_type(&self, pop: &Population) -> u32 {
        self.ids.first().and_then(|&id| find(pop, id)).map_or(0, |c| c.selection_type())
    }

    /// `DotSelection` at node `(x, y)`. `screen(x, y)` is a node's screen point (anchor with
    /// height) and `bbox(unit)` a settler's bounding box `(width, height, left, top)` there.
    #[allow(clippy::too_many_arguments)]
    pub fn click(
        &mut self,
        pop: &Population,
        local: u8,
        size: usize,
        (x, y): (u16, u16),
        append: bool,
        screen: &dyn Fn(u16, u16) -> (f32, f32),
        bbox: &dyn Fn(&Candidate) -> Option<BBox>,
    ) -> Change {
        let objects = Objects::new(pop);
        let found = select_one(&objects, local, size, (x as i32, y as i32), screen, bbox);
        let Some(found) = found else {
            if append {
                return Change::None;
            }
            self.clear();
            return Change::Cleared;
        };
        let other = found.flags & OTHER != 0;
        let mut add = true;
        let mut replace = true;
        if append && !other {
            let cur = self.current_type(pop);
            let t = found.selection_type();
            replace = cur == 0 || cur == BUILDING_TYPE || cur != t;
            if !replace && self.ids.contains(&found.id) {
                add = false;
                self.ids.retain(|&i| i != found.id); // DeSelectEntity
            }
        }
        if replace {
            self.clear();
        }
        if add && self.ids.len() < MAX {
            self.ids.push(found.id);
            if other {
                self.other.push(found.id);
            }
        }
        Change::Selected
    }

    /// Shift or Alt + click at node `(x, y)`: the input queue (0x4e9ac0) turns event 0x1dd with
    /// key bits `& 0x14` into 0x1aa, whose handler (0x4eb5f0) drops the selection (0x4ee690),
    /// takes `SelectOne`'s entity there, its selection type (`+0x4c`, `+0x60` = 2) and the tiling
    /// sector of its node, and walks the spiral around the clicked node (0x4ed810; [`SAME_TYPE_NEAR`]
    /// entries, [`SAME_TYPE_FAR`] with Alt): every map object of the local player that is alive
    /// (flags `& 0x3000000`), selectable (0x2000, or a building), of exactly that type and in
    /// that sector is selected, nearest first, until the selection is full or holds a building.
    /// Nothing more when the sector is 0. **Ours, the user's request:** a unit must also be of
    /// the clicked one's settler type, i.e. the same weapon and the same rank (soldier types
    /// differ by rank), not only the same weapon (`GetEntitySelectionType` 0x4e69a0: 0x30000 +
    /// warrior type for every rank).
    #[allow(clippy::too_many_arguments)]
    pub fn same_type_around(
        &mut self,
        pop: &Population,
        local: u8,
        size: usize,
        (x, y): (u16, u16),
        wide: bool,
        screen: &dyn Fn(u16, u16) -> (f32, f32),
        bbox: &dyn Fn(&Candidate) -> Option<BBox>,
    ) -> Change {
        self.clear();
        let objects = Objects::new(pop);
        let Some(found) = select_one(&objects, local, size, (x as i32, y as i32), screen, bbox) else { return Change::Cleared };
        let ty = found.selection_type();
        let land = land_of(pop, &found);
        if land == 0 {
            return Change::Cleared;
        }
        let n = if wide { SAME_TYPE_FAR } else { SAME_TYPE_NEAR }.min(s4_sim::spiral::LEN);
        for i in 0..n {
            let (px, py) = (x as i32 + s4_sim::spiral::dx(i), y as i32 + s4_sim::spiral::dy(i));
            if px < 0 || py < 0 || px as usize >= size || py as usize >= size {
                continue;
            }
            let Some(c) = objects.by_node.get(&(px as u16, py as u16)) else { continue };
            let alive = c.obj == 8 || c.flags & 0x300_0000 != 0;
            let rank = c.obj == 8 || c.kind == found.kind;
            if c.owner != local || !alive || !(c.flags & SELECTABLE != 0 || c.obj == 8) || c.selection_type() != ty || !rank || land_of(pop, c) != land {
                continue;
            }
            if !self.ids.contains(&c.id) {
                self.ids.push(c.id);
            }
            if c.obj == 8 || self.ids.len() >= MAX {
                break;
            }
        }
        if self.ids.is_empty() { Change::Cleared } else { Change::Selected }
    }

    /// Box selection over the screen rectangle `(x0, y0, x1, y1)` (any corner order); `strict`
    /// = the add-to-selection modifier. `visible(c)` = the entity is drawn this frame.
    pub fn boxed(
        &mut self,
        pop: &Population,
        local: u8,
        rect: (f32, f32, f32, f32),
        strict: bool,
        screen: &dyn Fn(u16, u16) -> (f32, f32),
        visible: &dyn Fn(&Candidate) -> bool,
    ) -> Change {
        // PrepareBoxSelection.
        let cur = self.current_type(pop);
        let (ty, mode) = if strict && cur != 0 {
            (cur, 1)
        } else {
            self.clear();
            (0, 0)
        };
        self.box_select(pop, local, rect, mode, ty, screen, visible)
    }

    /// The settler search's mode 1 (`CInputProcessor` 0x4ec230: the box state set to the whole
    /// output, +0x60 = 2, +0x4c = `ty` kept, +0x34 = 1): the drawn own entities of exactly type
    /// `ty` (mask 0xFFFFFF in `IAnimatedEntity::BoxSelection` 0x532d20), then `BoxSelection`. The
    /// caller drops the selection first (`sub_EE690`).
    pub fn boxed_type(
        &mut self,
        pop: &Population,
        local: u8,
        rect: (f32, f32, f32, f32),
        ty: u32,
        screen: &dyn Fn(u16, u16) -> (f32, f32),
        visible: &dyn Fn(&Candidate) -> bool,
    ) -> Change {
        self.box_select(pop, local, rect, 2, ty, screen, visible)
    }

    /// `IAnimatedEntity::BoxSelection` for each drawn entity (`mode` +0x60: 0 by category, 1 the
    /// same `& 0xFFFF00`, 2 the same `& 0xFFFFFF`), then `BoxSelection` 0x4ed2c0.
    #[allow(clippy::too_many_arguments)]
    fn box_select(
        &mut self,
        pop: &Population,
        local: u8,
        rect: (f32, f32, f32, f32),
        mode: u8,
        mut ty: u32,
        screen: &dyn Fn(u16, u16) -> (f32, f32),
        visible: &dyn Fn(&Candidate) -> bool,
    ) -> Change {
        let (x0, x1) = (rect.0.min(rect.2), rect.0.max(rect.2));
        let (y0, y1) = (rect.1.min(rect.3), rect.1.max(rect.3));
        let strict = mode != 0;
        let mask = if mode == 2 { 0xFFFFFF } else if strict { 0xFFFF00 } else { 0xFF0000 };
        // IAnimatedEntity::BoxSelection for each drawn entity.
        let mut cands = Vec::new();
        for c in candidates(pop).into_iter().filter(|c| visible(c)) {
            let own = c.owner == local;
            if !own || !(c.flags & SELECTABLE != 0 || c.obj == 8) {
                continue;
            }
            let t = c.selection_type();
            let take = if strict {
                t & mask == ty & mask
            } else if t & 0xFF0000 > ty & 0xFF0000 {
                true
            } else {
                t & 0xFF0000 == ty & 0xFF0000 && t & 0xFF00 == ty & 0xFF00
            };
            let (sx, sy) = screen(c.x, c.y);
            if take && (x0..=x1).contains(&sx) && (y0..=y1).contains(&sy) {
                // +0x34 clear: the type follows the candidate (the search keeps its own).
                if mode != 2 {
                    ty = t;
                }
                cands.push(c);
            }
        }
        // BoxSelection.
        if ty as i32 > 0 {
            let limit = if ty == BUILDING_TYPE && !self.is_empty() { 0 } else { cands.len() };
            for c in &cands[..limit] {
                if self.ids.len() >= MAX {
                    break;
                }
                if c.selection_type() & 0xFFFF00 == ty & 0xFFFF00 && !self.ids.contains(&c.id) {
                    self.ids.push(c.id);
                    if ty == BUILDING_TYPE {
                        break;
                    }
                }
            }
        }
        // 0x4ed2c0: an empty selection after the box goes through `sub_EDC90` (0x25b).
        if self.is_empty() {
            Change::Cleared
        } else {
            Change::Selected
        }
    }

    /// Drop what left the population, changed owner or stopped being selectable, and an id that
    /// another entity took over since the last snapshot (`RemoveFromSelection` 0x4eda10 from
    /// `CMapObjectMgr::Kill` 0x56d030 / the vehicles' 0x597100 at the death itself, and from the
    /// roles clearing `Selected`). Returns whether anything was dropped.
    pub fn refresh(&mut self, pop: &Population, local: u8) -> bool {
        let n = self.ids.len();
        let seen = std::mem::take(&mut self.seen);
        let keep = |id: &u16| {
            find(pop, *id).is_some_and(|c| c.owner == local && (c.obj == 8 || c.selectable()))
                && seen.get(id).is_none_or(|&was| identity(pop, *id) == Some(was))
        };
        self.ids.retain(keep);
        let ids = self.ids.clone();
        self.other.retain(|i| ids.contains(i));
        self.stamp(pop);
        self.ids.len() != n
    }

    /// Remember what the selected ids are in `pop` (the snapshot the selection was made in).
    pub fn stamp(&mut self, pop: &Population) {
        self.seen = self.ids.iter().filter_map(|&id| identity(pop, id).map(|i| (id, i))).collect();
    }

    /// The marker icons (`Gfx.gfx` images) of the selection: `(x, y, entity, icon)`. A selected
    /// building has [`BUILDING_ICON`] at its position; a settler its selection icon, then the
    /// health icon; a vehicle its selection icon, then ammo and health.
    pub fn markers(&self, pop: &Population) -> Vec<Icon> {
        let mut out = Vec::new();
        for b in pop.buildings.iter().filter(|b| self.ids.contains(&b.id)) {
            out.push((b.x, b.y, b.id, BUILDING_ICON));
        }
        for u in pop.settlers.iter().filter(|u| self.ids.contains(&u.id)) {
            if self.other.contains(&u.id) {
                out.push((u.x, u.y, u.id, 91));
                continue;
            }
            out.push((u.x, u.y, u.id, kind_of_selection(u.kind)));
            if u.max_health > 0 {
                let hp = u.health.min(u.max_health) as u32;
                out.push((u.x, u.y, u.id, (9 - 7 * hp / u.max_health as u32) as u8));
            }
        }
        // Vehicles (`crate::vehicle_gfx`): gfx info +0x16 by Selected (0x200) / "other" (0x400),
        // then the class's +0x15 and +0x14 while its flag is set.
        for u in pop.vehicles.iter().filter(|u| self.ids.contains(&u.id)) {
            use crate::vehicle_gfx as vg;
            let other = self.other.contains(&u.id);
            let (flag, icon) = match (u.obj == 2, other) {
                (false, false) => (0x200, vg::SELECTED),
                (false, true) => (0x400, vg::OTHER),
                (true, false) => (0x200, vg::SHIP_SELECTED),
                (true, true) => (0x400, vg::SHIP_OTHER),
            };
            out.push((u.x, u.y, u.id, icon));
            if let Some(g) = u.vehicle.filter(|g| g.icon_flag == flag) {
                out.extend(g.icons.iter().filter(|&&i| i != 0).map(|&i| (u.x, u.y, u.id, i)));
            }
        }
        out
    }
}

/// `CSoldierRole::GetKindOfSelection` (rank markers); other roles return 1 (`ISettlerRole`).
/// [inf] `CPriestRole` (94) is not mapped to settler types here: its types are not known yet.
pub fn kind_of_selection(kind: u16) -> u8 {
    match kind {
        30 | 33 | 36 | 39 | 42 | 62 => 92,
        31 | 34 | 37 | 40 | 43 | 63 => 93,
        44 => 95,
        45 => 94,
        _ => 1,
    }
}

/// The tiling sector of a candidate's node (units: [`Unit::land`]; buildings: their ensign's).
fn land_of(pop: &Population, c: &Candidate) -> u16 {
    if c.obj == 8 {
        pop.buildings.iter().find(|b| b.id == c.id).map_or(0, |b| b.land)
    } else {
        pop.settlers.iter().chain(&pop.vehicles).find(|u| u.id == c.id).map_or(0, |u| u.land)
    }
}

fn find(pop: &Population, id: u16) -> Option<Candidate> {
    pop.settlers.iter().chain(&pop.vehicles).find(|u| u.id == id).map(Candidate::of_unit).or_else(|| {
        pop.buildings.iter().find(|b| b.id == id).map(|b| Candidate { id: b.id, x: b.x, y: b.y, obj: 8, kind: b.kind as u16, flags: 0, owner: b.owner })
    })
}

/// `CInputProcessor::SelectOne` (0x4ed630). The radius-5 fallback also takes the local player's
/// building whose position is the node (obj type 8 passes without the 0x6000 flags).
fn select_one(
    objects: &Objects,
    local: u8,
    size: usize,
    (x, y): (i32, i32),
    screen: &dyn Fn(u16, u16) -> (f32, f32),
    bbox: &dyn Fn(&Candidate) -> Option<BBox>,
) -> Option<Candidate> {
    let in_world = |px: i32, py: i32| px >= 0 && py >= 0 && (px as usize) < size && (py as usize) < size;
    let owned = |c: &Candidate| c.owner == local;
    let mut near = Vec::new();
    for i in 0..=s4_sim::spiral::last(2) {
        let (px, py) = (x + s4_sim::spiral::dx(i), y + s4_sim::spiral::dy(i));
        if !in_world(px, py) {
            continue;
        }
        if let Some(c) = objects.by_node.get(&(px as u16, py as u16))
            && owned(c)
            && c.selectable()
        {
            near.push(*c);
        }
    }
    // GetTheBestEntity: the clicked node's screen point inside the entity's box; largest y.
    let (cx, cy) = if in_world(x, y) { screen(x as u16, y as u16) } else { (f32::MIN, f32::MIN) };
    let mut best: Option<Candidate> = None;
    for c in &near {
        let Some((w, h, left, top)) = bbox(c) else { continue };
        let (sx, sy) = screen(c.x, c.y);
        let (bx, by) = (sx - left, sy - top);
        if bx <= cx && bx + w >= cx && by <= cy && by + h >= cy && c.y as u32 > best.map_or(0, |b| b.y as u32) {
            best = Some(*c);
        }
    }
    if best.is_some() {
        return best;
    }
    for i in 0..=s4_sim::spiral::last(5) {
        let (px, py) = (x + s4_sim::spiral::dx(i), y + s4_sim::spiral::dy(i));
        if !in_world(px, py) {
            continue;
        }
        let Some(c) = objects.by_node.get(&(px as u16, py as u16)) else { continue };
        if !owned(c) || !(c.selectable() || c.obj == 8) {
            continue;
        }
        return if c.obj == 1 && i > 1 { None } else { Some(*c) };
    }
    None
}


#[cfg(test)]
mod tests {
    use super::*;

    fn unit(id: u16, x: u16, y: u16, kind: u16, flags: u32, owner: u8) -> Unit {
        Unit { id, obj: 1, flags, x, y, kind, owner, health: 10, max_health: 10, ..Default::default() }
    }

    /// Soldiers (warrior type 2 in the low flag bits) and a specialist (type 7) of player 1, a
    /// carrier (not selectable) and an enemy soldier.
    fn pop() -> Population {
        let mut p = Population { size: 64, ..Default::default() };
        p.settlers = vec![
            unit(1, 10, 10, 29, SELECTABLE | 0x100 | 2, 1),
            unit(2, 12, 10, 30, SELECTABLE | 0x100 | 2, 1),
            unit(3, 14, 10, 47, SELECTABLE | 0x100 | 7, 1),
            unit(4, 16, 10, 1, 0x100, 1),
            unit(5, 18, 10, 29, SELECTABLE | 0x100 | 2, 2),
        ];
        p
    }

    fn screen(x: u16, y: u16) -> (f32, f32) {
        (24.0 * x as f32 - 12.0 * y as f32, 12.0 * y as f32)
    }

    #[test]
    fn types() {
        assert_eq!(entity_type(8, 13, 0), 0x10000);
        assert_eq!(entity_type(1, 29, 2), 0x30002);
        assert_eq!(entity_type(1, 47, 7), 0x22F00);
        assert_eq!(entity_type(1, 1, 0), 0);
    }

    #[test]
    fn box_takes_the_highest_category_only() {
        let p = pop();
        let mut s = Selection::default();
        let ch = s.boxed(&p, 1, (-1000.0, -1000.0, 1000.0, 1000.0), false, &screen, &|_| true);
        assert_eq!(ch, Change::Selected);
        // Soldiers (0x30000) beat the specialist (0x20000); not the carrier or the enemy.
        assert_eq!(s.ids, vec![1, 2]);
        let m = s.markers(&p);
        assert_eq!(m, vec![(10, 10, 1, 1), (10, 10, 1, 2), (12, 10, 2, 92), (12, 10, 2, 2)]);
    }

    /// The settler search's mode 1: only the type asked for, though soldiers outrank it.
    #[test]
    fn box_of_a_fixed_type() {
        let p = pop();
        let mut s = Selection::default();
        let ch = s.boxed_type(&p, 1, (-1000.0, -1000.0, 1000.0, 1000.0), entity_type(1, 47, 7), &screen, &|_| true);
        assert_eq!((ch, s.ids.clone()), (Change::Selected, vec![3]));
        let mut s = Selection::default();
        assert_eq!(s.boxed_type(&p, 1, (-1000.0, -1000.0, 1000.0, 1000.0), entity_type(1, 48, 7), &screen, &|_| true), Change::Cleared);
    }

    #[test]
    fn click_selects_appends_and_clears() {
        let p = pop();
        let mut s = Selection::default();
        let bbox = |_: &Candidate| Some((25.0, 30.0, 12.0, 25.0));
        assert_eq!(s.click(&p, 1, 64, (10, 10), false, &screen, &bbox), Change::Selected);
        assert_eq!(s.ids, vec![1]);
        // Ctrl: same type is added; clicking it again removes it.
        s.click(&p, 1, 64, (12, 10), true, &screen, &bbox);
        assert_eq!(s.ids, vec![1, 2]);
        s.click(&p, 1, 64, (12, 10), true, &screen, &bbox);
        assert_eq!(s.ids, vec![1]);
        // Another type replaces the selection even with Ctrl.
        s.click(&p, 1, 64, (14, 10), true, &screen, &bbox);
        assert_eq!(s.ids, vec![3]);
        // The carrier is not selectable; nothing selectable near (16, 10) beyond spiral index 1.
        assert_eq!(s.click(&p, 1, 64, (40, 40), false, &screen, &bbox), Change::Cleared);
        assert!(s.is_empty());
    }

    /// Shift + click (0x1aa → 0x4ed810, plus the user's rank rule): around the clicked swordsman
    /// (type 29, rank 1) exactly the local player's alive, selectable type-29 units in its sector
    /// within the spiral's first 3000 entries; with Alt also those up to entry 19823; never the
    /// rank-2 swordsman, a bowman, another player's, another sector's, an unselectable or a dead
    /// one; nearest first; at most `MAX`.
    #[test]
    fn shift_click_selects_the_same_type_and_rank_around() {
        use s4_sim::spiral::{dx, dy};
        const ALIVE: u32 = 0x200_0000;
        let at = |i: usize| ((100 + dx(i)) as u16, (100 + dy(i)) as u16);
        let mut n = 0u16;
        let mut u = |i: usize, kind: u16, warrior: u32, flags: u32, owner: u8, land: u16| {
            n += 1;
            let (x, y) = at(i);
            Unit { id: n, obj: 1, flags: flags | warrior, x, y, kind, owner, land, health: 10, max_health: 10, ..Default::default() }
        };
        let ok = SELECTABLE | ALIVE;
        let settlers = vec![
            u(0, 29, 1, ok, 1, 5),          // 1: clicked
            u(2000, 29, 1, ok, 1, 5),       // 2: near
            u(10, 30, 1, ok, 1, 5),         // 3: rank 2
            u(11, 32, 2, ok, 1, 5),         // 4: bowman
            u(12, 29, 1, ok, 2, 5),         // 5: another player's
            u(13, 29, 1, ok, 1, 6),         // 6: another sector
            u(14, 29, 1, ALIVE, 1, 5),      // 7: not selectable
            u(15, 29, 1, SELECTABLE, 1, 5), // 8: not alive
            u(5000, 29, 1, ok, 1, 5),       // 9: Alt only
            u(19824, 29, 1, ok, 1, 5),      // 10: out of range
            u(1, 29, 1, ok, 1, 5),          // 11: next to the click
        ];
        let p = Population { size: 256, settlers, ..Default::default() };
        let screen = |x: u16, y: u16| (x as f32 * 10.0, y as f32 * 10.0);
        let none = |_: &Candidate| None;
        let mut s = Selection::default();
        assert_eq!(s.same_type_around(&p, 1, 256, at(0), false, &screen, &none), Change::Selected);
        assert_eq!(s.ids, [1, 11, 2], "near: same type and rank, nearest first");
        assert_eq!(s.same_type_around(&p, 1, 256, at(0), true, &screen, &none), Change::Selected);
        assert_eq!(s.ids, [1, 11, 2, 9], "Alt: the wide range");
        // Nothing under the click: cleared.
        assert_eq!(s.same_type_around(&p, 1, 256, (20, 20), false, &screen, &none), Change::Cleared);
        assert!(s.ids.is_empty());
        // The cap: 1100 of them packed around the click.
        let settlers: Vec<Unit> = (0..1100).map(|i| u(i, 29, 1, ok, 1, 5)).collect();
        let p = Population { size: 256, settlers, ..Default::default() };
        s.same_type_around(&p, 1, 256, at(0), false, &screen, &none);
        assert_eq!(s.ids.len(), MAX);
    }

    #[test]
    fn refresh_drops_unselectable() {
        let mut p = pop();
        let mut s = Selection { ids: vec![1, 2], ..Default::default() };
        p.settlers[1].flags &= !SELECTABLE;
        assert!(s.refresh(&p, 1));
        assert_eq!(s.ids, vec![1]);
    }

    /// An id whose entity died and was taken by another between two snapshots (the release
    /// deselects at the death itself, 0x56d030 → 0x4eda10): dropped, not kept for the new one.
    #[test]
    fn refresh_drops_a_reused_id() {
        let mut p = pop();
        let mut s = Selection { ids: vec![1, 2], ..Default::default() };
        s.stamp(&p);
        assert!(!s.refresh(&p, 1));
        p.settlers[0].uid ^= 0x55;
        assert!(s.refresh(&p, 1));
        assert_eq!(s.ids, vec![2]);
    }

    /// A selected building gets icon 0x1c at its position (0x5367d0); crushed (gone from the
    /// population) it is dropped and nothing stays drawn.
    #[test]
    fn building_brackets_until_it_is_gone() {
        let mut p = pop();
        let b = crate::population::Building {
            id: 40, x: 20, y: 30, kind: 1, owner: 1, race: 0, inhabitants: Vec::new(), site: None, worker: None,
            door: (21, 31), land: 0, ready: true, generic: 1, flag: None, pen: None, mushroom: None,
        };
        p.buildings.push(b);
        let mut s = Selection { ids: vec![40], ..Default::default() };
        assert_eq!(s.markers(&p), vec![(20, 30, 40, BUILDING_ICON)]);
        p.buildings.clear();
        assert!(s.refresh(&p, 1));
        assert!(s.is_empty() && s.markers(&p).is_empty());
    }
}
