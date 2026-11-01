//! The rivers of the main pass (0x68e7b9..0x68e9ea): one from each player's area to the sea
//! (path 0x68fff0), extra ones from random land cells (path 0x690140), each path carved node by
//! node (0x690550) and ended by a spill into nearby water (0x692940).

use crate::blob::{HX, HY};
use crate::generator::{Gen, cvtt, cvttd};

/// The spiral offsets (8000 short pairs): entry 0 is (0, 0); then per pick of a candidate
/// (r, i) by the lowest weight i²·0x1d91 + (2r − i)²·0x9c4 among 2000 (r = 1.., i < r), its six
/// rotations (r, i), (r − i, r), (−i, r − i), (−r, −i), (i − r, −r), (i, i − r), while the next
/// entry is below 0x1f3b.
///
/// `reset`: 0x692c70 (the main pass's local copy) starts each pick at 0x7fffffff. The static
/// initializer 0x4270a0 of the global copy 0x11f71b8 (read by 0x692940) keeps the minimum in
/// 0x11feec4 across picks instead: after the first pick nothing is lower, so it repeats the
/// first pick, (1, 0)'s ring, to the end (the unicorn run of 0x4270a0 shows the same).
pub fn spiral(reset: bool) -> Vec<(i32, i32)> {
    let mut cand = Vec::with_capacity(2000);
    let (mut r, mut i) = (1i32, 0i32);
    while cand.len() < 2000 {
        cand.push([i * i * 0x1d91 + (2 * r - i) * (2 * r - i) * 0x9c4, r, i]);
        i += 1;
        if i == r {
            i = 0;
            r += 1;
        }
    }
    let mut t = vec![(0i32, 0i32); 8000];
    let (mut min, mut pick) = (0x7fffffff, i as usize);
    let mut at = 1;
    while at < 0x1f3b {
        if reset {
            min = 0x7fffffff;
        }
        for (k, c) in cand.iter().enumerate() {
            if c[0] < min {
                min = c[0];
                pick = k;
            }
        }
        cand[pick][0] = 0x7fffffff;
        let (a, b) = (cand[pick][1] as i16 as i32, cand[pick][2] as i16 as i32);
        t[at..at + 6].copy_from_slice(&[(a, b), (a - b, a), (-b, a - b), (-a, -b), (b - a, -a), (b, b - a)]);
        at += 6;
    }
    t
}

/// The global spiral 0x11f71b8 (`spiral(false)`).
pub fn global_spiral() -> &'static [(i32, i32)] {
    static S: std::sync::OnceLock<Vec<(i32, i32)>> = std::sync::OnceLock::new();
    S.get_or_init(|| spiral(false))
}

impl Gen {
    /// 0x68e7b9..0x68e9ea.
    pub(crate) fn rivers(&mut self) {
        let players = self.players.len() as i32;
        let mut count = players;
        if self.mirror & 3 == 3 {
            count = (players > 4) as i32 + 1;
        }
        if self.mirror & 3 != 0 {
            count = cvtt(players as f32 * 0.5 + 0.5);
        }
        for k in 0..count.max(0) as usize {
            let p = self.players[k];
            let path = self.player_river(p.bx * 2, p.by * 2);
            self.cp(0x68e83e);
            for w in path.windows(2) {
                self.carve(w[0].0, w[0].1, w[1].0, w[1].1);
                self.cp(0x68e880);
            }
        }
        let n = self.grid.n as u32;
        for _ in 0..self.extra.max(0) {
            let (x, y) = loop {
                let x = (self.rng.next() % n) as i32;
                let y = (self.rng.next() % n) as i32;
                let gn = self.grid.n;
                if x < gn && y < gn && matches!(self.grid.get(x, y), 0 | 0x66) {
                    continue;
                }
                break (x, y);
            };
            let path = self.extra_river(x, y);
            self.cp(0x68e95f);
            for w in path.windows(2) {
                self.carve(w[0].0, w[0].1, w[1].0, w[1].1);
                self.cp(0x68e9a0);
            }
        }
    }

    /// 0x68fff0: from grid cell (x, y) (unless on the grid's edge row / column 0 or n − 2: from
    /// the nearest empty cell) the nearest water node; the path is the cell's far corner to it.
    fn player_river(&mut self, x: i32, y: i32) -> Vec<(i32, i32)> {
        let gn = self.grid.n;
        let (mut px, mut py) = (x, y);
        if !(x == 0 || x == gn - 2 || y == 0 || y == gn - 2) && !self.grid.nearest(&mut px, &mut py, 0, false) {
            return Vec::new();
        }
        let c = self.cell;
        let at = |v: i32| cvtt(v as f32 * c + 50.0 + c);
        let i = self.water(at(px), at(py));
        if i == -1 {
            return Vec::new();
        }
        vec![(at(x), at(y)), (i % self.size, i / self.size)]
    }

    /// 0x690140: from the empty grid cell nearest to (x, y), one step (+x, +y, −x, −y) onto a
    /// cell neither empty nor 'f' nor 'j' (else no path); marks land cells (1) on the way 'g'
    /// (0x67). The path is that cell's centre to the water nearest the empty cell.
    fn extra_river(&mut self, x: i32, y: i32) -> Vec<(i32, i32)> {
        let (mut x, mut y) = (x, y);
        if !self.grid.nearest(&mut x, &mut y, 0, false) {
            return Vec::new();
        }
        let (c, h) = (self.cell, self.cell * 0.5);
        let at = |v: i32| cvtt(v as f32 * c + 50.0 + h);
        let i = self.water(at(x), at(y));
        let end = (i % self.size, i / self.size);
        let gn = self.grid.n;
        let mark = |g: &mut Gen, x: i32, y: i32| {
            if x < gn && y < gn && g.grid.get(x, y) == 1 {
                g.grid.set(x, y, 0x67);
            }
        };
        mark(self, x, y);
        let open = |g: &Gen, x: i32, y: i32| !(x < gn && y < gn) || !matches!(g.grid.get(x, y), 0 | 0x66 | 0x6a);
        let moved = if x + 1 < gn && open(self, x + 1, y) {
            x += 1;
            true
        } else if y + 1 < gn && open(self, x, y + 1) {
            y += 1;
            true
        } else if x - 1 > 0 && open(self, x - 1, y) {
            x -= 1;
            true
        } else if y - 1 > 0 && open(self, x, y - 1) {
            y -= 1;
            true
        } else {
            false
        };
        mark(self, x, y);
        if !moved {
            return Vec::new();
        }
        vec![(at(x), at(y)), end]
    }

    /// 0x6909e0: a breadth-first search over the hex neighbours (a 1024-entry ring queue) from
    /// node (x, y) for the first water node (type 0) reached; its index or −1.
    fn water(&self, x: i32, y: i32) -> i32 {
        let n = self.size;
        let total = n * n;
        let mut seen = vec![false; total as usize];
        let mut queue = [0i32; 1024];
        queue[0] = n * y + x;
        seen[queue[0] as usize] = true;
        let (mut head, mut tail) = (0usize, 1usize);
        loop {
            let cur = queue[head];
            head = (head + 1) & 0x3ff;
            for k in 0..6 {
                let next = match k {
                    0 if cur - n > 0 => cur - n - 1,
                    1 if cur - n >= 0 => cur - n,
                    2 => (cur - 1).max(0),
                    3 if cur + 1 < total => cur + 1,
                    4 if n + cur < total => n + cur,
                    5 if n + 1 + cur < total => n + 1 + cur,
                    _ => 0,
                };
                if !seen[next as usize] {
                    queue[tail] = next;
                    tail = (tail + 1) & 0x3ff;
                    seen[next as usize] = true;
                    if self.land[next as usize][1] == 0 {
                        return next;
                    }
                }
            }
            if head == tail {
                return -1;
            }
        }
    }

    /// 0x690550: walks from (x, y) towards (x1, y1) along the hex direction closest to the goal
    /// (or, one time in four while more than 8 away, a three-step bend), painting land (0x10) as
    /// river 0x60 + width (the width grows every quarter of the distance covered; object byte 3
    /// = rng % resources) and raising the banks: neighbours of type 8 or 0x1x / 0x3x / 0x4x but
    /// 0x10 / 0x11 become 0x10, and where water touches it the walk stops (the old node 0x63).
    /// It ends 3 or fewer from the goal (or at water) with the spill 0x692940.
    fn carve(&mut self, x0: i32, y0: i32, x1: i32, y1: i32) {
        let n = self.size;
        let (mut x, mut y) = (x0, y0);
        let sq = |v: i32| (v as f64) * (v as f64);
        let per = cvttd((sq(y1 - y0) + sq(x1 - x0)).sqrt() * 0.25);
        let (mut dir, mut bank, mut width, mut turn) = (0usize, false, 0u8, 0u32);
        let step = per.wrapping_neg();
        let mut thresh = per.wrapping_mul(3);
        loop {
            let (ex, ey) = (x1 - x, y1 - y);
            let mut best = 0xffff7;
            let mut d = dir;
            for k in 0..6 {
                let v = cvttd((sq(ex - HX[k]) + sq(ey - HY[k])).sqrt());
                if v < best {
                    best = v;
                    if turn == 0 {
                        d = k;
                    }
                }
            }
            dir = d;
            if self.rng.next() & 3 == 0 && turn == 0 && best > 8 {
                turn = 3;
                dir = if dir + 1 == 6 { 0 } else { dir + 1 };
            }
            if best < thresh {
                width = width.wrapping_add(1);
                thresh = thresh.wrapping_add(step);
            }
            let i = (y * n + x) as usize;
            if self.land[i][1] == 0x10 && !bank {
                self.land[i][1] = width.wrapping_add(0x60);
                self.objs[i][3] = (self.rng.next() % self.resources as u32) as u8;
            } else {
                bank = false;
            }
            let (ox, oy) = (x, y);
            x += HX[dir];
            y += HY[dir];
            if turn != 0 {
                if turn & 1 != 0 {
                    dir = if dir == 0 { 5 } else { dir - 1 };
                }
                turn -= 1;
            }
            let (old, new) = ((oy * n + ox) as usize, (y * n + x) as usize);
            let t = |g: &Gen, i: i32| g.land[i as usize][1];
            for k in 0..6 {
                let (nx, ny) = (x + HX[k], y + HY[k]);
                let ni = ny * n + nx;
                let v = t(self, ni);
                if (matches!(v & 0xf0, 0x10 | 0x30 | 0x40) || v == 8) && v != 0x10 && v != 0x11 {
                    let (ul, lr) = (ni - n - 1, ni + n + 1);
                    let mut hit = |g: &mut Gen| {
                        best = 0;
                        g.land[old][1] = 0x63;
                        g.land[new][1] = 0x10;
                    };
                    if t(self, ul) == 0 {
                        hit(self);
                    }
                    if t(self, ni) == 0 {
                        hit(self);
                    }
                    if t(self, lr) == 0 {
                        hit(self);
                        if t(self, lr) == 0 {
                            hit(self);
                        }
                    }
                    if t(self, ni) == 0 {
                        hit(self);
                    }
                    if t(self, ul) == 0 {
                        hit(self);
                    }
                    self.land[ni as usize][1] = 0x10;
                }
                if t(self, ni) == 0 {
                    self.land[new][1] = 0x63;
                    best = 0;
                }
                if t(self, ni) == 0x11 {
                    bank = true;
                }
            }
            if best <= 3 {
                self.spill(x, y);
                return;
            }
        }
    }

    /// 0x6927d0: how many of node (x, y)'s hex neighbours (in the map) are water or river
    /// (0, 0x60..=0x63).
    fn wet(&self, x: i32, y: i32) -> i32 {
        let n = self.size;
        let inside = |v: i32| (0..n).contains(&v);
        let w = |dx: i32, dy: i32| {
            let t = self.land[((y + dy) * n + x + dx) as usize][1];
            (t == 0 || (0x60..=0x63).contains(&t)) as i32
        };
        let mut c = 0;
        if inside(y - 1) {
            if inside(x - 1) {
                c += w(-1, -1);
            }
            if inside(x) {
                c += w(0, -1);
            }
        }
        if inside(y) && inside(x + 1) {
            c += w(1, 0);
        }
        if inside(y + 1) {
            if inside(x + 1) {
                c += w(1, 1);
            }
            if inside(x) {
                c += w(0, 1);
            }
        }
        if inside(y) && inside(x - 1) {
            c += w(-1, 0);
        }
        c
    }

    /// 0x692940: along the global spiral (91 entries) around (x, y), the first river end 0x63 with
    /// exactly one wet neighbour; then the first water node along the same spiral, and a line of
    /// water (type 0) from that node to the river end.
    fn spill(&mut self, x: i32, y: i32) {
        let n = self.size;
        let s = global_spiral();
        let inside = |x: i32, y: i32| (0..n).contains(&x) && (0..n).contains(&y);
        let t = |g: &Gen, x: i32, y: i32| g.land[(y * n + x) as usize][1];
        let Some((px, py)) = s[..0x5b]
            .iter()
            .map(|&(dx, dy)| (x + dx, y + dy))
            .find(|&(px, py)| inside(px, py) && t(self, px, py) == 0x63 && self.wet(px, py) == 1)
        else {
            return;
        };
        let Some((sx, sy)) = s[..0x5b].iter().map(|&(dx, dy)| (x + dx, y + dy)).find(|&(sx, sy)| inside(sx, sy) && t(self, sx, sy) == 0)
        else {
            return;
        };
        let (dxv, dyv) = (sx - px, sy - py);
        let (adx, ady) = (dxv.abs(), dyv.abs());
        let (mut cx, mut cy) = (sx, sy);
        let put = |g: &mut Gen, x: i32, y: i32| g.land[(y * n + x) as usize][1] = 0;
        if adx >= ady {
            let slope = if dyv != 0 { ady as f32 / adx as f32 } else { 0.0 };
            let yf = sy as f32;
            for c in 1..=adx {
                put(self, cx, cy);
                let v = if dyv < 0 { c as f32 * slope + yf } else { yf - c as f32 * slope };
                cy = cvtt(v + 0.5);
                put(self, cx, cy);
                cx = if dxv >= 0 { cx - 1 } else { cx + 1 };
            }
        } else {
            let slope = if dxv != 0 { adx as f32 / ady as f32 } else { 0.0 };
            let xf = sx as f32;
            for c in 1..=ady {
                put(self, cx, cy);
                let v = if dxv < 0 { c as f32 * slope + xf } else { xf - c as f32 * slope };
                cx = cvtt(v + 0.5);
                put(self, cx, cy);
                cy = if dyv >= 0 { cy - 1 } else { cy + 1 };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::spiral;

    fn crc(t: &[(i32, i32)]) -> u32 {
        let b: Vec<u8> = t.iter().flat_map(|&(x, y)| [(x as i16).to_le_bytes(), (y as i16).to_le_bytes()].concat()).collect();
        crate::crc32(&b)
    }

    /// The CRC-32 of the 8000 entries: 0x4270a0 at 0x11f71b8, and 0x692c70 into a buffer, run in
    /// unicorn (`tools/re/mapgen/mapgen_emu.py`).
    #[test]
    fn spirals_match_the_release() {
        assert_eq!(crc(&spiral(false)), 0x7594b249);
        assert_eq!(crc(&spiral(true)), 0xeb8b0276);
    }
}
