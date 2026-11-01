//! The statistic counters and history in a running game (roman02, `tick()` only): the per-type
//! settler counts follow the settler entities exactly (`AddSettler`, `ChangeType`, removal), and
//! the archive samples at tick 1 and every 840 ticks (0x4f2720 → 0x4f9330).

use s4_sim::World;
use s4_sim::entity::ObjType;
use s4_sim::statistic::{CATEGORIES, SAMPLES};

fn roman02() -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map");
    if !p.exists() {
        return None;
    }
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    let rules = s4_sim::Rules::load_default().ok()?;
    s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).ok().map(|(w, _)| w)
}

fn entity_counts(w: &World, player: u8) -> Vec<i32> {
    let mut v = vec![0; 67];
    for (_, e) in w.entities.iter() {
        if e.obj_type == ObjType::Settler && e.owner == player && !e.is_dead() && (e.kind as usize) < 67 {
            v[e.kind as usize] += 1;
        }
    }
    v
}

#[test]
fn counters_and_history_follow_the_game() {
    let Some(mut w) = roman02() else { return };
    for t in 1..=1700u32 {
        s4_sim::tick(&mut w, &[]);
        if t % 300 == 0 {
            for p in 1..=w.player_count() {
                assert_eq!(w.statistic.players[p as usize].settlers, entity_counts(&w, p), "player {p} at tick {}", w.tick);
                let mgr: Vec<i32> = (0..67).map(|t| w.settler_lists.settlers(p, t)).collect();
                assert_eq!(mgr, entity_counts(&w, p), "CSettlerMgr count, player {p}");
            }
        }
    }
    let a = &w.statistic.archive;
    // Samples at ticks 1, 840 and 1680: ring 0 at slot 2, ring 1 at slot 0 (steps after 3), ring 2
    // at slot 0.
    assert_eq!((a.rings[0].index, a.rings[1].index, a.rings[2].index), (2, 0, 0));
    let p = &w.statistic.players[1];
    eprintln!(
        "player 1: land {} % (max {}), value {}, civilians {}, soldiers {}, settlers {}",
        p.land, p.land_max, p.value, p.civilians, p.soldiers, p.settlers_total
    );
    assert!(p.land > 0 && p.value > 0 && p.civilians > 0);
    let civ = a.rings[0].slots(1, 2);
    assert!(civ[0] > 0 && civ[1] > 0 && civ[2] > 0 && civ[3..].iter().all(|&v| v == 0), "{civ:?}");
    // Ring 1's single slot holds the latest of its three samples.
    assert_eq!(a.rings[1].slots(1, 2)[0], civ[2]);
    assert_eq!(a.rings[0].data.len(), 9 * CATEGORIES * SAMPLES);
}

fn roman02_with(setup: &s4_sim::NewGame) -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map");
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    let rules = s4_sim::Rules::load_default().ok()?;
    s4_sim::new_game(&map, rules, setup).ok().map(|(w, _)| w)
}

/// 0x4f8450's two setup branches: game type 3 quarters the strength; a computer player (control
/// 3: an AI at level 2) counts its buildings once more instead of its eye catchers twice.
#[test]
fn strength_game_type_and_computer_players() {
    let Some(plain) = roman02() else { return };
    let mut setup = s4_sim::NewGame { game_type: 3, ..Default::default() };
    let Some(quartered) = roman02_with(&setup) else { return };
    setup.game_type = 0;
    setup.control[1] = 3;
    let Some(mut ai) = roman02_with(&setup) else { return };
    assert!(ai.ai.is_ai(2));
    // The map's buildings count before the AI players exist (0x4ff5f0 is called at 0x4fdbda,
    // after the map load): the start strength is the plain one until the next recompute.
    assert_eq!(ai.statistic.players[2].foreign, plain.statistic.players[2].foreign);
    s4_sim::statistic::recompute(&mut ai, 2);
    for p in 1..=2u8 {
        let (a, q) = (&plain.statistic.players[p as usize], &quartered.statistic.players[p as usize]);
        assert!(a.foreign > 0);
        assert_eq!(q.foreign, if a.foreign < 4 { 1 } else { a.foreign / 4 }, "player {p}: quartered");
        assert_eq!(q.own, s4_sim::statistic::own_strength(q.foreign));
        assert_eq!((q.own_max, q.foreign_max), (q.own.max(0x100), q.foreign), "player {p}: maxima");
    }
    let (h, c) = (&plain.statistic.players[2], &ai.statistic.players[2]);
    let sum = h.cost[1] + 2 * h.cost[2] + h.cost[0];
    let eye = 2 * (h.eye_cost[1] + 2 * h.eye_cost[2] + h.eye_cost[0]);
    assert!(sum > 0 && sum != eye, "player 2's buildings tell the two sums apart");
    let div = plain.rules.logic.fighting_strength_divisor.max(1);
    let sets = (64..76).map(|k| h.count[k]).min().unwrap();
    assert_eq!(h.foreign, s4_sim::statistic::foreign_strength(h.over, h.base, sum, eye, sets, div, false));
    assert_eq!(c.foreign, s4_sim::statistic::foreign_strength(h.over, h.base, sum, sum, sets, div, false));
    assert_ne!(c.foreign, h.foreign);
    // Player 1 is not a computer player: unchanged.
    assert_eq!(ai.statistic.players[1].foreign, plain.statistic.players[1].foreign);
}
