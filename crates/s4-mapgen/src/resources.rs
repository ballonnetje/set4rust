//! The main pass after the rivers (0x68e9ea..0x68efd3): resources (object byte 3) around the
//! players and scattered over the mountains, fish in the water, the mirror copies, the slope
//! limit and the last type passes.

use crate::generator::{Gen, cvtt};
use crate::rivers::spiral;

impl Gen {
    /// Resource byte: `rng % (16 − resources) + resources`, the kind in the high nibble.
    fn amount(&mut self) -> u8 {
        let res = self.resources as u32;
        ((self.rng.next() % 16u32.wrapping_sub(res)) as u8).wrapping_add(res as u8)
    }

    /// 0x68e9ea..0x68ef67.
    pub(crate) fn resources(&mut self) {
        let sp = spiral(true);
        self.cp(0x68e9f5);
        let n = self.size;
        let inside = |x: i32, y: i32| (0..n).contains(&x) && (0..n).contains(&y);
        // Per player three spots on mountain (0x20) without a resource, 50 tries each, near the
        // player's 'f' cell: 16 spiral nodes of coal 0x10 / iron 0x20 / gold 0x50 [inf: kinds].
        let span = cvtt(self.cell) as u32;
        for p in 0..self.players.len() {
            let (gx, gy) = (self.players[p].gx, self.players[p].gy);
            for kind in 0..3 {
                let (mut x, mut y);
                let mut tries = 0;
                loop {
                    tries += 1;
                    let r1 = self.rng.next() % span;
                    let r2 = self.rng.next() % span;
                    x = cvtt(r1 as f32 + (gx as f32 * self.cell + 50.0));
                    y = cvtt(r2 as f32 + (gy as f32 * self.cell + 50.0));
                    let i = (y * n + x) as usize;
                    if (self.land[i][1] == 0x20 && self.objs[i][3] == 0) || tries >= 0x32 {
                        break;
                    }
                }
                let flag = [0x10, 0x20, 0x50][kind];
                for &(dx, dy) in &sp[..0x10] {
                    let (px, py) = (x + dx, y + dy);
                    if inside(px, py) && self.land[(py * n + px) as usize][1] == 0x20 {
                        let v = self.amount() | flag;
                        self.objs[(py * n + px) as usize][3] = v;
                    }
                }
            }
        }
        // Each mountain node one time in 250 − 10 · resources: a deposit of a random kind and
        // size on the free mountain nodes of the spiral; water (types 0..=6) gets fish.
        for i in 0..n * n {
            let iu = i as usize;
            if self.land[iu][1] == 0x20 && self.rng.next().is_multiple_of(0xfa - self.resources as u32 * 10) {
                let (flag, count) = match self.rng.next() % 0x14 {
                    0..7 => (0x10, 0x30),
                    7..12 => (0x20, 0x18),
                    12..15 => (0x30, 0x12),
                    15..17 => (0x40, 0x18),
                    _ => (0x50, 0x18),
                };
                let (x0, y0) = (i % n, i / n);
                for &(dx, dy) in &sp[..count] {
                    let (px, py) = (x0 + dx, y0 + dy);
                    let j = (py * n + px) as usize;
                    if inside(px, py) && self.land[j][1] == 0x20 && self.objs[j][3] == 0 {
                        self.objs[j][3] = self.amount() | flag;
                    }
                }
            }
            if self.land[iu][1] <= 6 {
                self.objs[iu][3] = self.amount();
            }
        }
        if self.mirror & 1 != 0 {
            self.mirror_diagonal();
        }
        self.cp(0x68ef77);
        if self.mirror & 2 != 0 {
            self.mirror_anti_diagonal();
        }
        self.cp(0x68ef81);
        self.slopes();
        self.cp(0x68ef86);
        self.spread(0x80, 0x20, 0x23);
        self.cp(0x68ef95);
        self.spread(0x80, 0x23, 0x81);
        self.cp(0x68efa7);
        self.streaks(0x18, 0x10, 0x19);
        self.cp(0x68efb6);
        self.spread(0x40, 0x10, 0x14);
        self.cp(0x68efc5);
        self.streaks(0x40, 0x14, 0x41);
        self.cp(0x68efd3);
        // Around each start, 96 spiral nodes become land (0x10) without resources.
        for p in 0..self.players.len() {
            let (sx, sy) = (self.players[p].sx, self.players[p].sy);
            self.cp(0x68f026);
            for &(dx, dy) in &sp[..0x60] {
                let (x, y) = (sx + dx, sy + dy);
                if inside(x, y) {
                    let i = (y * n + x) as usize;
                    self.land[i][1] = 0x10;
                    self.objs[i][3] = 0;
                }
            }
        }
        self.finalize();
        self.cp(0x68f0fb);
    }

    /// The host's vtable +8 (0x50a820): landscape byte 2's low nibble = (height above − height
    /// below) / 2 + 8 (arithmetic shift; the edge rows use the node itself), object byte 2 bit 0
    /// where the type's flags (.data 0x11f67d8) have 0x10: types 0..=8, 0x50, 0x51, 0x80, 0x81
    /// [inf: not walkable].
    fn finalize(&mut self) {
        let n = self.size as usize;
        let nodes = n * n;
        for i in 0..nodes {
            let h = self.land[i][0] as i32;
            let above = if i < n { h } else { self.land[i - n][0] as i32 };
            let below = if i >= nodes - n { h } else { self.land[i + n][0] as i32 };
            let v = (((above - below) >> 1) + 8) as u8;
            self.land[i][2] = (self.land[i][2] & 0xf0) | (v & 0xf);
            if matches!(self.land[i][1], 0..=8 | 0x50 | 0x51 | 0x80 | 0x81) {
                self.objs[i][2] |= 1;
            }
        }
    }

    /// 0x68fc20: nodes below the diagonal (x < y) copied to their mirror (y, x).
    fn mirror_diagonal(&mut self) {
        let n = self.size as usize;
        for r in 0..n {
            for c in 0..r {
                self.land[c * n + r] = self.land[r * n + c];
                self.objs[c * n + r] = self.objs[r * n + c];
            }
        }
    }

    /// 0x68fcd0: nodes above the anti-diagonal (x + y < n − 1) copied to (n − 1 − y, n − 1 − x).
    fn mirror_anti_diagonal(&mut self) {
        let n = self.size as usize;
        for r in 0..n {
            for c in 0..(n - 1 - r) {
                let d = (n - 1 - c) * n + (n - 1 - r);
                self.land[d] = self.land[r * n + c];
                self.objs[d] = self.objs[r * n + c];
            }
        }
    }

    /// 0x68fd90: row by row, each inner node at most 5 above the lower and at least 7 below the
    /// higher of its two upper neighbours ((x − 1, y − 1), (x, y − 1)); a mountain node (0x20)
    /// that ends above the level becomes rock 0x80 (resource cleared), rock below it 0x20.
    fn slopes(&mut self) {
        let n = self.size;
        let level = self.level;
        let settle = |g: &mut Gen, i: usize| {
            let h = g.land[i][0];
            if h > level {
                if g.land[i][1] == 0x20 {
                    g.land[i][1] = 0x80;
                    g.objs[i][3] = 0;
                }
            } else if h < level && g.land[i][1] == 0x80 {
                g.land[i][1] = 0x20;
            }
        };
        for y in 1..n {
            for x in 1..n - 1 {
                let i = (y * n + x) as usize;
                let (a, b) = (self.land[i - n as usize - 1][0], self.land[i - n as usize][0]);
                let d = a.min(b) as i32 - self.land[i][0] as i32 + 5;
                if d < 0 {
                    self.land[i][0] = self.land[i][0].wrapping_add(d as u8);
                    settle(self, i);
                }
                let (a, b) = (self.land[i - n as usize - 1][0], self.land[i - n as usize][0]);
                let e = self.land[i][0] as i32 - a.max(b) as i32 + 7;
                if e < 0 {
                    self.land[i][0] = self.land[i][0].wrapping_sub(e as u8);
                    settle(self, i);
                }
            }
        }
    }
}
