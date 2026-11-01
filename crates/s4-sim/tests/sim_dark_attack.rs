//! `CAIAgentDarkTribeAttack` (0x42cf00, `s4_sim::ai::dark_attack`) on AO_trojan04: the Dark
//! Tribe (control 3) raids player 1 by manakopter once `AttackDelay` has passed. Player 1 gets a
//! storage area (a target the shaman AI 0x4440d0 accepts for boards and iron). No oracle (an AI
//! save would settle the timings); this checks the raid's course and determinism. Skipped
//! without the game data.

use s4_sim::World;
use s4_sim::entity::ObjType;

fn game() -> Option<(World, u8)> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("AO_trojan04.map");
    let (Ok(map), Ok(rules)) = (s4_formats::map::MapFile::open(&p), s4_sim::Rules::load_default()) else { return None };
    let (w, _) = s4_sim::new_game(&map, rules.clone(), &s4_sim::NewGame::default()).ok()?;
    let dt = (1..=8u8).find(|&p| w.player(p).is_some_and(|q| q.race == 3))?;
    let mut setup = s4_sim::NewGame::default();
    setup.control[dt as usize - 1] = 3;
    let (mut w, _) = s4_sim::new_game(&map, rules, &setup).ok()?;
    let (x, y) = w.entities.iter().find(|(_, e)| e.owner == 1 && e.obj_type == ObjType::Settler && e.kind == 1).map(|(_, e)| (e.x(), e.y()))?;
    assert!(s4_sim::build::scripted::add_building_scripted(&mut w, x, y, 1, 0x22, 10) != 0, "a storage area of player 1");
    Some((w, dt))
}

/// The attack agent's saved ints: step, players, strength, x, y, manakopters, next.
fn agent(w: &World, dt: u8) -> Vec<i32> {
    let a = s4_sim::ai::player::get(w, dt).unwrap();
    a.body.sectors.iter().find_map(|s| match &s.kind {
        s4_sim::ai::model::SectorKind::Dark { agents } => Some(agents[0].extra.clone()),
        _ => None,
    }).expect("a Dark Tribe sector AI")
}

fn slaves(w: &World, dt: u8) -> usize {
    w.entities.iter().filter(|(_, e)| e.obj_type == ObjType::Settler && e.owner == dt && e.kind == 0x37 && !e.is_dead()).count()
}

#[test]
fn the_dark_tribe_raids_by_manakopter() {
    let Some((mut a, dt)) = game() else { return };
    let (mut b, _) = game().unwrap();
    let delay = a.ai.vars.get(dt, 1) as u32 * 840;
    let mut steps = vec![0];
    let mut landed_near = false;
    let slaves0 = slaves(&a, dt);
    for t in 1..=60_000u32 {
        s4_sim::tick(&mut a, &[]);
        s4_sim::tick(&mut b, &[]);
        let st = agent(&a, dt);
        if *steps.last().unwrap() != st[0] {
            if st[0] == 1 {
                assert!(t > delay, "no raid before AttackDelay · 840 ({t})");
            }
            if st[0] == 3 {
                // The manakopters have come down at the target (spread: x − 0..15, y + 0..15, then
                // the first fitting node of a spiral).
                let (x, y) = (st[3], st[4]);
                landed_near = s4_sim::flying::list(&a, dt, 1).iter().all(|&m| {
                    let e = a.entities.by_index(m as u32).unwrap();
                    s4_sim::spiral::grid_distance(e.x() - x, e.y() - y) <= 40
                });
            }
            steps.push(st[0]);
            if st[0] == 0 {
                break;
            }
        }
        if t % 2000 == 0 {
            assert_eq!(a.state_hash(), b.state_hash(), "tick {t}");
        }
    }
    assert_eq!(steps, [0, 1, 2, 3, 4, 5, 6, 7, 0], "a whole flying raid");
    assert!(landed_near, "the manakopters landed at the target");
    assert!(slaves(&a, dt) > slaves0, "the shamans brought slaves home");
    let st = agent(&a, dt);
    assert_eq!((st[1], st[2], st[5]), (0, -1, 0), "reset");
    assert!(st[6] as u32 > a.ai.tick, "the next raid waits DTAttackCheckDelay");
    assert_eq!(a.state_hash(), b.state_hash());
}
