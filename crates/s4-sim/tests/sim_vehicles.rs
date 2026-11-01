//! Vehicles in the simulation (`vehicles`): creation (`AddVehicle` 0x596ad0 with the ctor
//! fields), the land footprint (0x59a4c0: the node and the placeholders 1..=6 around it), the
//! manager's list and counters, and a cart sent somewhere (`SendTo` → event 0x11 / 0xd →
//! GO_TO_POS → `CWalkingCatapult` steps, 0x59a3a0) that arrives and stands. Skipped without the
//! game data.

use s4_sim::entity::{EntityData, ObjType};
use s4_sim::vehicles::{self, kind};
use s4_sim::{Command, PlayerCommand, World};

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

/// The footprint at the vehicle's node: itself there, the six neighbours 1..=6.
fn footprint_ok(w: &World, id: u16) -> bool {
    let e = w.entities.by_index(id as u32).unwrap();
    let (x, y) = (e.x(), e.y());
    let n = w.grid.index(x as u32, y as u32);
    let nb = [(1, 0), (1, 1), (0, 1), (-1, 0), (-1, -1), (0, -1)];
    w.layers.map_object[n] == id && nb.iter().enumerate().all(|(d, &(dx, dy))| w.layers.map_object[w.grid.index((x + dx) as u32, (y + dy) as u32)] == d as u16 + 1)
}

/// A free spot for a land vehicle near (x, y): catapult ground, the seven nodes free.
fn free_spot(w: &World, x: i32, y: i32) -> Option<(i32, i32)> {
    s4_sim::spiral::walk(w.grid, x, y, 20).find(|&(px, py)| {
        let ok = |qx: i32, qy: i32| {
            w.grid.checked_index(qx, qy).is_some_and(|n| w.layers.map_object[n] == 0 && w.tiling.catapult[n] != 0 && w.tiling.catapult[n] < 0xc000 && w.layers.move_cost(n) < 7)
        };
        ok(px, py) && [(1, 0), (1, 1), (0, 1), (-1, 0), (-1, -1), (0, -1)].iter().all(|&(dx, dy)| ok(px + dx, py + dy))
    })
}

#[derive(Debug, PartialEq, Eq)]
struct Run {
    hash: u64,
    start: (i32, i32),
    end: (i32, i32),
    arrived_at: Option<u32>,
    steps: usize,
}

fn run(ticks: u32) -> Option<Run> {
    let mut w = roman02()?;
    let pl = w.player(1)?.clone();
    let (sx, sy) = free_spot(&w, pl.start_x as i32 + 8, pl.start_y as i32 + 8)?;
    let cart = vehicles::add_vehicle(&mut w, sx, sy, 1, kind::CART, -1, false)?;
    let e = w.entities.by_index(cart as u32)?;
    assert_eq!((e.obj_type, e.kind, e.owner), (ObjType::LandVehicle, kind::CART, 1));
    assert_eq!(e.flags & 0x0300_0100, 0x0200_0100, "READY and VISIBLE, not BIRTH");
    assert!(footprint_ok(&w, cart), "footprint at the start");
    assert_eq!(w.vehicles.head(1, kind::CART), cart, "head of its list");
    let i = kind::CART as usize + 6;
    assert_eq!((w.vehicles.finished[i], w.vehicles.stat_made[i]), (1, 1), "counted");
    let v = vehicles::get(&w, cart)?;
    let info = w.rules.vehicles.get(e.race, kind::CART as u8);
    assert_eq!((v.health as i32, v.speed as i32), (info.hitpoints, info.walk_steps), "ctor fields from the vehicle table");
    // +0x26 / +0x61: the type's first job of Gfx\<30 + race>.gfx and its direction-2 frames.
    assert_eq!(v.job_part, w.rules.vehicles.first_job(e.race, kind::CART as u8));
    assert!(v.frames > 0, "the ctor's frame count");
    let mut frames_seen = std::collections::BTreeSet::new();
    // A target 12 nodes away.
    let (tx, ty) = free_spot(&w, sx + 12, sy + 3)?;
    let order = PlayerCommand { player: 1, command: Command::SendTo { x: tx as u16, y: ty as u16, mode: 0, entities: vec![cart] } };
    let (mut arrived_at, mut steps, mut last) = (None, 0, (sx, sy));
    for t in 0..ticks {
        s4_sim::tick(&mut w, if t == 0 { std::slice::from_ref(&order) } else { &[] });
        let e = w.entities.by_index(cart as u32)?;
        let p = (e.x(), e.y());
        if p != last {
            steps += 1;
            last = p;
            assert!(footprint_ok(&w, cart), "footprint after a step at tick {}", w.tick);
        }
        if let Some(v) = vehicles::get(&w, cart) {
            frames_seen.insert(v.frame);
        }
        if arrived_at.is_none() && p == (tx, ty) {
            arrived_at = Some(w.tick);
        }
    }
    assert!(frames_seen.len() > 1, "the walk animates: frames {frames_seen:?}");
    s4_sim::warmap::check(&w).ok()?;
    Some(Run { hash: w.state_hash(), start: (sx, sy), end: last, arrived_at, steps })
}

#[test]
fn cart_drives_to_its_place() {
    let Some(r) = run(600) else {
        eprintln!("skipping: no game data or no room for a cart");
        return;
    };
    eprintln!("{r:?}");
    assert!(r.arrived_at.is_some(), "the cart did not arrive");
    assert!(r.steps >= 12, "steps {}", r.steps);
    assert_eq!(run(600).unwrap(), r, "two runs differ");
}
