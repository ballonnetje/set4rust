//! `projectile::flight` (0x4e2310's flight time, done in integers) against the release's `f32`
//! steps done in `f32`, and the screen heights (0x65b130) against their `f32` formula.

use s4_sim::projectile::{KINDS, flight};

fn f32_flight(kind: usize, (fx, fy): (i32, i32), (tx, ty): (i32, i32), (ox, oy): (i32, i32)) -> u32 {
    let k = KINDS[kind];
    let (sx, sy) = (ox as f32 + fx as f32, oy as f32 + fy as f32);
    let (dx, dy) = (tx as f32 - sx, ty as f32 - sy);
    let d = ((dy * dy + dx * dx) as f64).sqrt() as f32;
    if d > 0.0 {
        let launch = k.launch as f32;
        let l = d - (launch + launch);
        if l > 0.0 {
            let n = ((f32::from_bits(k.speed) * l) as i32).max(1);
            return n as u32 + 1;
        }
    }
    1
}

#[test]
fn the_integer_flight_equals_f32() {
    let mut seed = 12345u64;
    let mut rnd = |n: i32| {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((seed >> 33) % n as u64) as i32
    };
    for case in 0..200_000 {
        let kind = case % KINDS.len();
        // Screen positions of a 1024 map: x' in −12 288..24 576, y' in −316..12 276.
        let from = (rnd(36_864) - 12_288, rnd(12_592) - 316);
        let near = case % 3 == 0;
        let to = if near { (from.0 + rnd(801) - 400, from.1 + rnd(801) - 400) } else { (rnd(36_864) - 12_288, rnd(12_592) - 316) };
        let off = if case % 5 == 0 { (rnd(81) - 40, rnd(81) - 60) } else { (0, 0) };
        assert_eq!(flight(kind as u8, from, to, off), f32_flight(kind, from, to, off), "kind {kind} {from:?} → {to:?} + {off:?}");
    }
}

#[test]
fn screen_heights_equal_the_f32_rounding() {
    let c = f32::from_bits(0x3d53_3333);
    for h in 0..=255i32 {
        let f = ((h as f32 * c) * 24.0).round_ties_even() as i32;
        let v = 99 * h;
        let (q, r) = (v / 80, v % 80);
        assert_eq!(q + (r > 40 || (r == 40 && q & 1 == 1)) as i32, f, "height {h}");
    }
}
