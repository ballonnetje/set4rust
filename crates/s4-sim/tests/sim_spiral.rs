//! `spiral.bin` equals the `CSpiralOffsets` table in the shipped exe (file offset 0xd67f68).

#[test]
fn spiral_table_matches_exe() {
    let p = s4_formats::game_dir().join("S4_Main.exe");
    let Ok(exe) = std::fs::read(&p) else {
        eprintln!("skipping: {} not found", p.display());
        return;
    };
    let base = 0xd67f68;
    for i in 0..s4_sim::spiral::LEN {
        let b = &exe[base + 4 * i..base + 4 * i + 4];
        assert_eq!((b[0] as i8, b[1] as i8, b[2], b[3]), s4_sim::spiral::entry(i), "entry {i}");
    }

}

/// `spiral::first` / `last` equal the `{first, last}` table in the shipped exe (file offset
/// 0xd7b538, VA 0x117cf38, read by the `CSpiralWalk` ctor at 0x431210).
#[test]
fn spiral_rings_match_exe() {
    let p = s4_formats::game_dir().join("S4_Main.exe");
    let Ok(exe) = std::fs::read(&p) else { return };
    let base = 0xd7b538;
    for r in 0..=75u8 {
        let b = &exe[base + 4 * r as usize..base + 4 * r as usize + 4];
        let (f, l) = (u16::from_le_bytes([b[0], b[1]]) as usize, u16::from_le_bytes([b[2], b[3]]) as usize);
        assert_eq!((s4_sim::spiral::first(r), s4_sim::spiral::last(r)), (f, l), "radius {r}");
    }
}

/// `tiling::hex_points` equals `g_sSurroundingHexPoints8` (721 hex-ring entries, .rdata file
/// offset 0xc1b1b8, `{i8 x, i8 y, u8 ring, 0}`).
#[test]
fn hex_points_match_exe() {
    let p = s4_formats::game_dir().join("S4_Main.exe");
    let Ok(exe) = std::fs::read(&p) else { return };
    let base = 0xc1b1b8;
    let pts = s4_sim::tiling::hex_points(15);
    assert_eq!(pts.len(), 721);
    for (i, &(x, y)) in pts.iter().enumerate() {
        let b = &exe[base + 4 * i..base + 4 * i + 4];
        assert_eq!((b[0] as i8 as i32, b[1] as i8 as i32), (x, y), "entry {i}");
    }
}

/// `buildings::search::HEX_RADIUS2` (the radius-2 part of the offsets 0x5afb90 builds at
/// 0x1454148) equals the first 19 entries of `g_sSurroundingHexPoints8`.
#[test]
fn search_hex_radius2_matches_exe() {
    let p = s4_formats::game_dir().join("S4_Main.exe");
    let Ok(exe) = std::fs::read(&p) else { return };
    let base = 0xc1b1b8;
    for (k, &(dx, dy)) in s4_sim::buildings::search::HEX_RADIUS2.iter().enumerate() {
        assert_eq!((exe[base + 4 * k] as i8, exe[base + 4 * k + 1] as i8), (dx, dy), "entry {k}");
    }
    let pts = s4_sim::tiling::hex_points(2);
    assert_eq!(pts, s4_sim::buildings::search::HEX_RADIUS2.iter().map(|&(x, y)| (x as i32, y as i32)).collect::<Vec<_>>());
}
