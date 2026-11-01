//! `CWorkshopBuildingRole` on roman02, driven by `tick()` only: a vehicle hall (type 23) orders
//! its worker, the player orders a cart (command 5040), the hall places the cart under
//! construction at its `workingpos` (`AddVehicle(.., 1)`, BIRTH), the worker brings it the boards
//! and ironbars the carriers deliver; the built cart calls a donkey (`CCart` 0x55dc50: "no
//! donkey" 0x9e0 every 16 ticks without one), the donkey walks to it and is hitched (0x576d80 →
//! 0x55dd50: the donkey is deleted), and the cart is finished (BIRTH off, counted finished). The
//! order is used up when the cart is placed. A second run orders a catapult and cancels it (command 5030: the
//! vehicle is deleted). roman02 has no iron, so a free pile of 8 iron is put down near the
//! castle. Skipped when the game data is not present.

use s4_sim::build::possible_sites;
use s4_sim::buildings::workshop;
use s4_sim::entity::{EntityData, flag};
use s4_sim::vehicles::{self, kind};
use s4_sim::{Command, PlayerCommand, World};

const VEHICLE_HALL: u8 = 23;
const IRON: u8 = 20;
const DONKEY: u16 = 60;

fn roman02() -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map");
    if !p.exists() {
        return None;
    }
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    let rules = s4_sim::Rules::load_default().ok()?;
    let (mut w, _) = s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).ok()?;
    let towers: Vec<(i32, i32)> = w
        .entities
        .iter()
        .filter_map(|(id, e)| match &e.data {
            EntityData::Building(_) if s4_sim::buildings::have_inhabitant(&w, id) => Some((e.x(), e.y())),
            _ => None,
        })
        .collect();
    for (x, y) in towers {
        s4_sim::territory::set_owner(&mut w, x, y);
    }
    Some(w)
}

/// roman02 with a vehicle hall near player 1's castle and 8 iron on a free node.
fn setup() -> Option<(World, u16)> {
    let mut w = roman02()?;
    let castle = w.entities.iter().find(|(id, e)| e.owner == 1 && matches!(&e.data, EntityData::Building(_)) && s4_sim::buildings::have_inhabitant(&w, *id)).map(|(_, e)| (e.x(), e.y()))?;
    let size = w.grid.size() as i32;
    let &(x, y, _) = possible_sites(&w, 1, VEHICLE_HALL, (0, 0), (size, size))
        .iter()
        .min_by_key(|s| (s.0 as i32 - castle.0).abs() + (s.1 as i32 - castle.1).abs())?;
    let hall = s4_sim::create::add_building(&mut w, x as i32, y as i32, 1, VEHICLE_HALL).expect("vehicle hall");
    let k = (s4_sim::spiral::first(6)..s4_sim::spiral::last(12)).find(|&k| {
        let n = w.grid.index((castle.0 + s4_sim::spiral::dx(k)) as u32, (castle.1 + s4_sim::spiral::dy(k)) as u32);
        w.layers.flags[n] & 0x7F == 0 && w.layers.map_object[n] == 0 && w.layers.deco_object[n] == 0
    })?;
    let (px, py) = (castle.0 + s4_sim::spiral::dx(k), castle.1 + s4_sim::spiral::dy(k));
    s4_sim::create::add_pile(&mut w, px, py, IRON, 8, s4_sim::pile::role::ANYWHERE).expect("iron pile");
    Some((w, hall))
}

fn order(hall: u16, item: u16, amount: u32) -> PlayerCommand {
    PlayerCommand { player: 1, command: Command::WorkshopOrder { building: hall, item, amount } }
}

#[derive(Debug, PartialEq, Eq)]
struct Run {
    hash: u64,
    /// Tick the vehicle appeared under construction, its id, the tick it finished.
    placed: Option<(u32, u16)>,
    finished: Option<u32>,
    /// Ticks of the "no donkey" (0x9e0) and "donkey called" (0x776) messages.
    no_donkey: Vec<u32>,
    called: Vec<u32>,
    /// The donkey: the tick it was deleted.
    donkey_gone: Option<u32>,
    /// Progress (+0x70) seen over the run, and the end (+0x72).
    progress: Vec<u16>,
    end: u16,
}

fn run(ticks: u32, with_donkey: bool) -> Option<Run> {
    let (mut w, hall) = setup()?;
    // A donkey of player 1 a few nodes from the hall, on its sector.
    let mut donkey = 0;
    if with_donkey {
        let b = w.entities.by_index(hall as u32)?;
        let EntityData::Building(d) = &b.data else { return None };
        let at = d.working_area;
        let sector = w.tiling.sector_of(w.grid.index(at.x() as u32, at.y() as u32));
        let (dx, dy) = s4_sim::spiral::walk(w.grid, at.x() + 8, at.y() + 4, 12).find(|&(x, y)| {
            let n = w.grid.index(x as u32, y as u32);
            w.layers.map_object[n] == 0 && w.owner_of(n) == 1 && w.tiling.sector_of(n) == sector
        })?;
        donkey = s4_sim::create::add_settler(&mut w, dx, dy, 1, DONKEY)?;
    }
    let (mut no_donkey, mut called, mut donkey_gone) = (Vec::new(), Vec::new(), None);
    let s = workshop::state(&w, hall)?;
    assert_eq!((s.work.state, s.vehicle, s.orders), (1, 0, [0; 5]));
    assert!(!s.piles.is_empty(), "no input piles");
    let cmds = [order(hall, kind::CART, 1)];
    let (mut placed, mut finished, mut progress, mut end) = (None, None, Vec::new(), 0);
    for t in 0..ticks {
        s4_sim::tick(&mut w, if t == 0 { &cmds } else { &[] });
        if t == 0 {
            assert_eq!(workshop::state(&w, hall)?.orders, [0, 0, 0, 0, 1], "5040 counted");
        }
        let s = workshop::state(&w, hall)?;
        if placed.is_none() && s.vehicle != 0 {
            placed = Some((w.tick, s.vehicle));
            let e = w.entities.by_index(s.vehicle as u32)?;
            assert_eq!((e.kind, e.owner), (kind::CART, 1));
            assert!(e.flags & flag::BIRTH != 0, "under construction");
            assert_eq!(s.orders, [0; 5], "the order is used up when the vehicle is placed");
            let b = w.entities.by_index(hall as u32)?;
            let EntityData::Building(d) = &b.data else { unreachable!() };
            assert_eq!(e.pos, d.working_area, "made at the workingpos");
            assert_eq!(w.vehicles.building[kind::CART as usize + 6], 1);
        }
        for e in std::mem::take(&mut w.events) {
            if let s4_sim::event::SimEvent::Message { text, .. } = e {
                match text {
                    0x9e0 => no_donkey.push(w.tick),
                    0x776 => called.push(w.tick),
                    _ => {}
                }
            }
        }
        if donkey != 0 && donkey_gone.is_none() && w.entities.by_index(donkey as u32).is_none_or(|e| e.is_dead()) {
            donkey_gone = Some(w.tick);
        }
        if let Some((_, v)) = placed {
            if let Some(x) = vehicles::get(&w, v) {
                end = x.progress[1];
                if progress.last() != Some(&x.progress[0]) {
                    progress.push(x.progress[0]);
                }
            }
            if finished.is_none() && w.entities.by_index(v as u32).is_some_and(|e| e.flags & flag::BIRTH == 0) {
                finished = Some(w.tick);
            }
        }
    }
    Some(Run { hash: w.state_hash(), placed, finished, no_donkey, called, donkey_gone, progress, end })
}

#[test]
fn vehicle_hall_builds_an_ordered_cart() {
    let Some(r) = run(12000, true) else {
        eprintln!("skipping: no game data or no site");
        return;
    };
    eprintln!("{r:?}");
    assert!(r.placed.is_some(), "no cart was placed");
    assert!(r.end > 0);
    assert_eq!(r.progress, (0..=r.end).collect::<Vec<_>>(), "one good at a time");
    assert!(r.no_donkey.is_empty(), "a donkey was there");
    assert_eq!(r.called.len(), 1, "the donkey is called once");
    let (called, gone, finished) = (r.called[0], r.donkey_gone.expect("the donkey was not hitched"), r.finished.expect("the cart did not finish"));
    assert!(called < gone && gone == finished, "called {called}, hitched {gone}, finished {finished}");
    assert_eq!(run(12000, true).unwrap(), r, "two runs differ");
}

#[test]
fn a_built_cart_waits_for_a_donkey() {
    let Some(r) = run(9000, false) else { return };
    eprintln!("placed {:?} progress {:?} no donkey {:?}", r.placed, r.progress.last(), r.no_donkey.first());
    assert_eq!(r.progress.last(), Some(&r.end), "the cart was not built");
    assert!(r.finished.is_none(), "finished without a donkey");
    assert!(r.no_donkey.len() >= 2 && r.no_donkey.windows(2).all(|p| p[1] - p[0] == 16), "\"no donkey\" every 16 ticks: {:?}", r.no_donkey);
}

#[test]
fn cancel_deletes_the_vehicle_under_construction() {
    let Some((mut w, hall)) = setup() else { return };
    s4_sim::tick(&mut w, &[order(hall, kind::WAR_MACHINE, 100)]);
    assert_eq!(workshop::state(&w, hall).unwrap().orders[3], 100, "endless");
    let mut v = 0;
    for _ in 0..6000 {
        s4_sim::tick(&mut w, &[]);
        v = workshop::state(&w, hall).unwrap().vehicle;
        if v != 0 {
            break;
        }
    }
    assert!(v != 0, "no catapult was placed");
    assert_eq!(workshop::state(&w, hall).unwrap().orders[3], 100, "endless stays");
    s4_sim::tick(&mut w, &[PlayerCommand { player: 1, command: Command::CancelProduction { building: hall as u32 } }]);
    assert_eq!(workshop::state(&w, hall).unwrap().vehicle, 0);
    assert!(w.entities.by_index(v as u32).is_none_or(|e| e.is_dead()), "the catapult is gone");
    assert_eq!(w.vehicles.building[kind::WAR_MACHINE as usize + 6], 0);
}

#[test]
fn take_order_counts() {
    let Some((mut w, hall)) = setup() else { return };
    let o = |w: &World| workshop::state(w, hall).unwrap().orders[4];
    for (amount, want) in [(5u32, 5u8), (u32::MAX, 4), (200, 99), (0, 0), (100, 100), (3, 100), (u32::MAX - 9, 100)] {
        s4_sim::tick(&mut w, &[order(hall, kind::CART, amount)]);
        assert_eq!(o(&w), want, "amount {}", amount as i32);
    }
    s4_sim::tick(&mut w, &[order(hall, 0, 5), order(hall, 6, 5)]);
    assert_eq!(workshop::state(&w, hall).unwrap().orders, [0, 0, 0, 0, 100]);
}
