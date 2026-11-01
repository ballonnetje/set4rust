//! Write contact sheets of sprite files to `out/gfx/`.
//! `cargo run -p s4-formats --example gfx_dump --release [file numbers...]`

use std::path::Path;

use s4_formats::gfx::{DecodeOptions, GfxFile, Image, Layout, PlayerColor};

/// RGBA canvas, fully transparent where no sprite pixel lands (straight alpha, no backdrop).
struct Canvas {
    w: usize,
    h: usize,
    px: Vec<u8>,
}

impl Canvas {
    fn new(w: usize, h: usize) -> Self {
        Self { w, h, px: vec![0u8; w * h * 4] }
    }

    /// Source-over composite in straight alpha (sprites do not overlap, so this is a copy).
    fn blit(&mut self, img: &Image, x0: i64, y0: i64) {
        for y in 0..img.height as i64 {
            for x in 0..img.width as i64 {
                let (dx, dy) = (x0 + x, y0 + y);
                if dx < 0 || dy < 0 || dx >= self.w as i64 || dy >= self.h as i64 {
                    continue;
                }
                let s = &img.rgba[((y * img.width as i64 + x) * 4) as usize..][..4];
                let sa = s[3] as u32;
                if sa == 0 {
                    continue;
                }
                let d = &mut self.px[((dy as usize * self.w + dx as usize) * 4)..][..4];
                let da = d[3] as u32;
                let oa = sa * 255 + da * (255 - sa); // alpha * 255
                for c in 0..3 {
                    d[c] = ((s[c] as u32 * sa * 255 + d[c] as u32 * da * (255 - sa)) / oa.max(1)) as u8;
                }
                d[3] = (oa / 255) as u8;
            }
        }
    }

    fn save(&self, path: &Path) -> anyhow::Result<()> {
        let file = std::fs::File::create(path)?;
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), self.w as u32, self.h as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header()?;
        w.write_image_data(&self.px)?;
        w.finish()?;
        println!("wrote {}", path.display());
        Ok(())
    }
}

/// Shelf-pack a list of images into a sheet `width` wide.
fn sheet(imgs: &[Image], width: usize) -> Canvas {
    let mut placed = Vec::new();
    let (mut x, mut y, mut row_h) = (0usize, 0usize, 0usize);
    for img in imgs {
        let (w, h) = (img.width as usize + 2, img.height as usize + 2);
        if x + w > width && x > 0 {
            x = 0;
            y += row_h;
            row_h = 0;
        }
        placed.push((x + 1, y + 1));
        x += w;
        row_h = row_h.max(h);
    }
    let mut c = Canvas::new(width, (y + row_h).max(1));
    for (img, &(px, py)) in imgs.iter().zip(&placed) {
        c.blit(img, px as i64, py as i64);
    }
    c
}

/// Rows = (job, direction), columns = frames; every frame is placed by its hot-spot, so an
/// animation lines up across its row. Row height fits that row's frames.
fn job_sheet(f: &GfxFile, jobs: &[usize], max_frames: usize, opt: &DecodeOptions) -> anyhow::Result<Canvas> {
    let mut rows: Vec<Vec<Image>> = Vec::new();
    for &job in jobs {
        let Some(dirs) = f.job_dirs(job) else { continue };
        for d in 0..dirs.len() {
            let Some(frames) = f.job_frames(job, d) else { continue };
            let row: Vec<Image> =
                frames.take(max_frames).map(|i| f.decode_with(i, opt)).collect::<Result<_, _>>()?;
            rows.push(row);
        }
    }
    let ext = |imgs: &mut dyn Iterator<Item = &Image>| {
        let (mut l, mut t, mut r, mut b) = (0i32, 0i32, 1i32, 1i32);
        for i in imgs {
            l = l.max(i.left);
            t = t.max(i.top);
            r = r.max(i.width as i32 - i.left);
            b = b.max(i.height as i32 - i.top);
        }
        (l, t, r, b)
    };
    let (left, _, right, _) = ext(&mut rows.iter().flatten());
    let cw = (left + right + 2) as usize;
    let ncol = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
    let heights: Vec<(i32, usize)> = rows
        .iter()
        .map(|r| {
            let (_, t, _, b) = ext(&mut r.iter());
            (t, (t + b + 2) as usize)
        })
        .collect();
    let mut c = Canvas::new(cw * ncol, heights.iter().map(|h| h.1).sum::<usize>().max(1));
    let mut y = 0usize;
    for (row, &(top, h)) in rows.iter().zip(&heights) {
        for (k, img) in row.iter().enumerate() {
            let (hx, hy) = ((k * cw) as i64 + 1 + left as i64, y as i64 + 1 + top as i64);
            c.blit(img, hx - img.left as i64, hy - img.top as i64);
        }
        y += h;
    }
    Ok(c)
}

fn decode_all(f: &GfxFile, idx: impl Iterator<Item = usize>, opt: &DecodeOptions) -> anyhow::Result<Vec<Image>> {
    Ok(idx.map(|i| f.decode_with(i, opt)).collect::<Result<_, _>>()?)
}

fn main() -> anyhow::Result<()> {
    let dir = s4_formats::game_dir().join("Gfx");
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/gfx");
    std::fs::create_dir_all(&out)?;
    let blue = DecodeOptions { player: Some(PlayerColor { slot: 1, rgb: [40, 90, 255] }), ..Default::default() };
    let def = DecodeOptions::default();
    let args: Vec<u32> = std::env::args().skip(1).filter_map(|a| a.parse().ok()).collect();
    let files = if args.is_empty() {
        vec![0, 1, 3, 4, 5, 6, 7, 8, 9, 10, 13, 14, 18, 19, 20, 23, 24, 28, 29, 30, 34, 36, 37, 39, 40]
    } else {
        args
    };
    for n in files {
        let f = GfxFile::open(&dir, n)?;
        let name = |s: &str| out.join(format!("{n}_{s}.png"));
        match f.layout {
            Layout::Jobs if f.has_team_colour() => {
                // Settlers/vehicles: first job of each of the first job groups (types), all 6 directions.
                let mut jobs: Vec<usize> = f.job_groups().iter().map(|&j| j as usize).collect();
                jobs.retain(|&j| j != 0 && f.job_dirs(j).is_some());
                jobs.dedup();
                job_sheet(&f, &jobs[..jobs.len().min(6)], 12, &blue)?.save(&name("jobs"))?;
                let mid: Vec<usize> = jobs.iter().copied().skip(20).take(6).collect();
                if !mid.is_empty() {
                    job_sheet(&f, &mid, 12, &blue)?.save(&name("jobs2"))?;
                }
                let n_img = f.image_count().min(400);
                sheet(&decode_all(&f, 0..n_img, &blue)?, 2048).save(&name("first400"))?;
            }
            Layout::Jobs => {
                // One image (dir 0, frame 0) per job, then the full sheet of the first jobs.
                let firsts: Vec<usize> =
                    (0..f.job_count()).filter_map(|j| f.job_frames(j, 0).map(|r| r.start)).collect();
                sheet(&decode_all(&f, firsts.into_iter(), &def)?, 2048).save(&name("perjob"))?;
                let n_img = f.image_count().min(300);
                sheet(&decode_all(&f, 0..n_img, &def)?, 2048).save(&name("first300"))?;
            }
            Layout::Buildings => {
                // Finished building (dil +1) and construction image (dil +0) of every building.
                let mut fin = Vec::new();
                let mut con = Vec::new();
                for b in 0..f.job_count() {
                    let Some(d) = f.job_dirs(b) else { continue };
                    if let Some(r) = f.dil_frames(d.start + 1).filter(|_| d.len() > 1) {
                        fin.push(r.start);
                    }
                    if let Some(r) = f.dil_frames(d.start) {
                        con.push(r.start);
                    }
                }
                sheet(&decode_all(&f, fin.into_iter(), &def)?, 2400).save(&name("finished"))?;
                sheet(&decode_all(&f, con.into_iter(), &def)?, 2400).save(&name("dil0"))?;
            }
            Layout::Flat => {
                let small: Vec<usize> = (0..f.image_count())
                    .filter(|&i| f.header(i).is_ok_and(|h| h.width <= 400 && h.height <= 300))
                    .collect();
                sheet(&decode_all(&f, small.into_iter().take(600), &def)?, 2048).save(&name("small"))?;
                if let Some(big) = (0..f.image_count()).find(|&i| f.header(i).is_ok_and(|h| h.is_raw() && h.width >= 640)) {
                    sheet(&[f.decode(big)?], 800).save(&name(&format!("raw{big}")))?;
                }
            }
        }
    }
    Ok(())
}
