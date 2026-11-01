//! Decode a converted video as fast as possible: `cargo run --release -p s4-video --example decode -- <file.ivf> [frame to dump as <file>.ppm]`.
//! Prints the decode rate and the luma range (full or limited range check).

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let path = std::path::Path::new(&args[1]);
    let dump: Option<usize> = args.get(2).and_then(|s| s.parse().ok());
    let ivf = s4_video::ivf::Ivf::open(path)?;
    let mut dec = s4_video::av1::Decoder::new()?;
    let t = std::time::Instant::now();
    let (mut n, mut lo, mut hi) = (0usize, 255u8, 0u8);
    let mut worst = 0f64;
    for i in 0..ivf.len() {
        let (time, data) = ivf.frame(i);
        let f0 = std::time::Instant::now();
        let f = dec.decode(data, time)?;
        worst = worst.max(f0.elapsed().as_secs_f64());
        let Some(f) = f else { continue };
        for &y in &f.y {
            lo = lo.min(y);
            hi = hi.max(y);
        }
        if dump == Some(n) {
            let mut ppm = format!("P6\n{} {}\n255\n", f.width, f.height).into_bytes();
            for y in 0..f.height as usize {
                for x in 0..f.width as usize {
                    let c = (y / 2) * f.width.div_ceil(2) as usize + x / 2;
                    let (yy, u, v) = (f.y[y * f.width as usize + x] as f32, f.u[c] as f32 - 128.0, f.v[c] as f32 - 128.0);
                    let l = (yy - 16.0) * 1.164;
                    for ch in [l + 1.596 * v, l - 0.392 * u - 0.813 * v, l + 2.017 * u] {
                        ppm.push(ch.clamp(0.0, 255.0) as u8);
                    }
                }
            }
            std::fs::write(path.with_extension("ppm"), ppm)?;
        }
        n += 1;
    }
    let s = t.elapsed().as_secs_f64();
    println!("{n} of {} frames in {s:.2} s: {:.1} ms/frame, worst {:.1} ms; luma {lo}..={hi}; duration {:.2} s", ivf.len(), s * 1000.0 / n as f64, worst * 1000.0, ivf.duration());
    Ok(())
}
