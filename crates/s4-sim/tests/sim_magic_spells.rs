//! The spells of `magic::spells` on roman02, cast through `magic::cast_spell` with a race from
//! the flags (as the map scripts do: free, no range check) and the game RNG. Skipped without the
//! game data.

use s4_sim::World;
use s4_sim::entity::{EntityData, ObjType, flag};
use s4_sim::magic::{self, cast_flag};

fn roman02() -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map");
    if !p.exists() {
        return None;
    }
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    let rules = s4_sim::Rules::load_default().ok()?;
    let (w, _) = s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).ok()?;
    Some(w)
}

/// `CastSpell` returns 1 for a cast; the spell's own result (what it did) comes from running it
/// on the context the cast would build.
fn cast(w: &mut World, race: u32, spell: u8, x: i32, y: i32) -> (i32, i32) {
    let ctx = magic::SpellCtx { player: 1, race: race as u8, x, y, radius: 0, limit: 0, duration: 0, flags: 0, caster: 0, spell };
    let mut probe = w.clone();
    let c = magic::context(&mut probe, ctx);
    let n = magic::run_spell(&mut probe, &c);
    let r = magic::cast_spell(w, 1, spell, x, y, cast_flag::RACE | race << 24, 0);
    // The same world but for the pool (casts + 1).
    let parts = |w: &World| w.subsystem_hashes().into_iter().filter(|(k, _)| *k != "magic").collect::<Vec<_>>();
    assert_eq!(parts(w), parts(&probe), "the cast is the spell on its context");
    (r, n)
}

/// The first node (row by row) for which `f` holds.
fn find(w: &World, f: impl Fn(&World, usize, i32, i32) -> bool) -> Option<(i32, i32)> {
    let s = w.grid.size() as i32;
    (8..s - 8).flat_map(|y| (8..s - 8).map(move |x| (x, y))).find(|&(x, y)| f(w, w.grid.index(x as u32, y as u32), x, y))
}

fn piles_near(w: &World, x: i32, y: i32, r: i32) -> Vec<(u16, u8, u8, u32)> {
    w.entities
        .iter()
        .filter_map(|(id, e)| match &e.data {
            EntityData::Pile(p) if s4_sim::spiral::grid_distance(e.x() - x, e.y() - y) <= r => Some((id, p.good, p.amount, e.flags)),
            _ => None,
        })
        .collect()
}

#[test]
fn gift_of_goods() {
    let Some(mut w) = roman02() else { return };
    // A grass node with grass around (the gift needs free ground 0x1X).
    let (x, y) = find(&w, |w, n, _, _| w.layers.ground[n] & 0xf0 == 0x10 && w.layers.flags[n] == 0 && w.layers.deco_object[n] == 0 && w.layers.map_object[n] == 0).expect("grass");
    let before = piles_near(&w, x, y, 8);
    let calls = w.rng.calls;
    assert_eq!(cast(&mut w, 0, 0, x, y).0, 1);
    let after = piles_near(&w, x, y, 8);
    let new: Vec<_> = after.iter().filter(|p| !before.contains(p)).collect();
    eprintln!("gift at ({x}, {y}): {new:?}, {} draws", w.rng.calls - calls);
    // SpellLimits Roman 0: 3..5 piles.
    assert!((3..=5).contains(&new.len()), "{new:?}");
    let goods = w.rules.magic.present_goods[0];
    for &&(id, good, amount, flags) in &new {
        assert!(goods.contains(&(good as i32)) && (1..=8).contains(&amount), "pile {id}: {good} × {amount}");
        assert!(flags & flag::MAGIC_INVISIBLE != 0);
    }
    for _ in 0..8 {
        s4_sim::tick(&mut w, &[]);
    }
    assert!(new.iter().all(|p| w.entities.by_index(p.0 as u32).unwrap().flags & flag::MAGIC_INVISIBLE == 0), "still invisible");
}

#[test]
fn convert_goods() {
    let Some(mut w) = roman02() else { return };
    let (src, dst) = (w.rules.magic.convert_good_src[0] as u8, w.rules.magic.convert_good_dst[0] as u8);
    let (x, y) = find(&w, |w, n, _, _| w.layers.ground[n] & 0xf0 == 0x10 && w.layers.flags[n] == 0 && w.layers.deco_object[n] == 0 && w.layers.map_object[n] == 0).expect("grass");
    // Three piles of 4 near the target: the limit (Roman spell 1: 6) allows up to 8 (limit + 2)
    // and stops at 4 (limit − 2 is below 8, so 8 is the stop).
    let mut piles = Vec::new();
    for (dx, dy) in [(0, 0), (2, 0), (0, 2)] {
        piles.push(s4_sim::pile::add_pile(&mut w, x + dx, y + dy, src, 4, s4_sim::pile::role::ANYWHERE).expect("pile"));
    }
    let (_, n) = cast(&mut w, 0, 1, x, y);
    let goods: Vec<u8> = piles.iter().map(|&p| s4_sim::pile::get(&w, p).unwrap().good).collect();
    eprintln!("convert {src} → {dst}: {n}, {goods:?}");
    assert_eq!(n, 8);
    assert_eq!(goods.iter().filter(|&&g| g == dst).count(), 2);
    assert_eq!(goods.iter().filter(|&&g| g == src).count(), 1);
}

#[test]
fn fish_and_mountains() {
    let Some(mut w) = roman02() else { return };
    let water = find(&w, |w, n, x, y| {
        w.layers.ground[n] & 0xf0 == 0 && s4_sim::spiral::walk(w.grid, x, y, 8).all(|(a, b)| w.layers.ground[w.grid.index(a as u32, b as u32)] & 0xf0 == 0)
    });
    if let Some((x, y)) = water {
        let sum = |w: &World| s4_sim::spiral::walk(w.grid, x, y, 15).map(|(a, b)| (w.layers.resource[w.grid.index(a as u32, b as u32)] & 0xf) as i32).sum::<i32>();
        let before = sum(&w);
        let (_, n) = cast(&mut w, 0, 2, x, y);
        eprintln!("fish at ({x}, {y}): {n}, {before} → {}", sum(&w));
        assert!(n > 0);
        let d = sum(&w) - before;
        assert!(d == n || (d == 0 && n == 1), "added {d}, counted {n}");
    }
    let mountain = find(&w, |w, n, _, _| w.layers.ground[n] & 0xf0 == 0x20 && (1..15).contains(&(w.layers.resource[n] & 0xf)));
    if let Some((x, y)) = mountain {
        let sum = |w: &World| s4_sim::spiral::walk(w.grid, x, y, 8).map(|(a, b)| (w.layers.resource[w.grid.index(a as u32, b as u32)] & 0xf) as i32).sum::<i32>();
        let before = sum(&w);
        let (_, n) = cast(&mut w, 0, 7, x, y);
        eprintln!("mountains at ({x}, {y}): {n}, {before} → {}", sum(&w));
        assert!(n >= w.rules.magic.limits_min[0][7].min(1));
        assert!(sum(&w) > before);
    }
}

#[test]
fn viking_animals_and_maya_crops() {
    let Some(mut w) = roman02() else { return };
    let (x, y) = find(&w, |w, n, x, y| {
        w.layers.ground[n] & 0xf0 == 0x10 && s4_sim::spiral::walk(w.grid, x, y, 6).all(|(a, b)| w.layers.ground[w.grid.index(a as u32, b as u32)] & 0xf0 == 0x10)
    })
    .expect("a grass field");
    let animals = |w: &World| w.entities.iter().filter(|(_, e)| e.obj_type == ObjType::Animal).count();
    let before = animals(&w);
    let (_, n) = cast(&mut w, 1, 2, x, y);
    eprintln!("animals at ({x}, {y}): {n}");
    // SpellLimits Viking 2: 4..8.
    assert!((1..=8).contains(&n));
    assert_eq!(animals(&w) - before, n as usize);
    // A planted grain field there: the Maya's crop spell makes it grow every 15 ticks.
    let (gx, gy) = (x + 1, y + 1);
    s4_sim::deco::plant(&mut w, gx, gy, 209);
    let plant = w.layers.deco_object[w.grid.index(gx as u32, gy as u32)];
    assert_ne!(plant, 0);
    let (_, n) = cast(&mut w, 2, 2, x, y);
    assert!(n >= 1);
    assert!(w.entities.by_index(plant as u32).unwrap().flags & s4_sim::deco::plant::FLAG_FAST != 0);
}

#[test]
fn maya_terrain() {
    let Some(mut w) = roman02() else { return };
    // A grass field far from anything else (the conversion stops near foreign ground).
    let (x, y) = find(&w, |w, n, x, y| {
        w.layers.ground[n] == 0x10 && s4_sim::spiral::walk(w.grid, x, y, 12).all(|(a, b)| w.layers.ground[w.grid.index(a as u32, b as u32)] == 0x10)
    })
    .expect("a grass field");
    let (r, kind) = cast(&mut w, 2, 3, x, y);
    assert_eq!((r, kind), (1, 3), "Maya: kind race + 1");
    let radius = w.rules.magic.radius(2, 3);
    for _ in 0..=radius + 1 {
        s4_sim::tick(&mut w, &[]);
    }
    let g = |w: &World, a: i32, b: i32| w.layers.ground[w.grid.index(a as u32, b as u32)];
    eprintln!("terrain at ({x}, {y}), radius {radius}: centre {:#x}", g(&w, x, y));
    // Level 3 (0x40) wherever the radius-3 spiral was all grass family; the rest stays grass.
    let converted = s4_sim::spiral::walk(w.grid, x, y, radius as u8).filter(|&(a, b)| g(&w, a, b) == 0x40).count();
    assert!(converted > 0 && g(&w, x, y) == 0x40, "{converted}");
    assert!(s4_sim::spiral::walk(w.grid, x, y, (radius + 3) as u8).all(|(a, b)| matches!(g(&w, a, b), 0x10 | 0x14 | 0x41 | 0x40) || g(&w, a, b) & 0xf0 == 0x10));
}

/// A free grass field of radius 6 (player 1 casts there; soldiers of both players are put on it).
fn field(w: &World) -> (i32, i32) {
    find(w, |w, n, x, y| {
        w.layers.ground[n] & 0xf0 == 0x10
            && s4_sim::spiral::walk(w.grid, x, y, 6).all(|(a, b)| {
                let m = w.grid.index(a as u32, b as u32);
                w.layers.ground[m] & 0xf0 == 0x10 && w.layers.flags[m] & 1 == 0 && w.layers.map_object[m] == 0
            })
    })
    .expect("a free field")
}

fn soldiers(w: &mut World, x: i32, y: i32, player: u8, kind: u16, n: usize) -> Vec<u16> {
    (0..n).filter_map(|i| s4_sim::create::add_settler(w, x + 2 * i as i32, y, player, kind)).collect()
}

fn health(w: &World, s: u16) -> Option<u8> {
    match w.entities.by_index(s as u32).map(|e| &e.data) {
        Some(EntityData::Settler(d)) => Some(d.health),
        _ => None,
    }
}

#[test]
fn roman_soldier_spells() {
    let Some(mut w) = roman02() else { return };
    let (x, y) = field(&w);
    let enemy = soldiers(&mut w, x - 2, y, 2, 29, 3);
    let own = soldiers(&mut w, x - 2, y + 2, 1, 29, 3);
    // Shield (spell 4): the own soldiers (allies) get 0x200000; a hit of 40 does 20.
    let (_, n) = cast(&mut w, 0, 4, x, y);
    assert_eq!(n, 3);
    assert!(own.iter().all(|&s| w.entities.by_index(s as u32).unwrap().flags & magic::spells::FLAG_SHIELD != 0));
    assert!(enemy.iter().all(|&s| w.entities.by_index(s as u32).unwrap().flags & magic::spells::FLAG_SHIELD == 0));
    let h = health(&w, own[0]).unwrap();
    s4_sim::combat::take_damage(&mut w, own[0], 40, 2);
    assert_eq!(health(&w, own[0]), Some(h - 20));
    // Convert (spell 6): the enemy swordsmen stand there as player 1's.
    let (_, n) = cast(&mut w, 0, 6, x, y);
    assert_eq!(n, 3);
    assert!(enemy.iter().all(|&s| w.entities.by_index(s as u32).is_none_or(|e| e.is_dead())));
    let mine = w.entities.iter().filter(|(_, e)| e.owner == 1 && e.kind == 29 && !e.is_dead() && s4_sim::spiral::grid_distance(e.x() - x, e.y() - y) <= 6).count();
    assert_eq!(mine, 6);
    // Lightning (spell 5) on new enemies: 100 damage kills a swordsman.
    let enemy = soldiers(&mut w, x - 2, y - 2, 2, 29, 2);
    let (_, n) = cast(&mut w, 0, 5, x, y);
    assert_eq!(n, 2);
    assert!(enemy.iter().all(|&s| w.entities.by_index(s as u32).is_none_or(|e| e.is_dead())));
    // The shield ends after the spell's duration.
    let d = w.rules.magic.duration(0, 4);
    for _ in 0..=d {
        s4_sim::tick(&mut w, &[]);
    }
    assert!(w.entities.by_index(own[1] as u32).is_some_and(|e| e.flags & magic::spells::FLAG_SHIELD == 0));
}

#[test]
fn viking_maya_trojan_soldier_spells() {
    let Some(mut w) = roman02() else { return };
    let (x, y) = field(&w);
    let enemy = soldiers(&mut w, x - 2, y, 2, 29, 2);
    let own = soldiers(&mut w, x - 2, y + 2, 1, 29, 2);
    // Viking freeze (4) on the enemy, bloodlust (5) on the own soldiers.
    assert_eq!(cast(&mut w, 1, 4, x, y).1, 2);
    assert!(enemy.iter().all(|&s| w.entities.by_index(s as u32).unwrap().flags & magic::spells::FLAG_FROZEN != 0));
    assert_eq!(cast(&mut w, 1, 5, x, y).1, 2);
    assert!(own.iter().all(|&s| w.entities.by_index(s as u32).unwrap().flags & magic::spells::FLAG_BLOODLUST != 0));
    // Maya upgrade (6): the own level-1 swordsmen become level 2 (type 30).
    assert_eq!(cast(&mut w, 2, 6, x, y).1, 2);
    let lvl2 = w.entities.iter().filter(|(_, e)| e.owner == 1 && e.kind == 30 && !e.is_dead() && s4_sim::spiral::grid_distance(e.x() - x, e.y() - y) <= 6).count();
    assert_eq!(lvl2, 2);
    // Maya bowmen (5): enemy bowmen vanish, each leaving a butterfly for the client.
    let bowmen = soldiers(&mut w, x - 2, y - 2, 2, 32, 2);
    w.events.clear();
    assert_eq!(cast(&mut w, 2, 5, x, y).1, 2);
    assert!(bowmen.iter().all(|&s| w.entities.by_index(s as u32).is_none_or(|e| e.is_dead())));
    assert_eq!(w.events.iter().filter(|e| matches!(e, s4_sim::event::SimEvent::Butterfly { .. })).count(), 2);
    // Trojan disarm (6): the enemies become player 2's carriers on a sword each.
    assert_eq!(cast(&mut w, 4, 6, x, y).1, 2);
    let near: Vec<_> = w.entities.iter().filter(|(_, e)| e.owner == 2 && e.kind == 1 && s4_sim::spiral::grid_distance(e.x() - x, e.y() - y) <= 6).map(|(id, e)| (id, e.x(), e.y(), e.flags, e.is_dead())).collect();
    eprintln!("carriers {near:?}");
    let carriers = near.iter().filter(|c| !c.4).count();
    assert_eq!(carriers, 2);
    let swords = piles_near(&w, x, y, 6).iter().filter(|p| p.1 == 0x22).count();
    assert_eq!(swords, 2);
}

#[test]
fn viking_strike_and_maya_stones() {
    let Some(mut w) = roman02() else { return };
    // The first enemy building on the spiral around player 2's castle takes 300 after 25 ticks.
    let (b, bx, by) = w.entities.iter().find(|(_, e)| e.owner == 2 && e.obj_type == ObjType::Building).map(|(id, e)| (id, e.x(), e.y())).expect("an enemy building");
    let h0 = match &w.entities.by_index(b as u32).unwrap().data {
        EntityData::Building(d) => d.health,
        _ => unreachable!(),
    };
    assert_eq!(cast(&mut w, 1, 7, bx, by).1, 1);
    for _ in 0..30 {
        s4_sim::tick(&mut w, &[]);
    }
    let h1 = w.entities.by_index(b as u32).and_then(|e| match &e.data {
        EntityData::Building(d) => Some(d.health),
        _ => None,
    });
    eprintln!("strike: building {b} health {h0} → {h1:?}");
    assert!(h1.is_none_or(|h| h < h0) || w.entities.by_index(b as u32).is_none_or(|e| e.is_dead()));
    // Maya stones (7) on a grass field.
    let (x, y) = field(&w);
    let stones = |w: &World| s4_sim::spiral::walk(w.grid, x, y, 10).filter(|&(a, b)| w.layers.resource[w.grid.index(a as u32, b as u32)] & 0xf0 == 0x60).count();
    let before = stones(&w);
    let (_, n) = cast(&mut w, 2, 7, x, y);
    eprintln!("stones: {n}");
    assert!(n > 0);
    assert_eq!(stones(&w) - before, n as usize);
}
