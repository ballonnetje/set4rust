//! Ships (`vehicles::ship`, V5a), driven by `tick()` on roman02's sea.
//!
//! * `ferry_sails_to_a_place`: a finished ferry on open water gets a move order (event 0x11 /
//!   0xd, as `SendTo`): it walks with `CWalkingShip` over the water and arrives; deterministic.
//! * `marks_follow_the_ships`: with two ferries passing each other, after every tick the water
//!   flags hold bit 0x800 exactly on the ships' nodes and every water node's low byte is the
//!   closeness of its two nearest ships (`10 − distance`, within 9), as recomputing from scratch
//!   gives; no two ships share a node.
//!
//! * `ships_spread_after_arriving`: three ferries sent to one place arrive and then drift apart
//!   by `CWalkingShip::IdleWalk` (0x5ba8a0: while some neighbour is closer to another ship than
//!   another neighbour, a weighted random step to a least crowded one) until no ship's
//!   neighbourhood has that slope; then they stand. Deterministic. (Seen in the viewer as ships
//!   "stepping on" for some ticks after arriving: that is the original's behaviour; the
//!   function itself is checked against the release by `idle_emu.py`, 4000 vectors, 0
//!   mismatches.)
//! * `killed_ship_leaves_no_marks`: a ferry killed under way (`CMapObjectMgr::Kill` 0x56d030 →
//!   Delete 0x594a10) is off the map at once — position 0, out of the logic update (0x56d720) —
//!   and stays so until it is destroyed 31 ticks later: no cell entry, no bit 0x800 anywhere, the
//!   other ferry sails through where it was.
//! * `dump_idle_vectors` (ignored): random water flags around roman02's sea and `IdleWalk`
//!   queries with our answers, for `tools/re/vehicles/idle_emu.py`, which runs the release's
//!   `CWalkingShip::IdleWalk` (0x5ba8a0) on them in unicorn.
//!
//! Skipped when the game data is not present.

use s4_sim::World;
use s4_sim::jobs::state::EntityEvent;
use s4_sim::vehicles::{self, kind};

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

/// The first node of the spiral around (x, y) where a ship can be made.
fn water_near(w: &World, x: i32, y: i32, r: u8) -> Option<(i32, i32)> {
    s4_sim::spiral::walk(w.grid, x, y, r).find(|&(px, py)| vehicles::can_place(w, px, py, kind::FERRY))
}

fn send(w: &mut World, id: u16, x: i32, y: i32) {
    let p = s4_sim::grid::Y16X16::pack(x, y);
    vehicles::logic::set_event(w, id, EntityEvent { event: 0x11, kind: 0xd, data: [0, p.0 as i32, 0] });
}

fn pos(w: &World, id: u16) -> (i32, i32) {
    let e = w.entities.by_index(id as u32).unwrap();
    (e.x(), e.y())
}

fn dist(a: (i32, i32), b: (i32, i32)) -> i32 {
    s4_sim::spiral::grid_distance(a.0 - b.0, a.1 - b.1)
}

/// Bit 0x800 only on the ships' nodes, the low bytes as recomputed from all ships.
fn check_marks(w: &World, ships: &[u16]) -> Result<(), String> {
    let at: Vec<(i32, i32)> = ships.iter().map(|&s| pos(w, s)).collect();
    let side = w.grid.size() as i32;
    for n in 0..w.grid.len() {
        let wf = w.tiling.water_flags[n];
        let (x, y) = (n as i32 % side, n as i32 / side);
        let here = at.contains(&(x, y));
        if (wf & 0x800 != 0) != here {
            return Err(format!("bit 0x800 at ({x},{y}): {wf:#x}, ships {at:?}"));
        }
        if wf < 0xc000 {
            continue;
        }
        let mut d: Vec<i32> = at.iter().map(|&p| dist(p, (x, y))).filter(|&d| d < 10).collect();
        d.sort();
        let near = |i: usize| d.get(i).map_or(0, |&d| 10 - d) as u16;
        let want = near(1) << 4 | near(0);
        if wf & 0xff != want {
            return Err(format!("closeness at ({x},{y}): {:#x}, want {want:#x}, ships {at:?}", wf & 0xff));
        }
    }
    Ok(())
}

/// (state hash, start, end, the tick it came within 3 of the place).
type Sail = (u64, (i32, i32), (i32, i32), Option<u32>);

fn sail(ticks: u32) -> Option<Sail> {
    let mut w = roman02()?;
    let (sx, sy) = water_near(&w, 200, 90, 12)?;
    let f = vehicles::add_vehicle(&mut w, sx, sy, 1, kind::FERRY, 0, false)?;
    let (tx, ty) = water_near(&w, sx + 25, sy + 10, 8)?;
    send(&mut w, f, tx, ty);
    let mut arrived = None;
    for _ in 0..ticks {
        s4_sim::tick(&mut w, &[]);
        w.events.clear();
        if arrived.is_none() && dist(pos(&w, f), (tx, ty)) <= 3 {
            arrived = Some(w.tick);
        }
    }
    Some((w.state_hash(), (sx, sy), pos(&w, f), arrived))
}

#[test]
fn ferry_sails_to_a_place() {
    let Some(r) = sail(1200) else {
        eprintln!("skipping: no game data or no water");
        return;
    };
    eprintln!("from {:?} to {:?}, within 3 of the place at tick {:?}", r.1, r.2, r.3);
    assert!(r.3.is_some(), "the ferry did not arrive: {r:?}");
    assert!(dist(r.1, r.2) >= 20, "it moved");
    assert_eq!(sail(1200).unwrap(), r, "two runs differ");
}

#[test]
fn marks_follow_the_ships() {
    let Some(mut w) = roman02() else { return };
    let Some((ax, ay)) = water_near(&w, 200, 90, 12) else { return };
    let Some((bx, by)) = water_near(&w, ax + 20, ay + 6, 8) else { return };
    let a = vehicles::add_vehicle(&mut w, ax, ay, 1, kind::FERRY, 0, false).unwrap();
    let b = vehicles::add_vehicle(&mut w, bx, by, 1, kind::FERRY, 3, false).unwrap();
    check_marks(&w, &[a, b]).unwrap();
    send(&mut w, a, bx, by);
    send(&mut w, b, ax, ay);
    let start = (pos(&w, a), pos(&w, b));
    for _ in 0..900 {
        s4_sim::tick(&mut w, &[]);
        w.events.clear();
        if let Err(e) = check_marks(&w, &[a, b]) {
            panic!("tick {}: {e}", w.tick);
        }
        assert_ne!(pos(&w, a), pos(&w, b));
    }
    let end = (pos(&w, a), pos(&w, b));
    eprintln!("a {:?} → {:?}, b {:?} → {:?}", start.0, end.0, start.1, end.1);
    assert!(dist(end.0, start.1) < dist(start.0, start.1), "a came closer to b's start");
    assert!(dist(end.1, start.0) < dist(start.1, start.0), "b came closer to a's start");
}

#[test]
fn killed_ship_leaves_no_marks() {
    let Some(mut w) = roman02() else { return };
    let Some((ax, ay)) = water_near(&w, 200, 90, 12) else { return };
    let Some((tx, ty)) = water_near(&w, ax + 25, ay + 10, 8) else { return };
    let a = vehicles::add_vehicle(&mut w, ax, ay, 1, kind::FERRY, 0, false).unwrap();
    send(&mut w, a, tx, ty);
    for _ in 0..200 {
        s4_sim::tick(&mut w, &[]);
        w.events.clear();
    }
    let at = pos(&w, a);
    assert!(dist(at, (ax, ay)) > 3, "under way: {at:?}");
    w.kill(a, 0);
    let e = w.entities.by_index(a as u32).unwrap();
    assert_eq!((e.pos.0, e.flags & s4_sim::entity::flag::REGISTERED), (0, 0), "off the map and out of the logic update");
    for _ in 0..40 {
        s4_sim::tick(&mut w, &[]);
        w.events.clear();
        let entries = w.vehicles.ship_cells.0.iter().flatten().filter(|&&p| p != 0).count();
        let marks = w.tiling.water_flags.iter().filter(|&&f| f & 0x800 != 0).count();
        assert_eq!((entries, marks), (0, 0), "tick {}: cell entries and marks of the dead ferry", w.tick);
    }
    // Another ferry passes the spot.
    let b = vehicles::add_vehicle(&mut w, ax, ay, 1, kind::FERRY, 0, false).unwrap();
    send(&mut w, b, tx, ty);
    let mut passed = false;
    for _ in 0..1200 {
        s4_sim::tick(&mut w, &[]);
        w.events.clear();
        passed |= dist(pos(&w, b), at) <= 1;
    }
    assert!(passed, "the second ferry came by {at:?}");
}

/// The six neighbours' "second ship" closeness (water flags bits 4..7; 0xb where & 0x300) and
/// whether `IdleWalk` would drift there (`lo < hi`, hi over the open ones).
fn drifts(w: &World, (x, y): (i32, i32)) -> bool {
    let (mut lo, mut hi) = (10u16, 0u16);
    for (dx, dy) in [(1, 0), (1, 1), (0, 1), (-1, 0), (-1, -1), (0, -1)] {
        let Some(n) = w.grid.checked_index(x + dx, y + dy) else { continue };
        let wf = w.tiling.water_flags[n];
        if wf & 0x300 != 0 {
            continue;
        }
        let v = wf >> 4 & 0xf;
        lo = lo.min(v);
        hi = hi.max(v);
    }
    lo < hi
}

type Spread = (u64, Vec<(i32, i32)>, u32, Option<u32>);

/// Three ferries sent to one place: (state hash, end positions, last tick a ship moved, the
/// tick the first one came within 3 of the place).
fn spread(ticks: u32) -> Option<Spread> {
    let mut w = roman02()?;
    let (tx, ty) = water_near(&w, 220, 100, 12)?;
    let mut ships = Vec::new();
    for (i, (dx, dy)) in [(-18, -4), (-16, 6), (-20, 2)].into_iter().enumerate() {
        let (x, y) = water_near(&w, tx + dx, ty + dy, 6)?;
        ships.push(vehicles::add_vehicle(&mut w, x, y, 1, kind::FERRY, i as i32, false)?);
    }
    for &s in &ships {
        send(&mut w, s, tx, ty);
    }
    let mut at: Vec<(i32, i32)> = ships.iter().map(|&s| pos(&w, s)).collect();
    let (mut moved, mut arrived) = (0, None);
    for _ in 0..ticks {
        s4_sim::tick(&mut w, &[]);
        w.events.clear();
        let now: Vec<(i32, i32)> = ships.iter().map(|&s| pos(&w, s)).collect();
        if now != at {
            moved = w.tick;
            at = now;
        }
        if arrived.is_none() && at.iter().any(|&p| dist(p, (tx, ty)) <= 3) {
            arrived = Some(w.tick);
        }
    }
    check_marks(&w, &ships).unwrap();
    for &p in &at {
        assert!(!drifts(&w, p), "a standing ship at {p:?} still has a slope around it");
    }
    Some((w.state_hash(), at, moved, arrived))
}

#[test]
fn ships_spread_after_arriving() {
    let Some(r) = spread(2500) else {
        eprintln!("skipping: no game data or no water");
        return;
    };
    eprintln!("ends {:?}, first within 3 of the place at {:?}, last move at tick {}", r.1, r.3, r.2);
    assert!(r.2 + 500 < 2500, "they came to rest: last move at tick {}", r.2);
    for i in 0..3 {
        for j in i + 1..3 {
            assert!(dist(r.1[i], r.1[j]) >= 2, "apart: {:?}", r.1);
        }
    }
    assert_eq!(spread(2500).unwrap(), r, "two runs differ");
}

/// Test vectors for `tools/re/vehicles/idle_emu.py`: roman02 with random water flags in a
/// window of its sea (open water with random closeness nibbles, ship / shipyard / blocked bits,
/// some land), the node sectors, and per query (cur, flags +0x30, target +0x1c, RNG seed) in and
/// our (result, flags, target, seed) out. Written to out/audio/idle_vec.json.
#[test]
#[ignore]
fn dump_idle_vectors() {
    use s4_sim::pathing::walking::{WalkState, walk_flag, walking_type};
    let Some(mut w) = roman02() else { return };
    let size = w.grid.size() as i32;
    let mut r = s4_sim::rng::Random16::new(0x1234_5678, 0);
    let mut next = |n: u32| r.rand() % n;
    let (x0, y0, x1, y1) = (180, 60, 240, 120);
    for y in y0..y1 {
        for x in x0..x1 {
            let n = w.grid.index(x as u32, y as u32);
            let wf: u16 = if next(12) == 0 {
                0
            } else {
                let hi = if next(10) < 6 { 0 } else { next(11) as u16 };
                let lo = if next(4) == 0 { 0 } else { next(11) as u16 };
                let mut f = 0xc000 | hi << 4 | lo;
                if next(5) == 0 {
                    f |= 0x800;
                }
                if next(3) == 0 {
                    f |= 0x1000;
                }
                if next(10) == 0 {
                    f |= 0x200;
                }
                if next(20) == 0 {
                    f |= 0x100;
                }
                f
            };
            w.tiling.water_flags[n] = wf;
        }
    }
    let mut q = Vec::new();
    for i in 0..4000u32 {
        let (x, y) = (x0 + 16 + next((x1 - x0 - 32) as u32) as i32, y0 + 16 + next((y1 - y0 - 32) as u32) as i32);
        let cur = y << 16 | x;
        let flags = if next(2) == 0 { walk_flag::IDLE_KEEP } else { 0 };
        let target = if next(3) == 0 { -1 } else { (y + next(9) as i32 - 4) << 16 | (x + next(9) as i32 - 4) };
        let seed = 0x9e37_79b9u32.wrapping_mul(i + 1);
        let mut s = WalkState::new(walking_type::SHIP, 1);
        s.flags = flags;
        s.idle_target = target;
        w.rng = s4_sim::rng::Random16::new(seed, 0);
        let ret = s4_sim::vehicles::ship::idle_walk(&mut w, &mut s, cur);
        q.push(format!("[{cur},{flags},{target},{seed},{ret},{},{},{}]", s.flags, s.idle_target, w.rng.seed));
    }
    let wf: String = w.tiling.water_flags.iter().map(|v| format!("{:04x}", v.swap_bytes())).collect();
    let sec: Vec<String> = (0..(size * size) as usize).map(|n| s4_sim::vehicles::ship::sector(&w, Some(n)).to_string()).collect();
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/audio");
    std::fs::create_dir_all(&out).unwrap();
    let json = format!("{{\"size\":{size},\"wf\":\"{wf}\",\"sector\":[{}],\"queries\":[{}]}}", sec.join(","), q.join(","));
    std::fs::write(out.join("idle_vec.json"), json).unwrap();
}
