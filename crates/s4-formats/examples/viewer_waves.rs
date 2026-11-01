//! Dump the wave animations of file 5 (jobs 517..=532, see s4-viewer `waves.rs`): size, offsets,
//! flags and pixel-class counts of every frame, plus a contact sheet of frame 0 of each job
//! (shadow pixels drawn black on a mid-blue background) to `out/viewer/waves_sheet.png`.

use s4_formats::gfx::GfxFile;

fn main() -> anyhow::Result<()> {
    let f = GfxFile::open(s4_formats::game_dir().join("Gfx"), 5)?;
    let opt = Default::default();
    let mut tiles: Vec<(u32, u32, Vec<u8>)> = Vec::new();
    for job in 517..=532usize {
        let Some(r) = f.job_frames(job, 0) else {
            println!("job {job}: none");
            continue;
        };
        let pal = f.working_palette(r.start, &opt)?;
        for (k, i) in r.clone().enumerate() {
            let im = f.decode_indexed(i)?;
            let h = &im.header;
            let (mut t, mut s, mut c) = (0, 0, 0);
            for &p in &im.pixels {
                match p {
                    0 => t += 1,
                    1 => s += 1,
                    _ => c += 1,
                }
            }
            let keep = k == 0 || (std::env::args().nth(1).as_deref() == Some(&job.to_string()));
            if keep {
                if k == 0 { println!(
                    "job {job}: {} frames; frame 0 image {i} {}x{} left {} top {} half {} raw {} | transparent {t} shadow {s} colour {c}",
                    r.len(),
                    h.width,
                    h.height,
                    h.left,
                    h.top,
                    h.half_scale(),
                    im.raw
                ); }
                let (w, hh) = (h.width as u32, h.height as u32);
                let mut rgba = Vec::with_capacity((w * hh * 4) as usize);
                for &p in &im.pixels {
                    let c = match p {
                        0 => [40, 70, 140, 255],
                        1 => [0, 0, 0, 255],
                        p => pal[p as usize],
                    };
                    rgba.extend_from_slice(&c);
                }
                tiles.push((w, hh, rgba));
            }
        }
    }
    // Stack the tiles vertically with a 4 px gap.
    let w = tiles.iter().map(|t| t.0).max().unwrap_or(1);
    let h: u32 = tiles.iter().map(|t| t.1 + 4).sum();
    let mut img = vec![255u8; (w * h * 4) as usize];
    let mut y0 = 0;
    for (tw, th, px) in &tiles {
        for y in 0..*th {
            let d = (((y0 + y) * w) * 4) as usize;
            let s = (y * tw * 4) as usize;
            img[d..d + (*tw * 4) as usize].copy_from_slice(&px[s..s + (*tw * 4) as usize]);
        }
        y0 += th + 4;
    }
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/viewer/waves_sheet.png");
    let mut e = png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(&out)?), w, h);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header()?.write_image_data(&img)?;
    println!("wrote {}", out.display());
    Ok(())
}
