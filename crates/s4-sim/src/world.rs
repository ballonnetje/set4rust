//! The whole simulation state. Replaces the original's singletons (`CWorldManager`,
//! `CMapObjectMgr`, the per-kind managers, `CGameData::m_sRandom`, `CPlayerManager`).

use crate::create::DecoPools;
use crate::entity::{Entities, flag};
use crate::grid::{Grid, Layers};
use crate::hash::StateHasher;
use crate::rng::{ANIMAL_SEED, GAME_SEED, Random16};
use crate::rules::Rules;
use crate::schedule::Schedule;
use crate::tiling::Tiling;

/// `CPlayerInfo`, simulation-relevant fields only.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Player {
    /// 1..8.
    pub id: u8,
    /// 0..4 (`CPlayerInfo::Race`).
    pub race: u8,
    pub start_x: u32,
    pub start_y: u32,
}

/// Setup values of the game that systems read while it runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GameSetup {
    /// Control per player 0..=8 (0x149b604 + 0x3c·player; [`crate::NewGame::control`] for 1..=8):
    /// 0 open, 1 human, 2 / 3 a computer player.
    pub control: [u8; 9],
    /// `GlobalGameStateObject +0x58`: the game type. Only 3 is ever tested (reads at 0x47b54c,
    /// 0x48937a, 0x4f8605, 0x4fda70, 0x5144d1): it quarters the fighting strengths
    /// (`statistic::recompute`) and installs the victory hook 0x5c8580 (after about an hour, team
    /// 1 against the rest compared over 7 statistic categories). [inf] Its writer was not located
    /// (no `[GlobalGameStateObject] + 0x58` store); the setup passes it ([`crate::NewGame::game_type`]).
    pub game_type: u32,
    /// `GlobalGameStateObject +0x78..+0x7e`: game type 3's seven goods (copied from
    /// `GlobalGameState +0x2fc` by 0x460540; the single-player lobby 0x51c1a0 draws them for a new
    /// game, 0x51f5c0; a savegame keeps them in its header, 0x50015c / 0x4ffbab). Read by the
    /// goods race dialog's fill 0x4f8da0 and the victory hook 0x5c8580.
    #[cfg_attr(feature = "serde", serde(default))]
    pub race_goods: [u8; 7],
    /// `GlobalGameState +0x2e0`: the team the game is decided for from the start (with the
    /// decided byte +0x2dc; 0x460540 copies both to `GlobalGameStateObject +0x34 / +0x38`). The
    /// free game's start sets 1 in mode 5, free settle (map settings 0x520fa5, random map
    /// parameters 0x526b45); `Statistic::Update` 0x4f88d0 freezes the start snapshot once it is
    /// above 0. 0 otherwise.
    #[cfg_attr(feature = "serde", serde(default))]
    pub start_winner: i32,
    /// Bit `p`: player `p` is out of the game — the record byte 0x149b608 + 0x3c·p, set to 1 for
    /// the players of a game at its start (0x50a040), cleared when a player loses (0x513070,
    /// `EventManagement`'s loss message 0x8d1 at 0x517631). A player who leaves a network game
    /// gets an AI only while it is still in (0x51777c, [`crate::ai::player_left`]).
    #[cfg_attr(feature = "serde", serde(default))]
    pub out: u16,
}

/// Manager-wide counts that gate creation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Counters {
    /// `CSettlerMgr +24` (cap 0x7FFF in `AddSettler`).
    pub settlers: u16,
    /// `CBuildingMgr +24` (cap 4000 in `AddBuilding`).
    pub buildings: u32,
    /// `CPileMgr +24` (cap 20000 in `AddPile`).
    pub piles: u32,
}

#[derive(Clone, Debug)]
pub struct World {
    pub grid: Grid,
    pub layers: Layers,
    /// `g_pTiling`: tiles, sectors, eco-sectors (rebuilt by `tiling::init`).
    pub tiling: Tiling,
    pub entities: Entities,
    pub schedule: Schedule,
    /// `CGameData::m_sRandom`, the game RNG.
    pub rng: Random16,
    /// `CAnimalMgr`'s own RNG (+0x168). Only the ambient effects draw from it, and the ones near
    /// the screen centre (0x52ff10, 0x5301c0) draw a camera-dependent number of times, so in the
    /// original it is per-client state. The simulation never draws it (see [`crate::animals`]).
    pub animal_rng: Random16,
    /// Index = player id; slot 0 unused.
    pub players: Vec<Option<Player>>,
    /// `CStateGame::m_uTickCounter`: number of the tick being / last run (first tick is 1).
    pub tick: u32,
    /// `CDecoObjMgr`'s shared-instance tables.
    pub deco_pools: DecoPools,
    pub counters: Counters,
    pub rules: Rules,
    /// Construction sites and digging slots (`build` module).
    pub sites: crate::build::Sites,
    /// Building roles (`buildings`).
    pub buildings: crate::buildings::BuildingRoles,
    /// `CEcoSectorMgr`: the eco-sector records.
    pub economy: crate::eco::Economy,
    /// `CSettlerMgr`'s per player and type lists of warriors (`AttachSettler`).
    pub settler_lists: crate::settler_lists::SettlerLists,
    /// `CWarMap` (see [`crate::warmap`]).
    pub warmap: Box<crate::warmap::WarMap>,
    /// `CStatistic`: the fighting strengths (see [`crate::statistic`]).
    pub statistic: crate::statistic::Statistic,
    /// `CLogic::m_pFutureEvents` (see [`crate::future`]).
    pub future: crate::future::FutureEvents,
    /// `CGroupMgr` 0x153fd30: the players' unit groups (see [`crate::group`]).
    pub groups: crate::group::Groups,
    /// `CAlliances` (0x12dccf8): the teams, fixed when the game starts (see [`crate::alliances`]).
    pub alliances: crate::alliances::Alliances,
    /// The game setup the simulation reads later (fixed when the game starts).
    pub setup: GameSetup,
    /// `CLogic +0x28 / +0x2c`: the world step's cursor ([`crate::wear`]).
    pub world_step: crate::wear::WorldStep,
    /// `CAnimalMgr` (see [`crate::animals`]).
    pub animals: crate::animals::AnimalMgr,
    /// The players' mana (`CPlayerGameData`, see [`crate::magic`]).
    pub magic: crate::magic::Magic,
    /// `CAIMain` 0x146b148: the computer players (see [`crate::ai`]).
    pub ai: crate::ai::Ai,
    /// `CVehicleMgr`'s lists and counters (see [`crate::vehicles`]).
    pub vehicles: crate::vehicles::VehicleMgr,
    /// `CFlyingMgr`'s lists (see [`crate::flying`]).
    pub flying: crate::flying::FlyingMgr,
    /// `CBuildingMgr` 0x14f1924: set while `LoadBuildingData` (0x53d940) places the map's
    /// buildings; `FirstSettlerEnter` then only marks tower bits. Transient (load only).
    pub loading_buildings: bool,
    /// `CBuilding +0x25` of the building being killed by 0x536a80 (destroyed); 0 otherwise.
    /// Transient (set and reset around the kill).
    pub delete_cause: u8,
    /// The last mission of the Dark Tribe's or the Trojans' campaign (`GlobalGameState +0x2d4` 4
    /// or 15, +0x2d8 11): a destroyed building of type 0x33 stays and marks its owner lost
    /// ([`crate::buildings::crush::destroy`]). Set by the host from the map, also after a load.
    pub final_temple: bool,
    /// Notifications of the current tick for the client (not state; drained by the caller).
    pub events: Vec<crate::event::SimEvent>,
    /// Game events for the map script (not state; drained by the script driver).
    pub script_events: crate::event::ScriptEvents,
    /// `g_pFogging`: the fog of war of the player a client shows (see [`crate::fog`]). Not
    /// simulation state: never read by the simulation, not hashed; `None` without a client.
    pub fog: Option<Box<crate::fog::Fogging>>,
}

/// The parts of [`World::state_hash`] in its order, each feeding a hasher.
#[allow(clippy::type_complexity)]
pub(crate) const HASH_PARTS: [(&str, fn(&World, &mut StateHasher)); 24] = [
    ("header", |w, h| w.hash_header(h)),
    ("rng", |w, h| w.rng.hash(h)),
    ("animal_rng", |w, h| w.animal_rng.hash(h)),
    ("players", |w, h| w.hash_players(h)),
    ("layers", |w, h| w.layers.hash(h)),
    ("tiling", |w, h| w.tiling.hash(h)),
    ("entities", |w, h| w.entities.hash(h)),
    ("schedule", |w, h| w.schedule.hash(h)),
    ("deco_pools", |w, h| w.deco_pools.hash(h)),
    ("counters", |w, h| w.hash_counters(h)),
    ("sites", |w, h| w.sites.hash(h)),
    ("buildings", |w, h| w.buildings.hash(h)),
    ("economy", |w, h| w.economy.hash(h)),
    ("settler_lists", |w, h| w.settler_lists.hash(h)),
    ("warmap", |w, h| w.warmap.hash(h)),
    ("statistic", |w, h| w.statistic.hash(h)),
    ("future", |w, h| w.future.hash(h)),
    ("groups", |w, h| w.groups.hash(h)),
    ("alliances", |w, h| w.alliances.hash(h)),
    ("animals", |w, h| w.animals.hash(h)),
    ("magic", |w, h| w.magic.hash(h)),
    ("ai", |w, h| w.ai.hash(h)),
    ("vehicles", |w, h| w.vehicles.hash(h)),
    ("flying", |w, h| w.flying.hash(h)),
];

impl World {
    /// An empty world of the given size (what `CWorldManager::Construct` + fresh managers give).
    pub fn new(grid: Grid, rules: Rules) -> World {
        World {
            grid,
            layers: Layers::new(grid),
            tiling: Tiling::new(grid),
            entities: Entities::new(),
            schedule: Schedule::default(),
            rng: Random16::new(GAME_SEED, 0),
            animal_rng: Random16::new(ANIMAL_SEED, 0),
            players: vec![None; 9],
            tick: 0,
            deco_pools: DecoPools::default(),
            counters: Counters::default(),
            rules,
            sites: Default::default(),
            buildings: Default::default(),
            economy: Default::default(),
            settler_lists: Default::default(),
            warmap: Default::default(),
            statistic: Default::default(),
            future: Default::default(),
            groups: Default::default(),
            alliances: Default::default(),
            setup: Default::default(),
            world_step: Default::default(),
            animals: Default::default(),
            magic: Default::default(),
            ai: Default::default(),
            vehicles: Default::default(),
            flying: Default::default(),
            loading_buildings: false,
            delete_cause: 0,
            final_temple: false,
            events: Vec::new(),
            script_events: Default::default(),
            fog: None,
        }
    }

    pub fn player(&self, id: u8) -> Option<&Player> {
        self.players.get(id as usize)?.as_ref()
    }

    /// `PlayerCount` (0x1294828, `CPlayerManager`): players are numbered 1..=count, so this is
    /// the highest player id in the game [inf].
    pub fn player_count(&self) -> u8 {
        self.players.iter().rposition(|p| p.is_some()).unwrap_or(0) as u8
    }

    /// `CPlayerInfo::Race` of `player` (0 for an unknown player).
    pub fn race_of(&self, player: u8) -> u8 {
        self.player(player).map_or(0, |p| p.race)
    }

    /// `IAnimatedEntity::RegisterForLogicUpdate(delta)` → `CMapObjectMgr::RegisterForLogicUpdate`
    /// + `SetLastLogicUpdate(slot)`.
    pub fn register_for_logic_update(&mut self, index: u16, delta: i32) {
        let Some(e) = self.entities.by_index_mut(index as u32) else { return };
        debug_assert!(e.flags & flag::DIED == 0, "registering a dead entity");
        let slot = self.schedule.register(delta, index, e.last_slot);
        e.last_slot = Some(slot);
        e.flags |= flag::REGISTERED;
    }

    /// `IAnimatedEntity::UnRegisterFromLogicUpdate`.
    pub fn unregister_from_logic_update(&mut self, index: u16) {
        let Some(e) = self.entities.by_index_mut(index as u32) else { return };
        if let Some(slot) = e.last_slot.take()
            && self.schedule.unregister(slot, index)
        {
            e.flags &= !flag::REGISTERED;
        }
    }

    /// `CMapObjectMgr::Kill` (release 0x56d030), the parts ported so far: dead bit on the unique
    /// id, alive flags → `DIED` (keeping `BIRTH` as 0x100000), the war map entry
    /// (`CWarMap::RemoveEntity`), the groups when the group word is not 0
    /// (`CGroupMgr::DetachEntityFromAllGroups`, [`crate::group::remove_all`]), `entity->Delete()`
    /// (vtbl+0x10, class-specific removal from the map and the managers), destroy queued 31
    /// ticks ahead; before `Delete` the kill / loss statistics for `attacker`
    /// ([`crate::statistic::killed`]). TODO(port): the +0x10 object (0x566d80); deselecting is
    /// not simulation.
    pub fn kill(&mut self, index: u16, attacker: u8) {
        let Some(e) = self.entities.by_index_mut(index as u32) else { return };
        if e.is_dead() {
            return;
        }
        let obj_type = e.obj_type;
        e.uid |= crate::entity::UID_DEAD;
        if e.flags & flag::BIRTH != 0 {
            e.flags |= 0x10_0000;
        } else {
            e.flags &= !0x10_0000;
        }
        e.flags = (e.flags & !flag::ALIVE_MASK) | flag::DIED;
        if self.warmap.contains(index) {
            crate::warmap::remove(self, index);
        }
        if crate::group::word(self, index) != 0 {
            crate::group::remove_all(self, index);
        }
        // 0x56d119: the kill / loss statistics.
        crate::statistic::killed(self, index, attacker);
        // [inf] Its AI info goes with it (0x566d80): out of its AI task force.
        crate::ai::player::on_delete(self, index);
        // entity->Delete().
        if obj_type == crate::entity::ObjType::Pile {
            crate::pile::on_delete(self, index);
        }
        if obj_type == crate::entity::ObjType::Building {
            crate::buildings::on_delete(self, index);
        }
        if obj_type == crate::entity::ObjType::Settler {
            crate::create::settler_delete(self, index);
        }
        if obj_type == crate::entity::ObjType::Animal {
            crate::animals::on_delete(self, index);
        }
        if matches!(obj_type, crate::entity::ObjType::Ship | crate::entity::ObjType::LandVehicle) {
            crate::vehicles::on_delete(self, index);
        }
        if obj_type == crate::entity::ObjType::Flying {
            crate::flying::on_delete(self, index);
        }
        self.schedule.queue_destroy(index);
    }

    /// `ITiling::OwnerId(node)`.
    pub fn owner_of(&self, node: usize) -> u8 {
        self.tiling.owner_of(node)
    }

    fn tiling_ctx(&mut self) -> (&mut Tiling, crate::tiling::Ctx<'_>) {
        let l = &mut self.layers;
        let ctx = crate::tiling::Ctx { grid: self.grid, ground: &l.ground, flags: &mut l.flags, gfx_bits: &mut l.gfx_bits, map_object: &l.map_object, deco_object: &l.deco_object };
        (&mut self.tiling, ctx)
    }

    /// `CWorldManager::SetBlockedPosition` → `CTiling::SetBlockedLand`.
    pub fn set_blocked_land(&mut self, x: i32, y: i32) {
        let (t, mut ctx) = self.tiling_ctx();
        t.set_blocked_land(&mut ctx, x, y);
        self.apply_tiling_notes();
    }

    /// `IGfxEngine::UpdateWorldPosition(x, y)` (release 0x65a4b0): on the map, the node's shading
    /// is refreshed (`RefreshShading` 0x66d7b0, [`crate::grid::Layers::refresh_shading`]); the
    /// renderer's row bookkeeping that follows is the client's.
    pub fn update_world_position(&mut self, x: i32, y: i32) {
        if let Some(n) = self.grid.checked_index(x, y) {
            let size = self.grid.size() as usize;
            self.layers.refresh_shading(n, size);
        }
    }

    /// `CWorldManager::ClearBlockedPosition` → `CTiling::ClearBlockedLand`.
    pub fn clear_blocked_land(&mut self, x: i32, y: i32) {
        let (t, mut ctx) = self.tiling_ctx();
        t.clear_blocked_land(&mut ctx, x, y);
        self.apply_tiling_notes();
    }

    /// `CTiling::SetBlockedWater` (tiling vtbl+0x10, release 0x5b7580).
    pub fn set_blocked_water(&mut self, x: i32, y: i32) {
        let (t, mut ctx) = self.tiling_ctx();
        t.set_blocked_water(&mut ctx, x, y);
        self.apply_tiling_notes();
    }

    /// `CTiling::ClearBlockedWater` (tiling vtbl+0x18, release 0x5b7840).
    pub fn clear_blocked_water(&mut self, x: i32, y: i32) {
        let (t, mut ctx) = self.tiling_ctx();
        t.clear_blocked_water(&mut ctx, x, y);
        self.apply_tiling_notes();
    }

    /// `CWorldManager::SetOwnerId` → `CTiling::ChangeOwner`.
    pub fn change_owner(&mut self, x: i32, y: i32, owner: u8) {
        let (t, mut ctx) = self.tiling_ctx();
        t.change_owner(&mut ctx, x, y, owner);
        self.apply_tiling_notes();
    }

    /// The tiling's notifications of the last operation: the economy's, then the fog's
    /// (`UpdateOwnerFogging`; it only touches the fog layer, so the order is free).
    fn apply_tiling_notes(&mut self) {
        crate::eco::apply_tiling_notes(self);
        let notes = std::mem::take(&mut self.tiling.fog_notes);
        if let Some(f) = self.fog.as_mut() {
            for (x, y, owner) in notes {
                f.update_owner_fogging(x, y, owner);
            }
        }
    }

    /// Give the world the fog of war of `player` (`CFogging::Init` + `Start`), from the saved
    /// gfx bits when `chunk135` is given (`CFogging::Load`).
    pub fn start_fog(&mut self, player: u8, chunk135: Option<&[u8]>) {
        let mut f = crate::fog::Fogging::new(self, player);
        if let Some(c) = chunk135 {
            f.load(c);
        }
        f.start(self);
        self.fog = Some(Box::new(f));
    }

    /// `CTiling::Init` on the current layers with `owner` as the per-node start owner
    /// (`StartUpOwner`).
    pub fn init_tiling(&mut self, owner: &[u8]) {
        let l = &mut self.layers;
        self.tiling = crate::tiling::init(crate::tiling::InitLayers {
            grid: self.grid,
            ground: &l.ground,
            flags: &mut l.flags,
            gfx_bits: &mut l.gfx_bits,
            owner,
        });
        // A fresh CEcoSectorMgr gets the eco-sectors Init creates (NotifyCreateEcoSector).
        self.economy = Default::default();
        crate::eco::apply_tiling_notes(self);
    }

    /// 64-bit fingerprint of the complete mutable state (not [`Rules`], which is fixed per game).
    pub fn state_hash(&self) -> u64 {
        let mut h = StateHasher::default();
        for (_, part) in HASH_PARTS {
            part(self, &mut h);
        }
        h.finish()
    }

    /// One fingerprint per part of [`World::state_hash`], in its order (for desync reports:
    /// which subsystem diverged). Each part is hashed on its own, so they do not combine into
    /// `state_hash`.
    pub fn subsystem_hashes(&self) -> Vec<(&'static str, u64)> {
        self.hashes().1
    }

    /// [`World::state_hash`] and [`World::subsystem_hashes`] in one pass over the state (the
    /// hasher's second lane hashes each part on its own), for about the cost of one of them.
    pub fn hashes(&self) -> (u64, Vec<(&'static str, u64)>) {
        let mut h = StateHasher::default();
        let parts = HASH_PARTS
            .iter()
            .map(|(name, part)| {
                h.start_part();
                part(self, &mut h);
                (*name, h.part())
            })
            .collect();
        (h.finish(), parts)
    }

    fn hash_counters(&self, h: &mut StateHasher) {
        h.u16(self.counters.settlers);
        h.u32(self.counters.buildings);
        h.u32(self.counters.piles);
    }

    fn hash_header(&self, h: &mut StateHasher) {
        h.u32(self.grid.size());
        h.u32(self.tick);
        self.setup.control.iter().for_each(|&c| h.u8(c));
        h.u32(self.setup.game_type);
        // Only once a player is out, so a world from before the field (save version 29, all 0)
        // hashes as it did.
        if self.setup.out != 0 {
            h.u16(self.setup.out);
        }
        self.world_step.hash(h);
    }

    fn hash_players(&self, h: &mut StateHasher) {
        for p in &self.players {
            match p {
                Some(p) => {
                    h.u8(p.id);
                    h.u8(p.race);
                    h.u32(p.start_x);
                    h.u32(p.start_y);
                }
                None => h.u8(0),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `hashes` in one pass gives `state_hash` and, per part, what a fresh hasher over that part
    /// alone gives (roman02 after 300 ticks; skipped without the game data).
    #[test]
    fn hashes_in_one_pass_match_the_separate_ones() {
        let p = s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map");
        let (Ok(map), Ok(rules)) = (s4_formats::map::MapFile::open(&p), crate::Rules::load_default()) else { return };
        let (mut w, _) = crate::new_game(&map, rules, &crate::NewGame::default()).expect("new game");
        for _ in 0..300 {
            crate::tick(&mut w, &[]);
        }
        let (whole, parts) = w.hashes();
        let mut h = StateHasher::default();
        for (name, part) in HASH_PARTS {
            part(&w, &mut h);
            let mut own = StateHasher::default();
            part(&w, &mut own);
            assert_eq!(parts.iter().find(|p| p.0 == name).map(|p| p.1), Some(own.finish()), "{name}");
        }
        assert_eq!(whole, h.finish());
        assert_eq!(whole, w.state_hash());
    }
}
