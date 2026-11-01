//! `CFogging` (Pathing/Fogging.cpp, release vtable 0x1060a84): the fog of war of one player.
//!
//! The original keeps one `CFogging` for the local player. It is updated from `CTiling::Update`
//! once per tick and told about every owner change by the tiling (`UpdateOwnerFogging`). Gameplay
//! never reads it back (only sounds and a trading dialog ask `IsPositionVisible`), so it is not
//! part of [`World::state_hash`]: a client sets [`World::fog`] for its player; a headless sim
//! leaves it `None`.
//!
//! # Layers
//! * `layer` (`m_pFogLayer`), one byte per node: bits 0..2 the **static** level (0 on the
//!   player's and allies' territory, 1..6 on the rings around it, 7 elsewhere; lost land drops
//!   to 3 = "explored"), bits 3..5 the **dynamic** level (reset to the static level each cycle,
//!   lowered by every seeing entity), bits 6..7 a counter of cycles with dynamic == static.
//! * `gfx`, byte 3 of `T_GFX_MAP_ELEMENT` as far as fog owns it: bits 0..2 the old and bits
//!   3..5 the new level the renderer blends between with the fade step (0 visible .. 7 black).
//!
//! # Cycle (`Update`)
//! Phase 1, 32 ticks: the fade step goes 1..15; the dynamic layer is computed over the entities
//! in 32 id slices; fog spots (`LiftFogAtSpot`) are applied every tick. Phase 2, 8 ticks: row
//! slices `[k·w/8, (k+1)·w/8)` go to the gfx layer (`UpdateGfxLayerAndInitializeDynamicLayer`, an
//! MMX loop over groups of 8 nodes, ported per byte), then a pending alliance-option change, the
//! land-exploration scan and the changed-row range for the renderer.
//!
//! Every function was re-checked against the release decompilation (vtable 0x1060a84 and the
//! helpers it calls; addresses on each function). Differences: the release indexes some
//! neighbours without bounds checks (here skipped outside the map) and keeps the reveal-map
//! state obfuscated (see [`Fogging::update`]).
//!
//! Alliances: the player's ally bits come from [`World::alliances`] (0x12dcd30) when the fog is
//! made; [`Fogging::set_ally_bits`] overrides them.

use std::collections::VecDeque;

use crate::World;
use crate::entity::{ObjType, flag};
use crate::spiral;

const STATIC: u8 = 0x07;
const DYNAMIC: u8 = 0x38;
const COUNTER: u8 = 0xC0;
/// `g_iFadeYBorder` after `IGfxEngine::SetFoggingFadingStep`: no row uses step 0.
pub const NO_BORDER: i32 = -50000;
/// `CFogging::Save` chunk versions.
const VERSION_SPOTS: u32 = 0x6EAB_CB06;
const VERSION_ALLIES: u32 = 0x6EAB_CB07;

/// `CAlliances::PlayerBit(player)`: `(1 << player) >> 1`, so player 0 has none.
pub fn player_bit(player: u8) -> u32 {
    1u32.checked_shl(player as u32).unwrap_or(0) >> 1
}

/// `SFogSpotInfo`: a spot kept visible (radius 35) until `until` (tick).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Spot {
    pub x: i32,
    pub y: i32,
    pub until: u32,
    pub player: u8,
}

/// The `CFogging` fields `IsPositionVisible` (release 0x5aee70) reads: +0x10 phase, +0x1c the
/// fade counter of phase 1, +0x24 the last changed row of phase 2.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Visibility {
    pub phase: u32,
    pub fade_counter: u32,
    pub range_max: i32,
}

impl Visibility {
    /// `CFogging::IsPositionVisible(x, y)` on `gfx` (the fog bits, `width` nodes per row): the
    /// level the renderer currently shows (phase 1: the old one until the fade counter reaches
    /// 15; phase 2: the old one up to the last changed row; otherwise 0) is below 3.
    pub fn is_visible(self, gfx: &[u8], width: usize, x: i32, y: i32) -> bool {
        if (x as u32 as usize) >= width || (y as u32 as usize) >= width {
            return false;
        }
        let Some(&g) = gfx.get(x as usize + width * y as usize) else { return false };
        let (old, new) = (g & STATIC, (g & DYNAMIC) >> 3);
        let level = match self.phase {
            1 => {
                if self.fade_counter >= 15 {
                    new
                } else {
                    old
                }
            }
            2 => {
                if y > self.range_max {
                    new
                } else {
                    old
                }
            }
            _ => 0,
        };
        level < 3
    }
}

#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Fogging {
    width: usize,
    player: u8,
    /// `CAlliances::PlayerAllyBits(local player)`.
    ally_bits: u32,
    /// +12 `m_pFogLayer`.
    layer: Vec<u8>,
    /// +8 the gfx map's fog bits.
    gfx: Vec<u8>,
    /// +4 the gfx bits came from a savegame (`Start` keeps them).
    loaded: bool,
    /// +16 phase (1 compute, 2 write), +20 step in the phase.
    phase: u32,
    step: u32,
    /// +24 next entity id of `CalculateDynamicLayer`.
    next_entity: u32,
    /// +28 fade counter of phase 1.
    fade_counter: u32,
    /// +32 / +36 first and last gfx row changed in this phase 2.
    range_min: i32,
    range_max: i32,
    /// +40 / +44 "reveal world map" as set and as in effect (copied at each cycle end,
    /// `UpdateRWM`), +48 cycles it has been in effect.
    rwm_set: bool,
    rwm: bool,
    rwm_cycles: u32,
    /// +52 land-exploration check on, +56/+60 the square (v, w) it has reached.
    land_check: bool,
    land_v: usize,
    land_w: usize,
    /// +64 `std::list<SFogSpotInfo>`.
    spots: VecDeque<Spot>,
    /// +76 allies do not reveal fog (in effect), +77 as set, +78 change pending.
    own_only: bool,
    own_only_set: bool,
    own_only_pending: bool,
    /// The gfx engine's fog state: `SetFoggingFadingStep` / `SetFoggingUpdateMode` /
    /// `SetFoggingRange`.
    fade: u32,
    border: i32,
    range: (i32, i32),
}

impl Fogging {
    /// `CFogging::CFogging` + `Init` for `player` on the world's map (not started).
    pub fn new(world: &World, player: u8) -> Fogging {
        let width = world.grid.size() as usize;
        let n = width * width;
        Fogging {
            width,
            player,
            ally_bits: world.alliances.ally_bits[(player as usize).min(8)] as u32,
            layer: vec![0; n],
            gfx: vec![0; n],
            loaded: false,
            phase: 0,
            step: 0,
            next_entity: 0,
            fade_counter: 0,
            range_min: 0,
            range_max: 0,
            rwm_set: false,
            rwm: false,
            rwm_cycles: 0,
            land_check: false,
            land_v: 0,
            land_w: 0,
            spots: VecDeque::new(),
            own_only: false,
            own_only_set: false,
            own_only_pending: false,
            fade: 0,
            border: NO_BORDER,
            range: (0, -1),
        }
    }

    pub fn player(&self) -> u8 {
        self.player
    }

    /// Byte 3 fog bits per node (bits 0..2 old level, 3..5 new level), row-major.
    pub fn gfx(&self) -> &[u8] {
        &self.gfx
    }

    /// `g_iFogFadeStep` (0..=15): how far the renderer has blended from old to new.
    pub fn fade(&self) -> u32 {
        self.fade
    }

    /// `g_iFadeYBorder`: rows below it are drawn with fade step 0 (their new level).
    pub fn border(&self) -> i32 {
        self.border
    }

    /// `SetFoggingRange`: the rows (inclusive) whose gfx bits changed in the last phase 2;
    /// `(0, -1)` when none did.
    pub fn changed_rows(&self) -> (i32, i32) {
        self.range
    }

    /// `CAlliances::PlayerAllyBits` of the player (default: its own bit).
    pub fn set_ally_bits(&mut self, bits: u32) {
        self.ally_bits = bits;
    }

    /// `DbgStaticFogValue` / `DbgDynamicFogValue`.
    pub fn static_level(&self, i: usize) -> u8 {
        self.layer.get(i).map_or(7, |b| b & STATIC)
    }

    pub fn dynamic_level(&self, i: usize) -> u8 {
        self.layer.get(i).map_or(7, |b| (b & DYNAMIC) >> 3)
    }

    /// The players whose territory and entities lift this player's fog.
    fn local_bits(&self) -> u32 {
        if self.own_only { player_bit(self.player) } else { self.ally_bits }
    }

    /// `CWorldManager::Index` arithmetic as the original does it (rows wrap); `None` outside the
    /// buffer.
    fn raw(&self, x: i32, y: i32) -> Option<usize> {
        let i = x as i64 + y as i64 * self.width as i64;
        (0..self.layer.len() as i64).contains(&i).then_some(i as usize)
    }

    fn in_world(&self, x: i32, y: i32) -> bool {
        (x as u32 as usize) < self.width && (y as u32 as usize) < self.width
    }

    /// `IGfxEngine::SetFoggingFadingStep`.
    fn set_fading_step(&mut self, step: u32) {
        self.fade = step;
        self.border = NO_BORDER;
    }

    /// `CFogging::Start` (vtbl+0x10, release 0x5ae9d0).
    pub fn start(&mut self, world: &World) {
        self.clear_dynamic_and_calc_static(world);
        self.initialize_dynamic(0, self.width);
        for i in 0..32 {
            self.calculate_dynamic(world, i);
        }
        if !self.loaded {
            self.set_whole_gfx();
            self.loaded = false;
        }
        self.set_fading_step(0);
        self.phase = 1;
        self.step = 0;
        self.fade_counter = 0;
        self.rwm_set = false;
        self.rwm = self.rwm_set;
        self.rwm_cycles = 0;
        self.enable_land_explored_check(false);
    }

    /// `CFogging::ResetFogging` (vtbl+0x30, 0x5add30): start again from the world, dropping
    /// loaded gfx bits.
    pub fn reset(&mut self, world: &World) {
        self.loaded = false;
        self.own_only = self.own_only_set;
        self.own_only_pending = false;
        self.start(world);
    }

    /// `CFogging::Load` (vtbl+0, 0x5aef70; chunk 135): the gfx fog bits (ORed in), the spots and
    /// the alliance option. `Start` then keeps the gfx bits. Short or malformed data loads what it
    /// can. (`Save` is vtbl+4, 0x5af0e0: gfx bits & 0x3f, version 0x6eabcb07, spot count and
    /// spots, the option as set (+0x49), end mark 0x6eabcbff.)
    pub fn load(&mut self, chunk: &[u8]) {
        let n = self.gfx.len();
        for (g, &b) in self.gfx.iter_mut().zip(chunk.iter().take(n)) {
            *g |= b;
        }
        self.spots.clear();
        self.own_only = false;
        self.own_only_set = false;
        self.own_only_pending = false;
        let u32_at = |at: usize| chunk.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
        let mut at = n;
        let version = u32_at(at).unwrap_or(0);
        at += 4;
        if version >= VERSION_SPOTS {
            let count = u32_at(at).unwrap_or(0);
            at += 4;
            for _ in 0..count {
                let Some(r) = chunk.get(at..at + 16) else { break };
                let i32_at = |k: usize| i32::from_le_bytes([r[k], r[k + 1], r[k + 2], r[k + 3]]);
                self.spots.push_back(Spot { x: i32_at(0), y: i32_at(4), until: i32_at(8) as u32, player: r[12] });
                at += 16;
            }
        }
        if version >= VERSION_ALLIES {
            self.own_only = u32_at(at).is_some_and(|v| v != 0);
            self.own_only_set = self.own_only;
        }
        self.loaded = true;
    }

    /// `CFogging::Update` (vtbl+0x14, 0x5aeab0), from `CTiling::Update`. The release keeps the
    /// reveal-map state (+0x28 as set, +0x2c in effect) as keyed values (`(MapSize·16 +
    /// [0x12dccf8])·16 + on·0x9745935`, the copy XOR 0x90909090, 0x5add50) and compares them with
    /// the "on" key; here two bools.
    pub fn update(&mut self, world: &World) {
        let rwm_active = self.rwm_cycles == 1 || self.rwm_cycles == 2;
        match self.phase {
            1 => {
                if (!self.rwm || rwm_active) && self.fade_counter < 15 {
                    self.fade_counter += 1;
                    self.set_fading_step(self.fade_counter);
                }
                if self.rwm {
                    if rwm_active {
                        let (a, b) = self.y_range(self.step, 32);
                        self.set_dynamic_full_bright(a, b);
                    }
                } else {
                    self.calculate_dynamic(world, self.step);
                }
                self.step += 1;
                if self.step >= 32 {
                    self.phase = 2;
                    self.step = 0;
                    self.range_min = 0x4000;
                    self.range_max = -1;
                }
                if !self.spots.is_empty() {
                    for k in 0..self.spots.len() {
                        let s = self.spots[k];
                        self.update_entity_fogging(s.x, s.y, 35);
                    }
                    if self.spots.front().is_some_and(|s| s.until <= world.tick) {
                        self.spots.pop_front();
                    }
                }
            }
            2 => {
                if !self.rwm || rwm_active {
                    let (a, b) = self.y_range(self.step, 8);
                    self.update_gfx_rows(a, b);
                    // IGfxEngine::SetFoggingUpdateMode(end row).
                    self.fade = 15;
                    self.border = b as i32;
                }
                self.step += 1;
                if self.step >= 8 {
                    if self.own_only_pending {
                        self.own_only_pending = false;
                        self.own_only = self.own_only_set;
                        self.clear_dynamic_and_calc_static(world);
                        self.initialize_dynamic(0, self.width);
                    }
                    self.update_land_exploration(world);
                    self.range = if self.range_max < 0 { (0, -1) } else { (self.range_min, self.range_max) };
                    self.phase = 1;
                    self.step = 0;
                    self.fade_counter = 0;
                    self.rwm = self.rwm_set;
                    if self.rwm {
                        self.rwm_cycles += 1;
                    } else {
                        self.rwm_cycles = 0;
                    }
                }
            }
            _ => {}
        }
    }

    /// `CFogging::CalcYRange(slice, slices)`.
    fn y_range(&self, slice: u32, slices: u32) -> (usize, usize) {
        let s = self.width / slices as usize;
        let lo = s * slice as usize;
        (lo, (lo + s).min(self.width))
    }

    /// `CFogging::ClearDynamicFoggingAndCalcStatic` (0x5ae6d0): static 7, 0 on the territory of
    /// the local bits (the release visits only the 16×16 squares whose owner-bits byte at
    /// 0x12ef998 has a local bit, then the eco-sector owner of each node); then per territory node (16×16 owner squares in row order,
    /// their nodes in row order) the six neighbours drop to 1, and if any did, spiral entries
    /// 7..=Last(6) to their radius. Dynamic bits and counters are cleared.
    fn clear_dynamic_and_calc_static(&mut self, world: &World) {
        self.layer.fill(7);
        let bits = self.local_bits();
        for (i, b) in self.layer.iter_mut().enumerate() {
            if bits & player_bit(world.owner_of(i)) != 0 {
                *b = 0;
            }
        }
        let w = self.width;
        let side = w / 16;
        let last6 = spiral::last(6);
        for sw in 0..side {
            for sv in 0..side {
                for y in sw * 16..sw * 16 + 16 {
                    for x in sv * 16..sv * 16 + 16 {
                        if self.layer[x + w * y] != 0 {
                            continue;
                        }
                        let (x, y) = (x as i32, y as i32);
                        let mut lowered = false;
                        for k in 1..=6 {
                            if let Some(m) = self.raw(x + spiral::dx(k), y + spiral::dy(k))
                                && self.layer[m] > 1
                            {
                                self.layer[m] = 1;
                                lowered = true;
                            }
                        }
                        if lowered {
                            for k in 7..=last6 {
                                let r = spiral::radius(k);
                                if let Some(m) = self.raw(x + spiral::dx(k), y + spiral::dy(k))
                                    && self.layer[m] > r
                                {
                                    self.layer[m] = r;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// `CFogging::InitializeDynamicLayer(y0, y1)` (0x5ae080, which always starts at row 0; only
    /// called with (0, width)): dynamic := static (the byte shifted left by 3,
    /// so the counter takes the old dynamic level's low bits, as in the original).
    fn initialize_dynamic(&mut self, y0: usize, y1: usize) {
        let w = self.width;
        for b in &mut self.layer[(y0 * w).min(w * w)..(y1 * w).min(w * w)] {
            *b = (*b & STATIC) | (*b << 3);
        }
    }

    /// `CFogging::SetDynamicLayerToFullBright(y0, y1)`.
    fn set_dynamic_full_bright(&mut self, y0: usize, y1: usize) {
        let w = self.width;
        for b in &mut self.layer[(y0 * w).min(w * w)..(y1 * w).min(w * w)] {
            *b &= !DYNAMIC;
        }
    }

    /// `CFogging::SetWholeGfxLayerAndInitializeDynamicLayer` (0x5ae590): both gfx levels :=
    /// dynamic.
    fn set_whole_gfx(&mut self) {
        for (b, g) in self.layer.iter_mut().zip(&mut self.gfx) {
            let v = *b;
            *b = (v & STATIC) | (v << 3);
            *g = ((v & DYNAMIC) >> 3) | (v & DYNAMIC) | (*g & COUNTER);
        }
    }

    /// `CFogging::ResetGfxFogLayer(reveal)` (vtbl+0x18, 0x5adf90): every node visible (0) or
    /// black (7).
    pub fn reset_gfx(&mut self, reveal: bool) {
        let v = if reveal { 0 } else { 7 };
        for g in &mut self.gfx {
            *g = (v << 3) | v | (*g & COUNTER);
        }
    }

    /// `CFogging::CalculateDynamicLayer(slice)` (0x5ae210): entity ids up to
    /// `(slice + 1) · ⌈last / 32⌉` lower the dynamic layer by their sight.
    fn calculate_dynamic(&mut self, world: &World, slice: u32) {
        if slice == 0 {
            self.next_entity = 0;
        }
        let bits = self.local_bits();
        let last = world.entities.last_used();
        let end = ((slice + 1) * ((last + 31) >> 5)).min(last);
        let mut i = self.next_entity;
        while i <= end {
            if let Some(e) = world.entities.by_index(i) {
                if e.obj_type == ObjType::Building && (e.kind == 50 || e.kind == 51) {
                    // Dark Tribe temples / fortresses show their area to everybody.
                    if e.flags & 0x20_0000 != 0 {
                        self.update_entity_fogging(e.x(), e.y(), 25);
                    }
                } else if bits & player_bit(e.owner) != 0 && e.flags & flag::READY != 0 {
                    let f = e.flags;
                    let r = match e.obj_type {
                        ObjType::Settler if f & flag::WARRIOR_TYPE_MASK != 0 && f & flag::ON_BOARD == 0 => {
                            if e.kind == 48 { 25 } else { 15 }
                        }
                        ObjType::Ship => 25,
                        ObjType::LandVehicle => 20,
                        ObjType::Building => match e.kind {
                            31 | 32 => 20,
                            45 => 45,
                            46 if crate::buildings::have_inhabitant(world, i as u16) => 35,
                            47 if crate::buildings::have_inhabitant(world, i as u16) => 40,
                            48 if crate::buildings::have_inhabitant(world, i as u16) => 45,
                            _ => 0,
                        },
                        _ => 0,
                    };
                    if r > 0 && e.x() >= 2 && e.y() >= 2 {
                        self.update_entity_fogging(e.x(), e.y(), r);
                    }
                }
            }
            i += 1;
        }
        self.next_entity = end + 1;
    }

    /// `CFogging::UpdateEntityFogging(x, y, radius)` (0x5ae100): dynamic 0 within the radius, at
    /// most `j` on ring `radius + j` (j = 1..6; entries `Last(r + j − 1)..=Last(r + j)`, from the
    /// table at 0x117cf36). Only the nodes are bounds-checked, not the centre.
    fn update_entity_fogging(&mut self, x: i32, y: i32, radius: u8) {
        let w = self.width;
        for k in 0..=spiral::last(radius) {
            let (px, py) = (x + spiral::dx(k), y + spiral::dy(k));
            if self.in_world(px, py) {
                self.layer[px as usize + w * py as usize] &= !DYNAMIC;
            }
        }
        let mut level = 0u8;
        for j in radius as u32 + 1..=radius as u32 + 6 {
            level += 8;
            let (lo, hi) = (spiral::last((j - 1).min(255) as u8), spiral::last(j.min(255) as u8));
            for k in lo..=hi {
                let (px, py) = (x + spiral::dx(k), y + spiral::dy(k));
                if self.in_world(px, py) {
                    let b = &mut self.layer[px as usize + w * py as usize];
                    if level < *b & DYNAMIC {
                        *b = level | (*b & !DYNAMIC);
                    }
                }
            }
        }
    }

    /// `CFogging::UpdateGfxLayerAndInitializeDynamicLayer(y0, y1)` (0x5ae3f0, MMX over groups of
    /// 8 nodes; constants .rdata 0x101bd30..0x101bd50: 0x40, 0x18, 0x07, 0x38, 0xC0). Per node: counter :=
    /// dynamic == static ? min(counter + 1, 3) : 0, and dynamic := static. Unless every node of
    /// the group has counter 3, the gfx bits become new := min(D, max(N, 3)) (N = 7: min(D, 7)),
    /// old := N. Rows where a new level changed widen the changed-row range.
    fn update_gfx_rows(&mut self, y0: usize, y1: usize) {
        let w = self.width;
        for y in y0..y1 {
            let mut row_same = true;
            for g in 0..w >> 3 {
                let base = y * w + g * 8;
                let mut dynamic = [0u8; 8];
                let mut all_stable = true;
                for (k, d) in dynamic.iter_mut().enumerate() {
                    let b = self.layer[base + k];
                    let s = b & STATIC;
                    *d = b & DYNAMIC;
                    let counter = if s << 3 == *d { (b.saturating_add(0x40)) & COUNTER } else { 0 };
                    all_stable &= counter == COUNTER;
                    self.layer[base + k] = counter | s | (s << 3);
                }
                if all_stable {
                    continue;
                }
                for (k, &d) in dynamic.iter().enumerate() {
                    let gb = self.gfx[base + k];
                    let n = gb & DYNAMIC;
                    let floor = if n == DYNAMIC { DYNAMIC } else { 0x18 };
                    let new = d.min(n.max(floor));
                    row_same &= new == n;
                    self.gfx[base + k] = new | (gb & COUNTER) | (n >> 3);
                }
            }
            if !row_same {
                if (y as i32) < self.range_min {
                    self.range_min = y as i32;
                }
                self.range_max = y as i32;
            }
        }
    }

    /// `CFogging::UpdateOwnerFogging(x, y, owner)` (vtbl+0x1c, 0x5aed30): the tiling changed the
    /// node's owner. Gained by the local bits: static 0 and `SmoothFullBrightStaticFog`. Lost
    /// (static < 3): static 3, and the nodes within radius 2 get the radius of the nearest static-0
    /// node within radius 2 of them (3 if none).
    pub fn update_owner_fogging(&mut self, x: i32, y: i32, owner: u8) {
        let Some(n) = self.raw(x, y) else { return };
        if self.local_bits() & player_bit(owner) != 0 {
            if self.layer[n] & STATIC != 0 {
                self.layer[n] &= DYNAMIC;
                self.smooth_full_bright_static(x, y);
            }
        } else if self.layer[n] & STATIC < 3 {
            self.layer[n] = (self.layer[n] & DYNAMIC) | 3;
            for i in 0..19 {
                let (mx, my) = (x + spiral::dx(i), y + spiral::dy(i));
                let Some(m) = self.raw(mx, my) else { continue };
                if self.layer[m] & STATIC == 0 {
                    continue;
                }
                let mut level = 3;
                for j in 1..19 {
                    if self.raw(mx + spiral::dx(j), my + spiral::dy(j)).is_some_and(|k| self.layer[k] & STATIC == 0) {
                        level = spiral::radius(j);
                        break;
                    }
                }
                self.layer[m] = level | (self.layer[m] & DYNAMIC);
            }
        }
    }

    /// `CFogging::SmoothFullBrightStaticFog(x, y)` (0x5ae010): spiral entries 1..=Last(6) at most their
    /// radius (the counter bits are cleared, as in the original).
    fn smooth_full_bright_static(&mut self, x: i32, y: i32) {
        for i in 1..=spiral::last(6) {
            let Some(m) = self.raw(x + spiral::dx(i), y + spiral::dy(i)) else { continue };
            let r = spiral::radius(i);
            if self.layer[m] & STATIC > r {
                self.layer[m] = r | (self.layer[m] & DYNAMIC);
            }
        }
    }

    /// `CFogging::IsPositionVisible(x, y)` (vtbl+0x20, 0x5aee70): the level the renderer currently shows is below 3.
    pub fn is_position_visible(&self, x: i32, y: i32) -> bool {
        self.visibility().is_visible(&self.gfx, self.width, x, y)
    }

    /// What `IsPositionVisible` reads besides the gfx bits (for a copy of the fog, e.g. the
    /// client's snapshot).
    pub fn visibility(&self) -> Visibility {
        Visibility { phase: self.phase, fade_counter: self.fade_counter, range_max: self.range_max }
    }

    /// `CFogging::SetRWM` (vtbl+0x28, 0x5adcc0): reveal the whole map (takes effect at the next cycle end).
    pub fn set_reveal_map(&mut self, on: bool) {
        self.rwm_set = on;
    }

    /// `CFogging::SetAlliesDontRevealFog` (vtbl+0x2c, 0x5add10): only the player's own territory and entities count
    /// (takes effect at the end of the next phase 2, with a full static recalculation).
    pub fn set_allies_dont_reveal_fog(&mut self, on: bool) {
        if self.own_only_set != on {
            self.own_only_set = on;
            self.own_only_pending = true;
        }
    }

    /// `CFogging::LiftFogAtSpot(x, y, until, player)` (vtbl+0x3c, 0x5aeef0; the ally bits
    /// 0x12dcd30, whatever the option): for an ally's spell, the spot stays
    /// visible until tick `until`.
    pub fn lift_fog_at_spot(&mut self, x: i32, y: i32, until: u32, player: u8) {
        if self.ally_bits & player_bit(player) != 0 {
            self.spots.push_back(Spot { x, y, until, player });
        }
    }

    /// `CFogging::EnableOrDisableLandExploredCheck` (vtbl+0x34, 0x5addc0).
    pub fn enable_land_explored_check(&mut self, on: bool) {
        self.land_check = on;
        self.land_v = 0;
        self.land_w = 0;
    }

    /// `CFogging::IsAlmostAllLandExplored` (vtbl+0x38, 0x5adde0): the scan passed every square row.
    pub fn is_almost_all_land_explored(&self) -> bool {
        self.land_w >= self.width / 16
    }

    /// `CFogging::UpdateLandExplorationState` (0x5addf0): advance over the squares that are explored.
    fn update_land_exploration(&mut self, world: &World) {
        if !self.land_check {
            return;
        }
        let side = self.width / 16;
        while self.land_w < side && self.square_explored(world, self.land_v, self.land_w) {
            self.land_v += 1;
            if self.land_v >= side {
                self.land_v = 0;
                self.land_w += 1;
            }
        }
    }

    /// `CFogging::IsSquareLandExploredVW(v, w)` (0x5ade50; ground byte 1 of the 4-byte landscape
    /// element at 0x15630d4, node flags 0x15630d8): squares at the map edge (distance ≤ 2) always
    /// count; otherwise fewer than min(80, 176 − 16·distance) land nodes may still show a new
    /// level above 3.
    fn square_explored(&self, world: &World, v: usize, w: usize) -> bool {
        let side = self.width / 16;
        let d = (w + 1).min(side.saturating_sub(w)).min((v + 1).min(side.saturating_sub(v)));
        if d <= 2 {
            return true;
        }
        let limit = 80.min(176i64 - 16 * d as i64);
        let mut fogged = 0i64;
        for y in w * 16..w * 16 + 16 {
            for x in v * 16..v * 16 + 16 {
                let i = x + self.width * y;
                let (ground, flags) = (world.layers.ground[i], world.layers.flags[i]);
                if (ground & 0xF0 == 0x10 || flags & 1 == 0) && (self.gfx[i] & DYNAMIC) >> 3 > 3 {
                    fogged += 1;
                    if fogged >= limit {
                        return false;
                    }
                }
            }
        }
        true
    }
}
