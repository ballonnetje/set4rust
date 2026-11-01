//! The main pass 0x68d8c0, in the release's order. Checkpoints (`Gen::cp`) carry the release's
//! return address after the call they follow, for the unicorn trace (`tools/re/mapgen/
//! trace_vectors.py`).

use crate::blob::{Blob, Marks};
use crate::generator::{Gen, cvtt};

impl Gen {
    /// Records a checkpoint: the RNG and the CRC-32 of the landscape, the objects and the grid.
    pub fn cp(&mut self, addr: u32) {
        if let Some(t) = self.trace.as_mut() {
            let land: Vec<u8> = self.land.iter().flatten().copied().collect();
            let objs: Vec<u8> = self.objs.iter().flatten().copied().collect();
            t.push(format!(
                "{addr:x} {:08x} {} {:08x} {:08x} {:08x}",
                self.rng.state,
                self.rng.count,
                crate::crc32(&land),
                crate::crc32(&objs),
                crate::crc32(&self.grid.cells)
            ));
        }
    }

    /// 0x68d8c0.
    pub fn main_pass(&mut self) {
        self.rng = crate::generator::Rng { state: self.params.seed, count: 0 };
        let mut marks = Marks::new();
        let n = self.grid.n;
        let nn = n * n;
        self.extra = self.size / (0x96 - self.land_amount) - self.players.len() as i32;
        let f8 = (self.size - 100) as f32 / n as f32;
        self.cell = f8;
        let c = cvtt(f8 * 0.5);
        // Land blobs on every set grid cell.
        for i in 0..nn {
            let x = cvtt((i % n) as f32 * f8 + 50.0);
            let y = cvtt((i / n) as f32 * f8 + 50.0);
            if self.grid.at(i) == 0 {
                continue;
            }
            if i < (n - 1) * n
                && i % n != n - 1
                && !(i + 1 < nn && self.grid.cells[(i + 1) as usize] == 0)
                && !(i + n < nn && self.grid.cells[(i + n) as usize] == 0)
                && self.grid.at(i + n + 1) != 0
            {
                let x1 = cvtt(x as f32 + f8 + c as f32);
                let y1 = cvtt(y as f32 + f8 + c as f32);
                let b = Blob::new(self, &mut marks, x + c, y + c, x1, y1, 0x10, 0x32).land();
                self.cp(0x68db85);
                b.draw(self, &mut marks);
                self.cp(0x68dc09);
            }
            let r = (self.rng.next() % c as u32) as i32;
            let b = Blob::new(self, &mut marks, r - c + x, r - c + y, r + x + c, r + y + c, 0x10, 0x32).land();
            self.cp(0x68dc68);
            b.draw(self, &mut marks);
            self.cp(0x68dcec);
            let r = (self.rng.next() % c as u32) as i32;
            let y1 = cvtt((f8 * 1.5 + y as f32) - r as f32);
            let b = Blob::new(self, &mut marks, r - c + x, y - r + c, r + x + c, y1, 0x10, 0x32).land();
            self.cp(0x68dd69);
            b.draw(self, &mut marks);
            self.cp(0x68dded);
            let r = (self.rng.next() % c as u32) as i32;
            let x1 = cvtt((f8 * 1.5 + x as f32) - r as f32);
            let b = Blob::new(self, &mut marks, x - r + c, r - c + y, x1, r + y + c, 0x10, 0x32).land();
            self.cp(0x68de6c);
            b.draw(self, &mut marks);
            self.cp(0x68def0);
            let r = (self.rng.next() % c as u32) as i32;
            let y1 = cvtt((f8 * 1.5 + y as f32) - r as f32);
            let x1 = cvtt((f8 * 1.5 + x as f32) - r as f32);
            let b = Blob::new(self, &mut marks, x - r + c, y - r + c, x1, y1, 0x10, 0x32).land();
            self.cp(0x68df7e);
            b.draw(self, &mut marks);
            self.cp(0x68e002);
            let x1 = cvtt(x as f32 + f8);
            let y1 = cvtt(y as f32 + f8);
            let b = Blob::new(self, &mut marks, x, y, x1, y1, 0x10, 0x32).land();
            self.cp(0x68e04f);
            b.draw(self, &mut marks);
            self.cp(0x68e0d3);
        }
        // Mountain blobs on the 'f' cells, reaching out a half cell towards set / 'f' neighbours.
        let half = cvtt(f8 * 0.5);
        for i in 0..nn {
            if self.grid.at(i) != 0x66 {
                continue;
            }
            let (mut top, mut bottom, mut left, mut right) = (1i32, 1i32, 1i32, 1i32);
            let x = cvtt((i % n) as f32 * f8 + 50.0);
            let y = cvtt((i / n) as f32 * f8 + 50.0);
            let g = |c: i32| self.grid.cells[c as usize];
            if i - 1 > 0 && i - 1 < nn && (g(i - 1) == 1 || g(i - 1) == 0x66) {
                left = half;
            }
            if i - n > 0 && i - n < nn && (g(i - n) == 1 || g(i - n) == 0x66) {
                top = half;
            }
            if i + 1 < nn && g(i + 1) == 0x66 {
                right = half;
            }
            if i + n < nn && g(i + n) == 0x66 {
                bottom = half;
            }
            let ry = (self.rng.next() % top as u32) as i32;
            let rx = (self.rng.next() % left as u32) as i32;
            let y1 = cvtt((y as f32 + f8) + bottom as f32);
            let x1 = cvtt((x as f32 + f8) + right as f32);
            let b = Blob::new(self, &mut marks, x - rx, y - ry, x1, y1, 0, 0x32).mountain();
            self.cp(0x68e32a);
            b.draw(self, &mut marks);
            self.cp(0x68e35a);
        }
        // The 'j' cells: type 0x40; land and 'h' cells one in six: type 0x18.
        for i in 0..nn {
            let x = cvtt((i % n) as f32 * f8 + 50.0);
            let y = cvtt((i / n) as f32 * f8 + 50.0);
            let v = self.grid.at(i);
            if v == 0x6a {
                let q = f8 * 0.25;
                let b = Blob::new(self, &mut marks, cvtt(q + x as f32), cvtt(q + y as f32), cvtt(x as f32 + f8), cvtt(y as f32 + f8), 0x40, 0x32).flat();
                self.cp(0x68e46d);
                b.draw(self, &mut marks);
                self.cp(0x68e62b);
            } else if i < nn && (v == 1 || v == 0x68) && self.rng.next() % 6 > 4 {
                let h = f8 * 0.5;
                let b = Blob::new(self, &mut marks, cvtt(x as f32 + h), cvtt(y as f32 + h), cvtt(x as f32 + f8), cvtt(y as f32 + f8), 0x18, 0x32).flat();
                self.cp(0x68e5a7);
                b.draw(self, &mut marks);
                self.cp(0x68e62b);
            }
        }
        for (a, b, c, at) in [(0x40, 7, 0x10, 0x68e67a), (0x40, 0x20, 0x10, 0x68e689), (0x18, 7, 0x10, 0x68e698), (0x18, 0x20, 0x10, 0x68e6a7), (7, 0x10, 0xff, 0x68e6b9), (7, 0xff, 0xfe, 0x68e6cd)] {
            self.spread(a, b, c);
            self.cp(at);
        }
        self.replace(0xff, 0x30);
        self.cp(0x68e6df);
        // The host's vtable +0x20 (0x50a4c0) is false: 0x30.
        self.cp(0x68e6ec);
        self.replace(0xfe, 0x30);
        self.cp(0x68e704);
        self.spread(0x20, 0x10, 0x11);
        self.cp(0x68e713);
        self.spread(0x20, 0x11, 0x21);
        self.cp(0x68e722);
        self.cp(0x68e732);
        self.spread(7, 0x30, 0xff);
        self.cp(0x68e74b);
        for (b, c, at) in [(0xff, 1, 0x68e75f), (1, 2, 0x68e76e), (2, 3, 0x68e77d), (3, 4, 0x68e78c), (4, 5, 0x68e79b), (5, 6, 0x68e7aa)] {
            self.streaks(7, b, c);
            self.cp(at);
        }
        self.replace(0xff, 0);
        self.cp(0x68e7b9);
        self.rivers();
        self.resources();
        if self.flag25 == 0 {
            self.scatter(&mut marks);
        }
    }

    /// 0x68f830: inner nodes (by columns) of type `a` with a hex neighbour of type `b` become
    /// `c` (in place).
    pub(crate) fn spread(&mut self, a: u8, b: u8, c: u8) {
        let n = self.size;
        let t = |g: &Gen, x: i32, y: i32| g.land[(y * n + x) as usize][1];
        for x in 1..n - 1 {
            for y in 1..n - 1 {
                if t(self, x, y) == a
                    && (t(self, x + 1, y) == b
                        || t(self, x - 1, y) == b
                        || t(self, x, y - 1) == b
                        || t(self, x, y + 1) == b
                        || t(self, x + 1, y + 1) == b
                        || t(self, x - 1, y - 1) == b)
                {
                    self.land[(y * n + x) as usize][1] = c;
                }
            }
        }
    }

    /// 0x68fb70: inner nodes of type `a` become `b`.
    pub(crate) fn replace(&mut self, a: u8, b: u8) {
        let n = self.size;
        for x in 1..n - 1 {
            for y in 1..n - 1 {
                let l = &mut self.land[(y * n + x) as usize];
                if l[1] == a {
                    l[1] = b;
                }
            }
        }
    }

    /// 0x68f910: nodes (4 from the edge, by columns) of type `a`: a random run of 1..=4 nodes of
    /// type `a` away from each hex neighbour of type `b` becomes `c`.
    pub(crate) fn streaks(&mut self, a: u8, b: u8, c: u8) {
        let n = self.size;
        let t = |g: &Gen, x: i32, y: i32| g.land[(y * n + x) as usize][1];
        for x in 4..n - 4 {
            for y in 4..n - 4 {
                if t(self, x, y) != a {
                    continue;
                }
                let len = (self.rng.next() & 3) as i32 + 1;
                let run = |g: &mut Gen, sx: i32, sy: i32| {
                    for j in 0..len {
                        let (px, py) = (x + sx * j, y + sy * j);
                        let l = &mut g.land[(py * n + px) as usize];
                        if l[1] == a {
                            l[1] = c;
                        }
                    }
                };
                if t(self, x + 1, y) == b {
                    run(self, -1, 0);
                }
                if t(self, x - 1, y) == b {
                    run(self, 1, 0);
                }
                if t(self, x, y - 1) == b {
                    run(self, 0, 1);
                }
                if t(self, x, y + 1) == b {
                    run(self, 0, -1);
                }
                if t(self, x + 1, y + 1) == b {
                    run(self, -1, -1);
                }
                if t(self, x - 1, y - 1) == b {
                    run(self, 1, 1);
                }
            }
        }
    }
}

