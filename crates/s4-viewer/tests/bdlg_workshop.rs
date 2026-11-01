//! The workshop dialog D24, its sidebar D51 and the cart dialog D41 on the sim: a vehicle hall on
//! roman02 (as `s4-sim/tests/sim_workshop.rs`) gets an endless cart order and two war machines
//! through D51's buttons (UI event 0x1d5 → net 5040); the sidebar fill 0x5597b0 shows them; the
//! cart under construction shows its progress (0x559930) in D24; the finished cart's dialog
//! (0x55bdb0 → D41) shows a trade cart. Skipped without the game data.

use s4_sim::build::possible_sites;
use s4_sim::buildings::workshop;
use s4_sim::entity::{EntityData, flag};
use s4_sim::vehicles::kind;
use s4_sim::{Command, PlayerCommand, World};
use s4_viewer::building_dialog::{CART_CODE, Facts, Infos, fill, sidebar_fill, update, workshop_progress};
use s4_viewer::guistate::GuiState;
use s4_viewer::workshop_sidebar::{Call, WorkshopSide};

const VEHICLE_HALL: u8 = 23;
const IRON: u8 = 20;
const DONKEY: u16 = 60;

fn setup() -> Option<(World, u16)> {
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
    let castle = w.entities.iter().find(|(id, e)| e.owner == 1 && matches!(&e.data, EntityData::Building(_)) && s4_sim::buildings::have_inhabitant(&w, *id)).map(|(_, e)| (e.x(), e.y()))?;
    let size = w.grid.size() as i32;
    let &(x, y, _) = possible_sites(&w, 1, VEHICLE_HALL, (0, 0), (size, size)).iter().min_by_key(|s| (s.0 as i32 - castle.0).abs() + (s.1 as i32 - castle.1).abs())?;
    let hall = s4_sim::create::add_building(&mut w, x as i32, y as i32, 1, VEHICLE_HALL).ok()?;
    let k = (s4_sim::spiral::first(6)..s4_sim::spiral::last(12)).find(|&k| {
        let n = w.grid.index((castle.0 + s4_sim::spiral::dx(k)) as u32, (castle.1 + s4_sim::spiral::dy(k)) as u32);
        w.layers.flags[n] & 0x7F == 0 && w.layers.map_object[n] == 0 && w.layers.deco_object[n] == 0
    })?;
    s4_sim::create::add_pile(&mut w, castle.0 + s4_sim::spiral::dx(k), castle.1 + s4_sim::spiral::dy(k), IRON, 8, s4_sim::pile::role::ANYWHERE)?;
    // A donkey for the finished cart.
    let b = w.entities.by_index(hall as u32)?;
    let EntityData::Building(d) = &b.data else { return None };
    let at = d.working_area;
    let sector = w.tiling.sector_of(w.grid.index(at.x() as u32, at.y() as u32));
    let (dx, dy) = s4_sim::spiral::walk(w.grid, at.x() + 8, at.y() + 4, 12).find(|&(x, y)| {
        let n = w.grid.index(x as u32, y as u32);
        w.layers.map_object[n] == 0 && w.owner_of(n) == 1 && w.tiling.sector_of(n) == sector
    })?;
    s4_sim::create::add_settler(&mut w, dx, dy, 1, DONKEY)?;
    Some((w, hall))
}

/// The UI events D51's buttons send, as net commands the way s4-game's bdlg.rs does (0x4ea4e0).
fn press(side: &mut WorkshopSide, link: u16, hall: u16) -> Vec<PlayerCommand> {
    let mut out = Vec::new();
    side.command(link, true, &mut out);
    out.into_iter()
        .filter_map(|c| match c {
            Call::Event(0x1d5, w, l) => Some(PlayerCommand { player: 1, command: Command::WorkshopOrder { building: hall, item: w as u16, amount: l as u32 } }),
            _ => None,
        })
        .collect()
}

#[test]
fn workshop_and_cart_dialogs() {
    if !s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map").exists() {
        return;
    }
    let (mut w, hall) = setup().expect("the hall set up");
    let Ok(set) = s4_formats::gui::GuiSet::open(s4_formats::game_dir().join("Menu/GUISetRomans.dat")) else { return };
    let mut st = GuiState::new(set);
    let mut infos = Infos::default();
    let f = Facts::from_world(&w, hall).unwrap();
    assert_eq!(sidebar_fill(&mut infos, &f, 0x1cb), Some(0x11));
    let info = infos.obj[0x11];
    // The hall's records: war machine (4) and cart (5) with the Roman costs.
    let costs = |t: u8| {
        let i = w.rules.vehicles.get(0, t);
        (i.boards as u8, i.ironbars as u8)
    };
    assert_eq!((info[8], info[9], (info[10], info[11])), (4, 0, costs(4)));
    assert_eq!((info[12], info[13], (info[14], info[15])), (5, 0, costs(5)));
    let mut side = WorkshopSide::default();
    let mut calls = Vec::new();
    side.init(&info, 0, &mut calls);
    assert!(!side.shipyard && side.selected == 4, "the hall's layout, war machine selected");
    // War machine +5: the sidebar shows 5.
    let cmds = press(&mut side, 0xf6, hall);
    s4_sim::tick(&mut w, &cmds);
    assert_eq!(workshop::state(&w, hall).unwrap().orders, [0, 0, 0, 5, 0]);
    let f = Facts::from_world(&w, hall).unwrap();
    sidebar_fill(&mut infos, &f, 0x1cb);
    let mut calls = Vec::new();
    side.update(&infos.obj[0x11], &mut calls);
    assert!(calls.contains(&Call::Text(0xf3, "5".into())) && calls.contains(&Call::Visible(0xf3, true)), "{calls:?}");
    // −5, then the cart radio and "endless": only the cart is ordered.
    let mut cmds = press(&mut side, 0xf7, hall);
    side.command(0xfe, true, &mut calls);
    cmds.extend(press(&mut side, 0xf4, hall));
    s4_sim::tick(&mut w, &cmds);
    assert_eq!(workshop::state(&w, hall).unwrap().orders, [0, 0, 0, 0, 100]);
    let f = Facts::from_world(&w, hall).unwrap();
    sidebar_fill(&mut infos, &f, 0x1cb);
    let mut calls = Vec::new();
    side.update(&infos.obj[0x11], &mut calls);
    assert!(calls.contains(&Call::Text(0xfb, "E".into())) && calls.contains(&Call::Visible(0xf3, false)), "{calls:?}");
    // The vehicle under construction in D24 (0x559930): progress and type.
    let mut seen = Vec::new();
    let mut vehicle = 0;
    for _ in 0..6000 {
        s4_sim::tick(&mut w, &[]);
        w.events.clear();
        let s = workshop::state(&w, hall).unwrap();
        if s.vehicle != 0 {
            vehicle = s.vehicle;
            let f = Facts::from_world(&w, hall).unwrap();
            assert_eq!(fill(&mut infos, &f), Some(9));
            let v = s4_sim::vehicles::get(&w, s.vehicle).unwrap();
            let p = u32::from_le_bytes(infos.obj[9][0x20..0x24].try_into().unwrap()) as i32;
            assert_eq!(p, workshop_progress(v.progress[0], v.progress[1]));
            assert_eq!(u32::from_le_bytes(infos.obj[9][0x24..0x28].try_into().unwrap()), s.kind as u32);
            if seen.last() != Some(&p) {
                seen.push(p);
            }
        }
        if w.entities.by_index(vehicle as u32).is_some_and(|e| e.kind == kind::CART && e.flags & flag::BIRTH == 0) {
            break;
        }
    }
    assert!(seen.len() >= 3 && seen.windows(2).all(|p| p[0] < p[1]), "progress {seen:?}");
    // The finished cart's dialog (D41).
    let carts: Vec<u16> = w.entities.iter().filter(|(_, e)| e.kind == kind::CART && matches!(e.obj_type, s4_sim::entity::ObjType::LandVehicle) && e.flags & flag::BIRTH == 0).map(|(id, _)| id).collect();
    assert_eq!(carts.first(), Some(&vehicle), "the cart was finished");
    {
        let cart = vehicle;
        let f = Facts::from_world(&w, cart).unwrap();
        assert!(f.cart.as_ref().is_some_and(|c| !c.foundation));
        assert_eq!(fill(&mut infos, &f), Some(CART_CODE));
        update(CART_CODE, &infos, &mut st, &|id| format!("#{id}"));
        assert_eq!(st.texts.get(&(41, 0x6b2)).map(String::as_str), Some("#2230"), "a trade cart's title (0x8b6)");
    }
}
