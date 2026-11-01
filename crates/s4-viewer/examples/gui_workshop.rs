//! The vehicle hall's dialog D24 with its order sidebar D51, and the finished cart's dialog D41,
//! on roman02, composed on the CPU: a vehicle hall placed next to player 1's castle with 8 iron
//! and a donkey nearby, a cart ordered endless and 3 war machines (command 5040), run until the
//! first cart is `ticks` into its construction (D24 + D51) and until it is finished (D41).
//!
//! cargo run --release -p s4-viewer --example gui_workshop -- OUT_PREFIX [ticks]

use s4_formats::gamedata::Race;
use s4_sim::buildings::workshop;
use s4_sim::entity::{EntityData, flag};
use s4_sim::vehicles::kind;
use s4_sim::{Command, PlayerCommand};
use s4_viewer::building_dialog::Facts;
use s4_viewer::gui::Gui;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out = args.first().cloned().unwrap_or_else(|| "out/gui/workshop".into());
    let ticks: u32 = args.get(1).map_or(Ok(600), |s| s.parse())?;
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
    let castle = w
        .entities
        .iter()
        .find(|(id, e)| e.owner == 1 && matches!(&e.data, EntityData::Building(_)) && s4_sim::buildings::have_inhabitant(&w, *id))
        .map(|(_, e)| (e.x(), e.y()))
        .ok_or_else(|| anyhow::anyhow!("no castle"))?;
    let size = w.grid.size() as i32;
    let &(x, y, _) = s4_sim::build::possible_sites(&w, 1, 23, (0, 0), (size, size))
        .iter()
        .min_by_key(|s| (s.0 as i32 - castle.0).abs() + (s.1 as i32 - castle.1).abs())
        .ok_or_else(|| anyhow::anyhow!("no place for a vehicle hall"))?;
    let hall = s4_sim::create::add_building(&mut w, x as i32, y as i32, 1, 23).map_err(|e| anyhow::anyhow!("{e:?}"))?;
    let k = (s4_sim::spiral::first(6)..s4_sim::spiral::last(12))
        .find(|&k| {
            let n = w.grid.index((castle.0 + s4_sim::spiral::dx(k)) as u32, (castle.1 + s4_sim::spiral::dy(k)) as u32);
            w.layers.flags[n] & 0x7F == 0 && w.layers.map_object[n] == 0 && w.layers.deco_object[n] == 0
        })
        .ok_or_else(|| anyhow::anyhow!("no free node"))?;
    s4_sim::create::add_pile(&mut w, castle.0 + s4_sim::spiral::dx(k), castle.1 + s4_sim::spiral::dy(k), 20, 8, s4_sim::pile::role::ANYWHERE);
    let EntityData::Building(d) = &w.entities.by_index(hall as u32).unwrap().data else { unreachable!() };
    let at = d.working_area;
    let sector = w.tiling.sector_of(w.grid.index(at.x() as u32, at.y() as u32));
    if let Some((dx, dy)) = s4_sim::spiral::walk(w.grid, at.x() + 8, at.y() + 4, 12).find(|&(x, y)| {
        let n = w.grid.index(x as u32, y as u32);
        w.layers.map_object[n] == 0 && w.owner_of(n) == 1 && w.tiling.sector_of(n) == sector
    }) {
        s4_sim::create::add_settler(&mut w, dx, dy, 1, 60);
    }
    let orders = [
        PlayerCommand { player: 1, command: Command::WorkshopOrder { building: hall, item: kind::CART, amount: 100 } },
        PlayerCommand { player: 1, command: Command::WorkshopOrder { building: hall, item: kind::WAR_MACHINE, amount: 3 } },
    ];
    s4_sim::tick(&mut w, &orders);
    // Into the cart's construction (the round robin takes the war machine first: slot 3 < 4, so
    // cancel it when it appears).
    let mut started = None;
    let mut cart = 0;
    for _ in 0..20_000 {
        s4_sim::tick(&mut w, &[]);
        w.events.clear();
        let s = workshop::state(&w, hall).unwrap();
        if s.vehicle != 0 && s.kind != kind::CART as u8 {
            s4_sim::tick(&mut w, &[PlayerCommand { player: 1, command: Command::CancelProduction { building: hall as u32 } }]);
            continue;
        }
        if s.vehicle != 0 && started.is_none() {
            started = Some(w.tick);
            cart = s.vehicle;
        }
        if started.is_some_and(|t| w.tick >= t + ticks) {
            break;
        }
    }
    let pop = s4_viewer::population::from_world(&w);
    let mut gui = Gui::new(Race::ROMAN, Some(&pop), 1, 1024)?;
    let f = Facts::from_world(&w, hall).ok_or_else(|| anyhow::anyhow!("no facts"))?;
    gui.show_building(&f);
    assert!(gui.open_sidebar(&f, 0x1cb), "sidebar not opened");
    gui.show_building(&f);
    let img = gui.compose()?;
    let p = format!("{out}_d24.png");
    s4_viewer::client::write_png(std::path::Path::new(&p), img.width, img.height, &img.rgba)?;
    println!("{p}: tick {}, orders {:?}", w.tick, workshop::state(&w, hall).unwrap().orders);
    // The finished cart's dialog.
    for _ in 0..10_000 {
        if w.entities.by_index(cart as u32).is_some_and(|e| e.flags & flag::BIRTH == 0) {
            break;
        }
        s4_sim::tick(&mut w, &[]);
        w.events.clear();
    }
    let pop = s4_viewer::population::from_world(&w);
    let mut gui = Gui::new(Race::ROMAN, Some(&pop), 1, 1024)?;
    let f = Facts::from_world(&w, cart).ok_or_else(|| anyhow::anyhow!("no cart facts"))?;
    gui.show_building(&f);
    let img = gui.compose()?;
    let p = format!("{out}_d41.png");
    s4_viewer::client::write_png(std::path::Path::new(&p), img.width, img.height, &img.rgba)?;
    println!("{p}: tick {}, cart {cart} {:?}", w.tick, f.cart);
    Ok(())
}
