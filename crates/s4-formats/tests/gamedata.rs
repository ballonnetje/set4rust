//! GameData readers and the gamedata → gfx resolution, against the real install (skipped if absent).

use s4_formats::gamedata::resolve::{self, BuildingPart};
use s4_formats::gamedata::*;
use s4_formats::gfx::GfxFile;

fn dirs() -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    let g = s4_formats::game_dir();
    g.join("GameData/buildingInfo.xml").exists().then(|| (g.join("GameData"), g.join("Gfx")))
}

#[test]
fn all_files_parse() {
    let Some((gd_dir, _)) = dirs() else { return };
    let gd = GameData::load(gd_dir).unwrap();
    assert_eq!(gd.buildings.len(), 271);
    assert_eq!(gd.triggers.len(), 153);
    assert_eq!(gd.jobs.len(), 401);
    assert_eq!(gd.jobs.iter().map(|j| j.nodes.len()).sum::<usize>(), 2630);
    assert_eq!(gd.objects.len(), 241); // 246 in the file, 5 ids unknown to the exe
    assert_eq!(gd.settlers.len(), 89);
    assert_eq!(gd.vehicles.len(), 20);
    // Every job node's jobPart is a known settler gfx job.
    assert!(gd.jobs.iter().flat_map(|j| &j.nodes).all(|n| n.job_part.is_some()));
}

#[test]
fn gfx_job_names_line_up() {
    let Some((_, gfx)) = dirs() else { return };
    for race in Race::ALL.iter().copied() {
        let b = GfxFile::open(&gfx, resolve::building_file(race)).unwrap();
        for &bld in &Building::ALL[1..] {
            let d = b.job_dirs(bld as usize).unwrap().start;
            assert!(BUILDING_JOB_NAMES[d].starts_with(&format!("{}_", bld.name())), "{race:?} {bld:?}");
            assert_eq!(BUILDING_JOB_NAMES[d + 1], format!("{}_STANDARD", bld.name()));
        }
        let s = GfxFile::open(&gfx, resolve::settler_file(race)).unwrap();
        assert_eq!(s.job_count(), SETTLER_JOB_NAMES.len());
        let carrier = resolve::settler_job(&s, Settler::CARRIER).unwrap();
        assert_eq!(SETTLER_JOB_NAMES[carrier], "C_WALK");
    }
    let o = GfxFile::open(&gfx, resolve::OBJECT_FILE).unwrap();
    for &obj in &Object::ALL[1..] {
        let j = resolve::first_job(&o, obj as u16).unwrap();
        assert!(OBJECT_JOB_NAMES[j].starts_with(obj.name()), "{obj:?}");
    }
    let v = GfxFile::open(&gfx, 30).unwrap();
    assert_eq!(VEHICLE_JOB_NAMES[resolve::first_job(&v, Vehicle::WARSHIP as u16).unwrap()], "VEHICLE_WARSHIP_MOVE");
}

#[test]
fn buildings_resolve_to_images() {
    let Some((gd_dir, gfx)) = dirs() else { return };
    let gd = GameData::load(gd_dir).unwrap();
    for race in Race::ALL.iter().copied() {
        let f = GfxFile::open(&gfx, resolve::building_file(race)).unwrap();
        for b in gd.buildings.iter().filter(|b| b.race == race) {
            let r = resolve::building_frames(&f, b.building, BuildingPart::Finished).unwrap();
            f.decode(r.start).unwrap();
            for p in &b.patches {
                if let Some(d) = p.job {
                    assert!(BUILDING_JOB_NAMES[d as usize] == p.job_name);
                }
            }
        }
    }
}

#[test]
fn enums_round_trip() {
    for &b in Building::ALL {
        assert_eq!(Building::from_name(b.name()), Some(b));
        assert_eq!(Building::from_u16(b as u16), Some(b));
        assert_eq!(define_value(b.name()), Some(b as i32));
    }
    assert_eq!(define_value("JOB_WOODCUTTER_WORK"), Some(AnimList::WOODCUTTER_WORK as i32));
    assert_eq!(define_value("C_WALK"), Some(1));
}

#[test]
fn settler_wait_lists() {
    let Some((_, gfx)) = dirs() else { return };
    let roman = GfxFile::open(&gfx, resolve::settler_file(Race::ROMAN)).unwrap();
    let (mut lists, mut own_differs, mut empty) = (0, 0, Vec::new());
    for race in Race::ALL.iter().copied() {
        let f = GfxFile::open(&gfx, resolve::settler_file(race)).unwrap();
        for k in 1..=66u16 {
            let Some(w) = resolve::settler_wait_job(&roman, &f, k) else { continue };
            lists += 1;
            // A zero frame count is the constructor's debug assert ("SettlerJobFrameCount > 0").
            if w.frames == 0 || f.job_frames(w.job, 2).is_none_or(|r| r.is_empty()) {
                empty.push((race, k, SETTLER_JOB_NAMES[w.job]));
            }
            own_differs += (resolve::first_job(&f, k).is_some_and(|j| j != w.job)) as usize;
        }
    }
    // Race 0 defines every type that any race has, and the per-race first jobs agree with it.
    assert!(lists >= 5 * 60, "{lists}");
    assert_eq!(own_differs, 0);
    // Types a race has no art for (other races' specialists, most Dark Tribe settlers) stay empty.
    assert!(empty.len() < lists / 2, "{} of {lists} empty", empty.len());
    let j = resolve::settler_wait_job(&roman, &roman, Settler::BOWMAN_01 as u16).unwrap();
    assert_eq!(SETTLER_JOB_NAMES[j.job], "BML01_WALK");
}
