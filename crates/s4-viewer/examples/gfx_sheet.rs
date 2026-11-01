//! Contact sheet of a gfx file's images: `cargo run -p s4-viewer --example gfx_sheet -- FILE START COUNT OUT.png`.
//! Images are laid out on a grid of 64×64 cells with their index above (cell order = index).

fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (file, start, count, out) = (a[0].parse::<u32>()?, a[1].parse::<usize>()?, a[2].parse::<usize>()?, &a[3]);
    let f = s4_formats::gfx::GfxFile::open(s4_formats::game_dir().join("Gfx"), file)?;
    let n = count.min(f.image_count().saturating_sub(start));
    let cols = 16;
    let cell = 72u32;
    let rows = n.div_ceil(cols) as u32;
    let (w, h) = (cols as u32 * cell, rows.max(1) * cell);
    let mut px = vec![40u8; (w * h * 4) as usize];
    for k in 0..n {
        let img = f.decode(start + k)?;
        let (cx, cy) = ((k % cols) as u32 * cell, (k / cols) as u32 * cell);
        for y in 0..img.height.min(cell) {
            for x in 0..img.width.min(cell) {
                let s = ((y * img.width + x) * 4) as usize;
                if img.rgba[s + 3] == 0 {
                    continue;
                }
                let d = (((cy + y) * w + cx + x) * 4) as usize;
                px[d..d + 4].copy_from_slice(&img.rgba[s..s + 4]);
            }
        }
        println!("{}: {}x{}", start + k, img.width, img.height);
    }
    s4_viewer::client::write_png(std::path::Path::new(out), w, h, &px)?;
    Ok(())
}
