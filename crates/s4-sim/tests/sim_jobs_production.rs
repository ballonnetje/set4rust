//! `CProductionBuildingRole` on roman02, driven by `tick()` only: a toolsmith (type 21), set up
//! like a map building, orders its smith (a carrier fetches the hammer and changes type),
//! carriers bring iron and coal to its delivery piles, and the player's tool order (command
//! 5033) decides what it makes (0x54ce20, sim's `eco::choose_product`). Without an order it
//! makes nothing. roman02 has coal but no iron, so one free pile of 8 iron is put down near the
//! castle, as a map's goods pile would be. Skipped when the game data is not present.

use s4_sim::build::possible_sites;
use s4_sim::entity::EntityData;
use s4_sim::{Command, PlayerCommand, World};

const TOOLSMITH: u8 = 21;
const IRON: u8 = 20;

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

#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    hash: u64,
    /// Goods that arrived on the product piles, per product pile (in the role's order).
    made: Vec<(u8, u32)>,
    /// Goods taken off the delivery piles by the smith or that arrived there.
    delivered: u32,
}

fn run(order: Option<(usize, u32)>, ticks: u32) -> Option<Outcome> {
    let mut w = roman02()?;
    let castle = w.entities.iter().find(|(id, e)| e.owner == 1 && matches!(&e.data, EntityData::Building(_)) && s4_sim::buildings::have_inhabitant(&w, *id)).map(|(_, e)| (e.x(), e.y()))?;
    let size = w.grid.size() as i32;
    let &(x, y, _) = possible_sites(&w, 1, TOOLSMITH, (0, 0), (size, size))
        .iter()
        .min_by_key(|s| (s.0 as i32 - castle.0).abs() + (s.1 as i32 - castle.1).abs())?;
    let smith = s4_sim::create::add_building(&mut w, x as i32, y as i32, 1, TOOLSMITH).expect("toolsmith");
    // A free iron pile on the first empty node of the castle's spiral beyond radius 6.
    let k = (s4_sim::spiral::first(6)..s4_sim::spiral::last(12)).find(|&k| {
        let n = w.grid.index((castle.0 + s4_sim::spiral::dx(k)) as u32, (castle.1 + s4_sim::spiral::dy(k)) as u32);
        w.layers.flags[n] & 0x7F == 0 && w.layers.map_object[n] == 0 && w.layers.deco_object[n] == 0
    })
    .expect("no free node for the iron");
    let (px, py) = (castle.0 + s4_sim::spiral::dx(k), castle.1 + s4_sim::spiral::dy(k));
    s4_sim::create::add_pile(&mut w, px, py, IRON, 8, s4_sim::pile::role::ANYWHERE).expect("iron pile");
    let (products, delivery, goods) = match &w.buildings.roles[&smith].kind {
        s4_sim::buildings::RoleKind::Production(s) => (s.products.clone(), s.delivery.clone(), s.goods.clone()),
        _ => panic!("the toolsmith has no production role"),
    };
    assert!(!products.is_empty() && !delivery.is_empty());
    let cmds: Vec<PlayerCommand> = order
        .map(|(k, n)| PlayerCommand { player: 1, command: Command::ProductionOrder { building: smith, good: goods[k] as u16, amount: n } })
        .into_iter()
        .collect();
    let amount = |w: &World, p: u16| s4_sim::pile::get(w, p).map_or(0, |d| d.amount as u32);
    let mut last: Vec<u32> = products.iter().chain(&delivery).map(|&p| amount(&w, p)).collect();
    let mut made = vec![0u32; products.len()];
    let mut delivered = 0;
    for t in 0..ticks {
        s4_sim::tick(&mut w, if t == 0 { &cmds } else { &[] });
        for (k, &p) in products.iter().chain(&delivery).enumerate() {
            let a = amount(&w, p);
            if k < products.len() {
                made[k] += a.saturating_sub(last[k]);
            } else {
                delivered += a.saturating_sub(last[k]);
            }
            last[k] = a;
        }
    }
    Some(Outcome { hash: w.state_hash(), made: goods.iter().copied().zip(made).collect(), delivered })
}

#[test]
fn toolsmith_makes_what_is_ordered() {
    let Some(none) = run(None, 8000) else { return };
    eprintln!("no order: {none:?}");
    assert!(none.made.iter().all(|&(_, n)| n == 0), "a toolsmith without orders made {:?}", none.made);
    let r = run(Some((1, 2)), 8000).unwrap();
    eprintln!("order 2 of product 1: {r:?}");
    assert!(r.delivered >= 1, "no iron or coal was brought");
    let total: u32 = r.made.iter().map(|&(_, n)| n).sum();
    assert_eq!(r.made[1].1, total, "made other tools than ordered: {:?}", r.made);
    assert_eq!(total, 2, "the order was for 2: {:?}", r.made);
    assert_eq!(run(Some((1, 2)), 8000).unwrap(), r, "two runs differ");
}
