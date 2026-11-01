//! Savegame oracle for the mana pools (`magic`, chunk 140 via `save::players`): in every save
//! of `Documents\TheSettlers4\Save` each player's spell costs must follow from a base cost and
//! the cast counts by `magic::raise_cost` (SpellCostIncrease 10 %, at most +300 %, the install's
//! Magic.cfg), and the mana must be the mana gained minus what those casts cost — less for a
//! Viking (war machine shots, 0x4f59f0, take mana without a cast). The base costs themselves
//! differ between saves (other Magic.cfg versions); they are inferred.

use s4_sim::magic::{MagicCfg, SPELLS, raise_cost};

/// The mana spent on `casts` casts from base cost `base`, if they end at `cost`.
fn spent(cfg: &MagicCfg, base: i32, casts: u32, cost: i32) -> Option<i32> {
    let max = base.clamp(1, 1000) * (cfg.spell_cost_max_increase + 100) / 100;
    let rise = cfg.cost_increase(0);
    let (mut c, mut sum) = (base, 0);
    for _ in 0..casts {
        sum += c;
        c = raise_cost(c, rise, max);
    }
    (c == cost).then_some(sum)
}

#[test]
fn mana_pools_of_the_saves() {
    let dir = dirs_save();
    let Some(dir) = dir.filter(|d| d.exists()) else { return };
    let cfg = MagicCfg::load_default();
    let mut files = Vec::new();
    collect(&dir, &mut files);
    let (mut players, mut casting, mut vikings_short) = (0, 0, 0);
    for f in files {
        let Ok(ps) = s4_sim::save::players(&f) else { continue };
        for (i, p) in ps.iter().enumerate() {
            players += 1;
            assert_eq!(p.unknown_c, 0, "{} player {}", f.display(), i + 1);
            // Every combination of the bases that explain each spell's cost.
            let options: Vec<Vec<i32>> = (0..SPELLS)
                .map(|s| {
                    let (n, c) = (p.magic.casts[s], p.magic.costs[s]);
                    if n == 0 {
                        return vec![0];
                    }
                    let mut v: Vec<i32> = (1..=1000).filter_map(|b| spent(&cfg, b, n, c)).collect();
                    v.sort_unstable();
                    v.dedup();
                    v
                })
                .collect();
            assert!(options.iter().all(|o| !o.is_empty()), "{} player {}: costs {:?} not reachable by casts {:?}", f.display(), i + 1, p.magic.costs, p.magic.casts);
            if p.magic.casts.iter().any(|&n| n > 0) {
                casting += 1;
            }
            let mut sums = vec![0i32];
            for o in &options {
                sums = sums.iter().flat_map(|s| o.iter().map(move |x| s + x)).collect();
                sums.sort_unstable();
                sums.dedup();
            }
            let left: Vec<i32> = sums.iter().map(|s| p.magic.gained - s).collect();
            if left.contains(&p.magic.mana) {
                continue;
            }
            assert!(p.race == 1 && left.iter().any(|&l| p.magic.mana < l), "{} player {} (race {}): mana {} gained {} spent one of {:?}", f.display(), i + 1, p.race, p.magic.mana, p.magic.gained, sums);
            vikings_short += 1;
        }
    }
    eprintln!("{players} players, {casting} with casts, {vikings_short} Vikings below (war machines)");
    assert!(players > 100 && casting > 10);
}

fn dirs_save() -> Option<std::path::PathBuf> {
    std::env::var_os("USERPROFILE").map(|h| std::path::PathBuf::from(h).join("Documents").join("TheSettlers4").join("Save"))
}

fn collect(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut v: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    v.sort();
    for p in v {
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("sav")) {
            out.push(p);
        }
    }
}

#[test]
fn keys_match_the_init() {
    // 0x4f5400 stores cost i with these constants.
    let k: Vec<u32> = (0..8).map(s4_sim::magic::keys::cost).collect();
    assert_eq!(k, [0x93476345, 0x268ec68a, 0xb9d629cf, 0x4d1d8d14, 0xe064f059, 0x73ac539e, 0x06f3b6e3, 0x9a3b1a28]);
    // 0x4f6e00's second key series starts at 0xbdbdbe07 (= −0x424241f9).
    assert_eq!(s4_sim::magic::keys::casts(0), 0xbdbdbe07);
}
