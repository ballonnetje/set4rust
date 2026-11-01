//! Checks against the real install; skipped when the game data is not present.

use s4_formats::gfx::{GfxFile, HeaderKind, Layout};

fn gfx_dir() -> Option<std::path::PathBuf> {
    let d = s4_formats::game_dir().join("Gfx");
    d.join("20.gfx").exists().then_some(d)
}

#[test]
fn roman_settlers_structure() {
    let Some(dir) = gfx_dir() else { return };
    let f = GfxFile::open(&dir, 20).unwrap();
    assert_eq!(f.header_kind, HeaderKind::Settler);
    assert_eq!(f.layout, Layout::Jobs);
    assert_eq!(f.direction_count(), 6);
    assert_eq!(f.job_count(), 367); // SIV_MAX_JOB
    assert!(f.has_team_colour());
    // Every direction of a real job has frames, and every frame decodes.
    let job = f.job_groups()[1] as usize;
    for d in 0..6 {
        let frames = f.job_frames(job, d).unwrap();
        assert!(!frames.is_empty());
        for i in frames {
            let img = f.decode(i).unwrap();
            assert_eq!(img.rgba.len(), (img.width * img.height * 4) as usize);
        }
    }
}

#[test]
fn rle_streams_end_at_next_image() {
    let Some(dir) = gfx_dir() else { return };
    let f = GfxFile::open(&dir, 10).unwrap();
    assert_eq!(f.layout, Layout::Buildings);
    for i in 0..f.image_count() - 1 {
        let a = f.decode_indexed(i).unwrap();
        let next = f.header(i + 1).unwrap().data_offset - 12;
        assert!(a.terminated);
        assert_eq!(a.header.data_offset + a.stream_len, next, "image {i}");
    }
}

#[test]
fn gui_raw_backgrounds() {
    let Some(dir) = gfx_dir() else { return };
    let f = GfxFile::open(&dir, 0).unwrap();
    let raw = (0..f.image_count()).find(|&i| f.header(i).unwrap().is_raw()).unwrap();
    let ix = f.decode_indexed(raw).unwrap();
    assert!(ix.raw);
    assert_eq!(ix.stream_len, ix.pixels.len());
}
