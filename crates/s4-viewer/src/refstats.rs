//! `--refstats`: compare a render with a reference screenshot on matched surfaces.
//!
//! Whole-frame statistics mix sprites, GUI and terrain and say little. This mode renders the same
//! view three times (lit with sprites, lit without sprites, unlit without sprites) and compares
//! only terrain pixels that have no sprite in our frame. Each pixel is classed by its *unlit*
//! texture colour (deep/shallow water, beach, grass) and its light level (lit / unlit ratio), and
//! the mean colour, luminance, saturation and local spread of ours and the reference are reported
//! per class.
//!
//! The reference may be scaled (e.g. Windows DPI scaling, 1.25): `--ref-scale` box-filters it back
//! to game pixels. `--mask-left` hides a GUI panel (game px). Near-black reference pixels
//! (unexplored land) are skipped.

use std::path::Path;

use crate::gpu::{Gpu, View};

pub struct RefStatsArgs<'a> {
    pub reference: &'a str,
    pub ref_scale: f32,
    pub mask_left: u32,
    pub view: View,
    pub size: (u32, u32),
    pub save_prefix: Option<&'a str>,
}

/// Area-average resample of an RGBA image to `(w, h)`, reading `src_w × src_h` source pixels
/// scaled by `scale` (source px per destination px).
fn box_downscale(src: &[u8], sw: u32, sh: u32, scale: f32, w: u32, h: u32) -> Vec<[f32; 3]> {
    let mut out = vec![[0f32; 3]; (w * h) as usize];
    for y in 0..h {
        let (y0, y1) = (y as f32 * scale, (y + 1) as f32 * scale);
        for x in 0..w {
            let (x0, x1) = (x as f32 * scale, (x + 1) as f32 * scale);
            let (mut acc, mut wsum) = ([0f32; 3], 0f32);
            for sy in y0.floor() as u32..(y1.ceil() as u32).min(sh) {
                let wy = (y1.min(sy as f32 + 1.0) - y0.max(sy as f32)).max(0.0);
                for sx in x0.floor() as u32..(x1.ceil() as u32).min(sw) {
                    let wx = (x1.min(sx as f32 + 1.0) - x0.max(sx as f32)).max(0.0);
                    let o = ((sy * sw + sx) * 4) as usize;
                    for c in 0..3 {
                        acc[c] += src[o + c] as f32 * wx * wy;
                    }
                    wsum += wx * wy;
                }
            }
            if wsum > 0.0 {
                out[(y * w + x) as usize] = acc.map(|v| v / wsum);
            }
        }
    }
    out
}

fn px(img: &[u8], i: usize) -> [f32; 3] {
    [img[i * 4] as f32, img[i * 4 + 1] as f32, img[i * 4 + 2] as f32]
}

fn sat(c: [f32; 3]) -> f32 {
    let (mx, mn) = (c[0].max(c[1]).max(c[2]), c[0].min(c[1]).min(c[2]));
    if mx > 0.0 { (mx - mn) / mx } else { 0.0 }
}

#[derive(Default)]
struct Acc {
    n: usize,
    ours: [f64; 3],
    refc: [f64; 3],
    lum: [f64; 2],
    lum2: [f64; 2],
    sat: [f64; 2],
}

impl Acc {
    fn add(&mut self, o: [f32; 3], r: [f32; 3]) {
        self.n += 1;
        for c in 0..3 {
            self.ours[c] += o[c] as f64;
            self.refc[c] += r[c] as f64;
        }
        for (k, v) in [o, r].iter().enumerate() {
            let l = (v[0] + v[1] + v[2]) as f64 / 3.0;
            self.lum[k] += l;
            self.lum2[k] += l * l;
            self.sat[k] += sat(*v) as f64;
        }
    }
}

pub fn run(gpu: &mut Gpu, a: &RefStatsArgs) -> anyhow::Result<()> {
    let (w, h) = a.size;
    let (rw, rh, rimg) = crate::client::read_png(a.reference)?;
    let reference = box_downscale(&rimg, rw, rh, a.ref_scale, w, h);

    let lit = gpu.capture(a.view)?;
    let sprites = gpu.sprites.take();
    let bare = gpu.capture(a.view)?;
    let lighting = gpu.terrain.as_ref().is_some_and(|t| t.lighting);
    if let Some(t) = &mut gpu.terrain {
        t.lighting = false;
    }
    let unlit = gpu.capture(a.view)?;
    if let Some(t) = &mut gpu.terrain {
        t.lighting = lighting;
    }
    gpu.sprites = sprites;
    if let Some(p) = a.save_prefix {
        for (name, img) in [("", &lit), ("_nosprites", &bare), ("_unlit", &unlit)] {
            crate::client::write_png(Path::new(&format!("{p}{name}.png")), w, h, img)?;
        }
    }

    let names = ["deep water", "shallow water", "beach", "grass lit (>0.9)", "grass mid (0.6-0.9)", "grass shaded (<0.6)"];
    let mut acc: Vec<Acc> = (0..names.len()).map(|_| Acc::default()).collect();
    let (mut diff_sum, mut diff_n, mut within8) = (0f64, 0usize, 0usize);
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) as usize;
            let r = reference[i];
            if x < a.mask_left || r[0].max(r[1]).max(r[2]) <= 40.0 {
                continue;
            }
            let (o, n, u) = (px(&lit, i), px(&bare, i), px(&unlit, i));
            if o != n {
                continue; // a sprite covers it in our frame
            }
            let d = (0..3).map(|c| (o[c] - r[c]).abs()).fold(0f32, f32::max);
            diff_sum += d as f64;
            diff_n += 1;
            within8 += (d <= 8.0) as usize;
            let light = (n[0] + n[1] + n[2]) / (u[0] + u[1] + u[2]).max(1.0);
            let water = u[2] > u[0] + 30.0 && u[2] > 100.0;
            let class = if water {
                Some(if u[1] < u[2] - 40.0 { 0 } else { 1 })
            } else if u[0] > u[1] && u[1] > u[2] + 20.0 && u[0] > 150.0 {
                Some(2)
            } else if u[1] > u[0] && u[1] > u[2] + 20.0 {
                Some(if light > 0.9 { 3 } else if light > 0.6 { 4 } else { 5 })
            } else {
                None
            };
            if let Some(k) = class {
                acc[k].add(o, r);
            }
        }
    }
    println!("{:22} {:>8}  {:>16}  {:>16}  {:>13}  {:>11}  {:>11}", "surface", "n", "ours RGB", "ref RGB", "lum o/r", "sat o/r", "std o/r");
    for (name, a) in names.iter().zip(&acc) {
        if a.n < 500 {
            println!("{name:22} {:>8}  (too few)", a.n);
            continue;
        }
        let n = a.n as f64;
        let rgb = |v: &[f64; 3]| format!("[{:.0},{:.0},{:.0}]", v[0] / n, v[1] / n, v[2] / n);
        let lum = [a.lum[0] / n, a.lum[1] / n];
        let std = [0, 1].map(|k| (a.lum2[k] / n - lum[k] * lum[k]).max(0.0).sqrt());
        println!(
            "{name:22} {:>8}  {:>16}  {:>16}  {:>6.1}/{:<6.1}  {:>5.2}/{:<5.2}  {:>5.1}/{:<5.1}",
            a.n,
            rgb(&a.ours),
            rgb(&a.refc),
            lum[0],
            lum[1],
            a.sat[0] / n,
            a.sat[1] / n,
            std[0],
            std[1]
        );
    }
    if diff_n > 0 {
        println!(
            "masked pixels {diff_n}: mean max-channel diff {:.2}, within 8: {:.2}%",
            diff_sum / diff_n as f64,
            within8 as f64 / diff_n as f64 * 100.0
        );
    }
    Ok(())
}
