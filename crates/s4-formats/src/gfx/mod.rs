//! Sprite graphics in `Gfx\N.*` (everything except the landscape textures 2/41).
//!
//! Mirrors `CGfxManager::EnableGfxFile` (0x13637e0) and the loaders it calls:
//! `LoadGfxFile` 0x1365750, `LoadGilFile` 0x1365ce0, `LoadDilFile` 0x1365f30,
//! `LoadJilFile` 0x1366180, `LoadSilFile` 0x13663d0, `LoadPilFile` 0x1366520,
//! `LoadPalFile` 0x1366790; lookups mirror `GetSettlerGfxInfo` 0x1362210, `GetBuildingGfxInfo`
//! 0x1361af0, `GetJobFrameCount` 0x1363d10 and `GetImageInfo` 0x1364430.
//!
//! The files form a chain of offset tables (all 20-byte-header blobs, see [`container`]):
//! ```text
//! .jil[job]   -> byte offset into .dil   (job       -> first "direction")
//! .dil[d]     -> byte offset into .gil   (direction -> first frame)
//! .gil[i]     -> byte offset into .gfx   (image     -> image header + RLE data)
//! .sil[k]     -> byte offset into .jil   (job group -> first job; e.g. settler type)
//! .pi2[job]   -> byte offset into .p2x   (job -> palette)      [.pil[image] -> .pa5/.pa6 for GUI]
//! ```
//! A frame range ends where the next larger .dil value starts (or at the end of .gil), exactly
//! how `GetJobFrameCount` walks forward to the next distinct entry. An entry equal to entry 0
//! (offset 0, relocated to 20) means "absent" for every index > 0.
//!
//! Image layout and RLE: [`image`]. Palettes and team colour: [`palette`]. File contents: [`catalogue`].
//!
//! # Addresses
//! All addresses in this module are the `// address=[...]` comments of the IDA decompilation
//! (`S4Forge.RE\Decompilation\S4_Main\generated`). They do **not** match the installed
//! `S4_Main.exe` (PE timestamp 1563878175, ImageBase 0x400000, SizeOfImage 0x12bf000, .text at
//! 0x401000..0xf14ef0), and they are not a constant rebase of it either:
//! * `syms.txt` puts `CGfxManager::GetNrDirection` at 0x176a1e0 while its comment says 0x136a1e0
//!   (a 0x400000 shift); neither lies in this exe's .text, and the data tables
//!   `dword_3687E28` (directions per file) / `byte_3687DFC` (has-.sil per file) lie outside the image.
//! * Locating functions in this exe by the code that references their unique strings gives:
//!   `GetSettlerFirstJob` 0x1362410 → references at 0x461e39 (plus 0x567578/0x56779d);
//!   `GetJobFrameCount` 0x1363d10 → 0x46200e; `LoadFile` 0x1364ff0 → 0x462a8b/0x462dcd;
//!   `MoveWavesToGfxEngine` 0x1366c80 → 0x4637f9; `UpdateGui` 0x2f9dce0 → 0x67254e.
//!   The deltas differ (0xf01d02, 0xf03487, 0x292b792), and the decompilation's debug-assert strings
//!   (e.g. "Width or height > 512!") are absent from this exe, so the decompilation comes from a
//!   different build (an assert-enabled one). Logic was cross-checked against the data files
//!   instead; directions per file are derived from the .jil spacing, not read from the exe table.

pub mod landscape;
pub mod catalogue;
pub mod container;
pub mod image;
pub mod palette;

use std::ops::Range;
use std::path::Path;

use crate::{Result, bail};
pub use catalogue::{FileInfo, Layout};
pub use image::{HeaderKind, ImageHeader, IndexedImage, PIX_SHADOW, PIX_TRANSPARENT};
pub use palette::{PaletteFile, PixelFormat, PlayerColor};

/// Where an image sits in the job/direction/frame hierarchy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FramePos {
    pub job: u32,
    /// Direction (or, for buildings, part/animation index within the building).
    pub dir: u32,
    pub frame: u32,
}

/// A decoded RGBA8 sprite.
#[derive(Clone, Debug)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    /// Hot-spot: draw the top-left corner at `(x - left, y - top)`.
    pub left: i32,
    pub top: i32,
    /// Draw at half size (header flag 0x10).
    pub half_scale: bool,
    /// `width*height*4`, straight (non-premultiplied) alpha.
    pub rgba: Vec<u8>,
}

/// Options for [`GfxFile::decode_with`].
#[derive(Clone, Copy, Debug)]
pub struct DecodeOptions {
    /// Substitute team colour (only affects files with 512-entry palettes: settlers, vehicles).
    pub player: Option<PlayerColor>,
    /// Brightness level of the palette (0 = full, 1 = half for p2x).
    pub level: usize,
    /// How shadow pixels are emitted.
    pub shadow: ShadowMode,
    /// Take alpha from the ARGB4444 `.p24` palette (smoke, fire, magic in the effect files).
    pub use_4444_alpha: bool,
}

impl Default for DecodeOptions {
    fn default() -> Self {
        Self { player: None, level: 0, shadow: ShadowMode::Dither, use_4444_alpha: true }
    }
}

/// Shadow pixels (RLE `01 n` runs).
///
/// The software blitter `ZoomBobNormal` (0x2f72630) draws a shadow run as a checkerboard: it flips
/// a sign every pixel, seeded from the destination pixel's x parity and the row parity, and writes
/// colour 0 (black) only on every other pixel — a 50% dither in **screen** space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShadowMode {
    /// Opaque black on `(x + y) % 2 == 0` of the image, transparent otherwise. Matches the original
    /// when the image is placed at an even `x + y` screen position; a renderer that wants the exact
    /// screen-space phase should dither in its shader from [`IndexedImage`] instead.
    Dither,
    /// Every shadow pixel gets this RGBA (e.g. `[0, 0, 0, 128]` for a smooth translucent shadow).
    Color([u8; 4]),
}

/// One numbered sprite file with all its index and palette tables loaded.
pub struct GfxFile {
    pub number: u32,
    pub version: u32,
    pub header_kind: HeaderKind,
    pub layout: Layout,
    gfx: Vec<u8>,
    /// Absolute offsets into `gfx`, one per image.
    gil: Vec<u32>,
    /// Per direction: first gil index.
    dil: Vec<u32>,
    /// Per job: first dil index.
    jil: Vec<u32>,
    /// Per group: first job index (raw, not relocated).
    sil: Vec<u32>,
    /// Per job (or per image for `Layout::Flat`): byte offset into the palette file.
    pal_index: Vec<u32>,
    palette: PaletteFile,
    /// The ARGB4444 palette (`.p24`) when present, same layout as `palette`.
    palette_4444: Option<PaletteFile>,
    /// Directions per job (`Layout::Jobs`), derived from the .jil spacing.
    directions: usize,
    /// Sorted distinct .dil values, to find where a frame range ends.
    dil_bounds: Vec<u32>,
    /// Sorted distinct .jil values (building layout).
    jil_bounds: Vec<u32>,
    /// Image index -> position (first job/dir/frame referencing it).
    positions: Vec<Option<FramePos>>,
}

fn read(dir: &Path, n: u32, ext: &str) -> Result<Vec<u8>> {
    let p = crate::path::resolve(dir.join(format!("{n}.{ext}")));
    std::fs::read(&p).map_err(|e| crate::Error::Format(format!("{}: {e}", p.display())))
}

fn read_opt(dir: &Path, n: u32, ext: &str) -> Result<Option<Vec<u8>>> {
    let p = crate::path::resolve(dir.join(format!("{n}.{ext}")));
    if p.exists() { Ok(Some(read(dir, n, ext)?)) } else { Ok(None) }
}

/// Smallest gap between consecutive distinct palette offsets (= bytes per palette).
fn palette_stride(index: &[u32], file_len: usize) -> Result<usize> {
    let mut u: Vec<u32> = index.to_vec();
    u.sort_unstable();
    u.dedup();
    let mut stride = usize::MAX;
    for w in u.windows(2) {
        stride = stride.min((w[1] - w[0]) as usize);
    }
    if let Some(&last) = u.last() {
        stride = stride.min(file_len.saturating_sub(last as usize));
    }
    if stride == 0 || stride == usize::MAX {
        bail!("cannot derive palette size");
    }
    Ok(stride)
}

impl GfxFile {
    /// Open `N.gfx` and its companions from the game's `Gfx` directory.
    pub fn open(dir: impl AsRef<Path>, n: u32) -> Result<GfxFile> {
        let dir = dir.as_ref();
        let gfx = read(dir, n, "gfx")?;
        let version = container::version(&gfx, "gfx")?;
        let gil_raw = read(dir, n, "gil")?;
        let gil = container::offsets(&gil_raw, "gil")?;
        for &o in &gil {
            if o as usize >= gfx.len() {
                bail!("{n}.gil: offset {o:#x} beyond .gfx ({} bytes)", gfx.len());
            }
        }

        let (dil, jil, sil) = match (read_opt(dir, n, "dil")?, read_opt(dir, n, "jil")?) {
            (Some(d), Some(j)) => {
                let dil = container::to_indices(&container::offsets(&d, "dil")?, gil.len(), "dil")?;
                let jil = container::to_indices(&container::offsets(&j, "jil")?, dil.len(), "jil")?;
                let sil = match read_opt(dir, n, "sil")? {
                    Some(s) => {
                        let raw = container::offsets(&s, "sil")?;
                        container::to_indices(&raw, jil.len(), "sil")?
                    }
                    None => Vec::new(),
                };
                (dil, jil, sil)
            }
            _ => (Vec::new(), Vec::new(), Vec::new()),
        };

        let header_kind = if (20..=25).contains(&n) { HeaderKind::Settler } else { HeaderKind::Object };
        let layout = if jil.is_empty() {
            Layout::Flat
        } else if (10..20).contains(&n) {
            Layout::Buildings
        } else {
            Layout::Jobs
        };

        // Palettes: GUI files use .pil + .pa6 (one level), the rest .pi2 + .p26 (two levels).
        let (pal_index, palette, palette_4444) = if let Some(pil) = read_opt(dir, n, "pil")? {
            let idx = container::offsets(&pil, "pil")?;
            let data = read(dir, n, "pa6")?;
            let stride = palette_stride(&idx, data.len())?;
            (idx, PaletteFile::new(PixelFormat::Rgb565, data, stride / 2, 1), None)
        } else {
            let idx = container::offsets(&read(dir, n, "pi2")?, "pi2")?;
            let data = read(dir, n, "p26")?;
            let stride = palette_stride(&idx, data.len())?;
            if !stride.is_multiple_of(4) {
                bail!("{n}.p26: palette stride {stride} not a multiple of 4");
            }
            let entries = stride / 4;
            let p4444 = read_opt(dir, n, "p24")?.map(|d| PaletteFile::new(PixelFormat::Argb4444, d, entries, 2));
            (idx, PaletteFile::new(PixelFormat::Rgb565, data, entries, 2), p4444)
        };

        let mut dil_bounds = dil.clone();
        dil_bounds.sort_unstable();
        dil_bounds.dedup();
        let mut jil_bounds = jil.clone();
        jil_bounds.sort_unstable();
        jil_bounds.dedup();
        let directions = jil_bounds.windows(2).map(|w| (w[1] - w[0]) as usize).min().unwrap_or(1).max(1);

        let mut f = GfxFile {
            number: n,
            version,
            header_kind,
            layout,
            gfx,
            gil,
            dil,
            jil,
            sil,
            pal_index,
            palette,
            palette_4444,
            directions,
            dil_bounds,
            jil_bounds,
            positions: Vec::new(),
        };
        f.positions = f.build_positions();
        Ok(f)
    }

    /// Open from the default install (`crate::game_dir()/Gfx`).
    pub fn open_default(n: u32) -> Result<GfxFile> {
        Self::open(crate::game_dir().join("Gfx"), n)
    }

    fn build_positions(&self) -> Vec<Option<FramePos>> {
        let mut pos = vec![None; self.gil.len()];
        for job in 0..self.job_count() {
            let Some(dirs) = self.job_dirs(job) else { continue };
            for (di, d) in dirs.enumerate() {
                let Some(frames) = self.dil_frames(d) else { continue };
                for (fi, g) in frames.enumerate() {
                    if pos[g].is_none() {
                        pos[g] = Some(FramePos { job: job as u32, dir: di as u32, frame: fi as u32 });
                    }
                }
            }
        }
        pos
    }

    pub fn image_count(&self) -> usize {
        self.gil.len()
    }

    /// Number of jobs (.jil entries); 0 for flat files.
    pub fn job_count(&self) -> usize {
        self.jil.len()
    }

    /// Directions per job for `Layout::Jobs` files (6 for settlers/animals/vehicles).
    pub fn direction_count(&self) -> usize {
        self.directions
    }

    /// The .sil table: group -> first job (e.g. settler type -> its first job).
    pub fn job_groups(&self) -> &[u32] {
        &self.sil
    }

    /// Range of .dil indices belonging to `job` (`None` = job absent).
    pub fn job_dirs(&self, job: usize) -> Option<Range<usize>> {
        let &s = self.jil.get(job)?;
        if job != 0 && s == self.jil[0] {
            return None;
        }
        let s = s as usize;
        let e = match self.layout {
            Layout::Flat => return None,
            Layout::Jobs => s + self.directions,
            Layout::Buildings => {
                self.jil_bounds.iter().find(|&&b| b as usize > s).map_or(self.dil.len(), |&b| b as usize)
            }
        };
        Some(s..e.min(self.dil.len()))
    }

    /// Range of gil (image) indices for one .dil entry (`None` = absent).
    pub fn dil_frames(&self, d: usize) -> Option<Range<usize>> {
        let &s = self.dil.get(d)?;
        if d != 0 && s == self.dil[0] {
            return None;
        }
        let e = self.dil_bounds.iter().find(|&&b| b > s).map_or(self.gil.len(), |&b| b as usize);
        Some(s as usize..e)
    }

    /// Image indices of `(job, dir)`: the frames of one animation.
    pub fn job_frames(&self, job: usize, dir: usize) -> Option<Range<usize>> {
        let dirs = self.job_dirs(job)?;
        let d = dirs.start + dir;
        if d >= dirs.end {
            return None;
        }
        self.dil_frames(d)
    }

    /// Where image `index` sits (first job/dir/frame that references it).
    pub fn position(&self, index: usize) -> Option<FramePos> {
        self.positions.get(index).copied().flatten()
    }

    /// Byte offset of the palette used for image `index` (in [`Self::palette_file`]), as the
    /// engine resolves it: `.pil[image]` for GUI/flat files, `.pi2[job]` otherwise.
    pub fn palette_offset(&self, index: usize) -> Option<u32> {
        match self.layout {
            Layout::Flat => self.pal_index.get(index).copied(),
            _ => self.pal_index.get(self.position(index)?.job as usize).copied(),
        }
    }

    pub fn palette_file(&self) -> &PaletteFile {
        &self.palette
    }

    pub fn palette_4444_file(&self) -> Option<&PaletteFile> {
        self.palette_4444.as_ref()
    }

    /// GUI file (`.pil` + `.pa5/.pa6` palettes, one palette per image).
    pub fn is_gui(&self) -> bool {
        self.palette.levels == 1
    }

    /// True when palettes carry per-player ramps (512 entries; settlers and vehicles).
    pub fn has_team_colour(&self) -> bool {
        self.palette.entries >= 512
    }

    pub fn header(&self, index: usize) -> Result<ImageHeader> {
        let Some(&off) = self.gil.get(index) else {
            bail!("gfx {}: image {index} out of range ({})", self.number, self.gil.len());
        };
        ImageHeader::read(&self.gfx, off as usize, self.header_kind)
    }

    /// Decode image `index` to palette indices (0 = transparent, 1 = shadow).
    pub fn decode_indexed(&self, index: usize) -> Result<IndexedImage> {
        image::decode_rle(&self.gfx, self.header(index)?)
    }

    /// The 256-colour working palette for an image as RGBA, including team-colour substitution.
    pub fn working_palette(&self, index: usize, opt: &DecodeOptions) -> Result<[[u8; 4]; 256]> {
        let off = self.palette_offset(index).unwrap_or(self.pal_index.first().copied().unwrap_or(20));
        let cols = self.palette.level(off, opt.level.min(self.palette.levels - 1))?;
        let alpha = match (&self.palette_4444, opt.use_4444_alpha) {
            (Some(p), true) => Some(p.level(off, opt.level.min(p.levels - 1))?),
            _ => None,
        };
        let mut out = [[0u8; 4]; 256];
        for (i, o) in out.iter_mut().enumerate() {
            *o = self.palette.format.to_rgba(cols[i]);
            if let Some(a) = &alpha {
                o[3] = PixelFormat::Argb4444.to_rgba(a[i])[3];
            }
        }
        if let (Some(p), true) = (opt.player, self.has_team_colour()) {
            for k in 0..32 {
                let [r, g, b] = p.gradient(k);
                out[192 + k] = [r, g, b, 255];
                let src = 256 + 32 * (p.slot as usize & 7) + k;
                if let Some(&c) = cols.get(src) {
                    out[224 + k] = self.palette.format.to_rgba(c);
                }
            }
        }
        Ok(out)
    }

    /// Decode image `index` to RGBA with default options (no team colour substitution).
    pub fn decode(&self, index: usize) -> Result<Image> {
        self.decode_with(index, &DecodeOptions::default())
    }

    pub fn decode_with(&self, index: usize, opt: &DecodeOptions) -> Result<Image> {
        let ix = self.decode_indexed(index)?;
        let pal = self.working_palette(index, opt)?;
        let mut rgba = Vec::with_capacity(ix.pixels.len() * 4);
        let w = ix.header.width.max(1) as usize;
        for (i, &p) in ix.pixels.iter().enumerate() {
            if ix.raw {
                // Raw GUI backgrounds: index 0 is the colour key. `IsNonTransparentGuiArea`
                // (0x2f9ba00) hit-tests "raw byte != 0", and `DrawControl` (0x2f9c0c0) swaps palette
                // entries 0 and 1 for colour 31 under dialog flag bit 0. Index 1 stays opaque.
                let c = pal[p as usize];
                let a = if p == 0 { 0 } else { 255 };
                rgba.extend_from_slice(&[c[0], c[1], c[2], a]);
                continue;
            }
            match p {
                PIX_TRANSPARENT => rgba.extend_from_slice(&[0, 0, 0, 0]),
                // GUI files are unpacked with `UnpackGfxTransparent` (0x2fa4990), which writes
                // colour 0 (opaque black) for every shadow pixel.
                PIX_SHADOW if self.is_gui() => rgba.extend_from_slice(&[0, 0, 0, 255]),
                PIX_SHADOW => match opt.shadow {
                    ShadowMode::Dither => {
                        let on = (i % w + i / w).is_multiple_of(2);
                        rgba.extend_from_slice(if on { &[0, 0, 0, 255] } else { &[0, 0, 0, 0] });
                    }
                    ShadowMode::Color(c) => rgba.extend_from_slice(&c),
                },
                _ => rgba.extend_from_slice(&pal[p as usize]),
            }
        }
        let h = ix.header;
        Ok(Image {
            width: h.width as u32,
            height: h.height as u32,
            left: h.left as i32,
            top: h.top as i32,
            half_scale: h.half_scale(),
            rgba,
        })
    }

    /// Catalogue entry for this file, if it is a known one.
    pub fn info(&self) -> Option<&'static FileInfo> {
        catalogue::info(self.number)
    }
}

/// Numbers of all sprite files (`N.gfx` + `N.gil`) present in a Gfx directory, sorted.
pub fn list_files(dir: impl AsRef<Path>) -> Result<Vec<u32>> {
    let mut v = Vec::new();
    for e in std::fs::read_dir(crate::path::resolve(dir))? {
        let p = e?.path();
        if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("gfx"))
            && crate::path::resolve(p.with_extension("gil")).exists()
            && let Some(n) = p.file_stem().and_then(|s| s.to_str()).and_then(|s| s.parse().ok())
        {
            v.push(n);
        }
    }
    v.sort_unstable();
    Ok(v)
}
