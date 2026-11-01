//! Contact sheets drawn purely through the gamedata → gfx resolution:
//! every building of every race (from buildingInfo.xml, finished image) and every landscape object
//! type (Object enum → 5.sil → job → dil[amount-1] → frame 0). Transparent PNGs in `out/gfx/`.
//! `cargo run -p s4-formats --example gamedata_sheet --release`

use std::path::Path;

use s4_formats::gamedata::resolve::{self, BuildingPart};
use s4_formats::gamedata::{GameData, Object, Race};
use s4_formats::gfx::{DecodeOptions, GfxFile, Image};

fn save(path: &Path, w: usize, h: usize, px: &[u8]) -> anyhow::Result<()> {
    let file = std::fs::File::create(path)?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut wr = enc.write_header()?;
    wr.write_image_data(px)?;
    wr.finish()?;
    println!("wrote {}", path.display());
    Ok(())
}

/// Shelf-pack images into a transparent sheet.
fn sheet(imgs: &[Image], width: usize, path: &Path) -> anyhow::Result<()> {
    let (mut x, mut y, mut row_h) = (0usize, 0usize, 0usize);
    let mut pos = Vec::new();
    for i in imgs {
        let (w, h) = (i.width as usize + 4, i.height as usize + 4);
        if x + w > width && x > 0 {
            x = 0;
            y += row_h;
            row_h = 0;
        }
        pos.push((x + 2, y + 2));
        x += w;
        row_h = row_h.max(h);
    }
    let h = (y + row_h).max(1);
    let mut px = vec![0u8; width * h * 4];
    for (img, &(ox, oy)) in imgs.iter().zip(&pos) {
        for yy in 0..img.height as usize {
            for xx in 0..img.width as usize {
                let s = &img.rgba[(yy * img.width as usize + xx) * 4..][..4];
                if s[3] != 0 && ox + xx < width {
                    px[((oy + yy) * width + ox + xx) * 4..][..4].copy_from_slice(s);
                }
            }
        }
    }
    save(path, width, h, &px)
}

fn main() -> anyhow::Result<()> {
    let gd = GameData::load_default()?;
    let gfx_dir = s4_formats::game_dir().join("Gfx");
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/gfx");
    std::fs::create_dir_all(&out)?;
    let opt = DecodeOptions::default();

    for race in Race::ALL.iter().copied() {
        let file = GfxFile::open(&gfx_dir, resolve::building_file(race))?;
        let mut imgs = Vec::new();
        let mut missing = Vec::new();
        for b in gd.buildings.iter().filter(|b| b.race == race) {
            match resolve::building_frames(&file, b.building, BuildingPart::Finished) {
                Some(r) => imgs.push(file.decode_with(r.start, &opt)?),
                None => missing.push(b.building.name()),
            }
        }
        println!("{race:?}: {} buildings drawn, missing gfx: {missing:?}", imgs.len());
        sheet(&imgs, 2400, &out.join(format!("gamedata_buildings_{}.png", race.name())))?;
    }

    let objects = GfxFile::open(&gfx_dir, resolve::OBJECT_FILE)?;
    let mut imgs = Vec::new();
    let mut missing = Vec::new();
    for &o in Object::ALL.iter().filter(|&&o| o != Object::NO_OBJECT) {
        match resolve::object_frames(&objects, o, 1) {
            Some(r) => imgs.push(objects.decode_with(r.start, &opt)?),
            None => missing.push(o.name()),
        }
    }
    println!("objects: {} of {} drawn, missing gfx: {missing:?}", imgs.len(), Object::ALL.len() - 1);
    sheet(&imgs, 2400, &out.join("gamedata_objects.png"))?;
    Ok(())
}
