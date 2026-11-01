//! The generator's state and its init 0x68c640 (with the layout grids 0x692fb0 / 0x693600, the
//! grid 0x690ea0 and its fill 0x690f50).
//!
//! The release keeps the state in globals: 0x146b0e8 the host, 0x146b0ec / 0x146b104 the host's
//! landscape / object buffers, 0x146b108 the map size, 0x146b0d4 the players, 0x146b0da their
//! count, 0x146b0f0 the RNG, 0x146b114 the layout grid, 0x146b110 the grid, 0x146b0f8 the grid
//! cell's size in nodes, 0x146b0fc the land amount + 9, 0x146b0d8 the resources, 0x146b0e4 the
//! mirror mode, 0x146b100 the seed, 0x146b0d9 the block's +0x25.

use crate::key::Params;

/// `cvttss2si`: truncation, with 0x80000000 for NaN and out-of-range values (Rust's `as`
/// saturates and gives 0 for NaN instead).
pub fn cvtt(f: f32) -> i32 {
    if f.is_nan() || !(-2147483648.0..2147483648.0).contains(&f) { i32::MIN } else { f as i32 }
}

/// `cvttsd2si`.
pub fn cvttd(f: f64) -> i32 {
    if f.is_nan() || !(-2147483648.0..2147483648.0).contains(&f) { i32::MIN } else { f as i32 }
}

/// 0x146b0f0: `state = state · 0x6c078965 + 1`, values `state >> 16`; +4 counts the draws.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rng {
    pub state: u32,
    pub count: u32,
}

impl Rng {
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> u32 {
        self.count = self.count.wrapping_add(1);
        self.state = self.state.wrapping_mul(0x6c078965).wrapping_add(1);
        self.state >> 16
    }
}

/// A player (0x20 bytes, 0x146b0d4[k]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Player {
    /// +0 / +4: the layout grid cell.
    pub bx: i32,
    pub by: i32,
    /// +8 / +0xc: the start node.
    pub sx: i32,
    pub sy: i32,
    /// +0x10 / +0x14: the grid cell of the 'f' next to the start.
    pub gx: i32,
    pub gy: i32,
    /// +0x18: the parameter block's player record +4; +0x1c: its +8 & 1.
    pub rec: u32,
    pub flag: bool,
}

/// The direction tables of the grid (+0x14 dx, +0x34 dy; 0x690ea0): from the upper left
/// clockwise.
pub const DX: [i32; 8] = [-1, 0, 1, 1, 1, 0, -1, -1];
pub const DY: [i32; 8] = [-1, -1, -1, 0, 1, 1, 1, 0];

/// The grid (0x54 bytes at 0x146b110): `n × n` cells, +8 / +0xc / +0x10 counts its fill sets.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Grid {
    pub n: i32,
    pub cells: Vec<u8>,
    pub c2: i32,
    pub c3: i32,
    pub c4: i32,
}

impl Grid {
    pub fn get(&self, x: i32, y: i32) -> u8 {
        self.cells[(y * self.n + x) as usize]
    }
    pub fn set(&mut self, x: i32, y: i32, v: u8) {
        self.cells[(y * self.n + x) as usize] = v;
    }
    /// The grid's own bounded read (`i < n²` else −1), as the main pass does it.
    pub fn at(&self, i: i32) -> i32 {
        if i < self.n * self.n { self.cells[i as usize] as i32 } else { -1 }
    }

    /// 0x691880: a breadth-first search from (x, y) over the 8 neighbours (a 256-entry ring
    /// queue, as the release's) for the cell of value `want` nearest by |dx| + |dy| among those
    /// reached (`through_set`: only through non-zero cells). Updates (x, y) and returns whether
    /// one was found.
    pub fn nearest(&self, x: &mut i32, y: &mut i32, want: u8, through_set: bool) -> bool {
        let n = self.n;
        let mut seen = vec![false; (n * n) as usize];
        let mut queue = [0i32; 256];
        let (x0, y0) = (*x, *y);
        queue[0] = n * y0 + x0;
        seen[(n * y0 + x0) as usize] = true;
        let (mut head, mut tail) = (0usize, 1usize);
        let (mut bx, mut by) = (0x400i32, 0x400i32);
        loop {
            let cur = queue[head];
            head = (head + 1) & 0xff;
            for k in 0..8 {
                let (cy, cx) = (cur / n, cur % n);
                let (mut nx, mut ny) = (cx, cy);
                match k {
                    0 => {
                        if cx > 0 && cy > 0 {
                            nx = cx - 1;
                            ny = cy - 1;
                        }
                    }
                    1 => {
                        if cy > 0 {
                            ny = cy - 1;
                        }
                    }
                    2 => {
                        if cx + 1 < n && cy > 0 {
                            nx = cx + 1;
                            ny = cy - 1;
                        }
                    }
                    3 => {
                        if cx > 0 {
                            nx = cx - 1;
                        }
                    }
                    4 => {
                        if cx + 1 < n {
                            nx = cx + 1;
                        }
                    }
                    5 => {
                        if cy + 1 < n {
                            ny = cy + 1;
                        }
                    }
                    6 => {
                        if cx + 1 < n && cy + 1 < n {
                            nx = cx + 1;
                            ny = cy + 1;
                        }
                    }
                    _ => {
                        if cx > 0 && cy + 1 < n {
                            nx = cx - 1;
                            ny = cy + 1;
                        }
                    }
                }
                let i = (n * ny + nx) as usize;
                if seen[i] {
                    continue;
                }
                seen[i] = true;
                if through_set && self.cells[i] == 0 {
                    continue;
                }
                queue[tail] = n * ny + nx;
                tail = (tail + 1) & 0xff;
                if self.cells[i] == want && (ny - y0).abs() + (nx - x0).abs() < (by - y0).abs() + (bx - x0).abs() {
                    by = ny;
                    bx = nx;
                }
            }
            if head == tail {
                if bx == 0x400 && by == 0x400 {
                    return false;
                }
                *x = bx;
                *y = by;
                return true;
            }
        }
    }
}

/// The layout grid (0x146b114: +4 cells, +8 n, +0xc the land amount + 9).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Layout {
    pub n: i32,
    pub cells: Vec<u8>,
    pub land: i32,
}

/// The generator.
#[derive(Clone, Debug)]
pub struct Gen {
    pub params: Params,
    /// 0x146b108.
    pub size: i32,
    /// The host's buffers (+0x138 landscape, +0x13c objects), 4 bytes per node.
    pub land: Vec<[u8; 4]>,
    pub objs: Vec<[u8; 4]>,
    pub rng: Rng,
    pub players: Vec<Player>,
    /// The host's start positions (+0xd8 / +0xfc, players 1..=8).
    pub starts: [(i32, i32); 9],
    pub layout: Layout,
    pub grid: Grid,
    /// 0x146b0f8: (size − 100) / grid n.
    pub cell: f32,
    /// 0x146b0f4: size / (150 − land − 9) − players.
    pub extra: i32,
    /// 0x146b0fc, 0x146b0d8, 0x146b0e4, 0x146b0d9.
    pub land_amount: i32,
    pub resources: u8,
    pub mirror: u32,
    pub flag25: u8,
    /// 0x116426d: size / 15 + 0x50 (a height).
    pub level: u8,
    /// Checkpoints of the main pass when recording (`Gen::cp`).
    pub trace: Option<Vec<String>>,
    /// The host's object records (0x153f37c, `objects::records`); none: all zero.
    pub records: Vec<crate::objects::Record>,
    /// The preview's canvas (0x146b0e0, 320 × 320 RGB555) while it is drawn.
    pub preview: Vec<u16>,
}

impl Gen {
    /// The host ctor 0x50a4d0 and its init 0x50a720, then the generator's init 0x68c640.
    pub fn new(params: &Params) -> Gen {
        let size = (params.size as i32).clamp(0x40, 0x400) & !0x3f;
        let nodes = (size * size) as usize;
        let mut land = vec![[0u8; 4]; nodes];
        for l in &mut land {
            l[1] = 7;
        }
        let players = params.players as usize & 0xff;
        let mut g = Gen {
            params: *params,
            size,
            land,
            objs: vec![[0u8; 4]; nodes],
            rng: Rng { state: params.seed, count: 0 },
            players: Vec::new(),
            starts: [(0, 0); 9],
            layout: Layout::default(),
            grid: Grid::default(),
            cell: 0.0,
            extra: 0,
            land_amount: params.land as i32 + 9,
            resources: params.resources as u8,
            mirror: params.mirror,
            flag25: params.flag25,
            level: ((size / 15) as u8).wrapping_add(0x50),
            trace: None,
            records: Vec::new(),
            preview: Vec::new(),
        };
        // sqrt(size) · 0.5, truncated, to a multiple of 4 (up above 512, down else).
        let mut n = ((size as f64).sqrt() * 0.5) as i32;
        let r = n % 4;
        if r != 0 {
            n = if size > 0x200 { n + 4 - r } else { n - r };
        }
        g.extra = size / (0x96 - g.land_amount) - players as i32;
        g.cell = (size - 100) as f32 / n as f32;
        for k in 0..players {
            let (rec, flag) = params.records.get(k).copied().unwrap_or((0, false));
            g.players.push(Player { rec, flag, ..Default::default() });
        }
        g.layout = Layout { n: n / 2, cells: vec![0; ((n / 2) * (n / 2)) as usize], land: g.land_amount };
        if g.mirror & 3 == 0 {
            g.basic_layout();
        } else {
            g.mirror_layout();
        }
        g.grid = Grid { n, cells: vec![0; (n * n) as usize], ..Default::default() };
        g.fill_grid();
        let c = g.cell;
        for k in 0..players {
            let p = &mut g.players[k];
            p.sx = ((p.bx as f32 * c + p.bx as f32 * c + 50.0) + c) as i32;
            let y = p.by as f32 * c;
            p.sy = ((y + y + 50.0) + c) as i32;
            if k < 8 {
                g.starts[k + 1] = (p.sx, p.sy);
            }
        }
        g
    }

    /// `CBasicGrid` vtable 0 (0x692fb0): the players' cells spread by a cubic falloff along the
    /// rows and columns, then the land cells (1) at random free cells.
    fn basic_layout(&mut self) {
        let n = self.layout.n;
        let x0 = (self.rng.next() % n as u32) as i32;
        let y0 = (self.rng.next() % n as u32) as i32;
        self.layout.cells[(n * y0 + x0) as usize] = 0x65;
        self.players[0].bx = x0;
        self.players[0].by = y0;
        let mut xs = vec![0i32; n as usize];
        let mut ys = vec![0i32; n as usize];
        let (mut px, mut py) = (x0, y0);
        let cube = |v: i32| {
            let d = v as f64;
            d * d * d
        };
        let peak = cube(n);
        for k in 1..self.players.len() {
            xs[px as usize] = (xs[px as usize] as f64 + peak) as i32;
            ys[py as usize] = (ys[py as usize] as f64 + peak) as i32;
            for i in 1..n {
                let v = cube(n - i) as i32;
                if px - i >= 0 {
                    xs[(px - i) as usize] += v;
                }
                if px + i < n {
                    xs[(px + i) as usize] += v;
                }
                if py - i >= 0 {
                    ys[(py - i) as usize] += v;
                }
                if py + i < n {
                    ys[(py + i) as usize] += v;
                }
            }
            let (mut ax, mut ay) = (0i32, 0i32);
            for c in 1..n {
                if xs[ax as usize] > xs[c as usize] {
                    ax = c;
                }
                if ys[ay as usize] > ys[c as usize] {
                    ay = c;
                }
            }
            self.players[k].bx = ax;
            self.players[k].by = ay;
            let placed: Vec<(i32, i32)> = self.players[..k].iter().map(|p| (p.bx, p.by)).collect();
            let mut far = 0i32;
            let mut nx = ax;
            let thr = (xs[ax as usize] as f64 * 0.4) as i32;
            for c in 0..n {
                if c == ax || xs[ax as usize] + thr <= xs[c as usize] {
                    continue;
                }
                let (mut da, mut dc) = (2 * n, 2 * n);
                for &(qx, qy) in &placed {
                    let dy = (qy - ay).abs();
                    da = da.min((qx - ax).abs() + dy);
                    dc = dc.min((qx - c).abs() + dy);
                }
                if dc > da && dc > far {
                    nx = c;
                    far = dc;
                }
            }
            let mut ny = ay;
            let thr = (ys[ay as usize] as f64 * 0.4) as i32;
            for c in 0..n {
                if c == ay || ys[ay as usize] + thr <= ys[c as usize] {
                    continue;
                }
                let (mut da, mut dc) = (2 * n, 2 * n);
                for &(qx, qy) in &placed {
                    let dx = (qx - nx).abs();
                    da = da.min((qy - ay).abs() + dx);
                    dc = dc.min((qy - c).abs() + dx);
                }
                if dc > da && dc > far {
                    ny = c;
                    far = dc;
                }
            }
            self.layout.cells[(n * ny + nx) as usize] = 0x65;
            self.players[k].bx = nx;
            self.players[k].by = ny;
            px = nx;
            py = ny;
        }
        let nn = (n * n) as u32;
        let count = ((self.layout.land as f32 / 100.0) * nn as f32 - self.players.len() as f32) as i32;
        for _ in 0..count.max(0) {
            let mut i = self.rng.next() % nn;
            while self.layout.cells[i as usize] != 0 {
                i = if i + 1 < nn { i + 1 } else { 0 };
            }
            self.layout.cells[i as usize] = 1;
        }
    }

    /// `CMirrorBasicGrid` vtable 0 (0x693600): land cells in the part the mirror copies (mode 1
    /// y ≥ x, mode 2 y < n − x, mode 3 the quarter), then the players: a free cell from
    /// [`Gen::mirror_cell`] (40 tries), its mirror images taking the next players; the players
    /// get the positions in the order of the four lists (own, x↔y, anti-diagonal, both).
    fn mirror_layout(&mut self) {
        let n = self.layout.n;
        let players = self.players.len() as i32;
        let mode = self.mirror & 3;
        let f = if mode == 3 { 0.25f32 } else { 0.5f32 };
        let count = ((((self.layout.land as f32 / 100.0) * (n * n) as f32) - players as f32) * f) as i32;
        for _ in 0..count.max(0) {
            let (mut x, mut y);
            loop {
                match mode {
                    1 => {
                        x = self.rng.next() % n as u32;
                        y = self.rng.next() % (n as u32 - x) + x;
                    }
                    2 => {
                        x = self.rng.next() % n as u32;
                        y = self.rng.next() % (n as u32 - x);
                    }
                    _ => {
                        x = self.rng.next() % (n / 2) as u32;
                        y = self.rng.next() % (n as u32).wrapping_sub(2 * x) + x;
                    }
                }
                if self.layout.cells[(n as u32 * y + x) as usize] == 0 {
                    break;
                }
            }
            self.layout.cells[(n as u32 * y + x) as usize] = 1;
        }
        // Lists: 0 own (0..4), 1 x↔y (4..8), 2 anti-diagonal (8..12), 3 both (12..16).
        let mut lists = [-1i32; 16];
        let mut fill = [0usize, 4, 8, 12];
        let mut pos = [(0i32, 0i32); 16];
        let mut i = 0i32;
        while i < players {
            let (mut x, mut y) = (0, 0);
            for _ in 0..0x28 {
                (x, y) = self.mirror_cell(false);
                let c = self.layout.cells[(n * y + x) as usize];
                if c != 0x65 && c != 1 {
                    break;
                }
            }
            pos[i as usize] = (x, y);
            self.layout.cells[(n * y + x) as usize] = 0x65;
            lists[fill[0]] = i;
            fill[0] += 1;
            let mut j = i + 1;
            if mode & 2 != 0 && j < players {
                pos[j as usize] = (n - y - 1, n - x - 1);
                lists[fill[2]] = j;
                fill[2] += 1;
                j = i + 2;
            }
            i = j;
            if mode & 1 != 0 && i < players {
                pos[i as usize] = (y, x);
                lists[fill[1]] = i;
                fill[1] += 1;
                i += 1;
            }
            if mode == 3 {
                if i >= players {
                    break;
                }
                pos[i as usize] = (n - x - 1, n - y - 1);
                lists[fill[3]] = i;
                fill[3] += 1;
                i += 1;
            }
        }
        let mut k = 0;
        for &e in &lists {
            if e != -1 {
                self.players[k].bx = pos[e as usize].0;
                self.players[k].by = pos[e as usize].1;
                k += 1;
            }
        }
    }

    /// 0x693a00 with its last argument 0: a random layout cell in the mirror's own part.
    fn mirror_cell(&mut self, whole: bool) -> (i32, i32) {
        let n = self.layout.n as u32;
        let m = self.mirror;
        let (x, y);
        if m & 1 != 0 && m & 2 == 0 {
            let r = self.rng.next();
            if !whole {
                x = r % (n - 1);
                y = x + 1 + self.rng.next() % (n - x - 1);
            } else {
                x = r % n;
                y = self.rng.next() % (n - x) + x;
            }
        } else if m & 2 != 0 && m & 1 == 0 {
            let r = self.rng.next();
            if !whole {
                x = r % (n - 1);
                y = self.rng.next() % (n - x - 1);
            } else {
                x = r % n;
                y = self.rng.next() % (n - x);
            }
        } else {
            let r = self.rng.next();
            if whole {
                x = r % (n / 2);
                y = self.rng.next() % (n - 2 * x) + x;
            } else {
                x = r % ((n - 1) / 2);
                y = x + 1 + self.rng.next() % (n - 2 * x - 2);
            }
        }
        (x as i32, y as i32)
    }

    /// 0x690f50: the layout grid's cells as 2 × 2 grid cells (land cells lose one at random
    /// where not surrounded), then the counted sets: 'f' (0x66) pairs, the players' 'f' next to
    /// their start, 'h' (0x68) / 'i' (0x69) at land, 'h' / 'j' (0x6a) / 'i' at the starts.
    fn fill_grid(&mut self) {
        let ln = self.layout.n;
        let lay = |x: i32, y: i32| self.layout.cells[(ln * y + x) as usize];
        let mut set = 0i32;
        let mut cells = std::mem::take(&mut self.grid.cells);
        let gn = self.grid.n;
        for y in 0..ln {
            for x in 0..ln {
                if lay(x, y) == 1 {
                    let inside = x + 1 < ln
                        && x > 0
                        && y + 1 < ln
                        && y > 0
                        && lay(x + 1, y) != 0
                        && lay(x - 1, y) != 0
                        && lay(x, y + 1) != 0
                        && lay(x, y - 1) != 0;
                    let skip = if inside { 100 } else { self.rng.next() % 6 };
                    for (k, (dx, dy)) in [(0, 0), (1, 0), (0, 1), (1, 1)].into_iter().enumerate() {
                        if skip != k as u32 {
                            set += 1;
                            cells[((2 * y + dy) * gn + 2 * x + dx) as usize] = 1;
                        }
                    }
                }
                if lay(x, y) == 0x65 {
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        cells[((2 * y + dy) * gn + 2 * x + dx) as usize] = 0x65;
                    }
                }
            }
        }
        self.grid.cells = cells;
        let players = self.players.len() as i32;
        let land = set - players;
        let half = land as f32 * 0.5;
        let part = (land as f32 * 0.15f32) as i32;
        self.grid.c3 = half as i32;
        self.grid.c2 = part;
        self.grid.c4 = part;
        let m = self.mirror & 3;
        if m != 0 {
            self.grid.c3 = half as i32 + (players >> 1);
            self.grid.c4 = (players >> 1) + part;
        }
        if m == 3 {
            self.grid.c3 += players >> 2;
            self.grid.c4 += players >> 2;
        }
        // 'f' pairs at land.
        for _ in 0..self.grid.c2 {
            let mut tries = 0;
            let (mut x, mut y);
            loop {
                x = (self.rng.next() % gn as u32) as i32;
                y = (self.rng.next() % gn as u32) as i32;
                if self.grid.nearest(&mut x, &mut y, 1, true) {
                    break;
                }
                if tries >= 5 {
                    break;
                }
                tries += 1;
            }
            if tries >= 5 {
                continue;
            }
            self.grid.set(x, y, 0x66);
            let (mut tx, mut ty) = (x, y);
            if x + 1 < gn && self.grid.get(x + 1, y) == 1 {
                tx = x + 1;
            } else if y + 1 < gn && self.grid.get(x, y + 1) == 1 {
                ty = y + 1;
            } else if x - 1 > 0 && self.grid.get(x - 1, y) == 1 {
                tx = x - 1;
            } else if y - 1 > 0 && self.grid.get(x, y - 1) == 1 {
                ty = y - 1;
            }
            self.grid.set(tx, ty, 0x66);
        }
        // Each player's 'f' within 2 of the start, else one placed around it.
        for k in 0..self.players.len() {
            let (sx, sy) = (2 * self.players[k].bx, 2 * self.players[k].by);
            let (mut x, mut y) = (sx, sy);
            if self.grid.nearest(&mut x, &mut y, 0x66, true) && (y - sy).abs() + (x - sx).abs() <= 2 {
                self.players[k].gx = x;
                self.players[k].gy = y;
                continue;
            }
            let r = self.rng.next() & 7;
            let mut d = if r < 7 { r + 1 } else { 0 };
            let mut bx = if sx != 0 { sx } else { 1 };
            if bx == gn - 1 {
                bx -= 1;
            }
            let mut by = if sy != 0 { sy } else { 1 };
            if by == gn - 1 {
                by -= 1;
            }
            let mut placed = false;
            while d != r && !placed {
                if self.mirror & 3 == 3 && d == 4 && r != 5 {
                    d = 5;
                }
                let (cx, cy) = (DX[d as usize] + bx, DY[d as usize] + by);
                if cx >= 0 && cx < gn && cy >= 0 && cy < gn && self.grid.get(cx, cy) != 0x65 {
                    self.grid.set(cx, cy, 0x66);
                    placed = true;
                    self.players[k].gx = cx;
                    self.players[k].gy = cy;
                }
                d = if d + 1 != 8 { d + 1 } else { 0 };
            }
        }
        // 'h' and 'i' at land, counted up to c3 / c4 (one more each, as the release counts).
        let (mut h, mut i) = (0i32, 0i32);
        for _ in 0..land.max(0) {
            if h <= self.grid.c3 {
                let mut x = (self.rng.next() % gn as u32) as i32;
                let mut y = (self.rng.next() % gn as u32) as i32;
                if self.grid.nearest(&mut x, &mut y, 1, false) {
                    h += 1;
                    self.grid.set(x, y, 0x68);
                }
            }
            if i <= self.grid.c4 {
                let mut x = (self.rng.next() % gn as u32) as i32;
                let mut y = (self.rng.next() % gn as u32) as i32;
                if self.grid.nearest(&mut x, &mut y, 1, false) {
                    i += 1;
                    self.grid.set(x, y, 0x69);
                }
            }
        }
        // An 'h' on the start block's other cells.
        for k in 0..self.players.len() {
            let r = self.rng.next() % 3;
            let s = r + 3;
            let mut d = if s < 7 { s + 1 } else { 0 };
            let (bx, by) = (2 * self.players[k].bx, 2 * self.players[k].by);
            let mut done = false;
            while d != s && !done {
                let (cx, cy) = (DX[d as usize] + bx, DY[d as usize] + by);
                if cx >= 0 && cx < gn && cy >= 0 && cy < gn && self.grid.get(cx, cy) == 0x65 {
                    self.grid.set(cx, cy, 0x68);
                    done = true;
                }
                d = if d + 1 != 8 { d + 1 } else { 0 };
            }
        }
        // The 'j' (0x6a) nearest the start: land within 3, else the nearest empty cell.
        for k in 0..self.players.len() {
            let (sx, sy) = (2 * self.players[k].bx, 2 * self.players[k].by);
            let (mut x, mut y) = (sx, sy);
            let found = self.grid.nearest(&mut x, &mut y, 1, true);
            if !found || (y - sy).abs() + (x - sx).abs() > 3 {
                x = sx;
                y = sy;
                self.grid.nearest(&mut x, &mut y, 0, false);
            }
            self.grid.set(x, y, 0x6a);
        }
        // An 'i' next to the start on land or the start block.
        for k in 0..self.players.len() {
            let r = self.rng.next() & 7;
            let mut d = if r < 7 { r + 1 } else { 0 };
            let (bx, by) = (2 * self.players[k].bx, 2 * self.players[k].by);
            let mut done = false;
            while d != r && !done {
                let (cx, cy) = (DX[d as usize] + bx, DY[d as usize] + by);
                if cx >= 0 && cx < gn && cy >= 0 && cy < gn {
                    let v = self.grid.get(cx, cy);
                    if v == 1 || v == 0x65 {
                        self.grid.set(cx, cy, 0x69);
                        done = true;
                    }
                }
                d = if d + 1 != 8 { d + 1 } else { 0 };
            }
        }
    }
}
