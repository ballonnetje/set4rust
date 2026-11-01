//! `CAStarTiling::FindPath` on a real map: between land nodes of one sector a path exists and
//! is a chain of linked tiles; across sectors there is none (or a partial one with flag 0x40).
//! Skipped when the install is absent.

use s4_formats::map::MapFile;
use s4_sim::pathing::{Alliances, Waypoints, astar_tiling};
use s4_sim::rng::Random16;
use s4_sim::{NewGame, Rules, Y16X16, new_game};

#[test]
fn tiling_paths_follow_sectors() {
    let p = s4_formats::game_dir().join(r"Map\Campaign\AO_maya1.map");
    if !p.exists() {
        eprintln!("skipping: {} not found", p.display());
        return;
    }
    let map = MapFile::open(p).unwrap();
    let (w, _) = new_game(&map, Rules::load_default().unwrap(), &NewGame::default()).unwrap();
    let t = &w.tiling;
    let size = w.grid.size() as i32;
    let side = size as usize / 16;
    let al = Alliances::default();
    let mut r = Random16::new(3, 0);
    let (mut same, mut found, mut cross, mut cross_none, mut partial, mut checked) = (0, 0, 0, 0, 0, 0);
    let mut wp = Waypoints::default();
    while same < 300 || cross < 100 {
        let a = Y16X16::pack((r.rand() % size as u32) as i32, (r.rand() % size as u32) as i32);
        let b = Y16X16::pack((r.rand() % size as u32) as i32, (r.rand() % size as u32) as i32);
        let (ia, ib) = (w.grid.index(a.x() as u32, a.y() as u32), w.grid.index(b.x() as u32, b.y() as u32));
        let (ta, tb) = (t.normal[ia], t.normal[ib]);
        if ta < 10 || tb < 10 || t.tile(ta).ty & 0x1000_0000 == 0 || t.tile(tb).ty & 0x1000_0000 == 0 {
            continue;
        }
        let res = astar_tiling(t, &al, &w.warmap, side, a, b, 0, &mut wp);
        if t.tile(ta).sector == t.tile(tb).sector {
            same += 1;
            if res == 1 {
                found += 1;
                if ta == tb {
                    assert_eq!((wp.goal, wp.back()), (-1, b));
                    continue;
                }
                assert_eq!(wp.goal, b.0 as i32);
                // Start tile, then the waypoints from the back (nearest the start) to the goal:
                // consecutive ones lie on linked tiles.
                if wp.total <= 16 {
                    let mut tiles = vec![ta];
                    let mut c = wp.clone();
                    while c.count > 0 {
                        let p = c.back();
                        tiles.push(t.normal[w.grid.index(p.x() as u32, p.y() as u32)]);
                        c.pop_back();
                    }
                    for pair in tiles.windows(2) {
                        assert!(t.tile(pair[0]).links.contains(&pair[1]), "tiles {} -> {} not linked", pair[0], pair[1]);
                    }
                    checked += 1;
                }
            }
        } else {
            cross += 1;
            if res == 0 {
                cross_none += 1;
            }
            if astar_tiling(t, &al, &w.warmap, side, a, b, 0x40, &mut wp) == -1 {
                partial += 1;
            }
        }
    }
    eprintln!("same sector {same}: found {found} ({checked} chains checked); cross {cross}: none {cross_none}, partial with 0x40 {partial}");
    assert_eq!(found, same);
    assert_eq!(cross_none, cross);
}
