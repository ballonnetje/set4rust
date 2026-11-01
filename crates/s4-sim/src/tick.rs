//! One simulation tick in the original order: `CGameRun::Run` (release 0x4fe060, called by the
//! game loop 0x513c80) and `CLogic::LogicUpdate` (release 0x4f2720). UI-only work (selection, camera, cursor, sound, soundtrack)
//! is left out; every simulation step has a function, ported or `TODO(port)` naming its original.
//!
//! Rate: one tick per 71 ms at game speed 1 (`CStateGame` frame pacing), so 840 ticks ≈ 1 minute.

use crate::command::{Command, PlayerCommand};
use crate::entity::{EntityData, ObjType, flag};
use crate::grid::Y16X16;
use crate::jobs::state::EntityEvent;
use crate::world::World;

/// Advance the world by one tick, running `commands` (already scheduled for this tick, in the
/// order the network layer delivers them) at its start.
pub fn tick(world: &mut World, commands: &[PlayerCommand]) {
    tick_with_script(world, commands, &mut NoScript);
}

/// The map script's slot in the tick: `CGameScriptManager::Update(tick)`, called by
/// `CGameRun::Run` after the tiling update and before `CAnimalMgr::Update` (step 4). The client
/// implements this with its Lua engine; it gets `&mut World` so script effects land in the
/// original order relative to the other systems.
pub trait ScriptDriver {
    fn update(&mut self, world: &mut World, tick: u32);
}

/// No script (tests, maps without one).
pub struct NoScript;

impl ScriptDriver for NoScript {
    fn update(&mut self, _world: &mut World, _tick: u32) {}
}

/// [`tick`] with the map script run at its original position.
pub fn tick_with_script(world: &mut World, commands: &[PlayerCommand], script: &mut dyn ScriptDriver) {
    // The monitor's timings ([`crate::debug`]), beside the tick: the same work in the same order.
    if crate::debug::enabled() {
        let mut t = crate::debug::TickTimer::new(world.tick.wrapping_add(1));
        tick_probed(world, commands, script, &mut t);
        t.finish();
    } else {
        tick_probed(world, commands, script, &mut NoProbe);
    }
}

/// What a tick works on next, for profiling ([`tick_probed`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Probe {
    /// A step of the tick (the names of the functions below).
    Phase(&'static str),
    /// One entity's logic update: its object type, kind and id.
    Entity(ObjType, u16, u16),
    /// A construction site's logic update (building kind, id).
    Site(u16, u16),
}

/// Told where a tick goes next ([`tick_probed`]). The simulation never reads a clock; a
/// profiler charges the time between two calls to the earlier one.
pub trait TickProbe {
    fn at(&mut self, what: Probe);
}

/// No profiling.
pub struct NoProbe;

impl TickProbe for NoProbe {
    #[inline(always)]
    fn at(&mut self, _what: Probe) {}
}

/// [`tick_with_script`] that reports each step and each entity update to `probe` (the same work
/// in the same order; `examples/sim_soak.rs --profile`).
pub fn tick_probed<P: TickProbe>(world: &mut World, commands: &[PlayerCommand], script: &mut dyn ScriptDriver, probe: &mut P) {
    // `CStateGame` sets `m_uTickCounter = ValidTick` before calling `CGameRun::Run`.
    world.tick = world.tick.wrapping_add(1);
    logic_update(world, commands, probe);
    probe.at(Probe::Phase("ai_update"));
    ai_update(world);
    probe.at(Probe::Phase("tiling_update"));
    tiling_update(world);
    let t = world.tick;
    probe.at(Probe::Phase("script"));
    script.update(world, t);
    probe.at(Probe::Phase("animal_update"));
    animal_update(world);
    probe.at(Probe::Phase("eco_sector_update"));
    eco_sector_update(world);
    update_alliances(world);
    probe.at(Probe::Phase("statistic_update"));
    statistic_update(world);
    probe.at(Probe::Phase("end"));
}

/// `CLogic::LogicUpdate` (release 0x4f2720).
fn logic_update<P: TickProbe>(world: &mut World, commands: &[PlayerCommand], probe: &mut P) {
    probe.at(Probe::Phase("input_process"));
    input_process(world, commands);
    map_object_logic_update(world, probe);
    probe.at(Probe::Phase("logic_callbacks"));
    match world.tick & 0xF {
        14 => fill_extra_info(world),
        15 => {
            probe.at(Probe::Phase("init_transport"));
            init_transport(world);
            probe.at(Probe::Phase("recruit_specialist"));
            recruit_specialist(world);
            probe.at(Probe::Phase("recruit_worker"));
            recruit_worker(world);
        }
        _ => {}
    }
    if world.tick.is_multiple_of(840) || world.tick == 1 {
        trigger_statistic(world);
    }
    // 0x4f2d00: path decay, fish, signs, plants ([`crate::wear::world_step`]).
    probe.at(Probe::Phase("world_step"));
    crate::wear::world_step(world);
    probe.at(Probe::Phase("future_events"));
    future_events_execute(world);
    effects_execute(world);
}

/// `CInputProcessor::Process`: queued messages 5001..5052 go to `CNetInputProcessor::Process`.
/// TODO(check): `CInputProcessor::WorkOnBuildingSites`, called first, looks UI-side (placement
/// preview); confirm it does not change simulation state.
fn input_process(world: &mut World, commands: &[PlayerCommand]) {
    for c in commands {
        if let Some(cmd) = c.command.canonical() {
            execute(world, c.player, &cmd);
        }
    }
    // The AI's orders of the last tick ([`crate::ai::send`]).
    for o in std::mem::take(&mut world.ai.orders) {
        let m = crate::command::NetMessage { kind: o.kind, player: o.player, wparam: o.wparam, lparam: o.lparam, data: Vec::new() };
        let c = PlayerCommand::from_message(&m);
        if let Some(cmd) = c.command.canonical() {
            execute(world, c.player, &cmd);
        }
    }
}

/// `CNetInputProcessor::GetAliveBuildingPtrOfPlayer`: a building, not dead (`DIED` clear),
/// owned by `player`.
fn alive_building_of(world: &World, id: u32, player: u8) -> Option<u16> {
    let e = world.entities.by_index(id)?;
    (e.obj_type == ObjType::Building && e.flags & flag::DIED == 0 && e.owner == player).then_some(id as u16)
}

/// `CNetInputProcessor::Process` (release 0x4f6240).
/// `CNetInputProcessor::GetEcoSectorPtrOfPlayer(eco, player)`.
fn own_eco(world: &World, eco: u16, player: u8) -> bool {
    world.economy.get(eco).is_some_and(|e| e.owner == player)
}

fn execute(world: &mut World, player: u8, cmd: &Command) {
    use Command::*;
    match cmd {
        SetWorkingArea { building, x, y } => {
            if let Some(b) = alive_building_of(world, *building, player)
                && let Some(EntityData::Building(d)) = world.entities.by_index_mut(b as u32).map(|e| &mut e.data)
            {
                // 5001 (release 0x4f62f5): building +0x4c = the new centre, then the role's
                // +0x14 / +0x18 (the search cursors) and +0x1c are cleared; the local player's
                // UI event 0x193 is UI only.
                d.working_area = Y16X16::pack(*x as i32, *y as i32);
                crate::buildings::reset_search(world, b);
            }
        }
        CrushBuilding { building } => {
            // CNetInputProcessor 5002 (release 0x4f6342): the player's live building.
            if let Some(b) = alive_building_of(world, *building, player) {
                crate::buildings::crush::crush_building(world, b);
            }
        }
        SwitchBuilding { building } => {
            // CNetInputProcessor 5003 (release 0x4f6240): the player's live building, role vtbl+32.
            if let Some(b) = alive_building_of(world, *building, player) {
                crate::buildings::switch(world, b);
            }
        }
        PrioritizeBuildingSite { building } => {
            // CNetInputProcessor 5004 (release 0x4f6240): the player's live building, still a
            // site (BIRTH), its CBuildingSiteRole vtbl+36.
            if let Some(b) = alive_building_of(world, *building, player)
                && world.entities.by_index(b as u32).is_some_and(|e| e.flags & flag::BIRTH != 0)
                && world.sites.roles.contains_key(&b)
            {
                crate::build::order::prioritize(world, b);
            }
        }
        SetTradeTarget { building, target } => {
            // CNetInputProcessor 5005 (release 0x4f6240 case 0x138d): the player's live building
            // and a building not DIED, both trading roles → SetTradeTarget (0x552a90).
            if let Some(b) = alive_building_of(world, *building, player)
                && let Ok(t) = u16::try_from(*target)
                && crate::buildings::trading::can_target(world, b, t)
            {
                crate::buildings::trading::set_trade_target(world, b, t);
            }
        }
        TransportGood { building, good, amount } => {
            // 5006 (case 0x138e): the player's live trading building → TransportGood (0x552f60).
            if let Some(b) = alive_building_of(world, *building as u32, player)
                && crate::buildings::trading::state(world, b).is_some()
            {
                crate::buildings::trading::transport_good(world, b, *good, *amount as i32);
            }
        }
        TradeWith { building, value } => {
            // 5007 (case 0x138f): the player's live building with a trading role → 0x5530a0.
            if let Some(b) = alive_building_of(world, *building, player)
                && crate::buildings::trading::state(world, b).is_some()
            {
                crate::buildings::trading::trade_with(world, b, *value);
            }
        }
        TradeGood { building, value } => {
            // 5008 (case 0x1390): the player's live building with a trading role → 0x5530f0.
            if let Some(b) = alive_building_of(world, *building, player)
                && crate::buildings::trading::state(world, b).is_some()
            {
                crate::buildings::trading::trade_good(world, b, *value);
            }
        }
        // 5009: vtbl+0x98 of a READY vehicle (0x4f6150) — empty for carts and catapults
        // (0x55a940) and the other ships (0x590350); a ferry unloads (0x58d8e0).
        VehicleUnload { vehicle } => {
            if let Ok(v) = u16::try_from(*vehicle)
                && world.entities.by_index(v as u32).is_some_and(|e| e.flags & flag::READY != 0)
                && crate::vehicles::ferry::is_ferry(world, v)
            {
                crate::vehicles::ferry::unload(world, v);
            }
        }
        // 5010: a READY catapult without a transporter below `MaxAmmo` (0x4f6542 → 0x560d80).
        CatapultLoad { vehicle } => {
            if let Ok(v) = u16::try_from(*vehicle) {
                crate::vehicles::ammo::catapult_load(world, v);
            }
        }
        // 0x4f6190: a READY cart; then 0x55c130 / 0x55c360 / 0x55c020.
        CreateSettlement { cart } => {
            if let Some(c) = crate::vehicles::cart::command_target(world, *cart) {
                crate::vehicles::cart::create_settlement(world, c);
            }
        }
        CreateFoundationCart { cart } => {
            if let Some(c) = crate::vehicles::cart::command_target(world, *cart) {
                crate::vehicles::cart::create_foundation_cart(world, c);
            }
        }
        CreateNormalCart { cart } => {
            if let Some(c) = crate::vehicles::cart::command_target(world, *cart) {
                crate::vehicles::cart::create_normal_cart(world, c);
            }
        }
        SendTo { x, y, mode, entities } => crate::group::send_to(world, entities, *x as i32, *y as i32, *mode),
        // 5021 (0x4f662c): each READY settler (0x4f60e0) of the player, without flag 0x8000, of
        // warrior type 7 (a specialist), standing in an eco-sector of the player, becomes a
        // carrier (`CSettler::ChangeType(1, …, 0)`, 0x580ce0: wait list, no offer).
        MorphToCarrier { entities } => {
            for &id in entities {
                let Some(e) = world.entities.by_index(id as u32) else { continue };
                if e.obj_type != ObjType::Settler || e.flags & flag::READY == 0 || e.owner != player || e.flags & 0x8000 != 0 || e.flags & 0xf != 7 {
                    continue;
                }
                let Some(n) = world.grid.checked_index(e.x(), e.y()) else { continue };
                let eco = crate::eco::eco_of(world, n);
                if eco != 0 && world.economy.owner(eco) == player {
                    crate::jobs::core::change_type(world, id, 1, true, false);
                }
            }
        }
        Build { building, x, y } => crate::build::command_build(world, player, *building, *x, *y),
        // GetEcoSectorPtrOfPlayer: the eco-sector must exist and belong to the player.
        SetUnitReservation { eco_sector, kind, value } if own_eco(world, *eco_sector, player) => {
            crate::eco::change_min_max_values(world, *eco_sector, *kind, *value as i32)
        }
        SetBuildingSupplyPriority { eco_sector, good, building, priority } if own_eco(world, *eco_sector, player) => {
            crate::eco::change_building_supply_priority(world, *eco_sector, *good, *building, *priority as u8 as i8)
        }
        SetTransportPriority { eco_sector, good, delta } if own_eco(world, *eco_sector as u16, player) => {
            crate::eco::change_transport_priority(world, *eco_sector as u16, *good, *delta as i32)
        }
        SetUnitReservation { .. } | SetBuildingSupplyPriority { .. } | SetTransportPriority { .. } => {}
        CancelProduction { building } => {
            // 5030 (0x4f6240 case 0x13a6): the player's building, a CWorkshopBuildingRole only.
            if let Some(b) = alive_building_of(world, *building, player)
                && crate::buildings::workshop::state(world, b).is_some()
            {
                crate::buildings::workshop::cancel_production(world, b);
            }
        }
        FillTower { building, value } => {
            // 5031 (release 0x4f6240 case 0x13a7): the player's live building gets event
            // {0x12, 0, 0, 0, 0} (.rdata 0x1071010) when lparam is -1 (military: wishes = slots).
            if let Some(b) = alive_building_of(world, *building, player)
                && *value == u32::MAX
            {
                crate::buildings::set_event(world, b, EntityEvent { event: 0x12, kind: 0, data: [0; 3] });
            }
        }
        EmptyTower { building, kind, amount } => {
            // 5032 (case 0x13a8): event 0x13, type = the kind (0 swordsmen, else bowmen), data0 =
            // the amount (negative: everyone out but one).
            if let Some(b) = alive_building_of(world, *building as u32, player) {
                crate::buildings::set_event(world, b, EntityEvent { event: 0x13, kind: *kind as i32, data: [*amount as i32, 0, 0] });
            }
        }
        ProductionOrder { building, good, amount } => {
            // 5033 (release 0x4f687d): a READY production building of the player; toolsmith (21)
            // tool order, weaponsmith (22) weapon order, at the eco-sector of its ensign.
            if let Some(b) = alive_building_of(world, *building as u32, player)
                && world.entities.by_index(b as u32).is_some_and(|e| e.flags & flag::READY != 0)
                && crate::buildings::role(world, b).is_some_and(|r| r.base.house_kind == crate::buildings::house_kind::PRODUCTION)
            {
                let eco = crate::buildings::storage::eco_of_building(world, b);
                match world.entities.by_index(b as u32).map_or(0, |e| e.kind) {
                    21 => crate::eco::change_tool_order(world, eco, *good as u8, *amount as i32),
                    22 => crate::eco::change_weapon_order(world, eco, *good as u8, *amount as i32),
                    _ => {}
                }
            }
        }
        SetWeaponPercentage { building, percent } => {
            // 5034 (release 0x4f68f5): the percentages, and the made-counters start again.
            if let Some(b) = alive_building_of(world, *building, player) {
                let eco = crate::buildings::storage::eco_of_building(world, b);
                crate::eco::set_weapon_percentage(world, eco, *percent);
                if let Some(e) = world.economy.get_mut(eco) {
                    e.weapons_made = [0; 4];
                }
            }
        }
        SetWeaponAutoProduction { building, value } => {
            // 5035 (release 0x4f6947): auto-production on (100) unless `value` is 1.
            if let Some(b) = alive_building_of(world, *building, player) {
                let eco = crate::buildings::storage::eco_of_building(world, b);
                if let Some(e) = world.economy.get_mut(eco) {
                    e.weapon_auto = if *value != 1 { 100 } else { 0 };
                }
            }
        }
        SetSpecialistWish { eco_sector, specialist, value } if own_eco(world, *eco_sector, player) => {
            crate::eco::change_specialist_wish(world, *eco_sector, *specialist, *value as i32)
        }
        SetSpecialistWish { .. } => {}
        WorkshopOrder { building, item, amount } => {
            // 5040 (0x4f6240 case 0x13b0): TakeOrder(type, amount) on the player's workshop.
            if let Some(b) = alive_building_of(world, *building as u32, player)
                && crate::buildings::workshop::state(world, b).is_some()
            {
                crate::buildings::workshop::take_order(world, b, *item, *amount as i32);
            }
        }
        // 5041 (case 0x13b1): clear the group, then each READY own entity joins it.
        DefineGroup { group, entities } => crate::group::define(world, player, *group as usize, entities),
        StorageGoodSwitch { building, good, value } => {
            // CNetInputProcessor 5044: the player's live storage building.
            if let Some(b) = alive_building_of(world, *building as u32, player)
                && matches!(crate::buildings::role(world, b).map(|r| &r.kind), Some(crate::buildings::RoleKind::Storage(_)))
            {
                crate::buildings::storage::switch_good(world, b, *good as u8, *value != 0);
            }
        }
        SoldierOrder { building, kind, amount } => {
            // 5047 (case 0x13b7): on a Dark Tribe building the role cast to `CDarkTempleRole`
            // (0x4f6af8, RTTI 0x12241a0) takes the order (0x545d60, `dark_temple::take_order`);
            // for any other Dark Tribe building the cast fails and nothing happens. Otherwise the
            // soldier wish of the eco-sector at the building's ensign (0x4f6200 →
            // `CEcoSector::ChangeSoldierWish` 0x4d68a0).
            if let Some(b) = alive_building_of(world, *building as u32, player) {
                if world.entities.by_index(b as u32).is_some_and(|e| e.race == 3) {
                    if matches!(crate::buildings::role(world, b).map(|r| &r.kind), Some(crate::buildings::RoleKind::DarkTemple(_))) {
                        crate::buildings::dark_temple::take_order(world, b, *kind, *amount as i32);
                    }
                } else {
                    let eco = crate::buildings::storage::eco_of_building(world, b);
                    if world.economy.get(eco).is_some() {
                        crate::eco::change_soldier_wish(world, eco, *kind, *amount as i32);
                    }
                }
            }
        }
        CastSpell { priest, spell, x, y } => {
            // 5048 (case 0x13b8): a READY settler (0x4f60e0) of type 45 (priest) of the player
            // gets event 3 (kind = spell, data = player, 0, target), `magic` / `jobs::priest`.
            let ok = world.entities.by_index(*priest as u32).is_some_and(|e| {
                e.obj_type == crate::entity::ObjType::Settler && e.flags & crate::entity::flag::READY != 0 && e.kind == 45 && e.owner == player
            });
            if ok {
                let target = (*y as i32) << 16 | *x as i32;
                let ev = crate::jobs::state::EntityEvent { event: 3, kind: *spell as i32, data: [player as i32, 0, target] };
                crate::jobs::core::set_event(world, *priest, ev);
            }
        }
        // 5049 (0x4f6ba4): the player's vehicle, not dying: effect 0x3e (sound 3), deleted
        // (0x597100).
        DestroyVehicle { vehicle } => {
            let v = u16::try_from(*vehicle).unwrap_or(0);
            let ok = world.entities.by_index(v as u32).is_some_and(|e| {
                e.flags & crate::entity::flag::DIED == 0 && e.owner == player && matches!(e.obj_type, crate::entity::ObjType::Ship | crate::entity::ObjType::LandVehicle)
            });
            if ok {
                let p = world.entities.by_index(v as u32).map_or(crate::grid::Y16X16(0), |e| e.pos);
                world.events.push(crate::event::SimEvent::Effect { effect: 0x3e, sound: 3, x: p.x(), y: p.y() });
                crate::vehicles::delete(world, v);
            }
        }
        SetTradeStatus { building, player: p, value } => {
            // 5050 (case 0x13ba): the player's live trading building → 0x557250(p, value != 0).
            if let Some(b) = alive_building_of(world, *building as u32, player)
                && crate::buildings::trading::state(world, b).is_some()
            {
                crate::buildings::trading::set_trade_status(world, b, *p, *value != 0);
            }
        }
        HjbStart | HjbRequest { .. } => {}  // TODO(port): CHJBMgr vtable +12 / +16
        PlayerLeft { player, control } => crate::ai::player_left(world, *player, *control),
        Unsupported(_) | Other { .. } => {}
    }
}

/// `CMapObjectMgr::LogicUpdate` (release 0x56d280).
fn map_object_logic_update<P: TickProbe>(world: &mut World, probe: &mut P) {
    world.schedule.update_crc();
    let cur = world.schedule.cursor as usize;
    while let Some(id) = world.schedule.slots[cur].pop_front() {
        let Some(e) = world.entities.by_index_mut(id as u32) else {
            continue; // "EntityPtr for entity id %i in list %i is NULL!"
        };
        e.flags &= !flag::REGISTERED;
        let (obj, kind) = (e.obj_type, e.kind);
        probe.at(if obj == ObjType::Building && world.sites.roles.contains_key(&id) { Probe::Site(kind, id) } else { Probe::Entity(obj, kind, id) });
        entity_logic_update(world, id);
    }
    probe.at(Probe::Phase("destroy_killed"));
    while let Some(id) = world.schedule.killed[cur].pop_front() {
        // The building's role object goes with it (the destructor); the id may be reused by any
        // entity from now on. `~CBuilding` 0x535e40 (vtbl+8 0x535d60 from `Destroy` 0x56d200)
        // deletes the role at +0x50, a site's `CBuildingSiteRole` too.
        if world.entities.by_index(id as u32).is_some_and(|e| e.obj_type == ObjType::Building) {
            world.buildings.roles.remove(&id);
            world.sites.roles.remove(&id);
        }
        world.entities.destroy(id);
    }
    world.schedule.cursor = ((cur + 1) % crate::schedule::SLOTS) as u8;
}

/// `IAnimatedEntity::LogicUpdate`, dispatched on the entity's class.
fn entity_logic_update(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    match (e.obj_type, e.kind) {
        (ObjType::Settler, _) => crate::jobs::logic_update(world, id), // CSettler::LogicUpdate
        // CBuilding::LogicUpdate: the site role (build) or the building's role.
        (ObjType::Building, _) if world.sites.roles.contains_key(&id) => crate::build::building_logic_update(world, id),
        (ObjType::Building, _) => crate::buildings::logic_update(world, id),
        (ObjType::Pile, _) => crate::pile::logic_update(world, id), // CPile::LogicUpdate
        (ObjType::Ship | ObjType::LandVehicle, _) => crate::vehicles::logic::logic_update(world, id),
        (ObjType::Deco, _) => crate::deco::logic_update(world, id), // CTree, CPlant, CHive, CMushroom, CShadowHerb
        (ObjType::Flying, _) => crate::flying::logic_update(world, id), // CManakopter vtbl+0x64
        (ObjType::Animal, _) => crate::animals::logic_update(world, id), // CAnimal vtbl+0x64
        (ObjType::None, _) => {}
    }
}

/// `CLogic` callbacks at `tick & 15 == 14` (sidebar / dialog info, 0x4f2789) and
/// `CStatistic::FillExtraInfo` 0x4f8b30 for the extension bar: all GUI, filled by the client from
/// its snapshots here.
fn fill_extra_info(_world: &mut World) {}
/// `CEcoSectorMgr::InitTransport`, per eco-sector (0x4d5d30, release 0x4f27d6).
fn init_transport(world: &mut World) {
    crate::eco::for_each_sector(world, crate::eco::init_transport);
}
/// `CEcoSectorMgr::RecruiteSpecialist`.
fn recruit_specialist(world: &mut World) {
    crate::eco::for_each_sector(world, crate::eco::recruite_specialist);
}
/// `CEcoSectorMgr::RecruiteWorker`.
fn recruit_worker(world: &mut World) {
    crate::eco::for_each_sector(world, crate::eco::recruite_worker);
}
/// `CStatisticArchive::TriggerStatistic` (release 0x4f9330).
fn trigger_statistic(world: &mut World) {
    crate::statistic::trigger(world);
}
/// `CLogic::m_pFutureEvents` vtable +16 (`CFutureEvents::Execute`, release 0x4e4c00).
fn future_events_execute(world: &mut World) {
    crate::future::execute(world);
}
/// `CLogic::m_pEffects` vtable +0x60 (0x4e4230): expires a slice of the effect records. They are
/// the client's (0x4e3940 frees a record and writes no simulation state); nothing to do here.
fn effects_execute(_world: &mut World) {}
/// `g_pAI` vtable +24 (`CAIMain::Update` 0x4384d0, see [`crate::ai`]).
fn ai_update(world: &mut World) {
    crate::ai::update(world);
}
/// `g_pTiling` vtable +8, `CTiling::Update`: TODO(port) `LinksGarbageCollection(16)`; then
/// `g_pFogging->Update()` (`fog`, client-held, not simulation state).
fn tiling_update(world: &mut World) {
    if let Some(mut f) = world.fog.take() {
        f.update(world);
        world.fog = Some(f);
    }
}
/// `CAnimalMgr::Update` (0x52edf0): the land-animal spawning (its ambient effects are client
/// side, see [`crate::animals`]).
fn animal_update(world: &mut World) {
    crate::animals::update(world);
}
/// `CEcoSectorMgr::Update`: the strike check every 32 ticks (`eco::update`).
fn eco_sector_update(world: &mut World) {
    crate::eco::update(world);
}
/// TODO(port): `CMinimapHandler::UpdateAlliances` (check whether it is UI only).
fn update_alliances(_world: &mut World) {}
/// `CStatistic::Update` (release 0x4f88d0): land shares, mana copies, the fighting strengths
/// (0x4f8450) of every player, then the one-time start snapshot once `GlobalGameState +0x2e0 >
/// 0` (free settle's start, [`crate::world::GameSetup::start_winner`]) and not yet frozen.
fn statistic_update(world: &mut World) {
    // `Statistic::Update` (0x4f88d0): the land shares (0x4f8850) and mana copies (0x4f87f0), then
    // the strengths.
    crate::statistic::land_share(world);
    crate::statistic::copy_mana(world);
    for p in 1..9u8 {
        if world.player(p).is_some() {
            crate::statistic::recompute(world, p);
        }
    }
    if world.setup.start_winner > 0 {
        crate::statistic::freeze_start(world);
    }
}
