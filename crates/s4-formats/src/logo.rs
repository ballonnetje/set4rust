//! Player logos (heraldic figures): `Logo\*.PNG` and the 22 × 22 images the game composes from
//! them into sprite file 1 (`Gfx\1.*`, two images per player), which the military buildings' flags
//! and the lobby draw (`CGfxManager` vt+4 0x461cd0: player `p` → file 1 image `2 (p − 1)`).
//!
//! The release:
//! * **List** (`CStateLobbyGameSettings` 0x51c7e0): `Logo\*.PNG` in `FindFirstFile` order, the
//!   files whose image info is 20 × 20 at 24 bits per pixel, at most 1000. Only the first eight
//!   can be chosen: a player's logo index (GameState +0x168, default the slot index, reset
//!   0x506320) wraps in 0..8 (`UserDataChange` 0x5a5270 case 4).
//! * **Current** (`Logo\Current\HerFig_<slot>.png`): the lobby copies list entries there and
//!   composes all eight into file 1 (0x51e710 → `CRTComp` 0x6943d0), replacing `Gfx\1.*`; the
//!   shipped `Gfx\1.*` equals `Gfx\35.*` (the defaults, copied back at every lobby open 0x51f080).
//! * **Compose** (`CRTComp` 0x6943d0, per `HerFig_%d.png`): load (0x6955a0: 20 × 20 RGB or
//!   paletted), the bounding box of the pixels that are not exactly (255, 0, 255) (0x695ad0), the
//!   colour key and 5-bit channels (0x695be0), an octree palette of at most 253 colours (0x696300,
//!   0x696760) with the key colour (0xf8, 0, 0xf8) moved to index 0 = transparent (0x695d00), the
//!   box centred when it is not the full 20 × 20 (0x695f70), a one-pixel transparent border →
//!   22 × 22 (0x696050), hotspot (1 − w/2, 1 − h/2) = (−10, −10).
//!
//! Here the list is read in file-name order (case-insensitive, [inf] NTFS's `FindFirstFile`
//! order) and the octree reduction is not ported: a 20 × 20 logo has at most 400 colours, and
//! the shipped ones have fewer than 253, where the reduction does nothing.

use std::path::{Path, PathBuf};

use crate::{Result, bail};

/// Logo files are this wide and high.
pub const SIZE: usize = 20;
/// The composed image: the logo with a one-pixel border.
pub const FRAME: usize = SIZE + 2;
/// The composed image's offset from the point it is drawn at (`1 − FRAME / 2`).
pub const HOTSPOT: i32 = 1 - FRAME as i32 / 2;
/// Logo indices a player can choose (0x5a5270 wraps at 8).
pub const CHOICES: u8 = 8;
/// The release keeps at most this many list entries.
const MAX_LIST: usize = 1000;

/// The list of usable logo files in `<game>\Logo` (0x51c7e0).
pub fn list(game_dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(crate::path::resolve(game_dir.join("Logo"))) else { return Vec::new() };
    let mut files: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("png")))
        .collect();
    files.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
    files.retain(|p| read_rgb(p).is_ok());
    files.truncate(MAX_LIST);
    files
}

/// `Logo\Current\HerFig_<slot>.png`.
pub fn current_path(game_dir: &Path, slot: usize) -> PathBuf {
    game_dir.join("Logo").join("Current").join(format!("HerFig_{slot}.png"))
}

/// A logo file's 20 × 20 RGB pixels (row-major). The list takes 24-bit images only; [inf] an
/// 8-bit RGB PNG is what the image info reports as 24 bits.
pub fn read_rgb(path: &Path) -> Result<Vec<u8>> {
    let mut r = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(crate::path::resolve(path))?))
        .read_info()
        .map_err(|e| crate::Error::Format(format!("{}: {e}", path.display())))?;
    let info = r.info();
    if info.width as usize != SIZE || info.height as usize != SIZE || info.color_type != png::ColorType::Rgb || info.bit_depth != png::BitDepth::Eight {
        bail!("{}: not a 20 x 20 24-bit logo", path.display());
    }
    let mut buf = vec![0; r.output_buffer_size()];
    r.next_frame(&mut buf).map_err(|e| crate::Error::Format(format!("{}: {e}", path.display())))?;
    buf.truncate(SIZE * SIZE * 3);
    Ok(buf)
}

/// One composed pixel: `None` transparent, else the 5-bit channels (0..=31).
pub type Pixel = Option<[u8; 3]>;

/// Compose a logo (20 × 20 RGB) into the 22 × 22 image of file 1 (`CRTComp` 0x6943d0).
pub fn compose(rgb: &[u8]) -> Vec<Pixel> {
    // 0x695ad0: the box of the pixels that are not exactly the key (255, 0, 255).
    let px = |x: usize, y: usize| &rgb[(y * SIZE + x) * 3..][..3];
    let (mut x0, mut y0, mut x1, mut y1) = (1000, 1000, 0, 0);
    for y in 0..SIZE {
        for x in 0..SIZE {
            if px(x, y) != [255, 0, 255] {
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
            }
        }
    }
    // 0x695be0: (> 200, < 110, > 200) is the key; every channel to 5 bits, `(int)(c * (float)
    // (31 / 255)) << 3` (the constant at 0x1070da0).
    let scale = |c: u8| (c as f32 * (31.0f32 / 255.0)) as i32 as u8;
    let pixel = |x: usize, y: usize| -> Pixel {
        let [r, g, b] = [px(x, y)[0], px(x, y)[1], px(x, y)[2]];
        if r > 200 && g < 0x6e && b > 200 { None } else { Some([scale(r), scale(g), scale(b)]) }
    };
    let mut out = vec![None; FRAME * FRAME];
    if x1 < x0 {
        return out;
    }
    // 0x695f70: a box that is not the full 20 × 20 moves to (10 − w/2, 10 − h/2).
    let (w, h) = (x1 - x0 + 1, y1 - y0 + 1);
    let (dx, dy) = if w == SIZE && h == SIZE { (0, 0) } else { (SIZE / 2 - w / 2, SIZE / 2 - h / 2) };
    for y in 0..h {
        for x in 0..w {
            // 0x696050: one pixel of border.
            out[(dy + y + 1) * FRAME + dx + x + 1] = pixel(x0 + x, y0 + y);
        }
    }
    out
}

/// [`compose`] as RGBA (transparent pixels all zero), as the 16-bit 565 palette shows it: red and
/// blue widened from 5 bits, green (`g << 1`) from 6, by bit replication.
pub fn compose_rgba(rgb: &[u8]) -> Vec<u8> {
    compose(rgb).iter().flat_map(|p| p.map_or([0; 4], widen)).collect()
}

fn widen([r, g, b]: [u8; 3]) -> [u8; 4] {
    let e5 = |v: u8| (v << 3) | (v >> 2);
    let g6 = g << 1;
    [e5(r), (g6 << 2) | (g6 >> 4), e5(b), 255]
}

/// [`compose`] as a sprite: pixels (0 transparent, 2.. palette indices as in a gfx image) and the
/// 256-entry RGBA palette. Colours past the 254 a palette holds take the nearest earlier one
/// ([inf] the release's octree merges them before; no shipped logo has that many).
pub fn indexed(rgb: &[u8]) -> (Vec<u8>, Vec<[u8; 4]>) {
    let mut palette = vec![[0u8; 4]; 256];
    let mut colours: Vec<[u8; 3]> = Vec::new();
    let pixels = compose(rgb)
        .iter()
        .map(|p| {
            let Some(c) = *p else { return 0 };
            let i = match colours.iter().position(|&k| k == c) {
                Some(i) => i,
                None if colours.len() < 254 => {
                    colours.push(c);
                    palette[colours.len() + 1] = widen(c);
                    colours.len() - 1
                }
                None => {
                    let d = |k: &[u8; 3]| (0..3).map(|j| (k[j] as i32 - c[j] as i32).pow(2)).sum::<i32>();
                    (0..colours.len()).min_by_key(|&i| d(&colours[i])).unwrap_or(0)
                }
            };
            i as u8 + 2
        })
        .collect();
    (pixels, palette)
}

/// The installed `Logo\Current\HerFig_0..8.png` (what sprite file 1 was last composed from).
pub fn installed_current(game_dir: &Path) -> [Option<Vec<u8>>; 8] {
    std::array::from_fn(|slot| read_rgb(&current_path(game_dir, slot)).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_logos_are_centred_and_keyed() {
        // A 2 × 3 block at (0, 0) on the key; one near-key pixel (key by 0x695be0) inside it.
        let mut rgb = [255u8, 0, 255].repeat(SIZE * SIZE);
        for (x, y) in [(0, 0), (1, 0), (0, 1), (1, 1), (0, 2), (1, 2)] {
            rgb[(y * SIZE + x) * 3..][..3].copy_from_slice(&[255, 255, 255]);
        }
        rgb[(SIZE + 1) * 3..][..3].copy_from_slice(&[250, 3, 246]);
        let img = compose(&rgb);
        // Moved to (10 − 1, 10 − 1), plus the border.
        let at = |x: usize, y: usize| img[(y + 1) * FRAME + x + 1];
        assert_eq!(at(9, 9), Some([31, 31, 31]));
        assert_eq!(at(10, 11), Some([31, 31, 31]));
        assert_eq!(at(10, 10), None);
        assert_eq!(at(0, 0), None);
        assert_eq!(img.iter().filter(|p| p.is_some()).count(), 5);
    }

    /// The shipped `Gfx\1.*` is the composition of the shipped `Logo\Current` (two images per
    /// player), pixel for pixel: the key, the centring (HerFig_0 and _3 are smaller than 20 × 20)
    /// and the colours.
    #[test]
    fn shipped_file_1_is_the_composed_current_logos() {
        let dir = crate::game_dir();
        let Ok(f) = crate::gfx::GfxFile::open_default(1) else { return };
        assert_eq!(f.image_count(), 16);
        for i in 0..16 {
            let h = f.header(i).unwrap();
            assert_eq!((h.width as usize, h.height as usize, h.left as i32, h.top as i32), (FRAME, FRAME, HOTSPOT, HOTSPOT));
            let mine = compose_rgba(&read_rgb(&current_path(&dir, i / 2)).unwrap());
            assert_eq!(f.decode(i).unwrap().rgba, mine, "image {i}");
            let (px, pal) = indexed(&read_rgb(&current_path(&dir, i / 2)).unwrap());
            let via: Vec<u8> = px.iter().flat_map(|&k| if k == 0 { [0; 4] } else { pal[k as usize] }).collect();
            assert_eq!(via, mine);
        }
        assert_eq!(list(&dir).len(), 9);
    }
}
