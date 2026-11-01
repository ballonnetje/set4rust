//! Savegame oracle for the end-of-game statistics (`statistic`): chunk 133 (`CStatistic`, nine
//! blocks of 0x1128 bytes) and chunk 137 (`CStatisticArchive`) of every save in
//! `Documents\TheSettlers4\Save`.
//!
//! * +0x10c must be the sum of the per-type counts +0x0 + 4·type, +0x220 / +0x224 the civilians
//!   and soldiers by warrior type (`statistic::settler_added`) — in every save. The per-type
//!   counts against the alive settler entities of chunk 161: the release's own counts drift by a
//!   few per thousand (reported, bounded at 1 %).
//! * The history: the newest slot of the first ring of a game whose ring has not wrapped holds
//!   the values of a sample — every category equal to a value the block had then; only checked
//!   for consistency of shape here: slots past the samples taken are zero (games started in a
//!   fresh process; the ring positions are not saved).

use std::path::{Path, PathBuf};

fn dirs_save() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join("Documents").join("TheSettlers4").join("Save"))
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("sav")) {
            out.push(p);
        }
    }
}

#[test]
fn settler_counts_of_the_saves() {
    let Some(dir) = dirs_save().filter(|d| d.exists()) else { return };
    let Ok(rules) = s4_sim::Rules::load_default() else { return };
    let mut files = Vec::new();
    collect(&dir, &mut files);
    let (mut players, mut exact, mut checked, mut all, mut off) = (0, 0, 0, 0i64, 0i64);
    let mut worst: Vec<(String, u8, i32)> = Vec::new();
    for f in &files {
        let (Ok(st), Ok(save)) = (s4_sim::save::statistic(f), s4_sim::save::SaveGame::open(f)) else { continue };
        checked += 1;
        for p in 1..9u8 {
            let b = &st[p as usize];
            let mut ours = vec![0i32; 67];
            for s in save.settlers.iter().filter(|s| s.owner == p && s.obj_type == 1 && (s.kind as usize) < 67 && s.flags & 0x400_0000 == 0) {
                ours[s.kind as usize] += 1;
            }
            let theirs = &b[0..67];
            if ours.iter().all(|&c| c == 0) && theirs.iter().all(|&c| c == 0) {
                continue;
            }
            players += 1;
            // The block's own sums.
            let total: i32 = theirs.iter().sum();
            assert_eq!(b[0x10c / 4], total, "{} player {p}: +0x10c", f.display());
            let class = |t: usize| match rules.warrior_type(t as u16) {
                0 => Some(false),
                1 | 6 | 7 | 14 => None,
                _ => Some(true),
            };
            let civ: i32 = (0..67).filter(|&t| class(t) == Some(false)).map(|t| theirs[t]).sum();
            let sol: i32 = (0..67).filter(|&t| class(t) == Some(true)).map(|t| theirs[t]).sum();
            assert_eq!((b[0x220 / 4], b[0x224 / 4]), (civ, sol), "{} player {p}: civilians / soldiers", f.display());
            let diff: i32 = (0..67).map(|t| (ours[t] - theirs[t]).abs()).sum();
            all += ours.iter().sum::<i32>() as i64;
            off += diff as i64;
            if diff == 0 {
                exact += 1;
            } else {
                if worst.len() < 3 {
                    let d: Vec<(usize, i32, i32)> = (0..67).filter(|&t| ours[t] != theirs[t]).map(|t| (t, ours[t], theirs[t])).collect();
                    eprintln!("{} player {p}: (type, entities, statistic) {d:?}", f.display());
                }
                worst.push((f.file_name().unwrap().to_string_lossy().into(), p, diff));
            }
        }
    }
    worst.sort_by_key(|w| -w.2);
    eprintln!(
        "{checked} saves, {players} players: {exact} with every per-type count equal to chunk 161; {off} of {all} settlers off; largest differences {:?}",
        &worst[..worst.len().min(8)]
    );
    // The release's own counts drift a little from its entities (paths that create or remove a
    // settler without the counters); a few per thousand.
    assert!(off * 100 <= all, "per-type settler counts disagree with the entities: {off} of {all}");
}

#[test]
fn history_shape_of_the_saves() {
    let Some(dir) = dirs_save().filter(|d| d.exists()) else { return };
    let mut files = Vec::new();
    collect(&dir, &mut files);
    let (mut games, mut fits) = (0, 0);
    for f in &files {
        let (Ok(rings), Ok(save)) = (s4_sim::save::statistic_archive(f), s4_sim::save::SaveGame::open(f)) else { continue };
        let samples = (save.tick / 840 + 1) as usize;
        if samples > 16 {
            continue;
        }
        games += 1;
        // Ring 0: slots 0..samples written; the rest zero.
        let zero_beyond = |ring: &[i32], used: usize| {
            (0..9 * 49).all(|pc| ring[pc * 16 + used..pc * 16 + 16].iter().all(|&v| v == 0))
        };
        let ok = zero_beyond(&rings[0], samples) && zero_beyond(&rings[1], (samples - 1) / 3 + 1) && zero_beyond(&rings[2], (samples - 1) / 15 + 1);
        fits += ok as usize;
    }
    eprintln!("{games} saves with at most 16 samples: {fits} with the rings filled exactly as far as their samples");
    assert!(games == 0 || fits * 2 >= games, "the ring shapes do not match the sampling rule");
}

/// `CSettlerMgr`'s per-type count (0x1545658, saved with the settlers) against the statistic's
/// per-type count (chunk 133): both are moved by the same calls (`AddSettler`, `ChangeType`, the
/// removal), so they must agree in every save.
#[test]
fn settler_manager_counts_equal_the_statistic() {
    let Some(dir) = dirs_save().filter(|d| d.exists()) else { return };
    let mut files = Vec::new();
    collect(&dir, &mut files);
    let (mut saves, mut equal) = (0, [0usize; 3]);
    for f in &files {
        let (Ok(st), Ok(tables)) = (s4_sim::save::statistic(f), s4_sim::save::settler_mgr_tables(f)) else { continue };
        saves += 1;
        for (k, t) in tables.iter().enumerate() {
            if (0..9).all(|p| (0..67).all(|ty| t[p * 67 + ty] == st[p][ty])) {
                equal[k] += 1;
            }
        }
    }
    eprintln!("{saves} saves; tables equal to the statistic's per-type counts: {equal:?}");
    assert!(saves == 0 || equal.contains(&saves), "no CSettlerMgr table equals the statistic in every save");
}

/// The fighting strength of every block (0x4f8450): the own-land value (+0xbbc) follows the
/// foreign-land value (+0xbc0), the maxima (+0x1070 / +0x1074) are at least the current values,
/// and for a player without eye catchers the foreign value is the formula of the block's sums
/// (+0xba0 base, +0xba4.. costs, +0x1124 override) — with `b = 0`, or `b = a` for an AI player at
/// level 2 or more; game type 3 (quartered) is counted separately.
#[test]
fn fighting_strength_of_the_saves() {
    let Some(dir) = dirs_save().filter(|d| d.exists()) else { return };
    let Ok(rules) = s4_sim::Rules::load_default() else { return };
    let div = if rules.logic.fighting_strength_divisor > 0 { rules.logic.fighting_strength_divisor } else { 1000 };
    let mut files = Vec::new();
    collect(&dir, &mut files);
    let (mut blocks, mut ai_distinct, mut distinct, mut plain, mut ai, mut quartered, mut checked, mut other) = (0, 0, 0, 0, 0, 0, 0, Vec::new());
    for f in &files {
        let Ok(st) = s4_sim::save::statistic(f) else { continue };
        // The AI players at level 2 or more (chunk 150: the players with an AI, their records).
        let ai_level2: Vec<usize> = s4_formats::map::container::Container::parse(std::fs::read(f).unwrap_or_default())
            .ok()
            .and_then(|c| c.load_id(150, 0).ok().flatten())
            .and_then(|(_, bytes)| s4_sim::ai::chunk::parse(&bytes).ok())
            .map(|ch| ch.players.iter().map(|&(id, _)| id as usize).filter(|&id| ch.records.get(id).is_some_and(|r| r.level >= 2)).collect())
            .unwrap_or_default();
        for (p, b) in st.iter().enumerate().skip(1) {
            let w = |off: usize| b[off / 4];
            let (base, own, foreign) = (w(0xba0), w(0xbbc), w(0xbc0));
            if base == 0 {
                continue;
            }
            blocks += 1;
            assert_eq!(own, s4_sim::statistic::own_strength(foreign), "{} player {p}: own from foreign", f.display());
            assert!(w(0x1070) >= own && w(0x1074) >= foreign, "{} player {p}: maxima below the current strength", f.display());
            if (0xbb0..0xbbc).step_by(4).any(|o| w(o) != 0) {
                continue;
            }
            checked += 1;
            let a = w(0xba4) + w(0xba8) + 2 * w(0xbac);
            let over = w(0x1124);
            let fs = |b: i32, q: bool| s4_sim::statistic::foreign_strength(over, base, a, b, 0, div, q);
            if fs(0, false) != fs(a, false) {
                distinct += 1;
                if ai_level2.contains(&p) {
                    ai_distinct += 1;
                }
            }
            if foreign == fs(0, false) {
                plain += 1;
            } else if foreign == fs(a, false) {
                ai += 1;
            } else if foreign == fs(0, true) || foreign == fs(a, true) {
                quartered += 1;
            } else if other.len() < 6 {
                other.push((f.file_name().unwrap().to_string_lossy().to_string(), p, base, a, foreign));
            }
        }
    }
    eprintln!(
        "{blocks} strength blocks; {checked} without eye catchers ({distinct} where b = a would differ, {ai_distinct} of them AI players at level 2+): {plain} plain, {ai} with b = a, {quartered} quartered; unmatched {other:?}"
    );
    assert!(other.is_empty(), "strengths the formula does not give: {other:?}");
}

/// The capture counts (`FirstSettlerEnter` 0x54871c / 0x548730): +0x374 + 4·type the buildings a
/// player captured, +0x4c0 + 4·type those it lost to a capture. Every capture adds one to each,
/// so per building type the players' sums are equal in every save; only military types count.
#[test]
fn capture_counts_balance_in_the_saves() {
    let Some(dir) = dirs_save().filter(|d| d.exists()) else { return };
    let mut files = Vec::new();
    collect(&dir, &mut files);
    let mut captures = 0;
    for f in &files {
        let Ok(st) = s4_sim::save::statistic(f) else { continue };
        for t in 0..83 {
            let won: i32 = (0..9).map(|p| st[p][0x374 / 4 + t]).sum();
            let lost: i32 = (0..9).map(|p| st[p][0x4c0 / 4 + t]).sum();
            assert_eq!(won, lost, "{} type {t}", f.display());
            if won != 0 {
                assert!((46..=48).contains(&t), "{} type {t}: {won} captures", f.display());
            }
            captures += won;
        }
    }
    eprintln!("{captures} captures in {} saves", files.len());
}

/// The produced counts (`statistic::settler_produced`): +0x110 + 4·type per type, +0x21c their
/// total — written together at the barracks (0x5739c5), the dark temple (0x54586a) and the
/// residence (0x54ddc6, type 1), so the total is the sum in every save.
#[test]
fn produced_counts_sum_in_the_saves() {
    let Some(dir) = dirs_save().filter(|d| d.exists()) else { return };
    let mut files = Vec::new();
    collect(&dir, &mut files);
    let mut by_type = [0i64; 67];
    for f in &files {
        let Ok(st) = s4_sim::save::statistic(f) else { continue };
        for (p, b) in st.iter().enumerate() {
            let per: i32 = b[0x110 / 4..0x110 / 4 + 67].iter().sum();
            assert_eq!(b[0x21c / 4], per, "{} player {p}: +0x21c", f.display());
            for t in 0..67 {
                by_type[t] += b[0x110 / 4 + t] as i64;
            }
        }
    }
    let made: Vec<(usize, i64)> = (0..67).filter(|&t| by_type[t] != 0).map(|t| (t, by_type[t])).collect();
    eprintln!("{} saves; produced per type {made:?}", files.len());
}

/// Every total of the block is the sum of its per-type counts in every save: finished buildings
/// now (+0x60c / +0x228, 83 types), finished ever (+0x75c / +0x610), goods made (+0xb60 /
/// +0xab4, 43 goods), vehicles made (+0xb7c / +0xb64, 6 types), vehicles finished (+0xb98 /
/// +0xb80).
#[test]
fn totals_are_the_sums_in_the_saves() {
    let Some(dir) = dirs_save().filter(|d| d.exists()) else { return };
    let mut files = Vec::new();
    collect(&dir, &mut files);
    let mut sums = [0i64; 6];
    for f in &files {
        let Ok(st) = s4_sim::save::statistic(f) else { continue };
        for (p, b) in st.iter().enumerate() {
            for (total, start, n) in [(0x60c, 0x228, 83), (0x75c, 0x610, 83), (0xb60, 0xab4, 43), (0xb7c, 0xb64, 6), (0xb98, 0xb80, 6)] {
                let per: i32 = b[start / 4..start / 4 + n].iter().sum();
                assert_eq!(b[total / 4], per, "{} player {p}: +{total:#x}", f.display());
            }
            for (k, o) in [0x60c, 0x75c, 0xa00, 0xb60, 0xb7c, 0xb98].into_iter().enumerate() {
                sums[k] += b[o / 4] as i64;
            }
        }
    }
    eprintln!("{} saves; summed over all players: now {}, ever {}, beds {}, goods made {}, vehicles made {} / finished {}", files.len(), sums[0], sums[1], sums[2], sums[3], sums[4], sums[5]);
}

/// +0xa00, the beds (`CResidenceBuildingRole::Init` 0x54db94 adds the residence's info +0x1f0,
/// its vtbl+0x2c 0x54df40 takes them off): against the finished residences of chunk 162. Not
/// the Settlers United saves: the mod changed the beds.
#[test]
fn beds_of_the_saves() {
    let Some(dir) = dirs_save().filter(|d| d.exists()) else { return };
    let Ok(rules) = s4_sim::Rules::load_default() else { return };
    let mut files = Vec::new();
    collect(&dir, &mut files);
    let (mut players, mut equal, mut off) = (0, 0, Vec::new());
    for f in &files {
        let (Ok(st), Ok(save)) = (s4_sim::save::statistic(f), s4_sim::save::SaveGame::open(f)) else { continue };
        if save.settlers_united {
            continue;
        }
        for p in 1..9u8 {
            let ours: i32 = save
                .buildings
                .iter()
                .filter(|b| b.owner == p && b.flags & 0x1_0000 != 0 && b.flags & 0x400_0000 == 0 && b.kind < 0x100)
                .map(|b| rules.building(b.race, b.kind as u8))
                .filter(|r| r.house_kind == 14)
                .map(|r| r.beds)
                .sum();
            let theirs = st[p as usize][0xa00 / 4];
            if ours == 0 && theirs == 0 {
                continue;
            }
            players += 1;
            if ours == theirs {
                equal += 1;
            } else if off.len() < 8 {
                off.push((f.file_name().unwrap().to_string_lossy().to_string(), p, ours, theirs));
            }
        }
    }
    eprintln!("{players} players with beds: {equal} equal to their finished residences; others (save, player, residences, statistic) {off:?}");
    assert_eq!(equal, players, "bed totals");
}
