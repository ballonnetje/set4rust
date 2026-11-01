//! `CThiefRole` (role 19, settler type 48, release vtable 0x105c4b4) on roman02: a thief of player 1
//! sent to player 2's goods steals one, walks back to where he was sent from and puts it down
//! there. Skipped without the game data.

use s4_sim::entity::{EntityData, ObjType};
use s4_sim::{Command, PlayerCommand, World};

const THIEF: u16 = 48;

fn roman02() -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map");
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    let rules = s4_sim::Rules::load_default().ok()?;
    s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).ok().map(|(w, _)| w)
}

fn loot(w: &World, s: u16) -> u8 {
    match &w.entities.by_index(s as u32).map(|e| &e.data) {
        Some(EntityData::Settler(d)) => d.job.thief.loot,
        _ => 0,
    }
}

#[test]
fn a_thief_steals_from_the_enemy_and_brings_it_home() {
    let Some(mut w) = roman02() else { return };
    // Player 2's goods: a pile on its land with a good to take.
    let piles: Vec<(u16, i32, i32)> = w
        .entities
        .iter()
        .filter(|(_, e)| e.obj_type == ObjType::Pile)
        .filter(|(_, e)| w.grid.checked_index(e.x(), e.y()).is_some_and(|n| w.owner_of(n) == 2))
        .filter(|(_, e)| matches!(&e.data, EntityData::Pile(d) if d.amount > d.leaving))
        .map(|(i, e)| (i, e.x(), e.y()))
        .collect();
    let Some(&(_, px, py)) = piles.first() else { return };
    // The thief on player 1's land nearest those goods.
    let size = w.grid.size() as i32;
    let mut best = None;
    for y in 2..size - 2 {
        for x in 2..size - 2 {
            let n = w.grid.index(x as u32, y as u32);
            if w.owner_of(n) == 1 && w.layers.flags[n] & 0x5f == 0 && w.layers.map_object[n] == 0 {
                let d = (x - px).abs() + (y - py).abs();
                if best.is_none_or(|(b, _, _)| d < b) {
                    best = Some((d, x, y));
                }
            }
        }
    }
    let (_, tx, ty) = best.unwrap();
    let thief = s4_sim::create::add_settler(&mut w, tx, ty, 1, THIEF).expect("thief");
    let role = match &w.entities.by_index(thief as u32).unwrap().data {
        EntityData::Settler(d) => d.job.role,
        _ => 0,
    };
    assert_eq!(role, s4_sim::jobs::role::THIEF);
    s4_sim::tick(&mut w, &[]);
    s4_sim::tick(&mut w, &[PlayerCommand { player: 1, command: Command::SendTo { x: px as u16, y: py as u16, mode: 0, entities: vec![thief] } }]);
    let mut stolen = None;
    for t in 0..6000 {
        s4_sim::tick(&mut w, &[]);
        let l = loot(&w, thief);
        if stolen.is_none() && l != 0 {
            stolen = Some((t, l));
        }
        if let Some((_, good)) = stolen
            && l == 0
        {
            // Put down on his own side.
            let at = w.entities.by_index(thief as u32).map(|e| (e.x(), e.y())).unwrap();
            assert!((at.0 - tx).abs() + (at.1 - ty).abs() <= 6, "put down at {at:?}, sent from ({tx}, {ty})");
            let here = |e: &s4_sim::Entity| (e.x() - at.0).abs() + (e.y() - at.1).abs() <= 4;
            let dropped: u32 = w
                .entities
                .iter()
                .filter(|(_, e)| e.obj_type == ObjType::Pile && here(e))
                .map(|(_, e)| match &e.data {
                    EntityData::Pile(d) if d.good == good => d.amount as u32,
                    _ => 0,
                })
                .sum();
            assert!(dropped >= 1, "the stolen good {good} lies where he came back to");
            s4_sim::eco::check_offer_lists(&w).unwrap();
            return;
        }
    }
    panic!("the thief did not steal and bring home a good: {stolen:?}");
}

/// `CSettler::ChangeType` (0x580ce0) sets a thief free first (0x580de4, role vtbl+0x40 with −1):
/// a thief with loot that becomes a carrier on his own land puts the good down.
#[test]
fn a_thief_changed_into_a_carrier_puts_his_loot_down() {
    let Some(mut w) = roman02() else { return };
    let piles: Vec<(i32, i32)> = w
        .entities
        .iter()
        .filter(|(_, e)| e.obj_type == ObjType::Pile)
        .filter(|(_, e)| w.grid.checked_index(e.x(), e.y()).is_some_and(|n| w.owner_of(n) == 2))
        .filter(|(_, e)| matches!(&e.data, EntityData::Pile(d) if d.amount > d.leaving))
        .map(|(_, e)| (e.x(), e.y()))
        .collect();
    let Some(&(px, py)) = piles.first() else { return };
    let size = w.grid.size() as i32;
    let mut best = None;
    for y in 2..size - 2 {
        for x in 2..size - 2 {
            let n = w.grid.index(x as u32, y as u32);
            if w.owner_of(n) == 1 && w.layers.flags[n] & 0x5f == 0 && w.layers.map_object[n] == 0 {
                let d = (x - px).abs() + (y - py).abs();
                if best.is_none_or(|(b, _, _)| d < b) {
                    best = Some((d, x, y));
                }
            }
        }
    }
    let (_, tx, ty) = best.unwrap();
    let thief = s4_sim::create::add_settler(&mut w, tx, ty, 1, THIEF).expect("thief");
    s4_sim::tick(&mut w, &[]);
    s4_sim::tick(&mut w, &[PlayerCommand { player: 1, command: Command::SendTo { x: px as u16, y: py as u16, mode: 0, entities: vec![thief] } }]);
    let goods = |w: &World, good: u8| -> u32 {
        w.entities
            .iter()
            .map(|(_, e)| match &e.data {
                EntityData::Pile(d) if d.good == good => d.amount as u32,
                _ => 0,
            })
            .sum()
    };
    for _ in 0..6000 {
        s4_sim::tick(&mut w, &[]);
        let good = loot(&w, thief);
        let home = w.entities.by_index(thief as u32).and_then(|e| w.grid.checked_index(e.x(), e.y())).is_some_and(|n| w.owner_of(n) == 1);
        if good != 0 && home {
            let before = goods(&w, good);
            s4_sim::jobs::core::change_type(&mut w, thief, 1, false, false);
            assert_eq!(w.entities.by_index(thief as u32).map(|e| e.kind), Some(1), "a carrier now");
            assert_eq!(goods(&w, good), before + 1, "the loot lies on a pile");
            s4_sim::eco::check_offer_lists(&w).unwrap();
            return;
        }
    }
    panic!("the thief never came home with loot");
}
