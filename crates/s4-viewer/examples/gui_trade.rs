//! The trading building's dialog D17 and its "goods to send" sidebar D53 on roman02, composed on
//! the CPU: a marketplace placed (like a map building) next to player 1's start, asked to send 10
//! boards, 4 stone and endless (100) gold (command 5006), run for `ticks`; then the dialog is
//! shown, the sidebar opened (UI event 0x1c7) and the given group tab clicked.
//!
//! With event 456 (0x1c8) the foreign trade sidebar D54 instead, after boards set to import,
//! stone to export (5008) and trade with player 2 (5050).
//!
//! cargo run --release -p s4-viewer --example gui_trade -- OUT.png [ticks] [group tab link] [event]

use s4_formats::gamedata::Race;
use s4_sim::buildings::trading;
use s4_sim::entity::EntityData;
use s4_sim::{Command, PlayerCommand};
use s4_viewer::building_dialog::Facts;
use s4_viewer::gui::Gui;
use s4_viewer::guistate::UiEvent;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out = args.first().cloned().unwrap_or_else(|| "out/gui/trade.png".into());
    let ticks: u32 = args.get(1).map_or(Ok(1500), |s| s.parse())?;
    let tab: u16 = args.get(2).map_or(Ok(0x338), |s| s.parse())?;
    let event: u16 = args.get(3).map_or(Ok(0x1c7), |s| s.parse())?;
    let map = s4_formats::map::MapFile::open(s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map"))?;
    let (mut w, _) = s4_sim::new_game(&map, s4_sim::Rules::load_default()?, &s4_sim::NewGame::default())?;
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
    let (cx, cy) = w.player(1).map(|p| (p.start_x as i32, p.start_y as i32)).unwrap_or_default();
    let size = w.grid.size() as i32;
    let kind = trading::MARKETPLACE as u8;
    let &(mx, my, _) = s4_sim::build::possible_sites(&w, 1, kind, (0, 0), (size, size))
        .iter()
        .min_by_key(|s| (s.0 as i32 - cx).abs() + (s.1 as i32 - cy).abs())
        .ok_or_else(|| anyhow::anyhow!("no place for a marketplace"))?;
    let market = s4_sim::create::add_building(&mut w, mx as i32, my as i32, 1, kind).map_err(|e| anyhow::anyhow!("{e:?}"))?;
    let orders: Vec<PlayerCommand> = [(7, 10), (32, 4), (14, 100)]
        .iter()
        .map(|&(good, amount)| PlayerCommand { player: 1, command: Command::TransportGood { building: market, good, amount } })
        .collect();
    for t in 0..ticks {
        s4_sim::tick(&mut w, if t == 0 { &orders } else { &[] });
    }
    if event == 0x1c8 {
        let foreign = [
            PlayerCommand { player: 1, command: Command::TradeGood { building: market as u32, value: 7 } },
            PlayerCommand { player: 1, command: Command::TradeGood { building: market as u32, value: 32 } },
            PlayerCommand { player: 1, command: Command::TradeGood { building: market as u32, value: 32 } },
            PlayerCommand { player: 1, command: Command::SetTradeStatus { building: market, player: 2, value: 1 } },
        ];
        s4_sim::tick(&mut w, &foreign);
    }
    let f = Facts::from_world(&w, market).ok_or_else(|| anyhow::anyhow!("no facts"))?;
    println!("trade piles {:?}", f.trade_piles);
    let pop = s4_viewer::population::from_world(&w);
    let mut gui = Gui::new(Race::ROMAN, Some(&pop), 1, 1024)?;
    gui.show_building(&f);
    assert!(gui.open_sidebar(&f, event), "sidebar not opened");
    let mut ev = Vec::new();
    gui.state.select(if event == 0x1c8 { 54 } else { 53 }, tab, &mut ev);
    for e in ev {
        if let UiEvent::Command { dialog, link, effects } = e {
            gui.sidebar_click(dialog, link, effects);
        }
    }
    // A refresh (the provider's 0x25e) with the same facts.
    gui.show_building(&f);
    let img = gui.compose()?;
    s4_viewer::client::write_png(std::path::Path::new(&out), img.width, img.height, &img.rgba)?;
    println!("{out}: {}x{}", img.width, img.height);
    Ok(())
}
