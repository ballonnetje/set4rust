//! IEEE-754 single precision in integers, for the few places where the release computes with
//! SSE singles (`cvtdq2ps`, `divss`, `subss`, `mulss`, `cvttss2si`) and the result decides
//! game state. Round to nearest even, as the SSE default; denormals are kept (no DAZ / FTZ).
//! Checked against the hardware in `tests/sfloat.rs`.

/// A single-precision value by its bits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Sf(pub u32);

const SIGN: u32 = 0x8000_0000;
const QNAN: u32 = 0x7fc0_0000;

impl Sf {
    pub const ZERO: Sf = Sf(0);
    pub const ONE: Sf = Sf(0x3f80_0000);

    fn sign(self) -> u32 {
        self.0 & SIGN
    }
    fn exp(self) -> i32 {
        ((self.0 >> 23) & 0xff) as i32
    }
    fn frac(self) -> u32 {
        self.0 & 0x7f_ffff
    }
    fn is_nan(self) -> bool {
        self.exp() == 0xff && self.frac() != 0
    }
    fn is_inf(self) -> bool {
        self.exp() == 0xff && self.frac() == 0
    }
    /// ±0.
    pub fn is_zero(self) -> bool {
        self.0 & !SIGN == 0
    }

    /// The significand with the hidden bit and the unbiased-style exponent (`value = m · 2^(e −
    /// 150)`), denormals normalised to exponent 1.
    fn unpack(self) -> (u32, i32) {
        if self.exp() == 0 { (self.frac(), 1) } else { (self.frac() | 0x80_0000, self.exp()) }
    }

    /// `cvtsi2ss` / `cvtdq2ps`.
    pub fn from_i32(v: i32) -> Sf {
        if v == 0 {
            return Sf::ZERO;
        }
        let sign = if v < 0 { SIGN } else { 0 };
        let m = v.unsigned_abs() as u64;
        // value = m · 2^0; normalise to 24 bits + 3 extra (guard, round, sticky) via `round`.
        round(sign, 150, m << 3)
    }

    /// `cvttss2si`: truncation; out of range or NaN gives 0x80000000.
    pub fn to_i32_trunc(self) -> i32 {
        if self.is_nan() || self.is_inf() {
            return i32::MIN;
        }
        let (m, e) = self.unpack();
        let shift = e - 150;
        let mag: i64 = if shift >= 0 {
            if shift > 40 {
                return i32::MIN;
            }
            (m as i64) << shift
        } else if shift <= -32 {
            0
        } else {
            (m as i64) >> (-shift)
        };
        let v = if self.sign() != 0 { -mag } else { mag };
        if v < i32::MIN as i64 || v > i32::MAX as i64 { i32::MIN } else { v as i32 }
    }

    fn negate(self) -> Sf {
        Sf(self.0 ^ SIGN)
    }

    /// `addss`.
    fn plus(self, o: Sf) -> Sf {
        if self.is_nan() || o.is_nan() {
            return Sf(QNAN);
        }
        if self.is_inf() || o.is_inf() {
            if self.is_inf() && o.is_inf() && self.sign() != o.sign() {
                return Sf(QNAN | SIGN);
            }
            return if self.is_inf() { self } else { o };
        }
        if self.is_zero() && o.is_zero() {
            return Sf(self.0 & o.0 & SIGN);
        }
        if self.is_zero() {
            return o;
        }
        if o.is_zero() {
            return self;
        }
        let (ma, ea) = self.unpack();
        let (mb, eb) = o.unpack();
        // Work with 3 extra bits and a wide sticky.
        let (mut a, mut b) = ((ma as u64) << 32, (mb as u64) << 32);
        let e = ea.max(eb);
        let sh = |v: u64, s: i32| -> u64 {
            if s >= 64 {
                (v != 0) as u64
            } else {
                let out = v >> s;
                out | ((out << s != v) as u64)
            }
        };
        a = sh(a, e - ea);
        b = sh(b, e - eb);
        let (sign, mag) = if self.sign() == o.sign() {
            (self.sign(), a + b)
        } else if a >= b {
            (self.sign(), a - b)
        } else {
            (o.sign(), b - a)
        };
        if mag == 0 {
            return Sf::ZERO;
        }
        // mag = m · 2^(e − 150 − 32); bring it to the 3-extra-bit form round() takes.
        round_wide(sign, e - 32, mag)
    }

    /// `subss`.
    fn minus(self, o: Sf) -> Sf {
        self.plus(o.negate())
    }

    /// `divss`.
    fn quotient(self, o: Sf) -> Sf {
        let sign = self.sign() ^ o.sign();
        if self.is_nan() || o.is_nan() {
            return Sf(QNAN);
        }
        if self.is_inf() {
            return if o.is_inf() { Sf(QNAN | SIGN) } else { Sf(sign | 0x7f80_0000) };
        }
        if o.is_inf() {
            return Sf(sign);
        }
        if o.is_zero() {
            return if self.is_zero() { Sf(QNAN | SIGN) } else { Sf(sign | 0x7f80_0000) };
        }
        if self.is_zero() {
            return Sf(sign);
        }
        let (mut ma, mut ea) = self.unpack();
        let (mut mb, mut eb) = o.unpack();
        // Denormals normalised, so the quotient keeps enough bits.
        while ma < 0x80_0000 {
            ma <<= 1;
            ea -= 1;
        }
        while mb < 0x80_0000 {
            mb <<= 1;
            eb -= 1;
        }
        // q = (ma << 40) / mb: at least 40 − 24 + 1 = 17 bits beyond the 24 needed.
        let n = (ma as u128) << 40;
        let q = n / mb as u128;
        let r = n % mb as u128;
        let q = (q as u64) | (r != 0) as u64;
        // value = q · 2^(ea − eb − 40) relative to 2^0; as m · 2^(e − 150): e = ea − eb + 150 − 40.
        round_wide(sign, ea - eb + 150 - 40, q)
    }

    /// `mulss`.
    fn product(self, o: Sf) -> Sf {
        let sign = self.sign() ^ o.sign();
        if self.is_nan() || o.is_nan() {
            return Sf(QNAN);
        }
        if self.is_inf() || o.is_inf() {
            return if self.is_zero() || o.is_zero() { Sf(QNAN | SIGN) } else { Sf(sign | 0x7f80_0000) };
        }
        if self.is_zero() || o.is_zero() {
            return Sf(sign);
        }
        let (ma, ea) = self.unpack();
        let (mb, eb) = o.unpack();
        let p = ma as u64 * mb as u64;
        // value = p · 2^(ea + eb − 300) = p · 2^(e − 150) with e = ea + eb − 150.
        round_wide(sign, ea + eb - 150, p)
    }

    /// `comiss` ordering for non-NaN values: `self >= o`.
    pub fn ge(self, o: Sf) -> bool {
        key(self) >= key(o)
    }
}

impl std::ops::Add for Sf {
    type Output = Sf;
    fn add(self, o: Sf) -> Sf {
        self.plus(o)
    }
}

impl std::ops::Sub for Sf {
    type Output = Sf;
    fn sub(self, o: Sf) -> Sf {
        self.minus(o)
    }
}

impl std::ops::Mul for Sf {
    type Output = Sf;
    fn mul(self, o: Sf) -> Sf {
        self.product(o)
    }
}

impl std::ops::Div for Sf {
    type Output = Sf;
    fn div(self, o: Sf) -> Sf {
        self.quotient(o)
    }
}

impl std::ops::Neg for Sf {
    type Output = Sf;
    fn neg(self) -> Sf {
        self.negate()
    }
}

/// A total order key for non-NaN singles.
fn key(v: Sf) -> i64 {
    if v.is_zero() {
        return 0;
    }
    let mag = (v.0 & !SIGN) as i64;
    if v.0 & SIGN != 0 { -mag } else { mag }
}

/// `m · 2^(e − 150)` with any number of bits: normalised and rounded.
fn round_wide(sign: u32, e: i32, m: u64) -> Sf {
    // Bring m to the form with exactly 27 significant bits (24 + guard, round, sticky).
    let bits = 64 - m.leading_zeros() as i32;
    let (mut m, mut e) = (m, e);
    if bits > 27 {
        let s = bits - 27;
        let out = m >> s;
        m = out | ((out << s != m) as u64);
        e += s;
    } else {
        m <<= 27 - bits;
        e -= 27 - bits;
    }
    // Now value = m · 2^(e − 150) with m in [2^26, 2^27): the significand is m >> 3 with e + 3.
    round(sign, e + 3, m)
}

/// `(m >> 3) · 2^(e − 150)` with m's low 3 bits guard / round / sticky (m may hold up to 27
/// significant bits, or fewer for small integers): rounded to nearest even.
fn round(sign: u32, e: i32, m: u64) -> Sf {
    let mut m = m;
    let mut e = e;
    // Normalise so the hidden bit is bit 26 (24 + 3 extra bits), keeping sticky.
    while m >= 1 << 27 {
        m = (m >> 1) | (m & 1);
        e += 1;
    }
    while m < 1 << 26 && m != 0 {
        m <<= 1;
        e -= 1;
    }
    let mut denormal = false;
    if e < 1 {
        // Denormal: shift right until the exponent is 1 (a denormal's scale), keeping sticky.
        let s = 1 - e;
        if s >= 32 {
            m = (m != 0) as u64;
        } else {
            let out = m >> s;
            m = out | ((out << s != m) as u64);
        }
        e = 1;
        denormal = true;
    }
    let low = m & 7;
    let mut sig = m >> 3;
    if low > 4 || (low == 4 && sig & 1 == 1) {
        sig += 1;
    }
    if denormal {
        // Exponent field 0 unless rounding reached the smallest normal.
        if sig >= 1 << 23 {
            return Sf(sign | (1 << 23) | (sig as u32 & 0x7f_ffff));
        }
        return Sf(sign | sig as u32);
    }
    if sig >= 1 << 24 {
        sig >>= 1;
        e += 1;
    }
    if e >= 0xff {
        return Sf(sign | 0x7f80_0000);
    }
    Sf(sign | ((e as u32) << 23) | (sig as u32 & 0x7f_ffff))
}
