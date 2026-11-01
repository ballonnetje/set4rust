//! The minimap, as `DrawMinimap2ToSurface` (release 0x6654b0, debug 0x2f8d020), its per-frame
//! updates (0x665250, 0x664f80, 0x665ce0) and `MarkCurrentArea` (0x665940) draw it.
//!
//! # Placement
//! `CGameStateHandler` calls `IGfxEngine::EnableMiniMap(1, 15, 8, …)`: a 240×160 surface at (15, 8)
//! of the screen, i.e. inside the minimap frame (GUI slot 0), whose colour-keyed hole shows it.
//!
//! # Projection
//! The minimap samples the map on a 160×160 grid, `k = Size / 160` (`SetMiniMapGradient`):
//! minimap row `i` is map row `(int)(i·k + 0.5)`, column `j` the node `(int)(j·k + 0.5)` of it
//! (accumulated in floats as the original does). Row `i` starts at pixel `80 − ⌊i/2⌋`: each
//! surface row moves one pixel left every other row, the same 2:1 shear as the main view, so the
//! square map becomes a 240×160 parallelogram.
//!
//! # Colours (landscape layer)
//! Per sampled node: the ground colour bytes of `CLandscapeColorProperties` ([`ground_rgb`]; the
//! release's initialised table at 0x11f6b40, 3 bytes per type, equal for all 35 types) plus
//! `8·gradient − 64` (the node's shading index, byte 2 & 15 via 0x54ec30, so slopes facing the
//! light are brighter); dark land
//! (byte 2 & 0x40) becomes grey `(2b + g + r) / 4`; new fog 7 → (9, 9, 9), other non-zero fog
//! halves the colour; every channel clamped to 9..=255; stored as RGB565
//! (`(int)(v · 31/255)`, `(int)(v · 63/255)`, truncated: `FloatingPointCheck(0x300, 0x300)` sets
//! round-to-chop), which we expand back to 8 bits. The `MINIMAP_COLORS` config section only
//! colours the object layer's groups.
//!
//! # Object layer
//! Every node is mapped to `(⌊x·160/Size + 0.5⌋, ⌊y·160/Size + 0.5⌋)`. The colour callback
//! (`CMinimapHandler::GetObjectColor`: buildings, settlers, … by group) only answers for groups the
//! player switched on with the minimap toggles, and `CMinimapHandler::Init` starts with every group
//! off. Otherwise a node with the border-stone bit (byte 3 & 0x80) and fog < 3 is painted in its
//! owner's player colour. So at the start of a game the minimap shows the land and the borders.
//!
//! # Updates
//! Drawn whole once when the engine starts, then one pixel of each 8 × 8 block per rendered frame
//! ([`Minimap`]): ground changes (path wear), fog and borders reach it within 64 frames.
//!
//! # View rectangle
//! `MarkCurrentArea` (release 0x665940) outlines in white, on its own surface, the area the main
//! view shows, whenever the camera is set: the top-left at the view's whole node
//! (`viewportX/Y`, 0x658ad0) times `160/Size`, the size the output in whole nodes times
//! `160/Size` (0x665ba0); the edges are axis-aligned on the surface and clipped to the
//! parallelogram's rows ([`mark_area`]).
//!
//! # Clicks (s4-game, `EventManagement` 0x5172f0)
//! A left press on it jumps the camera (case 7, [`crate::client::view_from_minimap`]); moves with
//! the button held do nothing (case 5 while +0xd4 is set). A right press sends the selection
//! there (case 9, [`crate::client::minimap_order_node`]).

use s4_formats::map::ground_rgb;

use crate::sprites::PLAYER_COLORS;

pub const WIDTH: u32 = 240;
pub const HEIGHT: u32 = 160;
/// `EnableMiniMap(1, 15, 8, …)`.
pub const POS: (i32, i32) = (15, 8);

/// Node data the minimap reads: (ground, byte 2, byte 3) and the territory owner.
pub struct Nodes<'a> {
    pub size: usize,
    pub node: &'a dyn Fn(usize) -> (u8, u8, u8),
    pub owner: &'a [u8],
    /// The border-stone bit (byte 3 & 0x80, `CTiling::CalculateBorderstoneBit`).
    pub border: &'a dyn Fn(usize) -> bool,
    /// The object layer's colour of a node ([`ObjectLayer::color`]), `None` for nothing.
    pub object: &'a dyn Fn(usize) -> Option<[u8; 3]>,
}

/// The landscape and border layers of the minimap (RGBA, 240×160; outside the parallelogram
/// transparent), drawn whole (0x6654b0, see [`Minimap`]).
pub fn render(m: &Nodes) -> Vec<u8> {
    let mut mm = Minimap::new(m.size);
    mm.full_draw(m);
    mm.img
}

/// A landscape pixel of node `n` (0x6654b0 / 0x664f80): the ground colour plus `8·gradient −
/// 64`, dark land grey, fog 7 → 9, other fog halved, clamped to 9..=255.
fn land_rgb(m: &Nodes, n: usize) -> [u8; 3] {
    let (ground, byte2, byte3) = (m.node)(n);
    let grad = (byte2 & 15) as i32;
    let base = ground_rgb(ground).unwrap_or([0; 3]).map(i32::from);
    let d = 8 * grad - 64;
    let (mut r, mut g, mut b) = (base[0] + d, base[1] + d, base[2] + d);
    if byte2 & 0x40 != 0 {
        b = (b + b + g + r) / 4;
        g = b;
        r = b;
    }
    let fog = (byte3 >> 3) & 7;
    if fog == 7 {
        (r, g, b) = (9, 9, 9);
    } else if fog != 0 {
        (r, g, b) = (r / 2, g / 2, b / 2);
    }
    rgb565([r, g, b].map(|c| c.clamp(9, 255)))
}

/// A border stone (byte 3 & 0x80) the player can see (fog below 3) in its owner's colour: what
/// the object layer shows where `_MinimapIndexedColorLookup` answers nothing.
fn border_rgb(m: &Nodes, n: usize) -> Option<[u8; 3]> {
    if !(m.border)(n) || ((m.node)(n).2 >> 3) & 7 >= 3 {
        return None;
    }
    let own = m.owner[n] as usize;
    (own != 0).then(|| rgb565(PLAYER_COLORS[(own - 1) & 7].map(|v| v as i32)))
}

/// The minimap's object groups (`MinimapIndexedColorLookup` 0x60da80, tables set up by 0x59a820
/// and switched by 0x59a9a0 from `MinimapViewFlags` 0x15aabbc, the four toggles on dialog 0):
///
/// | flag | toggle | entity types | colour |
/// |---|---|---|---|
/// | 0x1 | 614 (0x266) | buildings (8, 10) | `other` |
/// | 0x2 | 616 (0x268) | soldiers (0x16), ships (2), vehicles (4) | `own` / `ally` / `foe` |
/// | 0x4 | 615 (0x267) | settlers (1) | `other` |
/// | 0x8 | 617 (0x269) | the territory (pseudo type 0x7f) | the owner's player colour |
///
/// The colours are `MINIMAP_COLORS` (`R[4]`, `G[4]`, `B[4]`: own, ally, foe, other; static
/// default 255, 255, 255, 191 each, 0x425160; the shipped `Config\MainConfig.cfg` sets them).
/// `own` / `ally` / `foe` (`Minimap::Update` 0x59a9e0): the local player, a player of the same
/// team (0x12dcd0c), any other. Every flag starts clear (dialog 0's init, 0x49b70a).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectLayer {
    pub flags: u32,
    /// own, ally, foe, other.
    pub colors: [[u8; 3]; 4],
}

impl Default for ObjectLayer {
    fn default() -> Self {
        ObjectLayer { flags: 0, colors: [[255; 3], [255; 3], [255; 3], [191; 3]] }
    }
}

impl ObjectLayer {
    /// The colours of `Config\MainConfig.cfg`'s `[MINIMAP_COLORS]` section (the defaults where it
    /// is missing).
    pub fn from_config(text: &str) -> ObjectLayer {
        let mut l = ObjectLayer::default();
        let mut inside = false;
        for line in text.lines() {
            let t = line.trim();
            if t.starts_with('[') {
                inside = t.eq_ignore_ascii_case("[MINIMAP_COLORS]");
                continue;
            }
            if !inside || t.starts_with(';') {
                continue;
            }
            let Some((key, vals)) = t.split_once('=') else { continue };
            let ch = match key.trim().chars().next() {
                Some('R') => 0,
                Some('G') => 1,
                Some('B') => 2,
                _ => continue,
            };
            for (i, v) in vals.split(',').filter_map(|v| v.trim().parse::<i32>().ok()).take(4).enumerate() {
                l.colors[i][ch] = v.clamp(0, 255) as u8;
            }
        }
        l
    }

    /// The installed game's colours (`Config\MainConfig.cfg`).
    pub fn load() -> ObjectLayer {
        std::fs::read(s4_formats::path::resolve(s4_formats::game_dir().join("Config").join("MainConfig.cfg"))).map(|b| ObjectLayer::from_config(&String::from_utf8_lossy(&b))).unwrap_or_default()
    }

    /// 0x60da80 for a node: `obj` its entity (see `Population::minimap_obj`), `owner` its
    /// territory owner (the eco sector's, for the territory group), `local` the local player and
    /// `teams` the players' teams. The colour as the minimap stores it, or `None`.
    pub fn color(&self, obj: u16, owner: u8, local: u8, teams: &[u8; 9]) -> Option<[u8; 3]> {
        if obj != 0 {
            let (ty, who) = (obj & 0xff, (obj >> 8) as u8);
            let group = match ty {
                8 | 10 => Some((0x1, false)),
                2 | 4 | 0x16 => Some((0x2, true)),
                1 => Some((0x4, false)),
                _ => None,
            };
            if let Some((bit, relation)) = group
                && self.flags & bit != 0
            {
                let c = if !relation {
                    self.colors[3]
                } else {
                    let who = if who == 0xff { local } else { who };
                    let team = |p: u8| teams.get(p as usize).copied().unwrap_or(0);
                    if who == local {
                        self.colors[0]
                    } else if team(who) == team(local) {
                        self.colors[1]
                    } else {
                        self.colors[2]
                    }
                };
                return Some(rgb565(c.map(i32::from)));
            }
        }
        (self.flags & 0x8 != 0 && owner != 0).then(|| rgb565(PLAYER_COLORS[(owner as usize - 1) & 7].map(|v| v as i32)))
    }
}

/// Side of the per-pixel cell arrays (0x6684 bytes = 162²), indexed `x · 162 + y`.
const CELLS: usize = 162;

/// The minimap as the release keeps it current (`IGfxEngine::RenderFrame` 0x659080):
/// * once, when the engine starts (`UpdateMinimapBackground` 0x116928c, set only by the engine
///   inits 0x65b6b0 / 0x65bef0), the whole surface (0x6654b0): the landscape of every pixel,
///   then every node's object or border stone;
/// * then every rendered frame one pixel of each 8 × 8 block: the objects of the nodes at the
///   current block position (0x665250, map rows and columns), the landscape of the pixels there
///   whose cell shows no object (0x664f80), then the next position (0x665ce0: column + 11,
///   carried into the row, both mod 8).
///
/// A shown border stone keeps its pixel for one round of visits (the cell counts them down from
/// the template 0x15fafb8, 0x665bf0: how many nodes map to the pixel); then it is checked again,
/// and when it is gone the landscape comes back. So path wear, fog and border changes reach the
/// minimap within 64 frames, block by block.
pub struct Minimap {
    size: usize,
    /// 0x11695b4 = 160 / size, 0x11695bc = size / 160, 0x11695b0 = that · 8 (0x665b60).
    shear: f32,
    k: f32,
    k8: f32,
    img: Vec<u8>,
    /// 0x15f4930: visits left before the pixel's object is checked again (0xff: an object, every
    /// visit); 0: the landscape shows.
    cells: Vec<u8>,
    /// 0x15fafb8.
    template: Vec<u8>,
    /// `someMinimapOffset` 0x145b28c (row) and 0x145b290 (column), 0..8.
    row: usize,
    col: usize,
}

impl Minimap {
    pub fn new(size: usize) -> Minimap {
        let n = size as f32;
        let (shear, k) = (160.0 / n, n / 160.0);
        let mut template = vec![0u8; CELLS * CELLS];
        for r in 0..size {
            let y = (r as f32 * shear + 0.5) as usize;
            for c in 0..size {
                let x = (c as f32 * shear + 0.5) as usize;
                if let Some(t) = template.get_mut(x * CELLS + y) {
                    *t = t.wrapping_add(1);
                }
            }
        }
        Minimap { size, shear, k, k8: k * 8.0, img: vec![0u8; (WIDTH * HEIGHT * 4) as usize], cells: vec![0; CELLS * CELLS], template, row: 0, col: 0 }
    }

    pub fn image(&self) -> &[u8] {
        &self.img
    }

    fn put(&mut self, px: i32, py: i32, rgb: [u8; 3]) {
        if px >= 0 && py >= 0 && (px as u32) < WIDTH && (py as u32) < HEIGHT {
            let o = ((py as u32 * WIDTH + px as u32) * 4) as usize;
            self.img[o..o + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
    }

    /// Where a landscape pixel row's nodes start: map row `(int)(i · k + 0.5)` (0x6654b0 /
    /// 0x664f80; the columns are then accumulated in floats).
    fn row_base(&self, i: usize) -> f32 {
        ((i as f32 * self.k + 0.5) as usize * self.size) as f32
    }

    /// The pixel (x, y) of node (row r, column c) in the object layer.
    fn node_pixel(&self, r: usize, c: usize) -> (usize, usize) {
        ((c as f32 * self.shear + 0.5) as usize, (r as f32 * self.shear + 0.5) as usize)
    }

    /// 0x6654b0: everything.
    pub fn full_draw(&mut self, m: &Nodes) {
        let last = self.size * self.size - 1;
        for i in 0..160usize {
            let mut v = self.row_base(i);
            for j in 0..160usize {
                let n = ((v + 0.5) as usize).min(last);
                v += self.k;
                let rgb = land_rgb(m, n);
                self.put(80 - i as i32 / 2 + j as i32, i as i32, rgb);
            }
        }
        self.cells.iter_mut().for_each(|c| *c = 0);
        for r in 0..self.size {
            for c in 0..self.size {
                self.object(m, r, c, true);
            }
        }
    }

    /// The object layer of node (r, c): an object of a group that is on (fog below 3) or else a
    /// border stone sets its pixel and the cell's countdown (`full`: the full draw, which does
    /// not count down first and marks an object 0xff, checked at every visit).
    fn object(&mut self, m: &Nodes, r: usize, c: usize, full: bool) {
        let (x, y) = self.node_pixel(r, c);
        let i = x * CELLS + y;
        if i >= self.cells.len() {
            return;
        }
        if !full {
            let mut u = self.cells[i];
            let mut due = u == 0;
            if !due {
                u -= 1;
                self.cells[i] = u;
                due = u == 0;
            }
            if !(due || u == 0xfe) {
                return;
            }
        }
        let n = r * self.size + c;
        if let Some(rgb) = (m.object)(n) {
            if ((m.node)(n).2 >> 3) & 7 < 3 {
                self.cells[i] = if full { 0xff } else { self.template[i] };
                self.put(80 - y as i32 / 2 + x as i32, y as i32, rgb);
            }
        } else if let Some(rgb) = border_rgb(m, n) {
            self.cells[i] = self.template[i];
            self.put(80 - y as i32 / 2 + x as i32, y as i32, rgb);
        }
    }

    /// One rendered frame (0x665250, 0x664f80, 0x665ce0).
    pub fn frame(&mut self, m: &Nodes) {
        let size = self.size;
        let mut r = self.row;
        while r < size {
            let mut c = self.col;
            while c < size {
                self.object(m, r, c, false);
                c += 8;
            }
            r += 8;
        }
        let last = size * size - 1;
        let start = self.col as f32 * self.k;
        let mut i = self.row;
        while i < 160 {
            let mut v = self.row_base(i) + start;
            let mut j = self.col;
            while j < 160 {
                if self.cells[j * CELLS + i] == 0 {
                    let n = ((v + 0.5) as usize).min(last);
                    let rgb = land_rgb(m, n);
                    self.put(80 - i as i32 / 2 + j as i32, i as i32, rgb);
                }
                v += self.k8;
                j += 8;
            }
            i += 8;
        }
        self.col += 11;
        let carry = self.col >> 3;
        self.col -= carry * 8;
        for _ in 0..carry {
            self.row = (self.row + 1) % 8;
        }
    }
}

/// RGB565 the way the original stores minimap pixels, expanded back to 8 bits.
fn rgb565([r, g, b]: [i32; 3]) -> [u8; 3] {
    let (r5, g6, b5) = ((r as f32 * 0.121_568_63) as u32, (g as f32 * 0.247_058_82) as u32, (b as f32 * 0.121_568_63) as u32);
    [(r5 << 3 | r5 >> 2) as u8, (g6 << 2 | g6 >> 4) as u8, (b5 << 3 | b5 >> 2) as u8]
}

/// `MarkCurrentArea` (release 0x665940): the main view's outline in white, on its own surface
/// over the minimap (cleared first, vtbl+0x14), redrawn whenever the camera is set
/// (`SetCameraPosition` 0x658ad0, the zoom 0x658bc0). `view` = (`viewportX`, `viewportY`), the
/// view's top-left in nodes; `rect` = (0x11695b8, 0x11695ac), its width and height on the
/// minimap ([`crate::client::minimap_view`]). The edges are walked clockwise from the top-left:
/// the right edge goes straight down while the parallelogram's row window moves one pixel left
/// every second step (and back on the left edge), and a pixel is drawn only inside that window
/// and when the edge's start lies on the surface, as the original does.
pub fn mark_area(img: &mut [u8], size: usize, view: (i32, i32), rect: (i32, i32)) {
    let shear = 160.0 / size as f32;
    let c = WIDTH as i32;
    // The surface bounds the original tests the pixel pointer against (exclusive): the start
    // and row 159, x 160.
    let limit = c * 159 + 160;
    let mut plot = |p: i32| {
        if (0..(WIDTH * HEIGHT) as i32).contains(&p) {
            let o = p as usize * 4;
            img[o..o + 4].copy_from_slice(&[255, 255, 255, 255]);
        }
    };
    let inside = |p: i32| 0 < p && p < limit;
    // `ROUND` under `FloatingPointCheck(0x300, 0x300)`: truncation.
    let vy = (view.1 as f32 * shear) as i32;
    let mut lo = -(vy / 2);
    let mut hi = lo + 160;
    let mut x = (view.0 as f32 * shear) as i32;
    let mut p = vy * c + 80 + x;
    let (w, h) = (rect.0.max(0), rect.1.max(0));
    if inside(p) {
        for _ in 0..w {
            if x <= hi && lo < x {
                plot(p);
            }
            p += 1;
            x += 1;
        }
    } else {
        x += w;
        p += w;
    }
    p -= 1;
    x -= 1;
    for u in 0..h {
        if inside(p) && x <= hi && lo < x {
            plot(p);
        }
        p += c;
        if u & 1 != 0 {
            hi -= 1;
            lo -= 1;
        }
    }
    p -= c;
    if inside(p) {
        for _ in 0..w {
            if x <= hi && lo < x {
                plot(p);
            }
            p -= 1;
            x -= 1;
        }
    } else {
        x -= w;
        p -= w;
    }
    p += 1;
    for u in 0..h {
        if inside(p) && x < hi && lo <= x {
            plot(p);
        }
        p -= c;
        if u & 1 != 0 {
            lo += 1;
            hi += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The offsets (0x665ce0) visit all 64 block positions in 64 frames, so a changed landscape
    /// is on the minimap after 64 frames and only partly before.
    #[test]
    fn a_landscape_change_is_drawn_within_64_frames() {
        let size = 256;
        let ground = std::cell::RefCell::new(vec![0x10u8; size * size]);
        let node = |n: usize| (ground.borrow()[n], 8u8, 0u8);
        let owner = vec![0u8; size * size];
        let border = |_: usize| false;
        let none = |_: usize| None;
        let nodes = Nodes { size, node: &node, owner: &owner, border: &border, object: &none };
        let mut m = Minimap::new(size);
        m.full_draw(&nodes);
        let before = m.image().to_vec();
        ground.borrow_mut().iter_mut().for_each(|g| *g = 0x1d);
        let changed = |m: &Minimap| m.image().chunks(4).zip(before.chunks(4)).filter(|(a, b)| a[3] != 0 && a != b).count();
        let mut seen = std::collections::HashSet::new();
        for f in 0..64 {
            seen.insert((m.row, m.col));
            m.frame(&nodes);
            if f == 31 {
                let n = changed(&m);
                assert!(n > 0 && n < 160 * 160 * 2 / 3, "half way: {n} pixels");
            }
        }
        assert_eq!(seen.len(), 64, "every block position once");
        assert_eq!(changed(&m), 160 * 160, "all landscape pixels after 64 frames");
    }

    /// A border stone stays while it exists (each visit draws it again), and its pixel goes back
    /// to the landscape at the next visit of its block after it is gone.
    #[test]
    fn a_border_stone_pixel_lasts_one_round() {
        let size = 160;
        let flag = std::cell::Cell::new(true);
        let node = |_: usize| (0x10u8, 8u8, 0u8);
        let owner = vec![1u8; size * size];
        let at = 80 * size + 80;
        let border = |n: usize| n == at && flag.get();
        let none = |_: usize| None;
        let nodes = Nodes { size, node: &node, owner: &owner, border: &border, object: &none };
        let mut m = Minimap::new(size);
        m.full_draw(&nodes);
        let px = |m: &Minimap| {
            let o = ((80 * WIDTH + (80 - 40 + 80)) * 4) as usize;
            m.image()[o..o + 3].to_vec()
        };
        let stone = px(&m);
        for _ in 0..130 {
            m.frame(&nodes);
            assert_eq!(px(&m), stone, "the stone stays");
        }
        flag.set(false);
        let mut back = None;
        for f in 0..200 {
            m.frame(&nodes);
            if px(&m) != stone {
                back = Some(f);
                break;
            }
        }
        // One node per pixel at 160 (template 1): its next visit, within one 64-frame round.
        assert!(back.is_some_and(|f| f < 64), "{back:?}");
    }
}
