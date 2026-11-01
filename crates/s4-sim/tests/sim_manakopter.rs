//! The Dark Tribe's manakopter (`CManakopter`, `s4_sim::flying`) and its hall
//! (`CManakopterHallRole`) on AO_trojan04, whose Dark Tribe player has a hall; no AI. Skipped
//! without the game data.

use s4_sim::World;
use s4_sim::entity::{EntityData, ObjType, flag};
use s4_sim::flying::{self, state};
use s4_sim::jobs::state::EntityEvent;

const SWORDSMAN: u16 = 29;

fn game() -> Option<(World, u8)> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("AO_trojan04.map");
    let (Ok(map), Ok(rules)) = (s4_formats::map::MapFile::open(&p), s4_sim::Rules::load_default()) else { return None };
    let (w, _) = s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).ok()?;
    let dt = (1..=8u8).find(|&p| w.player(p).is_some_and(|q| q.race == 3))?;
    Some((w, dt))
}

fn hall(w: &World, dt: u8) -> u16 {
    w.entities.iter().find(|(_, e)| e.owner == dt && e.kind == 80 && e.obj_type == ObjType::Building && !e.is_dead()).map(|(i, _)| i).expect("a hall")
}

fn free_node(w: &World, x: i32, y: i32) -> bool {
    w.grid.checked_index(x, y).is_some_and(|n| w.layers.ground[n] >= 16 && w.layers.flags[n] & 0x7B == 0 && w.layers.map_object[n] == 0 && w.tiling.sector_of(n) != 0)
}

/// A free node near the hall (the Dark Tribe owns no land) with a free node east of it.
fn spot(w: &World, dt: u8) -> (i32, i32) {
    let h = w.entities.by_index(hall(w, dt) as u32).unwrap();
    let (hx, hy) = (h.x(), h.y());
    (s4_sim::spiral::first(6)..=s4_sim::spiral::last(30))
        .map(|k| (hx + s4_sim::spiral::dx(k), hy + s4_sim::spiral::dy(k)))
        .find(|&(x, y)| free_node(w, x, y) && free_node(w, x + 1, y))
        .expect("free land")
}

fn ticks(w: &mut World, n: u32) {
    for _ in 0..n {
        s4_sim::tick(w, &[]);
    }
}

/// Init (0x547990) leaves the hall full in state 2; [`start`] turns to state 3, which uses one
/// unit per `3 · frames(slot 5) − 1` ticks and then makes the manakopter at the ensign.
#[test]
fn the_hall_builds_a_manakopter() {
    let Some((mut w, dt)) = game() else { return };
    let req = w.rules.magic.dark.manakopter_required;
    assert_eq!(req, [4, 5, 5], "Balancing.cfg [AI]");
    let h = hall(&w, dt);
    ticks(&mut w, 40);
    let s = s4_sim::buildings::manakopter::get(&w, h).unwrap().clone();
    assert_eq!((s.state, s.stock), (2, req));
    assert!(flying::list(&w, dt, flying::MANAKOPTER).is_empty());
    let f5 = w.rules.dark.hall_patch_frames[3][5] as u32;
    assert!(f5 > 0, "the hall's slot 5 animation has frames");
    assert!(s4_sim::buildings::manakopter::start(&mut w, h));
    let t0 = w.tick;
    let mut made = None;
    for _ in 0..2000 {
        s4_sim::tick(&mut w, &[]);
        if let Some(&m) = flying::list(&w, dt, flying::MANAKOPTER).first() {
            made = Some((w.tick, m));
            break;
        }
    }
    let (t, m) = made.expect("a manakopter");
    // Started between two ticks, the first update is two ticks on (from the AI, inside a tick,
    // one); then 14 units, then the manakopter.
    assert_eq!(t - t0, 2 + 14 * (3 * f5 - 1));
    let ensign = match &w.entities.by_index(h as u32).unwrap().data {
        EntityData::Building(b) => b.ensign,
        _ => unreachable!(),
    };
    let e = w.entities.by_index(m as u32).unwrap();
    assert_eq!((e.obj_type, e.kind, e.owner, e.race, e.pos), (ObjType::Flying, 1, dt, 3, ensign));
    assert_eq!(e.flags & 0x0202_810f, 0x0202_8109, "ON_BOARD, READY, 0x20000, VISIBLE, warrior type 9");
    assert_eq!(flying::get(&w, m).unwrap().health, 250);
    assert_eq!((w.flying.last, flying::count(&w, dt)), (m, 1));
    let s = s4_sim::buildings::manakopter::get(&w, h).unwrap();
    assert_eq!((s.state, s.stock, s.used), (2, [0; 3], [0; 3]));
    // Without boards and iron no mana collects and nothing starts.
    ticks(&mut w, 100);
    assert_eq!(s4_sim::buildings::manakopter::get(&w, h).unwrap().stock, [0; 3]);
    assert!(!s4_sim::buildings::manakopter::start(&mut w, h));
}

/// The ticks of one move of the effect (0x4e28e0): the screen distance's integer root · 60 >> 9,
/// 1..=255 — here with `f64`, exact for these sizes.
fn move_ticks(w: &World, from: (i32, i32), to: (i32, i32)) -> u32 {
    let (a, b) = (s4_sim::projectile::screen(w, from.0, from.1), s4_sim::projectile::screen(w, to.0, to.1));
    let d = (((b.0 - a.0) as f64).powi(2) + ((b.1 - a.1) as f64).powi(2)).sqrt().floor() as u32;
    ((d * 60) >> 9).clamp(1, 255)
}

/// `FlyTo`: 30 ticks up (5 a tick from −50 to −200), one node a move toward the goal (x and y
/// each a step), each move as long as its screen distance says; at the goal 30 ticks down, the
/// order bit cleared in the first; landed it stays.
#[test]
fn a_manakopter_flies_to_its_goal_and_lands() {
    let Some((mut w, dt)) = game() else { return };
    let (x, y) = spot(&w, dt);
    let m = flying::add_manakopter(&mut w, x, y, dt).unwrap();
    ticks(&mut w, 12);
    let goal = (x + 7, y - 3);
    flying::fly_to(&mut w, m, goal.0, goal.1);
    let pos = |w: &World| w.entities.by_index(m as u32).map(|e| (e.x(), e.y())).unwrap();
    let height = |w: &World| flying::get(w, m).unwrap().offset.1;
    // Up.
    let t0 = w.tick;
    while height(&w) > -200 {
        s4_sim::tick(&mut w, &[]);
        assert_eq!(pos(&w), (x, y));
    }
    // Ordered between two ticks, the first update is two ticks on; 30 steps of 5.
    assert_eq!(w.tick - t0, 2 + 29);
    // The moves: the first in the next update, each next one in the update that ends the last;
    // the node is reached when the move ends.
    let mut at = (x, y);
    let mut path = Vec::new();
    let mut start = w.tick + 1;
    while at != goal {
        let next = (at.0 + (goal.0 - at.0).signum(), at.1 + (goal.1 - at.1).signum());
        let n = move_ticks(&w, at, next);
        let mut arrived = None;
        for _ in 0..300 {
            s4_sim::tick(&mut w, &[]);
            if pos(&w) == next {
                arrived = Some(w.tick);
                break;
            }
            assert_eq!(pos(&w), at, "between nodes the entity stays on the last");
        }
        assert_eq!(arrived, Some(start + n), "the move {at:?} → {next:?} takes {n} ticks");
        path.push(next);
        at = next;
        start = w.tick;
    }
    assert_eq!(path.len(), 7);
    // The update that arrives starts down at once and clears the order.
    assert_eq!(flying::get(&w, m).unwrap().state & state::ORDERED, 0);
    assert_eq!(height(&w), -195);
    let t1 = w.tick;
    while height(&w) < -50 {
        s4_sim::tick(&mut w, &[]);
    }
    assert_eq!(w.tick - t1, 29);
    assert_eq!(pos(&w), goal);
    ticks(&mut w, 50);
    assert_eq!((pos(&w), height(&w)), (goal, -50));
}

/// A swordsman called by event 0x1c walks to the manakopter, is its observer and, at CHECKIN,
/// its passenger (off the map); `LeaveMk` (0x56b990) sends him out next to it.
#[test]
fn a_soldier_boards_and_leaves_a_manakopter() {
    let Some((mut w, dt)) = game() else { return };
    let (x, y) = spot(&w, dt);
    let m = flying::add_manakopter(&mut w, x, y, dt).unwrap();
    let s = s4_sim::create::add_settler(&mut w, x + 1, y, dt, SWORDSMAN).expect("a swordsman");
    ticks(&mut w, 5);
    s4_sim::jobs::core::set_event(&mut w, s, EntityEvent { event: 0x1c, kind: 0, data: [m as i32, 0, 0] });
    ticks(&mut w, 2);
    let f = flying::get(&w, m).unwrap();
    assert_eq!(f.observers, vec![s]);
    assert_eq!(f.state & (state::EMPTY | state::CREW), state::CREW);
    let job = |w: &World| match &w.entities.by_index(s as u32).unwrap().data {
        EntityData::Settler(d) => d.job.home,
        _ => 0,
    };
    assert_eq!(job(&w), m, "the manakopter is his home");
    let mut boarded = false;
    for _ in 0..300 {
        s4_sim::tick(&mut w, &[]);
        if flying::get(&w, m).unwrap().passengers.iter().any(|p| p.id == s) {
            boarded = true;
            break;
        }
    }
    assert!(boarded, "aboard");
    let e = w.entities.by_index(s as u32).unwrap();
    assert!(e.flags & flag::ON_BOARD != 0 && !w.warmap.contains(s));
    // Out again (the scripts' LeaveMk on the last one made).
    flying::unload(&mut w, m);
    assert!(flying::get(&w, m).unwrap().passengers.is_empty());
    ticks(&mut w, 3);
    let e = w.entities.by_index(s as u32).unwrap();
    assert_eq!(e.flags & flag::ON_BOARD, 0, "on the map again");
    assert!(s4_sim::spiral::grid_distance(e.x() - (x + 1), e.y() - (y + 2)) <= 2, "next to spiral entry 11 of the manakopter");
    assert_eq!(job(&w), 0, "no home");
    assert!(flying::get(&w, m).unwrap().observers.is_empty());
}

/// Damage (vtbl+0x20 0x56b490) loses `DTManakopterArmor` (3) a hit, at least 1; at 0 it dies:
/// its passengers die with it, it leaves the list, the explosion effect 0x72 starts.
#[test]
fn damage_and_death() {
    let Some((mut w, dt)) = game() else { return };
    let (x, y) = spot(&w, dt);
    let m = flying::add_manakopter(&mut w, x, y, dt).unwrap();
    let s = s4_sim::create::add_settler(&mut w, x + 1, y, dt, SWORDSMAN).unwrap();
    assert!(flying::board(&mut w, m, s));
    s4_sim::combat::take_damage(&mut w, m, 100, 1);
    assert_eq!(flying::get(&w, m).unwrap().health, 250 - 97);
    s4_sim::combat::take_damage(&mut w, m, 2, 1);
    assert_eq!(flying::get(&w, m).unwrap().health, 152);
    w.events.clear();
    s4_sim::combat::take_damage(&mut w, m, 155, 1);
    assert!(w.entities.by_index(m as u32).unwrap().is_dead());
    assert!(w.entities.by_index(s as u32).unwrap().is_dead(), "the passenger dies");
    assert!(flying::list(&w, dt, flying::MANAKOPTER).is_empty() && flying::count(&w, dt) == 0);
    assert!(w.events.iter().any(|e| matches!(e, s4_sim::event::SimEvent::Effect { effect: 0x72, sound: 0x65, .. })));
    ticks(&mut w, 40);
}
