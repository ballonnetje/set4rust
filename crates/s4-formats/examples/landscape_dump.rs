//! Decode every landscape texture file to PNG atlases and render a synthetic terrain patch.
//!
//! `cargo run -p s4-formats --example landscape_dump` → `out/landscape/`.

use s4_formats::gfx::landscape::*;
use std::path::{Path, PathBuf};

fn out_dir() -> PathBuf {
    let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/landscape");
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn write_png(path: &Path, w: u32, h: u32, rgba: &[u8]) -> anyhow::Result<()> {
    let f = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut e = png::Encoder::new(f, w, h);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header()?.write_image_data(rgba)?;
    Ok(())
}

/// Pages side by side, `cols` per row, 2 px gap.
fn atlas(pages: &[TexturePage], cols: usize) -> (u32, u32, Vec<u8>) {
    let s = pages[0].size as usize;
    let rows = pages.len().div_ceil(cols);
    let (w, h) = (cols * (s + 2), rows * (s + 2));
    let mut img = vec![0u8; w * h * 4];
    for (i, p) in pages.iter().enumerate() {
        let (ox, oy) = ((i % cols) * (s + 2), (i / cols) * (s + 2));
        for y in 0..s {
            let dst = ((oy + y) * w + ox) * 4;
            img[dst..dst + s * 4].copy_from_slice(&p.rgba[y * s * 4..(y + 1) * s * 4]);
        }
    }
    (w as u32, h as u32, img)
}

fn dump_files(out: &Path) -> anyhow::Result<Vec<TexturePage>> {
    let game = s4_formats::game_dir();
    let mut hq_pages = Vec::new();
    for set in [TextureSet::Classic, TextureSet::New] {
        for hq in [true, false] {
            for fmt in [Rgb16::Rgb555, Rgb16::Rgb565] {
                let path = texture_path(&game, set, hq, fmt);
                let tex = LandscapeTextures::open(&path, fmt)?;
                let name = path.file_name().unwrap().to_string_lossy().replace('.', "_");
                for (ri, r) in tex.records.iter().enumerate() {
                    println!(
                        "{name} rec{ri} @{:#x}: flags={} ver={} count_byte={} pages={} {}x{}",
                        r.offset,
                        r.flags,
                        r.version,
                        r.count_byte,
                        r.pages.len(),
                        r.pages[0].size,
                        r.pages[0].size
                    );
                    // Only write the 565 variants as PNG (555 decodes to the same picture).
                    if fmt == Rgb16::Rgb565 {
                        let (w, h, img) = atlas(&r.pages, 11);
                        write_png(&out.join(format!("{name}_rec{ri}.png")), w, h, &img)?;
                    }
                }
                if set == TextureSet::Classic && hq && fmt == Rgb16::Rgb565 {
                    hq_pages = tex.hardware_pages().unwrap().to_vec();
                }
            }
        }
    }
    // Compare 555 vs 565 decode of the same file pair.
    for set in [TextureSet::Classic, TextureSet::New] {
        let a = LandscapeTextures::open(&texture_path(&game, set, true, Rgb16::Rgb555), Rgb16::Rgb555)?;
        let b = LandscapeTextures::open(&texture_path(&game, set, true, Rgb16::Rgb565), Rgb16::Rgb565)?;
        let (pa, pb) = (&a.records[0].pages, &b.records[0].pages);
        let mut maxd = 0i32;
        for (x, y) in pa.iter().zip(pb) {
            for (p, q) in x.rgba.iter().zip(&y.rgba) {
                maxd = maxd.max((*p as i32 - *q as i32).abs());
            }
        }
        println!("{set:?} gh5 vs gh6 record 0: max channel difference {maxd}");
    }
    Ok(hq_pages)
}

// ------------------------------------------------------------------------------------------------
// Synthetic terrain
// ------------------------------------------------------------------------------------------------

const MW: usize = 40;
const MH: usize = 36;

struct Map {
    h: Vec<u8>,
    g: Vec<u8>,
}

fn synthetic_map() -> Map {
    let mut h = vec![0u8; MW * MH];
    let mut g = vec![0u8; MW * MH];
    for y in 0..MH {
        for x in 0..MW {
            // Work in screen-aligned coordinates so the bands look vertical on screen.
            let sx = x as f32 - y as f32 / 2.0; // ranges about -18..40
            let i = y * MW + x;
            let ty = if sx < 10.0 {
                16
            } else if sx < 13.0 {
                48
            } else {
                // Water levels 0..7 with distance from the beach.
                (((sx - 13.0) / 1.5) as i32).clamp(0, 7) as u8
            };
            g[i] = ty;
            // A hill centred in the grass area.
            let (dx, dy) = (sx - 1.0, y as f32 - 16.0);
            let r2 = dx * dx + dy * dy * 0.5;
            let hill = 45.0 * (-r2 / 60.0).exp();
            let base = if ty == 16 { 6.0 } else if ty == 48 { 3.0 } else { 0.0 };
            h[i] = (base + hill).round() as u8;
            // A patch of rock (with its two-step band) on the hilltop.
            if r2 < 6.0 {
                g[i] = 32;
            } else if r2 < 14.0 {
                g[i] = 33;
            } else if r2 < 24.0 {
                g[i] = 17;
            }
        }
    }
    Map { h, g }
}

fn clamp8(v: f32) -> u8 {
    v.clamp(0.0, 255.0).round() as u8
}

struct Vert {
    x: f32,
    y: f32,
    u: f32,
    v: f32,
    d: [f32; 3],
    s: [f32; 3],
}

/// Affine textured, Gouraud-lit triangle with point sampling; pixel centres at +0.5,
/// top-left fill rule approximated by `>= 0` edge tests on the centre.
/// `centre` = where the pixel centre sits: 0.5 (D3D10+/wgpu) or 0.0 (D3D7, what the game used).
fn fill(img: &mut [u8], w: usize, h: usize, page: &TexturePage, t: [Vert; 3], centre: f32) {
    let area = (t[1].x - t[0].x) * (t[2].y - t[0].y) - (t[2].x - t[0].x) * (t[1].y - t[0].y);
    if area.abs() < 1e-6 {
        return;
    }
    let minx = t.iter().map(|v| v.x).fold(f32::MAX, f32::min).floor().max(0.0) as usize;
    let maxx = (t.iter().map(|v| v.x).fold(f32::MIN, f32::max).ceil() as usize + 1).min(w);
    let miny = t.iter().map(|v| v.y).fold(f32::MAX, f32::min).floor().max(0.0) as usize;
    let maxy = (t.iter().map(|v| v.y).fold(f32::MIN, f32::max).ceil() as usize + 1).min(h);
    let size = page.size as f32;
    for py in miny..maxy {
        for px in minx..maxx {
            let (cx, cy) = (px as f32 + centre, py as f32 + centre);
            let w0 = ((t[1].x - cx) * (t[2].y - cy) - (t[2].x - cx) * (t[1].y - cy)) / area;
            let w1 = ((t[2].x - cx) * (t[0].y - cy) - (t[0].x - cx) * (t[2].y - cy)) / area;
            let w2 = 1.0 - w0 - w1;
            if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                continue;
            }
            let u = w0 * t[0].u + w1 * t[1].u + w2 * t[2].u;
            let v = w0 * t[0].v + w1 * t[1].v + w2 * t[2].v;
            let tex = page.texel((u * size).floor() as i32, (v * size).floor() as i32);
            let o = (py * w + px) * 4;
            for c in 0..3 {
                let d = w0 * t[0].d[c] + w1 * t[1].d[c] + w2 * t[2].d[c];
                let s = w0 * t[0].s[c] + w1 * t[1].s[c] + w2 * t[2].s[c];
                img[o + c] = clamp8(tex[c] as f32 * d / 255.0 + s);
            }
            img[o + 3] = 255;
        }
    }
}

fn render(map: &Map, pages: &[TexturePage], shading: bool, centre: f32) -> (usize, usize, Vec<u8>) {
    let tri_w = DEFAULT_TRIANGLE_SIZE;
    let (wpx, hpx) = (24i32, 12i32);
    let ox = hpx * MH as i32; // shift so x - y/2 >= 0 stays on screen
    let oy = 100;
    let (w, h) = ((wpx * MW as i32) as usize, (hpx * MH as i32 + oy + 20) as usize);
    let mut img = vec![0u8; w * h * 4];
    for p in img.as_chunks_mut::<4>().0 {
        p.copy_from_slice(&[0, 0, 0, 255]); // black, like the viewer's clear colour
    }
    let at = |x: usize, y: usize| y * MW + x;
    let hgt = |x: usize, y: usize| map.h[at(x.min(MW - 1), y.min(MH - 1))];
    let vert = |x: usize, y: usize, uv: [f32; 2]| -> Vert {
        let hh = hgt(x, y);
        let up = if y > 0 { hgt(x, y - 1) } else { hh };
        let down = if y + 1 < MH { hgt(x, y + 1) } else { hh };
        let (d, s) = if shading { shade_colors(shade_index(up, down)) } else { ([255; 3], [0; 3]) };
        let sx = wpx * x as i32 - (wpx / 2) * y as i32 + ox;
        let y16 = ((hpx * y as i32 + oy) << 16) - height_offset_fixed(hh, tri_w);
        Vert {
            x: sx as f32,
            y: (y16 >> 16) as f32,
            u: uv[0] + PATTERN_UV_BIAS,
            v: uv[1] + PATTERN_UV_BIAS,
            d: d.map(|v| v as f32),
            s: s.map(|v| v as f32),
        }
    };
    let corner = |x: usize, y: usize| Corner { ground: map.g[at(x, y)], dark: false };
    // Draw order of DrawTexturedLandscape: rows top to bottom, columns left to right, Up then Down.
    for y in 0..MH - 1 {
        for x in 0..MW - 1 {
            let [up, down] = column_pieces(corner(x, y), corner(x, y + 1), corner(x + 1, y + 1), corner(x + 1, y), x & 1 == 1);
            let uv = up.uv(x as i32, y as i32, Tri::Up);
            let t = [vert(x, y, uv[0]), vert(x, y + 1, uv[1]), vert(x + 1, y + 1, uv[2])];
            fill(&mut img, w, h, &pages[up.page as usize], t, centre);
            let uv = down.uv(x as i32, y as i32, Tri::Down);
            let t = [vert(x, y, uv[0]), vert(x + 1, y, uv[1]), vert(x + 1, y + 1, uv[2])];
            fill(&mut img, w, h, &pages[down.page as usize], t, centre);
        }
    }
    (w, h, img)
}

/// Every transition-table entry drawn as its cell outline over the page, to eyeball the atlas
/// layout: pages 0..22 with each used cell's six triangles outlined.
fn cells_overlay(pages: &[TexturePage]) -> (u32, u32, Vec<u8>) {
    let mut pages: Vec<TexturePage> = pages[..22].to_vec();
    let table = transition_table();
    let mut used = std::collections::BTreeSet::new();
    for row in &table {
        for e in row {
            if *e != TransitionEntry::default() {
                used.insert((e.page, e.cell_x, e.cell_y));
                if e.alternate {
                    used.insert((e.page, e.cell_x, e.cell_y + 1));
                }
            }
        }
    }
    used.insert((5, 2, 2));
    used.insert((5, 3, 2));
    for (page, cx, cy) in used {
        let p = &mut pages[page as usize];
        let s = p.size as f32;
        for k in 0..6 {
            let uv = pattern_uv(cx, cy, k);
            for i in 0..3 {
                let (a, b) = (uv[i], uv[(i + 1) % 3]);
                for t in 0..64 {
                    let f = t as f32 / 64.0;
                    let x = ((a[0] + (b[0] - a[0]) * f) * s) as usize;
                    let y = ((a[1] + (b[1] - a[1]) * f) * s) as usize;
                    let o = (y.min(p.size as usize - 1) * p.size as usize + x.min(p.size as usize - 1)) * 4;
                    p.rgba[o..o + 4].copy_from_slice(&[255, 0, 255, 255]);
                }
            }
        }
    }
    atlas(&pages, 11)
}

fn main() -> anyhow::Result<()> {
    let out = out_dir();
    let pages = dump_files(&out)?;
    let (w, h, img) = cells_overlay(&pages);
    write_png(&out.join("cells_overlay.png"), w, h, &img)?;
    let map = synthetic_map();
    let (w, h, img) = render(&map, &pages, true, 0.5);
    write_png(&out.join("synthetic.png"), w as u32, h as u32, &img)?;
    let (w, h, img) = render(&map, &pages, true, 0.0);
    write_png(&out.join("synthetic_d3d.png"), w as u32, h as u32, &img)?;
    let (w, h, img) = render(&map, &pages, false, 0.5);
    write_png(&out.join("synthetic_unlit.png"), w as u32, h as u32, &img)?;
    println!("wrote {}", out.display());
    Ok(())
}
