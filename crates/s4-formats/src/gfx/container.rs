//! The 20-byte-header blob shared by every Gfx index and palette file.
//!
//! Layout (checked on every file in `Gfx\`):
//! ```text
//! 0x00 u16  magic   0x8000 = .gfx image data (0x8003 for the GUI .gfx files 0/9/18/19/28/29/39/40)
//!                   0x8003 = offset table (.gil .dil .jil .sil .pil .pi2 .pi4)
//!                   0x8001 = palette data (.p2x / .p4x; the GUI .pa5/.pa6 use 0x8003)
//! 0x02 ..   zero
//! 0x10 u32  version (compared against the expected value by CGfxManager::LoadFile 0x1364ff0)
//! 0x14 u32[] offset table (index files only), entry count = (file_len - 20) / 4
//! ```
//! `CGfxManager::LoadGilFile` (0x1365ce0) and its siblings replace an offset of 0 with 20
//! before relocating, so a zero entry aliases entry 0. `LoadSilFile` (0x13663d0) does *not*
//! relocate and keeps the raw value.

use crate::reader::Reader;
use crate::{Result, bail};

pub const HEADER_LEN: usize = 20;

/// Version field of a blob (offset 16).
pub fn version(data: &[u8], what: &'static str) -> Result<u32> {
    let mut r = Reader::at(data, 16, what);
    r.u32()
}

/// The raw u32 offset table of an index file (zero entries replaced by 20, as the loader does).
pub fn offsets(data: &[u8], what: &'static str) -> Result<Vec<u32>> {
    if data.len() < HEADER_LEN || !(data.len() - HEADER_LEN).is_multiple_of(4) {
        bail!("{what}: bad index file length {}", data.len());
    }
    let mut r = Reader::at(data, HEADER_LEN, what);
    let n = (data.len() - HEADER_LEN) / 4;
    (0..n).map(|_| Ok(r.u32()?.max(20))).collect()
}

/// Turn byte offsets into a table of u32 entries (the .dil/.jil/.sil convention) into entry indices.
pub fn to_indices(offs: &[u32], target_len: usize, what: &'static str) -> Result<Vec<u32>> {
    offs.iter()
        .map(|&o| {
            if o < 20 || !(o - 20).is_multiple_of(4) {
                bail!("{what}: misaligned offset {o:#x}");
            }
            let i = (o - 20) / 4;
            // An offset equal to the table end is legal (empty trailing group).
            if i as usize > target_len {
                bail!("{what}: offset {o:#x} beyond target table ({target_len} entries)");
            }
            Ok(i)
        })
        .collect()
}
