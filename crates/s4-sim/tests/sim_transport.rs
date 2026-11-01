//! Transport and recruiting through the eco-sector (`CEcoSector::TransportGood`,
//! `RecruiteWorker`) with the jobs agent's carrier role. Skipped when the install is absent.

use s4_sim::entity::flag;
use s4_sim::{EntityData, Grid, Player, Rules, World, eco, pile, tick};

const CARRIER: u16 = 1;
const BUILDER: u16 = 3;
const BREAD: u8 = 9;
const HAMMER: u8 = 18;

fn world() -> Option<World> {
    if !s4_formats::game_dir().join("GameData").exists() {
        eprintln!("skipping: install not found");
        return None;
    }
    let mut w = World::new(Grid::new(64).unwrap(), Rules::load_default().unwrap());
    w.players[1] = Some(Player { id: 1, race: 0, start_x: 0, start_y: 0 });
    w.layers.ground.fill(16);
    let owner = vec![1u8; w.grid.len()];
    w.init_tiling(&owner);
    Some(w)
}

fn carrier(w: &mut World, x: i32, y: i32) -> u16 {
    // The CSettler constructor gives it its carrier role and wait list.
    s4_sim::create::add_settler(w, x, y, 1, CARRIER).unwrap()
}

fn next_type(w: &World, s: u16) -> u8 {
    match &w.entities.by_index(s as u32).unwrap().data {
        EntityData::Settler(d) => d.job.carrier.next_type,
        _ => 0,
    }
}

#[test]
fn transport_good_moves_one_good() {
    let Some(mut w) = world() else { return };
    let e = eco::eco_of(&w, w.grid.index(30, 30));
    assert_ne!(e, 0, "the owned land is an eco-sector");
    let src = pile::add_pile(&mut w, 24, 20, BREAD, 5, pile::role::STORAGE).unwrap();
    let dst = pile::add_pile(&mut w, 24, 30, BREAD, 0, pile::role::STORAGE).unwrap();
    assert_eq!(w.economy.get(e).unwrap().goods_on_piles[BREAD as usize], 5);
    let c = carrier(&mut w, 20, 20);
    assert_eq!(eco::get_nearest_settler(&w, e, CARRIER, 24, 20), c, "the carrier offers itself");
    assert!(eco::transport_good(&mut w, e, src, dst));
    assert_eq!(pile::get(&w, src).map(|p| p.leaving), Some(1));
    assert_eq!(pile::get(&w, dst).map(|p| p.coming), Some(1));
    assert_eq!(eco::get_nearest_settler(&w, e, CARRIER, 24, 20), 0, "the carrier left the offer list");
    for _ in 0..800 {
        tick(&mut w, &[]);
        if pile::get(&w, dst).is_some_and(|p| p.amount == 1) && w.entities.by_index(c as u32).unwrap().flags & flag::OFFERED != 0 {
            break;
        }
    }
    let (s, d) = (pile::get(&w, src).unwrap(), pile::get(&w, dst).unwrap());
    assert_eq!((s.amount, s.leaving, d.amount, d.coming), (4, 0, 1, 0), "one good moved");
    assert_eq!(w.economy.get(e).unwrap().goods_on_piles[BREAD as usize], 5, "still five in the eco-sector");
    assert!(w.entities.by_index(c as u32).unwrap().flags & flag::OFFERED != 0, "the carrier is free again");
}

#[test]
fn recruite_worker_turns_a_carrier_into_a_builder() {
    let Some(mut w) = world() else { return };
    let e = eco::eco_of(&w, w.grid.index(30, 30));
    let hammers = pile::add_pile(&mut w, 40, 40, HAMMER, 2, pile::role::STORAGE).unwrap();
    let cs: Vec<u16> = (0..7).map(|k| carrier(&mut w, 10 + 3 * k, 12)).collect();
    assert_eq!(w.economy.get(e).unwrap().settlers[CARRIER as usize], 7);
    eco::change_min_max_values(&mut w, e, BUILDER, 1);
    assert_eq!(w.economy.get(e).unwrap().worker_delta, [1, 0]);
    eco::recruite_worker(&mut w, e);
    let chosen: Vec<u16> = cs.iter().copied().filter(|&c| next_type(&w, c) == BUILDER as u8).collect();
    assert_eq!(chosen.len(), 1, "one carrier ordered");
    let r = w.economy.get(e).unwrap();
    assert_eq!((r.worker_delta, r.carriers_with_job_order), ([0, 0], 1));
    assert_eq!(pile::get(&w, hammers).map(|p| p.leaving), Some(1), "its hammer reserved");
    assert_eq!(w.entities.by_index(chosen[0] as u32).unwrap().flags & flag::OFFERED, 0);
    // The head of the carrier offer list (the last carrier offered) was taken.
    assert_eq!(chosen[0], *cs.last().unwrap());
    // With only the minimum left free, nothing more is ordered.
    eco::change_min_max_values(&mut w, e, CARRIER, 100);
    eco::change_min_max_values(&mut w, e, BUILDER, 1);
    eco::recruite_worker(&mut w, e);
    assert_eq!(cs.iter().filter(|&&c| next_type(&w, c) == BUILDER as u8).count(), 1);
}

/// `CCarrierRole::SetFree` (0x573c10) of a carrier on its way to become a builder: the
/// eco-sector's builder order comes back (`ChangeMinMaxValues(3, 1)`, 0x4d5750), the pending
/// type is gone, and the carriers-with-job-order count stays (the original does not lower it).
#[test]
fn set_free_returns_a_recruit_order() {
    let Some(mut w) = world() else { return };
    let e = eco::eco_of(&w, w.grid.index(30, 30));
    pile::add_pile(&mut w, 40, 40, HAMMER, 2, pile::role::STORAGE).unwrap();
    let cs: Vec<u16> = (0..7).map(|k| carrier(&mut w, 10 + 3 * k, 12)).collect();
    eco::change_min_max_values(&mut w, e, BUILDER, 1);
    eco::recruite_worker(&mut w, e);
    let c = *cs.iter().find(|&&c| next_type(&w, c) == BUILDER as u8).unwrap();
    assert_eq!(w.economy.get(e).unwrap().worker_delta, [0, 0]);
    s4_sim::jobs::roles::set_free(&mut w, c, -1);
    assert_eq!(next_type(&w, c), 0);
    let r = w.economy.get(e).unwrap();
    assert_eq!((r.worker_delta, r.carriers_with_job_order), ([1, 0], 1));
    assert!(w.entities.by_index(c as u32).unwrap().flags & flag::OFFERED != 0, "offered again");
    // A geologist (49) order comes back as a specialist wish (+0x288 + 1).
    let g = cs[0];
    if let EntityData::Settler(d) = &mut w.entities.by_index_mut(g as u32).unwrap().data {
        d.job.carrier.next_type = 49;
    }
    s4_sim::jobs::roles::set_free(&mut w, g, -1);
    let i = eco::SPECIALISTS.iter().position(|&t| t == 49).unwrap();
    assert_eq!(w.economy.get(e).unwrap().specialist_wish[i], 1);
}

/// A carrier ordered to become a woodcutter (`OrderWorker`: next type, attached to the hut) is
/// set free: the hut hears `SettlerLeft` (0x541080: no inhabitant, state 1, update next tick)
/// and orders a new worker.
#[test]
fn set_free_makes_the_hut_order_again() {
    let Some(mut w) = world() else { return };
    pile::add_pile(&mut w, 30, 20, 4, 2, pile::role::STORAGE).unwrap(); // axes
    for k in 0..7 {
        carrier(&mut w, 10 + 3 * k, 12);
    }
    let hut = s4_sim::create::add_building(&mut w, 30, 30, 1, 1).unwrap();
    let inhabitant = |w: &World| s4_sim::buildings::role(w, hut).unwrap().base.inhabitant;
    for _ in 0..40 {
        tick(&mut w, &[]);
        if inhabitant(&w) != 0 {
            break;
        }
    }
    let c = inhabitant(&w);
    assert_ne!(c, 0, "the hut ordered its worker");
    let kind = next_type(&w, c);
    assert_ne!(kind, 0, "a carrier that becomes the woodcutter");
    assert!(w.entities.by_index(c as u32).unwrap().flags & flag::ATTACHED != 0);
    s4_sim::jobs::roles::set_free(&mut w, c, -1);
    let state = |w: &World| match &s4_sim::buildings::role(w, hut).unwrap().kind {
        s4_sim::buildings::RoleKind::Gather(s) => s.work.state,
        k => panic!("{k:?}"),
    };
    assert_eq!((inhabitant(&w), state(&w)), (0, 1));
    let ce = w.entities.by_index(c as u32).unwrap();
    assert_eq!(ce.flags & flag::ATTACHED, 0);
    assert!(s4_sim::buildings::role(&w, hut).unwrap().base.observers.iter().all(|&(o, _)| o != c), "off the observer list");
    let mut t = 0;
    while inhabitant(&w) == 0 && t < 100 {
        tick(&mut w, &[]);
        t += 1;
    }
    eprintln!("ordered again after {t} ticks: {}", inhabitant(&w));
    assert_ne!(inhabitant(&w), 0, "a worker is ordered again");
}

/// A storage area (`CStorageBuildingRole`): its trade piles register as free storage piles; once
/// bread is switched on, a free pile of bread nearby asks for space (`RequestSpace`), a storage
/// pile is reserved for bread and a carrier brings the goods one by one.
#[test]
fn storage_area_takes_switched_on_goods() {
    let Some(mut w) = world() else { return };
    let b = s4_sim::create::add_building(&mut w, 32, 32, 1, s4_sim::buildings::storage::STORAGE_AREA as u8).unwrap();
    let e = s4_sim::buildings::storage::eco_of_building(&w, b);
    assert_ne!(e, 0);
    let n = w.rules.building(0, 34).piles.len();
    let free = w.economy.get(e).unwrap().free_storage_piles.len();
    assert!(n > 0 && free == n, "{free} of {n} storage piles registered free");
    for _ in 0..3 {
        carrier(&mut w, 20, 44);
    }
    // As after a map load (`CalculateInitialFreeBeds`): beds for the carriers, so none strike.
    s4_sim::eco::calculate_initial_free_beds(&mut w);
    let src = pile::add_pile(&mut w, 22, 44, BREAD, 3, pile::role::ANYWHERE).unwrap();
    // Not switched on: nothing moves.
    for _ in 0..100 {
        tick(&mut w, &[]);
    }
    assert_eq!(pile::get(&w, src).map(|p| p.amount), Some(3));
    let cmd = s4_sim::PlayerCommand { player: 1, command: s4_sim::Command::StorageGoodSwitch { building: b, good: BREAD as u16, value: 1 } };
    tick(&mut w, &[cmd]);
    assert_eq!(w.economy.get(e).unwrap().storage_goods, vec![(b, BREAD)]);
    let stored = |w: &World| -> Vec<(u8, u8, u8)> {
        w.economy.get(e).unwrap().good_storage_piles.iter().filter_map(|&(_, p, _)| pile::get(w, p).map(|d| (d.good, d.amount, d.trade_type))).collect()
    };
    let mut ticks = 0;
    while stored(&w).iter().map(|s| s.1 as u32).sum::<u32>() < 3 && ticks < 6000 {
        tick(&mut w, &[]);
        ticks += 1;
    }
    let stored = stored(&w);
    eprintln!("after {ticks} ticks");
    eprintln!("stored piles: {stored:?}");
    assert_eq!(stored.iter().map(|s| s.1 as u32).sum::<u32>(), 3, "all bread stored");
    assert!(stored.iter().all(|s| s.0 == BREAD && s.2 == 2));
    assert_eq!(w.economy.get(e).unwrap().goods_on_piles[BREAD as usize], 3);
}

/// `UpdateStrikeSettlers`: carriers beyond the beds leave the offer as strikers (every 32 ticks);
/// a finished residence gives beds and they come back.
#[test]
fn carriers_beyond_the_beds_strike() {
    let Some(mut w) = world() else { return };
    let offered = |w: &World| {
        let mut n = 0;
        let mut id = w.economy.get(1).unwrap().settler_offer[CARRIER as usize];
        while id != 0 {
            n += 1;
            id = w.entities.by_index(id as u32).unwrap().next;
        }
        n
    };
    for k in 0..25 {
        carrier(&mut w, 10 + k % 10, 10 + k / 10);
    }
    s4_sim::eco::calculate_initial_free_beds(&mut w);
    assert_eq!(s4_sim::eco::current_total_beds(&w, 1), 40, "10 * ceil(25 / 10) + 10");
    for k in 0..20 {
        carrier(&mut w, 10 + k % 10, 20 + k / 10);
    }
    assert_eq!(offered(&w), 45);
    for _ in 0..31 {
        tick(&mut w, &[]);
    }
    assert_eq!(s4_sim::eco::striking_carriers(&w, 1), 0, "the check runs every 32 ticks");
    tick(&mut w, &[]);
    assert_eq!((s4_sim::eco::striking_carriers(&w, 1), offered(&w)), (5, 40));
    // A small residence (10 beds) ends the strike: its Init (0x54db60) gives the 5 strikers 5 of
    // its beds (+0x17c) and runs UpdateStrikeSettlers at once.
    let home = s4_sim::create::add_building(&mut w, 40, 40, 1, 40).unwrap();
    assert_eq!(s4_sim::eco::current_total_beds(&w, 1), 50);
    assert_eq!((s4_sim::eco::striking_carriers(&w, 1), offered(&w)), (0, 45));
    assert_eq!(s4_sim::buildings::residence::residents(&w, home), Some(5));
    // Its first update, 31 ticks later, brings a new carrier for its 6th bed.
    for _ in 0..32 {
        tick(&mut w, &[]);
    }
    assert_eq!((s4_sim::eco::striking_carriers(&w, 1), offered(&w)), (0, 46));
    assert_eq!(s4_sim::buildings::residence::residents(&w, home), Some(6));
}

/// Command 5002 (`CBuilding::CrushBuilding`) on a storage area: the footprint is released,
/// half the cost lies on piles nearby, the eco-sector forgets the building's storage goods,
/// and the building is killed, then destroyed at its schedule slot.
#[test]
fn crushing_a_storage_area() {
    let Some(mut w) = world() else { return };
    let (x, y) = (32, 32);
    let b = s4_sim::create::add_building(&mut w, x, y, 1, s4_sim::buildings::storage::STORAGE_AREA as u8).unwrap();
    let e = s4_sim::buildings::storage::eco_of_building(&w, b);
    let cmd = |c| s4_sim::PlayerCommand { player: 1, command: c };
    tick(&mut w, &[cmd(s4_sim::Command::StorageGoodSwitch { building: b, good: BREAD as u16, value: 1 })]);
    assert_eq!(w.economy.get(e).unwrap().storage_goods, vec![(b, BREAD)]);
    let rule = w.rules.building(0, 34).clone();
    let cells: Vec<usize> = rule.building_cells.iter().map(|&(dx, dy)| w.grid.index((x + dx as i32) as u32, (y + dy as i32) as u32)).collect();
    assert!(cells.iter().any(|&n| w.layers.flags[n] & 0x08 != 0));
    let goods_before = |w: &World, g: u8| -> u32 { w.entities.iter().filter_map(|(i, _)| pile::get(w, i)).filter(|p| p.good == g).map(|p| p.amount as u32).sum() };
    let (boards, stone) = (goods_before(&w, 7), goods_before(&w, 32));
    // Another player's command is ignored.
    tick(&mut w, &[s4_sim::PlayerCommand { player: 2, command: s4_sim::Command::CrushBuilding { building: b as u32 } }]);
    assert!(!w.entities.by_index(b as u32).unwrap().is_dead());
    tick(&mut w, &[cmd(s4_sim::Command::CrushBuilding { building: b as u32 })]);
    let dead = w.entities.by_index(b as u32).is_none_or(|e| e.is_dead());
    assert!(dead, "killed");
    // Flags 0x01 / 0x08 gone; 0x02 only where a pile (the dropped goods) now lies.
    let bad: Vec<(usize, u8, u16)> = cells
        .iter()
        .filter(|&&n| w.layers.flags[n] & 0x09 != 0 || (w.layers.flags[n] & 0x02 != 0 && w.layers.deco_object[n] == 0))
        .map(|&n| (n, w.layers.flags[n], w.layers.deco_object[n]))
        .collect();
    assert!(bad.is_empty(), "footprint released: {bad:?}");
    assert!(cells.iter().all(|&n| w.layers.walk_move[n] & 7 == 2 || w.layers.deco_object[n] != 0));
    assert!(w.economy.get(e).unwrap().storage_goods.is_empty());
    assert_eq!(goods_before(&w, 7) - boards, rule.cost_boards as u32 / 2, "half the boards");
    assert_eq!(goods_before(&w, 32) - stone, rule.cost_stone as u32 / 2, "half the stone");
    for _ in 0..32 {
        tick(&mut w, &[]);
    }
    assert!(w.entities.by_index(b as u32).is_none_or(|e| e.obj_type != s4_sim::ObjType::Building), "destroyed");
}

/// Crushing a construction site digging its ground: the leveling flag 0x10 and the digging slot
/// go, half of the used boards come back (their pile exists), the site is killed.
#[test]
fn crushing_a_construction_site() {
    let Some(mut w) = world() else { return };
    // Uneven ground, so the site has to dig.
    for i in 0..w.grid.len() {
        w.layers.height[i] = 10 + ((i * 7) % 5) as u8;
    }
    let cmd = |c| s4_sim::PlayerCommand { player: 1, command: c };
    tick(&mut w, &[cmd(s4_sim::Command::Build { building: 3, x: 32, y: 32 })]);
    let (&id, s) = w.sites.roles.iter().next().expect("a site");
    let slot = s.dig_slot;
    assert!(slot >= 0, "the site digs");
    let flagged = |w: &World| w.layers.flags.iter().filter(|&&f| f & 0x10 != 0).count();
    assert!(flagged(&w) > 0);
    let src = pile::add_pile(&mut w, 20, 20, 7, 1, pile::role::ANYWHERE).unwrap();
    {
        let s = w.sites.roles.get_mut(&id).unwrap();
        s.state = 2;
        s.boards_used = 5;
        s.piles[0] = src;
    }
    let boards = |w: &World| -> u32 { w.entities.iter().filter_map(|(i, _)| pile::get(w, i)).filter(|p| p.good == 7).map(|p| p.amount as u32).sum() };
    let before = boards(&w);
    tick(&mut w, &[cmd(s4_sim::Command::CrushBuilding { building: id as u32 })]);
    assert_eq!(flagged(&w), 0, "leveling flags cleared");
    assert!(!w.sites.dig_slots.contains_key(&(1, slot as u8)), "slot freed");
    assert_eq!(boards(&w) - before, 2, "half of 5 used boards");
    assert!(w.entities.by_index(id as u32).is_none_or(|e| e.is_dead()));
}
