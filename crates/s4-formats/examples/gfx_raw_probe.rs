//! Which palette indices make up raw (flag 0x20) GUI images; how many are index 0.
use s4_formats::gfx::GfxFile;
fn main() -> anyhow::Result<()> {
    for n in [0u32, 9, 18, 19, 28, 29, 39, 40] {
        let f = GfxFile::open_default(n)?;
        let (mut raw, mut zero, mut total) = (0, 0usize, 0usize);
        for i in 0..f.image_count() {
            if !f.header(i)?.is_raw() { continue; }
            raw += 1;
            let ix = f.decode_indexed(i)?;
            zero += ix.pixels.iter().filter(|&&p| p == 0).count();
            total += ix.pixels.len();
        }
        println!("file {n}: raw images {raw}, index-0 pixels {zero} of {total}");
    }
    let f = GfxFile::open_default(0)?;
    let ix = f.decode_indexed(36)?;
    let w = ix.header.width as usize;
    let pal = f.working_palette(36, &Default::default())?;
    for (x, y) in [(5, 580), (790, 300), (20, 40), (400, 300)] {
        let p = ix.pixels[y * w + x];
        println!("img36 ({x},{y}) index {p} rgba {:?}", pal[p as usize]);
    }
    Ok(())
}
