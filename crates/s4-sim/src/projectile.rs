//! Flying effects: `CEffects` vtbl+0x18 (release 0x4e3b20 → 0x4e2310), the arrows, axes, stones
//! and darts the release starts when a shot or throw begins. Decompiles in
//! `tools/re/decompiled/sim/projectile*.c`.
//!
//! The effect only draws; the damage is the shooter's own business. Bowmen, axe throwers and
//! tower soldiers hit at the end of their WORK task (`jobs::warrior::hit`, 0x589250;
//! `jobs::soldier::tower_logic_update`, 0x58c200), the hunter's animal falls at the end of its
//! task: none of them reads the flight. The catapult (0x594c80) does: its stone hits by a future
//! event `flight + 1` ticks after the shot, `flight` being what [`launch`] returns.
//!
//! Callers (all six): the hunter's shot 0x57d60b, `Attack` 0x58a750 (bowmen and axe throwers),
//! the tower soldiers 0x58c38a / 0x58c46f, the catapult 0x594cae and a Lua function 0x5c43f0
//! (kind 0 between two nodes).
//!
//! The flight (0x4e2310, in `f32`): from the screen position of `from`
//! (`IGfxEngine::ConvertMapPositionToWorldScreenOffsets` 0x65b130, heights included) plus the
//! pixel offset, to that of `to`: with `d` the distance and `L = d − 2 · launch` (the kind's
//! launch distance), `max(1, (int)(speed · L)) + 1` ticks, or 1 tick when `L ≤ 0`. The effect
//! appears `delay` ticks after the call. The simulation has no floating point, so [`frames`]
//! does the release's `f32` steps exactly in integers ([`Dy`]); `tests/sim_projectile.rs`
//! compares it with real `f32` arithmetic. (The catapult's first port, `(isqrt(d²) − 2·launch) /
//! (1 / speed)`, equals it for every `d² < 2^24`, checked exhaustively; beyond that the `f32`
//! squares round.) The one 0x65b130 of the simulation is [`screen`] (also the mushroom farm's).

use crate::World;
use crate::event::SimEvent;
use crate::grid::Y16X16;

/// One record of .rdata 0x1000640 (stride 0x1c); the `f32` fields as their bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Kind {
    /// +0: the gfx effect (≥ 100: from the second effect file, minus 100).
    pub gfx: u32,
    /// +4: added to the start's screen y (`f32` bits).
    pub y_offset: u32,
    /// +8: the arc, the vertical speed factor (`f32` bits).
    pub arc: u32,
    /// +0xc: ticks per pixel (`f32` bits).
    pub speed: u32,
    /// +0x10: the distance from the shooter where the effect starts, and before the target where
    /// it ends (pixels; a whole `f32`).
    pub launch: i32,
    /// +0x18: the draw routine (table 0x10005fc; above 11 → 1).
    pub routine: u8,
}

/// .rdata 0x1000640: 0 arrow, 1 axe, 2..4 catapult stones (routine 10: they arc), 5 a tower
/// soldier's stone, 6 (no caller), 7 the Dark Tribe bowman's arrow, 8 / 9 the throwers of settler
/// types 0x3d..0x3f, 10 (no caller).
#[rustfmt::skip]
pub const KINDS: [Kind; 11] = [
    Kind { gfx: 1, y_offset: 0xc2280000, arc: 0x3dcccccd, speed: 0x3d4ccccd, launch: 50, routine: 4 },
    Kind { gfx: 84, y_offset: 0xc2000000, arc: 0x3d4ccccd, speed: 0x3d4ccccd, launch: 30, routine: 4 },
    Kind { gfx: 9, y_offset: 0xc1f00000, arc: 0x3e19999a, speed: 0x3d23d70a, launch: 90, routine: 10 },
    Kind { gfx: 5, y_offset: 0xc1a00000, arc: 0x3e000000, speed: 0x3d23d70a, launch: 90, routine: 10 },
    Kind { gfx: 10, y_offset: 0xc1a00000, arc: 0x3e000000, speed: 0x3d23d70a, launch: 90, routine: 10 },
    Kind { gfx: 19, y_offset: 0x00000000, arc: 0x00000000, speed: 0x3d4ccccd, launch: 0, routine: 4 },
    Kind { gfx: 23, y_offset: 0x00000000, arc: 0x00000000, speed: 0x00000000, launch: 0, routine: 2 },
    Kind { gfx: 65, y_offset: 0xc2280000, arc: 0x3dcccccd, speed: 0x3d4ccccd, launch: 50, routine: 4 },
    Kind { gfx: 113, y_offset: 0xc2280000, arc: 0x3dcccccd, speed: 0x3d4ccccd, launch: 50, routine: 4 },
    Kind { gfx: 112, y_offset: 0xc2280000, arc: 0x3dcccccd, speed: 0x3d4ccccd, launch: 50, routine: 4 },
    Kind { gfx: 107, y_offset: 0xc2280000, arc: 0x3dcccccd, speed: 0x3d4ccccd, launch: 50, routine: 10 },
];

/// `IGfxEngine::ConvertMapPositionToWorldScreenOffsets` (0x65b130), which is whole pixels:
/// `x' = (x − y / 2) · 24`, `y' = 12 · y − h`, `h = round(height · 0.0515625f · 24)` (the
/// height of the world manager's map element, 0 off the map; `cvtss2si`, ties to even). For
/// every height 0..=255 the `f32` steps give the ties-to-even rounding of `99 · height / 80`.
pub fn screen(world: &World, x: i32, y: i32) -> (i32, i32) {
    let h = world.grid.checked_index(x, y).map_or(0, |n| world.layers.height[n]);
    (24 * x - 12 * y, 12 * y - height_px(h))
}

/// `round(h · 0.0515625f · 24)` (ties to even) = `99h / 80` rounded half to even.
pub fn height_px(h: u8) -> i32 {
    let v = 99 * h as i32;
    let (q, r) = (v / 80, v % 80);
    q + (r > 40 || (r == 40 && q & 1 == 1)) as i32
}

/// An exact binary fraction `m · 2^e`, for the release's `f32` steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dy {
    pub m: i128,
    pub e: i32,
}

impl Dy {
    pub fn int(v: i64) -> Dy {
        Dy { m: v as i128, e: 0 }
    }

    /// The value of `f32` bits (normal numbers and zero).
    pub fn from_f32_bits(b: u32) -> Dy {
        let exp = ((b >> 23) & 0xff) as i32;
        if exp == 0 {
            return Dy::int(0);
        }
        let m = ((b & 0x7f_ffff) | 0x80_0000) as i128;
        Dy { m: if b >> 31 != 0 { -m } else { m }, e: exp - 150 }
    }

    fn align(a: Dy, b: Dy) -> (i128, i128, i32) {
        let e = a.e.min(b.e);
        (a.m << (a.e - e), b.m << (b.e - e), e)
    }

    /// Rounded to the nearest `f32` (24 significant bits, ties to even; no overflow or
    /// subnormals at these magnitudes). `above`: the exact value lies a little above `self`.
    fn round_sticky(self, above: bool) -> Dy {
        let neg = self.m < 0;
        let a = self.m.unsigned_abs();
        let bits = 128 - a.leading_zeros() as i32;
        if bits <= 24 {
            return self;
        }
        let drop = (bits - 24) as u32;
        let (mut q, rem, half) = (a >> drop, a & ((1u128 << drop) - 1), 1u128 << (drop - 1));
        if rem > half || (rem == half && (above || q & 1 == 1)) {
            q += 1;
        }
        let m = q as i128;
        Dy { m: if neg { -m } else { m }, e: self.e + drop as i32 }
    }

    pub fn round(self) -> Dy {
        self.round_sticky(false)
    }

    /// `(float)sqrt((double)x)` of a non-negative `f32` value: the correctly rounded `f32`
    /// square root (rounding through `f64` first cannot change it: 53 ≥ 2 · 24 + 2 bits).
    pub fn sqrt(self) -> Dy {
        if self.m <= 0 {
            return Dy::int(0);
        }
        let (mut m, mut e) = (self.m as u128, self.e);
        if e & 1 != 0 {
            m <<= 1;
            e -= 1;
        }
        // 60 more bits: the root has at least 30 significant bits.
        let big = m << 60;
        let r = isqrt(big);
        Dy { m: r as i128, e: e / 2 - 30 }.round_sticky(r * r != big)
    }

    /// `cvttss2si`: towards zero.
    pub fn trunc(self) -> i64 {
        if self.e >= 0 {
            (self.m << self.e) as i64
        } else {
            let d = 1i128 << -self.e;
            (self.m / d) as i64
        }
    }
}

/// Exact sums, differences and products (round with [`Dy::round`] where the `f32` step rounds).
impl std::ops::Add for Dy {
    type Output = Dy;
    fn add(self, b: Dy) -> Dy {
        let (x, y, e) = Dy::align(self, b);
        Dy { m: x + y, e }
    }
}

impl std::ops::Sub for Dy {
    type Output = Dy;
    fn sub(self, b: Dy) -> Dy {
        let (x, y, e) = Dy::align(self, b);
        Dy { m: x - y, e }
    }
}

impl std::ops::Mul for Dy {
    type Output = Dy;
    fn mul(self, b: Dy) -> Dy {
        Dy { m: self.m * b.m, e: self.e + b.e }
    }
}

fn isqrt(n: u128) -> u128 {
    if n < 2 {
        return n;
    }
    // Newton from above.
    let mut x = 1u128 << ((128 - n.leading_zeros()).div_ceil(2));
    loop {
        let y = (x + n / x) / 2;
        if y >= x {
            return x;
        }
        x = y;
    }
}

/// 0x4e2310's flight time in ticks (effect +0xc) of `kind` from `from` (+ the pixel offset) to
/// `to`.
pub fn frames(world: &World, kind: u8, from: Y16X16, to: Y16X16, offset: (i32, i32)) -> u32 {
    flight(kind, screen(world, from.x(), from.y()), screen(world, to.x(), to.y()), offset)
}

/// [`frames`] between two screen positions.
pub fn flight(kind: u8, (fx, fy): (i32, i32), (tx, ty): (i32, i32), offset: (i32, i32)) -> u32 {
    let Some(k) = KINDS.get(kind as usize) else { return 1 };
    // Whole numbers below 2^24 so far: exact in `f32`.
    let (dx, dy) = (Dy::int((tx - (offset.0 + fx)) as i64), Dy::int((ty - (offset.1 + fy)) as i64));
    let d2 = ((dy * dy).round() + (dx * dx).round()).round();
    let d = d2.sqrt();
    if d.m > 0 {
        let l = (d - Dy::int(2 * k.launch as i64)).round();
        if l.m > 0 {
            let n = (Dy::from_f32_bits(k.speed) * l).round().trunc().max(1);
            return n as u32 + 1;
        }
    }
    1
}

/// `CEffects` vtbl+0x18 `(kind, from, to, delay, ox, oy)` (0x4e3b20): the effect starts
/// (a [`SimEvent::Projectile`]) and the call returns `delay` + its flight. [deviation] The release
/// returns only `delay` when its effect pool has no free entry (+0x4226 = 0); s4rs keeps no pool.
pub fn launch(world: &mut World, kind: u8, from: Y16X16, to: Y16X16, delay: i32, offset: (i32, i32)) -> i32 {
    let frames = frames(world, kind, from, to, offset);
    world.events.push(SimEvent::Projectile { kind, from: (from.x(), from.y()), to: (to.x(), to.y()), delay, offset, frames });
    delay + frames as i32
}

/// The flying effect between two entities' positions (entity +0x18), if both exist.
pub fn launch_at(world: &mut World, kind: u8, shooter: u16, target: u16, delay: i32, offset: (i32, i32)) {
    let pos = |id: u16| world.entities.by_index(id as u32).map(|e| e.pos);
    if let (Some(from), Some(to)) = (pos(shooter), pos(target)) {
        launch(world, kind, from, to, delay, offset);
    }
}
