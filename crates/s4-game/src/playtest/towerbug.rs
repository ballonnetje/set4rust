//! Playtest save `towerbug.s4rs` (dark01, a siege): P1, P3, P4, P5, P7.

use super::load;
use s4_sim::entity::{EntityData, ObjType, flag};
use s4_sim::World;

/// P3 "towerbug": the swordsman 2374 hits the door settler of player 2's empty small tower 1482
/// for ever — each door settler he kills is replaced (0x54a120 finds no soldier inside and no
/// enemy that may take the tower). He is SELECTABLE and ATTACHED with home 664, which is no
/// building any more: he had been on his way to tower 664 when it was taken, and the capture
/// only sent event 9 without detaching its observers (fixed: `NotifyAndDetachAll` 0x537430, see
/// sim_military `a_taken_tower_detaches_the_soldiers_on_their_way`). 0x54a26c lets only a
/// SELECTABLE soldier without ATTACHED take the tower. The save keeps the broken state.
#[test]
fn towerbug_the_stuck_soldier_is_attached_to_a_gone_building() {
    let Some(mut w) = load("towerbug.s4rs") else { return };
    let s = 2374u16;
    let e = w.entities.by_index(s as u32).expect("the soldier");
    let home = match &e.data {
        EntityData::Settler(d) => d.job.home,
        _ => 0,
    };
    assert_eq!(e.flags & (flag::SELECTABLE | flag::ATTACHED), flag::SELECTABLE | flag::ATTACHED);
    assert!(w.entities.by_index(home as u32).is_none_or(|b| b.obj_type != ObjType::Building), "home {home} is a building");
    // The door settlers he kills are replaced at once.
    let mut doors = std::collections::BTreeSet::new();
    for _ in 0..400 {
        s4_sim::tick(&mut w, &[]);
        if let Some(m) = s4_sim::buildings::military::state(&w, 1482) {
            doors.insert(m.door);
        }
    }
    assert!(doors.len() > 2, "door settlers {doors:?}");
}

/// P1: the bowmen of player 2's big tower 686 (3 bowmen, entered during the game) shoot enemy
/// swordsmen standing 15..18 nodes from its door on its land until they die.
#[test]
fn towerbug_tower_bowmen_shoot() {
    let Some(mut w) = load("towerbug.s4rs") else { return };
    let tower = 686u16;
    let st = s4_sim::buildings::military::state(&w, tower).expect("tower 686").clone();
    assert_eq!(st.bow_in, 3);
    let dpos = w.entities.by_index(st.door as u32).unwrap().pos;
    let free = |w: &World, x: i32, y: i32| {
        w.grid.checked_index(x, y).is_some_and(|n| w.layers.ground[n] >= 16 && w.layers.flags[n] & 0x7F == 0 && w.layers.map_object[n] == 0 && w.tiling.sector_of(n) != 0)
    };
    let mut targets = Vec::new();
    for k in s4_sim::spiral::first(15)..=s4_sim::spiral::last(18) {
        let (x, y) = (dpos.x() + s4_sim::spiral::dx(k), dpos.y() + s4_sim::spiral::dy(k));
        if targets.len() < 3 && free(&w, x, y) && w.owner_of(w.grid.index(x as u32, y as u32)) == 2 && targets.iter().all(|&(s, _, _): &(u16, i32, i32)| w.entities.by_index(s as u32).is_some_and(|e| (e.x() - x).abs() + (e.y() - y).abs() > 3)) {
            let s = s4_sim::create::add_settler(&mut w, x, y, 1, 29).expect("swordsman");
            targets.push((s, w.entities.by_index(s as u32).unwrap().uid as i32, 0));
        }
    }
    assert_eq!(targets.len(), 3);
    // The same entity (ids are reused): its unique id.
    let alive = |w: &World, s: u16, uid: i32| w.entities.by_index(s as u32).is_some_and(|e| e.uid as i32 == uid && !e.is_dead());
    let mut arrows = 0;
    for t in 0..4000 {
        w.events.clear();
        s4_sim::tick(&mut w, &[]);
        if std::env::var_os("PT_DEBUG").is_some() && t % 100 == 0 {
            let hp: Vec<String> = targets.iter().map(|&(s, _, _)| match w.entities.by_index(s as u32).map(|e| (&e.data, e.x(), e.y())) {
                Some((EntityData::Settler(d), x, y)) => format!("{s}:{}@({x},{y})", d.health),
                _ => format!("{s}:-"),
            }).collect();
            let ts = s4_sim::buildings::military::state(&w, tower).unwrap().slots.clone();
            let sol: Vec<String> = ts.iter().filter(|&&x| x != 0).map(|&x| match w.entities.by_index(x as u32).map(|e| (&e.data, e.x(), e.y(), e.kind)) {
                Some((EntityData::Settler(d), ex, ey, k)) => format!("{x} k{k} @({ex},{ey}) task {} ent {} str {} st {} tick {}", d.job.task, d.job.entity, d.job.tower.strength, d.job.tower.state, d.job.tower.tick),
                _ => format!("{x}"),
            }).collect();
            eprintln!("t{t} {hp:?} | {sol:?} own {}", w.statistic.players[2].own);
        }
        arrows += w.events.iter().filter(|e| matches!(e, s4_sim::event::SimEvent::Projectile { offset, .. } if *offset != (0, 0))).count();
        if targets.iter().all(|&(s, uid, _)| !alive(&w, s, uid)) {
            break;
        }
    }
    let left: Vec<u16> = targets.iter().filter(|&&(s, uid, _)| alive(&w, s, uid)).map(|t| t.0).collect();
    assert!(arrows > 0, "no tower arrow");
    assert!(left.is_empty(), "{arrows} arrows; alive {left:?}");
}

/// Exploration P5: player 2's mine sites in towerbug — which of their nodes carry flag 0x08, and
/// whether DestroyBuilding at each finds them.
#[test]
#[ignore = "exploration"]
fn towerbug_mine_sites() {
    let Some(w0) = load("towerbug.s4rs") else { return };
    for site in [673u16, 1462, 1557] {
        let e = w0.entities.by_index(site as u32).unwrap();
        let (x, y) = (e.x(), e.y());
        let mut nodes = Vec::new();
        for (nx, ny) in s4_sim::spiral::walk(w0.grid, x, y, 4) {
            let n = w0.grid.index(nx as u32, ny as u32);
            if w0.layers.flags[n] & 8 != 0 {
                nodes.push((nx, ny));
            }
        }
        eprintln!("site {site} k{} at ({x},{y}): flag-8 nodes {nodes:?}, deco at pos {}", e.kind, w0.layers.deco_object[w0.grid.index(x as u32, y as u32)]);
        for &(nx, ny) in &nodes {
            let mut w = w0.clone();
            s4_sim::buildings::crush::destroy_building_at(&mut w, nx, ny, 0);
            let dead = w.entities.by_index(site as u32).is_none_or(|e| e.is_dead());
            eprintln!("  destroy at ({nx},{ny}): site dead {dead}");
        }
    }
}

/// Exploration P7: bowmen in towerbug that step A → B → A within 12 ticks, with their behaviour
/// state, target and the nearest enemy warrior at each step.
#[test]
#[ignore = "exploration"]
fn towerbug_bowmen_steps() {
    let Some(mut w) = load("towerbug.s4rs") else { return };
    let mut hist: std::collections::HashMap<u16, Vec<(u32, i32, i32, String)>> = Default::default();
    let mut found = 0;
    for _ in 0..600 {
        s4_sim::tick(&mut w, &[]);
        let t = w.tick;
        let bowmen: Vec<u16> = w.entities.iter().filter(|(_, e)| e.obj_type == ObjType::Settler && matches!(e.kind, 32..=34) && e.flags & flag::ON_BOARD == 0 && !e.is_dead()).map(|(i, _)| i).collect();
        for b in bowmen {
            let e = w.entities.by_index(b as u32).unwrap();
            let (x, y, owner) = (e.x(), e.y(), e.owner);
            let EntityData::Settler(d) = &e.data else { continue };
            let enemy = w
                .entities
                .iter()
                .filter(|(_, o)| o.obj_type == ObjType::Settler && o.owner != owner && o.owner != 0 && o.flags & 0xf != 0 && !o.is_dead() && o.flags & flag::ON_BOARD == 0)
                .map(|(i, o)| (s4_sim::spiral::grid_distance(o.x() - x, o.y() - y), i, o.kind))
                .min();
            let info = format!("st {} tgt {} bflags {} task {} next_search {} enemy {enemy:?}", d.job.soldier.state, d.job.soldier.target, d.job.soldier.bflags, d.job.task, d.job.soldier.next_search);
            let h = hist.entry(b).or_default();
            if h.last().is_none_or(|l| (l.1, l.2) != (x, y)) {
                h.push((t, x, y, info));
                let n = h.len();
                if n >= 3 && (h[n - 3].1, h[n - 3].2) == (x, y) && t - h[n - 3].0 <= 12 && found < 6 {
                    found += 1;
                    eprintln!("bowman {b} k{} back at ({x},{y}):", e.kind);
                    for l in &h[n.saturating_sub(5)..] {
                        eprintln!("    t{} ({},{}) {}", l.0, l.1, l.2, l.3);
                    }
                }
            }
        }
    }
}

/// Exploration P7: one bowman (P7_ID, default 858) tick by tick.
#[test]
#[ignore = "exploration"]
fn towerbug_bowman_trace() {
    let Some(mut w) = load("towerbug.s4rs") else { return };
    let id: u16 = std::env::var("P7_ID").ok().and_then(|v| v.parse().ok()).unwrap_or(858);
    for _ in 0..40 {
        s4_sim::tick(&mut w, &[]);
        let e = w.entities.by_index(id as u32).unwrap();
        let EntityData::Settler(d) = &e.data else { return };
        let n = w.grid.index(e.x() as u32, e.y() as u32);
        let ws = d.job.walking.as_deref();
        let around: Vec<(i32, i32, u16, u8)> = (0..6)
            .map(|k| s4_sim::jobs::walking::NEIGHBOR[k])
            .map(|(dx, dy)| {
                let m = w.grid.index((e.x() + dx) as u32, (e.y() + dy) as u32);
                (e.x() + dx, e.y() + dy, w.layers.map_object[m], w.layers.move_cost(m))
            })
            .collect();
        eprintln!(
            "t{} ({},{}) cost {} flags {:#x} dir {} task {} st {} | walk st {:?} to {:?} idle_t {:?} idle_dir {:?} wflags {:#x?} | around {around:?}",
            w.tick, e.x(), e.y(), w.layers.move_cost(n), e.flags, d.direction, d.job.task, d.job.soldier.state,
            ws.map(|s| s.state), ws.map(|s| { let p = s4_sim::grid::Y16X16(s.walk_to as u32); (p.x(), p.y()) }), ws.map(|s| s.idle_target), ws.map(|s| s.idle_dir), ws.map(|s| s.flags)
        );
    }
}

/// P4 "towerbug": player 1's sites 714 and 1183 made no progress. 1183 sat at an index whose
/// destroyed building had left its Simple role behind (78 such building roles and 4 site roles
/// in the save), so `GetPileIdWithGood` answered with that role's piles (none), `InitTransport`
/// skipped the site's requests (made every 15 ticks) and its builders stayed idle. Destroy now
/// deletes the role (sim_destroyed_role); the load drops what older builds left behind. 714 was
/// supplied all along, from stone piles ~90 nodes away. Both finish.
#[test]
fn towerbug_sites_progress() {
    let Some(mut w) = load("towerbug.s4rs") else { return };
    let sites = |w: &World| -> Vec<(u16, i32)> { w.sites.roles.iter().filter(|(id, _)| w.entities.by_index(**id as u32).is_some_and(|e| e.owner == 1)).map(|(&id, s)| (id, s.work_done)).collect() };
    let start = sites(&w);
    assert!(start.contains(&(1183, 0)), "{start:?}");
    assert!(s4_sim::buildings::role(&w, 1183).is_none(), "the stale role was not dropped");
    for _ in 0..6000 {
        s4_sim::tick(&mut w, &[]);
    }
    let now = sites(&w);
    for (id, done) in start {
        assert!(!now.iter().any(|s| s.0 == id), "site {id} (work {done}) not finished in 6000 ticks: {now:?}");
    }
}
