//! Territory against the original's savegames (`Documents\TheSettlers4\Save`), skipped when absent.
//!
//! 1. Static: from the save's buildings, our influence must reproduce the saved owner layer on
//!    every land node that exactly one player influences (elsewhere ownership is history).
//! 2. Dynamic: the saved state is a fixpoint of `SetOwner` — re-running our windowed port at every
//!    inhabited tower must change no node.

use std::path::{Path, PathBuf};

use s4_sim::entity::{BuildingData, Entity, EntityData, ObjType, flag};
use s4_sim::save::SaveGame;
use s4_sim::territory::{influence_info, influence_map, set_owner};
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

/// World with the save's layers and buildings; inhabited = the role record's m_bWorkerInHouse,
/// ready, and not Dark Tribe (`FirstSettlerEnter` skips them).
fn world_from(s: &SaveGame, rules: &Rules, render: &[u8]) -> World {
    let mut w = World::new(Grid::new(s.size).unwrap(), rules.clone());
    w.layers.flags.copy_from_slice(&s.flags);
    w.layers.map_object.copy_from_slice(&s.map_object);
    for (i, c) in render.as_chunks::<4>().0.iter().enumerate() {
        w.layers.ground[i] = c[1];
        w.layers.gfx_bits[i] = c[3];
    }
    // As on savegame load: Init from the saved owner layer.
    w.init_tiling(&s.owner);
    for b in &s.buildings {
        // IBuildingRole +29 m_bWorkerInHouse from the role record, and ready (not-ready buildings
        // of other role classes can carry +29 = 1 too), docs/SIM.md §10.1.
        let inhabited = b.flags & flag::READY != 0 && b.race != 3 && s.worker_in_house(b) == Some(true);
        let data = BuildingData {
            type_ex: b.kind as u8,
            door: Y16X16(0),
            ensign: Y16X16(0),
            working_area: Y16X16(0),
            health: 0,
            events: Vec::new(),
        };
        w.entities.insert(b.id, Entity::new(ObjType::Building, b.kind, b.owner, b.race, b.pos, EntityData::Building(data)));
        s4_sim::buildings::restore_military_inhabited(&mut w, b.id, inhabited);
    }
    w
}

#[test]
fn territory_matches_savegames() {
    let files = saves();
    if files.is_empty() {
        eprintln!("skipping: no savegames");
        return;
    }
    let rules = Rules::load_default().expect("rules");
    let mut flag_diff = 0u64;
    let (mut uniq, mut uniq_ok, mut fix_nodes, mut fix_changed, mut towers) = (0u64, 0u64, 0u64, 0u64, 0u64);
    for f in &files {
        let s = SaveGame::open(f).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
        assert_eq!(s.buildings.len() as u32, s.building_count, "{}: building header scan", f.display());
        let render = s4_formats::map::MapFile::open(f).unwrap().container.load_id(200, 0).unwrap().unwrap().1;
        let mut w = world_from(&s, &rules, &render);

        let (mask, _) = influence_map(&w);
        for (i, &m) in mask.iter().enumerate() {
            if m.count_ones() == 1 && !w.layers.is_water(i) {
                uniq += 1;
                uniq_ok += (s.owner[i] as u32 == m.trailing_zeros() + 1) as u64;
            }
        }

        let tower_pos: Vec<(i32, i32)> = w
            .entities
            .iter()
            .filter(|(id, e)| matches!(&e.data, EntityData::Building(_)) && s4_sim::buildings::have_inhabitant(&w, *id) && influence_info(e.kind).0 > 0)
            .map(|(_, e)| (e.x(), e.y()))
            .collect();
        for (x, y) in tower_pos {
            towers += 1;
            fix_nodes += s4_sim::spiral::last(40) as u64 + 1;
            fix_changed += set_owner(&mut w, x, y).changed as u64;
        }
        let d: Vec<usize> = (0..s.flags.len()).filter(|&i| (w.layers.flags[i] ^ s.flags[i]) & 0x80 != 0).collect();
        if !d.is_empty() {
            let set = d.iter().filter(|&&i| w.layers.flags[i] & 0x80 != 0).count();
            let water = d.iter().filter(|&&i| w.layers.is_water(i)).count();
            eprintln!("{}: 0x80 diff {} (we set {set}, water {water})", f.file_name().unwrap().to_string_lossy(), d.len());
            // Two kinds, both observed: (a) we set 0x80 on water — only in the two saves taken right
            // after a map start, where towers only had SetTowerBits (owned land only); (b) the save
            // has 0x80 where no current tower reaches — stale bits (inferred: left by towers that
            // were dismantled), never the other way round.
            assert!(set == 0 || (set == water && set == d.len()), "{}: unexplained 0x80 differences", f.display());
        }
        flag_diff += d.len() as u64;
    }
    eprintln!(
        "{} saves: unique-influence land nodes {uniq}, owner matches {uniq_ok}; SetOwner at {towers} towers: {fix_changed} of {fix_nodes} nodes changed, flag 0x80 differs on {flag_diff}",
        files.len()
    );
    // With m_bWorkerInHouse from the role record every node matches (docs/SIM.md §10.1).
    assert_eq!(uniq_ok, uniq, "unique-influence nodes with another owner");
    // 3140 of 6.56 M (0.05 %), all of the kinds asserted per save above.
    assert!(flag_diff * 1_000 <= fix_nodes, "flag 0x80 differs on {flag_diff} nodes");
    assert_eq!(fix_changed, 0, "SetOwner changed {fix_changed} of {fix_nodes} nodes");
}
