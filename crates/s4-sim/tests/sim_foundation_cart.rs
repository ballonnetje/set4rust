//! The foundation cart (`CCart` +0xbc; commands 5012 `CreateFoundationCart` 0x55c360, 5011
//! `CreateSettlement` 0x55c130), driven by `tick()` on roman02: a finished cart on player 1's
//! land next to the castle, 8 pioneers near it and 2 hammers on a free pile (the castle has the
//! boards and stones).
//!
//! * 5012 makes it a foundation cart (8 pioneers and 2 carriers to call, 4 boards, 8 stones,
//!   2 hammers, +0xbf 14, VULNERABLE, task 0x1b); every 16 ticks it calls settlers (0x55ce00:
//!   event 0x18) and orders goods (0x55d660: carriers with event 11, list 153);
//! * the pioneers and carriers walk behind it and check in (0x55d0c0: inside, ON_BOARD, off the
//!   map); the carriers bring the goods (LOAD_GOOD → 0x55d7e0);
//! * full (8 pioneers, 2 carriers, 14 goods) it is SELECTABLE and drives (5014 `SendTo`) out of
//!   the territory; 5011 there claims the land within radius 3 (`CTiling` vtbl+0x1c), lets the
//!   10 settlers out (0x55d460: events 0x19 / move), puts the goods down (0x55d910) and turns
//!   into a pile of 5 boards (0x55c250); the pioneers are on the map again a tick later.
//!
//! Skipped when the game data is not present.

use s4_sim::entity::{EntityData, ObjType, flag};
use s4_sim::vehicles::{self, cart, kind};
use s4_sim::{Command, PlayerCommand, World};

const PIONEER: u16 = 0x2f;
const HAMMER: u8 = 0x12;

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

fn free_node(w: &World, x: i32, y: i32, r: u8, owner: Option<u8>) -> Option<(i32, i32)> {
    s4_sim::spiral::walk(w.grid, x, y, r).find(|&(px, py)| {
        let n = w.grid.index(px as u32, py as u32);
        w.layers.map_object[n] == 0 && w.layers.flags[n] & 0x7f == 0 && w.layers.deco_object[n] == 0 && w.tiling.sector_of(n) != 0 && owner.is_none_or(|o| w.owner_of(n) == o)
    })
}

#[derive(Debug, PartialEq, Eq)]
struct Run {
    hash: u64,
    /// Ticks: foundation, all 8 pioneers in, both carriers in, all 14 goods in.
    morphed: bool,
    pioneers_in: Option<u32>,
    carriers_in: Option<u32>,
    goods_in: Option<u32>,
    /// The cart moved out, the settlement was made (tick), nodes claimed, settlers out.
    settled: Option<u32>,
    claimed: usize,
    out: usize,
    boards_pile: bool,
}

fn run(ticks: u32) -> Option<Run> {
    let mut w = roman02()?;
    let (cx, cy) = {
        let p = w.player(1)?;
        (p.start_x as i32, p.start_y as i32)
    };
    // The cart: a free cart node on player 1's land 6..12 from the start.
    let (vx, vy) = s4_sim::spiral::walk(w.grid, cx, cy, 12).skip(s4_sim::spiral::first(6)).find(|&(x, y)| {
        let n = w.grid.index(x as u32, y as u32);
        w.owner_of(n) == 1 && vehicles::can_place(&w, x, y, kind::CART)
    })?;
    let c = vehicles::add_vehicle(&mut w, vx, vy, 1, kind::CART, -1, false)?;
    // 8 pioneers and 2 hammers near the castle.
    let mut pioneers = Vec::new();
    for _ in 0..8 {
        let (px, py) = free_node(&w, cx + 4, cy - 4, 10, Some(1))?;
        pioneers.push(s4_sim::create::add_settler(&mut w, px, py, 1, PIONEER)?);
    }
    let (hx, hy) = free_node(&w, cx - 5, cy + 3, 10, Some(1))?;
    s4_sim::create::add_pile(&mut w, hx, hy, HAMMER, 2, s4_sim::pile::role::ANYWHERE)?;
    for _ in 0..5 {
        s4_sim::tick(&mut w, &[]);
    }
    assert!(cart::sector_has_goods(&w, c), "the sector offers boards, stones and hammers");
    let d = cart::dialog(&w, c)?;
    assert!(d.can_morph && !d.foundation && !d.can_settle && d.rows.len() == 6, "dialog {d:?}");
    s4_sim::tick(&mut w, &[PlayerCommand { player: 1, command: Command::CreateFoundationCart { cart: c as u32 } }]);
    let cd = cart::data(&w, c)?;
    let morphed = cd.foundation && cd.pioneers_due == 8 && cd.carriers_due == 2 && cd.goods_needed == 14;
    let (mut pioneers_in, mut carriers_in, mut goods_in, mut settled) = (None, None, None, None);
    let (mut sent, mut claimed, mut out, mut boards_pile) = (false, 0, 0, false);
    let mut target = None;
    for _ in 0..ticks {
        s4_sim::tick(&mut w, &[]);
        w.events.clear();
        let Some(cd) = cart::data(&w, c) else { break };
        if pioneers_in.is_none() && cd.pioneers >= 8 {
            pioneers_in = Some(w.tick);
        }
        if carriers_in.is_none() && cd.carriers >= 2 {
            carriers_in = Some(w.tick);
        }
        if goods_in.is_none() && cd.goods_in >= 14 {
            goods_in = Some(w.tick);
        }
        let full = cd.is_full();
        let flags = w.entities.by_index(c as u32)?.flags;
        if full && !sent {
            // The flags follow at its next update (0x55bca0 → 0x55de00, every 16 ticks).
            if flags & flag::SELECTABLE == 0 {
                assert!(w.tick <= goods_in?.max(pioneers_in?).max(carriers_in?) + 16, "a full foundation cart becomes selectable");
                continue;
            }
            // Out of the territory: the first cart node of nobody's land 15..25 from the start.
            let (tx, ty) = s4_sim::spiral::walk(w.grid, vx, vy, 30).skip(s4_sim::spiral::first(12)).find(|&(x, y)| {
                let n = w.grid.index(x as u32, y as u32);
                w.owner_of(n) == 0 && vehicles::can_place(&w, x, y, kind::CART) && s4_sim::spiral::walk(w.grid, x, y, 3).all(|(a, b)| w.owner_of(w.grid.index(a as u32, b as u32)) == 0)
            })?;
            target = Some((tx, ty));
            s4_sim::tick(&mut w, &[PlayerCommand { player: 1, command: Command::SendTo { x: tx as u16, y: ty as u16, mode: 0, entities: vec![c] } }]);
            sent = true;
            continue;
        }
        if sent && settled.is_none() && vehicles::get(&w, c)?.task == vehicles::logic::task::IDLE {
            let e = w.entities.by_index(c as u32)?;
            let (x, y) = (e.x(), e.y());
            let (tx, ty) = target?;
            if s4_sim::spiral::grid_distance(x - tx, y - ty) <= 2 && cart::room_for_settlement(&w, c) {
                let d = cart::dialog(&w, c)?;
                assert!(d.can_settle && d.foundation && !d.can_morph && d.settlers_in == 10 && d.settlers_needed == 10, "dialog {d:?}");
                assert_eq!(d.rows, vec![[7, 4, 4], [32, 8, 8], [18, 2, 2]]);
                s4_sim::tick(&mut w, &[PlayerCommand { player: 1, command: Command::CreateSettlement { cart: c as u32 } }]);
                settled = Some(w.tick);
                claimed = s4_sim::spiral::walk(w.grid, x, y, 3).filter(|&(a, b)| w.owner_of(w.grid.index(a as u32, b as u32)) == 1).count();
                assert!(w.entities.by_index(c as u32).is_none_or(|e| e.is_dead()), "the cart is gone");
                // The settlers get out at their next update (events 0x19 and the move).
                for _ in 0..2 {
                    s4_sim::tick(&mut w, &[]);
                }
                out = pioneers.iter().filter(|&&p| w.entities.by_index(p as u32).is_some_and(|e| e.flags & flag::ON_BOARD == 0 && e.obj_type == ObjType::Settler)).count();
                boards_pile = s4_sim::spiral::walk(w.grid, x, y, 3).any(|(a, b)| {
                    let n = w.grid.index(a as u32, b as u32);
                    let p = w.layers.deco_object[n];
                    p != 0 && s4_sim::pile::get(&w, p).is_some_and(|d| d.good == 7 && d.amount >= 5)
                });
                break;
            }
        }
    }
    Some(Run { hash: w.state_hash(), morphed, pioneers_in, carriers_in, goods_in, settled, claimed, out, boards_pile })
}

#[test]
fn a_foundation_cart_founds_a_settlement() {
    let Some(r) = run(20_000) else {
        eprintln!("skipping: no game data or no place");
        return;
    };
    eprintln!("{r:?}");
    assert!(r.morphed, "5012 made it a foundation cart");
    assert!(r.pioneers_in.is_some(), "the pioneers did not all board");
    assert!(r.carriers_in.is_some(), "the carriers did not board");
    assert!(r.goods_in.is_some(), "the goods did not all arrive");
    assert!(r.settled.is_some(), "no settlement");
    assert!(r.claimed >= 28, "the land around became player 1's: {}", r.claimed);
    assert_eq!(r.out, 8, "the pioneers are out again");
    assert!(r.boards_pile, "a pile of 5 boards");
    assert_eq!(run(20_000).unwrap(), r, "two runs differ");
}
