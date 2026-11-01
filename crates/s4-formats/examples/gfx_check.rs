//! Decode every image of every sprite file and report statistics.
//! `cargo run -p s4-formats --example gfx_check --release`

use s4_formats::gfx::{self, DecodeOptions, GfxFile, PIX_SHADOW, PIX_TRANSPARENT};

fn main() -> anyhow::Result<()> {
    let dir = s4_formats::game_dir().join("Gfx");
    let mut total = 0usize;
    let mut total_err = 0usize;
    for n in gfx::list_files(&dir)? {
        let f = match GfxFile::open(&dir, n) {
            Ok(f) => f,
            Err(e) => {
                println!("{n:>2}: OPEN FAILED: {e}");
                total_err += 1;
                continue;
            }
        };
        let (mut ok, mut err, mut unterminated, mut unmapped, mut gap_mismatch) = (0, 0, 0, 0, 0);
        let (mut shadow_px, mut lit_px, mut alpha0_px, mut partial_alpha_px) = (0u64, 0u64, 0u64, 0u64);
        let mut max_w = 0;
        let mut max_h = 0;
        let mut first_err = None;
        let mut flags = std::collections::BTreeMap::<u16, usize>::new();
        let mut offs: Vec<(usize, usize)> = Vec::new();
        for i in 0..f.image_count() {
            match f.decode_indexed(i) {
                Ok(ix) => {
                    ok += 1;
                    if !ix.terminated {
                        unterminated += 1;
                    }
                    if f.layout != gfx::Layout::Flat && f.position(i).is_none() {
                        unmapped += 1;
                    }
                    *flags.entry(ix.header.flags).or_default() += 1;
                    max_w = max_w.max(ix.header.width);
                    max_h = max_h.max(ix.header.height);
                    offs.push((ix.header.data_offset, ix.stream_len));
                    let pal = f.working_palette(i, &DecodeOptions::default())?;
                    for &p in &ix.pixels {
                        match p {
                            PIX_TRANSPARENT => {}
                            PIX_SHADOW => shadow_px += 1,
                            _ => {
                                lit_px += 1;
                                let a = pal[p as usize][3];
                                if a == 0 {
                                    alpha0_px += 1;
                                } else if a < 255 {
                                    partial_alpha_px += 1;
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    err += 1;
                    first_err.get_or_insert(format!("image {i}: {e}"));
                }
            }
        }
        // Does each stream end where the next image's header begins?
        offs.sort();
        let hdr = if f.header_kind == gfx::HeaderKind::Settler { 8 } else { 12 };
        for w in offs.windows(2) {
            if w[0].0 != w[1].0 && w[0].0 + w[0].1 + hdr != w[1].0 {
                gap_mismatch += 1;
            }
        }
        total += ok;
        total_err += err;
        println!(
            "{n:>2}: v{:<3} {:?}/{:?} imgs {:>5} ok {:>5} err {err} unterm {unterminated} gapmis {gap_mismatch} unmapped {unmapped} jobs {} dirs {} groups {} pal {}x{}x{} max {max_w}x{max_h} shadow {shadow_px} lit {lit_px} a0 {alpha0_px} apart {partial_alpha_px}",
            f.version,
            f.layout,
            f.header_kind,
            f.image_count(),
            ok,
            f.job_count(),
            f.direction_count(),
            f.job_groups().len(),
            f.palette_file().entries,
            f.palette_file().levels,
            f.palette_file().format as u8,
        );
        println!("    flags {flags:x?}");
        if let Some(e) = first_err {
            println!("    first error: {e}");
        }
    }
    println!("TOTAL decoded {total}, errors {total_err}");
    Ok(())
}
