//! Donkeys (`CDonkeyRole`, release vtable 0x105ae90, role 20) on MD_roman3, driven by `tick()`
//! only: the donkey ranch breeds a donkey, which leaves through the gate and waits with the
//! flags of `CDonkeyRole::Init` (0x576300). Orders (trading, vehicles) are not ported. Skipped
//! when the game data is not present.

use s4_sim::World;
use s4_sim::entity::{EntityData, ObjType};

const DONKEY: u16 = 60;

fn map() -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("MD_roman3.map");
    if !p.exists() {
        return None;
    }
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    let rules = s4_sim::Rules::load_default().ok()?;
    s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).ok().map(|(w, _)| w)
}

/// (ranch, delivery piles, animals, gate) of a donkey ranch (type 13).
type Ranch = (u16, Vec<u16>, u8, (i32, i32));

fn ranches(w: &World) -> Vec<Ranch> {
    w.buildings
        .roles
        .iter()
        .filter_map(|(&id, r)| match &r.kind {
            s4_sim::buildings::RoleKind::WorkUp(s) if r.base.house_kind == 3 && w.entities.by_index(id as u32).is_some_and(|e| e.kind == 13) => {
                let gate = match &w.entities.by_index(id as u32)?.data {
                    EntityData::Building(b) => (b.ensign.x(), b.ensign.y()),
                    _ => return None,
                };
                Some((id, s.delivery.clone(), s.animals, gate))
            }
            _ => None,
        })
        .collect()
}

fn donkeys_at(w: &World, (x, y): (i32, i32)) -> Vec<u16> {
    let n = w.grid.index(x as u32, y as u32);
    let id = w.layers.map_object[n];
    w.entities.by_index(id as u32).filter(|e| e.obj_type == ObjType::Settler && e.kind == DONKEY).map(|_| vec![id]).unwrap_or_default()
}

/// The donkey ranch's rules (0x533ff0), checked every tick while the role sets the pace: the
/// delivery piles are kept stocked (`IncreaseUnforeseen`), so every round has its grain and
/// water. Per round, with 25 %: a pen of more than 2 animals lets one out as a donkey at the
/// gate, a smaller one grows by one. `extra` RNG draws at the start shift every random decision
/// on the map. Returns the state hash and the donkeys bred.
fn run(ticks: u32, extra: u32) -> Option<(u64, Vec<u16>)> {
    let mut w = map()?;
    for _ in 0..extra {
        w.rng.rand();
    }
    let start = ranches(&w);
    assert!(!start.is_empty(), "no donkey ranch");
    let mut before: Vec<u8> = start.iter().map(|r| r.2).collect();
    let mut bred = Vec::new();
    for t in 1..=ticks {
        for (_, delivery, _, _) in ranches(&w) {
            for p in delivery {
                if s4_sim::pile::available(&w, p) < 2 {
                    s4_sim::pile::increase_unforeseen(&mut w, p, 2);
                }
            }
        }
        let gates: Vec<Vec<u16>> = ranches(&w).iter().map(|r| donkeys_at(&w, r.3)).collect();
        s4_sim::tick(&mut w, &[]);
        assert!(
            !w.events.iter().any(|e| matches!(e, s4_sim::event::SimEvent::Unported { what, .. } if what.contains("CDonkeyRole"))),
            "a donkey got an order"
        );
        for (k, (id, _, animals, gate)) in ranches(&w).into_iter().enumerate() {
            let was = before[k];
            let new: Vec<u16> = donkeys_at(&w, gate).into_iter().filter(|d| !gates[k].contains(d) && !bred.contains(d)).collect();
            match (was, animals, new.len()) {
                (a, b, 1) if a > 2 && b == a - 1 => bred.extend(new),
                (a, b, 0) if b == a || (b == a + 1 && a <= 2) => {}
                other => panic!("ranch {id} tick {t}: (animals before, after, new donkeys) {other:?}"),
            }
            before[k] = animals;
        }
    }
    for &id in &bred {
        let Some(e) = w.entities.by_index(id as u32).filter(|e| e.kind == DONKEY && !e.is_dead()) else { continue };
        let EntityData::Settler(s) = &e.data else { continue };
        // The idle donkeys as the saves have them: role 20 on the wait list (60), WAIT, and of
        // the flags Init touches only 0x4000 (with READY 0x1000 and VISIBLE 0x100).
        assert_eq!((s.job.role, s.job.list_id, s.job.task), (20, DONKEY as i16, 17), "donkey {id}");
        assert_eq!(e.flags & 0x3f300, 0x5100, "donkey {id} flags {:x}", e.flags);
    }
    Some((w.state_hash(), bred))
}

/// With grain and water always there, a round in four grows the pen until it holds 3 and then
/// lets a donkey out, which waits as the saved donkeys do. (The first donkey needs 3 successes
/// at 25 %; 12,000 ticks gave 2 to 8 donkeys in every run tried.)
#[test]
fn ranch_donkeys_wait_as_the_original() {
    let Some(a) = run(12000, 0) else { return };
    eprintln!("hash {:x}, donkeys {:?}", a.0, a.1);
    assert!(!a.1.is_empty(), "{a:?}");
    assert_eq!(Some(a), run(12000, 0), "not deterministic");
}

/// The same rules hold however the map's random decisions fall.
#[test]
fn ranch_rules_hold_under_other_draws() {
    for extra in 1..=4 {
        let Some(a) = run(12000, extra) else { return };
        eprintln!("{extra}: donkeys {:?}", a.1);
        assert!(!a.1.is_empty(), "{extra}: {a:?}");
    }
}
