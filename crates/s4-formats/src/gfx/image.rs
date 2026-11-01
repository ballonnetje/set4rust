//! One sprite inside a `.gfx` file: header + run-length-encoded 8-bit pixels.
//!
//! Two header layouts exist:
//!
//! * **Settler header** (files 20..=25, `CGfxManager::Debug_Check_LayerBlock` 0x1367b70 reads
//!   bytes for exactly these; `BlitSettler` 0x2f69ee0 / `BlitSettlerHardware` 0x2f92ac0):
//!   `u8 width, u8 height, u8 left, u8 top, u32 cache_id` — 8 bytes. `cache_id` is 1000000+n,
//!   the hardware path adds 1000000*player to key its texture cache.
//! * **Object header** (every other file; `BlitObject` 0x2f6a540 / `BlitObjectHardware`
//!   0x2f951e0 / `CRTComp::Compile` 0x2fcd6a0 which *writes* it):
//!   `u16 width, u16 height, i16 left, i16 top, u16 flags, u16 cache_id` — 12 bytes.
//!   `flags & 0x10` makes the blitter draw the image at half scale (both blitters check
//!   it). `flags & 0x20` = the pixels are *not* run-length encoded but stored raw, one palette
//!   index per pixel, `width*height` bytes, opaque (GUI dialog backgrounds: `UpdateGui` 0x2f9dce0
//!   refuses a background without 0x20 — "Dialog is packed. Illegal data!" — and blits it with
//!   `FastBlit8Bit`; `DrawControl` 0x2f9c0c0 only RLE-unpacks controls without 0x20). Observed flag values over the whole
//!   install: raw images carry exactly 0x20 (GUI files only); RLE images carry 0x0000/0x0001
//!   (GUI, objects, accessories, logos) or 0xHH01 with HH = 0x06/0x07/0x08/0x55 per file (meaning
//!   of the high byte unknown, no reader found); 0x0011 (half scale) occurs only in file 5 (267 images).
//!
//! `left`/`top` are the hot-spot: the image's top-left corner is drawn at
//! `(x - left, y - top)` (`BlitObject`: `a2 - (left << 16) * zoom`).
//!
//! Pixel stream (`CRTComp::EncodeImage` 0x2fd08f0 = encoder; `ZoomBobNormal` 0x2f72630,
//! `UnpackGfxTransparent` 0x2fa4990 = decoders). The stream covers the image row-major as one
//! linear run of `width*height` pixels (runs may cross row ends):
//! * `0x00 n`  — `n` transparent pixels
//! * `0x01 n`  — `n` shadow pixels (software blitter: 50% checkerboard of black;
//!   `UnpackGfxTransparent` writes colour 0). `0x01 0x00` terminates the image; any pixels not
//!   yet written stay transparent.
//! * `b >= 2`  — one pixel of palette index `b`.

use crate::reader::Reader;
use crate::{Result, bail};

/// Pixel value meaning "transparent" in [`IndexedImage::pixels`].
pub const PIX_TRANSPARENT: u8 = 0;
/// Pixel value meaning "shadow" in [`IndexedImage::pixels`].
pub const PIX_SHADOW: u8 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderKind {
    /// 8-byte header with byte-sized fields (settler files 20..=25).
    Settler,
    /// 12-byte header with 16-bit fields (all other files).
    Object,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageHeader {
    pub width: u16,
    pub height: u16,
    /// Hot-spot x: the image is drawn at `x - left`.
    pub left: i16,
    /// Hot-spot y: the image is drawn at `y - top`.
    pub top: i16,
    /// Object header only (0 for settler images). Bit 0x10 = draw at half scale.
    pub flags: u16,
    /// Texture-cache id stored in the file (u32 for settler images, u16 otherwise).
    pub cache_id: u32,
    /// Offset of the RLE stream within the .gfx file.
    pub data_offset: usize,
}

impl ImageHeader {
    pub fn read(gfx: &[u8], offset: usize, kind: HeaderKind) -> Result<Self> {
        let mut r = Reader::at(gfx, offset, "gfx image header");
        let h = match kind {
            HeaderKind::Settler => ImageHeader {
                width: r.u8()? as u16,
                height: r.u8()? as u16,
                left: r.u8()? as i16,
                top: r.u8()? as i16,
                flags: 0,
                cache_id: r.u32()?,
                data_offset: offset + 8,
            },
            HeaderKind::Object => ImageHeader {
                width: r.u16()?,
                height: r.u16()?,
                left: r.i16()?,
                top: r.i16()?,
                flags: r.u16()?,
                cache_id: r.u16()? as u32,
                data_offset: offset + 12,
            },
        };
        Ok(h)
    }

    /// The renderer should draw this image at half its pixel size.
    pub fn half_scale(&self) -> bool {
        self.flags & 0x10 != 0
    }

    /// Unpacked 8-bit image (flag 0x20), see the module docs.
    pub fn is_raw(&self) -> bool {
        self.flags & 0x20 != 0
    }
}

/// A decoded sprite in palette space.
#[derive(Clone, Debug)]
pub struct IndexedImage {
    pub header: ImageHeader,
    /// `width*height` row-major. 0 = transparent, 1 = shadow, 2..=255 = palette index.
    pub pixels: Vec<u8>,
    /// Bytes consumed from the stream including the terminator.
    pub stream_len: usize,
    /// The stream ended with an explicit `01 00` terminator (false: it ran out at `width*height`).
    pub terminated: bool,
    /// Raw (flag 0x20) image: every byte of `pixels` is a palette index, 0 and 1 included,
    /// and nothing is transparent.
    pub raw: bool,
}

/// Decode the pixel data of one image (RLE, or raw when `header.is_raw()`).
///
/// Fails if the stream runs off the end of the file, or a run overflows the image by more
/// than the encoder's slack (the original blitters clip, so a short overflow at the very end
/// is tolerated and clipped).
pub fn decode_rle(gfx: &[u8], header: ImageHeader) -> Result<IndexedImage> {
    let total = header.width as usize * header.height as usize;
    if header.is_raw() {
        let mut r = Reader::at(gfx, header.data_offset, "gfx raw image");
        let pixels = r.bytes(total)?.to_vec();
        return Ok(IndexedImage { header, pixels, stream_len: total, terminated: false, raw: true });
    }
    let mut px = vec![PIX_TRANSPARENT; total];
    let mut r = Reader::at(gfx, header.data_offset, "gfx rle stream");
    let mut pos = 0usize;
    let mut terminated = false;
    while pos < total {
        let b = r.u8()?;
        match b {
            0 => {
                let n = r.u8()? as usize;
                pos += n;
            }
            1 => {
                let n = r.u8()? as usize;
                if n == 0 {
                    terminated = true;
                    break;
                }
                let end = (pos + n).min(total);
                px[pos..end].fill(PIX_SHADOW);
                pos += n;
            }
            _ => {
                px[pos] = b;
                pos += 1;
            }
        }
    }
    if pos > total + 255 {
        bail!("gfx rle stream overflows image by {} pixels", pos - total);
    }
    // The encoder always appends `01 00`; consume it if the pixels ran out first.
    if !terminated && r.remaining() >= 2 {
        let mut peek = r.clone();
        if peek.u8()? == 1 && peek.u8()? == 0 {
            r = peek;
            terminated = true;
        }
    }
    Ok(IndexedImage {
        header,
        pixels: px,
        stream_len: r.pos() - header.data_offset,
        terminated,
        raw: false,
    })
}
