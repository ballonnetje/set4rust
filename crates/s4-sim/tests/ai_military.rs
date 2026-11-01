//! The normal computer players' military (stage S2): evaluations, squads, attacks. Cordoba (four
//! players on one land mass, towers and armies preplaced) with every player a computer player
//! (control 3, level 2). `AttackDelay` (45 minutes at level 2) is set to 10 minutes as
//! `AI.SetPlayerVar` would, so the attacks start within the test. No AI save exists to compare
//! with; these check that the ported agents act as their port says, that the AI's bookkeeping
//! stays consistent and that two worlds in lockstep stay equal. Skipped without the game data.

use s4_sim::World;
use s4_sim::ai::model::SectorKind;
use s4_sim::ai::player::TfRef;

fn game(map: &str) -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Singleplayer").join(map);
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    let setup = s4_sim::NewGame { control: [3; 8], ..Default::default() };
    let (mut w, _) = s4_sim::new_game(&map, s4_sim::Rules::load_default().ok()?, &setup).ok()?;
    for p in 1..=8usize {
        let level = w.ai.vars.records[p].level as usize;
        w.ai.vars.records[p].columns[level][1] = 10;
    }
    Some(w)
}

/// Every task-force member's info names that task force, and every info a task force that holds
/// the entity.
fn consistent(w: &World) {
    let mut seen = 0;
    for p in 1..=8u8 {
        let Some(a) = s4_sim::ai::player::get(w, p) else { continue };
        let mut check = |members: &[u16], r: TfRef| {
            for &m in members {
                assert_eq!(w.ai.infos.get(&m), Some(&r), "member {m}");
                seen += 1;
            }
        };
        for (k, t) in a.body.reservoirs.iter().enumerate() {
            check(&t.members, TfRef::Reservoir(p, k as u8));
        }
        check(&a.body.squad.members, TfRef::Squad(p));
        for (s, sec) in a.body.sectors.iter().enumerate() {
            for (k, t) in sec.slots.iter().enumerate() {
                if let Some((_, t)) = t {
                    check(&t.members, TfRef::Slot(p, s as u8, k as u8));
                }
            }
            if let SectorKind::Normal { forces, .. } = &sec.kind {
                for (k, t) in forces.iter().enumerate() {
                    check(&t.members, TfRef::Force(p, s as u8, k as u8));
                }
            }
            for it in &sec.group.items {
                check(&it.tf.members, TfRef::Group(p, s as u8, it.uid));
            }
        }
    }
    assert_eq!(seen, w.ai.infos.len(), "every info is a member");
}

fn military(w: &World) -> Vec<(u16, u8)> {
    w.entities.iter().filter(|(_, e)| e.obj_type == s4_sim::entity::ObjType::Building && (46..=48).contains(&e.kind)).map(|(i, e)| (i, e.owner)).collect()
}

#[test]
fn ai_players_guard_evaluate_and_attack() {
    let Some(mut w) = game("Cordoba.map") else { return };
    let mut b = game("Cordoba.map").unwrap();
    let ai: Vec<u8> = (1..=8).filter(|&p| s4_sim::ai::player::get(&w, p).is_some_and(|a| !a.dark && !a.body.sectors.is_empty())).collect();
    assert!(ai.len() >= 2, "computer players with sector AIs: {ai:?}");
    let start = military(&w);
    let (mut squads, mut targets, mut staged, mut charged, mut geologists) = (false, false, false, false, false);
    for t in 1..=20_000u32 {
        s4_sim::tick(&mut w, &[]);
        s4_sim::tick(&mut b, &[]);
        if t % 50 == 0 {
            for &p in &ai {
                let a = s4_sim::ai::player::get(&w, p).unwrap();
                for (s, sec) in a.body.sectors.iter().enumerate() {
                    let SectorKind::Normal { caches, agents, .. } = &sec.kind else { continue };
                    squads |= sec.group.items.iter().any(|i| i.kind == 5 && !i.tf.members.is_empty());
                    targets |= caches[3].count > 0;
                    staged |= sec.group.items.iter().any(|i| i.kind == 6 && i.tf.command() == 3 && !i.tf.members.is_empty());
                    charged |= agents[0].extra[0] == 2 && sec.group.items.iter().any(|i| i.kind == 6 && i.tf.command() == 4);
                    // `CAIAgentGeologists`: reservoir 1's geologists sent out (their info's tick).
                    if let SectorKind::Normal { forces, .. } = &sec.kind {
                        geologists |= forces[1].members.iter().any(|m| w.ai.ext.get(m).is_some_and(|x| x.tick > 0));
                    }
                    // Goal caches sorted, highest first, within 31.
                    for c in caches {
                        let e = c.entries();
                        assert!(e.len() <= 31 && e.windows(2).all(|x| x[0].2 >= x[1].2), "player {p} sector AI {s}: {e:?}");
                    }
                }
            }
        }
        if t % 1000 == 0 {
            consistent(&w);
            assert_eq!(w.state_hash(), b.state_hash(), "tick {t}");
        }
    }
    assert!(squads, "squads (type 5) guard the buildings");
    assert!(targets, "the offence evaluation lists enemy buildings (goal cache 3)");
    assert!(staged, "attackers (type 6) walk to the edge of the enemy land (order 3)");
    assert!(charged, "the attack charges (order 4)");
    assert!(geologists, "geologists are sent out");
    let end = military(&w);
    let changed = start.iter().filter(|&&(id, owner)| end.iter().find(|&&(i, _)| i == id).is_none_or(|&(_, o)| o != owner)).count();
    eprintln!("military buildings: {} at the start, {} lost or captured", start.len(), changed);
    assert!(changed > 0, "some military building fell");
    // Captures count once for the winner and once for the loser (statistic +0x374 / +0x4c0).
    let won: i32 = w.statistic.players.iter().map(|p| p.conquered.iter().sum::<i32>()).sum();
    let lost: i32 = w.statistic.players.iter().map(|p| p.captured_from.iter().sum::<i32>()).sum();
    eprintln!("{won} captures");
    assert_eq!(won, lost);
}

/// A sector AI of player `p` with a squad of more than 5.
fn sector_with_squad(w: &World, p: u8) -> Option<(u8, (i32, i32))> {
    let a = s4_sim::ai::player::get(w, p)?;
    a.body.sectors.iter().enumerate().find(|(_, s)| s.group.items.iter().any(|i| i.kind == 5 && i.tf.members.len() > 5)).map(|(k, s)| (k as u8, s.anchor()))
}

/// `CAIAgentIntruderCombat` (0x431bd0 / 0x431f60): an enemy thief put on a computer player's land
/// next to its anchor gets a hunter — a swordsman of a squad moves to reservoir 2, names the thief
/// as its partner and walks to it.
#[test]
fn intruders_are_hunted() {
    let Some(mut w) = game("Cordoba.map") else { return };
    for _ in 0..300 {
        s4_sim::tick(&mut w, &[]);
    }
    let ai: Vec<u8> = (1..=8).filter(|&p| s4_sim::ai::player::get(&w, p).is_some_and(|a| !a.dark)).collect();
    let (p, s, (ax, ay)) = ai.iter().find_map(|&p| sector_with_squad(&w, p).map(|(s, a)| (p, s, a))).expect("a computer player with a squad");
    let enemy = ai.iter().copied().find(|&q| q != p && w.alliances.hostile(p, q)).expect("an enemy");
    let thief = s4_sim::create::add_settler(&mut w, ax + 2, ay + 2, enemy, 48).expect("thief");
    let mut hunted = false;
    for _ in 0..400 {
        s4_sim::tick(&mut w, &[]);
        let a = s4_sim::ai::player::get(&w, p).unwrap();
        let SectorKind::Normal { forces, .. } = &a.body.sectors[s as usize].kind else { unreachable!() };
        if forces[2].members.iter().any(|m| w.ai.ext.get(m).is_some_and(|x| x.partner == thief)) {
            hunted = true;
            break;
        }
    }
    assert!(hunted, "player {p} sends a hunter after player {enemy}'s thief {thief}");
    consistent(&w);
}

/// `CAIAgentPioneers` (0x432620 / 0x432480): pioneers in slot 7 join reservoir 5 and are sent to
/// an unowned tile near the anchor (their info's tick is noted).
#[test]
fn pioneers_go_out() {
    let Some(mut w) = game("Cordoba.map") else { return };
    s4_sim::tick(&mut w, &[]);
    let p = (1..=8).find(|&p| s4_sim::ai::player::get(&w, p).is_some_and(|a| !a.dark && !a.body.sectors.is_empty())).expect("a computer player");
    let (ax, ay) = s4_sim::ai::player::get(&w, p).unwrap().body.sectors[0].anchor();
    let mut ids = Vec::new();
    for k in 0..3 {
        let id = s4_sim::create::add_settler(&mut w, ax + k, ay + 1, p, 47).expect("pioneer");
        // As the player AI's start (0x43a7f0) and the distribution (0x43a9f0) would put them.
        s4_sim::ai::player::add_to(&mut w, id, TfRef::Slot(p, 0, 7));
        ids.push(id);
    }
    let mut sent = false;
    for _ in 0..300 {
        s4_sim::tick(&mut w, &[]);
        sent = ids.iter().all(|i| w.ai.infos.get(i) == Some(&TfRef::Force(p, 0, 5)) && w.ai.ext.get(i).is_some_and(|x| x.tick > 0));
        if sent {
            break;
        }
    }
    assert!(sent, "the pioneers are in reservoir 5 and were sent");
    consistent(&w);
}

/// `AI.NewSquad(p, 7)` (event 0x1c): reservoir 4's units not on a sector AI's sector join the
/// player AI's squad and the global suicide mission (0x433210) is due at once; it orders the squad
/// to attack (order 4) an enemy military building — the nearest within 0x20, else player 1's
/// nearest tower or castle on the sector.
#[test]
fn global_suicide_mission_attacks() {
    let Some(mut w) = game("Cordoba.map") else { return };
    s4_sim::tick(&mut w, &[]);
    let p = (2..=8).find(|&p| s4_sim::ai::player::get(&w, p).is_some_and(|a| !a.dark && !a.body.sectors.is_empty())).expect("a computer player");
    let (ax, ay) = s4_sim::ai::player::get(&w, p).unwrap().body.sectors[0].anchor();
    let mut ids = Vec::new();
    for k in 0..4 {
        let id = s4_sim::create::add_settler(&mut w, ax + k, ay + 2, p, 29).expect("swordsman");
        ids.push(id);
    }
    s4_sim::tick(&mut w, &[]);
    // The squad task force directly (the release's reservoir 4 has no writer outside the AI's
    // own hand-out, so a script squad starts there).
    for &id in &ids {
        s4_sim::ai::player::add_to(&mut w, id, TfRef::Squad(p));
    }
    s4_sim::ai::post_event(&mut w, 0x1c, p as i32, 7, 0);
    let mut target = 0;
    for _ in 0..50 {
        s4_sim::tick(&mut w, &[]);
        let a = s4_sim::ai::player::get(&w, p).unwrap();
        if a.body.squad.command() == 4 && a.body.squad.target() > 0 {
            target = a.body.squad.target();
            break;
        }
    }
    assert!(target > 0, "the squad attacks");
    let e = w.entities.by_index(target as u32).expect("target");
    assert!((46..=48).contains(&e.kind) && w.alliances.hostile(p, e.owner), "an enemy military building: {} of player {}", e.kind, e.owner);
    consistent(&w);
}

/// The squad agent's strength `v15 · v16 / count` in SSE single precision (0x43428b..0x4342b0),
/// done in integers by `squads::single_mul_div`, against the hardware.
#[test]
fn single_precision_strength_matches_sse() {
    let mut seed = 12345u32;
    let mut next = || {
        seed = seed.wrapping_mul(1_812_433_253).wrapping_add(1);
        seed
    };
    let mut all: Vec<(i32, i32, i32)> = vec![(0, 5, 3), (7, 9, 2), (100_000, 300_001, 7), (16_777_217, 3, 5), (1 << 30, 3, 7), (12_345_679, 98_765, 4_321)];
    for _ in 0..20000 {
        let a = (next() >> (next() % 31)) as i32;
        let b = (next() >> (next() % 31)) as i32;
        let c = ((next() >> (next() % 31)) as i32).max(1);
        all.push((a, b, c));
    }
    for (a, b, c) in all {
        let f = (a as f32 * b as f32) / c as f32;
        let hw = if f.abs() >= 2_147_483_648.0 { i32::MIN } else { f as i32 };
        assert_eq!(s4_sim::ai::squads::single_mul_div(a, b, c), hw, "{a} * {b} / {c}");
    }
}

/// The priest task forces (`CAITaskForcePriests`, 0x4337e0 / 0x433930 / 0x446f20): priests in slot
/// 6 form a priest task force escorting a squad; with mana and enemy warriors near, one of them is
/// told to cast a military spell (event 3) and casts it.
#[test]
fn priests_cast_military_spells() {
    let Some(mut w) = game("Cordoba.map") else { return };
    for _ in 0..200 {
        s4_sim::tick(&mut w, &[]);
    }
    let ai: Vec<u8> = (1..=8).filter(|&p| s4_sim::ai::player::get(&w, p).is_some_and(|a| !a.dark && !a.body.sectors.is_empty())).collect();
    let p = ai[0];
    let enemy = ai.iter().copied().find(|&q| q != p && w.alliances.hostile(p, q)).expect("an enemy");
    let (ax, ay) = s4_sim::ai::player::get(&w, p).unwrap().body.sectors[0].anchor();
    s4_sim::magic::add_mana(&mut w, p, 500);
    let mut priests = Vec::new();
    for k in 0..2 {
        let id = s4_sim::create::add_settler(&mut w, ax + k, ay + 1, p, 45).expect("priest");
        priests.push(id);
    }
    for k in 0..8 {
        s4_sim::create::add_settler(&mut w, ax + 6 + k % 4, ay + 5 + k / 4, enemy, 29).expect("enemy swordsman");
    }
    s4_sim::tick(&mut w, &[]);
    for &id in &priests {
        s4_sim::ai::player::add_to(&mut w, id, TfRef::Slot(p, 0, 6));
    }
    let casts = |w: &World| w.magic.pool(p).map_or(0, |m| m.casts.iter().sum::<u32>());
    let before = casts(&w);
    let mut grouped = false;
    for _ in 0..1500 {
        s4_sim::tick(&mut w, &[]);
        let a = s4_sim::ai::player::get(&w, p).unwrap();
        grouped |= a.body.sectors[0].group.items.iter().any(|i| i.kind == 8 && !i.tf.members.is_empty());
        if casts(&w) > before {
            break;
        }
    }
    assert!(grouped, "the priests form a priest task force (type 8)");
    assert!(casts(&w) > before, "a priest of player {p} cast a spell");
    consistent(&w);
}

/// User bug B3 (viking01: shipped troops never move): `AI.NewSquad(p, 7)` (event 0x1c) right after
/// the script's `Settlers.AddSettlers`. The player AI hands reservoir 4 out (0x43a360) and then —
/// commands other than 8 / 0xd — passes the event on to every sector AI (0x43a420 `break` into
/// the forward loop), whose 0x1c (0x443520 → `new_squad`) makes a group of type 3 of the new
/// units (Force 3) and sends it home. Not passed on, they stayed in Force 3 for ever.
#[test]
fn new_squad_move_home_takes_the_new_units() {
    // 7 MOVE_HOME and 9 SUICIDE take this path; 8 / 0xd (send to a place) are handled by the
    // player AI alone (`ai_player` tests).
    for command in [7, 9] {
        new_squad_takes_the_new_units(command);
    }
}

fn new_squad_takes_the_new_units(command: i32) {
    let Some(mut w) = game("Cordoba.map") else { return };
    s4_sim::tick(&mut w, &[]);
    let p = (2..=8).find(|&p| s4_sim::ai::player::get(&w, p).is_some_and(|a| !a.dark && !a.body.sectors.is_empty())).expect("a computer player");
    let (ax, ay) = s4_sim::ai::player::get(&w, p).unwrap().body.sectors[0].anchor();
    let mut ids = Vec::new();
    for k in 0..4 {
        let id = s4_sim::create::add_settler(&mut w, ax + k, ay + 2, p, 29).expect("swordsman");
        // The script's `AddSettlers` tells the AI of each new unit (event 0x14, b 1).
        s4_sim::ai::post_event(&mut w, 0x14, p as i32, id as i32, 1);
        ids.push(id);
    }
    for _ in 0..5 {
        s4_sim::tick(&mut w, &[]);
    }
    let new = TfRef::Force(p, 0, 3);
    assert!(ids.iter().all(|i| w.ai.infos.get(i) == Some(&new)), "the new units in Force 3: {:?}", ids.iter().map(|i| w.ai.infos.get(i)).collect::<Vec<_>>());
    s4_sim::ai::post_event(&mut w, 0x1c, p as i32, command, 0);
    for _ in 0..5 {
        s4_sim::tick(&mut w, &[]);
    }
    let now: Vec<_> = ids.iter().map(|i| w.ai.infos.get(i).copied()).collect();
    eprintln!("after NewSquad {command}: {now:?}");
    assert!(now.iter().all(|r| matches!(r, Some(TfRef::Group(q, 0, _)) if *q == p)), "the new units form a group: {now:?}");
    consistent(&w);
}
