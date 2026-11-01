//! The player AI frame (stage S1) on MCD2_roman1 with player 2 a computer player (control 3):
//! the start (0x43b5f0: its units into reservoir 0, 0x43a7f0; sector AIs at its castles and
//! towers, 0x439e70), the per-tick distribution of reservoir 0 to the sector AIs' slots by
//! warrior type (0x43a9f0), the event dispatch (0x43a420) and the chunk-150 round trip of a
//! fresh AI. Skipped when the game data is not present.

use s4_sim::World;
use s4_sim::ai::player::TfRef;

fn world() -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("MCD2_roman1.map");
    if !p.exists() {
        return None;
    }
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    let rules = s4_sim::Rules::load_default().ok()?;
    let mut setup = s4_sim::NewGame::default();
    setup.control[1] = 3;
    s4_sim::new_game(&map, rules, &setup).ok().map(|(w, _)| w)
}

/// Every member is where its info says, and the other way round.
fn consistent(w: &World) {
    let a = s4_sim::ai::player::get(w, 2).expect("player 2 AI");
    let mut seen = 0;
    let mut check = |members: &[u16], r: TfRef| {
        for &m in members {
            assert_eq!(w.ai.infos.get(&m), Some(&r), "member {m}");
            seen += 1;
        }
    };
    for (k, t) in a.body.reservoirs.iter().enumerate() {
        check(&t.members, TfRef::Reservoir(2, k as u8));
    }
    check(&a.body.squad.members, TfRef::Squad(2));
    for (s, sec) in a.body.sectors.iter().enumerate() {
        if let s4_sim::ai::model::SectorKind::Normal { forces, .. } = &sec.kind {
            for (k, t) in forces.iter().enumerate() {
                check(&t.members, TfRef::Force(2, s as u8, k as u8));
            }
        }
        for it in &sec.group.items {
            check(&it.tf.members, TfRef::Group(2, s as u8, it.uid));
        }
        for (k, t) in sec.slots.iter().enumerate() {
            if let Some((_, t)) = t {
                check(&t.members, TfRef::Slot(2, s as u8, k as u8));
                for &m in &t.members {
                    let e = w.entities.by_index(m as u32).unwrap();
                    assert_eq!((e.flags & 0xf) as usize, k, "unit {m} in the slot of its warrior type");
                }
            }
        }
    }
    assert_eq!(seen, w.ai.infos.len());
}

#[test]
fn start_and_distribution() {
    let Some(mut w) = world() else { return };
    assert!(w.ai.is_ai(2) && !w.ai.is_ai(1));
    let a = s4_sim::ai::player::get(&w, 2).unwrap();
    let start = a.body.reservoirs[0].members.len();
    let sectors = a.body.sectors.len();
    eprintln!("player 2: {start} units in reservoir 0, {sectors} sector AIs");
    assert!(start > 0, "its soldiers join reservoir 0");
    assert!((1..=8).contains(&sectors), "sector AIs at its towers");
    consistent(&w);
    s4_sim::tick(&mut w, &[]);
    consistent(&w);
    let a = s4_sim::ai::player::get(&w, 2).unwrap();
    let in_slots: usize = a.body.sectors.iter().flat_map(|s| s.slots.iter().flatten()).map(|(_, t)| t.members.len()).sum();
    // The first tick's agents (newest first: the last sector AI's tower occupation, …, squad
    // management) may already have taken slot units into squads.
    let in_groups: usize = a.body.sectors.iter().flat_map(|s| s.group.items.iter()).map(|i| i.tf.members.len()).sum();
    assert!(in_slots + in_groups > 0, "units on a sector AI's sector move to its slots");
    assert_eq!(in_slots + in_groups + a.body.reservoirs[0].members.len(), start, "none lost");
    for _ in 0..300 {
        s4_sim::tick(&mut w, &[]);
    }
    consistent(&w);

    // Saved and loaded again: the same chunk.
    let c = s4_sim::ai::chunk::save(&w);
    let bytes = s4_sim::ai::chunk::write(&c);
    let back = s4_sim::ai::chunk::parse(&bytes).unwrap();
    let mut w2 = w.clone();
    s4_sim::ai::chunk::load(&mut w2, &back).unwrap();
    assert_eq!(s4_sim::ai::chunk::write(&s4_sim::ai::chunk::save(&w2)), bytes);
    // A load numbers the group's task forces anew (s4rs ids); by their place in the group the
    // infos are the same.
    let norm = |w: &World| -> Vec<(u16, String)> {
        let a = s4_sim::ai::player::get(w, 2).unwrap();
        w.ai.infos
            .iter()
            .map(|(&id, r)| {
                let r = match *r {
                    TfRef::Group(p, s, uid) => format!("G{p}/{s}/{}", a.body.sectors[s as usize].group.items.iter().position(|i| i.uid == uid).unwrap()),
                    r => format!("{r:?}"),
                };
                (id, r)
            })
            .collect()
    };
    assert_eq!(norm(&w2), norm(&w));
}

/// `AI.NewSquad(2, 8, x, y)` (event 0x1c): reservoir 2's units get the move order and leave it.
#[test]
fn new_squad_moves_reservoir_2() {
    let Some(mut w) = world() else { return };
    s4_sim::tick(&mut w, &[]);
    let units: Vec<u16> = w.ai.infos.keys().copied().take(3).collect();
    for &u in &units {
        s4_sim::ai::player::add_to(&mut w, u, TfRef::Reservoir(2, 2));
    }
    consistent(&w);
    let (x, y) = (100, 120);
    s4_sim::ai::post_event(&mut w, 0x1c, 2, 8, (y << 16) | x);
    s4_sim::tick(&mut w, &[]);
    let a = s4_sim::ai::player::get(&w, 2).unwrap();
    assert!(a.body.reservoirs[2].members.is_empty());
    for u in units {
        assert!(!w.ai.infos.contains_key(&u));
    }
    consistent(&w);
}

#[test]
fn deterministic() {
    let run = || {
        let mut w = world()?;
        for _ in 0..500 {
            s4_sim::tick(&mut w, &[]);
        }
        Some(w.state_hash())
    };
    assert_eq!(run(), run());
}

/// The player AI's scheduler (0x42e760 / 0x42e7e0 / 0x42e9b0): the ctors register every agent at
/// tick 0 with priority 8, so the list starts newest first (the last sector AI's tower
/// occupation … the global suicide mission). Each costs 0x100 of the budget 0x1ff, so one agent
/// runs per tick, and comes back after its period.
#[test]
fn scheduler_one_agent_per_tick() {
    use s4_sim::ai::model::NORMAL_AGENTS;
    use s4_sim::ai::sched::{AgentRef, NORMAL_ORDER, Agents};
    let Some(mut w) = world() else { return };
    let a = s4_sim::ai::player::get(&w, 2).unwrap();
    let mut want = vec![AgentRef::GlobalSuicide];
    for s in 0..a.body.sectors.len() as u8 {
        for k in NORMAL_ORDER {
            want.insert(0, AgentRef::Sector(s, k));
        }
    }
    assert_eq!(a.sched.queue, want);
    let period = |r: AgentRef| match r {
        AgentRef::Sector(_, k) => NORMAL_AGENTS[k as usize].2,
        _ => 0x96,
    };
    for &r in &want {
        let g = a.body.get(r).unwrap();
        assert_eq!((g.next, g.period, g.cost, g.prio), (0, period(r), 0x100, 8), "{r:?}");
    }
    // They run strictly in that order. An agent's `Execute` may return its own delay and cost
    // (bits 12.. / 0..11): the moving home and the suicide missions without task forces return
    // period · 10 at cost 1, so the next agent runs in the same tick; the default cost 0x100
    // ends the tick's budget after one more.
    let mut ran = Vec::new();
    for _ in 0..want.len() {
        s4_sim::tick(&mut w, &[]);
        let a = s4_sim::ai::player::get(&w, 2).unwrap();
        let now = w.ai.tick;
        let new: Vec<AgentRef> = want.iter().copied().filter(|&q| a.body.get(q).unwrap().next != 0 && !ran.contains(&q)).collect();
        assert!(!new.is_empty() || ran.len() == want.len(), "one agent at least ran at {now}");
        for q in new {
            assert!(a.body.get(q).unwrap().next > now, "{q:?} rescheduled after {now}");
            ran.push(q);
        }
    }
    assert_eq!(ran, want, "the start order");
    for &r in &want {
        let a = s4_sim::ai::player::get(&w, 2).unwrap();
        let next = a.body.get(r).unwrap().next;
        assert!(next >= period(r).min(w.ai.tick), "{r:?} at {next}");
    }
    // The list stays ordered by the next tick.
    let a = s4_sim::ai::player::get(&w, 2).unwrap();
    let nexts: Vec<u32> = a.sched.queue.iter().map(|&q| a.body.get(q).unwrap().next).collect();
    assert!(nexts.windows(2).all(|p| p[0] <= p[1]), "{nexts:?}");
}

/// `CAIMain`'s scheduler: the player evaluation (0x430fb0) every 0xf ticks, on to the next player.
#[test]
fn evaluation_every_15_ticks() {
    let Some(mut w) = world() else { return };
    let players = w.player_count() as i32;
    assert!(players >= 2);
    s4_sim::tick(&mut w, &[]);
    let t0 = w.ai.tick;
    assert_eq!((w.ai.evaluated, w.ai.evaluation.next), (2, t0 + 15));
    for k in 1..=3 * players {
        for _ in 0..15 {
            s4_sim::tick(&mut w, &[]);
        }
        assert_eq!(w.ai.evaluation.next, t0 + 15 * (k as u32 + 1));
        assert_eq!(w.ai.evaluated, (1 + k) % players + 1);
    }
}
