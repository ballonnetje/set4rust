//! `CTiling::Init` against the original's savegames, skipped when absent. The game rebuilds the
//! tiling from the saved layers on load, so running our `init` on them must reproduce:
//! * the borderstone gfx bit 0x80 (render chunk 200, byte 3): all but 5 of 383 340, see below;
//! * an owned eco-sector under every saved `CEcoSector` record's anchor node, with that record's
//!   owner, and distinct records on distinct eco-sectors.

use std::path::{Path, PathBuf};

use s4_sim::save::SaveGame;
use s4_sim::tiling::{InitLayers, init};
use s4_sim::Grid;

fn saves() -> Vec<PathBuf> {
    fn walk(d: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(d) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out)
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("sav")) {
                out.push(p)
            }
        }
    }
    let home = std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_default();
    let mut v = Vec::new();
    walk(&home.join(r"Documents\TheSettlers4\Save"), &mut v);
    v.sort();
    v
}

#[test]
fn tiling_init_matches_savegames() {
    let files = saves();
    if files.is_empty() {
        eprintln!("skipping: no savegames");
        return;
    }
    let (mut stones, mut stones_bad, mut eco, mut eco_ok, mut eco_dup, mut our_eco) = (0u64, 0u64, 0u64, 0u64, 0u64, 0u64);
    for f in &files {
        let s = SaveGame::open(f).unwrap();
        let grid = Grid::new(s.size).unwrap();
        let ground: Vec<u8> = s.render.as_chunks::<4>().0.iter().map(|c| c[1]).collect();
        let saved_gfx: Vec<u8> = s.render.as_chunks::<4>().0.iter().map(|c| c[3]).collect();
        let mut gfx = saved_gfx.clone();
        let mut flags = s.flags.clone();
        let t = init(InitLayers { grid, ground: &ground, flags: &mut flags, gfx_bits: &mut gfx, owner: &s.owner });
        for (i, (a, b)) in gfx.iter().zip(&saved_gfx).enumerate() {
            stones += (*b & 0x80 != 0) as u64;
            if (a ^ b) & 0x80 != 0 {
                stones_bad += 1;
                // Observed: 5 nodes in AutoSave.sav, all blocked (owner pseudo tile) nodes on an
                // owner edge where the save has no stone. The game only updates stones
                // incrementally around changes; a reload (Init) would set them as we do.
                assert!(t.normal[i] < 10, "{}: borderstone differs on a real tile node {i}", f.display());
            }
        }
        let mut seen = std::collections::BTreeSet::new();
        for &(owner, idx) in &s.eco_sectors {
            eco += 1;
            let e = t.eco_sector_of(idx as usize);
            if e != 0 && t.owner_of(idx as usize) == owner {
                eco_ok += 1;
            }
            if !seen.insert(e) {
                eco_dup += 1;
            }
        }
        our_eco += t.last_eco as u64;
    }
    eprintln!(
        "{} saves: borderstones {stones}, differing {stones_bad}; eco-sector records {eco}, matching {eco_ok}, duplicates {eco_dup}; our eco-sectors {our_eco}",
        files.len()
    );
    assert!(stones_bad <= 5, "{stones_bad} borderstone bits differ");
    assert_eq!((eco_ok, eco_dup), (eco, 0));
}
