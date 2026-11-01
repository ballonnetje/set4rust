//! The preview 0x68caf0: the grid drawn as blobs on a 320 × 320 RGB555 canvas (0x146b0e0),
//! averaged down to 160 × 160 (0x146b10c) and mirrored like the map.

use crate::blob::{Blob, Marks};
use crate::generator::{Gen, Rng, cvtt, cvttd};

/// The canvas side (the blobs' size, 0x140) and the preview's (0xa0).
const CANVAS: i32 = 0x140;
const SIDE: usize = 0xa0;

/// The colours by landscape type (the table 0x664d10 builds, +2 · type): water 7 (the canvas'
/// clear), land 0x10, mountain 0x20, 0x40 ('j').
const WATER: u16 = 0x66;
const LAND: u16 = 0x180;
const MOUNTAIN: u16 = 0x14a4;
const J: u16 = 0x3120;

impl Gen {
    /// 0x68caf0 after the init (it restarts the RNG from the seed; the main pass does again).
    pub fn preview(&mut self) -> Vec<u16> {
        self.rng = Rng { state: self.params.seed, count: 0 };
        self.preview = vec![WATER; (CANVAS * CANVAS) as usize];
        let mut marks = Marks::new();
        let n = self.grid.n;
        let f = 280.0f32 / n as f32;
        let ext = cvtt(f * 0.125) as u32;
        let blob = |g: &mut Gen, marks: &mut Marks, r: [i32; 4], colour: u16| {
            let b = Blob::sized(g, marks, r, colour as i32, 0x14, CANVAS).preview();
            b.draw(g, marks);
        };
        // Land cells: four blobs shifted by ext..2·ext to the corners, then the cell.
        for i in 0..n * n {
            if self.grid.at(i) == 0 {
                continue;
            }
            let x = cvtt((i % n) as f32 * f + 20.0);
            let y = cvtt((i / n) as f32 * f + 20.0);
            let (xf, yf) = (x as f32 + f, y as f32 + f);
            let shift = |g: &mut Gen| (g.rng.next() % ext + ext) as i32;
            let r = shift(self);
            blob(self, &mut marks, [x - r, y - r, cvtt(xf + r as f32), cvtt(r as f32 + yf)], LAND);
            let r = shift(self);
            blob(self, &mut marks, [x - r, y + r, cvtt(xf + r as f32), cvtt(r as f32 + yf)], LAND);
            let r = shift(self);
            blob(self, &mut marks, [x + r, y - r, cvtt(xf + r as f32), cvtt(yf - r as f32)], LAND);
            let r = shift(self);
            blob(self, &mut marks, [x + r, y + r, cvtt(xf + r as f32), cvtt(r as f32 + yf)], LAND);
            blob(self, &mut marks, [x, y, cvtt(xf), cvtt(yf)], LAND);
        }
        // Then each set cell in its colour ('f' mountain, 'j'), 'f' bridged half a cell into an
        // 'f' to the right and below.
        for i in 0..n * n {
            let v = self.grid.at(i);
            if v == 0 {
                continue;
            }
            let x = cvtt((i % n) as f32 * f + 20.0);
            let y = cvtt((i / n) as f32 * f + 20.0);
            let colour = match v {
                0x66 => MOUNTAIN,
                0x6a => J,
                _ => LAND,
            };
            let (x1, y1) = (cvtt(x as f32 + f), cvtt(y as f32 + f));
            blob(self, &mut marks, [x, y, x1, y1], colour);
            if v != 0x66 {
                continue;
            }
            if i % n < n - 1 && self.grid.at(i + 1) == 0x66 {
                let r = [cvtt(f * 0.5 + x as f32), y, cvttd(f as f64 * 1.5 + x as f64), y1];
                blob(self, &mut marks, r, colour);
            }
            if i < (n - 1) * n && self.grid.at(i + n) == 0x66 {
                let r = [x, cvtt(f * 0.5 + y as f32), x1, cvttd(f as f64 * 1.5 + y as f64)];
                blob(self, &mut marks, r, colour);
            }
        }
        // 2 × 2 averages per 5-bit channel.
        let c = &self.preview;
        let w = CANVAS as usize;
        let mut out = vec![0u16; SIDE * SIDE];
        for r in 0..SIDE {
            for k in 0..SIDE {
                let px = [c[2 * r * w + 2 * k], c[2 * r * w + 2 * k + 1], c[(2 * r + 1) * w + 2 * k], c[(2 * r + 1) * w + 2 * k + 1]];
                let avg = |m: u32| ((px.iter().map(|&p| p as u32 & m).sum::<u32>() >> 2) & m) as u16;
                out[r * SIDE + k] = avg(0x1f) + avg(0x7c00) + avg(0x3e0);
            }
        }
        if self.mirror & 1 != 0 {
            for r in 0..SIDE {
                for k in 0..r {
                    out[k * SIDE + r] = out[r * SIDE + k];
                }
            }
        }
        if self.mirror & 2 != 0 {
            for r in 0..SIDE {
                for k in 0..SIDE - 1 - r {
                    out[(SIDE - 1 - k) * SIDE + (SIDE - 1 - r)] = out[r * SIDE + k];
                }
            }
        }
        self.preview = Vec::new();
        out
    }
}
