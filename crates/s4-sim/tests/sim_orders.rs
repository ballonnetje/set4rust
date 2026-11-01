//! The eco-sector's tool and weapon orders (release `CEcoSector` ctor 0x4d2590, change functions
//! 0x4d6960 / 0x4d6a90, product choice 0x54ce20, commands 5033–5035).

use s4_sim::{Command, Grid, Player, PlayerCommand, Rules, World, eco, tick};

fn world() -> Option<World> {
    if !s4_formats::game_dir().join("GameData").exists() {
        return None;
    }
    let mut w = World::new(Grid::new(64).unwrap(), Rules::load_default().unwrap());
    w.players[1] = Some(Player { id: 1, race: 0, start_x: 0, start_y: 0 });
    w.layers.ground.fill(16);
    w.init_tiling(&vec![1u8; w.grid.len()]);
    Some(w)
}

#[test]
fn orders_defaults_changes_and_choice() {
    let Some(mut w) = world() else { return };
    let e = w.economy.get(1).unwrap().clone();
    let products = |t: u8| -> Vec<u8> { w.rules.building(0, t).piles.iter().filter(|p| p.kind == 0).map(|p| p.good).collect() };
    assert_eq!((e.weapons.clone(), e.tools.clone()), (products(22), products(21)));
    assert!(!e.tools.is_empty() && e.weapons.len() >= 2);
    assert_eq!((e.weapon_auto, &e.weapon_percent[..2]), (100, &[50, 50][..]));
    assert!(e.tool_orders.iter().chain(&e.weapon_orders).all(|&c| c == 0));

    // The count rule: 0 clears, 100 is unlimited, otherwise clamped to 0..=100.
    let t0 = e.tools[0];
    let tool = |w: &World| w.economy.get(1).unwrap().tool_orders[0];
    eco::change_tool_order(&mut w, 1, t0, 3);
    assert_eq!(tool(&w), 3);
    eco::change_tool_order(&mut w, 1, t0, -5);
    assert_eq!(tool(&w), 0);
    eco::change_tool_order(&mut w, 1, t0, 99);
    eco::change_tool_order(&mut w, 1, t0, 2);
    assert_eq!(tool(&w), 100);
    eco::change_tool_order(&mut w, 1, t0, 0);
    assert_eq!(tool(&w), 0);

    // Toolsmith: round-robin over the ordered tools, counts go down; nothing ordered, no work.
    let tools = e.tools.clone();
    let (mut last, mut product) = (0u8, 0u8);
    assert!(!eco::choose_product(&mut w, 1, 21, &tools, &mut last, &mut product));
    eco::change_tool_order(&mut w, 1, t0, 2);
    assert!(eco::choose_product(&mut w, 1, 21, &tools, &mut last, &mut product));
    assert_eq!((product, last, tool(&w)), (t0, 0, 1));

    // Weaponsmith without orders on auto-production: by percentage, 50/50 alternating.
    let weapons = e.weapons.clone();
    let mut made = Vec::new();
    for _ in 0..4 {
        assert!(eco::choose_product(&mut w, 1, 22, &weapons, &mut last, &mut product));
        made.push(product);
    }
    assert_eq!(made, vec![weapons[0], weapons[1], weapons[0], weapons[1]]);
    assert_eq!(&w.economy.get(1).unwrap().weapons_made[..2], &[2, 2]);
}

#[test]
fn order_commands() {
    let Some(mut w) = world() else { return };
    let smith = s4_sim::create::add_building(&mut w, 30, 30, 1, 21).unwrap();
    let weapons = s4_sim::create::add_building(&mut w, 40, 40, 1, 22).unwrap();
    let t0 = w.economy.get(1).unwrap().tools[0];
    let w0 = w.economy.get(1).unwrap().weapons[0];
    let cmd = |c| PlayerCommand { player: 1, command: c };
    tick(&mut w, &[
        cmd(Command::ProductionOrder { building: smith, good: t0 as u16, amount: 4 }),
        cmd(Command::ProductionOrder { building: weapons, good: w0 as u16, amount: 2 }),
        cmd(Command::SetWeaponPercentage { building: weapons as u32, percent: 0x00_1e_46 }),
        cmd(Command::SetWeaponAutoProduction { building: weapons as u32, value: 1 }),
    ]);
    let e = w.economy.get(1).unwrap();
    assert_eq!((e.tool_orders[0], e.weapon_orders[0]), (4, 2));
    assert_eq!(&e.weapon_percent[..2], &[0x46, 0x1e]);
    assert_eq!(e.weapon_auto, 0);
    // Another player's order is ignored.
    tick(&mut w, &[PlayerCommand { player: 2, command: Command::ProductionOrder { building: smith, good: t0 as u16, amount: 9 } }]);
    assert_eq!(w.economy.get(1).unwrap().tool_orders[0], 4);
}
