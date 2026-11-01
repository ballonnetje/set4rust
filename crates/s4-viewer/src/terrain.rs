//! Terrain data for the GPU, built on the CPU from the original's per-triangle rules
//! (`s4_formats::gfx::landscape`, see `docs/TERRAIN.md`).
//!
//! Nothing per-vertex is stored. The GPU gets two storage buffers and builds every vertex in
//! the vertex shader from `vertex_index` ("vertex pulling"):
//! - `nodes`: the four `T_GFX_MAP_ELEMENT` bytes of every node, one `u32` each (height, ground,
//!   gradient, gfx bits). This gives position, shading and fog.
//! - `pieces`: two `u32`s per cell (x, y) in `0..w−1 × 0..h−1`, the texture piece of the Up and
//!   Down triangle chosen by `column_pieces` (page, tiled flag, pattern cell and triangle k).
//!   The shader turns them into UVs with the same formulas as `landscape::Piece::uv`.
//!
//! That is 12 bytes per node, i.e. 2 bytes per triangle-vertex equivalent (12 MB for 1024²),
//! against 24 bytes per vertex (150 MB) for a plain vertex buffer.
//!
//! Triangle `t` of the draw is cell `t / 2`, Up if `t` is even. Cells are row-major, so the
//! natural vertex order *is* the original's draw order (rows top to bottom, columns left to
//! right, Up before Down). A GPU keeps primitive order, so where a hill's front triangles overlap
//! terrain behind them the result equals the original's painter's order. (Overlaps occur on any
//! viewer-facing slope steeper than ~10 height units per row.) The draw is split into bands of
//! [`BAND_ROWS`] rows, drawn top to bottom, only for vertical culling.

use s4_formats::gfx::landscape::{Corner, DEFAULT_TRIANGLE_SIZE, Piece, PieceKind, column_pieces, height_offset_fixed, shade_index};

/// One node as the renderer needs it: the four bytes of `T_GFX_MAP_ELEMENT`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TerrainNode {
    pub height: u8,
    /// Raw ground type id.
    pub ground: u8,
    /// Byte 2 (`m_uGradient`): low nibble shading index, 0x40 dark land, 0x80 dark crossing.
    pub gradient: u8,
    /// Byte 3 (`m_uGfxBits`): bits 0..2 old fog, 3..5 new fog.
    pub gfx_bits: u8,
}

impl TerrainNode {
    pub fn dark(&self) -> bool {
        self.gradient & 0x40 != 0
    }
    pub fn pack(&self) -> u32 {
        u32::from_le_bytes([self.height, self.ground, self.gradient, self.gfx_bits])
    }
}

/// Where terrain data comes from.
/// `Sync`: a large re-mesh computes its pieces on several threads.
pub trait TerrainSource: Sync {
    /// Width and height in nodes.
    fn size(&self) -> (usize, usize);
    fn node(&self, x: usize, y: usize) -> TerrainNode;
}

pub const BAND_ROWS: usize = 16;
/// Row height and triangle width in zoom-1 pixels (camera mode 1: H = W/2).
pub const TRI_W: i32 = 24;
pub const ROW_H: i32 = 12;

/// World-space position of node (x, y) at zoom 1: `(24x − 12y, floor(12y − hoff(h)))`.
/// The y floor is what `DrawTexturedLandscape` does (`(rowY16 − hoff16) >> 16`). The shader
/// computes the same.
pub fn node_pos(x: usize, y: usize, h: u8) -> [f32; 2] {
    let sx = TRI_W * x as i32 - (TRI_W / 2) * y as i32;
    let y16 = ((ROW_H * y as i32) << 16) - height_offset_fixed(h, DEFAULT_TRIANGLE_SIZE);
    [sx as f32, (y16 >> 16) as f32]
}

/// `GetObjectFog(_, old, new)` (release 0x65f1d0; its first argument is unused): the fog brightness
/// objects are drawn with, 255 = unfogged. It reads the blue byte of the fogged diffuse of shade 8
/// (grey 255) from the table the constructor fills through `CalcFogging` (loop at 0x66b021;
/// `[fade 16][shade 16][old 8][new 8]` x (diffuse, specular) at 0x1569b44, shade 8 at 0x156ab44),
/// recomputed here. The fade is the row's step in global 0x146436c, which the object layer
/// loop 0x660070 sets per row (0x6603e6, as [`row_fade`]); here the caller passes it.
/// `RenderObjectLayer` draws deco objects above 17, settlers, buildings and waves above 58/59.
pub fn object_fog(fade: u32, old: u32, new: u32) -> u8 {
    const RAMP: [u32; 9] = [0x49, 0x00, 0x0a, 0x16, 0x23, 0x3f, 0x66, 0xa5, 0xff];
    let mix = old * (15 - fade) + new * fade;
    let f = 16 * mix / 15;
    if f == 0 {
        return 255;
    }
    let (idx, frac) = ((f >> 4) as usize, f & 15);
    let a = RAMP[8 - idx];
    let b = if idx == 8 { a } else { RAMP[7 - idx] };
    let lum = ((a * (15 - frac) + b * frac) >> 4).min(255);
    (255.0f32 * (lum as f32 / 255.0)) as u8
}

/// Piece word: bits 0..5 page, 6 tiled, 7..8 cell x, 9..11 cell y, 12..14 triangle k.
pub fn pack_piece(p: Piece) -> u32 {
    match p.kind {
        PieceKind::Tiled => p.page as u32 | 1 << 6,
        PieceKind::Pattern { cell_x, cell_y, k } => {
            p.page as u32 | (cell_x as u32) << 7 | (cell_y as u32) << 9 | (k as u32) << 12
        }
    }
}

/// Vertical extent of a band in world px (for culling).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Band {
    pub y_min: f32,
    pub y_max: f32,
}

/// Rows of `nodes`, `ao` and `pieces` that changed since the GPU last synced (inclusive ranges,
/// as they came: the upload joins those that touch, and only those).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Dirty {
    pub nodes: Vec<(usize, usize)>,
    pub ao: Vec<(usize, usize)>,
    pub pieces: Vec<(usize, usize)>,
}

/// `ranges` sorted, the overlapping or adjacent ones joined.
pub fn join_ranges(ranges: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let mut v = ranges.to_vec();
    v.sort_unstable();
    let mut out: Vec<(usize, usize)> = Vec::with_capacity(v.len());
    for (a, b) in v {
        match out.last_mut() {
            Some(l) if a <= l.1 + 1 => l.1 = l.1.max(b),
            _ => out.push((a, b)),
        }
    }
    out
}

pub struct TerrainMesh {
    pub width: usize,
    pub height: usize,
    /// `width × height` packed nodes.
    pub nodes: Vec<u32>,
    /// `(width−1) × (height−1) × 2` packed pieces (Up, Down per cell).
    pub pieces: Vec<u32>,
    pub bands: Vec<Band>,
    pub dirty: Dirty,
    /// Heightfield ambient occlusion per node (optional effect, see `effects.rs`), 255 = open.
    pub ao: Vec<u8>,
    /// The effect is on ([`TerrainMesh::set_ao`]): while off, `update_rows` leaves `ao` alone
    /// (most of a re-mesh's time) and marks it stale.
    ao_on: bool,
    ao_stale: bool,
}

impl TerrainMesh {
    pub fn build(src: &dyn TerrainSource) -> Self {
        let (width, height) = src.size();
        let nbands = height.saturating_sub(1).div_ceil(BAND_ROWS);
        let mut m = TerrainMesh {
            width,
            height,
            nodes: vec![0; width * height],
            pieces: vec![0; (width - 1) * (height - 1) * 2],
            bands: vec![Band::default(); nbands],
            dirty: Dirty::default(),
            ao: vec![255; width * height],
            // Off until the effect asks ([`TerrainMesh::set_ao`], from `Gpu::sync_terrain`):
            // computing it for a whole map took ~380 ms of every game start.
            ao_on: false,
            ao_stale: true,
        };
        m.update_rows(src, 0, height - 1);
        m
    }

    /// Switch the AO effect: turned on after changes made while off, it is computed again for
    /// the whole map (from the nodes' heights, the low byte).
    pub fn set_ao(&mut self, on: bool) {
        if on && self.ao_stale {
            let (w, h) = (self.width, self.height);
            let nodes = &self.nodes;
            crate::effects::heightfield_ao(w, h, &|x, y| nodes[y * w + x] as u8, 0, h - 1, &mut self.ao);
            self.dirty.ao.push((0, h - 1));
            self.ao_stale = false;
        }
        self.ao_on = on;
    }

    /// Re-read nodes of rows `y0..=y1` (heights, ground, gradient, fog bits all may have
    /// changed) and recompute everything they feed: pieces of cell rows `y0−1..=y1` (a cell uses
    /// its own row and the one below) and the extents of the bands containing them.
    /// When heights change through [`crate::terrain::Grid::set_height`], the gradient of the
    /// rows above and below changes too, so callers pass `y0−1..=y1+1`.
    pub fn update_rows(&mut self, src: &dyn TerrainSource, y0: usize, y1: usize) {
        let (w, h) = (self.width, self.height);
        let y1 = y1.min(h - 1);
        let t = crate::perf::span("terrain: nodes");
        for y in y0..=y1 {
            for x in 0..w {
                self.nodes[y * w + x] = src.node(x, y).pack();
            }
        }
        self.dirty.nodes.push((y0, y1));
        // AO of a node depends on heights up to AO_RADIUS rows away.
        let r = crate::effects::AO_RADIUS as usize;
        let (a0, a1) = (y0.saturating_sub(r), (y1 + r).min(h - 1));
        drop(t);
        let t = crate::perf::span("terrain: ao");
        if self.ao_on {
            crate::effects::heightfield_ao(w, h, &|x, y| src.node(x, y).height, a0, a1, &mut self.ao);
            self.dirty.ao.push((a0, a1));
        } else {
            self.ao_stale = true;
        }
        let (c0, c1) = (y0.saturating_sub(1), y1.min(h - 2));
        drop(t);
        let t = crate::perf::span("terrain: pieces");
        let corner = |x: usize, y: usize| {
            let n = src.node(x, y);
            Corner { ground: n.ground, dark: n.dark() }
        };
        let row = |y: usize, out: &mut [u32]| {
            for x in 0..w - 1 {
                let [up, down] = column_pieces(corner(x, y), corner(x, y + 1), corner(x + 1, y + 1), corner(x + 1, y), x & 1 == 1);
                out[x * 2] = pack_piece(up);
                out[x * 2 + 1] = pack_piece(down);
            }
        };
        let rows = &mut self.pieces[c0 * (w - 1) * 2..(c1 + 1) * (w - 1) * 2];
        // A whole map (the start, the first fog) on all cores: ~80 ms → ~15 on UnitedNations.
        let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(16);
        if c1 + 1 - c0 >= 128 && threads > 1 {
            let per = (c1 + 1 - c0).div_ceil(threads);
            std::thread::scope(|sc| {
                for (k, chunk) in rows.chunks_mut(per * (w - 1) * 2).enumerate() {
                    let row = &row;
                    sc.spawn(move || {
                        for (j, out) in chunk.chunks_mut((w - 1) * 2).enumerate() {
                            row(c0 + k * per + j, out);
                        }
                    });
                }
            });
        } else {
            for (j, out) in rows.chunks_mut((w - 1) * 2).enumerate() {
                row(c0 + j, out);
            }
        }
        self.dirty.pieces.push((c0, c1));
        drop(t);
        let _t = crate::perf::span("terrain: bands");
        for b in c0 / BAND_ROWS..=c1 / BAND_ROWS {
            let rows = b * BAND_ROWS..=((b + 1) * BAND_ROWS).min(h - 1);
            let (mut lo, mut hi) = (f32::MAX, f32::MIN);
            for y in rows {
                for x in 0..w {
                    let p = node_pos(x, y, self.nodes[y * w + x] as u8);
                    lo = lo.min(p[1]);
                    hi = hi.max(p[1]);
                }
            }
            self.bands[b] = Band { y_min: lo, y_max: hi };
        }
    }

    pub fn triangle_count(&self) -> usize {
        self.pieces.len()
    }

    /// Cell-row range of band `b`.
    pub fn band_rows(&self, b: usize) -> std::ops::Range<usize> {
        b * BAND_ROWS..((b + 1) * BAND_ROWS).min(self.height - 1)
    }

    /// GPU bytes held per node (nodes + pieces).
    pub fn bytes(&self) -> usize {
        (self.nodes.len() + self.pieces.len()) * 4
    }
}

// ------------------------------------------------------------------------------------------------
// Node grids: real maps and the synthetic test grid
// ------------------------------------------------------------------------------------------------

/// A mutable node grid (row-major).
pub struct Grid {
    pub w: usize,
    pub h: usize,
    pub nodes: Vec<TerrainNode>,
}

impl Grid {
    /// Chunk 13 of a `.map` file, bytes taken verbatim (`CWorldManager::LoadGfxData` copies the
    /// chunk into the render map unchanged, so the stored gradient is what the game shades with).
    pub fn from_map(path: &std::path::Path) -> anyhow::Result<Self> {
        Self::from_file(&s4_formats::map::MapFile::open(path)?)
    }

    pub fn from_file(map: &s4_formats::map::MapFile) -> anyhow::Result<Self> {
        let l = map.landscape()?;
        let nodes = l
            .nodes
            .iter()
            .map(|n| TerrainNode { height: n.height, ground: n.ground, gradient: n.gradient, gfx_bits: n.gfx_bits })
            .collect();
        Ok(Grid { w: l.width as usize, h: l.height as usize, nodes })
    }

    /// Same grid as `s4-formats/examples/landscape_dump.rs::synthetic_map` (so the GPU output
    /// can be compared with the software rasteriser), with the gradient computed by
    /// `RefreshShading` for every node.
    pub fn synthetic() -> Self {
        let (w, h) = (40usize, 36usize);
        let mut nodes = vec![TerrainNode::default(); w * h];
        for y in 0..h {
            for x in 0..w {
                let sx = x as f32 - y as f32 / 2.0;
                let mut ty = if sx < 10.0 {
                    16
                } else if sx < 13.0 {
                    48
                } else {
                    (((sx - 13.0) / 1.5) as i32).clamp(0, 7) as u8
                };
                let (dx, dy) = (sx - 1.0, y as f32 - 16.0);
                let r2 = dx * dx + dy * dy * 0.5;
                let hill = 45.0 * (-r2 / 60.0).exp();
                let base = if ty == 16 {
                    6.0
                } else if ty == 48 {
                    3.0
                } else {
                    0.0
                };
                let height = (base + hill).round() as u8;
                if r2 < 6.0 {
                    ty = 32;
                } else if r2 < 14.0 {
                    ty = 33;
                } else if r2 < 24.0 {
                    ty = 17;
                }
                nodes[y * w + x] = TerrainNode { height, ground: ty, gradient: 0, gfx_bits: 0 };
            }
        }
        let mut g = Grid { w, h, nodes };
        for y in 0..h {
            for x in 0..w {
                g.refresh_shading(x, y);
            }
        }
        g
    }

    /// `RefreshShading` (rel 0x66d7b0) for one node: low nibble of byte 2 from the heights of
    /// (x, y−1) and (x, y+1); the other bits are kept.
    pub fn refresh_shading(&mut self, x: usize, y: usize) {
        let i = y * self.w + x;
        let hh = self.nodes[i].height;
        let up = if y > 0 { self.nodes[i - self.w].height } else { hh };
        let down = if y + 1 < self.h { self.nodes[i + self.w].height } else { hh };
        let n = &mut self.nodes[i];
        n.gradient = (n.gradient & 0xF0) | shade_index(up, down);
    }

    /// Change one node's height the way the game does (`IGfxEngine::UpdateWorldPosition` →
    /// `RefreshShading(i, false)`: the node and its vertical neighbours are re-shaded).
    pub fn set_height(&mut self, x: usize, y: usize, height: u8) {
        self.nodes[y * self.w + x].height = height;
        self.refresh_shading(x, y);
        if y > 0 {
            self.refresh_shading(x, y - 1);
        }
        if y + 1 < self.h {
            self.refresh_shading(x, y + 1);
        }
    }

    /// Flatten a disc of radius `r` nodes around (cx, cy) to `height` (stand-in for the
    /// simulation's building-site levelling). Returns the changed row range.
    pub fn flatten(&mut self, cx: usize, cy: usize, r: usize, height: u8) -> (usize, usize) {
        let (y0, y1) = (cy.saturating_sub(r), (cy + r).min(self.h - 1));
        for y in y0..=y1 {
            for x in cx.saturating_sub(r)..=(cx + r).min(self.w - 1) {
                let (dx, dy) = (x as i32 - cx as i32, y as i32 - cy as i32);
                if dx * dx + dy * dy <= (r * r) as i32 {
                    self.set_height(x, y, height);
                }
            }
        }
        (y0, y1)
    }
}

/// Runs `(first, last)` covering the ascending `rows`, joined across gaps of up to `gap` rows.
pub fn row_runs(rows: &[usize], gap: usize) -> Vec<(usize, usize)> {
    let mut runs: Vec<(usize, usize)> = Vec::new();
    for &y in rows {
        match runs.last_mut() {
            Some(r) if y <= r.1 + gap + 1 => r.1 = r.1.max(y),
            _ => runs.push((y, y)),
        }
    }
    runs
}

/// Fade step a row is drawn with: `g_iFogFadeStep`, or 0 above `g_iFadeYBorder`.
pub fn row_fade(y: usize, fade: u32, border: i32) -> u32 {
    if (y as i64) < border as i64 { 0 } else { fade }
}

/// A grid as the landscape pass sees it under fog: roads (28/29) whose `CalcCurrentFog` is
/// <= 59 are drawn as grass (16) (`DrawTexturedLandscape`).
pub struct FogView<'a> {
    pub grid: &'a Grid,
    pub fade: u32,
    pub border: i32,
}

impl FogView<'_> {
    pub fn road_hidden(&self, x: usize, y: usize) -> bool {
        let n = self.grid.node(x, y);
        let g = n.gfx_bits as u32;
        matches!(n.ground, 28 | 29) && object_fog(row_fade(y, self.fade, self.border), g & 7, (g >> 3) & 7) <= 59
    }
}

impl TerrainSource for FogView<'_> {
    fn size(&self) -> (usize, usize) {
        self.grid.size()
    }
    fn node(&self, x: usize, y: usize) -> TerrainNode {
        let mut n = self.grid.node(x, y);
        if self.road_hidden(x, y) {
            n.ground = 16;
        }
        n
    }
}

impl TerrainSource for Grid {
    fn size(&self) -> (usize, usize) {
        (self.w, self.h)
    }
    fn node(&self, x: usize, y: usize) -> TerrainNode {
        self.nodes[y * self.w + x]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesh_counts() {
        let s = Grid::synthetic();
        let m = TerrainMesh::build(&s);
        assert_eq!(m.triangle_count(), 2 * (s.w - 1) * (s.h - 1));
        assert_eq!(m.nodes[0], s.node(0, 0).pack());
    }

    #[test]
    fn dirty_rebuild_matches_full_build() {
        let mut s = Grid::synthetic();
        let mut m = TerrainMesh::build(&s);
        let (y0, y1) = s.flatten(5, 15, 3, 20);
        m.update_rows(&s, y0.saturating_sub(1), y1 + 1);
        let full = TerrainMesh::build(&s);
        assert_eq!(m.nodes, full.nodes);
        assert_eq!(m.pieces, full.pieces);
        assert_eq!(m.bands, full.bands);
    }
}
