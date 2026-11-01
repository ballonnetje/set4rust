//! Placing ports and shipyards (`CheckForBuildWater` 0x53f210 from `CheckForBuild` 0x53e4e0):
//! the check picks the orientation (types 52..=63, 76..=79) and the site gets that type.
//! Singleplayer/Harbour.map has a player-2 shipyard (type 77, SHIPYARDH) at (231, 107); it is
//! crushed and its material piles' flags cleared, which frees the coast around it. No savegame
//! holds a port or shipyard, so there is no save oracle. Skipped without the game data.

use s4_sim::build::check::water_variant;
use s4_sim::entity::{EntityData, ObjType};
use s4_sim::{Rules, World};

fn harbour() -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Singleplayer").join("Harbour.map");
    if !p.exists() {
        return None;
    }
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    let rules = Rules::load_default().ok()?;
    let (w, _) = s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).ok()?;
    Some(w)
}

fn crushed_shipyard() -> Option<(World, u8)> {
    let mut w = harbour()?;
    let (id, e) = w.entities.iter().find(|(_, e)| e.obj_type == ObjType::Building && (e.x(), e.y()) == (231, 107)).expect("shipyard");
    let EntityData::Building(d) = &e.data else { panic!() };
    let (owner, ty) = (e.owner, d.type_ex);
    assert_eq!(ty, 77);
    let info = w.rules.build.info(e.race, ty).unwrap().clone();
    s4_sim::buildings::crush::crush_building(&mut w, id);
    // The crush drops half the building's material as piles onto its footprint (flag 0x02); clear
    // them so the coast is free.
    for (cx, cy) in info.cells(&info.building_lines, 231, 107) {
        let n = w.grid.index(cx as u32, cy as u32);
        w.layers.flags[n] &= !0x02;
    }
    Some((w, owner))
}

fn places(w: &World, p: u8, building: u8) -> Vec<(u16, u16, u8)> {
    s4_sim::build::possible_sites(w, p, building, (200, 80), (260, 135))
}

#[test]
fn the_marker_query_offers_shipyards_only_on_the_coast() {
    let Some((w, p)) = crushed_shipyard() else { return };
    let sites = places(&w, p, 31);
    assert!(!sites.is_empty(), "no shipyard place around the crushed one");
    for &(x, y, _) in &sites {
        let t = water_variant(&w, x as i32, y as i32, p, 31, 0).expect("marker without orientation");
        assert!(matches!(t, 58..=63 | 76 | 77));
        // The working position of the chosen orientation is open water.
        let r = w.rules.build.info(w.race_of(p), t).unwrap();
        let n = w.grid.index((x as i32 + r.working.0 as i32) as u32, (y as i32 + r.working.1 as i32) as u32);
        assert_eq!(w.tiling.water_flags[n] & 0xC300, 0xC000);
        // Mode 10 + orientation accepts the same place.
        let v = [58, 59, 60, 61, 62, 63, 76, 77].iter().position(|&s| s == t).unwrap() as i32;
        assert_eq!(water_variant(&w, x as i32, y as i32, p, 31, 10 + v), Some(t));
    }
    // Not the player's land: nothing fits.
    let other = if p == 1 { 2 } else { 1 };
    assert!(sites.iter().all(|&(x, y, _)| water_variant(&w, x as i32, y as i32, other, 31, 0).is_none()));
    eprintln!("shipyard places {sites:?}, port places {:?}", places(&w, p, 32));
}

/// Command 5025 with type 31 builds a site of the orientation the check picked; the site's state
/// 4 (0x541f1b) marks the water cells (+0x314) with flag 0x08 and water bit 0x1000, and the
/// shipyard's `workingpos` with 0x1000.
#[test]
fn a_shipyard_command_makes_a_site_of_the_orientation() {
    let Some((mut w, p)) = crushed_shipyard() else { return };
    let &(x, y, _) = places(&w, p, 31).first().expect("no place");
    let t = water_variant(&w, x as i32, y as i32, p, 31, 0).unwrap();
    let before: Vec<u16> = w.entities.iter().map(|(id, _)| id).collect();
    s4_sim::build::command_build(&mut w, p, 31, x, y);
    let (id, e) = w.entities.iter().find(|(id, e)| !before.contains(id) && e.obj_type == ObjType::Building).expect("no site");
    let EntityData::Building(d) = &e.data else { panic!() };
    assert_eq!((e.x(), e.y(), d.type_ex), (x as i32, y as i32, t));
    // The CBuilding ctor (0x535b40) keeps the base type at +0xc.
    assert_eq!(e.kind, 31);
    // Skip the digging: state 4 takes the water and, with nobody in the way, goes to 5
    // (`SetToWorld`, which blocks the +0x314 water).
    let rule = w.rules.building(w.race_of(p), t).clone();
    w.sites.roles.get_mut(&id).unwrap().state = 4;
    s4_sim::build::building_logic_update(&mut w, id);
    assert_eq!(w.sites.roles[&id].state, 5);
    let (x, y) = (x as i32, y as i32);
    let at = |w: &World, (dx, dy): (i8, i8)| w.grid.index((x + dx as i32) as u32, (y + dy as i32) as u32);
    for &c in &rule.water_block_cells {
        let n = at(&w, c);
        assert_ne!(w.layers.flags[n] & 0x08, 0, "cell {c:?} without flag 0x08");
        if w.layers.flags[n] & 1 != 0 && w.tiling.water_flags[n] >= 0xC000 {
            assert_ne!(w.tiling.water_flags[n] & 0x1000, 0, "water cell {c:?} without bit 0x1000");
        }
    }
    assert_ne!(w.tiling.water_flags[at(&w, rule.working)] & 0x1000, 0, "workingpos without bit 0x1000");
}

/// Test vectors for the release's `CheckForBuildWater` run in unicorn
/// (`tools/re/script/water_emu.py`): per map, the layers the check reads, the races, the port /
/// shipyard records and our answer for every land node near water. Land near water is handed to
/// players 1..3 in 40-node squares first, so the territory tests see borders. Writes
/// `out/script/water_vec/<map>.json`; run with `--ignored`.
#[test]
#[ignore]
fn dump_water_vectors() {
    let maps = ["Singleplayer/Harbour.map", "Singleplayer/Harbour Island.map", "Singleplayer/MiddleIslands.map", "Tutorial/Tutorial10.map", "Singleplayer/CoastofDeath.map"];
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/script/water_vec");
    std::fs::create_dir_all(&out).unwrap();
    for m in maps {
        let p = s4_formats::game_dir().join("Map").join(m);
        let Ok(map) = s4_formats::map::MapFile::open(&p) else { continue };
        let (mut w, _) = s4_sim::new_game(&map, Rules::load_default().unwrap(), &s4_sim::NewGame::default()).unwrap();
        let size = w.grid.size() as i32;
        // Distance to water ≤ 24 (square), land only.
        let water: Vec<bool> = (0..(size * size) as usize).map(|i| w.layers.ground[i] < 16).collect();
        let mut near = vec![false; water.len()];
        for y in 0..size {
            for x in 0..size {
                if !water[(y * size + x) as usize] {
                    continue;
                }
                for yy in (y - 24).max(0)..=(y + 24).min(size - 1) {
                    for xx in (x - 24).max(0)..=(x + 24).min(size - 1) {
                        near[(yy * size + xx) as usize] = true;
                    }
                }
            }
        }
        for y in 1..size - 1 {
            for x in 1..size - 1 {
                let i = (y * size + x) as usize;
                if near[i] && !water[i] && w.owner_of(i) == 0 {
                    w.change_owner(x, y, (1 + (x / 40 + y / 40) % 3) as u8);
                }
            }
        }
        let n = (size * size) as usize;
        let hex = |b: &[u8]| b.iter().map(|v| format!("{v:02x}")).collect::<String>();
        let owner: Vec<u8> = (0..n).map(|i| w.owner_of(i)).collect();
        let wf: Vec<u8> = w.tiling.water_flags.iter().flat_map(|v| v.to_le_bytes()).collect();
        let races: Vec<u8> = (0..9).map(|p| w.race_of(p)).collect();
        let mut infos = Vec::new();
        for &r in &races {
            for t in [52u8, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 76, 77, 78, 79] {
                let Some(i) = w.rules.build.info(r, t) else { continue };
                let lines = |v: &[u32]| format!("[{}]", v.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(","));
                infos.push(format!(
                    "{{\"race\":{r},\"type\":{t},\"hot\":[{},{}],\"working\":[{},{}],\"bounds\":[{},{},{},{}],\"bld\":{},\"water\":{},\"wblock\":{},\"wfree\":{}}}",
                    i.hotspot.0, i.hotspot.1, i.working.0, i.working.1, i.bounds[0], i.bounds[1], i.bounds[2], i.bounds[3],
                    lines(&i.building_lines), lines(&i.water_lines), lines(&i.water_block_lines), lines(&i.water_free_lines)
                ));
            }
        }
        let mut queries = Vec::new();
        for y in 16..size - 16 {
            for x in 16..size - 16 {
                let i = (y * size + x) as usize;
                if !near[i] || water[i] {
                    continue;
                }
                for p in 1..=3u8 {
                    for b in [31u8, 32] {
                        // Every node of the right owner, and every 7th of another (the owner test).
                        if owner[i] == p || (x + y) % 7 == 0 {
                            let r = water_variant(&w, x, y, p, b, 0).unwrap_or(0);
                            queries.push(format!("[{x},{y},{p},{b},{r}]"));
                        }
                    }
                }
            }
        }
        let json = format!(
            "{{\"size\":{size},\"ground\":\"{}\",\"flags\":\"{}\",\"owner\":\"{}\",\"wf\":\"{}\",\"races\":{:?},\"infos\":[{}],\"queries\":[{}]}}",
            hex(&w.layers.ground),
            hex(&w.layers.flags),
            hex(&owner),
            hex(&wf),
            races,
            infos.join(","),
            queries.join(",")
        );
        let name = m.rsplit('/').next().unwrap().replace(' ', "_").replace(".map", ".json");
        std::fs::write(out.join(&name), json).unwrap();
        let hits = queries.iter().filter(|q| !q.ends_with(",0]")).count();
        eprintln!("{m}: {} queries, {hits} placeable", queries.len());
    }
}
