//! Starting a game from a map: `CGameRun::LoadEditorMap` (release 0x4ff460) and the loaders it calls,
//! in the original order (which fixes entity ids and RNG draws):
//!
//! 1. `LoadGeneralEditorInfo` (chunk 1): map size.
//! 2. `CWorldManager::LoadGfxData(map, 13, size)` (release 0x60d690): node layers from chunks 13 + 6,
//!    `ITiling::Init`, then row by row the deco objects and resources.
//! 3. `CWarMap::Init`.
//! 4. `CBuildingMgr::LoadBuildingData` (chunk 8), with start-tower garrisons.
//! 5. `CSettlerMgr::LoadSettlerData` (chunk 7): settlers and vehicles.
//! 6. `CPileMgr::LoadPileData` (chunk 9).
//! 7. Script chunk 16 → `CGameScriptManager::NewGameEx`.
//!
//! Then `CGameRun` init (0x4fd900) creates the start resources (0x4f26c0, [`StartResources`]),
//! runs `CLogic::PostLoadMap`, `CEcoSectorMgr::CalculateInitialFreeBeds`
//! (ported, release 0x4ddec0), `OrderDiggerAndBuilderAtStartUp` (ported, the loop at 0x4fdbb1),
//! `CStatistic::UpdateStartStatistic`, `ActivateAIs`, `StartScript`, `CAnimalMgr::Init` — the
//! others TODO(port).
//!
//! Players come from chunk 2 here; in the game `CGameType` / the lobby decide who plays which
//! race (255 in the map = random), so [`NewGame::races`] can override them.

use s4_formats::map::MapFile;

use crate::create::{self, BuildingSkip, DecoSkip};
use crate::grid::{Grid, flag};
use crate::rng::{GAME_SEED, Random16};
use crate::rules::Rules;
use crate::world::{Player, World};

/// Game setup outside the map file.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct NewGame {
    /// Game RNG seed (`CGameData` default 290671).
    pub seed: u32,
    /// Race per player id 1..8 (index 0..7); `None` = the map's value, with 255 ("random")
    /// mapped to Roman.
    pub races: [Option<u8>; 8],
    /// Team per player id 1..8 (index 0..7) as the game setup gives it (`GlobalGameState +0x90`:
    /// 0..7, e.g. from a map's team setup, chunk 3); `None` = no team (each such player gets a
    /// team of its own). See [`crate::alliances`].
    #[cfg_attr(feature = "serde", serde(default))]
    pub teams: [Option<u8>; 8],
    /// Control per player id 1..8 (index 0..7) as the game setup gives it (`GlobalGameState
    /// +0x6c + 4k`, 0x149b640 + 0x3c·p, set by the lobby's `SetSettings` 0x514d30; the lobby's
    /// slot event 0x57, 0x51a250 at 0x51d7ca, steps it 0 → 1 → 2 → 3 → 0 / 2): 0 an open slot,
    /// 1 a human, 2 or 3 a computer player (level 1 or 2, 0x4ff5f0, see [`crate::ai`]). Start
    /// resources ([`StartResources`]) go to every slot but the open ones.
    #[cfg_attr(feature = "serde", serde(default))]
    pub control: [u8; 8],
    /// The game type (`GlobalGameStateObject +0x58`, see [`crate::world::GameSetup::game_type`]).
    #[cfg_attr(feature = "serde", serde(default))]
    pub game_type: u32,
    /// How many of the map's player slots play (the map settings' count, `GlobalGameState
    /// +0x338`); `None` = all of them. The game start makes one player per playing slot
    /// (`AddPlayer` 0x509f80: `PlayerCount` 0x1294828 + 1, the slot's record at 0x149b5d4 +
    /// 0x3c·p with the present flag +0x34), so slots above the count have no player: no record,
    /// no AI (0x4ff5f0 loops to `PlayerCount`), no statistics, no mana pool, no alliance.
    /// [inf] Their preplaced settlers, buildings, piles and land stay: none of the map loaders
    /// reads `PlayerCount` or the present flag (buildings 0x53d940, settlers 0x584b10, piles
    /// 0x570e60, land 0x60d690; `AddSettler` 0x582600 / `AddBuilding` 0x53c350 only read the
    /// slot's race, 0 without a record). 47 of the 53 multiplayer maps place nothing for any slot.
    #[cfg_attr(feature = "serde", serde(default))]
    pub players: Option<u8>,
    /// Game type 3's seven goods (`GlobalGameState +0x2fc..+0x302`, drawn by the lobby 0x51f5c0;
    /// see [`crate::world::GameSetup::race_goods`]).
    #[cfg_attr(feature = "serde", serde(default))]
    pub race_goods: [u8; 7],
    /// `GlobalGameState +0x2e0` ([`crate::world::GameSetup::start_winner`]): 1 for a free game in
    /// mode 5, else 0.
    #[cfg_attr(feature = "serde", serde(default))]
    pub start_winner: i32,
    /// The start resources the game start gives each player (the map settings' goods choice,
    /// `GlobalGameState +0x38`; the map header's `m_iStartResources`): 0 none (campaigns,
    /// tutorials, maps that place their own units), 1 / 2 / 3 few / medium / many
    /// (`CreateStartResourcesFew` / `Medium` / `Many` of `Script\Internal\StartResources.txt`).
    #[cfg_attr(feature = "serde", serde(default))]
    pub start_resources: u8,
    /// A campaign mission's campaign type (`GlobalGameState +0x3b9` set, a campaign map, with
    /// `+0x2d4` the type: 1..4 the main campaigns, 5..10 Mission CD, 11..16 Add-On, 17..20 Mission
    /// CD 2, 21..24 Mission CD 3); `None` for any other game. The map load reads it for the AI
    /// ([`crate::ai::Ai::b18`]).
    #[cfg_attr(feature = "serde", serde(default))]
    pub campaign: Option<u8>,
}

impl NewGame {
    /// The teams of team setup `index` of the map (chunk 3), as the game setup takes them
    /// (release 0x5069b0 at 0x507421..0x507469): `MA_GetPlayerData(player, index)` (0x68bcf0)
    /// gives the slot's team byte, a value below 0 or above 8 becomes 0 (so the setup's 255,
    /// "no fixed team", lands in team 0 until the lobby changes it), 8 is no team.
    /// [inf] A missing setup leaves every player alone (the release returns its unset output).
    pub fn map_teams(map: &MapFile, index: u16) -> s4_formats::Result<[Option<u8>; 8]> {
        let mut teams = [None; 8];
        if let Some(s) = map.team_setups()?.into_iter().find(|s| s.index == index) {
            for (t, slot) in teams.iter_mut().zip(&s.slots) {
                let v = if slot.team > 8 { 0 } else { slot.team };
                *t = (v != 8).then_some(v);
            }
        }
        Ok(teams)
    }
}

impl Default for NewGame {
    fn default() -> Self {
        NewGame { seed: GAME_SEED, races: [None; 8], teams: [None; 8], control: [0; 8], game_type: 0, players: None, race_goods: [0; 7], start_winner: 0, start_resources: 0, campaign: None }
    }
}

/// Record counts per chunk and what became of them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LoadReport {
    /// Nodes with a non-zero chunk-6 object.
    pub object_nodes: u32,
    /// Ids 162 / 209 / 210 / 211: not placed by `LoadGfxData`.
    pub objects_ignored: u32,
    /// Objects that did not get a deco entity, by reason.
    pub objects_skipped: Vec<(u16, DecoSkip)>,
    /// Deco entities created (shared instances count once).
    pub deco_entities: u32,

    pub building_records: u32,
    /// Type 0 records (skipped by the loader).
    pub buildings_type0: u32,
    pub buildings_skipped: Vec<(u8, BuildingSkip)>,
    pub buildings: u32,
    /// Soldiers requested by garrison counts of military buildings, and how many were placed
    /// (the rest failed `IncWishAndOrder`: tower full, or medics `j >= 6` whose warrior type 4
    /// the tower does not take).
    pub soldiers_requested: u32,
    /// Of those, garrison slots `j >= 6` (medics).
    pub medics_requested: u32,
    pub soldiers: u32,

    pub settler_records: u32,
    /// Types 67..=199: ignored by `LoadSettlerData`.
    pub settlers_ignored: u32,
    pub settlers: u32,
    pub vehicles: u32,
    /// Vehicle records `Create` has no class for (Dark Tribe warship/catapult, type > 5).
    pub vehicles_skipped: u32,

    pub pile_records: u32,
    pub piles: u32,
}

/// Build a [`World`] from a map. Errors only on unreadable chunks or an invalid size.
/// One `CreateStartResources` call of the game start (0x4f26c0, called from `CGameRunInitEntry`
/// at 0x4fdba7, after the map load 0x4ff460 and before `CalculateInitialFreeBeds` 0x4ddec0).
/// The release makes it only when `GlobalGameState +0x34` is 1 (`MA_GetMapData` 0x68be90: the
/// map has neither a settlers chunk 7 nor a buildings chunk 8, `MA_ReadFile` 0x68b390 cases 7 /
/// 8; a random map's start 0x5069b0 sets it too) and `+0x38` (the goods) is 1..=3, then for each
/// player slot `k < +0x68` whose control (+0x6c + 4k) is not 0: `0x5bd7b0(+0x38, x = +0xfc + 4k,
/// y = +0x120 + 4k, k + 1, control == 1 ? 0 : control - 1)`, which runs
/// `Script\Internal\StartResources.txt`'s `CreateStartResourcesFew` / `Medium` / `Many(x, y,
/// player, ai)` in the game's script engine. The sim has no script engine: [`new_game_with`]
/// hands each call to its caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StartResources {
    /// 1 / 2 / 3: few / medium / many.
    pub amount: u8,
    /// The player's start position (chunk 2).
    pub x: i32,
    pub y: i32,
    pub player: u8,
    /// 0 for a human, 1 / 2 for a computer of control 2 / 3 (the script's `_AI`: extra piles,
    /// pioneers and swordsmen).
    pub ai: i32,
}

/// [`new_game_with`] without start resources.
pub fn new_game(map: &MapFile, rules: Rules, setup: &NewGame) -> s4_formats::Result<(World, LoadReport)> {
    new_game_with(map, rules, setup, &mut |_, _| {})
}

/// A new game; `start` makes each [`StartResources`] call (in player order).
pub fn new_game_with(map: &MapFile, rules: Rules, setup: &NewGame, start: &mut dyn FnMut(&mut World, &StartResources)) -> s4_formats::Result<(World, LoadReport)> {
    crate::debug::load_step("world");
    let general = map.general()?;
    let Some(grid) = Grid::new(general.size) else {
        return Err(s4_formats::Error::Format(format!("map size {} out of range", general.size)));
    };
    let mut w = World::new(grid, rules);
    w.rng = Random16::new(setup.seed, 0);
    let mut report = LoadReport::default();

    for p in map.players()? {
        if setup.players.is_some_and(|n| p.id > n) {
            continue;
        }
        let race = setup.races.get(p.id as usize - 1).copied().flatten().unwrap_or(match p.race {
            r @ 0..=4 => r as u8,
            _ => 0,
        });
        if let Some(slot) = w.players.get_mut(p.id as usize) {
            *slot = Some(Player { id: p.id, race, start_x: p.start_x, start_y: p.start_y });
        }
    }

    // CAlliances (0x4fe510 → 0x5ab700, game init 0x4fdb06): the teams of the players in the game.
    let present = std::array::from_fn(|i| w.players.get(i + 1).is_some_and(|p| p.is_some()));
    w.alliances = crate::alliances::Alliances::new(present, setup.teams);
    w.setup.game_type = setup.game_type;
    w.setup.race_goods = setup.race_goods;
    w.setup.start_winner = setup.start_winner;
    for (p, &c) in setup.control.iter().enumerate() {
        w.setup.control[p + 1] = c;
    }
    // CStatistic init (0x4f7ac0: cleared, fighting-strength bases 0x4f86b0); its position in the
    // game setup relative to the loaders is inferred (before the buildings count into it).
    crate::statistic::init(&mut w);
    // 0x4fdb42: every player's mana pool (0x4f5400).
    crate::magic::init(&mut w);
    crate::debug::load_step("landscape");
    load_gfx_data(&mut w, map, &mut report)?;
    // CWarMap::Init (0x5bb260): the empty map `World::new` made.
    crate::debug::load_step("buildings");
    load_building_data(&mut w, map, &mut report)?;
    crate::debug::load_step("settlers");
    load_settler_data(&mut w, map, &mut report)?;
    crate::debug::load_step("piles");
    load_pile_data(&mut w, map, &mut report)?;
    crate::debug::load_step("ai init");
    // Script chunk 16 (the client's). Then `CAIMain::Init` (vtbl+12, 0x438370, end of 0x4ff460):
    // the AI records and the land cells (0x43c100), which the animal manager reads.
    crate::ai::init(&mut w);
    // 0x4ff5ad (right after it): 0x1266b18 is 0 on a campaign map of a type below 11 (the main
    // campaigns and the Mission CD), else 0x10.
    w.ai.b18 = if setup.campaign.is_some_and(|c| c < 11) { 0 } else { 0x10 };
    // 0x4fdba7 → 0x4f26c0: the start resources.
    let places_units = map.container.chunks.iter().any(|c| matches!(c.kind, s4_formats::map::id::SETTLERS | s4_formats::map::id::BUILDINGS));
    if !places_units && (1..=3).contains(&setup.start_resources) {
        for k in 1..=8u8 {
            let Some(p) = w.players.get(k as usize).cloned().flatten() else { continue };
            // 0x4f26c0: `if (control != 0)`, `_AI` = `control == 1 ? 0 : control - 1`.
            let ai = match setup.control[k as usize - 1] {
                0 => continue,
                1 => 0,
                c => c as i32 - 1,
            };
            start(&mut w, &StartResources { amount: setup.start_resources, x: p.start_x as i32, y: p.start_y as i32, player: k, ai });
        }
    }
    // TODO(port): CLogic::PostLoadMap and the post-load steps listed above.
    crate::debug::load_step("beds + workers");
    crate::eco::calculate_initial_free_beds(&mut w);
    order_digger_and_builder_at_startup(&mut w);
    // CAnimalMgr::Init (0x52ddf0, called from CGameRunInitEntry at 0x4fdc36).
    crate::debug::load_step("animals");
    crate::animals::init(&mut w);
    crate::debug::load_step("ai players");
    // 0x4ff5f0 (CGameRunInitEntry, after the map load, before the script starts): the AI
    // players.
    let mut control = setup.control;
    for (i, c) in control.iter_mut().enumerate() {
        if w.players.get(i + 1).is_none_or(|p| p.is_none()) {
            *c = 0; // 0x4ff5f0 loops over the players only
        }
    }
    crate::ai::add_players_at_start(&mut w, &control);
    crate::debug::load_step("end");
    Ok((w, report))
}

/// `OrderDiggerAndBuilderAtStartUp`: the game init (release 0x4fdbb1, right after
/// `CalculateInitialFreeBeds`) calls 0x4d67d0 on every eco-sector 1..=count. An eco-sector whose
/// free carriers (`+0x26 − +0x22`) exceed its minimum (`+0x20`) asks for builders and diggers up
/// to 5 of each, as far as hammers (`+0x2b4`, good 18) and shovels (`+0x2ce`, good 31) lie on
/// its piles: `ChangeWorkerDeltaAmount` (0x4d56a0) by that many. `RecruiteWorker` then makes them
/// from carriers during the game.
fn order_digger_and_builder_at_startup(w: &mut World) {
    const HAMMER: usize = 18;
    const SHOVEL: usize = 31;
    let ids: Vec<u16> = w.economy.sectors.iter().flatten().map(|e| e.id).collect();
    for id in ids {
        let Some(e) = w.economy.get(id) else { continue };
        let free = e.settlers[1] as i32 - e.carriers_with_job_order as i32;
        if free <= e.min_carrier as i32 {
            continue;
        }
        let missing = |n: u16| (5 - n as i32).max(0);
        let builders = missing(e.settlers[3]).min(e.goods_on_piles[HAMMER] as i32);
        let diggers = missing(e.settlers[2]).min(e.goods_on_piles[SHOVEL] as i32);
        if builders > 0 {
            crate::eco::change_worker_delta_amount(w, id, 3, builders);
        }
        if diggers > 0 {
            crate::eco::change_worker_delta_amount(w, id, 2, diggers);
        }
    }
}

/// `CWorldManager::LoadGfxData` (release 0x60d690).
fn load_gfx_data(w: &mut World, map: &MapFile, r: &mut LoadReport) -> s4_formats::Result<()> {
    let land = map.landscape()?;
    crate::debug::load_step("landscape layers");
    let mut owner = vec![0u8; land.nodes.len()];
    for (i, n) in land.nodes.iter().enumerate() {
        let l = &mut w.layers;
        l.height[i] = n.height;
        l.ground[i] = n.ground;
        l.gradient[i] = n.gradient;
        l.gfx_bits[i] = n.gfx_bits;
        // StartUpOwner(i, OwnerPseudoTileId(water ? 0 : owner)).
        owner[i] = if n.ground <= 8 { 0 } else { n.owner };
        let mut f = n.flags;
        if n.gradient & 0x40 != 0 {
            f |= flag::GRADIENT_40;
        }
        if f & flag::BLOCKED_OBJECT != 0 {
            l.set_move_cost(i, 7);
        } else if n.ground == 28 {
            l.set_move_cost(i, 1);
        } else if n.ground == 29 {
            l.set_move_cost(i, 0);
            l.set_walk_count(i, 15);
        } else {
            l.set_move_cost(i, 2);
        }
        l.flags[i] = f;
    }
    // ITiling::Init (fog TODO(port)).
    crate::debug::load_step("tiling");
    w.init_tiling(&owner);
    crate::debug::load_step("decos");
    let size = w.grid.size() as usize;
    for (i, n) in land.nodes.iter().enumerate() {
        let (x, y) = ((i % size) as i32, (i / size) as i32);
        if n.object != 0 {
            r.object_nodes += 1;
            if matches!(n.object, 162 | 209 | 210 | 211) {
                r.objects_ignored += 1;
            } else {
                let count_before = w.entities.len();
                match create::add_deco_obj_without_flags(w, x, y, n.object as u16, 0) {
                    Ok(_) => r.deco_entities += (w.entities.len() - count_before) as u32,
                    Err(e) => r.objects_skipped.push((n.object as u16, e)),
                }
            }
        }
        if n.resource != 0 {
            w.layers.set_resource(i, n.resource & 0xF0, n.resource & 0x0F);
        }
    }
    Ok(())
}

/// `CBuildingMgr::LoadBuildingData` (release 0x53d940). The loading flag (0x14f1924) is set
/// for its duration: a garrisoned start tower then only marks its tower bits.
fn load_building_data(w: &mut World, map: &MapFile, r: &mut LoadReport) -> s4_formats::Result<()> {
    w.loading_buildings = true;
    let res = load_building_records(w, map, r);
    w.loading_buildings = false;
    res
}

fn load_building_records(w: &mut World, map: &MapFile, r: &mut LoadReport) -> s4_formats::Result<()> {
    for b in map.buildings()? {
        r.building_records += 1;
        if b.kind == 0 {
            r.buildings_type0 += 1;
            continue;
        }
        let id = match create::add_building(w, b.x as i32, b.y as i32, b.owner, b.kind) {
            Ok(id) => id,
            Err(e) => {
                r.buildings_skipped.push((b.kind, e));
                continue;
            }
        };
        r.buildings += 1;
        let kind = w.entities.by_index(id as u32).map_or(0, |e| e.kind);
        if w.rules.is_military(kind) && b.has_garrison != 0 {
            for (j, &n) in b.soldiers.iter().enumerate() {
                for _ in 0..n {
                    r.soldiers_requested += 1;
                    r.medics_requested += (j >= 6) as u32;
                    if create::add_soldier_to_start_tower(w, id, j as u8) {
                        r.soldiers += 1;
                    }
                }
            }
        }
        // Ports and shipyards (0x34..=0x3f, 0x4c..=0x4f): 0x541100, the +0x314 cells'
        // `SetBlockedWater` (a no-op after `SetToWorld`); shipyards then `CBuilding::PostLoad`
        // (vtbl+0xc 0x536170: the workshop's water bit 0x1000 at the workingpos).
        if matches!(b.kind, 0x3a..=0x3f | 0x4c | 0x4d) {
            crate::buildings::workshop::post_load(w, id);
        }
    }
    Ok(())
}

/// `CSettlerMgr::LoadSettlerData` (release 0x584b10).
fn load_settler_data(w: &mut World, map: &MapFile, r: &mut LoadReport) -> s4_formats::Result<()> {
    for s in map.settlers()? {
        r.settler_records += 1;
        let (x, y) = (s.x as i32, s.y as i32);
        if s.kind >= 200 {
            match create::add_vehicle(w, x, y, s.owner, s.kind - 200) {
                Some(_) => r.vehicles += 1,
                None => r.vehicles_skipped += 1,
            }
        } else if s.kind < 67 {
            if create::add_settler(w, x, y, s.owner, s.kind as u16).is_some() {
                r.settlers += 1;
            }
        } else {
            r.settlers_ignored += 1;
        }
    }
    Ok(())
}

/// `CPileMgr::LoadPileData` (release 0x570e60).
fn load_pile_data(w: &mut World, map: &MapFile, r: &mut LoadReport) -> s4_formats::Result<()> {
    for p in map.piles()? {
        r.pile_records += 1;
        if create::add_pile(w, p.x as i32, p.y as i32, p.good, p.amount, p.role_type()).is_some() {
            r.piles += 1;
        }
    }
    Ok(())
}
