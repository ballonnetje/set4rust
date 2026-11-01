//! Pioneers (`CPioneerRole`, role 11) on MCD2_roman1, driven by `tick()` and a SendTo command
//! only: three pioneers of player 1, sent onto unowned land beyond the border, claim it node by
//! node (0x57edf0 / 0x57efb0 / 0x57f0a0). Skipped when the game data is not present.

use s4_sim::World;

fn map(name: &str) -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join(name);
    if !p.exists() {
        return None;
    }
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    let rules = s4_sim::Rules::load_default().ok()?;
    s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).ok().map(|(w, _)| w)
}

const PIONEER: u16 = 47;
const PLAYER: u8 = 1;

#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    hash: u64,
    claimed: usize,
    target: (i32, i32),
}

/// An own free land node next to unowned land, and a free unowned land node 6 further out on
/// the same sector.
fn border(w: &World) -> ((i32, i32), (i32, i32)) {
    let size = w.grid.size() as i32;
    // (Own land has the influence flag 0x80.)
    let land = |x: i32, y: i32| {
        w.grid.checked_index(x, y).is_some_and(|n| w.layers.ground[n] & 0xf0 == 0x10 && w.layers.map_object[n] == 0 && w.layers.flags[n] & 0x39 == 0)
    };
    for y in 20..size - 20 {
        for x in 20..size - 20 {
            let n = w.grid.index(x as u32, y as u32);
            if w.owner_of(n) != PLAYER || !land(x, y) {
                continue;
            }
            let (tx, ty) = (x + 8, y);
            let m = w.grid.index(tx as u32, ty as u32);
            if w.owner_of(m) == 0
                && land(tx, ty)
                && w.layers.flags[m] & 0x80 == 0
                && w.tiling.sector_of(m) == w.tiling.sector_of(n)
                && (1..=8).all(|d| w.grid.checked_index(x + d, y).is_some_and(|k| w.layers.ground[k] & 0xf0 == 0x10))
            {
                return ((x, y), (tx, ty));
            }
        }
    }
    panic!("no border to unowned land");
}

fn run(ticks: u32) -> Option<Outcome> {
    let mut w = map("MCD2_roman1.map")?;
    let ((x, y), (tx, ty)) = border(&w);
    let before: Vec<u8> = (0..w.layers.flags.len()).map(|n| w.owner_of(n)).collect();
    let mut ps = Vec::new();
    for (px, py) in s4_sim::spiral::walk(w.grid, x, y, 4) {
        let n = w.grid.index(px as u32, py as u32);
        if ps.len() < 3 && w.owner_of(n) == PLAYER && w.layers.map_object[n] == 0 && w.layers.ground[n] & 0xf0 == 0x10 {
            ps.push(s4_sim::create::add_settler(&mut w, px, py, PLAYER, PIONEER).expect("a pioneer"));
        }
    }
    assert_eq!(ps.len(), 3);
    for &p in &ps {
        let e = w.entities.by_index(p as u32).unwrap();
        assert!(matches!(&e.data, s4_sim::entity::EntityData::Settler(s) if s.job.role == s4_sim::jobs::role::PIONEER));
    }
    let cmd = s4_sim::PlayerCommand { player: PLAYER, command: s4_sim::Command::SendTo { x: tx as u16, y: ty as u16, mode: 0, entities: ps.clone() } };
    s4_sim::tick(&mut w, &[cmd]);
    for _ in 0..ticks {
        w.events.clear();
        s4_sim::tick(&mut w, &[]);
        for e in &w.events {
            if let s4_sim::event::SimEvent::Unported { what, .. } = e {
                assert!(!what.contains("Pioneer"), "{what}");
            }
        }
    }
    let mut claimed = 0;
    for (n, &old) in before.iter().enumerate() {
        let now = w.owner_of(n);
        if now == old {
            continue;
        }
        let (nx, ny) = ((n % w.grid.size() as usize) as i32, (n / w.grid.size() as usize) as i32);
        // (The players' soldiers move borders elsewhere on the map.) Near the place only the
        // pioneers change owners: unowned land, taken by their player.
        if s4_sim::spiral::grid_distance(nx - tx, ny - ty) > 24 {
            continue;
        }
        assert_eq!((old, now), (0, PLAYER), "node ({nx}, {ny})");
        claimed += 1;
    }
    // The reservations left near the place are the pioneers' destinations. (Other settlers
    // reserve nodes with flag 0x20 too, elsewhere.)
    let size = w.grid.size() as usize;
    let reserved: Vec<(i32, i32)> = (0..w.layers.flags.len())
        .filter(|&n| w.layers.flags[n] & 0x20 != 0 && w.owner_of(n) != PLAYER)
        .map(|n| ((n % size) as i32, (n / size) as i32))
        .filter(|&(nx, ny)| s4_sim::spiral::grid_distance(nx - tx, ny - ty) <= 24)
        .collect();
    assert!(reserved.len() <= ps.len(), "reserved nodes {reserved:?}");
    for &p in &ps {
        let e = w.entities.by_index(p as u32).unwrap();
        if let s4_sim::entity::EntityData::Settler(s) = &e.data {
            assert_eq!(s.job.list_id, 192, "pioneer {p} works");
        }
    }
    Some(Outcome { hash: w.state_hash(), claimed, target: (tx, ty) })
}

#[test]
fn pioneers_claim_land() {
    let Some(a) = run(3000) else { return };
    eprintln!("{a:?}");
    assert!(a.claimed >= 20, "{a:?}");
    assert_eq!(Some(a), run(3000), "deterministic");
}
