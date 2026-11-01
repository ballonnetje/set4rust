//! The Dark Tribe computer player on its own (`CAIDarkTribe` 0x4490e0 and the mushroom farm agent
//! 0x430b00) on Dark Tribe campaign maps with the Dark Tribe at control 3, no commands at all.
//! Skipped without the game data.

use s4_sim::World;
use s4_sim::entity::ObjType;

fn dark_game(map: &str) -> Option<(World, u8)> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join(map);
    let (Ok(map), Ok(rules)) = (s4_formats::map::MapFile::open(&p), s4_sim::Rules::load_default()) else { return None };
    let (w, _) = s4_sim::new_game(&map, rules.clone(), &s4_sim::NewGame::default()).unwrap();
    let dt = (1..=8u8).find(|&p| w.player(p).is_some_and(|q| q.race == 3))?;
    let mut setup = s4_sim::NewGame::default();
    setup.control[dt as usize - 1] = 3;
    let (w, _) = s4_sim::new_game(&map, rules, &setup).unwrap();
    Some((w, dt))
}

fn buildings(w: &World, dt: u8, kind: u16) -> Vec<u16> {
    w.entities.iter().filter(|(_, e)| e.owner == dt && e.kind == kind && e.obj_type == ObjType::Building && !e.is_dead()).map(|(i, _)| i).collect()
}

fn plan(w: &World, dt: u8) -> &s4_sim::ai::dark::DarkTribe {
    s4_sim::ai::player::get(w, dt).and_then(|p| p.dark_ai.as_deref()).expect("the Dark Tribe's plan")
}

/// dark03, 20 000 ticks, two worlds in lockstep (hashes every 500 ticks): the plan stays sound,
/// the economy's lists too, and the farm agent orders farms as `DTMaxFarmsBase` +
/// `DTMaxFarmsPerHour` allow — each order a farm (site) at the ordered node.
#[test]
fn the_dark_tribe_ai_plays_dark03_for_20000_ticks() {
    let Some((mut a, dt)) = dark_game("dark03.map") else { return };
    let (mut b, _) = dark_game("dark03.map").unwrap();
    let start = plan(&a, dt).clone();
    start.check().unwrap();
    assert!(!start.cells.is_empty() && !start.frontier.is_empty() && !start.farm_cells.is_empty(), "dark03's dark land is planned");
    let farms0 = buildings(&a, dt, 49).len();
    let mut orders = Vec::new();
    for t in 1..=20_000u32 {
        s4_sim::tick(&mut a, &[]);
        s4_sim::tick(&mut b, &[]);
        orders.extend(a.ai.orders.iter().filter(|o| o.kind == 5025).map(|o| (t, *o)));
        if t % 500 == 0 {
            assert_eq!(a.state_hash(), b.state_hash(), "the worlds part at tick {t}");
            s4_sim::eco::check_offer_lists(&a).unwrap();
            plan(&a, dt).check().unwrap();
        }
    }
    assert!(!orders.is_empty(), "the Dark Tribe ordered nothing");
    for &(t, o) in &orders {
        assert_eq!((o.kind, o.player, o.wparam), (5025, dt, 49), "tick {t}: {o:?}");
        // The order ran at the start of the next tick: a farm stands at the node.
        let (x, y) = ((o.lparam & 0xffff) as i32, (o.lparam >> 16) as i32);
        assert!(
            buildings(&a, dt, 49).iter().any(|&f| a.entities.by_index(f as u32).is_some_and(|e| (e.x(), e.y()) == (x, y))),
            "no farm at the order of tick {t} ({x}, {y})"
        );
    }
    // Each order waits `DTMushroomFarmDelay` · 14 + 0..127 ticks.
    for w in orders.windows(2) {
        assert!(w[1].0 - w[0].0 >= 300 * 14, "orders at {} and {}", w[0].0, w[1].0);
    }
    // Never past the hourly limit (level 2: base 5, 15 per hour).
    let limit = |t: u32| (a.ai.vars.get(dt, 13) as u32 * t / 50_400) as i32 + a.ai.vars.get(dt, 12);
    assert!(orders.iter().enumerate().all(|(k, &(t, _))| ((farms0 + k) as i32) < limit(t)));
    let d = plan(&a, dt);
    eprintln!(
        "orders at {:?}; farms {farms0} → {}; cells {}, frontier {}, candidates {}, targets {}, farm cells {}, radius {}",
        orders.iter().map(|o| o.0).collect::<Vec<_>>(),
        buildings(&a, dt, 49).len(),
        d.cells.len(),
        d.frontier.len(),
        d.candidates.len(),
        d.targets.len(),
        d.farm_cells.len(),
        d.radius
    );
}

/// AO_trojan04 lists three hall positions for its Dark Tribe player. Once its standing hall is
/// destroyed (`DestroyBuilding` sets the hall tick), the AI orders a new one at the first listed
/// position that passes the check, in the first update `DTManakopterHallDelay` · 14 ticks later
/// (0x44b260), and the hall site is made at the start of the next tick.
#[test]
fn the_dark_tribe_ai_orders_a_hall_after_the_delay() {
    let Some((mut w, dt)) = dark_game("AO_trojan04.map") else { return };
    for _ in 0..100 {
        s4_sim::tick(&mut w, &[]);
    }
    assert!(w.ai.orders.iter().all(|o| o.wparam != 80), "no hall order while the hall stands");
    let hall = buildings(&w, dt, 80)[0];
    let (x, y) = {
        let e = w.entities.by_index(hall as u32).unwrap();
        (e.x(), e.y())
    };
    s4_sim::buildings::crush::destroy_building_at(&mut w, x, y, -1);
    let gone = w.tick;
    let delay = w.ai.vars.get(dt, 19) as u32;
    let due = gone + (delay + 1) * 14;
    let mut ordered = None;
    while w.tick < due + 5 {
        s4_sim::tick(&mut w, &[]);
        if let Some(o) = w.ai.orders.iter().find(|o| o.wparam == 80) {
            ordered = Some((w.tick, *o));
            break;
        }
    }
    let (t, o) = ordered.expect("a hall order");
    assert_eq!(t, due, "ordered in the first update after the delay");
    let pos = s4_sim::Y16X16(o.lparam);
    assert!(w.sites.halls.positions[dt as usize].contains(&pos), "at a listed position");
    s4_sim::tick(&mut w, &[]);
    let halls = buildings(&w, dt, 80);
    assert!(halls.iter().any(|&h| w.entities.by_index(h as u32).is_some_and(|e| (e.x(), e.y()) == (pos.x(), pos.y()))), "the hall site stands");
    s4_sim::tick(&mut w, &[]);
    assert!(w.ai.orders.iter().all(|o| o.wparam != 80), "no second order while it stands");
}

/// Every Dark Tribe campaign map: the plan built at the game start is sound and the Dark Tribe
/// plays 5 000 ticks on its own with sound lists.
#[test]
fn every_dark_campaign_map_plans_and_plays() {
    for k in 1..=12 {
        let name = format!("dark{k:02}.map");
        let Some((mut w, dt)) = dark_game(&name) else { continue };
        let Some(p) = s4_sim::ai::player::get(&w, dt) else { continue };
        let Some(d) = p.dark_ai.as_deref() else {
            // Without a temple or fortress there is no sector AI and no plan (0x43b3a0).
            assert!(buildings(&w, dt, 50).is_empty() && buildings(&w, dt, 51).is_empty(), "{name}");
            continue;
        };
        d.check().unwrap_or_else(|e| panic!("{name}: {e}"));
        for _ in 0..5000 {
            s4_sim::tick(&mut w, &[]);
        }
        s4_sim::eco::check_offer_lists(&w).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        let d = plan(&w, dt);
        d.check().unwrap_or_else(|e| panic!("{name}: {e}"));
        eprintln!("{name}: cells {}, frontier {}, targets {}, farm cells {}", d.cells.len(), d.frontier.len(), d.targets.len(), d.farm_cells.len());
    }
}

/// The plan follows the dark land (`PostAIEvent` 0x1d → the sector AI → 0x449110): a target
/// that turns dark stops being one and its frontier cell counts the change; a frontier cell
/// darkened all over leaves the frontier and settles; lightened again all over, it loses its entry.
#[test]
fn the_plan_follows_the_dark_land() {
    let Some((mut w, dt)) = dark_game("dark03.map") else { return };
    s4_sim::tick(&mut w, &[]);
    let node = plan(&w, dt).targets[0];
    let (x, y) = ((node & 0xffff) as i32, (node >> 16) as i32);
    let cell = ((y as u32 >> 4) << 16) | (x as u32 >> 4);
    let hits = |w: &World| {
        let d = plan(w, dt);
        d.frontier.iter().position(|&c| c == cell).map(|i| d.hits[i])
    };
    assert_eq!(hits(&w), Some(0));
    s4_sim::dark::darken_around(&mut w, x, y, false);
    s4_sim::tick(&mut w, &[]);
    assert!(!plan(&w, dt).targets.contains(&node), "the darkened target is dropped");
    assert_eq!(hits(&w), Some(1), "its cell counts the change");

    let (x0, y0) = ((x >> 4) << 4, (y >> 4) << 4);
    for yy in y0..y0 + 16 {
        for xx in x0..x0 + 16 {
            s4_sim::dark::darken_around(&mut w, xx, yy, false);
        }
    }
    s4_sim::tick(&mut w, &[]);
    let d = plan(&w, dt);
    d.check().unwrap();
    assert_eq!(hits(&w), None, "the dark cell left the frontier");
    assert!(!d.candidates.contains(&cell));
    assert!(d.targets.iter().all(|&t| ((t >> 20) << 16 | (t & 0xffff) >> 4) != cell), "no target left in it");
    let state = d.cells.iter().find(|e| e.0 == cell).map(|e| e.1);
    assert!(matches!(state, Some(0 | 1 | 4)), "settled: {state:?}");

    for yy in y0..y0 + 16 {
        for xx in x0..x0 + 16 {
            s4_sim::dark::lighten(&mut w, xx, yy, 0);
        }
    }
    s4_sim::tick(&mut w, &[]);
    let d = plan(&w, dt);
    d.check().unwrap();
    assert!(d.cells.iter().all(|e| e.0 != cell), "the light cell has no entry");
}

/// `CAIAgentDarkTribeRecruitment` (0x430070) on dark03: every 100 ticks (once mana allows) the
/// temple's orders for gardeners, shamans and, below `SoldierLimitAbsolute`, swordsmen and bowmen
/// are replaced (5047, amount + 10000); the next tick the temple holds exactly those amounts.
#[test]
fn recruitment_sets_the_temple_orders() {
    let Some((mut w, dt)) = dark_game("dark03.map") else { return };
    let mut seen = Vec::new();
    for _ in 0..15_000 {
        s4_sim::tick(&mut w, &[]);
        let o: Vec<_> = w.ai.orders.iter().filter(|o| o.kind == 5047).copied().collect();
        if o.is_empty() {
            continue;
        }
        let temple = (o[0].wparam >> 16) as u16;
        assert!(o.iter().all(|x| x.player == dt && x.wparam >> 16 == temple as u32 && x.lparam >= 10000), "{o:?}");
        let kinds: Vec<u32> = o.iter().map(|x| x.wparam & 0xffff).collect();
        assert!(kinds == [52, 54] || kinds == [52, 54, 29, 32], "{kinds:?}");
        assert!(o.iter().all(|x| x.lparam - 10000 <= if x.wparam & 0xffff >= 52 { 5 } else { 14 }));
        s4_sim::tick(&mut w, &[]);
        let st = s4_sim::buildings::dark_temple::get(&w, temple).expect("the dark temple");
        for x in &o {
            let slot = s4_sim::buildings::dark_temple::UNIT_TYPES.iter().position(|&t| t as u32 == x.wparam & 0xffff).unwrap();
            // The temple may have produced one unit of the slot in that tick.
            let want = (x.lparam - 10000) as i32;
            assert!(st.orders[slot] == want || st.orders[slot] == want - 1, "slot {slot}: {} for {want}", st.orders[slot]);
        }
        seen.push(w.tick);
    }
    assert!(seen.len() >= 2, "no recruitment orders: {seen:?}, mana {}", s4_sim::magic::mana(&w, dt));
    eprintln!("recruitment at {seen:?}");
}

/// Infos, member lists and the timer list agree: every member's info names its task force, every
/// info is a member there, every group task force is on the timer list once.
fn check_task_forces(w: &World, dt: u8) -> Result<(), String> {
    use s4_sim::ai::player::TfRef;
    let a = s4_sim::ai::player::get(w, dt).ok_or("no AI")?;
    let mut refs: Vec<TfRef> = (0..5).map(|k| TfRef::Reservoir(dt, k)).collect();
    refs.push(TfRef::Squad(dt));
    for (s, sec) in a.body.sectors.iter().enumerate() {
        for (k, t) in sec.slots.iter().enumerate() {
            if t.is_some() {
                refs.push(TfRef::Slot(dt, s as u8, k as u8));
            }
        }
        for it in &sec.group.items {
            refs.push(TfRef::Group(dt, s as u8, it.uid));
            if w.ai.timers.iter().filter(|&&r| r == TfRef::Group(dt, s as u8, it.uid)).count() != 1 {
                return Err(format!("group task force {} not once on the timer list", it.uid));
            }
            if it.link != 0 && sec.group.get(it.link).is_none_or(|o| o.link != it.uid) {
                return Err(format!("one-way link {} → {}", it.uid, it.link));
            }
        }
    }
    for &r in &refs {
        for &m in &s4_sim::ai::taskforce::get(w, r).unwrap().members {
            if w.ai.infos.get(&m) != Some(&r) {
                return Err(format!("member {m} of {r:?} has info {:?}", w.ai.infos.get(&m)));
            }
        }
    }
    for (&id, &r) in &w.ai.infos {
        if r.player() == dt && !s4_sim::ai::taskforce::get(w, r).is_some_and(|t| t.members.contains(&id)) {
            return Err(format!("info of {id} names {r:?}, which does not hold it"));
        }
    }
    Ok(())
}

/// The squad manager (0x4305d0), the dark gardener (0x430930) and shaman (0x430820) agents on
/// dark03, 30 000 ticks in lockstep: units collect into slots and task forces, which stay sound;
/// squads escort the gardeners and shamans; the gardeners, sent to the plan's targets, darken
/// land (without orders they idle).
#[test]
fn the_dark_tribe_forms_task_forces() {
    let Some((mut a, dt)) = dark_game("dark03.map") else { return };
    let (mut b, _) = dark_game("dark03.map").unwrap();
    let dark = |w: &World| w.layers.flags.iter().filter(|&&f| f & 4 != 0).count();
    let dark0 = dark(&a);
    for t in 1..=30_000u32 {
        s4_sim::tick(&mut a, &[]);
        s4_sim::tick(&mut b, &[]);
        if t % 1000 == 0 {
            assert_eq!(a.state_hash(), b.state_hash(), "the worlds part at tick {t}");
            check_task_forces(&a, dt).unwrap_or_else(|e| panic!("tick {t}: {e}"));
            let p = s4_sim::ai::player::get(&a, dt).unwrap();
            let sec = &p.body.sectors[0];
            let groups: Vec<String> = sec.group.items.iter().map(|i| format!("{}:{}x{}@{:#x}/{}", i.uid, i.kind, i.tf.members.len(), i.tf.state(), i.tf.phase())).collect();
            let slots: Vec<String> = sec.slots.iter().enumerate().filter_map(|(k, s)| s.as_ref().map(|s| format!("{k}:{}", s.1.members.len()))).collect();
            eprintln!("{t}: slots {slots:?} groups {groups:?}");
        }
    }
    let sec = &s4_sim::ai::player::get(&a, dt).unwrap().body.sectors[0];
    let of = |k: i32| sec.group.items.iter().filter(|i| i.kind == k).collect::<Vec<_>>();
    assert!(!of(2).is_empty() && !of(11).is_empty() && !of(12).is_empty(), "squads, gardeners and shamans");
    assert!(of(11).iter().chain(of(12).iter()).all(|i| i.link != 0 && sec.group.get(i.link).is_some_and(|q| q.kind == 2)), "each escorted by a squad");
    assert!(dark(&a) > dark0 + 3000, "dark land {} from {dark0}", dark(&a));
}

