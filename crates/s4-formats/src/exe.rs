//! The player's own `S4_Main.exe` as a data file. Tables the game keeps in its image are read from
//! there at run time, like every other game file; this repository holds none of them.

/// The bytes the image holds at virtual address `va`, from the bytes of the exe.
pub fn bytes_at(exe: &[u8], va: u32, len: usize) -> Option<&[u8]> {
    let rd = |o: usize| exe.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let pe = rd(0x3c)? as usize;
    if exe.get(pe..pe + 4) != Some(b"PE\0\0") {
        return None;
    }
    let sections = exe.get(pe + 6..pe + 8).map(|b| u16::from_le_bytes([b[0], b[1]]))? as usize;
    let opt_size = exe.get(pe + 20..pe + 22).map(|b| u16::from_le_bytes([b[0], b[1]]))? as usize;
    let rva = va.checked_sub(rd(pe + 24 + 28)?)?;
    (0..sections).find_map(|s| {
        let h = pe + 24 + opt_size + 40 * s;
        let (vsize, vaddr, raw) = (rd(h + 8)?, rd(h + 12)?, rd(h + 20)?);
        (vaddr..vaddr + vsize).contains(&rva).then(|| (raw + rva - vaddr) as usize).and_then(|o| exe.get(o..o + len))
    })
}

/// `len` bytes at `va` of the installed game's exe; `None` without an install.
pub fn table(va: u32, len: usize) -> Option<Vec<u8>> {
    let exe = std::fs::read(crate::path::data("S4_Main.exe")).ok()?;
    bytes_at(&exe, va, len).map(<[u8]>::to_vec)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal image: one section at RVA 0x1000, file offset 0x200, image base 0x400000.
    fn image() -> Vec<u8> {
        let mut e = vec![0u8; 0x400];
        e[0x3c] = 0x80;
        e[0x80..0x84].copy_from_slice(b"PE\0\0");
        e[0x86] = 1;
        e[0x94] = 0xe0;
        e[0x80 + 24 + 28..0x80 + 24 + 32].copy_from_slice(&0x400000u32.to_le_bytes());
        let h = 0x80 + 24 + 0xe0;
        e[h + 8..h + 12].copy_from_slice(&0x100u32.to_le_bytes());
        e[h + 12..h + 16].copy_from_slice(&0x1000u32.to_le_bytes());
        e[h + 20..h + 24].copy_from_slice(&0x200u32.to_le_bytes());
        e[0x210..0x214].copy_from_slice(&[1, 2, 3, 4]);
        e
    }

    #[test]
    fn reads_by_virtual_address() {
        let e = image();
        assert_eq!(bytes_at(&e, 0x401010, 4), Some(&[1, 2, 3, 4][..]));
        assert_eq!(bytes_at(&e, 0x402000, 4), None, "outside every section");
        assert_eq!(bytes_at(&e, 0x400000, 4), None, "in the headers");
        assert_eq!(bytes_at(&[0; 16], 0x401010, 4), None, "not a PE image");
    }
}
