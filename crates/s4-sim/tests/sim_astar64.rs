//! `CAStar64::FindPath` (Normal) on a real map: it finds a path exactly when a breadth-first search
//! over unblocked nodes inside the same 64×64 window reaches the goal, and the returned moves step
//! over unblocked nodes towards it (all the way when the path has at most 16 moves).
//! Skipped when the install is absent.

use std::collections::VecDeque;

use s4_formats::map::MapFile;
use s4_sim::pathing::{DirCache, Variant, astar64};
use s4_sim::rng::Random16;
use s4_sim::tiling::NEIGHBORS;
use s4_sim::{NewGame, Rules, World, Y16X16, new_game};

/// Reachability inside the window the search uses (low side clamped, as in `Init`).
fn bfs(w: &World, s: (i32, i32), g: (i32, i32)) -> Option<usize> {
    let (dx, dy) = ((g.0 - s.0).abs(), (g.1 - s.1).abs());
    let ox = (s.0.min(g.0) + (dx >> 1) - 32).max(0);
    let oy = (s.1.min(g.1) + (dy >> 1) - 32).max(0);
    let inside = |x: i32, y: i32| (ox..ox + 64).contains(&x) && (oy..oy + 64).contains(&y) && w.grid.in_world(x, y);
    let mut dist = std::collections::HashMap::from([(s, 0usize)]);
    let mut q = VecDeque::from([s]);
    while let Some((x, y)) = q.pop_front() {
        let d = dist[&(x, y)];
        for (ex, ey) in NEIGHBORS {
            let n = (x + ex, y + ey);
            if !inside(n.0, n.1) || dist.contains_key(&n) {
                continue;
            }
            if n == g {
                return Some(d + 1);
            }
            if w.layers.flags[w.grid.index(n.0 as u32, n.1 as u32)] & 1 != 0 {
                continue;
            }
            dist.insert(n, d + 1);
            q.push_back(n);
        }
    }
    None
}

#[test]
fn node_paths_match_reachability() {
    let p = s4_formats::game_dir().join(r"Map\Campaign\AO_maya1.map");
    if !p.exists() {
        eprintln!("skipping: {} not found", p.display());
        return;
    }
    let map = MapFile::open(p).unwrap();
    let (w, _) = new_game(&map, Rules::load_default().unwrap(), &NewGame::default()).unwrap();
    let size = w.grid.size() as i32;
    let mut r = Random16::new(11, 0);
    let (mut tried, mut found, mut walked) = (0, 0, 0);
    while tried < 400 {
        let s = (40 + (r.rand() % (size as u32 - 80)) as i32, 40 + (r.rand() % (size as u32 - 80)) as i32);
        let g = (s.0 + (r.rand() % 61) as i32 - 30, s.1 + (r.rand() % 61) as i32 - 30);
        if w.layers.flags[w.grid.index(s.0 as u32, s.1 as u32)] & 1 != 0 || s == g {
            continue;
        }
        tried += 1;
        let mut dirs = DirCache::default();
        let ok = astar64(&w, Variant::Normal, Y16X16::pack(s.0, s.1), Y16X16::pack(g.0, g.1), &mut dirs);
        let reach = bfs(&w, s, g);
        assert_eq!(ok, reach.is_some(), "start {s:?} goal {g:?}: search {ok}, reachable {reach:?}");
        if ok {
            found += 1;
            let (mut x, mut y) = s;
            let mut c = dirs;
            while c.count > 0 {
                let (ex, ey) = NEIGHBORS[c.back() as usize];
                x += ex;
                y += ey;
                c.pop_back();
                if (x, y) != g {
                    assert_eq!(w.layers.flags[w.grid.index(x as u32, y as u32)] & 1, 0, "path over a blocked node");
                }
            }
            if dirs.count < 16 {
                assert_eq!((x, y), g, "moves do not end at the goal");
                walked += 1;
            }
        }
    }
    eprintln!("{tried} searches: {found} found (= reachable in the window), {walked} walked to the goal");
}
