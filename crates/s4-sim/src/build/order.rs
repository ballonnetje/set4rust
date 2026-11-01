//! `CBuildingSiteRole`'s orders to the eco-sector, from the shipped exe (vtable 0x10591bc):
//! `OrderDigger` (0x5425f0), `OrderBuilder` (0x5426b0) and `OrderMaterial` (0x5427f0), which the
//! site's `LogicUpdate` (0x541cc0) calls per state (2: diggers; 3: diggers, material; 4:
//! material; 5: builders, material), and the site's pile slots `GetBuildingNeed` (vtbl+52,
//! 0x542f40) and `GetPileIdWithGood` (vtbl+56, 0x542fe0; vtbl+64 `GetPileIdWithNeedForGood` is
//! the base, which calls it).
//!
//! Role fields (release offsets): +0x184 diggers wanted, +0x188 leveling effort, +0x18c state,
//! +0x18d diggers at work, +0x190 builders wanted (info +7, `builderNumber`), +0x191 builders at
//! work, +0x193/+0x195/+0x197 boards / stones / gold needed and +0x194/+0x196/+0x198 used,
//! +0x1b0/+0x1b2/+0x1b4 their piles, +0x1b8..+0x1bc the site's priority flags.

use super::site::Site;
use crate::World;
use crate::eco::SitePriority;

/// Material goods: boards, stones, gold (the order of [`Site::piles`]).
pub const BOARDS: u8 = 7;
pub const STONES: u8 = 32;
pub const GOLD: u8 = 14;

/// The site's eco-sector, at its ensign (+0x48): 0 on unowned land.
fn eco(world: &World, site: u16) -> u16 {
    let Some(e) = world.entities.by_index(site as u32) else { return 0 };
    let crate::entity::EntityData::Building(d) = &e.data else { return 0 };
    world.grid.checked_index(d.ensign.x(), d.ensign.y()).map_or(0, |n| crate::eco::eco_of(world, n))
}

/// The site's priority flags (+0x1b8 .. +0x1bc) as the eco-sector counts them: with digger
/// priority it needs its diggers wanted (+0x184), with builder priority its `builderNumber`.
pub fn site_priority(world: &World, site: u16) -> SitePriority {
    let Some(s) = world.sites.roles.get(&site) else { return SitePriority::default() };
    let p = s.prio;
    SitePriority {
        prio: p.on,
        diggers: p.diggers.then_some(s.diggers as i16),
        builders: p.builders.then_some(wanted_builders(world, site) as i16),
        boards: p.boards,
        stone: p.stone,
    }
}

/// At most this many prioritised sites per eco-sector (eco +0x14 < 10).
const MAX_PRIO_SITES: u8 = 10;

/// `CBuildingSiteRole` vtbl+36 (release 0x542e40, command 5004): toggle the site's priority at
/// the eco-sector of its ensign (+0x48). On: +0x1b8 and the urgent board, stone, digger and
/// builder priorities as far as they apply; off: all of them cleared.
pub fn prioritize(world: &mut World, site: u16) {
    let eco = eco(world, site);
    if eco == 0 {
        return;
    }
    let Some(s) = world.sites.roles.get_mut(&site) else { return };
    if s.prio.on {
        s.prio.on = false;
        deactivate_all(world, site, eco);
    } else {
        s.prio.on = true;
        activate_boards(world, site, eco);
        activate_stone(world, site, eco);
        activate_diggers(world, site, eco);
        activate_builders(world, site, eco);
    }
}

/// Change the eco-sector's priority counters: prioritised sites (+0x14), board / stone priorities
/// (+0x1c / +0x1d), digger / builder priority sites (+0x15 / +0x16) and the diggers / builders
/// they need (+0x18 / +0x1a).
fn eco_prio(world: &mut World, eco: u16, f: impl FnOnce(&mut crate::eco::EcoSector)) {
    if let Some(e) = world.economy.get_mut(eco) {
        f(e);
    }
}

/// Whether the eco-sector takes one more prioritised site and the site's building works (0x1000).
fn may_activate(world: &World, site: u16, eco: u16) -> bool {
    let room = world.economy.get(eco).is_some_and(|e| e.sites_with_prio < MAX_PRIO_SITES);
    room && world.entities.by_index(site as u32).is_some_and(|e| e.flags & 0x1000 != 0)
}

fn set_prio(world: &mut World, site: u16, f: impl FnOnce(&mut super::site::SitePrio)) -> super::site::SitePrio {
    let Some(s) = world.sites.roles.get_mut(&site) else { return Default::default() };
    f(&mut s.prio);
    s.prio
}

fn prio(world: &World, site: u16) -> super::site::SitePrio {
    world.sites.roles.get(&site).map(|s| s.prio).unwrap_or_default()
}

/// `CheckActivateUrgentBoards` (0x543220): the site still needs boards that are not all on its
/// pile already. A site's first priority counts it as prioritised.
fn activate_boards(world: &mut World, site: u16, eco: u16) {
    let Some(s) = world.sites.roles.get(&site) else { return };
    let (p, left, pile) = (s.prio, s.boards as i32 - s.boards_used as i32, s.piles[0]);
    if s.boards == 0 || left == amount(world, pile) || !may_activate(world, site, eco) || p.boards {
        return;
    }
    if !p.stone && !p.builders && !p.diggers {
        eco_prio(world, eco, |e| e.sites_with_prio = e.sites_with_prio.wrapping_add(1));
    }
    set_prio(world, site, |p| p.boards = true);
    eco_prio(world, eco, |e| e.prio_boards = e.prio_boards.wrapping_add(1));
}

/// `CheckActivateUrgentStones` (0x5432d0), as the boards.
fn activate_stone(world: &mut World, site: u16, eco: u16) {
    let Some(s) = world.sites.roles.get(&site) else { return };
    let (p, left, pile) = (s.prio, s.stones as i32 - s.stones_used as i32, s.piles[1]);
    if s.stones == 0 || left == amount(world, pile) || !may_activate(world, site, eco) {
        return;
    }
    if !p.boards {
        if p.stone {
            return;
        }
        if !p.builders && !p.diggers {
            eco_prio(world, eco, |e| e.sites_with_prio = e.sites_with_prio.wrapping_add(1));
        }
    }
    if !p.stone {
        set_prio(world, site, |p| p.stone = true);
        eco_prio(world, eco, |e| e.prio_stone = e.prio_stone.wrapping_add(1));
    }
}

/// `CheckActivateUrgentDigger` (0x543400): leveling left (+0x188 > 0) and fewer diggers at work
/// than wanted; the eco-sector's prioritised diggers grow by the diggers wanted (+0x184).
fn activate_diggers(world: &mut World, site: u16, eco: u16) {
    let Some(s) = world.sites.roles.get(&site) else { return };
    let (p, wanted) = (s.prio, s.diggers);
    if s.dig_effort <= 0 || wanted <= s.diggers_working as i32 || !may_activate(world, site, eco) {
        return;
    }
    if !p.boards && !p.stone && !p.builders {
        if p.diggers {
            return;
        }
        eco_prio(world, eco, |e| e.sites_with_prio = e.sites_with_prio.wrapping_add(1));
    }
    if !p.diggers {
        set_prio(world, site, |p| p.diggers = true);
        eco_prio(world, eco, |e| {
            e.digger_prio_sites = e.digger_prio_sites.wrapping_add(1);
            e.needed_prio_diggers = e.needed_prio_diggers.wrapping_add(wanted as i16);
        });
    }
}

/// `CheckActivateUrgentBuilder` (0x543380): always (given room and a working building); the
/// eco-sector's prioritised builders grow by the building's `builderNumber` (info +7).
fn activate_builders(world: &mut World, site: u16, eco: u16) {
    let p = prio(world, site);
    if !world.sites.roles.contains_key(&site) || !may_activate(world, site, eco) {
        return;
    }
    if !p.boards && !p.stone {
        if p.builders {
            return;
        }
        if !p.diggers {
            eco_prio(world, eco, |e| e.sites_with_prio = e.sites_with_prio.wrapping_add(1));
        }
    }
    if !p.builders {
        let n = wanted_builders(world, site) as i16;
        set_prio(world, site, |p| p.builders = true);
        eco_prio(world, eco, |e| {
            e.builder_prio_sites = e.builder_prio_sites.wrapping_add(1);
            e.needed_prio_builders = e.needed_prio_builders.wrapping_add(n);
        });
    }
}

/// The tail of every `CheckDeactivateUrgent*`: with no priority left the site is no longer
/// prioritised (+0x1b8 = 0, eco +0x14 - 1).
fn deactivate_tail(world: &mut World, site: u16, eco: u16) {
    let p = prio(world, site);
    if !p.boards && !p.stone && !p.builders && !p.diggers {
        set_prio(world, site, |p| p.on = false);
        eco_prio(world, eco, |e| e.sites_with_prio = e.sites_with_prio.wrapping_sub(1));
    }
}

/// `CheckDeactivateUrgentBoards` (0x5434e0).
pub fn deactivate_boards(world: &mut World, site: u16, eco: u16) {
    if !prio(world, site).boards {
        return;
    }
    set_prio(world, site, |p| p.boards = false);
    eco_prio(world, eco, |e| e.prio_boards = e.prio_boards.wrapping_sub(1));
    deactivate_tail(world, site, eco);
}

/// `CheckDeactivateUrgentStones` (0x543540).
pub fn deactivate_stone(world: &mut World, site: u16, eco: u16) {
    if !prio(world, site).stone {
        return;
    }
    set_prio(world, site, |p| p.stone = false);
    eco_prio(world, eco, |e| e.prio_stone = e.prio_stone.wrapping_sub(1));
    deactivate_tail(world, site, eco);
}

/// `CheckDeactivateUrgentDigger` (0x543610; also from `DiggingIsReady` 0x543190).
pub fn deactivate_diggers(world: &mut World, site: u16, eco: u16) {
    let Some(s) = world.sites.roles.get(&site) else { return };
    if !s.prio.diggers {
        return;
    }
    let wanted = s.diggers as i16;
    set_prio(world, site, |p| p.diggers = false);
    eco_prio(world, eco, |e| {
        e.digger_prio_sites = e.digger_prio_sites.wrapping_sub(1);
        e.needed_prio_diggers = e.needed_prio_diggers.wrapping_sub(wanted);
    });
    deactivate_tail(world, site, eco);
}

/// `CheckDeactivateUrgentBuilder` (0x5435a0).
pub fn deactivate_builders(world: &mut World, site: u16, eco: u16) {
    if !prio(world, site).builders {
        return;
    }
    let n = wanted_builders(world, site) as i16;
    set_prio(world, site, |p| p.builders = false);
    eco_prio(world, eco, |e| {
        e.builder_prio_sites = e.builder_prio_sites.wrapping_sub(1);
        e.needed_prio_builders = e.needed_prio_builders.wrapping_sub(n);
    });
    deactivate_tail(world, site, eco);
}

/// Every priority off (0x543120, from the site's Switch and crush; the same four calls open
/// `BuildingDone` 0x5429f0).
pub fn deactivate_all(world: &mut World, site: u16, eco: u16) {
    deactivate_boards(world, site, eco);
    deactivate_stone(world, site, eco);
    deactivate_diggers(world, site, eco);
    deactivate_builders(world, site, eco);
}

/// [`deactivate_all`] at the site's eco-sector (read at the ensign, +0x48).
pub fn deactivate_all_here(world: &mut World, site: u16) {
    let eco = eco(world, site);
    if eco != 0 {
        deactivate_all(world, site, eco);
    }
}

/// [`deactivate_diggers`] at the site's eco-sector (`DiggingIsReady` 0x543190).
pub fn deactivate_diggers_here(world: &mut World, site: u16) {
    let eco = eco(world, site);
    if eco != 0 {
        deactivate_diggers(world, site, eco);
    }
}

/// `CPile` vtbl+0x28 (0x5700e0) is the amount (+0x41); +0x43 the goods on their way.
fn amount(world: &World, pile: u16) -> i32 {
    crate::pile::get(world, pile).map_or(0, |p| p.amount as i32)
}

fn coming(world: &World, pile: u16) -> i32 {
    crate::pile::get(world, pile).map_or(0, |p| p.coming as i32)
}

/// `CBuildingSiteRole::OrderDigger` (0x5425f0): while there is leveling to do, the site may
/// call a digger if it has digger priority or the prioritised sites' diggers (eco +0x18) are
/// fewer than the eco-sector's diggers (+0x28, settler type 2). Not while the diggers at work
/// have fewer than 10 effort each; one digger per call, up to the diggers wanted.
pub fn order_digger(world: &mut World, site: u16) {
    let Some(s) = world.sites.roles.get(&site) else { return };
    let (effort, wanted, working) = (s.dig_effort, s.diggers, s.diggers_working);
    let eco = eco(world, site);
    if effort <= 0 || eco == 0 {
        return;
    }
    let Some(e) = world.economy.get(eco) else { return };
    if site_priority(world, site).diggers.is_none() && (e.needed_prio_diggers as i32) >= e.settlers[2] as i32 {
        return;
    }
    if working != 0 && effort / working as i32 <= 9 {
        return;
    }
    if wanted - working as i32 > 0 && crate::eco::call_worker(world, eco, site, 2) != 0
        && let Some(s) = world.sites.roles.get_mut(&site)
    {
        s.diggers_working += 1;
    }
}

/// `CBuildingSiteRole::OrderBuilder` (0x5426b0): once a material pile has a good not reserved
/// (0x542ee0: amount +0x41 above leaving +0x42), the site may call a builder if it has builder
/// priority or the prioritised sites' builders (eco +0x1a) are fewer than the eco-sector's
/// builders (+0x2a, type 3), and it is past state 3 or the leveling is nearly done (effort < 40
/// with diggers at work); one builder per call, up to the info's `builderNumber`.
pub fn order_builder(world: &mut World, site: u16) {
    let Some(s) = world.sites.roles.get(&site) else { return };
    let material = s.piles.iter().any(|&p| crate::pile::get(world, p).is_some_and(|d| d.amount > d.leaving));
    let (state, effort, diggers, working) = (s.state, s.dig_effort, s.diggers_working, s.builders_working);
    let eco = eco(world, site);
    if !material || eco == 0 {
        return;
    }
    let Some(e) = world.economy.get(eco) else { return };
    if site_priority(world, site).builders.is_none() && (e.needed_prio_builders as i32) >= e.settlers[3] as i32 {
        return;
    }
    if state <= 3 && !(effort < 40 && diggers != 0) {
        return;
    }
    if wanted_builders(world, site) - working as i32 > 0 && crate::eco::call_worker(world, eco, site, 3) != 0
        && let Some(s) = world.sites.roles.get_mut(&site)
    {
        s.builders_working += 1;
    }
}

/// +0x190, set by `Init` (0x541a50) and `PostLoadInit` (0x5419c0) from info +7.
fn wanted_builders(world: &World, site: u16) -> i32 {
    let Some(e) = world.entities.by_index(site as u32) else { return 0 };
    let crate::entity::EntityData::Building(d) = &e.data else { return 0 };
    world.rules.build.info(e.race, d.type_ex).map_or(0, |i| i.builders.len() as i32)
}

/// `CBuildingSiteRole::OrderMaterial` (0x5427f0): once digging has diggers or is over (state
/// above 2), each material pile asks the eco-sector for one more good (`RequestGood`, 0x4d5b40)
/// while its goods plus those on their way are fewer than still needed and fewer than 8; boards
/// and stones not while other sites have that priority (eco +0x1c / +0x1d) and this one has not.
pub fn order_material(world: &mut World, site: u16) {
    let eco = eco(world, site);
    if eco == 0 {
        return;
    }
    // A prioritised site keeps its board / stone priority while goods are still missing.
    if let Some(s) = world.sites.roles.get(&site).cloned()
        && s.prio.on
    {
        if s.boards == 0 || s.boards as i32 - s.boards_used as i32 == amount(world, s.piles[0]) {
            deactivate_boards(world, site, eco);
        } else {
            activate_boards(world, site, eco);
        }
        if s.stones == 0 || s.stones as i32 - s.stones_used as i32 == amount(world, s.piles[1]) {
            deactivate_stone(world, site, eco);
        } else {
            activate_stone(world, site, eco);
        }
    }
    let prio = site_priority(world, site);
    let Some(s) = world.sites.roles.get(&site).cloned() else { return };
    if s.diggers_working == 0 && s.state <= 2 {
        return;
    }
    let Some(e) = world.economy.get(eco) else { return };
    let (prio_boards, prio_stone) = (e.prio_boards, e.prio_stone);
    let need = [
        (s.boards as i32 - s.boards_used as i32, prio_boards == 0 || prio.boards, BOARDS),
        (s.stones as i32 - s.stones_used as i32, prio_stone == 0 || prio.stone, STONES),
        (s.gold as i32 - s.gold_used as i32, true, GOLD),
    ];
    for (&pile, (left, allowed, good)) in s.piles.iter().zip(need) {
        if pile == 0 {
            continue;
        }
        let have = coming(world, pile) + amount(world, pile);
        if have < left && have < 8 && allowed {
            crate::eco::request_good(world, eco, site, good);
        }
    }
}

/// `CBuildingSiteRole::GetPileIdWithGood(good)` (0x542fe0): the boards, stones or gold pile.
pub fn pile_with_good(site: &Site, good: u8) -> u16 {
    match good {
        BOARDS => site.piles[0],
        STONES => site.piles[1],
        GOLD => site.piles[2],
        _ => 0,
    }
}

/// `CBuildingSiteRole::GetBuildingNeed(good)` (0x542f40): goods still needed and not on the pile
/// (0 for a material the building does not cost).
pub fn building_need(world: &World, site: &Site, good: u8) -> i32 {
    let (need, used, k) = match good {
        BOARDS => (site.boards, site.boards_used, 0),
        STONES => (site.stones, site.stones_used, 1),
        GOLD => (site.gold, site.gold_used, 2),
        _ => return 0,
    };
    if need == 0 {
        return 0;
    }
    need as i32 - used as i32 - amount(world, site.piles[k])
}
