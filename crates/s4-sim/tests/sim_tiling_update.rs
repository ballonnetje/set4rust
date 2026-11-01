//! Incremental tiling updates against a full rebuild. After random blocking / unblocking /
//! owner changes on a real map, `CTiling::Init` on the resulting layers must give the same
//! structure up to renumbering: the same tiles (node partition, type, size, links), sectors and
//! eco-sectors, and the same borderstones. Skipped when the install is absent.

use std::collections::{BTreeMap, BTreeSet};

use s4_formats::map::MapFile;
use s4_sim::rng::Random16;
use s4_sim::tiling::{Ctx, InitLayers, Tiling, init, init_with};
use s4_sim::{NewGame, Rules, World, new_game};

fn fresh(w: &World) -> (Tiling, Vec<u8>) {
    let mut flags = w.layers.flags.clone();
    let mut gfx = w.layers.gfx_bits.clone();
    let owner: Vec<u8> = (0..w.grid.len()).map(|i| w.tiling.owner_of(i)).collect();
    let l = InitLayers { grid: w.grid, ground: &w.layers.ground, flags: &mut flags, gfx_bits: &mut gfx, owner: &owner };
    // Water flags are only computed by Init in the game; keep the incremental world's.
    let t = init_with(l, Some(&w.tiling.water_flags));
    (t, gfx)
}

/// Bijection check between two labellings of the same nodes; returns mismatches.
fn bijective(a: impl Iterator<Item = (u32, u32)>) -> usize {
    let mut fwd: BTreeMap<u32, u32> = BTreeMap::new();
    let mut back: BTreeMap<u32, u32> = BTreeMap::new();
    let mut bad = 0;
    for (x, y) in a {
        if *fwd.entry(x).or_insert(y) != y || *back.entry(y).or_insert(x) != x {
            bad += 1;
        }
    }
    bad
}

fn compare(w: &World, step: usize) {
    let (f, gfx) = fresh(w);
    let inc = &w.tiling;
    let n = w.grid.len();
    let real = |t: &Tiling, i: usize, cat: bool| {
        let id = t.ids(cat)[i];
        (id >= 10).then_some(id)
    };
    for cat in [false, true] {
        let pairs: Vec<(u32, u32)> = (0..n)
            .filter_map(|i| match (real(inc, i, cat), real(&f, i, cat)) {
                (Some(a), Some(b)) => Some((a as u32, b as u32)),
                (None, None) => None,
                (a, b) => panic!(
                    "step {step}: node {i} ({}, {}) real tile {a:?} vs fresh {b:?} (catapult {cat}); ground {} flags {:#x} water {:#x}/{:#x} type {:#x}",
                    i as u32 % w.grid.size(), i as u32 / w.grid.size(), w.layers.ground[i], w.layers.flags[i], inc.water_flags[i], f.water_flags[i],
                    a.map_or(0, |a| inc.tile(a).ty)
                ),
            })
            .collect();
        assert_eq!(bijective(pairs.iter().copied()), 0, "step {step}: tile partition differs (catapult {cat})");
        let map: BTreeMap<u32, u32> = pairs.iter().copied().collect();
        for (&a, &b) in &map {
            let (ta, tb) = (inc.tile(a as u16), f.tile(b as u16));
            assert_eq!(ta.ty, tb.ty, "step {step}: tile {a}/{b} type/size");
            let la: BTreeSet<u32> = ta.links.iter().map(|&l| map.get(&(l as u32)).copied().unwrap_or(u32::MAX)).collect();
            let lb: BTreeSet<u32> = tb.links.iter().map(|&l| l as u32).collect();
            if !cat {
                assert_eq!(la, lb, "step {step}: links of tile {a}/{b}");
            }
        }
    }
    let sec = (0..n).filter_map(|i| real(inc, i, false).map(|_| (inc.sector_of(i) as u32, f.sector_of(i) as u32)));
    assert_eq!(bijective(sec), 0, "step {step}: sector partition differs");
    let eco = (0..n).filter(|&i| inc.owner_of(i) != 0 && real(inc, i, false).is_some()).map(|i| (inc.eco_sector_of(i) as u32, f.eco_sector_of(i) as u32));
    assert_eq!(bijective(eco), 0, "step {step}: eco-sector partition differs");
    // Blocked nodes are excluded: BlockedLandSurroundingCheck re-owns enclosed blocked land
    // without recomputing those nodes' stones (the game leaves them stale until a reload; the
    // savegame oracle shows the same on blocked nodes of AutoSave.sav).
    let stones = (0..n).filter(|&i| w.layers.flags[i] & 1 == 0 && (gfx[i] ^ w.layers.gfx_bits[i]) & 0x80 != 0).count();
    assert_eq!(stones, 0, "step {step}: borderstones differ");
}

#[test]
fn incremental_updates_match_rebuild() {
    let p = s4_formats::game_dir().join(r"Map\Campaign\AO_maya1.map");
    if !p.exists() {
        eprintln!("skipping: {} not found", p.display());
        return;
    }
    let map = MapFile::open(p).unwrap();
    let (mut w, _) = new_game(&map, Rules::load_default().unwrap(), &NewGame::default()).unwrap();
    // Start from a consistent tiling of the loaded state (footprints were stamped after Init).
    let owner: Vec<u8> = (0..w.grid.len()).map(|i| w.tiling.owner_of(i)).collect();
    w.tiling = init(InitLayers { grid: w.grid, ground: &w.layers.ground, flags: &mut w.layers.flags, gfx_bits: &mut w.layers.gfx_bits, owner: &owner });
    compare(&w, 0);
    let size = w.grid.size() as i32;
    let mut counts = [0u32; 3];
    let mut effective = 0u32;
    let mut shore_windows = 0;
    // Several 48x48 windows (some on the shore) so operations interact with water and borders.
    for seed in 1..=8u32 {
        let mut r = Random16::new(seed, 0);
        let (x0, y0) = (16 + (r.rand() % (size as u32 - 80)) as i32, 16 + (r.rand() % (size as u32 - 80)) as i32);
        let has_water = (0..48).any(|j| (0..48).any(|i| w.layers.ground[w.grid.index((x0 + i) as u32, (y0 + j) as u32)] <= 8));
        shore_windows += has_water as u32;
        for step in 1..=1500 {
            let x = x0 + (r.rand() % 48) as i32;
            let y = y0 + (r.rand() % 48) as i32;
            let op = r.rand() % 3;
            let i = w.grid.index(x as u32, y as u32);
            let before = (w.layers.flags[i], w.tiling.owner_of(i));
            let mut ctx = Ctx { grid: w.grid, ground: &w.layers.ground, flags: &mut w.layers.flags, gfx_bits: &mut w.layers.gfx_bits, map_object: &w.layers.map_object, deco_object: &w.layers.deco_object };
            match op {
                0 => w.tiling.set_blocked_land(&mut ctx, x, y),
                1 => w.tiling.clear_blocked_land(&mut ctx, x, y),
                _ => {
                    let owner = (r.rand() % 4) as u8;
                    w.tiling.change_owner(&mut ctx, x, y, owner)
                }
            }
            counts[op as usize] += 1;
            effective += (before != (w.layers.flags[i], w.tiling.owner_of(i))) as u32;
            if step % 300 == 0 {
                compare(&w, seed as usize * 10_000 + step);
            }
        }
    }
    eprintln!("{shore_windows} of 8 windows touch water");
    eprintln!("ops {counts:?}, {effective} changed a node: incremental tiling matches a rebuild");
    assert!(effective > 1000, "too few effective operations: {effective}");
}
