//! `CFogging` (`s4_sim::fog`): the cycle on a small synthetic world, and the static levels against
//! the fog bits the original's savegames hold (chunk 135, `CFogging::Save`), skipped when absent.

use std::path::{Path, PathBuf};

use s4_formats::map::MapFile;
use s4_sim::entity::{EntityData, ObjType, VehicleData, flag};
use s4_sim::save::SaveGame;
use s4_sim::{Entity, Grid, Rules, World, Y16X16};

fn hex(dx: i32, dy: i32) -> i32 {
    s4_sim::spiral::grid_distance(dx, dy)
}

/// 64×64 grass, player 1 owning the hex disc of radius 5 around (32, 32).
fn world() -> Option<World> {
    let rules = Rules::load_default().ok()?;
    let g = Grid::new(64).unwrap();
    let mut w = World::new(g, rules);
    w.layers.ground.fill(16);
    let owner: Vec<u8> = (0..g.len()).map(|i| (hex(g.x(i) as i32 - 32, g.y(i) as i32 - 32) <= 5) as u8).collect();
    w.init_tiling(&owner);
    Some(w)
}

fn new_level(w: &World, x: i32, y: i32) -> u8 {
    let f = w.fog.as_ref().unwrap();
    (f.gfx()[w.grid.index(x as u32, y as u32)] >> 3) & 7
}

fn old_level(w: &World, x: i32, y: i32) -> u8 {
    w.fog.as_ref().unwrap().gfx()[w.grid.index(x as u32, y as u32)] & 7
}

#[test]
fn fog_cycle_on_a_small_world() {
    let Some(mut w) = world() else {
        eprintln!("skipping: no game data");
        return;
    };
    w.start_fog(1, None);
    let g = w.grid;
    {
        let f = w.fog.as_ref().unwrap();
        for i in 0..g.len() {
            let (x, y) = (g.x(i) as i32, g.y(i) as i32);
            let d = hex(x - 32, y - 32);
            let s = f.static_level(i);
            // Territory 0, its neighbours 1, rings up to 6 around it, black beyond.
            match d {
                ..=5 => assert_eq!(s, 0, "({x}, {y})"),
                6 => assert_eq!(s, 1, "({x}, {y})"),
                7..=11 => assert!((1..=6).contains(&s), "({x}, {y}): {s}"),
                _ => assert_eq!(s, 7, "({x}, {y})"),
            }
            // No entities: both gfx levels are the static level.
            assert_eq!(f.gfx()[i], s | s << 3);
        }
        assert_eq!(f.fade(), 0);
    }

    // A land vehicle (sight 20) far from the territory.
    let mut e = Entity::new(ObjType::LandVehicle, 1, 1, 0, Y16X16::pack(10, 54), EntityData::Vehicle(VehicleData::default()));
    e.flags = flag::READY;
    let v = w.entities.alloc(e).unwrap().index;
    let mut fades = Vec::new();
    for _ in 0..40 {
        s4_sim::tick(&mut w, &[]);
        fades.push(w.fog.as_ref().unwrap().fade());
    }
    // Phase 1: fade 1..15 then 15; phase 2: 15 (SetFoggingUpdateMode).
    assert_eq!(&fades[..16], &(1..=15).chain([15]).collect::<Vec<_>>()[..]);
    assert!(fades.iter().skip(15).all(|&f| f == 15));
    assert_eq!(new_level(&w, 10, 54), 0);
    assert_eq!(old_level(&w, 10, 54), 7);
    for (dx, want) in [(20, 0), (21, 1), (24, 4), (26, 6), (27, 7)] {
        assert_eq!(new_level(&w, 10 + dx, 54), want, "dx {dx}");
    }
    // Rows reached by its sight rings (radius 20 + 6) changed.
    let top = 54 + (0..=s4_sim::spiral::last(26)).map(s4_sim::spiral::dy).min().unwrap();
    assert_eq!(w.fog.as_ref().unwrap().changed_rows(), (top, 63));
    // The next phase 1 shows the old level until the fade completes.
    assert!(!w.fog.as_ref().unwrap().is_position_visible(10, 54));
    for _ in 0..15 {
        s4_sim::tick(&mut w, &[]);
    }
    assert!(w.fog.as_ref().unwrap().is_position_visible(10, 54));

    // The vehicle leaves (phase 1 step 15: this cycle already saw it): one more full cycle and
    // what it saw stays explored (3).
    w.entities.by_index_mut(v as u32).unwrap().pos = Y16X16::pack(60, 2);
    for _ in 0..25 {
        s4_sim::tick(&mut w, &[]);
    }
    assert_eq!((new_level(&w, 10, 54), old_level(&w, 10, 54)), (0, 0));
    for _ in 0..40 {
        s4_sim::tick(&mut w, &[]);
    }
    assert_eq!((new_level(&w, 10, 54), old_level(&w, 10, 54)), (3, 0));
    for _ in 0..15 {
        s4_sim::tick(&mut w, &[]);
    }
    assert_eq!((new_level(&w, 34, 54), new_level(&w, 36, 54)), (4, 6), "ring levels above 3 stay");
    assert!(!w.fog.as_ref().unwrap().is_position_visible(10, 54));

    // Owner changes reach the static layer through the tiling (UpdateOwnerFogging).
    let edge = g.index(37, 32);
    w.change_owner(37, 32, 0);
    // Lost: 3, then the radius of the nearest own node within 2 (its neighbour (36, 32)).
    assert_eq!(w.fog.as_ref().unwrap().static_level(edge), 1);
    w.change_owner(38, 32, 1);
    let f = w.fog.as_ref().unwrap();
    assert_eq!(f.static_level(g.index(38, 32)), 0);
    assert_eq!(f.static_level(g.index(39, 32)), 1);
}

fn saves() -> Vec<PathBuf> {
    fn walk(d: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(d) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out)
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("sav")) {
                out.push(p)
            }
        }
    }
    let home = std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_default();
    let mut v = Vec::new();
    walk(&home.join(r"Documents\TheSettlers4\Save"), &mut v);
    v.sort();
    v
}

/// Chunk 135 (`CFogging::Save`): the gfx fog bits & 0x3F, version 0x6EABCB07, the spot count and
/// spots, the alliance option, end mark 0x6EABCBFF. Against our `Start` on the saved territory
/// of the local player (the one whose land is all visible): no saved new level is darker than
/// our static level (the gfx update only ever brightens below it), and `Load` + `Start` keep the
/// saved bits.
#[test]
fn saved_fog_matches_static_levels() {
    let Ok(rules) = Rules::load_default() else { return };
    let mut stats = (0u64, 0u64, 0u64, 0u64);
    for f in saves() {
        let s = SaveGame::open(&f).unwrap();
        let m = MapFile::open(&f).unwrap();
        let Some((_, c)) = m.container.load_id(135, 0).unwrap() else { continue };
        let n = (s.size * s.size) as usize;
        let le = |at: usize| u32::from_le_bytes(c[at..at + 4].try_into().unwrap());
        let spots = le(n + 4) as usize;
        assert_eq!((le(n), c.len()), (0x6EAB_CB07, n + 16 + 16 * spots), "{}", f.display());
        assert_eq!(le(c.len() - 4), 0x6EAB_CBFF);
        assert!(c[..n].iter().all(|&b| b & 0xC0 == 0));
        let mut w = World::new(Grid::new(s.size).unwrap(), rules.clone());
        w.layers.flags.copy_from_slice(&s.flags);
        for (i, r) in s.render.as_chunks::<4>().0.iter().enumerate() {
            w.layers.ground[i] = r[1];
            w.layers.gfx_bits[i] = r[3];
        }
        w.init_tiling(&s.owner);
        // The local player: the one whose territory is all visible.
        let local = (1..=8u8).find(|&p| {
            let mut any = false;
            let ok = (0..n).all(|i| w.owner_of(i) != p || {
                any = true;
                (c[i] >> 3) & 7 == 0
            });
            ok && any
        });
        let Some(local) = local else {
            eprintln!("  no local player");
            continue;
        };
        w.start_fog(local, None);
        let fog = w.fog.as_ref().unwrap();
        for (i, &b) in c[..n].iter().enumerate() {
            let (new, st) = ((b >> 3) & 7, fog.static_level(i));
            stats.0 += 1;
            stats.1 += (new > st) as u64;
            stats.2 += (new < st) as u64;
            stats.3 += (st < 7 && new == st) as u64;
        }
        w.start_fog(local, Some(&c));
        assert_eq!(w.fog.as_ref().unwrap().gfx(), &c[..n]);
    }
    eprintln!("nodes {}: darker than static {}, lighter {}, near territory and equal {}", stats.0, stats.1, stats.2, stats.3);
    assert_eq!(stats.1, 0);
}

/// The fog is not simulation state: a world with fog ticks to the same hash as one without.
#[test]
fn fog_does_not_change_the_simulation() {
    let Some(mut a) = world() else { return };
    let mut b = a.clone();
    b.start_fog(1, None);
    for _ in 0..100 {
        s4_sim::tick(&mut a, &[]);
        s4_sim::tick(&mut b, &[]);
    }
    assert_eq!(a.state_hash(), b.state_hash());
}
