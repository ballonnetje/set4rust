//! Chunk-header obfuscation and checksums.
//!
//! * `S4::CMapFile::Cryption` (0x13dbc20) and the MapAccess copy `Cryption` (0x2fbff00) build a
//!   fresh `Cryptor` (ctor 0x2f029a0), call `Cryptor::Set_Key("01234567890123456789")`
//!   (0x2f02a50) and XOR every byte with `Cryptor::Transform_Char` (0x2f02c10). Only the 24-byte
//!   chunk headers go through it, and because a new `Cryptor` is made per call the keystream is the
//!   same 24 bytes for every header — computed here once by running the LFSRs.
//! * `S4::CMapFile::Crc` (0x13dbd00) / `FileCRC` (0x13dbd60) use
//!   `cdm_crc::CRCGenerator<16,0x8005,0,0,reflect,reflect>`: CRC-16/ARC (reflected poly 0xA001,
//!   init 0, no xorout). `GetNormalCRC` (0x13e65b0) byte-reverses the 32-bit register and the
//!   callers shift right by 16, so the value stored in files is the CRC with its two bytes swapped.

/// `Cryptor` member words 7..18 after construction (0x2f029a0); words 7..9 are the three LFSR
/// states, 10..12 their feedback taps, 13..18 the masks.
const TAPS: [u32; 3] = [0x8000_0062, 0x4000_0020, 0x1000_0002];
const KEEP: [u32; 3] = [0x7FFF_FFFF, 0x3FFF_FFFF, 0x0FFF_FFFF];
const SET: [u32; 3] = [0x8000_0000, 0xC000_0000, 0xF000_0000];

/// Keystream XORed over every 24-byte chunk header.
pub fn header_keystream() -> [u8; 24] {
    // Set_Key: each state = big-endian pack of 4 key chars (key >= 12 chars, so no padding).
    let key = b"01234567890123456789";
    let pack = |i: usize| u32::from_be_bytes([key[i], key[i + 1], key[i + 2], key[i + 3]]);
    let mut s = [pack(0), pack(4), pack(8)];
    let mut out = [0u8; 24];
    for byte in &mut out {
        // Transform_Char: the two output bits are re-seeded from states 1/2 for every char.
        let mut b1 = s[1] & 1;
        let mut b2 = s[2] & 1;
        let mut v = 0u8;
        for _ in 0..8 {
            if s[0] & 1 != 0 {
                s[0] = SET[0] | ((TAPS[0] ^ s[0]) >> 1);
                if s[1] & 1 != 0 {
                    s[1] = SET[1] | ((TAPS[1] ^ s[1]) >> 1);
                    b1 = 1;
                } else {
                    s[1] = KEEP[1] & (s[1] >> 1);
                    b1 = 0;
                }
            } else {
                s[0] = KEEP[0] & (s[0] >> 1);
                if s[2] & 1 != 0 {
                    s[2] = SET[2] | ((TAPS[2] ^ s[2]) >> 1);
                    b2 = 1;
                } else {
                    s[2] = KEEP[2] & (s[2] >> 1);
                    b2 = 0;
                }
            }
            v = ((b1 ^ b2) as u8) | (v << 1);
        }
        *byte = v;
    }
    out
}

const fn crc_table() -> [u16; 256] {
    let mut t = [0u16; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u16;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { (c >> 1) ^ 0xA001 } else { c >> 1 };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
}
static CRC_TABLE: [u16; 256] = crc_table();

/// The checksum exactly as `CMapFile::Crc` returns it (byte-swapped CRC-16/ARC).
pub fn s4_crc(data: &[u8]) -> u16 {
    let mut c = 0u16;
    for &b in data {
        c = (c >> 8) ^ CRC_TABLE[((c ^ b as u16) & 0xFF) as usize];
    }
    c.swap_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc16_arc_check_value() {
        // CRC-16/ARC("123456789") = 0xBB3D; the game stores it byte-swapped.
        assert_eq!(s4_crc(b"123456789"), 0x3DBB);
    }

    #[test]
    fn keystream_decodes_zero_padding() {
        // The first header of Campaign\AO_maya1.map has a zero `param` (bytes 20..24); on disk
        // those bytes are 36 7b 06 03, i.e. the keystream itself.
        assert_eq!(&header_keystream()[20..], &[0x36, 0x7b, 0x06, 0x03]);
    }
}
