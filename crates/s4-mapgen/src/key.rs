//! The random map key (`RD_` + 8 characters, base 32 with digits `0-9A-V`, the first character
//! the lowest five bits): parse 0x50ada0, build 0x50abf0.

/// The parameter block (`CRandomMaps` / `CMapGeneratorHost`, 0xb8 bytes) as the key sets it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Params {
    /// +4: the seed (20 bits from the key).
    pub seed: u32,
    /// +8: the map size, 256..=1216 in steps of 64 (the host clamps it to 64..=1024).
    pub size: u32,
    /// +0xc: 10..=160 in steps of 10 (the generator uses it + 9). [inf] the land mass.
    pub land: u32,
    /// +0x10: 5 / 10 / 15 / 20. [inf] the resources (the generator takes `% this` for variants).
    pub resources: u32,
    /// +0x14: the mirror mode, 0..=3 (bit 0, bit 1: the two axes).
    pub mirror: u32,
    /// +0x20: the players, 1..=8.
    pub players: u32,
    /// +0x25 (0x146b0d9): skips a pass of the main pass when set; the key leaves it 0.
    pub flag25: u8,
    /// The player records at +0x28 + 16 k: (+4, +8 & 1); the key leaves them 0 (+0 = 0x10).
    pub records: [(u32, bool); 9],
}

fn digit(c: u8) -> Option<u64> {
    match c {
        b'0'..=b'9' => Some((c - b'0') as u64),
        b'A'..=b'V' => Some((c - b'A' + 10) as u64),
        _ => None,
    }
}

/// The 40-bit value of a key (`RD_` optional).
pub fn value(key: &str) -> Option<u64> {
    let k = key.strip_prefix("RD_").unwrap_or(key).as_bytes();
    if k.len() != 8 {
        return None;
    }
    let mut v = 0u64;
    for (i, &c) in k.iter().enumerate() {
        v |= digit(c)? << (5 * i);
    }
    Some(v)
}

/// 0x50ada0.
pub fn parse(key: &str) -> Option<Params> {
    let v = value(key)?;
    Some(Params {
        players: (v & 7) as u32 + 1,
        resources: ((v >> 3 & 3) as u32 + 1) * 5,
        size: ((v >> 5 & 0xf) as u32 + 4) * 0x40,
        mirror: (v >> 9 & 3) as u32,
        land: ((v >> 11 & 0xf) as u32 + 1) * 10,
        seed: (v >> 15 & 0xfffff) as u32,
        ..Default::default()
    })
}

/// A typed key as the dialog takes it (`CRandomMaps` vtable +0x14, 0x50b2c0): 0x50ada0 reads
/// the first eight characters as digits (below `'A'`: `c − '0'`, else `c − 0x37`, on the low
/// byte as a signed char, so any character gives some value) into a 64-bit sum, and 0x50af60
/// keeps the key when players 1..=8, resources 5..=15, size 256..=1024 in steps of 64, mirror
/// 0..=3, land 10..=90 in steps of 10, seed 0..=999999 and the eighth character is `'0'`. The
/// dialog refuses fewer than 8 characters before (0x4981b0).
pub fn entered(key: &str) -> Option<Params> {
    let c: Vec<u16> = key.encode_utf16().collect();
    if c.len() < 8 {
        return None;
    }
    let mut v = 0i64;
    for (k, &w) in c[..8].iter().enumerate() {
        let b = w as u8 as i8;
        let d = if b < b'A' as i8 { b as i64 - 0x30 } else { b as i64 - 0x37 };
        v = v.wrapping_add(d.wrapping_shl(5 * k as u32));
    }
    let lo = v as u32;
    let p = Params {
        players: (lo & 7) + 1,
        resources: ((lo >> 3 & 3) + 1) * 5,
        size: ((lo >> 5 & 0xf) + 4) * 0x40,
        mirror: lo >> 9 & 3,
        land: ((lo >> 11 & 0xf) + 1) * 10,
        seed: (v >> 15) as u32 & 0xfffff,
        ..Default::default()
    };
    let ok = (1..=8).contains(&p.players)
        && (1..=15).contains(&p.resources)
        && p.size.is_multiple_of(64)
        && (0x100..=0x400).contains(&p.size)
        && p.mirror <= 3
        && p.land.is_multiple_of(10)
        && (10..=90).contains(&p.land)
        && p.seed <= 999_999
        && c[7] == b'0' as u16;
    ok.then_some(p)
}

/// `CRandomMaps` vtable +0x24 (0x50b420): a map name of a random map, `RD_…` (the key
/// follows), or a name in `[…]` / `<…>`.
pub fn is_random_name(name: &str) -> bool {
    let c: Vec<char> = name.chars().collect();
    (c.len() >= 3 && name.starts_with("RD_"))
        || (c.len() >= 2 && ((c[0] == '[' && c[c.len() - 1] == ']') || (c[0] == '<' && c[c.len() - 1] == '>')))
}

/// 0x50abf0 (without `RD_`).
pub fn build(p: &Params) -> String {
    let v: u64 = (p.seed as u64) * 0x8000
        + ((p.land / 10) as u64 * 0x800).wrapping_sub(0x800)
        + ((p.size / 64) as u64 - 4) * 0x20
        + ((p.resources / 5) as u64 * 8).wrapping_sub(8)
        + (p.mirror & 3) as u64 * 0x200
        + (p.players as u64 - 1);
    (0..8).map(|i| {
        let d = (v >> (5 * i) & 0x1f) as u8;
        (if d < 10 { b'0' + d } else { b'A' + d - 10 }) as char
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_round_trip() {
        let p = parse("RD_H8GQE2M0").unwrap();
        assert_eq!(build(&p), "H8GQE2M0");
        assert!((1..=8).contains(&p.players) && p.size.is_multiple_of(64));
    }

    #[test]
    fn entered_keys() {
        assert_eq!(entered("H8GQE2M0"), parse("RD_H8GQE2M0"));
        // Resources 20 and land 100 are keys the generator takes but the dialog refuses.
        let mut p = parse("RD_H8GQE2M0").unwrap();
        p.resources = 20;
        assert_eq!(entered(&build(&p)), None);
        assert_eq!(entered("H8GQE2M"), None);
        assert_eq!(entered("H8GQE2M1"), None);
    }
}
