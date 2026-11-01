//! Lists the placeholder images of gfx files: every job / direction / frame at least 20×20 whose
//! area is 90 % opaque and 90 % of that near-black colour pixels (not shadow). Job 0 of most
//! files is one (settlers 61×42, effects 60×60, buildings 181×109); file 4 / 36 (the unused
//! hardware object engine) have more. Bug B4 was file 6 job 0 drawn for effect 0.
//! `cargo run -p s4-formats --example gfx_black --release [file numbers...]` (default: 20..=24).

use s4_formats::gfx::{DecodeOptions, GfxFile};

fn main() {
    let files: Vec<u32> = std::env::args().skip(1).filter_map(|a| a.parse().ok()).collect();
    let files = if files.is_empty() { (20..=24).collect() } else { files };
    for n in files {
        let Ok(f) = GfxFile::open_default(n) else {
            println!("file {n}: not readable");
            continue;
        };
        let opt = DecodeOptions::default();
        for job in 0..f.job_count() {
            for dir in 0..6 {
                let Some(r) = f.job_frames(job, dir) else { continue };
                for (k, i) in r.clone().enumerate() {
                    let Ok(ix) = f.decode_indexed(i) else { continue };
                    let Ok(pal) = f.working_palette(i, &opt) else { continue };
                    let (mut solid, mut black) = (0usize, 0usize);
                    for &p in &ix.pixels {
                        if p > 1 {
                            solid += 1;
                            let c = pal[p as usize];
                            if (c[0] as u32 + c[1] as u32 + c[2] as u32) < 40 {
                                black += 1;
                            }
                        }
                    }
                    let area = ix.header.width as usize * ix.header.height as usize;
                    if area >= 400 && solid * 10 >= area * 9 && black * 10 >= solid * 9 {
                        let h = &ix.header;
                        println!("file {n} job {job} dir {dir} frame {k}/{} image {i}: {}x{} solid {solid} black {black}", r.len(), h.width, h.height);
                    }
                }
            }
        }
    }
}
