//! Savegame oracle for `CTradingBuildingRole`'s record (store 0x551dc0, version 4; see
//! `buildings::trading`). Every marketplace / port in chunk 162 is parsed field by field and
//! checked:
//! * the record reads to its end: `4, +0x17c, 1 43 [+0x180], 1 43 [+0x1ab], 8 bools +0x257,
//!   1 43 [+0x1d6], n [piles], 13 slot words +0x270..+0x294, m [entity, kind, foreign],
//!   +0x17e, 1 43 [+0x201], 1 43 [+0x22c], +0x2a8`;
//! * one pile per buildingInfo pile, all distinct;
//! * +0x257.. are "owner is player 1" (`Init` 0x5524d0);
//! * the target is 0 or another trading building of the same owner;
//! * flags use bits 1..3 only, the amounts are 0..=100 (+0x201 also below 0: `GoodArrive`
//!   counts it down without a floor);
//! * transports are kinds 1..=3; a donkey transport (2) or a busy donkey slot is a type-60
//!   settler.
//!
//! Only the `H8GQE2M0_*` saves (Settlers United era) have marketplaces; none has a port.
//! Skipped without savegames.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use s4_formats::map::MapFile;
use s4_sim::Rules;

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

fn tokens(d: &[u8]) -> Vec<i64> {
    d.split(|c| c.is_ascii_whitespace() || *c == 0).filter(|t| !t.is_empty()).map(|t| std::str::from_utf8(t).ok().and_then(|s| s.parse().ok()).unwrap_or(i64::MIN)).collect()
}

#[derive(Clone, Copy, Debug)]
struct Head {
    at: usize,
    id: i64,
    kind: i64,
    race: i64,
    owner: i64,
    flags: i64,
}

/// `IEntity` headers: `class, 2, id, type, packedXY, health, objType, race | player << 4, flags, uid`.
fn heads(t: &[i64], obj: i64, max_kind: i64) -> Vec<Head> {
    let mut out = Vec::new();
    let mut k = 0;
    while k + 10 <= t.len() {
        let v = &t[k..k + 10];
        if v[1] == 2 && v[6] == obj && (1..0xFFFF).contains(&v[2]) && (1..max_kind).contains(&v[3]) && (v[4] & 0xFFFF) < 1024 && (0..1024).contains(&(v[4] >> 16)) && (1..=8).contains(&(v[7] >> 4)) {
            out.push(Head { at: k, id: v[2], kind: v[3], race: v[7] & 15, owner: v[7] >> 4, flags: v[8] });
            k += 10;
        } else {
            k += 1;
        }
    }
    out
}

/// A parsed record (the fields the checks use).
#[derive(Debug)]
struct Record {
    target: i64,
    collect: Vec<i64>,
    send: Vec<i64>,
    player_one: Vec<i64>,
    flags: Vec<i64>,
    piles: Vec<i64>,
    slots: Vec<i64>,
    transports: Vec<[i64; 3]>,
    partner: i64,
    foreign_collect: Vec<i64>,
    foreign_send: Vec<i64>,
}

fn table(r: &[i64], at: &mut usize) -> Option<Vec<i64>> {
    if r.get(*at..*at + 2)? != [1, 43] {
        return None;
    }
    let v = r.get(*at + 2..*at + 45)?.to_vec();
    *at += 45;
    Some(v)
}

fn parse(r: &[i64]) -> Option<Record> {
    if r.first() != Some(&4) {
        return None;
    }
    let mut at = 2;
    let target = r[1];
    let collect = table(r, &mut at)?;
    let send = table(r, &mut at)?;
    let player_one = r.get(at..at + 8)?.to_vec();
    at += 8;
    let flags = table(r, &mut at)?;
    let n = *r.get(at)? as usize;
    let piles = r.get(at + 1..at + 1 + n)?.to_vec();
    at += 1 + n;
    let slots = r.get(at..at + 13)?.to_vec();
    at += 13;
    let m = *r.get(at)? as usize;
    let transports = (0..m).map(|i| [r[at + 1 + 3 * i], r[at + 2 + 3 * i], r[at + 3 + 3 * i]]).collect();
    at += 1 + 3 * m;
    let partner = *r.get(at)?;
    at += 1;
    let foreign_collect = table(r, &mut at)?;
    let foreign_send = table(r, &mut at)?;
    r.get(at)?;
    Some(Record { target, collect, send, player_one, flags, piles, slots, transports, partner, foreign_collect, foreign_send })
}

#[test]
fn trading_records_parse_and_hold() {
    let files = saves();
    if files.is_empty() {
        eprintln!("skipping: no savegames");
        return;
    }
    let rules = Rules::load_default().expect("rules");
    let (mut records, mut with_target, mut donkeys) = (0, 0, 0);
    let mut failures = Vec::new();
    for f in &files {
        let m = MapFile::open(f).unwrap();
        let b = tokens(&m.container.load_id(162, 0).unwrap().unwrap().1);
        let s = tokens(&m.container.load_id(161, 0).unwrap().unwrap().1);
        let settlers: HashMap<i64, Head> = heads(&s, 1, 70).into_iter().map(|h| (h.id, h)).collect();
        let buildings = heads(&b, 8, 83);
        let by_id: HashMap<i64, Head> = buildings.iter().map(|h| (h.id, *h)).collect();
        for h in &buildings {
            // Finished ones only (`FLAG_BUILT` 0x10000); a site has the site role's record.
            if (h.kind != 32 && h.kind != 33) || h.flags & 0x10000 == 0 {
                continue;
            }
            let what = format!("{} building {} type {}", f.display(), h.id, h.kind);
            let seg = &b[h.at + 10..(h.at + 900).min(b.len())];
            // The role record: "4, target, 1, 43" after the IBuildingRole part.
            let Some(j) = (0..seg.len().saturating_sub(4)).find(|&j| seg[j] == 4 && seg[j + 2] == 1 && seg[j + 3] == 43) else {
                failures.push(format!("{what}: no trading record"));
                continue;
            };
            let Some(r) = parse(&seg[j..]) else {
                failures.push(format!("{what}: record does not parse"));
                continue;
            };
            records += 1;
            let info = rules.building(h.race as u8, h.kind as u8);
            if r.piles.len() != info.piles.len() {
                failures.push(format!("{what}: {} piles, info {}", r.piles.len(), info.piles.len()));
            }
            let mut ps = r.piles.clone();
            ps.sort();
            ps.dedup();
            if ps.len() != r.piles.len() || ps.contains(&0) {
                failures.push(format!("{what}: piles {:?}", r.piles));
            }
            if r.player_one.iter().any(|&v| v != (h.owner == 1) as i64) {
                failures.push(format!("{what}: +0x257 {:?}, owner {}", r.player_one, h.owner));
            }
            if r.target != 0 {
                with_target += 1;
                match by_id.get(&r.target) {
                    Some(t) if (t.kind == 32 || t.kind == 33) && t.owner == h.owner && t.id != h.id => {}
                    t => failures.push(format!("{what}: target {} is {t:?}", r.target)),
                }
            }
            if r.flags.iter().any(|&v| v & !0x0e != 0) {
                failures.push(format!("{what}: flags {:?}", r.flags));
            }
            for t in [&r.collect, &r.send, &r.foreign_send] {
                if t.iter().any(|&v| !(0..=100).contains(&v)) {
                    failures.push(format!("{what}: amounts {t:?}"));
                }
            }
            // +0x201 counts down per foreign good arriving (0x5522f0) without a floor.
            if r.foreign_collect.iter().any(|&v| !(-128..=100).contains(&v)) {
                failures.push(format!("{what}: foreign amounts {:?}", r.foreign_collect));
            }
            for t in &r.transports {
                if !(1..=3).contains(&t[1]) || !(0..=1).contains(&t[2]) {
                    failures.push(format!("{what}: transport {t:?}"));
                }
                if t[1] == 2 {
                    donkeys += 1;
                    if settlers.get(&t[0]).is_none_or(|s| s.kind != 60) {
                        failures.push(format!("{what}: donkey transport {} is {:?}", t[0], settlers.get(&t[0])));
                    }
                }
            }
            // +0x280 / +0x284: the loading donkey.
            if r.slots[6] != 0 && settlers.get(&r.slots[5]).is_none_or(|s| s.kind != 60) {
                failures.push(format!("{what}: loading donkey {} is {:?}", r.slots[5], settlers.get(&r.slots[5])));
            }
            let _ = r.partner;
        }
    }
    eprintln!("{} saves: {records} trading records, {with_target} with a target, {donkeys} donkey transports", files.len());
    for x in failures.iter().take(20) {
        eprintln!("  {x}");
    }
    assert!(failures.is_empty(), "{} mismatches", failures.len());
}
