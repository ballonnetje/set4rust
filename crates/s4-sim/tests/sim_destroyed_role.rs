//! A destroyed building's role (a site's too) goes with it (`CMapObjectMgr::Destroy` 0x56d200 → `~CBuilding`
//! 0x535e40 deletes the role at +0x50). Playtest P4 (towerbug, dark01): the role stayed behind,
//! a later construction site at the same entity index took the old building's piles for its own
//! (`GetPileIdWithGood` gave none), `InitTransport` found nothing to deliver to and the site
//! waited forever with its builders idle. Skipped when the game data is not present.

use s4_sim::entity::{EntityData, ObjType};
use s4_sim::{Command, PlayerCommand, World};

const WOODCUTTER: u8 = 1;

fn roman02() -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map");
    if !p.exists() {
        return None;
    }
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    let rules = s4_sim::Rules::load_default().ok()?;
    let (w, _) = s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).ok()?;
    Some(w)
}

/// Every site's material piles are its own (vtbl+0x38 of the building it is).
fn sites_have_their_piles(w: &World) {
    for (&id, s) in &w.sites.roles {
        assert!(s4_sim::buildings::role(w, id).is_none(), "site {id} has a building role");
        for (k, good) in [s4_sim::build::order::BOARDS, s4_sim::build::order::STONES].into_iter().enumerate() {
            assert_eq!(s4_sim::buildings::pile_with_good(w, id, good), s.piles[k], "site {id} good {good}");
        }
    }
}

#[test]
fn destroyed_building_leaves_no_role_for_the_next_entity() {
    let Some(mut w) = roman02() else { return };
    // Player 1's finished buildings with a role, lowest index first.
    let victims: Vec<u16> = w
        .entities
        .iter()
        .filter(|(id, e)| e.owner == 1 && e.obj_type == ObjType::Building && s4_sim::buildings::role(&w, *id).is_some() && !w.sites.roles.contains_key(id))
        .map(|(id, _)| id)
        .filter(|&id| !s4_sim::buildings::have_inhabitant(&w, id) || w.entities.by_index(id as u32).is_some_and(|e| !w.rules.is_military(e.kind)))
        .take(3)
        .collect();
    assert!(!victims.is_empty());
    let cmds: Vec<PlayerCommand> = victims.iter().map(|&b| PlayerCommand { player: 1, command: Command::CrushBuilding { building: b as u32 } }).collect();
    s4_sim::tick(&mut w, &cmds);
    // Killed now, destroyed 31 ticks on: the role lives until then.
    for _ in 0..40 {
        s4_sim::tick(&mut w, &[]);
    }
    for &b in &victims {
        let now = w.entities.by_index(b as u32).map(|e| e.obj_type);
        assert!(now.is_none() || !w.sites.roles.contains_key(&b) && now != Some(ObjType::Building), "{b} still a building: {now:?}");
        assert!(s4_sim::buildings::role(&w, b).is_none(), "building {b}'s role outlived it (index now {now:?})");
    }
    // New sites take the lowest free indices, i.e. the victims' (or entities made meanwhile).
    let size = w.grid.size() as i32;
    let spots: Vec<(u16, u16)> = s4_sim::build::possible_sites(&w, 1, WOODCUTTER, (0, 0), (size, size)).into_iter().map(|s| (s.0, s.1)).step_by(7).take(6).collect();
    for (x, y) in spots {
        s4_sim::tick(&mut w, &[PlayerCommand { player: 1, command: Command::Build { building: WOODCUTTER as u32, x, y } }]);
    }
    assert!(!w.sites.roles.is_empty());
    let reused = w.sites.roles.keys().filter(|id| victims.contains(id)).count();
    assert!(reused > 0, "no site at a destroyed building's index: {:?} vs {victims:?}", w.sites.roles.keys().collect::<Vec<_>>());
    sites_have_their_piles(&w);
    // roman02 starts without diggers and builders: recruit some (command 5027).
    let site = *w.sites.roles.keys().next().unwrap();
    let eco = match &w.entities.by_index(site as u32).unwrap().data {
        EntityData::Building(d) => s4_sim::eco::eco_of(&w, w.grid.index(d.ensign.x() as u32, d.ensign.y() as u32)),
        _ => unreachable!(),
    };
    let recruit: Vec<PlayerCommand> = [2, 3].iter().map(|&kind| PlayerCommand { player: 1, command: Command::SetUnitReservation { eco_sector: eco, kind, value: 4 } }).collect();
    s4_sim::tick(&mut w, &recruit);
    for _ in 0..2000 {
        s4_sim::tick(&mut w, &[]);
    }
    sites_have_their_piles(&w);
    // Material reached the sites (roman02 starts with boards and stones in store).
    let supplied = w.sites.roles.values().filter(|s| s.boards_used + s.stones_used > 0 || s.piles.iter().any(|&p| s4_sim::pile::get(&w, p).is_some_and(|d| d.amount + d.coming > 0))).count();
    assert!(supplied > 0, "no site supplied: {:?}", w.sites.roles.values().map(|s| (s.state, s.boards_used, s.stones_used)).collect::<Vec<_>>());
    // A crushed site's role goes when the site is destroyed.
    let gone: Vec<u16> = w.sites.roles.keys().copied().take(2).collect();
    let cmds: Vec<PlayerCommand> = gone.iter().map(|&b| PlayerCommand { player: 1, command: Command::CrushBuilding { building: b as u32 } }).collect();
    s4_sim::tick(&mut w, &cmds);
    for _ in 0..40 {
        s4_sim::tick(&mut w, &[]);
    }
    for b in gone {
        let now = w.entities.by_index(b as u32).map(|e| (e.obj_type, e.flags));
        assert!(!w.sites.roles.contains_key(&b) || now.is_some_and(|(t, f)| t == ObjType::Building && f & s4_sim::entity::flag::BIRTH != 0), "site {b}'s role outlived it (index now {now:?})");
    }
}
