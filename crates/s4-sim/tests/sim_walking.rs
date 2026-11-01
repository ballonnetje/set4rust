//! `CWalkingBase::Walk` (pathing/walking.rs) driving settlers on a real map through the job
//! code's `WalkDir`: walks between random same-sector nodes arrive on their goal, never step onto
//! blocked land, and replay identically; idle walking spreads crowds onto free nodes. Skipped when the
//! install is absent.

use s4_formats::map::MapFile;
use s4_sim::jobs::core::walk_dir;
use s4_sim::jobs::walking::{ActivePathing, Pathing, WALK_ARRIVED, WALK_FAILED};
use s4_sim::pathing::distance_fast;
use s4_sim::rng::Random16;
use s4_sim::{NewGame, Rules, World, Y16X16, new_game};

fn world() -> Option<World> {
    let p = s4_formats::game_dir().join(r"Map\Campaign\AO_maya1.map");
    if !p.exists() {
        eprintln!("skipping: {} not found", p.display());
        return None;
    }
    let map = MapFile::open(p).unwrap();
    Some(new_game(&map, Rules::load_default().unwrap(), &NewGame::default()).unwrap().0)
}

fn free(w: &World, x: i32, y: i32) -> bool {
    let i = w.grid.index(x as u32, y as u32);
    w.layers.flags[i] & 1 == 0 && w.layers.map_object[i] == 0 && w.tiling.normal[i] >= 10
}

/// Walks `n` settlers one after another; returns (arrived, failed, out of calls, hash).
fn run(mut w: World, n: u32) -> (u32, u32, u32, u64) {
    let size = w.grid.size() as i32;
    let mut r = Random16::new(5, 0);
    let (mut arrived, mut failed, mut stuck, mut tried) = (0, 0, 0, 0);
    while tried < n {
        let s = ((r.rand() % size as u32) as i32, (r.rand() % size as u32) as i32);
        let g = (s.0 + (r.rand() % 121) as i32 - 60, s.1 + (r.rand() % 121) as i32 - 60);
        if !w.grid.in_world(g.0, g.1) || !free(&w, s.0, s.1) || !free(&w, g.0, g.1) || s == g {
            continue;
        }
        let (si, gi) = (w.grid.index(s.0 as u32, s.1 as u32), w.grid.index(g.0 as u32, g.1 as u32));
        if w.tiling.sector_of(si) != w.tiling.sector_of(gi) {
            continue;
        }
        tried += 1;
        let id = s4_sim::create::add_settler(&mut w, s.0, s.1, 0, 1).unwrap();
        ActivePathing::create(&mut w, id, 0);
        let goal = Y16X16::pack(g.0, g.1);
        ActivePathing::walk_to_xy(&mut w, id, goal, 0);
        let start = Y16X16::pack(s.0, s.1);
        let limit = 8 * distance_fast(start, goal) + 64;
        let mut done = false;
        for _ in 0..limit {
            let step = ActivePathing::next_step(&mut w, id);
            let res = walk_dir(&mut w, id, step);
            w.events.clear();
            let pos = w.entities.by_index(id as u32).unwrap().pos;
            let i = w.grid.index(pos.x() as u32, pos.y() as u32);
            assert_eq!(w.layers.flags[i] & 1, 0, "stepped onto blocked land at {:?}", (pos.x(), pos.y()));
            if res & WALK_ARRIVED != 0 {
                assert_eq!(pos, goal, "arrived off the goal");
                arrived += 1;
                done = true;
                break;
            }
            if res & WALK_FAILED != 0 {
                eprintln!("failed: {s:?} -> {g:?} at {:?}", (pos.x(), pos.y()));
                failed += 1;
                done = true;
                break;
            }
        }
        stuck += !done as u32;
        // Leave it standing as an obstacle for the next walks.
        ActivePathing::walk_to_xy(&mut w, id, Y16X16(u32::MAX - 1), 0);
    }
    (arrived, failed, stuck, w.state_hash())
}

#[test]
fn walks_arrive_and_replay() {
    let Some(w) = world() else { return };
    let a = run(w.clone(), 150);
    let b = run(w, 150);
    eprintln!("150 walks: {} arrived, {} failed, {} out of calls", a.0, a.1, a.2);
    assert_eq!(a, b, "walking does not replay");
    assert_eq!((a.0, a.1, a.2), (150, 0, 0), "every same-sector walk arrives");
}

/// `IdleWalk` with flags 0 only steps away from occupied neighbours: lone settlers stand still,
/// clusters (a node and its six neighbours) spread out, onto free nodes only.
#[test]
fn idle_walk_spreads_clusters() {
    let Some(mut w) = world() else { return };
    let size = w.grid.size() as i32;
    let mut r = Random16::new(9, 0);
    let (mut lone, mut cluster) = (Vec::new(), Vec::new());
    let ring = [(0, 0), (1, 0), (1, 1), (0, 1), (-1, 0), (-1, -1), (0, -1)];
    while lone.len() < 20 || cluster.len() < 70 {
        let (x, y) = (8 + (r.rand() % (size as u32 - 16)) as i32, 8 + (r.rand() % (size as u32 - 16)) as i32);
        let area = ring.iter().chain(&[(2, 0), (-2, 0), (0, 2), (0, -2), (2, 2), (-2, -2)]);
        let clear = area.clone().all(|&(dx, dy)| free(&w, x + dx, y + dy));
        let (want, list) = if lone.len() < 20 { (&ring[..1], &mut lone) } else { (&ring[..], &mut cluster) };
        if !clear {
            continue;
        }
        for &(dx, dy) in want {
            let id = s4_sim::create::add_settler(&mut w, x + dx, y + dy, 0, 1).unwrap();
            ActivePathing::create(&mut w, id, 0);
            ActivePathing::walk_to_xy(&mut w, id, Y16X16(u32::MAX), 0);
            list.push(id);
        }
    }
    let (mut moves, rng_before) = (0, w.rng.calls);
    for _ in 0..30 {
        for &id in lone.iter().chain(&cluster) {
            let before = w.entities.by_index(id as u32).unwrap().pos;
            let step = ActivePathing::next_step(&mut w, id);
            assert_eq!(step & 0xF0, 0x10, "idle steps carry only the no-wear bit");
            walk_dir(&mut w, id, step);
            w.events.clear();
            let pos = w.entities.by_index(id as u32).unwrap().pos;
            if pos != before {
                assert!(!lone.contains(&id), "a lone settler idled away");
                moves += 1;
                assert_eq!(distance_fast(before, pos), 1);
                let i = w.grid.index(pos.x() as u32, pos.y() as u32);
                assert_eq!(w.layers.flags[i] & 1, 0, "idle step onto blocked land");
            }
        }
    }
    eprintln!("{moves} idle moves, {} RNG calls", w.rng.calls - rng_before);
    assert!(moves >= 60, "clusters did not spread");
}
