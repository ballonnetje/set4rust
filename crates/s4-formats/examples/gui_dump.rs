//! Parse every `Menu\GUISet*.DAT`, cross-check against the GUI gfx files (background and
//! control image sizes must equal the rectangles, as `DrawControl` demands), and render each
//! dialog (background + control images) to `out/gui/<set>_<slot>.png`.
//! Usage: `cargo run -p s4-formats --example gui_dump [nopng]`

use s4_formats::gfx::GfxFile;
use s4_formats::gui::{GuiSet, gfx_file_for};
use std::collections::BTreeMap;

fn main() -> anyhow::Result<()> {
    let game = s4_formats::game_dir();
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/gui");
    std::fs::create_dir_all(&out)?;
    let png = !std::env::args().any(|a| a == "nopng");

    let mut names: Vec<String> = std::fs::read_dir(game.join("Menu"))?
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.to_ascii_lowercase().starts_with("guiset"))
        .collect();
    names.sort();
    for name in names {
        let data = std::fs::read(game.join("Menu").join(&name))?;
        let set = GuiSet::parse(&data)?;
        let gn = gfx_file_for(&name).ok_or_else(|| anyhow::anyhow!("no gfx file for {name}"))?;
        let gfx = GfxFile::open(game.join("Gfx"), gn)?;

        // Dialogs must tile the file exactly.
        let mut spans: Vec<(usize, usize)> = Vec::new();
        let raw_offs: Vec<usize> = (0..set.dialogs.len())
            .map(|i| u32::from_le_bytes(data[16 + 4 * i..20 + 4 * i].try_into().unwrap()) as usize)
            .collect();
        for d in set.dialogs.iter().flatten() {
            spans.push((raw_offs[d.slot], d.byte_len()));
        }
        spans.sort();
        let tiles = spans.windows(2).all(|w| w[0].0 + w[0].1 == w[1].0)
            && spans.last().is_some_and(|l| l.0 + l.1 == data.len());

        let mut st: BTreeMap<&str, usize> = BTreeMap::new();
        let mut kinds: BTreeMap<u8, usize> = BTreeMap::new();
        let mut ctrl = 0;
        for d in set.dialogs.iter().flatten() {
            if d.flags & 2 != 0 {
                *st.entry("bg none (flag 2)").or_default() += 1;
            } else {
                let h = gfx.header(d.background as usize)?;
                let ok = (h.width, h.height) == (d.width, d.height);
                *st.entry(match (ok, h.is_raw()) {
                    (true, true) => "bg ok raw",
                    (true, false) => "bg ok RLE",
                    _ => "bg SIZE MISMATCH",
                })
                .or_default() += 1;
            }
            for c in &d.controls {
                ctrl += 1;
                *kinds.entry(c.kind).or_default() += 1;
                if c.dialog as usize != d.slot {
                    *st.entry("ctrl dialog backref WRONG").or_default() += 1;
                }
                for (img, what_ok, what_bad) in [
                    (
                        c.has_image().then_some(c.image),
                        "img ok",
                        "img SIZE MISMATCH",
                    ),
                    (
                        (c.has_image() && c.pressed_image != 0 && c.pressed_image != 0xFFFF)
                            .then_some(c.pressed_image),
                        "pressed ok",
                        "pressed SIZE MISMATCH",
                    ),
                ] {
                    let Some(img) = img else { continue };
                    match gfx.header(img as usize) {
                        // image 0 = placeholder the game fills at runtime (map preview, logos)
                        Ok(_) if img == 0 => *st.entry("img 0 (runtime)").or_default() += 1,
                        Ok(h) if (h.width, h.height) == (c.width, c.height) => {
                            *st.entry(what_ok).or_default() += 1
                        }
                        Ok(_) => *st.entry(what_bad).or_default() += 1,
                        Err(_) => *st.entry("img index out of range").or_default() += 1,
                    }
                }
            }
        }
        println!(
            "{name:28} gfx {gn:2} ({} images) v{} slots {} dialogs {} controls {ctrl} tiles_file {tiles}",
            gfx.image_count(),
            set.version,
            set.dialogs.len(),
            set.dialogs.iter().flatten().count()
        );
        println!("    {st:?}");
        println!("    types {kinds:?}");

        if png {
            for d in set.dialogs.iter().flatten() {
                render(
                    &gfx,
                    d,
                    &out.join(format!(
                        "{}_{:02}.png",
                        name.trim_end_matches(".DAT").trim_end_matches(".dat"),
                        d.slot
                    )),
                )?;
            }
        }
    }
    Ok(())
}

/// Background (or grey if none) with every image control's main image blitted on top; controls
/// without an image get a 1-px green outline so text/edit areas are visible.
fn render(
    gfx: &GfxFile,
    d: &s4_formats::gui::Dialog,
    path: &std::path::Path,
) -> anyhow::Result<()> {
    let (w, h) = (d.width as usize, d.height as usize);
    let mut px = vec![64u8; w * h * 4];
    if d.flags & 2 == 0
        && let Ok(bg) = gfx.decode(d.background as usize)
        && (bg.width as usize, bg.height as usize) == (w, h)
    {
        px.copy_from_slice(&bg.rgba);
        // Raw GUI backgrounds: palette index 0 is the colour key = transparent / click-through
        // (`IsNonTransparentGuiArea` 0x2f9ba00 tests the raw background byte != 0). The minimap
        // hole in the in-game panel is such an area.
        let ix = gfx.decode_indexed(d.background as usize)?;
        if ix.raw {
            for (i, &p) in ix.pixels.iter().enumerate() {
                if p == 0 {
                    px[i * 4..i * 4 + 4].copy_from_slice(&[0, 0, 0, 0]);
                }
            }
        }
    }
    for c in &d.controls {
        let (cx, cy, cw, ch) = (
            c.x as usize,
            c.y as usize,
            c.width as usize,
            c.height as usize,
        );
        let img = if c.has_image() {
            gfx.decode(c.image as usize).ok()
        } else {
            None
        };
        match img {
            Some(im) if (im.width as usize, im.height as usize) == (cw, ch) => {
                for yy in 0..ch {
                    for xx in 0..cw {
                        let (dx, dy) = (cx + xx, cy + yy);
                        let s = &im.rgba[(yy * cw + xx) * 4..][..4];
                        if dx < w && dy < h && s[3] > 0 {
                            px[(dy * w + dx) * 4..][..4].copy_from_slice(s);
                        }
                    }
                }
            }
            _ => {
                for yy in cy..(cy + ch).min(h) {
                    for xx in cx..(cx + cw).min(w) {
                        if yy == cy || xx == cx || yy + 1 == cy + ch || xx + 1 == cx + cw {
                            px[(yy * w + xx) * 4..][..4].copy_from_slice(&[0, 255, 0, 255]);
                        }
                    }
                }
            }
        }
    }
    let f = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut e = png::Encoder::new(f, w as u32, h as u32);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header()?.write_image_data(&px)?;
    Ok(())
}
