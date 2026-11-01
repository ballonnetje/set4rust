//! `s4_sim::sfloat` against the hardware singles (SSE on x86-64: round to nearest even).

use s4_sim::sfloat::Sf;

fn f(v: f32) -> Sf {
    Sf(v.to_bits())
}

/// xorshift for reproducible inputs.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn int(&mut self) -> i32 {
        match self.next() % 4 {
            0 => (self.next() % 200) as i32 - 100,
            1 => (self.next() % 100_000) as i32 - 50_000,
            2 => self.next() as i32,
            _ => ((self.next() % 0x200_0000) as i32) << (self.next() % 7),
        }
    }
    fn float(&mut self) -> f32 {
        match self.next() % 3 {
            0 => self.int() as f32,
            1 => (self.int() as f32) / ((self.next() % 1000 + 1) as f32),
            _ => f32::from_bits((self.next() as u32 & 0x7fff_ffff).min(0x7f7f_ffff) | ((self.next() as u32) & 0x8000_0000)),
        }
    }
}

#[test]
fn conversions_and_arithmetic_match_the_hardware() {
    let mut r = Rng(0x1234_5678_9abc_def1);
    for _ in 0..200_000 {
        let i = r.int();
        assert_eq!(Sf::from_i32(i), f(i as f32), "from_i32({i})");
        let (a, b) = (r.float(), r.float());
        if a.is_finite() && a.abs() < 2.0e9 {
            assert_eq!(f(a).to_i32_trunc(), a as i32, "trunc({a:e})");
        }
        let (sa, sb) = (f(a), f(b));
        for (name, got, want) in [("add", sa + sb, a + b), ("sub", sa - sb, a - b), ("mul", sa * sb, a * b), ("div", sa / sb, a / b)] {
            if want.is_nan() {
                continue;
            }
            // Denormal results (hardware without DAZ/FTZ) included.
            assert_eq!(got, f(want), "{name}({a:e}, {b:e}) = {:e}, want {want:e}", f32::from_bits(got.0));
        }
        assert_eq!(sa.ge(sb), a >= b, "ge({a:e}, {b:e})");
    }
}
