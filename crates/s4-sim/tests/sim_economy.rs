//! Piles and eco-sectors against the original's savegames (chunks 163 and 180), skipped when
//! absent.
//!
//! 1. The saves themselves: every pile record and eco-sector record decodes; the offer lists
//!    (eco-sector heads + the piles' `prev`/`next`) hold exactly the piles flagged OFFERED, each
//!    under its good — except the one list-0 entry per save that `ConnectEconomy` produces (it
//!    re-offers merged piles under `IEntity::Type`, 0 for piles); OFFERED implies the offer flag;
//!    a pile with the offer flag and available goods on an eco-sector is offered; live piles sit
//!    on their node (pile id, flag 0x02).
//! 2. Our code on the saved world: the tiling built from the saved layers creates as many
//!    eco-sectors as the save has records; recreating the live piles with `add_pile_ex` gives each
//!    eco-sector exactly the saved goods-on-piles counts, and production / storage / free piles
//!    with goods and no reservations are offered exactly when the save has them offered.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use s4_sim::entity::flag;
use s4_sim::save::SaveGame;
use s4_sim::save_economy::{self, SavedPile};
use s4_sim::{Grid, Rules, World, eco, pile};

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

#[derive(Default, Debug)]
struct Tally {
    piles: u64,
    live: u64,
    live_on_node: u64,
    listed: u64,
    list0: u64,
    unoffered_with_goods: u64,
    eco_records: u64,
    eco_cells: u64,
    eco_cell_mismatch: u64,
    offer_compared: u64,
    offer_mismatch: u64,
}

fn check(f: &Path, rules: &Rules, t: &mut Tally) {
    let s = SaveGame::open(f).unwrap();
    let ((piles, declared, failed), ecos) = save_economy::load(f).unwrap();
    assert_eq!((piles.len() as u32, failed), (declared, 0), "{}: pile records", f.display());
    assert_eq!(ecos.len(), s.eco_sectors.len(), "{}: eco-sector records", f.display());
    let by_id: HashMap<u16, &SavedPile> = piles.iter().map(|p| (p.id, p)).collect();
    let mut w = World::new(Grid::new(s.size).unwrap(), rules.clone());
    w.layers.flags.copy_from_slice(&s.flags);
    for (i, c) in s.render.as_chunks::<4>().0.iter().enumerate() {
        w.layers.ground[i] = c[1];
        w.layers.gfx_bits[i] = c[3];
    }
    w.init_tiling(&s.owner);
    let node = |p: &SavedPile| w.grid.index(p.pos.0 as u32, p.pos.1 as u32);

    // 1. The save's own consistency.
    t.piles += piles.len() as u64;
    for p in &piles {
        let offered = p.flags & flag::OFFERED != 0;
        assert!(!offered || p.offer, "{}: pile {} offered without the offer flag", f.display(), p.id);
        if p.flags & flag::DIED == 0 {
            t.live += 1;
            t.live_on_node += (s.deco_object[node(p)] == p.id && s.flags[node(p)] & 2 != 0) as u64;
        }
        if p.role != pile::role::TRADE && p.offer && p.amount > p.leaving && !offered {
            assert_eq!(eco::eco_of(&w, node(p)), 0, "{}: pile {} with goods not offered", f.display(), p.id);
            t.unoffered_with_goods += 1;
        }
    }
    let mut listed = HashSet::new();
    for e in &ecos {
        for (good, &head) in e.good_offer.iter().enumerate() {
            let mut id = head;
            let mut prev = 0;
            while id != 0 {
                let p = by_id[&id];
                assert_eq!(p.links.0, prev, "{}: offer list links", f.display());
                assert!(listed.insert(id), "{}: pile {id} in two lists", f.display());
                if good == 0 {
                    t.list0 += 1;
                } else {
                    assert_eq!(p.good as usize, good, "{}: pile {id} listed under another good", f.display());
                }
                prev = id;
                id = p.links.1;
            }
        }
    }
    t.listed += listed.len() as u64;
    let flagged: HashSet<u16> = piles.iter().filter(|p| p.flags & flag::OFFERED != 0).map(|p| p.id).collect();
    assert_eq!(listed, flagged, "{}: listed piles = OFFERED piles", f.display());

    // 2. Our code: eco-sectors from the tiling, piles recreated.
    let ours = w.economy.sectors.iter().filter(|e| e.is_some()).count();
    assert_eq!(ours, ecos.len(), "{}: eco-sectors from the tiling", f.display());
    let mut made = Vec::new();
    for p in piles.iter().filter(|p| p.flags & flag::DIED == 0) {
        let id = pile::add_pile_ex(&mut w, p.pos.0, p.pos.1, p.good, p.amount, p.role.min(pile::role::BUILDING_SITE), p.variant, p.patch, p.offset);
        made.push((p, id));
    }
    for e in &ecos {
        let id = eco::eco_of(&w, e.world_index as usize);
        let ours = w.economy.get(id).unwrap_or_else(|| panic!("{}: no eco-sector at {}", f.display(), e.world_index));
        assert_eq!(ours.owner, e.owner, "{}: owner of eco-sector {id}", f.display());
        t.eco_records += 1;
        for g in 1..eco::GOODS {
            t.eco_cells += 1;
            t.eco_cell_mismatch += (ours.goods_on_piles[g] != e.goods_on_piles[g]) as u64;
        }
    }
    for (p, id) in made {
        let Some(id) = id else { continue };
        if matches!(p.role, pile::role::PRODUCTION | pile::role::STORAGE | pile::role::ANYWHERE) && p.leaving == 0 && p.amount > 0 {
            t.offer_compared += 1;
            let ours = w.entities.by_index(id as u32).is_some_and(|e| e.flags & flag::OFFERED != 0);
            t.offer_mismatch += (ours != (p.flags & flag::OFFERED != 0)) as u64;
        }
    }
}

#[test]
fn piles_and_eco_sectors_match_savegames() {
    let files = saves();
    if files.is_empty() {
        eprintln!("skipping: no savegames");
        return;
    }
    let rules = Rules::load_default().unwrap();
    let mut t = Tally::default();
    for f in &files {
        check(f, &rules, &mut t);
    }
    eprintln!("{} saves: {t:?}", files.len());
    assert!(t.list0 > 0, "ConnectEconomy list-0 entries");
    assert!(t.live_on_node * 10000 >= t.live * 9999, "live piles on their node: {}/{}", t.live_on_node, t.live);
    assert_eq!(t.eco_cell_mismatch, 0, "goods on piles per eco-sector");
    assert_eq!(t.offer_mismatch, 0, "offered piles");
}

/// Every eco-sector of the tiling has a record and vice versa; per eco-sector the goods count is
/// the sum over the piles on its nodes; every offered free pile is listed in its node's
/// eco-sector.
fn assert_consistent(w: &World, step: usize) {
    let mut tiling_ecos = HashSet::new();
    let mut sums: HashMap<u16, [u32; eco::GOODS]> = HashMap::new();
    for n in 0..w.grid.len() {
        let e = eco::eco_of(w, n);
        if e != 0 {
            tiling_ecos.insert(e);
        }
    }
    let records: HashSet<u16> = (0..eco::ECO_MAX as u16).filter(|&i| w.economy.get(i).is_some()).collect();
    assert_eq!(tiling_ecos, records, "step {step}: eco-sector records");
    for (id, e) in w.entities.iter() {
        let Some(p) = pile::get(w, id) else { continue };
        if e.is_dead() {
            continue;
        }
        let n = w.grid.index(e.x() as u32, e.y() as u32);
        let ec = eco::eco_of(w, n);
        if ec != 0 {
            sums.entry(ec).or_insert([0; eco::GOODS])[p.good as usize] += p.amount as u32;
        }
        // Only free piles follow their node's owner (`Occupied` / `SetFree` of the other roles do
        // nothing: a storage pile whose node changes hands stays in the old list, as in the
        // original).
        if e.flags & flag::OFFERED != 0 && p.role == pile::role::ANYWHERE {
            let r = w.economy.get(ec).unwrap_or_else(|| panic!("step {step}: offered pile {id} off eco"));
            // Under its good, or in list 0 after a merge (ConnectEconomy keys by IEntity::Type).
            let mut found = false;
            for head in [r.good_offer[p.good as usize], r.good_offer[0]] {
                let mut x = head;
                while x != 0 {
                    found |= x == id;
                    x = w.entities.by_index(x as u32).map_or(0, |e| e.next);
                }
            }
            assert!(found, "step {step}: pile {id} not in eco-sector {ec}'s list");
        }
    }
    for id in records {
        let want = sums.get(&id).copied().unwrap_or([0; eco::GOODS]);
        let have = w.economy.get(id).unwrap().goods_on_piles;
        for g in 0..eco::GOODS {
            assert_eq!(have[g] as u32, want[g], "step {step}: eco-sector {id} good {g}");
        }
    }
}

/// Owners change at random over a map with piles (merges, splits, losses), and the economy
/// follows the tiling's notifications.
#[test]
fn economy_follows_ownership_changes() {
    let p = s4_formats::game_dir().join(r"Map\Campaign\AO_maya1.map");
    if !p.exists() {
        eprintln!("skipping: {} not found", p.display());
        return;
    }
    let map = s4_formats::map::MapFile::open(p).unwrap();
    let (mut w, _) = s4_sim::new_game(&map, Rules::load_default().unwrap(), &s4_sim::NewGame::default()).unwrap();
    let mut r = s4_sim::Random16::new(21, 0);
    let (cx, cy) = (200, 200);
    let mut piles_made = 0;
    for step in 0..600 {
        let (x, y) = (cx + (r.rand() % 60) as i32, cy + (r.rand() % 60) as i32);
        let owner = (r.rand() % 3) as u8;
        // Claim a small hex patch.
        for (dx, dy) in [(0, 0), (1, 0), (1, 1), (0, 1), (-1, 0), (-1, -1), (0, -1)] {
            w.change_owner(x + dx, y + dy, owner);
        }
        if step % 5 == 0 {
            let (px, py) = (cx + (r.rand() % 60) as i32, cy + (r.rand() % 60) as i32);
            let n = w.grid.index(px as u32, py as u32);
            if w.layers.flags[n] & 3 == 0 && w.layers.deco_object[n] == 0 {
                let role = [pile::role::STORAGE, pile::role::ANYWHERE, pile::role::PRODUCTION][(r.rand() % 3) as usize];
                if pile::add_pile(&mut w, px, py, 1 + (r.rand() % 40) as u8, 1 + (r.rand() % 8) as u8, role).is_some() {
                    piles_made += 1;
                }
            }
        }
        assert_consistent(&w, step);
    }
    let ecos = (0..eco::ECO_MAX as u16).filter(|&i| w.economy.get(i).is_some()).count();
    eprintln!("{piles_made} piles, {ecos} eco-sectors at the end");
    assert!(piles_made > 50);
}

/// The transport ranks and building supply shares a fresh eco-sector gets
/// (`CGoodTransportPriority` / `CBuildingSupplyPriority` defaults) against the saved records:
/// the saved rank layout identifies the race (each race ranks a different set of goods); the
/// supply maps must have exactly our building types and goods. Values may differ where the
/// player changed them; their agreement is reported.
#[test]
fn priority_defaults_match_savegames() {
    let files = saves();
    if files.is_empty() {
        eprintln!("skipping: no savegames");
        return;
    }
    let rules = Rules::load_default().unwrap();
    let (mut recs, mut prio_eq, mut supply_recs, mut supply_cells, mut supply_eq) = (0, 0, 0, 0, 0);
    for f in &files {
        let (_, ecos) = save_economy::load(f).unwrap();
        for e in &ecos {
            let race = (0..5u8).find(|&r| {
                let (d, n) = eco::default_transport_priority(r);
                n as i64 == e.transport_prio_count && (0..eco::GOODS).all(|g| (d[g] == 0) == (e.transport_prio[g] == 0))
            });
            let Some(race) = race else { panic!("{}: transport ranks of no race: {:?}", f.display(), e.transport_prio) };
            recs += 1;
            prio_eq += (eco::default_transport_priority(race).0.iter().zip(&e.transport_prio).all(|(a, b)| a == b)) as u32;
            if e.supply.is_empty() {
                continue;
            }
            supply_recs += 1;
            let def = &rules.supply[race as usize];
            let keys: Vec<(i64, Vec<i64>)> = e.supply.iter().map(|(b, m)| (*b, m.iter().map(|p| p.0).collect())).collect();
            let want: Vec<(i64, Vec<i64>)> = def.iter().map(|(&b, m)| (b as i64, m.keys().map(|&g| g as i64).collect())).collect();
            assert_eq!(keys, want, "{}: supply map layout (race {race})", f.display());
            for (b, m) in &e.supply {
                for &(g, v) in m {
                    supply_cells += 1;
                    supply_eq += (def[&(*b as u8)][&(g as u8)] as i64 == v) as u32;
                }
            }
        }
    }
    eprintln!("{recs} records: transport ranks = default in {prio_eq}; {supply_recs} supply maps, {supply_eq}/{supply_cells} shares = default");
    assert!(recs > 0 && supply_recs > 0);
}

/// `CPile::AdjustStatistic` (0x56fe80): the goods statistic of each player equals the goods on
/// the piles of its eco-sectors, per good and in total, after loading and after a while of play.
#[test]
fn goods_statistic_follows_the_piles() {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map");
    if !p.exists() {
        return;
    }
    let map = s4_formats::map::MapFile::open(&p).unwrap();
    let rules = s4_sim::Rules::load_default().unwrap();
    let (mut w, _) = s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).unwrap();
    let check = |w: &s4_sim::World| {
        for pl in 1..=8u8 {
            let mut sum = [0i32; s4_sim::statistic::GOODS];
            for e in w.economy.sectors.iter().flatten().filter(|e| w.economy.owner(e.id) == pl) {
                for (g, &n) in e.goods_on_piles.iter().enumerate() {
                    sum[g] += n as i32;
                }
            }
            let st = &w.statistic.players[pl as usize];
            assert_eq!(st.goods, sum, "player {pl} goods");
            assert_eq!(st.goods_total, sum.iter().sum::<i32>(), "player {pl} total");
        }
    };
    check(&w);
    assert!(w.statistic.players[1].goods_total > 0);
    for _ in 0..2000 {
        s4_sim::tick(&mut w, &[]);
    }
    check(&w);
}
