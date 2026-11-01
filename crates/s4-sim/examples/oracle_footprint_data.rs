//! Footprint oracle (`tests/sim_footprint.rs`) with building data from any `GameData` directory,
//! to test which data set a savegame was played with (e.g. a Settlers United balance config).
//! Prints totals per save group: saves with chunk 50 (Settlers United era) and without.
//! `cargo run --release -p s4-sim --example oracle_footprint_data -- GAMEDATA_DIR [--detail]`:
//! `--detail` tallies the repealing (0x02) cells we set that the save lacks, by building type,
//! cell, and what the save holds on the node.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use s4_formats::gamedata::GameData;
use s4_formats::gfx::GfxFile;
use s4_sim::entity::{BuildingData, Entity, EntityData, ObjType, flag};
use s4_sim::save::SaveGame;
use s4_sim::{Grid, Rules, World, Y16X16};

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

#[derive(Default)]
struct Tot {
    saves: u32,
    /// [0x01, 0x02, 0x08]: (set by us, also set in the save)
    fwd: [(u64, u64); 3],
    /// (race, type) -> (finished buildings, buildings missing some 0x08 cell)
    missing: BTreeMap<(u8, u16), (u32, u32)>,
}

fn main() {
    let dir = std::env::args().nth(1).expect("GameData dir");
    let data = GameData::load(&dir).expect("gamedata");
    let rules = Rules::new(&data, &GfxFile::open_default(5).expect("gfx 5"));
    let detail = std::env::args().any(|a| a == "--detail");
    let mut groups: BTreeMap<bool, Tot> = BTreeMap::new();
    // (race, type, cell) -> misses; and what the save has on missed nodes.
    let mut rep_cells: BTreeMap<(u8, u16, (i8, i8)), u32> = BTreeMap::new();
    let mut rep_what: BTreeMap<String, u32> = BTreeMap::new();
    let mut rep_examples: Vec<String> = Vec::new();
    for f in saves() {
        let mf = s4_formats::map::MapFile::open(&f).unwrap();
        let su = mf.container.load_id(50, 0).ok().flatten().is_some();
        let s = SaveGame::open(&f).unwrap();
        let t = groups.entry(su).or_default();
        t.saves += 1;
        let mut w = World::new(Grid::new(s.size).unwrap(), rules.clone());
        for (i, c) in s.render.as_chunks::<4>().0.iter().enumerate() {
            w.layers.ground[i] = c[1];
        }
        w.init_tiling(&s.owner);
        for b in &s.buildings {
            if b.kind == 31 || b.kind == 32 || b.flags & flag::READY == 0 {
                continue;
            }
            let data = BuildingData {
                type_ex: b.kind as u8,
                door: Y16X16(0),
                ensign: Y16X16(0),
                working_area: Y16X16(0),
                health: 0,
                events: Vec::new(),
            };
            w.entities.insert(b.id, Entity::new(ObjType::Building, b.kind, b.owner, b.race, b.pos, EntityData::Building(data)));
            s4_sim::create::set_to_world(&mut w, b.id);
            let r = rules.building(b.race, b.kind as u8);
            if detail {
                let walk = mf.container.load_id(209, 0).ok().flatten().map(|c| c.1).unwrap_or_default();
                for &c in &r.repealing_cells {
                    let (x, y) = (b.pos.x() + c.0 as i32, b.pos.y() + c.1 as i32);
                    let i = (x + y * s.size as i32) as usize;
                    if s.flags[i] & 0x02 != 0 {
                        continue;
                    }
                    *rep_cells.entry((b.race, b.kind, c)).or_default() += 1;
                    let mo = s.map_object[i];
                    let other = s.buildings.iter().find(|o| o.id == mo).map(|o| format!("building kind {}", o.kind));
                    let what = format!(
                        "flags {:#04x} move/walk {:?} mapobj {} deco {}",
                        s.flags[i],
                        walk.get(i),
                        other.unwrap_or_else(|| if mo != 0 { "other".into() } else { "0".into() }),
                        if s.deco_object[i] != 0 { "yes" } else { "0" }
                    );
                    *rep_what.entry(what.clone()).or_default() += 1;
                    if rep_examples.len() < 12 {
                        rep_examples.push(format!("{} b{} kind {} race {} at ({},{}) cell {:?}: {what}", f.file_name().unwrap().to_string_lossy(), b.id, b.kind, b.race, b.pos.x(), b.pos.y(), c));
                    }
                }
            }
            let miss = r.building_cells.iter().any(|c| {
                let i = (b.pos.x() + c.0 as i32 + (b.pos.y() + c.1 as i32) * s.size as i32) as usize;
                s.flags[i] & 0x08 == 0
            });
            let e = t.missing.entry((b.race, b.kind)).or_default();
            e.0 += 1;
            e.1 += miss as u32;
        }
        for i in 0..s.flags.len() {
            for (k, bit) in [0x01u8, 0x02, 0x08].into_iter().enumerate() {
                if w.layers.flags[i] & bit != 0 {
                    t.fwd[k].0 += 1;
                    t.fwd[k].1 += (s.flags[i] & bit != 0) as u64;
                }
            }
        }
    }
    if detail {
        let mut v: Vec<_> = rep_cells.iter().collect();
        v.sort_by_key(|(_, n)| std::cmp::Reverse(**n));
        println!("0x02 misses by (race, type, cell), top 40:");
        for (k, n) in v.iter().take(40) {
            println!("    {k:?}: {n}");
        }
        let mut v: Vec<_> = rep_what.iter().collect();
        v.sort_by_key(|(_, n)| std::cmp::Reverse(**n));
        println!("0x02 misses by save node state, top 30:");
        for (k, n) in v.iter().take(30) {
            println!("    {k}: {n}");
        }
        for e in &rep_examples {
            println!("    e.g. {e}");
        }
    }
    for (su, t) in &groups {
        println!(
            "{} ({} saves): ours->save 0x01 {}/{} ({} missing), 0x02 {}/{} ({} missing), 0x08 {}/{} ({} missing)",
            if *su { "with chunk 50" } else { "without chunk 50" },
            t.saves,
            t.fwd[0].1,
            t.fwd[0].0,
            t.fwd[0].0 - t.fwd[0].1,
            t.fwd[1].1,
            t.fwd[1].0,
            t.fwd[1].0 - t.fwd[1].1,
            t.fwd[2].1,
            t.fwd[2].0,
            t.fwd[2].0 - t.fwd[2].1
        );
        for (k, (n, bad)) in &t.missing {
            if *bad > 0 {
                println!("    (race, type) {k:?}: {bad}/{n} finished buildings miss a 0x08 cell");
            }
        }
    }
}
