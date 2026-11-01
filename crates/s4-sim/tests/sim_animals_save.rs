//! Savegames as oracle for the land animals (chunk 165, `animals::save`): every saved animal must
//! be in a state the ported `CLandAnimal` can produce. Skipped when no saves are present.

use s4_sim::animals::{self, AnimalRules, save::SavedAnimal};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn saves(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            saves(&p, out);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("sav")) {
            out.push(p);
        }
    }
}

fn save_dir() -> PathBuf {
    std::env::var_os("S4_SAVE_DIR").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(std::env::var_os("USERPROFILE").unwrap_or_default()).join(r"Documents\TheSettlers4\Save")
    })
}

struct Save {
    name: String,
    size: usize,
    animals: Vec<SavedAnimal>,
    flags: Vec<u8>,
    map_object: Vec<u16>,
    ground: Vec<u8>,
}

fn load(p: &Path) -> Option<Save> {
    let m = s4_formats::map::MapFile::open(p).ok()?;
    let chunk = |id: u16| m.container.load_id(id, 0).ok().flatten().map(|(_, d)| d);
    let animals = animals::save::parse(&chunk(165)?)?;
    let flags = chunk(205)?;
    let size = (1..=1024usize).find(|s| s * s == flags.len())?;
    let map_object = chunk(202)?.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
    let ground = chunk(200)?.chunks(4).map(|c| c[1]).collect();
    Some(Save { name: p.display().to_string(), size, animals, flags, map_object, ground })
}

#[test]
fn saved_animals_follow_the_land_animal_role() {
    let mut files = Vec::new();
    saves(&save_dir(), &mut files);
    if files.is_empty() {
        return;
    }
    let rules = AnimalRules::load_default();
    if rules.types.is_empty() {
        return;
    }
    let mut stats: BTreeMap<&str, usize> = BTreeMap::new();
    let mut per_kind: BTreeMap<u16, usize> = BTreeMap::new();
    let mut per_state: BTreeMap<i32, usize> = BTreeMap::new();
    let mut bad = Vec::new();
    for f in &files {
        let Some(s) = load(f) else {
            *stats.entry("saves not decoded").or_default() += 1;
            continue;
        };
        *stats.entry("saves").or_default() += 1;
        // The cap of `CAnimalMgr::Init`: MAX_ANIMALS or LAND_POP · land% · cells / 10000.
        let land = s.ground.iter().filter(|&&g| s4_sim::tiling::ground_tile_type(g) & s4_sim::tiling::TILE_LAND != 0).count();
        let percent = (land * 100 / (s.size * s.size)).max(1);
        let cells = (s.size >> 4) * (s.size >> 4);
        let cap = (rules.max_animals as usize).min(rules.land_pop as usize * percent * cells / 10000);
        if s.animals.len() == cap {
            *stats.entry("saves at the cap").or_default() += 1;
        } else if std::env::var_os("ANIMAL_CAPS").is_some() {
            eprintln!("{}: {} of {cap}", s.name, s.animals.len());
        }
        if s.animals.len() > cap {
            bad.push(format!("{}: {} animals > cap {cap}", s.name, s.animals.len()));
        }
        for a in &s.animals {
            *stats.entry("animals").or_default() += 1;
            *per_kind.entry(a.kind).or_default() += 1;
            *per_state.entry(a.state).or_default() += 1;
            let mut fail = |what: &str| bad.push(format!("{}: #{} kind {} state {} task {}: {what}", s.name, a.id, a.kind, a.state, a.task));
            let cfg = rules.types.get(a.kind as usize).copied().flatten();
            let Some(cfg) = cfg else {
                fail("type without config");
                continue;
            };
            if a.speed != 10 {
                fail("speed");
            }
            if (a.flags & 0x20000 != 0) != cfg.huntable {
                fail("huntable flag");
            }
            // The task fields are TakeJob's copy of the type's task.
            if (a.task_type, a.job, a.frames) != animals::task(&rules, a.kind, a.task) {
                fail("task fields");
            }
            let expect_task = match a.state {
                0 => 0,
                1 => 1,
                2 => 2,
                3 => 3,
                _ => 4,
            };
            if a.task != expect_task {
                fail("state / task");
            }
            let n = a.pos.y() as usize * s.size + a.pos.x() as usize;
            if a.state < 5 && s.map_object.get(n) != Some(&a.id) {
                fail("not on the map-object layer");
            }
            // Standing and idling animals decided where they stand: their land, no step since.
            if a.state <= 1 {
                *stats.entry("animals standing or idle").or_default() += 1;
                let dark = s.flags[n] & 4 != 0;
                if dark != (a.kind == 2 || a.kind == 3) {
                    fail("wrong land after a decision");
                }
                if a.moved != 0x48 {
                    fail("step flags after TakeJob");
                }
                // A fresh spawn (`Init`: last position 0, home here) has not decided yet.
                let fresh = a.state == 0 && a.last_pos == 0 && a.home == a.pos.0 as i32;
                if fresh {
                    *stats.entry("fresh spawns").or_default() += 1;
                } else if a.last_pos != a.pos.0 as i32 {
                    fail("last position");
                }
            }
            // Counter ranges: the enemy search runs every second decision, the cell check after
            // 8 roaming steps, a flee direction lasts 5 steps, 151 still decisions delete, and
            // one wrong-land decision deletes; the dead count up in 15s to 450.
            if !(0..=1).contains(&a.search) {
                fail("search counter");
            }
            // (A fleeing decision skips the reset, so a runaway type that walks back to its land
            // while fleeing can pass 9.)
            if a.roam < 0 || (a.roam > 9 && !cfg.runaway) {
                fail(&format!("roam counter {}", a.roam));
            }
            if !(-1..=4).contains(&a.flee_steps) {
                fail("flee steps");
            }
            if !(0..=0x96).contains(&a.stuck) {
                fail("stuck counter");
            }
            if a.wrong_land != 0 {
                fail("wrong-land counter");
            }
            if a.dead_ticks % 15 != 0 || !(0..0x1c2).contains(&a.dead_ticks) {
                fail("dead ticks");
            }
            if !cfg.runaway && (a.state == 3 || a.search != 0 || a.enemy != 0) {
                fail("flee state of a type that does not run away");
            }
            if matches!(a.state, 2 | 3) && a.moved & 0x88 == 0 {
                *stats.entry("animals walked a step").or_default() += 1;
                if a.moved != a.dir {
                    fail("step direction");
                }
            }
        }
    }
    eprintln!("{stats:?}\nper kind {per_kind:?}\nper state {per_state:?}");
    for b in bad.iter().take(30) {
        eprintln!("{b}");
    }
    assert!(bad.is_empty(), "{} mismatches", bad.len());
    assert!(stats.get("animals").copied().unwrap_or(0) > 0, "no saved animals decoded");
}
