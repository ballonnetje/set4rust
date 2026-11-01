//! A gfx file's jobs by direction: `cargo run -p s4-viewer --example effect_jobs -- FILE OUT.png JOB...`
//! prints each job's directions with their image ranges and writes a sheet, one row per
//! fourth direction (images at their hotspot on a 300-px grid, the hotspot marked).

fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let file = a[0].parse::<u32>()?;
    let out = &a[1];
    let jobs: Vec<usize> = a[2..].iter().map(|j| j.parse()).collect::<Result<_, _>>()?;
    let f = s4_formats::gfx::GfxFile::open(s4_formats::game_dir().join("Gfx"), file)?;
    let mut rows = Vec::new();
    for &job in &jobs {
        let dirs = f.job_dirs(job).map(|r| r.len()).unwrap_or(0);
        println!("file {file} job {job}: {dirs} directions");
        for d in 0..dirs {
            let r = f.job_frames(job, d).unwrap_or(0..0);
            let o = r.clone().next().map(|i| f.decode(i)).transpose()?.map(|i| (i.width, i.height, i.left, i.top));
            println!("  dir {d}: images {:?} ({} frames), first w/h/left/top {:?}", r, r.len(), o);
            if d % 4 == 0 && !r.is_empty() {
                rows.push(r);
            }
        }
    }
    let cell = 300i64;
    let cols = rows.iter().map(|r| r.len()).max().unwrap_or(1).max(1) as i64;
    let (w, h) = (cols * cell, rows.len().max(1) as i64 * cell);
    let mut px = vec![40u8; (w * h * 4) as usize];
    for (ry, r) in rows.iter().enumerate() {
        for (cx, i) in r.clone().enumerate() {
            let img = f.decode(i)?;
            let (ox, oy) = (img.left as i64, img.top as i64);
            let (hx, hy) = (cx as i64 * cell + cell / 2, ry as i64 * cell + cell / 2);
            for y in 0..img.height as i64 {
                for x in 0..img.width as i64 {
                    let s = ((y * img.width as i64 + x) * 4) as usize;
                    if img.rgba[s + 3] == 0 {
                        continue;
                    }
                    let (dx, dy) = (hx + ox + x, hy + oy + y);
                    if dx < 0 || dy < 0 || dx >= w || dy >= h {
                        continue;
                    }
                    let d = ((dy * w + dx) * 4) as usize;
                    px[d..d + 4].copy_from_slice(&img.rgba[s..s + 4]);
                }
            }
            let d = ((hy * w + hx) * 4) as usize;
            px[d..d + 4].copy_from_slice(&[255, 0, 255, 255]);
        }
    }
    s4_viewer::client::write_png(std::path::Path::new(out), w as u32, h as u32, &px)?;
    Ok(())
}
