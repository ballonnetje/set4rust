//! Fog of war: the renderer-facing half of `CFogging` (Pathing/Fogging.cpp) and a test pattern
//! standing in for the simulation's visibility.
//!
//! # Levels
//! Fog levels are 0 (visible) ..= 7 (unexplored, black). The renderer reads two of them per node
//! from byte 3 of `T_GFX_MAP_ELEMENT` (bits 0..2 old, 3..5 new) and blends between them with the
//! global fade step (`CalcFogging`, see docs/TERRAIN.md §6 and [`crate::terrain::object_fog`]).
//!
//! # Input (owned by the simulation later)
//! Per node, `CFogging` keeps one byte in `m_pFogLayer`: bits 0..2 = **static** level `S`
//! (`ClearDynamicFoggingAndCalcStatic`: 0 on the local player's territory, the spiral radius 1..6
//! around it, 7 elsewhere), bits 3..5 = **dynamic** level `D` (reset to `S` every cycle, then
//! lowered by every own unit/building: 0 within its sight radius, `j` on ring `radius + j`,
//! `UpdateEntityFogging`), bits 6..7 = a stability counter. [`FogInput`] is that pair per node.
//!
//! # Timing (`CFogging::Update`, called once per tick from `CTiling::Update`)
//! A cycle is 32 + 8 ticks.
//! * Phase 1, 32 ticks: the dynamic layer is computed in 32 slices; the fade step goes
//!   1, 2, … 15 and stays (`IGfxEngine::SetFoggingFadingStep`, which also clears the fade border).
//! * Phase 2, 8 ticks: rows `[k·w/8, (k+1)·w/8)` of slice `k` are written to the gfx layer
//!   (`UpdateGfxLayerAndInitializeDynamicLayer`), then `SetFoggingUpdateMode(end row)`: fade step 15,
//!   rows above the border use step 0. A rewritten row therefore keeps showing its *old* level, the
//!   rows below still show their *new* one, and the next phase 1 fades everything old → new.
//!
//! # Gfx layer update (MMX, 8 nodes at a time; constants 0x40, 0x18, 0x07, 0x38, 0xC0 at release
//! 0x101bd40)
//! Per node with gfx new level `N`:
//! * counter := `D == S` ? min(counter + 1, 3) : 0; the fog byte is re-initialised to `D := S`.
//! * unless all 8 nodes of the group have counter 3: `new := min(D, max(N, 3))` (`N == 7`: `D`),
//!   `old := N`. Seeing brightens at once; losing sight dims to level 3 at most ("explored").
//!
//! Here phase 1's slice-wise accumulation is replaced by sampling [`FogInput`] once, when phase 2
//! starts; everything the renderer sees (levels, fade step, border, timing) follows the original.

/// Fog layer byte as `CFogging` keeps it.
const STATIC: u8 = 0x07;
const DYNAMIC: u8 = 0x38;
const COUNTER: u8 = 0xC0;

/// Per-node levels for one cycle (row-major, `width²`).
pub struct FogInput {
    pub static_level: Vec<u8>,
    pub dynamic_level: Vec<u8>,
}

pub struct FogDriver {
    width: usize,
    layer: Vec<u8>,
    /// 1 = computing (fade), 2 = writing the gfx layer.
    phase: u8,
    step: usize,
    fade_counter: u32,
    /// `g_iFogFadeStep`.
    pub fade: u32,
    /// `g_iFadeYBorder`: rows `< border` use fade step 0.
    pub border: i32,
    /// Byte 3 of every node (only bits 0..5 are meaningful here).
    pub gfx: Vec<u8>,
    pending: Option<FogInput>,
}

/// Rows whose gfx bits changed (inclusive).
pub type RowRange = (usize, usize);

impl FogDriver {
    /// `CFogging::Start` with the first input: both gfx levels = the dynamic level, fade 0.
    pub fn start(width: usize, input: &FogInput) -> Self {
        let n = width * width;
        let mut layer = vec![0u8; n];
        let mut gfx = vec![0u8; n];
        for i in 0..n {
            let (s, d) = (input.static_level[i] & 7, input.dynamic_level[i].min(input.static_level[i]) & 7);
            layer[i] = s | s << 3;
            gfx[i] = d | d << 3;
        }
        FogDriver { width, layer, phase: 1, step: 0, fade_counter: 0, fade: 0, border: -50000, gfx, pending: None }
    }

    /// One `CFogging::Update`. `input` is asked for the next cycle's levels when phase 2 begins.
    pub fn tick(&mut self, input: impl FnOnce() -> FogInput) -> Option<RowRange> {
        let w = self.width;
        if self.phase == 1 {
            if self.fade_counter < 15 {
                self.fade_counter += 1;
                self.fade = self.fade_counter;
                self.border = -50000;
            }
            self.step += 1;
            if self.step >= 32 {
                self.phase = 2;
                self.step = 0;
                let inp = input();
                for (i, b) in self.layer.iter_mut().enumerate() {
                    let d = inp.dynamic_level[i].min(*b & STATIC) & 7;
                    *b = (*b & !DYNAMIC) | d << 3;
                }
                // The static level can change too (territory); it takes effect from the next cycle.
                self.pending = Some(inp);
            }
            return None;
        }
        let k = self.step;
        let (y0, y1) = (w / 8 * k, (w / 8 * (k + 1)).min(w));
        let changed = self.write_rows(y0, y1);
        self.fade = 15;
        self.border = y1 as i32;
        self.step += 1;
        if self.step >= 8 {
            if let Some(inp) = self.pending.take() {
                for (i, b) in self.layer.iter_mut().enumerate() {
                    let s = inp.static_level[i] & 7;
                    *b = (*b & COUNTER) | s | s << 3;
                }
            }
            self.phase = 1;
            self.step = 0;
            self.fade_counter = 0;
        }
        changed.then_some((y0, y1.saturating_sub(1)))
    }

    /// `UpdateGfxLayerAndInitializeDynamicLayer` for rows `y0..y1`.
    fn write_rows(&mut self, y0: usize, y1: usize) -> bool {
        let w = self.width;
        let mut changed = false;
        for y in y0..y1 {
            for g in 0..w / 8 {
                let base = y * w + g * 8;
                let mut all_stable = true;
                let mut dyn_ = [0u8; 8];
                for (b, dj) in self.layer[base..base + 8].iter_mut().zip(&mut dyn_) {
                    let (s, d) = (*b & STATIC, (*b & DYNAMIC) >> 3);
                    let counter = if d == s { ((*b >> 6) + 1).min(3) } else { 0 };
                    all_stable &= counter == 3;
                    *dj = d;
                    *b = counter << 6 | s | s << 3;
                }
                if all_stable {
                    continue;
                }
                for (g, &d) in self.gfx[base..base + 8].iter_mut().zip(&dyn_) {
                    let n = (*g >> 3) & 7;
                    let step = if n == 7 { 7 } else { 3 };
                    let new = d.min(n.max(step));
                    let v = (*g & COUNTER) | new << 3 | n;
                    changed |= v != *g;
                    *g = v;
                }
            }
        }
        changed
    }
}

/// Hex distance on the S4 node grid (neighbours (±1,0), (0,±1), (1,1), (−1,−1)).
fn hex_dist(dx: i32, dy: i32) -> i32 {
    dx.abs().max(dy.abs()).max((dx - dy).abs())
}

/// Test pattern: a territory disc around `centre` (static levels by distance, as
/// `ClearDynamicFoggingAndCalcStatic` rings it) and three scouts circling it with sight radius 15
/// (`UpdateEntityFogging` rings beyond the radius).
pub fn test_pattern(width: usize, centre: (usize, usize), tick: u64) -> FogInput {
    let n = width * width;
    let (cx, cy) = (centre.0 as i32, centre.1 as i32);
    let territory = 18;
    let mut static_level = vec![7u8; n];
    let mut dynamic_level = vec![7u8; n];
    let t = tick as f32 * 0.071; // seconds at game speed 1
    let scouts: Vec<(i32, i32, i32)> = [(40.0f32, 0.11f32, 0.0f32), (60.0, -0.07, 2.0), (28.0, 0.19, 4.0)]
        .iter()
        .map(|&(r, w, p)| {
            let a = p + w * t;
            // Screen-round circle: x − y/2 and y/2 isotropic.
            let (ex, ey) = (r * a.cos(), r * a.sin());
            let y = ey * 1.0;
            let x = ex + y / 2.0;
            (cx + x as i32, cy + y as i32, 15)
        })
        .collect();
    for y in 0..width as i32 {
        for x in 0..width as i32 {
            let i = (y * width as i32 + x) as usize;
            let d = hex_dist(x - cx, y - cy) - territory;
            let s = d.clamp(0, 7) as u8;
            static_level[i] = s;
            let mut dl = s;
            for &(ux, uy, r) in &scouts {
                let k = hex_dist(x - ux, y - uy) - r;
                if k <= 6 {
                    dl = dl.min(k.max(0) as u8);
                }
            }
            dynamic_level[i] = dl;
        }
    }
    FogInput { static_level, dynamic_level }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(width: usize, s: u8, d: u8) -> FogInput {
        FogInput { static_level: vec![s; width * width], dynamic_level: vec![d; width * width] }
    }

    #[test]
    fn cycle_timing_and_levels() {
        let w = 16;
        let mut f = FogDriver::start(w, &flat(w, 7, 7));
        assert_eq!(f.gfx[0], 7 | 7 << 3);
        // Phase 1: fade 1..15 then holds.
        for t in 1..=32 {
            f.tick(|| flat(w, 7, 0));
            assert_eq!(f.fade, (t as u32).min(15));
        }
        // Phase 2: rows written slice by slice; from 7 the level jumps straight to D.
        let r = f.tick(|| unreachable!());
        assert_eq!(r, Some((0, 1)));
        assert_eq!((f.fade, f.border), (15, 2));
        assert_eq!(f.gfx[0], 7); // old 7, new 0
        assert_eq!(f.gfx[2 * w], 7 | 7 << 3); // not yet written
        for _ in 0..7 {
            f.tick(|| unreachable!());
        }
        assert_eq!(f.border, 16);
        // Next cycle: sight lost (D = S = 7) → dims to level 3 only.
        for _ in 0..32 {
            f.tick(|| flat(w, 7, 7));
        }
        f.tick(|| unreachable!());
        assert_eq!(f.gfx[0], 3 << 3); // old 0, new 3
    }
}
