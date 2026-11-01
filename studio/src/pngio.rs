//! PNG reading and writing, and converting other image files to PNG.

use anyhow::{Context, Result};

/// Straight-alpha RGBA pixels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rgba {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>,
}

/// A PNG of any colour type as RGBA 8.
pub fn decode_rgba(bytes: &[u8]) -> Result<Rgba> {
    let mut dec = png::Decoder::new(bytes);
    dec.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = dec.read_info().context("not a PNG")?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).context("corrupt PNG")?;
    let (w, h) = (info.width, info.height);
    let n = (w as usize) * (h as usize);
    let src = &buf[..info.buffer_size()];
    let px = match info.color_type {
        png::ColorType::Rgba => src.to_vec(),
        png::ColorType::Rgb => {
            let mut px = Vec::with_capacity(n * 4);
            for [r, g, b] in src.as_chunks::<3>().0 {
                px.extend_from_slice(&[*r, *g, *b, 255]);
            }
            px
        }
        png::ColorType::GrayscaleAlpha => {
            let mut px = Vec::with_capacity(n * 4);
            for [v, a] in src.as_chunks::<2>().0 {
                px.extend_from_slice(&[*v, *v, *v, *a]);
            }
            px
        }
        png::ColorType::Grayscale => {
            let mut px = Vec::with_capacity(n * 4);
            for &v in src {
                px.extend_from_slice(&[v, v, v, 255]);
            }
            px
        }
        png::ColorType::Indexed => anyhow::bail!("indexed PNG was not expanded"),
    };
    Ok(Rgba { w, h, px })
}

/// RGBA 8 as a PNG file.
pub fn encode_rgba(w: u32, h: u32, px: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut e = png::Encoder::new(&mut out, w, h);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        let mut wr = e.write_header()?;
        wr.write_image_data(px)?;
        wr.finish()?;
    }
    Ok(out)
}

/// A frame file as stored in the pack: PNGs as they are, other images converted to PNG.
pub fn to_png(bytes: Vec<u8>, name: &str) -> Result<Vec<u8>> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Ok(bytes);
    }
    let img = image::load_from_memory(&bytes).with_context(|| format!("{name}: not an image the studio can read"))?.to_rgba8();
    encode_rgba(img.width(), img.height(), img.as_raw())
}

/// Grey value of a mask pixel: its red channel, over transparent black like a canvas draws it.
pub fn mask_value(px: &[u8], i: usize) -> u8 {
    ((px[i * 4] as u32 * px[i * 4 + 3] as u32 + 127) / 255) as u8
}

/// Team tint of a frame in place: `mix(rgb, player × luma(rgb), mask)`.
pub fn tint(frame: &mut Rgba, mask: &Rgba, player: [u8; 3]) {
    let same = mask.w == frame.w && mask.h == frame.h;
    let (fw, fh) = (frame.w as usize, frame.h as usize);
    let (mw, mh) = (mask.w as usize, mask.h as usize);
    if mw == 0 || mh == 0 {
        return;
    }
    for y in 0..fh {
        for x in 0..fw {
            let mi = if same { y * fw + x } else { (y * mh / fh) * mw + x * mw / fw };
            let t = mask_value(&mask.px, mi) as f32 / 255.0;
            if t == 0.0 {
                continue;
            }
            let o = (y * fw + x) * 4;
            let d = &mut frame.px[o..o + 3];
            let l = (0.299 * d[0] as f32 + 0.587 * d[1] as f32 + 0.114 * d[2] as f32) / 255.0;
            for c in 0..3 {
                let v = d[c] as f32 + (player[c] as f32 * l - d[c] as f32) * t;
                d[c] = v.round().clamp(0.0, 255.0) as u8;
            }
        }
    }
}

/// The 32 colours of palette indices 224..256 from a group's `team_ramps` entry.
pub fn parse_ramp(hex: &str) -> Vec<[u8; 3]> {
    let byte = |i: usize| hex.get(i..i + 2).and_then(|s| u8::from_str_radix(s, 16).ok()).unwrap_or(0);
    (0..32).map(|i| [byte(i * 6), byte(i * 6 + 2), byte(i * 6 + 4)]).collect()
}

/// A reference strip's team pixels (palette index in `mask`) in a player's colours, like the
/// game's working palette: 192..224 = player colour − 8·(223 − index) per channel, 224..256 = the
/// group's hand-made ramp.
pub fn recolour(strip: &mut Rgba, mask: &Rgba, player: [u8; 3], ramp: &[[u8; 3]]) {
    let n = strip.w.min(mask.w) as usize * strip.h.min(mask.h) as usize;
    if strip.w != mask.w || strip.h != mask.h {
        return;
    }
    for i in 0..n {
        let v = mask.px[i * 4] as usize;
        if v < 192 {
            continue;
        }
        let c = if v < 224 {
            let k = 8 * (223 - v) as i32;
            [(player[0] as i32 - k).max(0) as u8, (player[1] as i32 - k).max(0) as u8, (player[2] as i32 - k).max(0) as u8]
        } else {
            ramp[v - 224]
        };
        strip.px[i * 4..i * 4 + 3].copy_from_slice(&c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgba_round_trip() {
        let px: Vec<u8> = (0..16u8).collect();
        let png = encode_rgba(2, 2, &px).unwrap();
        assert_eq!(decode_rgba(&png).unwrap(), Rgba { w: 2, h: 2, px });
    }

    #[test]
    fn indexed_and_grey_pngs_expand() {
        let mut bytes = Vec::new();
        {
            let mut e = png::Encoder::new(&mut bytes, 2, 1);
            e.set_color(png::ColorType::Indexed);
            e.set_palette(vec![0, 0, 0, 10, 20, 30]);
            e.set_trns(vec![0, 128]);
            let mut w = e.write_header().unwrap();
            w.write_image_data(&[0, 1]).unwrap();
        }
        assert_eq!(decode_rgba(&bytes).unwrap().px, vec![0, 0, 0, 0, 10, 20, 30, 128]);
        let mut bytes = Vec::new();
        {
            let mut e = png::Encoder::new(&mut bytes, 1, 1);
            e.set_color(png::ColorType::Grayscale);
            let mut w = e.write_header().unwrap();
            w.write_image_data(&[200]).unwrap();
        }
        assert_eq!(decode_rgba(&bytes).unwrap().px, vec![200, 200, 200, 255]);
    }

    #[test]
    fn other_images_become_png() {
        let mut bmp = Vec::new();
        image::RgbaImage::from_pixel(3, 2, image::Rgba([1, 2, 3, 255])).write_to(&mut std::io::Cursor::new(&mut bmp), image::ImageFormat::Bmp).unwrap();
        let png = to_png(bmp, "a.bmp").unwrap();
        let d = decode_rgba(&png).unwrap();
        assert_eq!((d.w, d.h, &d.px[..4]), (3, 2, &[1, 2, 3, 255][..]));
        let same = encode_rgba(1, 1, &[1, 2, 3, 4]).unwrap();
        assert_eq!(to_png(same.clone(), "a.png").unwrap(), same);
        assert!(to_png(b"nope".to_vec(), "a.png").is_err());
    }

    #[test]
    fn team_tint_is_the_documented_mix() {
        let mut f = Rgba { w: 2, h: 1, px: vec![100, 200, 50, 255, 100, 200, 50, 255] };
        let m = Rgba { w: 2, h: 1, px: vec![255, 255, 255, 255, 0, 0, 0, 255] };
        tint(&mut f, &m, [200, 100, 0]);
        let l = (0.299 * 100.0 + 0.587 * 200.0 + 0.114 * 50.0) / 255.0;
        assert_eq!(&f.px[..3], &[(200.0f32 * l).round() as u8, (100.0f32 * l).round() as u8, 0]);
        assert_eq!(&f.px[4..7], &[100, 200, 50], "mask 0 leaves the pixel");
    }

    #[test]
    fn recolouring_follows_the_palette_rules() {
        let mut s = Rgba { w: 3, h: 1, px: vec![1; 12] };
        let m = Rgba { w: 3, h: 1, px: vec![0, 0, 0, 255, 223, 0, 0, 255, 225, 0, 0, 255] };
        let ramp = parse_ramp(&format!("000000{}", "0a0b0c".repeat(31)));
        assert_eq!(ramp[1], [10, 11, 12]);
        recolour(&mut s, &m, [32, 80, 228], &ramp);
        assert_eq!(&s.px[..3], &[1, 1, 1]);
        assert_eq!(&s.px[4..7], &[32, 80, 228]);
        assert_eq!(&s.px[8..11], &[10, 11, 12]);
        // Index 192 darkens by 8 · 31, floored at 0.
        let mut s = Rgba { w: 1, h: 1, px: vec![0; 4] };
        recolour(&mut s, &Rgba { w: 1, h: 1, px: vec![192, 0, 0, 255] }, [32, 80, 248], &ramp);
        assert_eq!(&s.px[..3], &[0, 0, 0]);
    }
}
