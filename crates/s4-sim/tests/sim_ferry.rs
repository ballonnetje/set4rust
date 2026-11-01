//! Ferries (`vehicles::ferry`, V5b), driven by `tick()` on roman02's sea.
//!
//! * `ferry_carries_settlers_and_a_cart`: a ferry on the open water off player 1's coast; a
//!   pioneer, a swordsman and a cart get one `SendTo` onto it (the click on the ferry, 0x5800b0 /
//!   0x59a240): each becomes a passenger record (0x58e760) and gets event 0x18 with its exit
//!   place on the shore; the ferry's own move to the first water node by that place fails (the
//!   coast's water is shallow, not open for ships), so it stands and everyone who checks in is
//!   taken aboard at once (0x58e6b0 → 0x58e0a0: ON_BOARD, off the map, the cart at position 0).
//!   The ferry sails 25 nodes along the coast (`SendTo`), 5009 unloads: each passenger gets a
//!   node near the land nearest to the ferry (0x58dd70) and event 0x19 and is on the map again;
//!   the list and the weights are empty. Deterministic.
//! * `deleted_ferry_kills_its_passengers`: deleting a loaded ferry kills the passenger aboard
//!   (0x58d560).
//!
//! Skipped when the game data is not present.

use s4_sim::entity::flag;
use s4_sim::grid::Y16X16;
use s4_sim::vehicles::{self, ferry, kind};
use s4_sim::{Command, PlayerCommand, World};

const PIONEER: u16 = 0x2f;

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

fn ground(w: &World, x: i32, y: i32) -> u8 {
    w.layers.ground[w.grid.index(x as u32, y as u32)]
}

fn pos(w: &World, id: u16) -> (i32, i32) {
    let e = w.entities.by_index(id as u32).unwrap();
    (e.x(), e.y())
}

fn dist(a: (i32, i32), b: (i32, i32)) -> i32 {
    s4_sim::spiral::grid_distance(a.0 - b.0, a.1 - b.1)
}

/// A ferry node on the spiral around (x, y) with land within 2.
fn coast(w: &World, x: i32, y: i32, r: u8) -> Option<(i32, i32)> {
    s4_sim::spiral::walk(w.grid, x, y, r).find(|&(px, py)| vehicles::can_place(w, px, py, kind::FERRY) && s4_sim::spiral::walk(w.grid, px, py, 6).any(|(a, b)| ground(w, a, b) > 8))
}

/// A free settler node on land near (x, y).
fn land(w: &World, x: i32, y: i32, r: u8) -> Option<(i32, i32)> {
    s4_sim::spiral::walk(w.grid, x, y, r).find(|&(px, py)| {
        let n = w.grid.index(px as u32, py as u32);
        ground(w, px, py) > 8 && w.layers.map_object[n] == 0 && w.layers.flags[n] & 0x7f == 0 && w.tiling.sector_of(n) != 0
    })
}

fn send(w: &mut World, ids: Vec<u16>, to: (i32, i32)) {
    s4_sim::tick(w, &[PlayerCommand { player: 1, command: Command::SendTo { x: to.0 as u16, y: to.1 as u16, mode: 0, entities: ids } }]);
}

const SWORDSMAN: u16 = 29;

struct Setup {
    w: World,
    f: u16,
    p: u16,
    s: u16,
    c: u16,
}

fn setup() -> Option<Setup> {
    let mut w = roman02()?;
    let fpos = coast(&w, 186, 90, 12)?;
    let f = vehicles::add_vehicle(&mut w, fpos.0, fpos.1, 1, kind::FERRY, 3, false)?;
    let lp = land(&w, 178, 92, 6)?;
    let p = s4_sim::create::add_settler(&mut w, lp.0, lp.1, 1, PIONEER)?;
    let ls = land(&w, 176, 88, 6)?;
    let s = s4_sim::create::add_settler(&mut w, ls.0, ls.1, 1, SWORDSMAN)?;
    let lc = s4_sim::spiral::walk(w.grid, 172, 94, 10).find(|&(x, y)| vehicles::can_place(&w, x, y, kind::CART))?;
    let c = vehicles::add_vehicle(&mut w, lc.0, lc.1, 1, kind::CART, 0, false)?;
    for _ in 0..5 {
        s4_sim::tick(&mut w, &[]);
    }
    Some(Setup { w, f, p, s, c })
}

fn on_map(w: &World, id: u16) -> bool {
    let e = w.entities.by_index(id as u32).unwrap();
    e.flags & flag::ON_BOARD == 0 && w.layers.map_object[w.grid.index(e.x() as u32, e.y() as u32)] == id
}

#[derive(Debug, PartialEq, Eq)]
struct Run {
    hash: u64,
    /// The tick each of pioneer, swordsman and cart came aboard.
    aboard: [Option<u32>; 3],
    sailed: (i32, i32),
    /// Where each went ashore.
    ashore: [(i32, i32); 3],
}

fn run() -> Option<Run> {
    let Setup { mut w, f, p, s, c } = setup()?;
    let ids = [p, s, c];
    let fp = pos(&w, f);
    send(&mut w, vec![p, s, c], fp);
    let mut aboard = [None; 3];
    let mut listed = [None; 3];
    for _ in 0..600 {
        s4_sim::tick(&mut w, &[]);
        w.events.clear();
        let d = ferry::data(&w, f)?;
        for (k, &id) in ids.iter().enumerate() {
            if listed[k].is_none() && d.passengers.iter().any(|r| r.id == id) {
                listed[k] = Some(w.tick);
            }
        }

        assert_eq!(vehicles::get(&w, f)?.task, vehicles::logic::task::IDLE, "the ferry stands");
        for (k, &id) in ids.iter().enumerate() {
            if aboard[k].is_none() && d.passengers.iter().any(|r| r.id == id && r.aboard) {
                aboard[k] = Some(w.tick);
                let e = w.entities.by_index(id as u32)?;
                assert!(e.flags & flag::ON_BOARD != 0, "{id} ON_BOARD");
                assert_eq!(e.flags & flag::ATTACHED, 0, "{id} detached");
                if id == c {
                    assert_eq!(e.pos, Y16X16(0), "the cart is at position 0");
                } else {
                    assert_ne!(w.layers.map_object[w.grid.index(e.x() as u32, e.y() as u32)], id, "{id} off the map");
                }
            }
        }
        if aboard.iter().all(Option::is_some) {
            break;
        }
    }
    eprintln!("listed at {listed:?}, aboard at {aboard:?}");
    let d = ferry::data(&w, f)?;
    assert!(aboard.iter().all(Option::is_some), "everyone aboard: {:?}", d.passengers);
    assert_eq!((d.aboard, d.reserved, d.state), (7, 7, 0));
    // Sail 25 nodes along the coast.
    let start = pos(&w, f);
    let to = coast(&w, 199, 111, 10)?;
    send(&mut w, vec![f], to);
    for _ in 0..1500 {
        s4_sim::tick(&mut w, &[]);
        w.events.clear();
        if dist(pos(&w, f), to) <= 3 && vehicles::get(&w, f)?.task == vehicles::logic::task::IDLE {
            break;
        }
    }
    let sailed = pos(&w, f);
    eprintln!("sailed {start:?} → {sailed:?} (to {to:?}) at tick {}", w.tick);
    assert!(dist(start, sailed) >= 20 && dist(sailed, to) <= 3, "the ferry sailed");
    assert_eq!(ferry::data(&w, f)?.aboard, 7, "still aboard");
    s4_sim::tick(&mut w, &[PlayerCommand { player: 1, command: Command::VehicleUnload { vehicle: f as u32 } }]);
    {
        let d = ferry::data(&w, f)?;
        assert!(d.passengers.is_empty() && d.aboard == 0 && d.reserved == 0, "unloaded: {d:?}");
    }
    for _ in 0..5 {
        s4_sim::tick(&mut w, &[]);
        w.events.clear();
    }
    let mut ashore = [(0, 0); 3];
    for (k, &id) in ids.iter().enumerate() {
        let e = w.entities.by_index(id as u32)?;
        // (A cart's own flags update, 0x55de00, takes SELECTABLE again unless it is a full
        // foundation cart.)
        assert!(e.flags & flag::ON_BOARD == 0 && (id == c || e.flags & flag::SELECTABLE != 0), "{id} ashore and selectable: {:#x}", e.flags);
        ashore[k] = pos(&w, id);
        assert!(dist(ashore[k], sailed) <= 12, "{id} ashore near the ferry: {:?}", ashore[k]);
        assert!(ground(&w, ashore[k].0, ashore[k].1) > 8, "on land");
    }
    assert!(on_map(&w, p) && on_map(&w, s), "the settlers hold their nodes");
    assert_eq!(w.layers.map_object[w.grid.index(ashore[2].0 as u32, ashore[2].1 as u32)], c, "the cart holds its node");
    eprintln!("ashore at {ashore:?}");
    Some(Run { hash: w.state_hash(), aboard, sailed, ashore })
}

#[test]
fn ferry_carries_settlers_and_a_cart() {
    let Some(r) = run() else {
        eprintln!("skipping: no game data");
        return;
    };
    assert_eq!(run().unwrap(), r, "two runs differ");
}

#[test]
fn deleted_ferry_kills_its_passengers() {
    let Some(Setup { mut w, f, p, .. }) = setup() else { return };
    let fp = pos(&w, f);
    send(&mut w, vec![p], fp);
    for _ in 0..300 {
        s4_sim::tick(&mut w, &[]);
        w.events.clear();
        if ferry::data(&w, f).is_some_and(|d| d.aboard == 1) {
            break;
        }
    }
    assert_eq!(ferry::data(&w, f).unwrap().aboard, 1, "the pioneer came aboard");
    vehicles::delete(&mut w, f);
    let e = w.entities.by_index(p as u32);
    assert!(e.is_none_or(|e| e.flags & flag::DIED != 0), "the pioneer died with the ferry");
}
