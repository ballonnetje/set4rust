//! Mip chain for the landscape pages (opt-in, for zoomed-out views).
//!
//! The transition pages are atlases: hexagonal pattern cells on filler colour (magenta 0xF81F,
//! cyan, white, black blocks). Averaging naively would bleed filler into the cell edges at
//! lower mips. So first every texel no triangle can sample ("unused") is replaced by the colour
//! of its nearest used texel (multi-source BFS), then the levels are 2×2 box-filtered on the raw
//! 8-bit values (the renderer does no gamma handling either).
//! "Used" = whole page for tiling pages, otherwise the six triangles of every pattern cell the
//! transition table (or the river mouth) references, rasterised from `pattern_uv`.

use s4_formats::gfx::landscape::{TILE_PAGE, TexturePage, pattern_uv, transition_table};
use std::collections::VecDeque;

/// Per page: used-texel mask (`size²`).
fn used_masks(pages: &[TexturePage]) -> Vec<Vec<bool>> {
    let size = pages[0].size as usize;
    let mut tiled = [false; 22];
    for &p in TILE_PAGE.iter() {
        // Page 3 only tiles for type 250 (unused); it is a pattern page.
        if p >= 0 && p != 3 {
            tiled[p as usize] = true;
        }
    }
    let mut cells: Vec<(u8, u8, u8)> = vec![(5, 2, 2), (5, 3, 2)]; // river mouth
    for row in transition_table() {
        for e in row {
            if e.page != 0 || e.cell_x != 0 || e.cell_y != 0 {
                cells.push((e.page, e.cell_x, e.cell_y));
                if e.alternate {
                    cells.push((e.page, e.cell_x, e.cell_y + 1));
                }
            }
        }
    }
    let mut base = vec![vec![false; size * size]; 22];
    for (p, mask) in base.iter_mut().enumerate() {
        if tiled[p] {
            mask.fill(true);
        }
    }
    for (p, cx, cy) in cells {
        let mask = &mut base[p as usize];
        for k in 0..6 {
            let t = pattern_uv(cx, cy, k).map(|[u, v]| [u * size as f32, v * size as f32]);
            let edge = |a: [f32; 2], b: [f32; 2], x: f32, y: f32| (b[0] - a[0]) * (y - a[1]) - (b[1] - a[1]) * (x - a[0]);
            let area = edge(t[0], t[1], t[2][0], t[2][1]);
            let (x0, x1) = (t.iter().map(|v| v[0]).fold(f32::MAX, f32::min), t.iter().map(|v| v[0]).fold(f32::MIN, f32::max));
            let (y0, y1) = (t.iter().map(|v| v[1]).fold(f32::MAX, f32::min), t.iter().map(|v| v[1]).fold(f32::MIN, f32::max));
            for y in (y0.floor() as usize)..(y1.ceil() as usize).min(size) {
                for x in (x0.floor() as usize)..(x1.ceil() as usize).min(size) {
                    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                    let w = [edge(t[1], t[2], px, py), edge(t[2], t[0], px, py), edge(t[0], t[1], px, py)];
                    if w.iter().all(|&e| e * area.signum() >= 0.0) {
                        mask[y * size + x] = true;
                    }
                }
            }
        }
    }
    // Dark-land pages 22..44 share the layout of 0..22.
    (0..pages.len()).map(|i| base[i % 22].clone()).collect()
}

/// Replace unused texels by their nearest used texel's colour (wrapping, 4-neighbour BFS).
fn pad(page: &TexturePage, mask: &[bool]) -> Vec<u8> {
    let s = page.size as usize;
    let mut out = page.rgba.clone();
    let mut done = mask.to_vec();
    let mut q: VecDeque<usize> = (0..s * s).filter(|&i| done[i]).collect();
    if q.is_empty() {
        return out;
    }
    while let Some(i) = q.pop_front() {
        let (x, y) = (i % s, i / s);
        for (nx, ny) in [((x + 1) % s, y), ((x + s - 1) % s, y), (x, (y + 1) % s), (x, (y + s - 1) % s)] {
            let j = ny * s + nx;
            if !done[j] {
                done[j] = true;
                out.copy_within(i * 4..i * 4 + 4, j * 4);
                q.push_back(j);
            }
        }
    }
    out
}

fn halve(src: &[u8], s: usize) -> Vec<u8> {
    let h = s / 2;
    let mut out = vec![0u8; h * h * 4];
    for y in 0..h {
        for x in 0..h {
            for c in 0..4 {
                let at = |xx: usize, yy: usize| src[(yy * s + xx) * 4 + c] as u32;
                let sum = at(2 * x, 2 * y) + at(2 * x + 1, 2 * y) + at(2 * x, 2 * y + 1) + at(2 * x + 1, 2 * y + 1);
                out[(y * h + x) * 4 + c] = ((sum + 2) / 4) as u8;
            }
        }
    }
    out
}

/// [`padded_mips`] of a page set, computed once per process: every game uses the same pages
/// and the levels took ~60 ms of each start. Keyed by the pages' count and a sample of their bytes.
pub fn cached_padded_mips(pages: &[TexturePage]) -> std::sync::Arc<Vec<Vec<Vec<u8>>>> {
    type Cache = Option<(usize, u64, std::sync::Arc<Vec<Vec<Vec<u8>>>>)>;
    static CACHE: std::sync::Mutex<Cache> = std::sync::Mutex::new(None);
    let key = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        // A sample of every page (hashing all 11 MB took longer than it saves).
        for p in pages {
            p.rgba.len().hash(&mut h);
            p.rgba.iter().step_by(251).for_each(|b| b.hash(&mut h));
        }
        h.finish()
    };
    let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((n, k, m)) = c.as_ref()
        && (*n, *k) == (pages.len(), key)
    {
        return m.clone();
    }
    let m = std::sync::Arc::new(padded_mips(pages));
    *c = Some((pages.len(), key, m.clone()));
    m
}

/// For every page, the padded level 0 followed by all smaller levels down to 1×1.
pub fn padded_mips(pages: &[TexturePage]) -> Vec<Vec<Vec<u8>>> {
    let masks = used_masks(pages);
    pages
        .iter()
        .zip(&masks)
        .map(|(p, m)| {
            let mut levels = vec![pad(p, m)];
            let mut s = p.size as usize;
            while s > 1 {
                let next = halve(levels.last().unwrap(), s);
                levels.push(next);
                s /= 2;
            }
            levels
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_cover_cells_and_tiles() {
        let page = |c: [u8; 4]| TexturePage { size: 256, rgba: c.repeat(256 * 256) };
        let pages: Vec<TexturePage> = (0..44).map(|_| page([255, 0, 255, 255])).collect();
        let m = used_masks(&pages);
        assert!(m[0].iter().all(|&b| b)); // grass tile
        // Page 3 cell (3,0) (grass<->beach) centre is used, the corner between hexagons is not.
        let at = |x: usize, y: usize| m[3][y * 256 + x];
        assert!(at(3 * 64 + 32, 32));
        assert!(!at(3 * 64 + 1, 1));
        assert_eq!(m[25], m[3]);
    }
}
