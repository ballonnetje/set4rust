//! `CAnimalRanchRole` (release Init 0x533f50, LogicUpdate 0x533ff0; the other slots are the
//! workup role's) on MCD2_roman1, driven by `tick()` only: the animal ranches order their farmers
//! to feed the animals (grain and water from the delivery piles), the pen fills up to 6 animals,
//! and from then on every feeding round puts one animal on the production pile. Skipped when the
//! game data is not present.

use s4_sim::World;
use s4_sim::buildings::RoleKind;
use s4_sim::entity::EntityData;

fn map() -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("MCD2_roman1.map");
    if !p.exists() {
        return None;
    }
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    let rules = s4_sim::Rules::load_default().ok()?;
    let (mut w, _) = s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).ok()?;
    let towers: Vec<(i32, i32)> = w
        .entities
        .iter()
        .filter_map(|(id, e)| match &e.data {
            EntityData::Building(_) if s4_sim::buildings::have_inhabitant(&w, id) => Some((e.x(), e.y())),
            _ => None,
        })
        .collect();
    for (x, y) in towers {
        s4_sim::territory::set_owner(&mut w, x, y);
    }
    Some(w)
}

/// (ranch, production pile, delivery piles, animals) of every animal ranch (house kind 3).
fn ranches(w: &World) -> Vec<(u16, u16, Vec<u16>, u8)> {
    w.buildings
        .roles
        .iter()
        .filter_map(|(&id, r)| match &r.kind {
            RoleKind::WorkUp(s) if r.base.house_kind == 3 => Some((id, s.product, s.delivery.clone(), s.animals)),
            _ => None,
        })
        .collect()
}

/// What a run saw.
#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    hash: u64,
    /// Most animals in a pen.
    most: u8,
    /// Animals put on the production piles.
    made: u32,
}

/// The ranch's rules, checked every tick while the role (not the map's economy) sets the pace:
/// the delivery piles are kept stocked (`IncreaseUnforeseen`) and the production pile is
/// emptied (`DecreaseUnforeseen`), so every round has its grain and water and room for an animal. `extra` RNG draws
/// at the start shift every random decision on the map (as any other change does).
fn run(ticks: u32, extra: u32) -> Option<Outcome> {
    let mut w = map()?;
    for _ in 0..extra {
        w.rng.rand();
    }
    let start = ranches(&w);
    assert!(!start.is_empty(), "no animal ranch");
    assert!(start.iter().all(|r| r.3 == 1), "a ranch starts with one animal: {start:?}");
    let amount = |w: &World, p: u16| s4_sim::pile::get(w, p).map_or(0, |p| p.amount);
    let mut before: Vec<(u8, u8)> = start.iter().map(|r| (r.3, amount(&w, r.1))).collect();
    let (mut most, mut made) = (0, 0);
    for t in 1..=ticks {
        for (_, _, delivery, _) in ranches(&w) {
            for p in delivery {
                if s4_sim::pile::available(&w, p) < 2 {
                    s4_sim::pile::increase_unforeseen(&mut w, p, 2);
                }
            }
        }
        s4_sim::tick(&mut w, &[]);
        for (k, (id, product, _, animals)) in ranches(&w).into_iter().enumerate() {
            let (was, pile_was) = before[k];
            let now = amount(&w, product);
            let delivered = now as i32 - pile_was as i32;
            assert!((1..=6).contains(&animals), "ranch {id} tick {t}: {animals} animals");
            // (Carriers may take reserved animals off the pile: only increases count.)
            let delivered = delivered.max(0);
            assert!(delivered <= 1, "ranch {id} tick {t}: {delivered} animals delivered at once");
            // A full pen gives one animal to the pile; a pen that is not full only grows.
            match (was, animals, delivered) {
                (6, 5, 1) => made += 1,
                (a, b, 0) if b == a || (b == a + 1 && a < 6) => {}
                other => panic!("ranch {id} tick {t}: (animals before, after, delivered) {other:?}"),
            }
            most = most.max(animals);
            // Room for the next animal.
            s4_sim::pile::decrease_unforeseen(&mut w, product, now as i32);
            before[k] = (animals, amount(&w, product));
        }
    }
    Some(Outcome { hash: w.state_hash(), most, made })
}

/// With grain, water and room always there, a round (the farmer's feeding list, about 330
/// ticks) grows a pen below 6 with 77 % and takes one animal from a full pen: about one
/// delivery per 1.3 rounds once the pen is full (14 in 6,000 ticks here).
#[test]
fn ranch_raises_animals_and_delivers_them() {
    let Some(a) = run(6000, 0) else { return };
    eprintln!("{a:?}");
    assert_eq!(a.most, 6, "the pen fills up");
    assert!(a.made >= 8, "{a:?}");
    assert_eq!(Some(a), run(6000, 0), "not deterministic");
}

/// The same rules hold however the map's random decisions fall.
#[test]
fn ranch_rules_hold_under_other_draws() {
    for extra in 1..=4 {
        let Some(a) = run(6000, extra) else { return };
        eprintln!("{extra}: {a:?}");
        assert_eq!(a.most, 6, "{extra}: {a:?}");
        assert!(a.made >= 8, "{extra}: {a:?}");
    }
}
