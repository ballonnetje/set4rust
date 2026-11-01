//! Read-only questions the map script asks (the Lua exports of `CGameScriptManager`, release
//! bodies at the addresses given; the decompilation has only `Game.ArePlayerAreasConnected`).
//! Pure reads of the world: deterministic, no state changes.

use crate::World;
use crate::entity::{ObjType, flag};
use crate::grid::{self, Y16X16};
use crate::spiral;

/// `Settlers.AmountInArea`'s "any settler type" (0x446).
pub const ANY_SETTLER: i32 = 0x446;

/// `CBuildingMgr::GetNumberOfBuildings` periods (`Buildings.UNDERCONSTRUCTION / READY / ALL`).
pub const UNDER_CONSTRUCTION: i32 = 0;
pub const READY: i32 = 1;
pub const ALL: i32 = 2;

impl World {
    /// `CAlliances::IsValidUsedPlayerId` (alliance id > 0: a player in the game).
    pub fn is_used_player(&self, player: i32) -> bool {
        (1..=8).contains(&player) && self.player(player as u8).is_some()
    }

    /// `CPlayerManager::NumberOfPlayers`: the highest player id in the game.
    pub fn number_of_players(&self) -> i32 {
        (1..=8).rev().find(|&p| self.is_used_player(p)).unwrap_or(0)
    }

    /// The buildings `CBuildingMgr` lists for (player, type): every building entity until it is
    /// destroyed (`CheckOutBuilding`), dead or not.
    fn buildings_of(&self, player: i32, building: i32) -> impl Iterator<Item = (u16, &crate::Entity)> {
        self.entities
            .iter()
            .filter(move |(_, e)| e.obj_type == ObjType::Building && e.owner as i32 == player && e.kind as i32 == building)
    }

    /// `CBuildingMgr`'s list of (player, type) (heads 0x14eea74 + 4 · (83 · player + type),
    /// linked through entity +0x28 / +0x2a; [`crate::buildings::BuildingRoles::lists`]), head
    /// first.
    pub fn building_list(&self, player: i32, building: i32) -> Vec<u16> {
        if !(0..=8).contains(&player) || !(0..=0xffff).contains(&building) {
            return Vec::new();
        }
        crate::buildings::list(self, player as u8, building as u16).to_vec()
    }

    /// `Buildings.GetFirstBuilding(player, type)` (0x5c5be0): the head of the list (0 if none;
    /// the release reads the table unchecked).
    pub fn first_building(&self, player: i32, building: i32) -> u16 {
        self.building_list(player, building).first().copied().unwrap_or(0)
    }

    /// `Buildings.CheckBuildingInhabitants(player, type)` (0x5c5930): 1 when the list is not empty
    /// and every building on it has its worker in the house (role +0x1d, `m_bWorkerInHouse`).
    pub fn check_building_inhabitants(&self, player: i32, building: i32) -> bool {
        let list = self.building_list(player, building);
        !list.is_empty() && list.iter().all(|&b| crate::buildings::role(self, b).is_some_and(|r| r.base.worker_in_house))
    }

    /// `Buildings.Amount(player, type, status)` (0x5c4fa0 → `CBuildingMgr::GetNumberOfBuildings`
    /// 0x53d1c0): the manager counts a building as finished from `AddBuilding` without a site or
    /// from `CBuilding::Ready`, as under construction before (flag 0x10000 here), until
    /// `CheckOutBuilding`. Invalid player, type (1..=82) or status gives 0.
    pub fn buildings_amount(&self, player: i32, building: i32, status: i32) -> i32 {
        if !self.is_used_player(player) || !(1..=82).contains(&building) || !(0..=2).contains(&status) {
            return 0;
        }
        self.buildings_of(player, building)
            .filter(|(_, e)| {
                let built = e.flags & crate::buildings::FLAG_BUILT != 0;
                status == ALL || built == (status == READY)
            })
            .count() as i32
    }

    /// `Buildings.ExistsBuildingInArea(player, type, x, y, radius, status = READY)` (0x5c5140):
    /// a building of the player and type within grid distance `radius` of (x, y) whose READY flag
    /// matches the status (`ALL`: any; any other value than `READY` asks for not READY).
    pub fn exists_building_in_area(&self, player: i32, building: i32, x: i32, y: i32, radius: i32, status: i32) -> bool {
        if !self.grid.in_world(x, y) || !(1..=self.number_of_players()).contains(&player) {
            return false;
        }
        self.buildings_of(player, building).any(|(_, e)| {
            spiral::grid_distance(e.x() - x, e.y() - y) <= radius
                && (status == ALL || (e.flags & flag::READY != 0) == (status == READY))
        })
    }

    /// `Buildings.IsWorkingArea(player, type, x, y)` (0x5c5530): a player 1..=`PlayerCount` and
    /// (x, y) on the map; some building of the list whose working-area centre (+0x4c) lies
    /// within grid distance below the working radius of the player's race and the type
    /// (buildingInfo +0x1ec). The release reads the type unchecked.
    pub fn is_working_area(&self, player: i32, building: i32, x: i32, y: i32) -> bool {
        if !self.grid.in_world(x, y) || !(1..=self.player_count() as i32).contains(&player) || !(0..83).contains(&building) {
            return false;
        }
        let radius = self.rules.work.info(self.race_of(player as u8), building as u8).map_or(0, |i| i.working_radius);
        self.buildings_of(player, building).any(|(id, _)| {
            crate::jobs::core::building_working_area(self, id).is_some_and(|a| spiral::grid_distance(a.x() - x, a.y() - y) < radius)
        })
    }

    /// `Buildings.GetInhabitantAmount(player, type)` (0x5c5a70; neither is checked): over the
    /// list, a military or castle building (Roman buildingInfo kind 10 / 15 of the entity's
    /// type, info +0x1dd) adds its bowmen and swordsmen inside (role +0x17c / +0x17d); any other
    /// adds 1 when its worker is **not** in the house (role +0x1d == 0), as the release does.
    pub fn inhabitant_amount(&self, player: i32, building: i32) -> i32 {
        self.buildings_of(player, building)
            .map(|(id, e)| {
                if matches!(self.rules.building(0, e.kind as u8).house_kind, 10 | 15) {
                    crate::buildings::military::state(self, id).map_or(0, |s| s.bow_in as i32 + s.sword_in as i32)
                } else {
                    !crate::buildings::have_inhabitant(self, id) as i32
                }
            })
            .sum()
    }

    /// `Goods.GetAmountInArea(player, good, x, y, radius)` (0x5c8a30): player −1 (any) or 1..8 in
    /// the game, good −1 (any) or 1..=0x2a, (x, y) on the map, radius 1..=0x4a; the goods of the
    /// piles (deco-object layer, object type 0x10) on the spiral walk whose node belongs to the
    /// player (tile owner) and of the good (pile vtbl+0x3c / +0x28: good, amount).
    pub fn goods_amount_in_area(&self, player: i32, good: i32, x: i32, y: i32, radius: i32) -> i32 {
        let player_ok = player == -1 || self.is_used_player(player);
        let good_ok = good == -1 || (1..0x2b).contains(&good);
        if !player_ok || !good_ok || !self.grid.in_world(x, y) || !(1..=0x4a).contains(&radius) {
            return 0;
        }
        spiral::walk(self.grid, x, y, radius as u8)
            .filter_map(|(px, py)| {
                let n = self.grid.index(px as u32, py as u32);
                let id = self.layers.deco_object[n];
                let e = self.entities.by_index(id as u32).filter(|e| id != 0 && e.obj_type == ObjType::Pile)?;
                let crate::entity::EntityData::Pile(p) = &e.data else { return None };
                ((player == -1 || self.owner_of(n) as i32 == player) && (good == -1 || p.good as i32 == good)).then_some(p.amount as i32)
            })
            .sum()
    }

    /// `Game.IsAreaOwned(player, x, y, radius)` (0x5c7100 → 0x5b19c0): every land node
    /// (ground > 8) of the spiral walk within `radius` (1..=74) belongs to the player.
    pub fn is_area_owned(&self, player: i32, x: i32, y: i32, radius: i32) -> bool {
        if !self.is_used_player(player) || !self.grid.in_world(x, y) || !(1..=74).contains(&radius) {
            return false;
        }
        spiral::walk(self.grid, x, y, radius as u8).all(|(px, py)| {
            let n = self.grid.index(px as u32, py as u32);
            self.layers.ground[n] <= 8 || self.owner_of(n) as i32 == player
        })
    }

    /// `Game.IsAreaGreen(x, y, radius)` (0x5c6f60 → 0x5b1a70): no node within `radius` (1..=74)
    /// has the dark-land flag (`m_cFlagMap` 0x04).
    pub fn is_area_green(&self, x: i32, y: i32, radius: i32) -> bool {
        if !self.grid.in_world(x, y) || !(1..=74).contains(&radius) {
            return false;
        }
        spiral::walk(self.grid, x, y, radius as u8)
            .all(|(px, py)| self.layers.flags[self.grid.index(px as u32, py as u32)] & grid::flag::GRADIENT_40 == 0)
    }

    /// `Game.ArePlayerAreasConnected(a, xa, ya, b, xb, yb)` (0x5c6150, `GameArePlayerAreasConnected`):
    /// for players of one alliance, the first free node of each player's own land within spiral
    /// radius 15 of its point, and a land path between them over allied land
    /// (`CAStarTiling::FindPath(.., a | 0x700) == 1`).
    pub fn are_player_areas_connected(&self, a: i32, xa: i32, ya: i32, b: i32, xb: i32, yb: i32) -> bool {
        if !self.is_used_player(a) || !self.is_used_player(b) || !self.grid.in_world(xa, ya) || !self.grid.in_world(xb, yb) {
            return false;
        }
        let alliances = &self.alliances;
        if alliances.alliance_id[a as usize] != alliances.alliance_id[b as usize] {
            return false;
        }
        let own_node = |x: i32, y: i32, p: i32| {
            spiral::walk(self.grid, x, y, 15).find(|&(px, py)| {
                let n = self.grid.index(px as u32, py as u32);
                self.layers.flags[n] & grid::flag::BLOCKED_LAND == 0 && self.owner_of(n) as i32 == p
            })
        };
        let (Some((sx, sy)), Some((ex, ey))) = (own_node(xa, ya, a), own_node(xb, yb, b)) else { return false };
        let mut wp = crate::pathing::Waypoints::default();
        let side = self.grid.size() as usize / 16;
        crate::pathing::astar_tiling(&self.tiling, alliances, &self.warmap, side, Y16X16::pack(sx, sy), Y16X16::pack(ex, ey), a as u32 | 0x700, &mut wp)
            == 1
    }

    /// `Settlers.AmountInArea(player, type, x, y, radius)` (0x5c0cd0): the READY settlers of the
    /// player standing on the nodes (map-object layer) of the spiral walk within `radius`, of
    /// the type or of any type ([`ANY_SETTLER`]). Settlers inside buildings are not on the map.
    pub fn settlers_amount_in_area(&self, player: i32, settler: i32, x: i32, y: i32, radius: i32) -> i32 {
        if !self.grid.in_world(x, y) || !(1..=self.number_of_players()).contains(&player) {
            return 0;
        }
        let r = radius.clamp(0, spiral::MAX_RADIUS as i32) as u8;
        spiral::walk(self.grid, x, y, r)
            .filter(|&(px, py)| {
                let id = self.layers.map_object[self.grid.index(px as u32, py as u32)];
                self.entities.by_index(id as u32).is_some_and(|e| {
                    id != 0
                        && e.obj_type == ObjType::Settler
                        && e.flags & flag::READY != 0
                        && e.owner as i32 == player
                        && (settler == ANY_SETTLER || e.kind as i32 == settler)
                })
            })
            .count() as i32
    }

    /// `Game.IsAlmostAllLandExplored` (0x5c6c90 → `CFogging` vtable +56): the local fog's scan
    /// passed every square row; false without a fog.
    pub fn is_almost_all_land_explored(&self) -> bool {
        self.fog.as_ref().is_some_and(|f| f.is_almost_all_land_explored())
    }

    /// `Game.EnableLandExploredCheck(on)` → `CFogging::EnableOrDisableLandExploredCheck`.
    pub fn enable_land_explored_check(&mut self, on: bool) {
        if let Some(f) = self.fog.as_mut() {
            f.enable_land_explored_check(on);
        }
    }

    /// 0x53efd0 `(player, n)`: the player has at least `n` READY, inhabited buildings of the
    /// types 46, 47, 48, 50, 51 (the table at 0x101a200: towers and castle, Dark Tribe fortress
    /// and temple).
    pub fn has_military_presence(&self, player: i32, n: i32) -> bool {
        let mut count = 0;
        for t in [46, 47, 48, 50, 51] {
            for (id, e) in self.buildings_of(player, t) {
                if e.flags & flag::READY != 0 && crate::buildings::have_inhabitant(self, id) {
                    count += 1;
                    if count >= n {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// The default "player lost" rule of `Game.DefaultPlayerLostCheck` (0x5c7ff0) and
    /// `Game.DefaultPlayersLostCheck` (0x5c8100): a player in the game who has no READY,
    /// inhabited military building ([`World::has_military_presence`] with 1). The script host
    /// then marks the player lost (once) and posts message 0x38.
    pub fn default_player_lost(&self, player: i32) -> bool {
        self.is_used_player(player) && !self.has_military_presence(player, 1)
    }
}
