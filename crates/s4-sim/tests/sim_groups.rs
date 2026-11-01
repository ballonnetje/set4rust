//! Unit groups (`CGroupMgr` 0x153fd30: command 5041 0x4f6240 case 0x13b1, Add 0x56a230, Remove
//! 0x56a340, DetachEntityFromAllGroups 0x56a430, Clear 0x56a4a0) and the squad leader
//! (`CSquadLeaderRole`: lead 0x58aea0, the 0x800 critical-hit bit read by 0x589250), on roman02
//! and driven by `tick()` only. Skipped without the game data.

use s4_sim::entity::{EntityData, ObjType, flag};
use s4_sim::jobs::role;
use s4_sim::{Command, PlayerCommand, Rules, World};

const SWORDSMAN_1: u16 = 29;
const SQUAD_LEADER: u16 = 0x2c;

fn roman02() -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map");
    if !p.exists() {
        return None;
    }
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    let rules = Rules::load_default().ok()?;
    let (w, _) = s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).ok()?;
    Some(w)
}

fn job(w: &World, s: u16) -> &s4_sim::jobs::JobState {
    match &w.entities.by_index(s as u32).expect("settler").data {
        EntityData::Settler(d) => &d.job,
        _ => panic!("{s} is not a settler"),
    }
}

fn free_node(w: &World, x: i32, y: i32) -> bool {
    w.grid.checked_index(x, y).is_some_and(|n| w.layers.ground[n] >= 16 && w.layers.flags[n] & 0x7F == 0 && w.layers.map_object[n] == 0 && w.tiling.sector_of(n) != 0)
}

/// `n` new settlers of `kind` for player 1 on free nodes of one sector, 12..40 from its castle.
fn new_settlers(w: &mut World, kinds: &[u16]) -> Vec<u16> {
    let castle = w
        .entities
        .iter()
        .find(|(id, e)| e.owner == 1 && e.obj_type == ObjType::Building && s4_sim::buildings::have_inhabitant(w, *id))
        .map(|(_, e)| (e.x(), e.y()))
        .expect("castle");
    let nodes: Vec<(i32, i32)> = (s4_sim::spiral::first(12)..=s4_sim::spiral::last(40))
        .map(|k| (castle.0 + s4_sim::spiral::dx(k), castle.1 + s4_sim::spiral::dy(k)))
        .filter(|&(x, y)| free_node(w, x, y))
        .collect();
    let sector = |w: &World, p: (i32, i32)| w.tiling.sector_of(w.grid.index(p.0 as u32, p.1 as u32));
    let s0 = sector(w, nodes[0]);
    let mut out = Vec::new();
    for &p in &nodes {
        if out.len() == kinds.len() {
            break;
        }
        if sector(w, p) == s0 && free_node(w, p.0, p.1) {
            out.push(s4_sim::create::add_settler(w, p.0, p.1, 1, kinds[out.len()]).expect("settler"));
        }
    }
    assert_eq!(out.len(), kinds.len());
    out
}

fn define(w: &mut World, group: u32, ids: &[u16]) {
    let cmd = PlayerCommand { player: 1, command: Command::DefineGroup { group, entities: ids.to_vec() } };
    s4_sim::tick(w, &[cmd]);
}

/// The group words: members added before the squad leader get 0x800, later ones do not; the
/// leader holds `group << 12 | bit`; redefining moves members; a leader's death takes 0x800 from
/// the members still listed; settlers without `ISelectableSettlerRole` (carriers) never join.
#[test]
fn group_words_follow_the_release_rules() {
    let Some(mut w) = roman02() else { return };
    let s = new_settlers(&mut w, &[SWORDSMAN_1, SQUAD_LEADER, SWORDSMAN_1, SWORDSMAN_1]);
    let (a, leader, b, c) = (s[0], s[1], s[2], s[3]);
    assert_eq!(job(&w, leader).role, role::SQUAD_LEADER);
    let carrier = w.entities.iter().find(|(_, e)| e.owner == 1 && e.obj_type == ObjType::Settler && e.kind == 1).map(|(id, _)| id).expect("carrier");
    define(&mut w, 1, &[a, leader, b, carrier]);
    assert_eq!(w.groups.members(1, 1), &[a, leader, b]);
    assert!(w.groups.has_leader(1, 1));
    assert_eq!(job(&w, a).groups, 0x800 | 2);
    assert_eq!(job(&w, leader).groups, 1 << 12 | 2);
    assert_eq!(job(&w, b).groups, 2, "a member added after the leader has no bonus");
    assert_eq!(job(&w, carrier).groups, 0);
    // Moving `a` into group 2: it leaves group 1 (the last member takes its place) and its word
    // is rebuilt from 0.
    define(&mut w, 2, &[a, c]);
    assert_eq!(w.groups.members(1, 1), &[b, leader]);
    assert_eq!(w.groups.members(1, 2), &[a, c]);
    assert_eq!(job(&w, a).groups, 4);
    // Redefining group 1 with the leader first: nobody listed before it, nobody gets the bit.
    define(&mut w, 1, &[leader, b]);
    assert_eq!(w.groups.members(1, 1), &[leader, b]);
    assert_eq!(job(&w, b).groups, 2);
    define(&mut w, 1, &[b, leader]);
    assert_eq!(job(&w, b).groups, 0x800 | 2);
    // The leader dies: the group loses its flag and its members the bonus.
    w.kill(leader, 0);
    assert!(!w.groups.has_leader(1, 1));
    assert_eq!(w.groups.members(1, 1), &[b]);
    assert_eq!(job(&w, b).groups, 2);
    // A second leader can not join a group that has one.
    let l2 = new_settlers(&mut w, &[SQUAD_LEADER, SQUAD_LEADER]);
    define(&mut w, 3, &[l2[0], l2[1], a]);
    assert_eq!(w.groups.members(1, 3), &[l2[0], a]);
    assert_eq!(job(&w, l2[1]).groups, 0);
    assert_eq!(job(&w, a).groups, 8, "added after the leader");
}

/// The leader's reach `(int)(sqrt(n) · 0.75 + 3.0)` in integers.
#[test]
fn lead_reach_matches_the_double_formula() {
    for n in 2..=100u32 {
        let f = ((n as f64).sqrt() * 0.75 + 3.0) as i32;
        assert_eq!(s4_sim::jobs::soldier::lead_reach(n), f, "n = {n}");
    }
}

/// The squad leader leads (0x58aea0): when only the leader is sent somewhere, its group's
/// soldiers are sent to the formation places around its waypoint every 45 ticks while they are
/// farther than the reach.
#[test]
fn a_squad_leader_brings_its_group_along() {
    let Some(mut w) = roman02() else { return };
    let s = new_settlers(&mut w, &[SWORDSMAN_1, SWORDSMAN_1, SQUAD_LEADER]);
    let (a, b, leader) = (s[0], s[1], s[2]);
    define(&mut w, 1, &[a, b, leader]);
    let lpos = w.entities.by_index(leader as u32).unwrap().pos;
    let sector = |w: &World, x: i32, y: i32| w.tiling.sector_of(w.grid.index(x as u32, y as u32));
    let s0 = sector(&w, lpos.x(), lpos.y());
    let target = (s4_sim::spiral::first(14)..=s4_sim::spiral::last(18))
        .map(|k| (lpos.x() + s4_sim::spiral::dx(k), lpos.y() + s4_sim::spiral::dy(k)))
        .find(|&(x, y)| free_node(&w, x, y) && sector(&w, x, y) == s0)
        .expect("a target");
    let cmd = PlayerCommand { player: 1, command: Command::SendTo { x: target.0 as u16, y: target.1 as u16, mode: 0, entities: vec![leader] } };
    s4_sim::tick(&mut w, &[cmd]);
    for _ in 0..900 {
        s4_sim::tick(&mut w, &[]);
    }
    let p = |w: &World, s: u16| w.entities.by_index(s as u32).unwrap().pos;
    let lp = p(&w, leader);
    let reach = s4_sim::jobs::soldier::lead_reach(3);
    for &m in &[a, b] {
        let mp = p(&w, m);
        let d = s4_sim::spiral::grid_distance(mp.x() - lp.x(), mp.y() - lp.y());
        eprintln!("leader {leader} at {lp:?} (target {target:?}), member {m} at {mp:?}, distance {d}, reach {reach}");
        assert!(d <= reach + 4, "member {m} did not follow");
    }
    // The formation's first place is the leader's own target (0x56a000 puts the target first and
    // the leader takes no place), so the first member stands there and the leader next to it.
    assert!(s4_sim::spiral::grid_distance(lp.x() - target.0, lp.y() - target.1) <= 3, "the leader did not arrive");
    assert!(w.entities.by_index(a as u32).unwrap().flags & flag::SELECTABLE != 0);
}

/// Groups and the leader's members are part of the state hash: two identical runs agree.
#[test]
fn group_runs_are_deterministic() {
    let run = || {
        let mut w = roman02()?;
        let s = new_settlers(&mut w, &[SWORDSMAN_1, SWORDSMAN_1, SQUAD_LEADER]);
        define(&mut w, 1, &s);
        for _ in 0..300 {
            s4_sim::tick(&mut w, &[]);
        }
        Some(w.state_hash())
    };
    let (Some(h1), Some(h2)) = (run(), run()) else { return };
    assert_eq!(h1, h2);
}

/// The squad-leader bonus table (0x12dbc40) from the shipped settler data.
#[test]
fn leader_bonus_table() {
    let Ok(rules) = Rules::load_default() else { return };
    let v: Vec<(u8, i32)> = (0..5).map(|r| (rules.settler_misc(r, SQUAD_LEADER), rules.leader_bonus(r))).collect();
    eprintln!("squad leader info +7 and bonus per race: {v:?}");
    let roman = match rules.settler_misc(0, SQUAD_LEADER) {
        0 => 10,
        m => m as i32,
    } * 256
        / 100;
    assert_eq!(v[0].1, roman);
}

/// A2: s4rs selects up to 1000 units ([deviation] from the release's 100). A group takes 1000
/// swordsmen (5041), and a 1000-unit SendTo's formation (0x56a000) gives each its own place,
/// spiralling on past entry 99; up to 100 units the places are the release's.
#[test]
fn a_thousand_unit_group_and_formation() {
    let Some(mut w) = roman02() else { return };
    let ids = new_settlers(&mut w, &[SWORDSMAN_1; 1000]);
    define(&mut w, 2, &ids);
    assert_eq!(w.groups.count(1, 2), 1000, "the group holds them all");
    let at = w.entities.by_index(ids[0] as u32).unwrap().pos;
    let f = s4_sim::group::formation(&w, at.x(), at.y(), 1000, 1, at.0 as i32);
    let distinct: std::collections::BTreeSet<u32> = f.positions.iter().map(|p| p.0).collect();
    assert_eq!((f.positions.len(), distinct.len()), (1000, 1000), "a place each");
    // Up to 100 the release's spiral entries 1..=99 only.
    let small = s4_sim::group::formation(&w, at.x(), at.y(), 100, 1, at.0 as i32);
    let first: Vec<u32> = f.positions.iter().take(small.positions.len()).map(|p| p.0).collect();
    assert_eq!(first, small.positions.iter().map(|p| p.0).collect::<Vec<_>>(), "the first places are the release's");
}
