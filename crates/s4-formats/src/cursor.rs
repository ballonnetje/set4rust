//! The game's mouse cursors: 36 Windows cursor resources of `S4_Main.exe` itself. The engine init
//! (release 0x65b6b0 / 0x65bef0) loads them with `LoadCursorA(hInstance, id)` into `CursorList`
//! (0x1458718) in the order of [`IDS`]; `IGfxEngine::SetCursorShape` (0x65e5b0) selects one by
//! index (below 0x24) with `SetCursor` and the window class. All 36 are 32 × 32, 1 bit per pixel,
//! black and white, and none uses the screen-inverting pixel (AND 1 with colour 1), so they
//! convert to RGBA exactly. Read from the PE's resource section: `RT_GROUP_CURSOR` (12) names
//! the `RT_CURSOR` (1) entry, which is the hotspot (2 × u16) and a DIB (XOR colour bits, then the
//! AND mask; the height counts both).

use crate::{Result, bail};

/// The resource ids of `CursorList` 0..36 (the pushes before the 36 `LoadCursorA` calls).
pub const IDS: [u16; 36] = [
    0x65, 0x66, 0x6d, 0x6e, 0x6f, 0x70, 0x71, 0x72, 0x77, 0x78, 0x79, 0x7a, 0x7b, 0x7c, 0x7d, 0x7e, 0x7f, 0x80, 0x81, 0x82, 0x83,
    0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8b, 0x8c, 0x8d, 0x8e, 0x8f, 0x90, 0x91, 0x92, 0x93,
];

/// One cursor: RGBA pixels (row-major, top row first) and the hotspot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cursor {
    pub width: u32,
    pub height: u32,
    pub hotspot: (u16, u16),
    pub rgba: Vec<u8>,
}

fn u16_at(d: &[u8], o: usize) -> Result<u16> {
    d.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]])).ok_or_else(|| crate::Error::Format(format!("cursor: short read at {o}")))
}

fn u32_at(d: &[u8], o: usize) -> Result<u32> {
    d.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).ok_or_else(|| crate::Error::Format(format!("cursor: short read at {o}")))
}

/// The resource section of a PE image: its file bytes from the section start, its RVA.
struct Rsrc<'a> {
    data: &'a [u8],
    rva: u32,
}

impl<'a> Rsrc<'a> {
    fn of(exe: &'a [u8]) -> Result<Rsrc<'a>> {
        let pe = u32_at(exe, 0x3c)? as usize;
        if exe.get(pe..pe + 4) != Some(b"PE\0\0") {
            bail!("cursor: not a PE image");
        }
        let sections = u16_at(exe, pe + 6)? as usize;
        let opt = pe + 24;
        let opt_size = u16_at(exe, pe + 20)? as usize;
        // PE32: the data directories start at +96; the resource table is the third.
        let rsrc_rva = u32_at(exe, opt + 96 + 2 * 8)?;
        for s in 0..sections {
            let h = opt + opt_size + 40 * s;
            let (va, raw_size, raw) = (u32_at(exe, h + 12)?, u32_at(exe, h + 16)?, u32_at(exe, h + 20)?);
            if (va..va + raw_size).contains(&rsrc_rva) {
                let start = (raw + rsrc_rva - va) as usize;
                let end = (raw + raw_size) as usize;
                let data = exe.get(start..end.min(exe.len())).ok_or_else(|| crate::Error::Format("cursor: resource section".into()))?;
                return Ok(Rsrc { data, rva: rsrc_rva });
            }
        }
        bail!("cursor: no resource section")
    }

    /// The entries of the directory at `off`: (id, offset, is a directory).
    fn entries(&self, off: usize) -> Result<Vec<(u32, usize, bool)>> {
        let n = u16_at(self.data, off + 12)? as usize + u16_at(self.data, off + 14)? as usize;
        (0..n)
            .map(|i| {
                let e = off + 16 + 8 * i;
                let (id, to) = (u32_at(self.data, e)?, u32_at(self.data, e + 4)?);
                Ok((id, (to & 0x7fff_ffff) as usize, to & 0x8000_0000 != 0))
            })
            .collect()
    }

    /// The data of resource `type_id` / `id` (first language).
    fn get(&self, type_id: u32, id: u32) -> Result<&'a [u8]> {
        let find = |off: usize, want: u32| -> Result<(usize, bool)> {
            self.entries(off)?.into_iter().find(|e| e.0 == want).map(|e| (e.1, e.2)).ok_or_else(|| crate::Error::Format(format!("cursor: resource {type_id}/{want} missing")))
        };
        let (t, _) = find(0, type_id)?;
        let (n, _) = find(t, id)?;
        let lang = *self.entries(n)?.first().ok_or_else(|| crate::Error::Format("cursor: no language".into()))?;
        let (rva, size) = (u32_at(self.data, lang.1)?, u32_at(self.data, lang.1 + 4)?);
        let start = (rva - self.rva) as usize;
        self.data.get(start..start + size as usize).ok_or_else(|| crate::Error::Format("cursor: resource data".into()))
    }
}

/// An `RT_CURSOR` entry as RGBA.
fn decode(blob: &[u8]) -> Result<Cursor> {
    let hotspot = (u16_at(blob, 0)?, u16_at(blob, 2)?);
    let dib = &blob[4..];
    let header = u32_at(dib, 0)? as usize;
    let width = u32_at(dib, 4)?;
    let height = u32_at(dib, 8)? / 2;
    let bpp = u16_at(dib, 14)?;
    if bpp != 1 {
        bail!("cursor: {bpp} bits per pixel");
    }
    let palette = [&dib[header..header + 4], &dib[header + 4..header + 8]];
    let stride = (width as usize).div_ceil(32) * 4;
    let xor = header + 8;
    let and = xor + stride * height as usize;
    let bit = |base: usize, row: usize, x: usize| (dib[base + row * stride + x / 8] >> (7 - x % 8)) & 1;
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height as usize {
        // Bottom-up rows.
        let row = height as usize - 1 - y;
        for x in 0..width as usize {
            let c = palette[bit(xor, row, x) as usize];
            let a = if bit(and, row, x) == 1 { 0 } else { 255 };
            rgba.extend_from_slice(&[c[2], c[1], c[0], a]);
        }
    }
    Ok(Cursor { width, height, hotspot, rgba })
}

/// The 36 cursors of `CursorList`, from the bytes of `S4_Main.exe`.
pub fn cursors(exe: &[u8]) -> Result<Vec<Cursor>> {
    let r = Rsrc::of(exe)?;
    IDS.iter()
        .map(|&id| {
            let group = r.get(12, id as u32)?;
            // GRPCURSORDIR (6 bytes), then 14-byte entries ending with the RT_CURSOR id.
            let entry = u16_at(group, 6 + 12)?;
            decode(r.get(1, entry as u32)?)
        })
        .collect()
}

/// [`cursors`] of the installed game's `S4_Main.exe`.
pub fn load() -> Result<Vec<Cursor>> {
    cursors(&std::fs::read(crate::path::data("S4_Main.exe"))?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The arrow (index 0): 32 × 32, hotspot (9, 5) as the resource says; the hourglass (4)
    /// at (16, 15); some pixels opaque black and white, the rest transparent.
    #[test]
    fn the_installed_cursors() {
        let Ok(c) = load() else { return };
        assert_eq!(c.len(), 36);
        assert_eq!((c[0].width, c[0].height, c[0].hotspot), (32, 32, (9, 5)));
        assert_eq!(c[4].hotspot, (16, 15));
        let px = |i: usize| c[i].rgba.chunks(4).map(|p| (p[0], p[3])).collect::<Vec<_>>();
        let a = px(0);
        assert!(a.contains(&(0, 255)) && a.contains(&(255, 255)) && a.contains(&(0, 0)));
    }
}
