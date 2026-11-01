//! Mushroom farms (`CDarkMushroomFarmRole`, house kind 20) with their farmer
//! (`CMushroomFarmerRole`, role 15) and the dark temple (`CDarkTempleRole`, kind 18), driven by
//! `tick()` only. Skipped when the game data is not present.

use s4_sim::World;
use s4_sim::entity::ObjType;

fn map(name: &str) -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join(name);
    if !p.exists() {
        return None;
    }
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    let rules = s4_sim::Rules::load_default().ok()?;
    s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).ok().map(|(w, _)| w)
}

/// The single-precision form of the release (0x65b130 / 0x4e25c0) against the integer one.
#[test]
fn screen_and_flight_are_exact() {
    let (c1, c2) = (0.051_562_5_f32, 24.0_f32);
    for h in 0..=255u8 {
        let v = (h as f32 * c1) * c2;
        assert_eq!(v.round_ties_even() as i32, s4_sim::buildings::mushfarm::height_px(h), "height {h}");
    }
    for a in -700i64..=700 {
        for b in (-700i64..=700).step_by(7) {
            let d = (a * a + b * b) as f32;
            let f = (((d as f64).sqrt() as f32) * 0.2_f32) as i32;
            assert_eq!(f as i64, ((a * a + b * b) / 25).isqrt(), "{a} {b}");
        }
    }
}

const FARM: u16 = 49;
const FARMER: u16 = 53;

#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    hash: u64,
    mana: i32,
    full: usize,
    arrived: usize,
    planted: usize,
    pulls: usize,
}

/// dark03, player 2: eight mushroom farms and a dark temple, no farmer at the start.
fn run(ticks: u32) -> Option<Outcome> {
    let mut w = map("dark03.map")?;
    let farms: Vec<u16> = w.entities.iter().filter(|(_, e)| e.obj_type == ObjType::Building && e.kind == FARM).map(|(i, _)| i).collect();
    assert_eq!(farms.len(), 8);
    assert!(!w.entities.iter().any(|(_, e)| e.obj_type == ObjType::Settler && e.kind == FARMER));
    let temple = s4_sim::dark::temple_of(&w, 2).expect("player 2 has a temple");
    let grow = w.rules.dark.farm_patch_frames[3][2] as i32;
    assert_eq!(w.rules.dark.farm_patch_frames[3], [20, 32, 8], "OPEN, FLOW, GROW of the Dark Tribe's farm");
    let mut last: Vec<i32> = vec![0; farms.len()];
    let (mut full, mut planted, mut pulls) = (0, 0, 0);
    let mut farmers = std::collections::BTreeSet::new();
    let mut mushrooms = std::collections::BTreeSet::new();
    for _ in 0..ticks {
        w.events.clear();
        s4_sim::tick(&mut w, &[]);
        for e in &w.events {
            match e {
                s4_sim::event::SimEvent::Unported { what, .. } => assert!(!what.contains("Mushroom") && !what.contains("SettlerEnter"), "{what}"),
                s4_sim::event::SimEvent::Effect { effect: 0x14, x, y, .. } => {
                    pulls += 1;
                    let n = w.grid.index(*x as u32, *y as u32);
                    let id = w.layers.deco_object[n];
                    assert!(w.entities.by_index(id as u32).is_some_and(|m| (212..=214).contains(&m.kind)), "a pull at a mushroom");
                }
                _ => {}
            }
        }
        for (k, &f) in farms.iter().enumerate() {
            let s = s4_sim::buildings::mushfarm::get(&w, f).unwrap();
            assert!(s.spheres <= grow);
            if last[k] == grow && s.spheres == 0 {
                full += 1;
            }
            last[k] = s.spheres;
        }
        for (i, e) in w.entities.iter() {
            if e.obj_type == ObjType::Settler && e.kind == FARMER && !e.is_dead() && farmers.insert((i, e.uid)) {
                // Made at the temple's exit (door + (1, 1)) or next to it.
                let exit = s4_sim::buildings::dark_temple::get(&w, temple).unwrap().exit;
                let (ex, ey) = ((exit & 0xffff) as i32, (exit >> 16) as i32);
                assert!(s4_sim::spiral::grid_distance(e.x() - ex, e.y() - ey) <= 15);
            }
            if e.obj_type == ObjType::Deco
                && (212..=214).contains(&e.kind)
                && let s4_sim::entity::EntityData::Deco(d) = &e.data
                && !d.static_instance
                && d.stage == 0
                && mushrooms.insert((i, e.uid))
            {
                planted += 1;
                let n = w.grid.index(e.x() as u32, e.y() as u32);
                assert_ne!(w.layers.flags[n] & 4, 0, "mushrooms grow on dark land");
            }
        }
    }
    // A farmer leaves the world only by entering the temple.
    let arrived = farmers.iter().filter(|&&(i, uid)| w.entities.by_index(i as u32).is_none_or(|e| e.uid != uid || e.is_dead())).count();
    Some(Outcome { hash: w.state_hash(), mana: s4_sim::magic::mana(&w, 2), full, arrived, planted, pulls })
}

#[test]
fn farms_pull_and_farmers_bring_mana() {
    let Some(a) = run(12_000) else { return };
    eprintln!("{a:?}");
    assert!(a.pulls > 50, "farms pull from ripe mushrooms");
    assert!(a.planted > 0, "farmers plant");
    assert!(a.full >= 8, "farms fill up");
    // Every farmer who reaches the temple brings DTManaPerSphere (6).
    assert_eq!(a.mana, 6 * a.arrived as i32);
    assert!(a.arrived + 8 >= a.full, "the full farms' farmers arrive");
    assert_eq!(Some(a), run(12_000), "deterministic");
}

/// A dark gardener entering the temple waits inside; the next update lets him out at the exit.
#[test]
fn temple_lets_gardeners_out() {
    let Some(mut w) = map("dark03.map") else { return };
    let temple = s4_sim::dark::temple_of(&w, 2).unwrap();
    let g = w.entities.iter().find(|(_, e)| e.obj_type == ObjType::Settler && e.kind == 52 && e.owner == 2).map(|(i, _)| i).expect("a dark gardener");
    assert!(s4_sim::buildings::dark_temple::settler_enter(&mut w, temple, g));
    let e = w.entities.by_index(g as u32).unwrap();
    assert_eq!(e.flags & 0x100, 0, "hidden inside");
    assert_eq!(s4_sim::buildings::dark_temple::get(&w, temple).unwrap().inside, vec![g]);
    for _ in 0..32 {
        s4_sim::tick(&mut w, &[]);
    }
    let exit = s4_sim::buildings::dark_temple::get(&w, temple).unwrap().exit;
    let e = w.entities.by_index(g as u32).unwrap();
    assert!(s4_sim::buildings::dark_temple::get(&w, temple).unwrap().inside.is_empty());
    assert_ne!(e.flags & 0x100, 0, "visible again");
    let d = s4_sim::spiral::grid_distance(e.x() - (exit & 0xffff) as i32, e.y() - (exit >> 16) as i32);
    assert!(d <= 2, "out at the exit");
}
