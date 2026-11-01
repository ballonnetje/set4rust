//! Headers and PNGs of sprite images, for checking what the game draws:
//! `cargo run -p s4-formats --release --example viewer_icons -- 7 20 40` (file 7 images 20..40)
//! or `-- 10 job 146` (the images of .dil entry 146 of file 10). Writes `out/gfx/f<file>_<n>.png`.
use s4_formats::gfx::GfxFile;

fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let file: u32 = a.first().and_then(|s| s.parse().ok()).unwrap_or(7);
    let f = GfxFile::open_default(file)?;
    let range = if a.get(1).map(String::as_str) == Some("job") {
        let job: usize = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
        f.dil_frames(job).ok_or_else(|| anyhow::anyhow!("no .dil entry {job}"))?
    } else {
        let n = |i: usize, d: usize| a.get(i).and_then(|s| s.parse().ok()).unwrap_or(d);
        n(1, 0)..n(2, 40).min(f.image_count())
    };
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/gfx");
    std::fs::create_dir_all(&out)?;
    for i in range {
        let h = f.header(i)?;
        println!("{i}: {}x{} left {} top {} flags {:#x}", h.width, h.height, h.left, h.top, h.flags);
        let img = f.decode(i)?;
        image_png(&out.join(format!("f{file}_{i}.png")), img.width, img.height, &img.rgba)?;
    }
    Ok(())
}

fn image_png(p: &std::path::Path, w: u32, h: u32, rgba: &[u8]) -> anyhow::Result<()> {
    let mut e = png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(p)?), w.max(1), h.max(1));
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header()?.write_image_data(rgba)?;
    Ok(())
}
