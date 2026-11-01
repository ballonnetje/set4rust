//! The object records the host builds (static init 0x564750, 8 bytes per object id at
//! 0x153f37c): what `CMapGeneratorHost::SetObject` 0x50a8f0 tests and marks.

use s4_formats::gamedata::GameData;

use crate::blob::{Blob, Marks};
use crate::generator::{Gen, cvtt};

/// One record: `[repelling ring, blocking, 0 / 1, blocking / 1, rings without buildings, 0, 0,
/// ping-pong]` (the derivation of s4-formats `ObjectInfo`: a non-blocking repellent object
/// (ids 162, 209, 210, 211, 255) gets `[1, 0, 1, 1, building + 1]`).
pub type Record = [u8; 8];

/// Records for object ids 0..=290 (0 for ids the XML does not list).
pub fn records(data: &GameData) -> Vec<Record> {
    let mut out = vec![[0u8; 8]; 291];
    for o in &data.objects {
        let id = o.object as usize;
        if id >= out.len() {
            continue;
        }
        let building = o.game_building();
        let r = &mut out[id];
        if o.blocking < 1 && o.game_repellent() {
            *r = [1, 0, 1, 1, (building + 1) as u8, 0, 0, 0];
        } else {
            let b = o.blocking as u8;
            *r = [0, b, 0, b, (o.blocking + building) as u8, 0, 0, 0];
        }
        r[7] = o.ping_pong as u8;
    }
    out
}

/// .data 0x117cf38 (word 0 of each 4-byte entry): the number of nodes within ring r - 1.
const RING_START: [usize; 11] = [0, 1, 7, 19, 37, 61, 91, 127, 175, 241, 301];

/// .data 0x1169968 (bytes 0 / 1 of each 4-byte entry): the node offsets (dx, dy), by rings.
const RING_OFFSETS: [(i32, i32); 301] = [
    (0, 0), (1, 0), (1, 1), (0, 1), (-1, 0), (-1, -1), (0, -1), (1, -1), (2, 0), (2, 1),
    (2, 2), (1, 2), (0, 2), (-1, 1), (-2, 0), (-2, -1), (-2, -2), (-1, -2), (0, -2), (3, 1),
    (2, 3), (-1, 2), (-3, -1), (-2, -3), (1, -2), (3, 2), (1, 3), (-2, 1), (-3, -2), (-1, -3),
    (2, -1), (3, 0), (3, 3), (0, 3), (-3, 0), (-3, -3), (0, -3), (4, 2), (2, 4), (-2, 2),
    (-4, -2), (-2, -4), (2, -2), (4, 1), (3, 4), (-1, 3), (-4, -1), (-3, -4), (1, -3), (4, 3),
    (1, 4), (-3, 1), (-4, -3), (-1, -4), (3, -1), (4, 0), (4, 4), (0, 4), (-4, 0), (-4, -4),
    (0, -4), (5, 2), (3, 5), (-2, 3), (-5, -2), (-3, -5), (2, -3), (5, 3), (2, 5), (-3, 2),
    (-5, -3), (-2, -5), (3, -2), (5, 1), (4, 5), (-1, 4), (-5, -1), (-4, -5), (1, -4), (5, 4),
    (1, 5), (-4, 1), (-5, -4), (-1, -5), (4, -1), (5, 0), (5, 5), (0, 5), (-5, 0), (-5, -5),
    (0, -5), (6, 3), (3, 6), (-3, 3), (-6, -3), (-3, -6), (3, -3), (6, 2), (4, 6), (-2, 4),
    (-6, -2), (-4, -6), (2, -4), (6, 4), (2, 6), (-4, 2), (-6, -4), (-2, -6), (4, -2), (6, 1),
    (5, 6), (-1, 5), (-6, -1), (-5, -6), (1, -5), (6, 5), (1, 6), (-5, 1), (-6, -5), (-1, -6),
    (5, -1), (6, 0), (6, 6), (0, 6), (-6, 0), (-6, -6), (0, -6), (7, 3), (4, 7), (-3, 4),
    (-7, -3), (-4, -7), (3, -4), (7, 4), (3, 7), (-4, 3), (-7, -4), (-3, -7), (4, -3), (7, 2),
    (5, 7), (-2, 5), (-7, -2), (-5, -7), (2, -5), (7, 5), (2, 7), (-5, 2), (-7, -5), (-2, -7),
    (5, -2), (7, 1), (6, 7), (-1, 6), (-7, -1), (-6, -7), (1, -6), (7, 6), (1, 7), (-6, 1),
    (-7, -6), (-1, -7), (6, -1), (7, 0), (7, 7), (0, 7), (-7, 0), (-7, -7), (0, -7), (8, 4),
    (4, 8), (-4, 4), (-8, -4), (-4, -8), (4, -4), (8, 3), (5, 8), (-3, 5), (-8, -3), (-5, -8),
    (3, -5), (8, 5), (3, 8), (-5, 3), (-8, -5), (-3, -8), (5, -3), (8, 2), (6, 8), (-2, 6),
    (-8, -2), (-6, -8), (2, -6), (8, 6), (2, 8), (-6, 2), (-8, -6), (-2, -8), (6, -2), (8, 1),
    (7, 8), (-1, 7), (-8, -1), (-7, -8), (1, -7), (8, 7), (1, 8), (-7, 1), (-8, -7), (-1, -8),
    (7, -1), (9, 4), (5, 9), (-4, 5), (-9, -4), (-5, -9), (4, -5), (9, 5), (4, 9), (-5, 4),
    (-9, -5), (-4, -9), (5, -4), (8, 0), (8, 8), (0, 8), (-8, 0), (-8, -8), (0, -8), (9, 3),
    (6, 9), (-3, 6), (-9, -3), (-6, -9), (3, -6), (9, 6), (3, 9), (-6, 3), (-9, -6), (-3, -9),
    (6, -3), (9, 2), (7, 9), (-2, 7), (-9, -2), (-7, -9), (2, -7), (9, 7), (2, 9), (-7, 2),
    (-9, -7), (-2, -9), (7, -2), (9, 1), (8, 9), (-1, 8), (-9, -1), (-8, -9), (1, -8), (9, 8),
    (1, 9), (-8, 1), (-9, -8), (-1, -9), (8, -1), (10, 5), (5, 10), (-5, 5), (-10, -5), (-5, -10),
    (5, -5), (10, 4), (6, 10), (-4, 6), (-10, -4), (-6, -10), (4, -6), (10, 6), (4, 10), (-6, 4),
    (-10, -6), (-4, -10), (6, -4), (10, 3), (7, 10), (-3, 7), (-10, -3), (-7, -10), (3, -7), (10, 7),
    (3, 10), (-7, 3), (-10, -7), (-3, -10), (7, -3), (9, 0), (9, 9), (0, 9), (-9, 0), (-9, -9),
    (0, -9),
];

impl Gen {
    /// The host's vtable +0x18 (0x50a8f0): object `id` at (x, y) if the node is in the map, free
    /// (object byte 0 zero) and unblocked (byte 2 & 0x43 zero), and per the record (+0 kind, +3
    /// rings `a`, +4 rings `b`): a kind 0 object needs rings `..a` free and unblocked, both kinds
    /// rings `a..b` unblocked. Then byte 0 = id; kind 0 sets 0x41 on rings `..a`, kind 1 0x42 on
    /// the node; rings `a..b` get 0x40.
    pub(crate) fn set_object(&mut self, x: i32, y: i32, id: i32) -> bool {
        let w = self.size;
        let inside = |x: i32, y: i32| (x as u32) < w as u32 && (y as u32) < w as u32;
        if !inside(x, y) {
            return false;
        }
        let i = (y * w + x) as usize;
        if self.objs[i][0] != 0 || self.objs[i][2] & 0x43 != 0 {
            return false;
        }
        let rec = self.records.get(id as usize).copied().unwrap_or_default();
        let (a, b) = (RING_START[rec[3] as usize], RING_START[rec[4] as usize]);
        let at = |j: usize| (x + RING_OFFSETS[j].0, y + RING_OFFSETS[j].1);
        if rec[0] == 0 {
            for j in 0..a {
                let (nx, ny) = at(j);
                if !inside(nx, ny) {
                    return false;
                }
                let o = self.objs[(ny * w + nx) as usize];
                if o[0] != 0 || o[2] & 0x43 != 0 {
                    return false;
                }
            }
        }
        for j in a..b {
            let (nx, ny) = at(j);
            if !inside(nx, ny) || self.objs[(ny * w + nx) as usize][2] & 0x43 != 0 {
                return false;
            }
        }
        self.objs[i][0] = id as u8;
        if rec[0] == 0 {
            for j in 0..a {
                let (nx, ny) = at(j);
                self.objs[(ny * w + nx) as usize][2] |= 0x41;
            }
        } else {
            self.objs[i][2] |= 0x42;
        }
        for j in a..b {
            let (nx, ny) = at(j);
            self.objs[(ny * w + nx) as usize][2] |= 0x40;
        }
        true
    }

    /// 0x68f0fb..0x68f6f9 (only when the block's +0x25 is 0): object blobs per grid cell over the
    /// cell's square (value 0x10: land types 0x1x; 0x40: 0x4x), as (id, spread, chance): 'h'
    /// (0x68): (1, 10, rng % 25 + 60), (0xae, 4, 40), (0xb5, 3, 5); 'i' (0x69): (0x7c, 11, 90);
    /// land / 'h' / 'i' / 'e': (0x4c, 10, 80), (0x2b, 7, 5); land one time in 10: (0xb8, 5, 80);
    /// land / 'h' / 'i' one time in 20: (0x38, 20, 2); 'j' (0x6a), value 0x40: (0x34, 2, −300),
    /// (0x78, 3, 0). Checkpoints after each blob's ctor and draw.
    pub(crate) fn scatter(&mut self, marks: &mut Marks) {
        let n = self.grid.n;
        let c = self.cell;
        for i in 0..n * n {
            let x = cvtt((i % n) as f32 * c + 50.0);
            let y = cvtt((i / n) as f32 * c + 50.0);
            let v = self.grid.at(i) as u8;
            let x1 = cvtt(x as f32 + c);
            let y1 = cvtt(y as f32 + c);
            let blob = |g: &mut Gen, marks: &mut Marks, value: i32, (id, spread, chance): (i32, u32, i32), at: [u32; 2]| {
                let b = Blob::new(g, marks, x, y, x1, y1, value, 0x32).object(id, spread, chance);
                g.cp(at[0]);
                b.draw(g, marks);
                g.cp(at[1]);
            };
            if v == 0x68 {
                let chance = (self.rng.next() % 0x19) as i32 + 0x3c;
                blob(self, marks, 0x10, (1, 10, chance), [0x68f1e2, 0x68f202]);
                blob(self, marks, 0x10, (0xae, 4, 0x28), [0x68f247, 0x68f27a]);
                blob(self, marks, 0x10, (0xb5, 3, 5), [0x68f2bf, 0x68f2f2]);
            }
            if v == 0x69 {
                blob(self, marks, 0x10, (0x7c, 0xb, 0x5a), [0x68f34b, 0x68f37e]);
            }
            if matches!(v, 0x69 | 1 | 0x68 | 0x65) {
                blob(self, marks, 0x10, (0x4c, 0xa, 0x50), [0x68f3d8, 0x68f40b]);
                blob(self, marks, 0x10, (0x2b, 7, 5), [0x68f450, 0x68f483]);
            }
            if v == 1 && self.rng.next().is_multiple_of(10) {
                blob(self, marks, 0x10, (0xb8, 5, 0x50), [0x68f4fd, 0x68f530]);
            }
            if matches!(v, 1 | 0x68 | 0x69) && self.rng.next().is_multiple_of(0x14) {
                blob(self, marks, 0x10, (0x38, 0x14, 2), [0x68f5aa, 0x68f5dd]);
            }
            if v == 0x6a {
                blob(self, marks, 0x40, (0x34, 2, -300), [0x68f633, 0x68f666]);
                blob(self, marks, 0x40, (0x78, 3, 0), [0x68f6ab, 0x68f6de]);
            }
        }
    }
}
