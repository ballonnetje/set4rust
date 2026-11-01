//! The generator's blobs (`CBlob` and its kinds): a rectangle whose outline is walked between
//! the four mid-edge points with random steps (0x691ef0, directions 0x6922d0), then filled from
//! the centre (0x692120), each node painted by the kind (vtable +4 the outline, +8 the fill).
//! A 256 × 256 mark buffer (0x146b0dc for the main pass) keeps what is painted.

use crate::generator::{Gen, cvtt};

/// The mark buffer (+0 the cells, +4 width, +8 height).
#[derive(Clone, Debug)]
pub struct Marks {
    pub w: i32,
    pub h: i32,
    pub cells: Vec<u16>,
}

impl Marks {
    pub fn new() -> Marks {
        Marks { w: 0x100, h: 0x100, cells: vec![0; 0x10000] }
    }
    fn get(&self, x: i32, y: i32) -> Option<u16> {
        (x < self.w && x >= 0 && y < self.h && y >= 0).then(|| self.cells[(self.w * y + x) as usize])
    }
    fn mark(&mut self, x: i32, y: i32, v: u16) {
        if x < self.w && x >= 0 && y < self.h && y >= 0 {
            self.cells[(self.w * y + x) as usize] = v;
        }
    }
}

impl Default for Marks {
    fn default() -> Self {
        Marks::new()
    }
}

/// What a blob paints.
#[derive(Clone, Copy, Debug)]
pub enum Kind {
    /// `CLandBlob` (vtable 0x1070ac4): the outline sets the type and raises the height by 1 (0x690d40);
    /// the fill sets the type, and with `slope` raises the height by 1 beyond 20 / `k` from the
    /// centre, else to `20 − d·k + rng % 3` above it (0x690c10).
    Land { cx: i32, cy: i32, k: f32, slope: bool },
    /// `CMountainBlob` (vtable 0x1070af4): the outline raises land (types 7 / 0x10) to type 0x10
    /// by 5 (0x692610); the fill walks inwards along the marks (0x693d50) setting each node to
    /// its lowest marked hex neighbour + 3 or 4, type 0x20 below the level, else 0x80 (0x692680).
    Mountain,
    /// `CObjectBlob` (vtable 0x1070ad4): the outline only marks (0x6923c0); a fill node, one time
    /// in 100 − `chance`, unless within 6 of a start on either axis, on a type whose high nibble
    /// is the blob's value, gets object `id + rng % spread` (and its mirrors) (0x691ba0). +0x2c
    /// `id`, +0x30 `spread`, +0x34 `chance`.
    Object { id: i32, spread: u32, chance: i32 },
    /// `CPreviewBlob` (vtable 0x1070ab4): the outline only marks (0x6923c0); a fill node gets the
    /// blob's value as its colour on the preview canvas (0x68ff90).
    Preview,
}

/// The hex neighbours of the mountain blob (+0x2c dx, +0x44 dy; .rdata 0x1071100..).
pub(crate) const HX: [i32; 6] = [-1, 0, 1, 1, 0, -1];
pub(crate) const HY: [i32; 6] = [0, 1, 1, 0, -1, -1];

/// A blob (`CBlob` 0x691d40 and the kind's fields).
#[derive(Clone, Copy, Debug)]
pub struct Blob {
    pub kind: Kind,
    /// +4: the value painted (a landscape type).
    pub value: i32,
    /// +8: the margin to the map's edge.
    pub margin: i32,
    /// +0xc: the map size.
    pub size: i32,
    /// +0x10 .. +0x1c: the rectangle.
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
    /// +0x24 / +0x28: the mark buffer's origin (x0 − 10, y0 − 10).
    pub ox: i32,
    pub oy: i32,
}

impl Blob {
    /// 0x691d40: clears the marks; a side within `margin` of the edge is moved to the margin and
    /// the rectangle shifted by `rng % (margin / 2) − margin / 4` (the later draw on each axis
    /// wins).
    #[allow(clippy::too_many_arguments)]
    pub fn new(g: &mut Gen, marks: &mut Marks, x0: i32, y0: i32, x1: i32, y1: i32, value: i32, margin: i32) -> Blob {
        let size = g.size;
        Blob::sized(g, marks, [x0, y0, x1, y1], value, margin, size)
    }

    /// `new` on a canvas of `size` (the preview's 320).
    pub fn sized(g: &mut Gen, marks: &mut Marks, [x0, y0, x1, y1]: [i32; 4], value: i32, margin: i32, size: i32) -> Blob {
        marks.cells.iter_mut().for_each(|c| *c = 0);
        let quarter = margin / 4;
        let mut shift_x = 0;
        let mut shift_y = 0;
        let bx0 = if margin < x0 {
            x0
        } else {
            shift_x = (g.rng.next() % (margin / 2) as u32) as i32 - quarter;
            margin
        };
        let by0 = if margin < y0 {
            y0
        } else {
            shift_y = (g.rng.next() % (margin / 2) as u32) as i32 - quarter;
            margin
        };
        let limit = size - margin;
        let bx1 = if x1 < limit {
            x1
        } else {
            shift_x = (g.rng.next() % (margin / 2) as u32) as i32 - quarter;
            size - margin
        };
        let by1 = if y1 < limit {
            y1
        } else {
            shift_y = (g.rng.next() % (margin / 2) as u32) as i32 - quarter;
            size - margin
        };
        let (x0, y0, x1, y1) = (bx0 + shift_x, by0 + shift_y, bx1 + shift_x, by1 + shift_y);
        Blob { kind: Kind::Land { cx: 0, cy: 0, k: 0.0, slope: false }, value, margin, size, x0, y0, x1, y1, ox: x0 - 10, oy: y0 - 10 }
    }

    /// A `CPreviewBlob`.
    pub fn preview(mut self) -> Blob {
        self.kind = Kind::Preview;
        self
    }

    /// A `CObjectBlob`.
    pub fn object(mut self, id: i32, spread: u32, chance: i32) -> Blob {
        self.kind = Kind::Object { id, spread, chance };
        self
    }

    /// A `CMountainBlob`.
    pub fn mountain(mut self) -> Blob {
        self.kind = Kind::Mountain;
        self
    }

    /// A `CLandBlob` without the height slope (its +0x3c 0).
    pub fn flat(self) -> Blob {
        let mut b = self.land();
        if let Kind::Land { slope, .. } = &mut b.kind {
            *slope = false;
        }
        b
    }

    /// The main pass's `CLandBlob` set-up: centre and slope factor from the rectangle.
    pub fn land(mut self) -> Blob {
        let r = ((self.y1 - self.y0) + (self.x1 - self.x0)) / 2;
        let cx = (self.x1 - self.x0) / 2 + self.x0;
        let cy = (self.y1 - self.y0) / 2 + self.y0;
        self.kind = Kind::Land { cx, cy, k: 20.0f32 / r as f32, slope: true };
        self
    }

    /// vtable +4: a node of the outline.
    fn outline(&self, g: &mut Gen, marks: &mut Marks, x: i32, y: i32) {
        if let Kind::Mountain = self.kind {
            return self.mountain_outline(g, marks, x, y);
        }
        marks.mark(x - self.ox, y - self.oy, 1);
        match self.kind {
            Kind::Land { .. } => {
                let i = (self.size * y + x) as usize;
                if g.land[i][1] as i32 != self.value {
                    g.land[i][1] = self.value as u8;
                }
                g.land[i][0] = g.land[i][0].wrapping_add(1);
            }
            Kind::Mountain | Kind::Object { .. } | Kind::Preview => {}
        }
    }

    /// `CObjectBlob` vtable +8 (0x691ba0), after the mark.
    fn object_inside(&self, g: &mut Gen, x: i32, y: i32, id: i32, spread: u32, chance: i32) {
        if !g.rng.next().is_multiple_of((100 - chance) as u32) {
            return;
        }
        if g.players.iter().any(|p| (x - p.sx).abs() < 7 || (y - p.sy).abs() < 7) {
            return;
        }
        if (g.land[(self.size * y + x) as usize][1] & 0xf0) as i32 != self.value {
            return;
        }
        let id = id + (g.rng.next() % spread) as i32;
        let w = self.size;
        g.set_object(x, y, id);
        if g.mirror & 1 != 0 {
            g.set_object(y, x, id);
        }
        if g.mirror & 2 != 0 {
            g.set_object(w - y - 1, w - x - 1, id);
        }
        if g.mirror & 3 == 3 {
            g.set_object(w - x - 1, w - y - 1, id);
        }
    }

    /// `CMountainBlob` vtable +4 (0x692610): it paints before it marks.
    fn mountain_outline(&self, g: &mut Gen, marks: &mut Marks, x: i32, y: i32) {
        let i = (self.size * y + x) as usize;
        let t = g.land[i][1];
        if t == 0x10 || t == 7 {
            g.land[i][1] = 0x10;
            g.land[i][0] = g.land[i][0].wrapping_add(5);
        }
        marks.mark(x - self.ox, y - self.oy, 1);
    }

    /// `CMountainBlob` vtable +8 (0x692680).
    fn mountain_inside(&self, g: &mut Gen, marks: &Marks, x: i32, y: i32) {
        let mut low = 0xffu8;
        for k in 0..6 {
            let h = g.land[((y + HY[k]) * self.size + HX[k] + x) as usize][0];
            if h < low && marks.get(HX[k] - self.ox + x, HY[k] - self.oy + y).is_some_and(|v| v != 0) {
                low = h;
            }
        }
        let i = (self.size * y + x) as usize;
        g.land[i][0] = if low < 0xfa { ((g.rng.next() & 1) as u8).wrapping_add(3).wrapping_add(low) } else { low };
        g.land[i][1] = if g.land[i][0] < g.level { 0x20 } else { 0x80 };
    }

    /// 0x693d50: from (x, y) (first moved up to a node next to the marks), wall-following the
    /// marked area and numbering the nodes it paints; backing up along the numbers where all
    /// neighbours are taken, until back at 1.
    fn mountain_fill(&self, g: &mut Gen, marks: &mut Marks, mut x: i32, mut y: i32) {
        let (ox, oy) = (self.ox, self.oy);
        let mk = |m: &Marks, x: i32, y: i32| m.get(x - ox, y - oy).unwrap_or(0);
        let mut counter: u32 = 1;
        let mut k = 0usize;
        loop {
            if mk(marks, HX[k] + x, HY[k] + y) != 0 || y <= 0 {
                break;
            }
            k += 1;
            if k == 6 {
                k = 0;
                y -= 1;
            }
        }
        'outer: loop {
            let mut k = 0usize;
            let mut flag = false;
            let mut steps = 0;
            loop {
                if flag && mk(marks, HX[k] + x, HY[k] + y) == 0 {
                    if steps >= 7 {
                        return;
                    }
                    self.mountain_inside(g, marks, x, y);
                    marks.mark(x - ox, y - oy, counter as u16);
                    counter += 1;
                    x += HX[k];
                    y += HY[k];
                    continue 'outer;
                }
                if steps >= 7 {
                    return;
                }
                if mk(marks, HX[k] + x, HY[k] + y) != 0 {
                    flag = true;
                }
                k = if k == 5 { 0 } else { k + 1 };
                steps += 1;
                if steps != 7 {
                    continue;
                }
                self.mountain_inside(g, marks, x, y);
                marks.mark(x - ox, y - oy, counter as u16);
                counter += 1;
                let mut done = false;
                let (mut bx, mut by) = (0, 0);
                while !done {
                    let mut all = true;
                    for j in 0..6 {
                        let (sx, sy) = (HX[j] + x, HY[j] + y);
                        let v = mk(marks, sx, sy);
                        if v == 0 {
                            steps = 0;
                            all = false;
                            break;
                        }
                        if v as i32 == mk(marks, x, y) as i32 - 1 {
                            bx = sx;
                            by = sy;
                        }
                    }
                    if all {
                        x = bx;
                        y = by;
                        counter = mk(marks, x, y) as u32;
                        done |= counter == 1;
                    } else {
                        break;
                    }
                }
            }
        }
    }

    /// vtable +8: a node of the fill.
    fn inside(&self, g: &mut Gen, marks: &mut Marks, x: i32, y: i32) {
        if let Kind::Mountain = self.kind {
            return self.mountain_inside(g, marks, x, y);
        }
        marks.mark(x - self.ox, y - self.oy, 1);
        match self.kind {
            Kind::Land { cx, cy, k, slope } => {
                let i = (self.size * y + x) as usize;
                if g.land[i][1] as i32 != self.value {
                    g.land[i][1] = self.value as u8;
                }
                if slope {
                    let d = ((x - cx).abs() + (y - cy).abs()) as f32;
                    if 20.0f32 <= d * k {
                        g.land[i][0] = g.land[i][0].wrapping_add(1);
                        return;
                    }
                    let r = g.rng.next() % 3;
                    let h = g.land[i][0];
                    g.land[i][0] = cvtt(((20.0f32 - d * k) + (r as f64) as f32) + h as f32) as u8;
                }
            }
            Kind::Object { id, spread, chance } => self.object_inside(g, x, y, id, spread, chance),
            Kind::Preview => g.preview[(self.size * y + x) as usize] = self.value as u16,
            Kind::Mountain => {}
        }
    }

    /// 0x691ef0: the outline between the mid-edge points (top, left, bottom, right), then the
    /// fill from the centre. Nothing for a side under 14.
    pub fn draw(&self, g: &mut Gen, marks: &mut Marks) {
        if self.x1 - self.x0 < 0xe || self.y1 - self.y0 < 0xe {
            return;
        }
        let xm = (self.x1 - self.x0) / 2 + self.x0;
        let ym = (self.y1 - self.y0) / 2 + self.y0;
        let px = [xm, self.x0, xm, self.x1];
        let py = [self.y0, ym, self.y1, ym];
        let s = self.size - 1;
        for i in 0..4 {
            let (mut x, mut y) = (px[i], py[i]);
            let (tx, ty) = if i == 3 { (px[0], py[0]) } else { (px[i + 1], py[i + 1]) };
            let mut dx = tx - x;
            let mut dy = ty - y;
            while x != tx || y != ty {
                match direction(g, dx.abs(), dy.abs()) {
                    1 => {
                        if tx > x && x < s {
                            x += 1;
                            dx -= 1;
                        }
                        if tx < x && x > 0 {
                            x -= 1;
                            dx += 1;
                        }
                    }
                    2 => {
                        let mut moved = false;
                        if tx < x {
                            if ty > y && y < s {
                                y += 1;
                                dy -= 1;
                                moved = true;
                            }
                        } else if ty >= y && x < s && y < s {
                            x += 1;
                            dx -= 1;
                            y += 1;
                            dy -= 1;
                            moved = true;
                        }
                        if !moved {
                            if tx > x {
                                if ty < y && y > 0 {
                                    y -= 1;
                                    dy += 1;
                                }
                            } else if ty <= y && x > 0 && y > 0 {
                                x -= 1;
                                dx += 1;
                                y -= 1;
                                dy += 1;
                            }
                        }
                    }
                    3 => {
                        let mut done = false;
                        if tx > x {
                            if ty > y && y < s {
                                y += 1;
                                dy -= 1;
                                done = true;
                            }
                        } else if ty >= y && x > 0 {
                            x -= 1;
                            dx += 1;
                            done = true;
                        }
                        if !done {
                            if tx < x {
                                if ty < y && y > 0 {
                                    y -= 1;
                                    dy += 1;
                                }
                            } else if ty <= y && x < s {
                                x += 1;
                                dx -= 1;
                            }
                        }
                    }
                    _ => {}
                }
                self.outline(g, marks, x, y);
            }
        }
        let (cx, cy) = ((self.x1 - self.x0) / 2 + self.x0, (self.y1 - self.y0) / 2 + self.y0);
        match self.kind {
            Kind::Mountain => self.mountain_fill(g, marks, cx, cy),
            _ => self.fill(g, marks, cx, cy),
        }
    }

    /// 0x692120: from (x, y) over the six hex neighbours (a 512-entry ring queue), painting each
    /// node not yet marked (or outside the marks).
    fn fill(&self, g: &mut Gen, marks: &mut Marks, x: i32, y: i32) {
        let n = self.size;
        let nn = n * n;
        let mut queue = [0i32; 512];
        queue[0] = n * y + x;
        let (mut head, mut tail) = (0usize, 1usize);
        self.inside(g, marks, x, y);
        loop {
            let cur = queue[head];
            head = (head + 1) & 0x1ff;
            for k in 0..6 {
                let c = match k {
                    0 => {
                        if cur - n > 0 { cur - n - 1 } else { 0 }
                    }
                    1 => {
                        if cur - n >= 0 { cur - n } else { 0 }
                    }
                    2 => (cur - 1).max(0),
                    3 => {
                        if cur + 1 >= nn { 0 } else { cur + 1 }
                    }
                    4 => {
                        if n + cur < nn { n + cur } else { 0 }
                    }
                    _ => {
                        if n + 1 + cur < nn { n + 1 + cur } else { 0 }
                    }
                };
                let (cy, cx) = (c / n, c % n);
                if marks.get(cx - self.ox, cy - self.oy).is_some_and(|v| v != 0) {
                    continue;
                }
                queue[tail] = c;
                tail = (tail + 1) & 0x1ff;
                self.inside(g, marks, cx, cy);
            }
            if head == tail {
                return;
            }
        }
    }
}

/// 0x6922d0: the walk's step kind from `rng % 100` against the steepness `|80 − 100·min/max|`
/// of the remaining (|dx|, |dy|): 1 along x, 2 diagonal, 3 along y (−1 past them).
fn direction(g: &mut Gen, a: i32, b: i32) -> i32 {
    let r = (g.rng.next() % 100) as i32;
    let (lo, hi) = if a < b { (a, b) } else { (b, a) };
    let v = 80.0f32 - (lo as f32 / hi as f32) * 100.0f32;
    let s = cvtt((v as f64).abs() as f32);
    let c = if s < 0x28 { s + 10 } else { (0x50 - s) / 3 };
    if r < c {
        1
    } else if r < s.wrapping_add(c) {
        2
    } else if r < 100 {
        3
    } else {
        -1
    }
}
