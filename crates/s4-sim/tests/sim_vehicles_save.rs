//! Savegame oracle for the vehicles (chunk 166, `vehicles::save`): every record of every save
//! parses to the class's own fields and the chunk's announced count matches.
//!
//! Checks per record (`CVehicle` ctor / `InitBuildInfo` 0x594750 and the classes):
//! * land vehicles (types 4 catapult, 5 cart) are obj type 4 and walk with a `CWalkingCatapult`
//!   (walker kind 3);
//! * the build record: +0x6c / +0x6e are the `VehicleInfo.xml` boards / ironbars of the race and
//!   type, +0x72 = their sum, +0x74 = 0xfffe / +0x72, +0x76 = 1; a finished vehicle (not BIRTH)
//!   has +0x6d = +0x6f = 0 and +0x70 = +0x72;
//! * the class: 57 for carts, 60 for catapults; a catapult's tail is `CWheeler` 1 + 3 ints,
//!   `CCatapult` 1, +0xb8, +0xbc, +0xc0, the race class's 1 (`save::parse_catapult`); a READY
//!   catapult's +0xc0 is at most 20 ticks behind its last update and its next shot (+0x68) at
//!   most `ReadyToFireDelay` ahead (`vehicles::catapult::update` / `fire`).

use std::path::{Path, PathBuf};

use s4_sim::vehicles::save;

fn saves() -> Vec<PathBuf> {
    let Some(home) = std::env::var_os("USERPROFILE") else { return vec![] };
    let mut stack = vec![Path::new(&home).join("Documents").join("TheSettlers4").join("Save")];
    let mut v = Vec::new();
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "sav") {
                v.push(p);
            }
        }
    }
    v.sort();
    v
}

#[test]
fn vehicle_records_parse_and_hold() {
    let files = saves();
    if files.is_empty() {
        eprintln!("skipping: no savegames");
        return;
    }
    let rules = s4_sim::Rules::load_default().expect("rules");
    let (mut with, mut total) = (0, 0);
    let mut classes = std::collections::BTreeMap::<(i64, u16), usize>::new();
    let mut failures = Vec::new();
    for f in &files {
        let Some((vs, announced, failed)) = save::load(f).unwrap() else { continue };
        let what = f.display().to_string();
        if failed != 0 || vs.len() as i64 != announced {
            failures.push(format!("{what}: {} parsed of {announced}, {failed} failed", vs.len()));
        }
        if !vs.is_empty() {
            with += 1;
        }
        for v in &vs {
            total += 1;
            *classes.entry((v.class, v.kind)).or_default() += 1;
            let w = format!("{what} vehicle {} type {}", v.id, v.kind);
            if matches!(v.kind, 4 | 5) && (v.obj_type != 4 || v.walking.map(|w| w[1]) != Some(3)) {
                failures.push(format!("{w}: obj type {} walker {:?}", v.obj_type, v.walking.map(|w| w[1])));
            }
            let info = rules.vehicles.get(v.race, v.kind as u8);
            let (boards, iron) = (info.boards as i64, info.ironbars as i64);
            let [b, bl, ir, irl] = v.b6c;
            let [done, sum, step, one, _] = v.s70;
            if (b, ir) != (boards, iron) || sum != b + ir || step != 0xfffe / sum.max(1) || one != 1 {
                failures.push(format!("{w}: build {:?} {:?}, info boards {boards} ironbars {iron}", v.b6c, v.s70));
            }
            // The catapults' class record (all of the saves' are Viking ones).
            if v.kind == 4 {
                match save::parse_catapult(&v.tail) {
                    Some(c) if (0..=1).contains(&c.may_load) && c.carrier >= 0 && c.last_tick >= 0 => {
                        // A READY catapult's update (0x5610c0) steps +0xc0 once it is 20 ticks
                        // old, and a shot sets +0x68 to now + `ReadyToFireDelay`.
                        let ready = v.flags & 0x200_0000 != 0;
                        let lut = v.last_update_tick;
                        if ready && (!(0..=20).contains(&(lut - c.last_tick)) || v.i68 - lut > info.fire_delay as i64) {
                            failures.push(format!("{w}: update tick {lut}, +0xc0 {}, +0x68 {}", c.last_tick, v.i68));
                        }
                    }
                    c => failures.push(format!("{w}: catapult tail {:?} → {c:?}", v.tail)),
                }
            }
            let birth = v.flags & 0x100_0000 != 0;
            if !birth && (bl != 0 || irl != 0 || done != sum) {
                failures.push(format!("{w}: finished but build {:?} {:?}", v.b6c, v.s70));
            }
        }
    }
    eprintln!("{} saves, {with} with vehicles, {total} records, (class, type) {classes:?}", files.len());
    for x in failures.iter().take(20) {
        eprintln!("  {x}");
    }
    assert!(failures.is_empty(), "{} mismatches", failures.len());
}
