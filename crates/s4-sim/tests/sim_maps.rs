//! Every shipped map (`Map\**\*.map`, `*.edm`) loads, and the entities created match the chunk
//! record counts minus what the original loaders skip. Skipped when the install is absent.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use s4_formats::map::MapFile;
use s4_sim::create::BuildingSkip;
use s4_sim::{NewGame, ObjType, Rules, new_game};

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("map") || x.eq_ignore_ascii_case("edm")) {
            out.push(p);
        }
    }
}

#[test]
fn all_maps_load_with_consistent_entity_counts() {
    let root = s4_formats::game_dir().join("Map");
    if !root.exists() {
        eprintln!("skipping: {} not found", root.display());
        return;
    }
    let rules = Rules::load_default().expect("rules");
    let mut maps = Vec::new();
    collect(&root, &mut maps);
    maps.sort();
    assert_eq!(maps.len(), 378, "expected the 378 shipped maps");

    let mut skips: BTreeMap<String, u32> = BTreeMap::new();
    let (mut requested, mut medics, mut placed, mut max_pool) = (0u32, 0u32, 0u32, 0u8);
    for path in &maps {
        let name = path.strip_prefix(&root).unwrap().display().to_string();
        let map = MapFile::open(path).unwrap_or_else(|e| panic!("{name}: {e}"));
        let (w, r) = new_game(&map, rules.clone(), &NewGame::default()).unwrap_or_else(|e| panic!("{name}: {e}"));

        // Deco: every non-ignored object node either got an entity (new or shared) or a reason.
        assert_eq!(r.deco_entities as usize, w.entities.count(ObjType::Deco), "{name}");
        let placed_nodes = w.layers.deco_object.iter().filter(|&&d| d != 0).count();
        let deco_nodes = r.object_nodes - r.objects_ignored - r.objects_skipped.len() as u32;
        // Piles overwrite the deco slot of their node, so placed nodes can only be fewer. That
        // includes the piles building roles create in their Init (a gather building's output, a
        // market's or port's trade piles, which its trading state keeps).
        let role_piles: usize = w
            .buildings
            .roles
            .values()
            .map(|r| {
                r.base.piles.len()
                    + match &r.kind {
                        s4_sim::buildings::RoleKind::Trading(t) => t.piles.len(),
                        _ => 0,
                    }
            })
            .sum();
        assert!(placed_nodes as u32 <= deco_nodes + r.piles + role_piles as u32, "{name}");

        // Buildings: records = type 0 + skipped + created; a second manakopter hall is created and
        // killed (it stays in the arena until its destroy slot comes round).
        let halls = r.buildings_skipped.iter().filter(|s| s.1 == BuildingSkip::SecondManakopterHall).count();
        assert_eq!(r.building_records, r.buildings_type0 + r.buildings_skipped.len() as u32 + r.buildings, "{name}");
        assert_eq!(w.entities.count(ObjType::Building), r.buildings as usize + halls, "{name}");

        // Settlers + vehicles: records = settlers + vehicles + ignored (no settler cap hit).
        assert_eq!(r.settler_records, r.settlers + r.settlers_ignored + r.vehicles + r.vehicles_skipped, "{name}");
        // Every military building's role Init adds its door settler (type 69, `CSettlerMgr`
        // 0x582810); the start garrison does not replace it (loading: only the tower bits).
        let doors = w.entities.iter().filter(|(_, e)| e.obj_type == ObjType::Settler && e.kind == s4_sim::create::DOOR_SETTLER).count();
        let military = w.buildings.roles.values().filter(|r| matches!(r.kind, s4_sim::buildings::RoleKind::Military(_))).count();
        assert_eq!(doors, military, "{name}");
        assert_eq!(w.entities.count(ObjType::Settler), (r.settlers + r.soldiers) as usize + doors, "{name}");
        assert_eq!(w.entities.count(ObjType::Ship) + w.entities.count(ObjType::LandVehicle), r.vehicles as usize, "{name}");

        // Piles: none skipped (cap 20000 not reached); plus the building roles' own piles.
        assert_eq!((r.piles, w.entities.count(ObjType::Pile)), (r.pile_records, r.pile_records as usize + role_piles), "{name}");

        // Everything plus the six placeholders.
        let total = 6 + r.deco_entities + r.buildings + halls as u32 + r.settlers + r.soldiers + doors as u32 + r.vehicles + r.piles + role_piles as u32;
        assert_eq!(w.entities.len(), total as usize, "{name}");

        // One RNG draw per new static tree/plant/mushroom instance and per settler (direction).
        assert!(w.rng.calls >= r.settlers + r.soldiers, "{name}");

        for (t, s) in &r.objects_skipped {
            *skips.entry(format!("object {t}: {s:?}")).or_default() += 1;
        }
        for (t, s) in &r.buildings_skipped {
            *skips.entry(format!("building {t}: {s:?}")).or_default() += 1;
        }
        if r.buildings_type0 > 0 {
            *skips.entry("building type 0".into()).or_default() += r.buildings_type0;
        }
        if r.settlers_ignored > 0 {
            *skips.entry("settler type 67..199".into()).or_default() += r.settlers_ignored;
        }
        if r.vehicles_skipped > 0 {
            *skips.entry("vehicle without class".into()).or_default() += r.vehicles_skipped;
        }
        if r.objects_ignored > 0 {
            *skips.entry("object 162/209/210/211".into()).or_default() += r.objects_ignored;
        }
        requested += r.soldiers_requested;
        medics += r.medics_requested;
        placed += r.soldiers;
        max_pool = max_pool.max(w.deco_pools.pools.iter().map(|p| p.count).max().unwrap_or(0));
    }
    eprintln!("maps: {}, soldiers requested {requested} (medics {medics}), placed {placed}, largest deco pool {max_pool}", maps.len());
    for (k, v) in &skips {
        eprintln!("  skipped {k}: {v}");
    }
}
