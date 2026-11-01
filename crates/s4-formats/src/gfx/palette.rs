//! Sprite palettes: `.p24/.p25/.p26`, `.p44/.p45/.p46`, `.pa5/.pa6`.
//!
//! File naming (writer: `SM_SIV_PaletteBlock::Save` 0x2fd30f0, called from `CRTComp::Compile`;
//! reader: `CGfxManager::LoadPalFile` 0x1366790):
//! * last digit = pixel format of every 16-bit entry:
//!   `4` = ARGB4444 (`Convert4444` 0x2fd33c0), `5` = RGB555 (`Convert555` 0x2fd34a0),
//!   `6` = RGB565 (`Convert565` 0x2fd3570). The engine picks `.p24` when
//!   `IGfxEngine::Use4444Palettes`, otherwise `.p26` in 565 display modes and `.p25` in 555.
//! * middle digit = number of brightness levels per palette
//!   (`SM_SIV_PaletteBlock::FillPalette` 0x2fd2a70): `p2x` = 2 levels (x1.0, x0.5),
//!   `p4x` = 4 levels (x1.0, x0.85, x0.70, x0.45). The game only loads the `p2x` set (indexed by
//!   `.pi2`); `p4x` (indexed by `.pi4`) is only written by `CRTComp` and checksummed by the lobby.
//!   Level 0 of a p4x palette is byte-identical to level 0 of the p2x palette (checked on every file).
//! * `.pa5/.pa6` are the GUI palettes (555/565), one level, indexed by `.pil`
//!   (`LoadPalFile`/`LoadPilFile` with `a4 = true`, used for `EnableGfxFile` mode 9).
//!
//! A palette is `levels` consecutive blocks of `entries` u16 colours. `entries` is 256, or 512
//! for the settler and vehicle files (20..=24, 30..=34): there, entries 256.. hold eight
//! 32-colour player-specific ramps (see [`PlayerColor`]).
//! Index 0 is the colour key (0x7c1f / 0xf81f magenta) and index 1 is the shadow slot; neither
//! is ever drawn through the palette (the RLE stream encodes them as runs).

use crate::{Result, bail};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelFormat {
    Argb4444,
    Rgb555,
    Rgb565,
}

impl PixelFormat {
    /// Expand one 16-bit entry to RGBA8 (bit replication).
    pub fn to_rgba(self, v: u16) -> [u8; 4] {
        let x5 = |c: u16| ((c << 3) | (c >> 2)) as u8;
        let x6 = |c: u16| ((c << 2) | (c >> 4)) as u8;
        let x4 = |c: u16| (c * 17) as u8;
        match self {
            PixelFormat::Rgb565 => [x5(v >> 11), x6((v >> 5) & 63), x5(v & 31), 255],
            PixelFormat::Rgb555 => [x5((v >> 10) & 31), x5((v >> 5) & 31), x5(v & 31), 255],
            PixelFormat::Argb4444 => [x4((v >> 8) & 15), x4((v >> 4) & 15), x4(v & 15), x4(v >> 12)],
        }
    }
}

/// A whole palette file plus the layout needed to address one palette inside it.
#[derive(Clone, Debug)]
pub struct PaletteFile {
    pub format: PixelFormat,
    pub data: Vec<u8>,
    /// Colours per level (256 or 512).
    pub entries: usize,
    /// Brightness levels per palette (1 for GUI, 2 for p2x, 4 for p4x).
    pub levels: usize,
}

impl PaletteFile {
    pub fn new(format: PixelFormat, data: Vec<u8>, entries: usize, levels: usize) -> Self {
        Self { format, data, entries, levels }
    }

    /// Bytes per palette.
    pub fn stride(&self) -> usize {
        self.entries * self.levels * 2
    }

    /// The raw u16 colours of one level of the palette that starts at byte `offset`
    /// (the value stored in the .pil/.pi2 table).
    pub fn level(&self, offset: u32, level: usize) -> Result<Vec<u16>> {
        if level >= self.levels {
            bail!("palette level {level} out of range ({} levels)", self.levels);
        }
        let start = offset as usize + level * self.entries * 2;
        let end = start + self.entries * 2;
        let Some(bytes) = self.data.get(start..end) else {
            bail!("palette offset {offset:#x} (level {level}) beyond file ({} bytes)", self.data.len());
        };
        Ok(bytes.as_chunks::<2>().0.iter().map(|&c| u16::from_le_bytes(c)).collect())
    }
}

/// A player's colour, as passed to `IGfxEngine::SetPlayerColor` (0x2f5a8a0).
///
/// Team colour in settler/vehicle palettes (`BlitSettler` 0x2f69ee0, `BlitVehicle`): before
/// blitting, the engine builds a 256-entry working palette:
/// * entries `0..192`   = the image's palette entries 0..192,
/// * entries `192..224` = the player's 32-step gradient from `CColorGradient::SetupGradients`
///   (0x2f6ff70): entry `192+k` = player colour minus `8*(31-k)` per channel, clamped at 0
///   (so 223 is the pure player colour, 192 the darkest),
/// * entries `224..256` = palette entries `256 + 32*player ..` (per-player hand-made ramps).
///
/// Buildings, objects and the GUI do not get this substitution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlayerColor {
    /// Player slot 0..8 (selects the 32-entry ramp in palette entries 256..512).
    pub slot: u8,
    pub rgb: [u8; 3],
}

impl PlayerColor {
    /// Gradient entry `k` (0..32) as RGB8, as `CColorGradient::SetupGradients` computes it.
    pub fn gradient(&self, k: usize) -> [u8; 3] {
        let d = 8 * (31 - k.min(31)) as i32;
        self.rgb.map(|c| (c as i32 - d).max(0) as u8)
    }
}
