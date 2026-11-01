//! Calls the job code makes into systems that are not ported yet (buildings, piles, vehicles,
//! eco-sectors, war map, ground wear). Each one names the original it stands for and, for now,
//! only records a [`SimEvent::Unported`] so tests and tools can see what a job needed. Replace
//! the bodies as those systems land.

use crate::World;
use crate::event::SimEvent;

fn unported(world: &mut World, what: &'static str, entity: u16) {
    world.events.push(SimEvent::Unported { what, entity });
}

/// `IBuildingRole::TakeJobTrigger(trigger)` on the home building (from `InitCommonTaskValues`).
pub fn take_job_trigger(world: &mut World, building: u16, trigger: u8) {
    crate::buildings::take_job_trigger(world, building, trigger);
}

/// What `ISettlerRole::SetFree` (0x587050) does per observer target 1..4: the target (role
/// vtbl+0x54; the base role knows only 2, +0x14, the carrier also 1, 3, 4) is detached from
/// (`CPile::Detach`, vtbl+0x40) and set to 0 (vtbl+0x50).
pub fn detach_from_pile(world: &mut World, settler: u16, target: u8) {
    let pile = match target {
        1 => super::core::job(world, settler).map_or(0, |j| j.carrier.dest_pile),
        2 => super::core::job(world, settler).map_or(0, |j| j.source_pile),
        3 => super::core::job(world, settler).map_or(0, |j| j.carrier.source_pile2),
        _ => super::core::job(world, settler).map_or(0, |j| j.carrier.source_pile3),
    };
    if pile == 0 {
        return;
    }
    crate::pile::detach(world, pile, settler);
    if let Some(j) = super::core::job_mut(world, settler) {
        match target {
            1 => j.carrier.dest_pile = 0,
            2 => j.source_pile = 0,
            3 => j.carrier.source_pile2 = 0,
            _ => j.carrier.source_pile3 = 0,
        }
    }
}

/// `CBuilding::Attach(settler)` (vtbl+0x74, 0x5370e0): the building's observer list
/// (`CObserverList::Attach`, target 0), so the worker gets the building's events (7 / 8 stopped
/// / started, 9 deleted) and is released when it is deleted (0x56e130).
pub fn building_attach(world: &mut World, building: u16, settler: u16) {
    crate::buildings::attach(world, building, settler);
}

/// `home->Detach(settler)` (`ISettlerRole::SetFree`; for a building also `InhabitantFlee`).
pub fn home_detach(world: &mut World, home: u16, settler: u16) {
    match world.entities.by_index(home as u32).map(|e| e.obj_type) {
        Some(crate::entity::ObjType::LandVehicle | crate::entity::ObjType::Ship) => crate::vehicles::detach_settler(world, home, settler),
        // `CBuilding::Detach` (vtbl+0x40, 0x537100): off the observer list, home 0, role +0x7c.
        Some(crate::entity::ObjType::Building) => crate::buildings::military::detach_settler(world, home, settler),
        // `IFlyingEntity` 0x569390.
        Some(crate::entity::ObjType::Flying) => crate::flying::detach(world, home, settler),
        // `CPile::Detach` (0x56fbc0).
        Some(crate::entity::ObjType::Pile) => crate::pile::detach(world, home, settler),
        // `IEntity` 0x567130 (settlers, decos, animals; a stale home id): nothing.
        _ => {}
    }
}

/// `CFleeRole` for a free non-warrior on foreign land (`ISettlerRole::SetFree`).
pub fn flee_role(world: &mut World, settler: u16) {
    super::flee::start(world, settler);
}

/// `CEcoSector::SetSettlerOffer(type, settler)` in the settler's eco-sector.
pub fn eco_sector_offer(world: &mut World, settler: u16) {
    let Some(e) = world.entities.by_index(settler as u32) else { return };
    let (kind, pos) = (e.kind, e.pos);
    let Some(n) = world.grid.checked_index(pos.x(), pos.y()) else { return };
    let eco = crate::eco::eco_of(world, n);
    crate::eco::set_settler_offer(world, eco, kind, settler);
}

/// `CEcoSectorMgr::RemoveCarrierFromStrikeSettlers(owner, settler)`.
pub fn remove_carrier_from_strike(world: &mut World, settler: u16) {
    let owner = world.entities.by_index(settler as u32).map_or(0, |e| e.owner);
    crate::eco::remove_carrier_from_strike_settlers(world, owner, settler);
}

/// `CPileMgr::SearchSpaceForGoods(x, y, good, 1)`: drop a carried good on the ground.
pub fn drop_good(world: &mut World, settler: u16, good: u8) {
    let Some(pos) = world.entities.by_index(settler as u32).map(|e| e.pos) else { return };
    crate::pile::search_space_for_goods(world, pos.x(), pos.y(), good, 1);
}

/// `CCarrierRole::SetFree` (0x573cd6..0x573d41): eco-sector `eco` takes back the order of a
/// carrier that was to become `next_type`. Builder / digger: `ChangeMinMaxValues(type, 1)`
/// (0x4d5750 → `ChangeWorkerDeltaAmount`); pioneer, geologist, gardener: their specialist wish
/// (+0x288, index from table 0x1164214) + 1, at most 99 (written directly, not through
/// `ChangeSpecialistWish`). Thieves, saboteurs and other types: nothing.
pub fn cancel_next_type(world: &mut World, eco: u16, next_type: u8) {
    if world.economy.get(eco).is_none() {
        return;
    }
    match next_type as u16 {
        k @ (2 | 3) => crate::eco::change_min_max_values(world, eco, k, 1),
        k @ (47 | 49 | 50) => {
            let Some(idx) = crate::eco::SPECIALISTS.iter().position(|&t| t == k) else { return };
            if let Some(e) = world.economy.get_mut(eco) {
                e.specialist_wish[idx] = (e.specialist_wish[idx] as i32 + 1).min(99) as i8;
            }
        }
        _ => {}
    }
}

/// `CPile::ChangeAmountAndDetach(settler)`: take one good off (GET) or put one on (PUT).
pub fn pile_change_amount_and_detach(world: &mut World, pile: u16, settler: u16) {
    crate::pile::change_amount_and_detach(world, pile, settler);
}

/// Vehicle side of a carrier's LOAD_GOOD / TRANSPORT_GOOD_TO_VEHICLE / boarding.
pub fn vehicle(world: &mut World, what: &'static str, vehicle: u16) {
    unported(world, what, vehicle);
}

/// Role vtbl+0x3c `ESChanged` (release `CSettler::LogicUpdate` 0x5814f0 calls it once for entity
/// flag 0x80, which `NotifyChangeEcoSector` 0x4de900 sets on a non-warrior whose node lost its
/// eco-sector — land that changed hands). The base (0x587450, every role but these three) sets
/// the settler free (`SetFree(-1)`, vtbl+0x40): on foreign land that makes it flee
/// (`CFleeRole`); otherwise, on land of an eco-sector of its owner, the settler counts there
/// again (`+0x24 + 2·type`, at most 0xffff). The mushroom farmer (0x57e070), the fleeing settler
/// (0x577ee0) and the slave (0x5889e0) do nothing. Always false: `LogicUpdate` goes on.
pub fn es_changed(world: &mut World, settler: u16) -> bool {
    use super::state::role;
    let r = super::core::job(world, settler).map_or(role::NONE, |j| j.role);
    if matches!(r, role::MUSHROOM_FARMER | role::FLEE | role::SLAVE) {
        return false;
    }
    if super::roles::set_free(world, settler, -1) {
        return false;
    }
    let Some(e) = world.entities.by_index(settler as u32) else { return false };
    let (kind, owner, pos) = (e.kind, e.owner, e.pos);
    let Some(n) = world.grid.checked_index(pos.x(), pos.y()) else { return false };
    let eco = crate::eco::eco_of(world, n);
    if eco != 0 && world.economy.owner(eco) == owner {
        crate::eco::change_nr_of_settler(world, eco, kind, 1);
    }
    false
}

/// `CWorldManager::IncreaseWalkCount(node)` (0x60ca10): path wear ([`crate::wear`]).
pub fn increase_walk_count(world: &mut World, node: usize) {
    crate::wear::increase_walk_count(world, node);
}

/// `CBuilding::SettlerEnter(building, settler)` (house worker CHECKIN).
pub fn building_settler_enter(world: &mut World, building: u16, settler: u16) {
    if crate::buildings::role(world, building).is_some() {
        crate::buildings::settler_enter(world, building, settler);
    } else {
        unported(world, "CBuilding::SettlerEnter", building);
    }
}

/// `CProductionBuildingRole::GetProductType` (house worker of type 15 on PUT_GOOD_VIRTUAL).
pub fn production_product_type(world: &mut World, building: u16) -> u8 {
    crate::buildings::production::product_type(world, building)
}

/// Building role vtbl+56(good): the building's pile for `good`; then
/// `CPile::IncreaseUnforeseen(1)` (put, `delta` = 1) or `DecreaseUnforeseen(1)` (get, −1).
pub fn building_pile_unforeseen(world: &mut World, building: u16, good: u8, delta: i32) {
    let pile = crate::buildings::pile_with_good(world, building, good);
    if pile == 0 {
        let what = if delta > 0 { "CPile::IncreaseUnforeseen" } else { "CPile::DecreaseUnforeseen" };
        unported(world, what, building);
    } else if delta > 0 {
        crate::pile::increase_unforeseen(world, pile, delta);
    } else {
        crate::pile::decrease_unforeseen(world, pile, -delta);
    }
}

/// The house worker's PRODUCE_VIRTUAL into a vehicle ([`crate::vehicles::add_building_material`]).
pub fn vehicle_add_building_material(world: &mut World, vehicle: u16, good: u8) {
    crate::vehicles::add_building_material(world, vehicle, good);
}

/// `entity->vtbl+24(-1)` on the settler a type-20 house worker (the healer) serves, at the end
/// of WORK_VIRTUAL, if it is a settler: `CSettler` vtbl+24 (0x5817d0) with a negative amount
/// sets the health to the type's maximum (`SettlerInfo` health).
pub fn served_settler_done(world: &mut World, settler: u16) {
    let Some(e) = world.entities.by_index(settler as u32) else { return };
    let max = world.rules.settler_health(e.race, e.kind);
    if let Some(s) = super::core::settler_mut(world, settler) {
        s.health = max;
    }
}

/// `CWarMap::AddEntity(settler)` (role `Init` / `PostLoadInit`: out first if it is in).
pub fn war_map_add(world: &mut World, settler: u16) {
    crate::warmap::readd(world, settler);
}
