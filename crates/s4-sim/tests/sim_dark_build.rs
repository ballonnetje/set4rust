//! `CheckForBuildDarkTribe` (0x53ebc0): the Dark Tribe builds on dark land of any owner, levels
//! the ground itself (no diggers) and gets a site; a mushroom farm's site finishes by itself
//! (states 6, 7, 8 of 0x541cc0). dark03, skipped without the data.

use s4_sim::World;
use s4_sim::build::check::CheckResult;

fn dark03() -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("dark03.map");
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    let rules = s4_sim::Rules::load_default().ok()?;
    s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).ok().map(|(w, _)| w)
}

#[test]
fn the_dark_tribe_levels_and_places_a_mushroom_farm() {
    let Some(mut w) = dark03() else { return };
    let p = (1..=8u8).find(|&p| w.player(p).is_some_and(|q| q.race == 3)).expect("a Dark Tribe player");
    const FARM: u8 = 49;
    let size = w.grid.size() as i32;
    // A dark-land spot where the check passes and something must be levelled.
    let spot = (20..size - 20)
        .flat_map(|y| (20..size - 20).map(move |x| (x, y)))
        .filter(|&(x, y)| w.layers.flags[w.grid.index(x as u32, y as u32)] & 0x5F == 4)
        .find(|&(x, y)| matches!(s4_sim::build::check::check_for_build(&w, x, y, p, FARM), CheckResult::Possible { effort, .. } if effort > 1));
    let Some((x, y)) = spot else { panic!("no dark-land spot for a mushroom farm") };
    let (_, plan) = s4_sim::build::check::dig_plan(&w, x, y, p, FARM).expect("a plan");
    let before = w.layers.height.clone();
    let sites = w.sites.roles.len();
    s4_sim::tick(&mut w, &[s4_sim::PlayerCommand::new(p, s4_sim::Command::Build { building: FARM as u32, x: x as u16, y: y as u16 })]);
    assert_eq!(w.sites.roles.len(), sites + 1, "a site at ({x}, {y})");
    let changed = (0..before.len()).filter(|&n| before[n] != w.layers.height[n]).count();
    assert!(changed > 0, "the ground was levelled at once");
    // No digger slot: the Dark Tribe does not dig.
    assert!(w.sites.dig_slots.keys().all(|&(q, _)| q != p));
    // Every planned node within the slope limits reached its plan height or stopped at a limit;
    // the shading follows the heights.
    let reached = (0..31)
        .flat_map(|c| (0..31).map(move |r| (c, r)))
        .filter(|&(c, r)| {
            let n = w.grid.index((x - 15 + c) as u32, (y - 15 + r) as u32);
            plan[(32 * c + r) as usize] == w.layers.height[n]
        })
        .count();
    eprintln!("({x}, {y}): {changed} nodes changed, {reached} of 961 at their plan height");
    for n in 0..w.layers.height.len() {
        let s = w.grid.size() as usize;
        let hh = |m: usize| w.layers.height[m] as i32;
        let up = if n >= s { hh(n - s) } else { hh(n) };
        let down = if n + s < w.layers.height.len() { hh(n + s) } else { hh(n) };
        assert_eq!(w.layers.gradient[n] & 0x0F, (((up - down) >> 1) + 8) as u8 & 0x0F, "shading at node {n}");
    }
    // States 6, 7, 8: no builders; the farm is finished once its block cells are free.
    let farm = w.sites.roles.keys().copied().max().unwrap();
    let mut done = None;
    for t in 0..600 {
        s4_sim::tick(&mut w, &[]);
        if !w.sites.roles.contains_key(&farm) {
            done = Some(t);
            break;
        }
    }
    eprintln!("farm {farm} finished after {done:?} ticks");
    assert!(done.is_some(), "the mushroom farm was never finished");
    let e = w.entities.by_index(farm as u32).unwrap();
    assert_eq!(e.flags & s4_sim::entity::flag::BIRTH, 0);
    assert_ne!(e.flags & s4_sim::entity::flag::READY, 0);
    assert!(matches!(w.buildings.roles.get(&farm).map(|r| &r.kind), Some(s4_sim::buildings::RoleKind::MushroomFarm(_))), "the farm role runs");
    assert_eq!(w.layers.map_object[w.grid.index(e.x() as u32, e.y() as u32)], farm);
}
