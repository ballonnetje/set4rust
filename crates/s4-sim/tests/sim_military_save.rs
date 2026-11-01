//! Savegame oracle for the military building role's layout and invariants. Every finished
//! military building's role record (`CMilitaryBuildingRole` store 0x548210, version 3: bytes
//! +0x17c..+0x181, the door settler +0x182, the guard +0x184, +0x188, +0x190, +0x194, +0x18c, the
//! u16 vectors +0x198 and +0x1a4) is read from chunk 162 and checked against the settlers of
//! chunk 161:
//! * the door settler is a type-69 settler on the building's door node;
//! * the slot vector has one entry per buildingInfo soldier slot; every soldier in it stands on
//!   the building's position (`SettlerEnter` 0x5488d0 moves it there), is ON_BOARD unless it is
//!   the door guard, and is a bowman (warrior type 3) in a `top` slot, a swordsman (2) elsewhere;
//! * a former door guard (0x54a120 ORs warrior nibble 0xd in; 0x548790 takes it back off the
//!   map without restoring the nibble or its position) stays ON_BOARD on the door node with
//!   warrior type 13;
//! * bowmen / swordsmen inside (+0x17c / +0x17d) equal the occupied `top` / other slots;
//! * the building is inhabited (`IBuildingRole +0x1d`) exactly when someone is inside.
//!
//! Skipped without savegames.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use s4_formats::map::MapFile;
use s4_sim::Rules;
use s4_sim::entity::flag;

fn saves() -> Vec<PathBuf> {
    let Some(home) = std::env::var_os("USERPROFILE") else { return vec![] };
    let root = Path::new(&home).join("Documents").join("TheSettlers4").join("Save");
    let mut v = Vec::new();
    for dir in [root.clone(), root.join("Archiv-2026-01-20")] {
        if let Ok(rd) = std::fs::read_dir(dir) {
            v.extend(rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "sav")));
        }
    }
    v.sort();
    v
}

fn tokens(d: &[u8]) -> Vec<i64> {
    d.split(|c| c.is_ascii_whitespace() || *c == 0).filter(|t| !t.is_empty()).map(|t| std::str::from_utf8(t).ok().and_then(|s| s.parse().ok()).unwrap_or(i64::MIN)).collect()
}

#[derive(Clone, Copy, Debug)]
struct Head {
    at: usize,
    id: i64,
    kind: i64,
    x: i64,
    y: i64,
    race: i64,
    flags: u32,
}

/// `IEntity` headers: `class, 2, id, type, packedXY, health, objType, race | player << 4, flags, uid`.
fn heads(t: &[i64], obj: i64, max_kind: i64) -> Vec<Head> {
    let mut out = Vec::new();
    let mut k = 0;
    while k + 10 <= t.len() {
        let v = &t[k..k + 10];
        let (x, y) = (v[4] & 0xFFFF, v[4] >> 16);
        if v[1] == 2 && v[6] == obj && (1..0xFFFF).contains(&v[2]) && (1..max_kind).contains(&v[3]) && x < 1024 && (0..1024).contains(&y) && (1..=8).contains(&(v[7] >> 4)) && (0..256).contains(&v[5]) {
            out.push(Head { at: k, id: v[2], kind: v[3], x, y, race: v[7] & 15, flags: v[8] as u32 });
            k += 10;
        } else {
            k += 1;
        }
    }
    out
}

#[derive(Debug, Default)]
struct Totals {
    buildings: u32,
    soldiers: u32,
    doors: u32,
    guards: u32,
    former_guards: u32,
    coming: u32,
    slot_count_mismatch: u32,
}

#[test]
fn military_records_match_settlers() {
    let files = saves();
    if files.is_empty() {
        eprintln!("skipping: no savegames");
        return;
    }
    let rules = Rules::load_default().expect("rules");
    let mut tot = Totals::default();
    let mut failures = Vec::new();
    for f in &files {
        let m = MapFile::open(f).unwrap();
        let b = tokens(&m.container.load_id(162, 0).unwrap().unwrap().1);
        let s = tokens(&m.container.load_id(161, 0).unwrap().unwrap().1);
        let settlers: HashMap<i64, Head> = heads(&s, 1, 70).into_iter().map(|h| (h.id, h)).collect();
        for h in heads(&b, 8, 83) {
            let kind = h.kind as u8;
            if !rules.is_military(h.kind as u16) || h.flags & flag::READY == 0 {
                continue;
            }
            let info = rules.building(h.race as u8, kind);
            // The role record: "3", six bytes, then a door id that is a type-69 settler or 0.
            let seg = &b[h.at + 10..(h.at + 400).min(b.len())];
            let Some(j) = (0..seg.len().saturating_sub(16)).find(|&j| {
                seg[j] == 3 && seg[j + 1..j + 7].iter().all(|&v| (0..256).contains(&v)) && (seg[j + 7] == 0 || settlers.get(&seg[j + 7]).is_some_and(|d| d.kind == 69))
            }) else {
                failures.push(format!("{}: building {} has no military record", f.display(), h.id));
                continue;
            };
            let r = &seg[j..];
            let (bow_in, sword_in, bow_coming, sword_coming) = (r[1], r[2], r[3], r[4]);
            let (door, guard) = (r[7], r[8]);
            let n198 = r[13] as usize;
            let n = r[14 + n198] as usize;
            let slots = &r[15 + n198..15 + n198 + n];
            // IBuildingRole +0x1d: the 10th token after the role's "4 _ _ id".
            let base = (0..j).rev().find(|&k| k >= 3 && seg[k] == h.id && seg[k - 3] == 4);
            let inhabited = base.map(|k| seg[k + 10] == 1);
            tot.buildings += 1;
            tot.coming += (bow_coming + sword_coming) as u32;
            let what = format!("{} building {} type {kind} race {}", f.display(), h.id, h.race);
            if door != 0 {
                tot.doors += 1;
                let d = &settlers[&door];
                let (dx, dy) = (h.x + info.door.0 as i64, h.y + info.door.1 as i64);
                if (d.x, d.y) != (dx, dy) {
                    failures.push(format!("{what}: door settler at ({}, {}), door ({dx}, {dy})", d.x, d.y));
                }
            }
            if n != info.soldier_slots.len() {
                // Settlers United changed some slot lists (docs/SIM.md §10.2).
                tot.slot_count_mismatch += 1;
                continue;
            }
            let (mut tops, mut walls) = (0, 0);
            for (k, &sid) in slots.iter().enumerate() {
                if sid == 0 {
                    continue;
                }
                tot.soldiers += 1;
                let Some(so) = settlers.get(&sid) else {
                    failures.push(format!("{what}: slot {k} soldier {sid} missing"));
                    continue;
                };
                let top = info.soldier_slots[k].top;
                let wt = so.flags & 0xF;
                let at_door = (so.x, so.y) == (h.x + info.door.0 as i64, h.y + info.door.1 as i64);
                if sid == guard {
                    tot.guards += 1;
                } else if wt == 0xd && so.flags & flag::ON_BOARD != 0 && at_door {
                    tot.former_guards += 1;
                } else if so.flags & flag::ON_BOARD == 0 || (so.x, so.y) != (h.x, h.y) {
                    failures.push(format!("{what}: slot {k} soldier {sid} flags {:#x} at ({}, {})", so.flags, so.x, so.y));
                }
                if wt != 0xd && wt != if top { 3 } else { 2 } {
                    failures.push(format!("{what}: slot {k} (top {top}) holds warrior type {wt} (type {})", so.kind));
                }
                if top {
                    tops += 1;
                } else {
                    walls += 1;
                }
            }
            if (bow_in, sword_in) != (tops, walls) {
                failures.push(format!("{what}: inside {bow_in}/{sword_in}, slots {tops}/{walls}"));
            }
            if inhabited.is_some_and(|i| i != (tops + walls > 0)) {
                failures.push(format!("{what}: inhabited {inhabited:?} with {tops}/{walls} inside"));
            }
        }
    }
    eprintln!("{} saves: {tot:?}", files.len());
    for x in failures.iter().take(20) {
        eprintln!("  {x}");
    }
    assert!(tot.buildings > 0);
    assert!(failures.is_empty(), "{} mismatches", failures.len());
}
