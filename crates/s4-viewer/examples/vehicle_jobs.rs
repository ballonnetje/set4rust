//! A vehicle file's jobs (`Gfx\30..34`, `race + 0x1e`): the `.sil` first jobs per vehicle type,
//! each job's frame counts per direction, and a sheet of one image per job.
//! `cargo run -p s4-viewer --example vehicle_jobs -- FILE DIR FRAME OUT.png`

fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (file, dir, frame, out) = (a[0].parse::<u32>()?, a[1].parse::<usize>()?, a[2].parse::<usize>()?, &a[3]);
    let f = s4_formats::gfx::GfxFile::open(s4_formats::game_dir().join("Gfx"), file)?;
    println!("jobs {}, sil {:?}", f.job_count(), f.job_groups());
    let n = f.job_count();
    let cols = 8;
    let cell = 128u32;
    let rows = n.div_ceil(cols) as u32;
    let (w, h) = (cols as u32 * cell, rows.max(1) * cell);
    let mut px = vec![40u8; (w * h * 4) as usize];
    for j in 0..n {
        let counts: Vec<usize> = (0..6).map(|d| f.job_frames(j, d).map_or(0, |r| r.len())).collect();
        println!("job {j}: frames {counts:?}");
        let Some(r) = f.job_frames(j, dir) else { continue };
        if r.is_empty() {
            continue;
        }
        let img = f.decode(r.start + frame.min(r.len() - 1))?;
        let (cx, cy) = ((j % cols) as u32 * cell, (j / cols) as u32 * cell);
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
    }
    s4_viewer::client::write_png(std::path::Path::new(out), w, h, &px)?;
    Ok(())
}
