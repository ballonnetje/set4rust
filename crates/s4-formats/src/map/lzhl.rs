//! LZH-Light decompressor (Sergey Ignatchenko's LZHL, as linked into S4_Main).
//!
//! Mirrors `LZHLDecompressor::decompress` (0x2f2b5e0), `LZHLDecompressor::_get` (0x2f2c4f0),
//! `HuffStat::makeSortedTmp` (0x2f2a9a0) with `shellSort` (0x2f2bce0) / `cmpHuffStatTmpStruct`
//! (0x2f2c980), and the driver loop in `S4::CMapFile::LoadChunk` (0x13dad70) that keeps calling
//! `decompress` on one decompressor until the packed input is used up.
//!
//! Stream: MSB-first bit reader. A symbol is coded as a 4-bit group index followed by that group's
//! `nBits` bits, indexing into a 274-entry symbol permutation:
//! * `0..=255` literal byte;
//! * `256..=271` match; `matchOver` = `sym-256` for `<264`, else `LEN_TABLE[sym-264]` extra bits;
//!   length = `matchOver + 4`; then a 3-bit displacement class, `DISP_TABLE[class].0 + 9` bits,
//!   `disp = bits + (DISP_TABLE[class].1 << 9)`;
//! * `272` rebuild the Huffman tables: sort symbols by halved running frequency (descending,
//!   ties by descending symbol), then 16 unary-coded group widths (cumulative);
//! * `273` end of block. `decompress` returns there; the next call resets the bit count (but not
//!   the bit register) — i.e. the next block starts on a byte boundary.
//!
//! Every symbol (including 272/273) increments its 16-bit frequency counter.
//! The tables were copied from the shipped S4_Main.exe .data (file offsets 0xd67520 symbol
//! table, 0xd67748 group table, 0xdfd718 length table, 0xdfdb58 displacement table).

use crate::{Result, bail};

const NSYM: usize = 274;

/// `LZHLDecoderStat::symbolTable0`.
#[rustfmt::skip]
const SYMBOL_TABLE0: [i16; NSYM] = [
    256,257,258,259,260,261,262,263,264,265,266,267,268,269,270,271,0,32,48,255,
    1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,
    21,22,23,24,25,26,27,28,29,30,31,33,34,35,36,37,38,39,40,41,
    42,43,44,45,46,47,49,50,51,52,53,54,55,56,57,58,59,60,61,62,
    63,64,65,66,67,68,69,70,71,72,73,74,75,76,77,78,79,80,81,82,
    83,84,85,86,87,88,89,90,91,92,93,94,95,96,97,98,99,100,101,102,
    103,104,105,106,107,108,109,110,111,112,113,114,115,116,117,118,119,120,121,122,
    123,124,125,126,127,128,129,130,131,132,133,134,135,136,137,138,139,140,141,142,
    143,144,145,146,147,148,149,150,151,152,153,154,155,156,157,158,159,160,161,162,
    163,164,165,166,167,168,169,170,171,172,173,174,175,176,177,178,179,180,181,182,
    183,184,185,186,187,188,189,190,191,192,193,194,195,196,197,198,199,200,201,202,
    203,204,205,206,207,208,209,210,211,212,213,214,215,216,217,218,219,220,221,222,
    223,224,225,226,227,228,229,230,231,232,233,234,235,236,237,238,239,240,241,242,
    243,244,245,246,247,248,249,250,251,252,253,254,272,273,
];

/// `LZHLDecoderStat::groupTable0`: (nBits, first index).
const GROUP_TABLE0: [(i32, i32); 16] = [
    (2, 0), (3, 4), (3, 12), (4, 20), (4, 36), (4, 52), (4, 68), (4, 84),
    (4, 100), (4, 116), (4, 132), (4, 148), (4, 164), (5, 180), (5, 212), (5, 244),
];

/// Match-length extension for symbols 264..=271: (extra bits, base matchOver).
const LEN_TABLE: [(i32, i32); 8] =
    [(1, 8), (2, 10), (3, 14), (4, 22), (5, 38), (6, 70), (7, 134), (8, 262)];

/// Displacement classes: (extra bits beyond 9, base in units of 512).
const DISP_TABLE: [(i32, i32); 8] =
    [(0, 0), (0, 1), (1, 2), (2, 4), (3, 8), (4, 16), (5, 32), (6, 64)];

struct Decoder {
    stats: [i16; NSYM],
    symbols: [i16; NSYM],
    groups: [(i32, i32); 16],
    bits: u32,
    nbits: i32,
}

struct Src<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Decoder {
    fn new() -> Self {
        Self { stats: [0; NSYM], symbols: SYMBOL_TABLE0, groups: GROUP_TABLE0, bits: 0, nbits: 0 }
    }

    /// `_get`: n in 1..=8; -1 when the input is exhausted (and the bit count is cleared).
    fn get(&mut self, src: &mut Src, n: i32) -> i32 {
        if self.nbits < n {
            let Some(&b) = src.data.get(src.pos) else {
                self.nbits = 0;
                return -1;
            };
            src.pos += 1;
            self.bits |= (b as u32) << (24 - self.nbits);
            self.nbits += 8;
        }
        let v = self.bits >> (32 - n);
        self.bits <<= n;
        self.nbits -= n;
        v as i32
    }

    fn rebuild(&mut self, src: &mut Src) {
        // makeSortedTmp: snapshot (symbol, count), halve the counts, sort.
        let mut tmp: Vec<(i16, i16)> = (0..NSYM).map(|i| (i as i16, self.stats[i])).collect();
        for s in &mut self.stats {
            *s >>= 1;
        }
        // cmpHuffStatTmpStruct: higher count first, equal counts -> higher symbol first.
        tmp.sort_by(|a, b| b.1.cmp(&a.1).then(b.0.cmp(&a.0)));
        for (dst, t) in self.symbols.iter_mut().zip(&tmp) {
            *dst = t.0;
        }
        let mut nbits = 0;
        let mut pos = 0;
        for j in 0..self.groups.len() {
            let mut zeros = 0;
            while self.get(src, 1) == 0 {
                zeros += 1;
            }
            nbits += zeros;
            self.groups[j] = (nbits, pos);
            pos += 1i32.wrapping_shl(nbits as u32);
        }
    }

    /// One `decompress` call: decode until symbol 273 into `out` (capacity `cap`).
    fn block(&mut self, src: &mut Src, out: &mut Vec<u8>, cap: usize) -> Result<()> {
        self.nbits = 0;
        loop {
            let g = self.get(src, 4);
            if g < 0 {
                bail!("lzhl: input ended inside a block");
            }
            let (gbits, gpos) = self.groups[g as usize];
            let idx = if gbits != 0 {
                let v = self.get(src, gbits);
                if v < 0 {
                    bail!("lzhl: input ended inside a symbol");
                }
                v + gpos
            } else {
                gpos
            };
            if !(0..NSYM as i32).contains(&idx) {
                bail!("lzhl: symbol index {idx} out of range");
            }
            let sym = self.symbols[idx as usize];
            if !(0..NSYM as i16).contains(&sym) {
                bail!("lzhl: symbol {sym} out of range");
            }
            let sym = sym as usize;
            self.stats[sym] = self.stats[sym].wrapping_add(1);
            match sym {
                0..=255 => {
                    if out.len() >= cap {
                        bail!("lzhl: output overflow");
                    }
                    out.push(sym as u8);
                }
                272 => self.rebuild(src),
                273 => return Ok(()),
                _ => {
                    let over = if sym >= 264 {
                        let (nb, base) = LEN_TABLE[sym - 264];
                        let v = self.get(src, nb);
                        if v < 0 {
                            bail!("lzhl: input ended inside a length");
                        }
                        v + base
                    } else {
                        sym as i32 - 256
                    };
                    let class = self.get(src, 3);
                    if class < 0 {
                        bail!("lzhl: input ended inside a displacement");
                    }
                    let (dbits, dbase) = DISP_TABLE[class as usize];
                    let mut nb = dbits + 9;
                    let mut hi = 0;
                    if nb > 8 {
                        nb -= 8;
                        hi = self.get(src, 8) << nb;
                    }
                    let lo = self.get(src, nb);
                    if lo < 0 {
                        bail!("lzhl: input ended inside a displacement");
                    }
                    let disp = ((lo | hi) + (dbase << 9)) as usize;
                    let len = over as usize + 4;
                    if out.len() + len > cap {
                        bail!("lzhl: output overflow");
                    }
                    if disp == 0 || disp >= 0x10000 {
                        bail!("lzhl: bad displacement {disp}");
                    }
                    // The 64 KiB ring buffer starts with its last 256 bytes zeroed (LZBuffer ctor
                    // 0x2f2a330); reaching further back than that would read uninitialised memory.
                    if disp > out.len() + 256 {
                        bail!("lzhl: displacement {disp} before start of data");
                    }
                    for _ in 0..len {
                        let b = if disp > out.len() { 0 } else { out[out.len() - disp] };
                        out.push(b);
                    }
                }
            }
        }
    }
}

/// Decompress one chunk payload to exactly `unpacked_len` bytes.
pub fn decompress(packed: &[u8], unpacked_len: usize) -> Result<Vec<u8>> {
    let mut dec = Decoder::new();
    let mut src = Src { data: packed, pos: 0 };
    let mut out = Vec::with_capacity(unpacked_len);
    while src.pos < packed.len() {
        let before = src.pos;
        dec.block(&mut src, &mut out, unpacked_len)?;
        if src.pos == before {
            bail!("lzhl: empty block");
        }
    }
    if out.len() != unpacked_len {
        bail!("lzhl: produced {} bytes, header says {unpacked_len}", out.len());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_table_is_permutation() {
        let mut seen = [false; NSYM];
        for &s in &SYMBOL_TABLE0 {
            assert!(!seen[s as usize]);
            seen[s as usize] = true;
        }
        let total: i32 = GROUP_TABLE0.iter().map(|g| 1 << g.0).sum();
        assert_eq!(total, 276); // 274 symbols + 2 unused codes in the last group
    }
}
