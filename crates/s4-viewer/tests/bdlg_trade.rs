//! D17 and the D53 fill of a marketplace on roman02 (placed like a map building next to player
//! 1's start, asked by command 5006 to send 10 boards and endless gold): the role fill 0x5533f0
//! writes each trade pile with goods as (good, amount) at +0x14 + 2i / +0x15 + 2i, the sidebar
//! fill 0x553300 the amount still to send of each good with the domestic flag at +8 + good, the
//! D54 fill 0x5531b0 the owner's trade-with flags and the building's foreign states.
//! Skipped without the game data.

use s4_sim::buildings::trading;
use s4_sim::entity::EntityData;
use s4_sim::{Command, PlayerCommand};
use s4_viewer::building_dialog::{Facts, Infos, fill, sidebar_fill};

#[test]
fn market_dialog_and_sidebar_fill() {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map");
    let Ok(map) = s4_formats::map::MapFile::open(&p) else { return };
    let Ok(rules) = s4_sim::Rules::load_default() else { return };
    let (mut w, _) = s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).unwrap();
    let towers: Vec<(i32, i32)> =
        w.entities.iter().filter_map(|(id, e)| matches!(e.data, EntityData::Building(_)).then_some(id).filter(|&id| s4_sim::buildings::have_inhabitant(&w, id)).map(|_| (e.x(), e.y()))).collect();
    for (x, y) in towers {
        s4_sim::territory::set_owner(&mut w, x, y);
    }
    let (cx, cy) = w.player(1).map(|p| (p.start_x as i32, p.start_y as i32)).unwrap();
    let size = w.grid.size() as i32;
    let kind = trading::MARKETPLACE as u8;
    let &(mx, my, _) = s4_sim::build::possible_sites(&w, 1, kind, (0, 0), (size, size)).iter().min_by_key(|s| (s.0 as i32 - cx).abs() + (s.1 as i32 - cy).abs()).unwrap();
    let market = s4_sim::create::add_building(&mut w, mx as i32, my as i32, 1, kind).unwrap();
    let orders: Vec<PlayerCommand> =
        [(7, 10), (14, 100)].iter().map(|&(good, amount)| PlayerCommand { player: 1, command: Command::TransportGood { building: market, good, amount } }).collect();
    for t in 0..1500 {
        s4_sim::tick(&mut w, if t == 0 { &orders } else { &[] });
    }
    let f = Facts::from_world(&w, market).unwrap();
    let mut infos = Infos::default();
    assert_eq!(fill(&mut infos, &f), Some(5));
    let s = trading::state(&w, market).unwrap();
    let mut boards = 0;
    for (i, &p) in s.piles.iter().enumerate() {
        let d = s4_sim::pile::get(&w, p).unwrap();
        let slot = (infos.obj[5][0x14 + 2 * i], infos.obj[5][0x15 + 2 * i]);
        assert_eq!(slot, if d.amount == 0 { (0, 0) } else { (d.good, d.amount) }, "pile {i}");
        if d.good == 7 {
            boards += d.amount as i32;
        }
    }
    assert_eq!(boards, 10, "the ten boards are on the market's piles");
    assert_eq!(sidebar_fill(&mut infos, &f, 0x1c7), Some(0xe));
    let o = &infos.obj[0xe];
    assert_eq!((o[8 + 7], o[8 + 14]), (s.send[7] as u8, 100));
    assert!((0..43).filter(|&g| g != 7 && g != 14).all(|g| o[8 + g] == 0));
    // D54 (0x5531b0): boards to import (5008 cycles 0 -> 4), trade with player 2 (5050).
    let cmds = [
        PlayerCommand { player: 1, command: Command::TradeGood { building: market as u32, value: 7 } },
        PlayerCommand { player: 1, command: Command::SetTradeStatus { building: market, player: 2, value: 1 } },
    ];
    s4_sim::tick(&mut w, &cmds);
    let f = Facts::from_world(&w, market).unwrap();
    assert!(f.trader && f.player_count >= 2);
    assert_eq!(sidebar_fill(&mut infos, &f, 0x1c8), Some(0xd));
    let o = &infos.obj[0xd];
    assert_eq!((o[8 + 1], o[8 + 2]), (0, 1), "trades with player 2");
    assert_eq!(o[0x11 + 7], 4, "boards: import");
    assert!((0..43).filter(|&g| g != 7).all(|g| o[0x11 + g] == 0));
}
