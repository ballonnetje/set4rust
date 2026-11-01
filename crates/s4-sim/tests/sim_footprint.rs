//! Building footprints (`CBuilding::SetToWorld`) against the original's savegames, skipped when
//! absent. Each save is judged with the building data it was played with (docs/SIM.md §10.2):
//! the two vanilla saves with the installed `GameData`, the Settlers United saves (chunk 50) with
//! the mod's `usd` data if `S4_USD_GAMEDATA` points at it (e.g. an extracted
//! `BalanceConfigs.zip/usd`), otherwise not at all. For every finished building in a save, stamp it with our `set_to_world` into an empty
//! world and compare the bits it sets with the save's flag layer (chunk 205):
//! * forward: every bit we set (0x08 footprint, 0x01 blocked, 0x02 repelled) is set in the save;
//! * backward: every save node with 0x08 is covered by some building's footprint.

use std::path::{Path, PathBuf};

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

/// `[0x01, 0x02, 0x08]`: (set by us, also set in the save); backward: (save 0x08 land nodes,
/// covered by finished footprints, covered by construction sites).
#[derive(Default)]
struct Tally {
    saves: u32,
    fwd: [(u64, u64); 3],
    back: (u64, u64, u64),
}

fn check(s: &SaveGame, rules: &Rules, t: &mut Tally) {
    t.saves += 1;
    let mut w = World::new(Grid::new(s.size).unwrap(), rules.clone());
    let mut sites = World::new(Grid::new(s.size).unwrap(), rules.clone());
    for (i, c) in s.render.as_chunks::<4>().0.iter().enumerate() {
        w.layers.ground[i] = c[1];
        sites.layers.ground[i] = c[1];
    }
    // Empty flags, the save's owners: the tiling (Init) then receives the footprints.
    w.init_tiling(&s.owner);
    sites.init_tiling(&s.owner);
    for b in &s.buildings {
        // Ports and shipyards are saved with m_nType 32 / 31 and their race variant elsewhere in
        // the record; without the variant their masks are unknown, so skip them.
        if b.kind == 31 || b.kind == 32 {
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
        let target = if b.flags & flag::READY != 0 { &mut w } else { &mut sites };
        target.entities.insert(b.id, Entity::new(ObjType::Building, b.kind, b.owner, b.race, b.pos, EntityData::Building(data)));
        s4_sim::create::set_to_world(target, b.id);
    }
    for i in 0..s.flags.len() {
        for (k, bit) in [0x01u8, 0x02, 0x08].into_iter().enumerate() {
            if w.layers.flags[i] & bit != 0 {
                t.fwd[k].0 += 1;
                t.fwd[k].1 += (s.flags[i] & bit != 0) as u64;
            }
        }
        if s.flags[i] & 0x08 != 0 && !w.layers.is_water(i) {
            t.back.0 += 1;
            t.back.1 += (w.layers.flags[i] & 0x08 != 0) as u64;
            t.back.2 += (w.layers.flags[i] & 0x08 == 0 && sites.layers.flags[i] & 0x08 != 0) as u64;
        }
    }
}

fn assert_tally(name: &str, t: &Tally) {
    let f = t.fwd;
    eprintln!(
        "{name}: {} saves; ours->save 0x01 {}/{}, 0x02 {}/{}, 0x08 {}/{}; save 0x08 nodes {}: finished {}, sites {}",
        t.saves, f[0].1, f[0].0, f[1].1, f[1].0, f[2].1, f[2].0, t.back.0, t.back.1, t.back.2
    );
    assert_eq!(f[0].1, f[0].0, "{name}: blocked (0x01) cells missing in the save");
    assert_eq!(f[2].1, f[2].0, "{name}: footprint (0x08) cells missing in the save");
    // Open: ~0.1-0.3 % of the repelled (0x02) cells we set are clear in the save, with either data
    // set and in the vanilla saves too — not a data question (docs/SIM.md §10.3).
    assert!(f[1].1 * 1000 >= f[1].0 * 995, "{name}: 0x02 {}/{}", f[1].1, f[1].0);
    // Backward: save 0x08 land nodes are covered by finished footprints or sites (sites stamp
    // 0x08 themselves); the rest is ports/shipyards, skipped above.
    assert!((t.back.1 + t.back.2) * 1000 >= t.back.0 * 999, "{name}: backward {:?}", t.back);
}

#[test]
fn footprints_match_savegames() {
    let files = saves();
    if files.is_empty() {
        eprintln!("skipping: no savegames");
        return;
    }
    let vanilla = Rules::load_default().expect("rules");
    let usd = std::env::var_os("S4_USD_GAMEDATA").map(|d| {
        let data = s4_formats::gamedata::GameData::load(d).expect("usd GameData");
        Rules::new(&data, &s4_formats::gfx::GfxFile::open_default(5).expect("gfx 5"))
    });
    let (mut tv, mut tu, mut skipped) = (Tally::default(), Tally::default(), 0);
    for f in &files {
        // Without the mod's data the Settlers United saves (chunk 50) are skipped: look at the
        // chunk list first instead of parsing the whole save.
        if usd.is_none()
            && s4_formats::map::MapFile::open(f).is_ok_and(|m| m.container.chunks.iter().any(|h| h.kind == 50))
        {
            skipped += 1;
            continue;
        }
        let s = SaveGame::open(f).unwrap();
        match (&usd, s.settlers_united) {
            (_, false) => check(&s, &vanilla, &mut tv),
            (Some(r), true) => check(&s, r, &mut tu),
            (None, true) => skipped += 1,
        }
    }
    assert!(tv.saves > 0, "no vanilla save found");
    assert_tally("vanilla data, vanilla saves", &tv);
    if usd.is_some() {
        assert_tally("usd data, Settlers United saves", &tu);
    } else {
        eprintln!("{skipped} Settlers United saves skipped (set S4_USD_GAMEDATA to check them)");
    }
}
