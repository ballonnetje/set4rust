//! The part of `CStatistic` (per player 0x1128 bytes from .bss 0x11abe34) the simulation reads:
//! the **fighting strength** (own reverse engineering of the shipped exe).
//!
//! * Every finished building of the player adds its cost to three sums (+0xba4 boards, +0xba8
//!   stone, +0xbac gold; eye catchers 64..75 also to +0xbb0..+0xbb8) — 0x4f8730 with ±1, called
//!   by `BuildingDone` (0x5362f0), the map placer (0x53c350), `CheckOutBuilding` on deletion of
//!   a finished building (0x53c680) and an owner change (0x53f050). The per-type counts of
//!   finished buildings (`CBuildingMgr` 0x14eea58 +0xbcc, i.e. 0x14ef624) change at exactly
//!   these four places (write references: 0x536340, 0x53c4f8, 0x53c769, 0x53f08e / 0x53f128;
//!   besides them only the savegame code 0x53bce3 / 0x53c13c and the uncalled 0x53d220), so they
//!   are kept here with the costs.
//! * 0x4f8450 then recomputes `s = ((a + b) · 256 + 127) / FightingStrengthDivisor + base +
//!   30 · min(eye catcher counts of types 64..75)` with `a = boards + 2·gold + stone` and
//!   `b = 2 · (the same of the eye catchers)` (`b = a` for a computer player — control 2 / 3,
//!   0x4f7a40 — of an active AI, 0x146b148 +0x10, at level 2 or more, 0x434600); `s` is compressed
//!   above 128 (halved), 256 (halved again), 320 (halved again), at most 384; game type 3
//!   (`GlobalGameStateObject +0x58`) quarters it (at least 1). A set override (+0x1124 > 0,
//!   script 0x4f8380) replaces the whole formula; a base of 256 makes it 256.
//! * Strength on foreign land (+0xbc0, read through 0x11ac9f4) is `s`; on the player's own land
//!   (+0xbbc, 0x11ac9f0) `max(256, 256 + (s − 256) / 2)`. 256 = 100 %. Damage is
//!   `(strength · damage + 127) >> 8`.
//! * The base (+0xba0) is set at game start (0x4f86b0): 256 for a Dark Tribe player, else
//!   `768 / (players + 4)`.

use crate::hash::StateHasher;
use crate::world::World;

const PLAYERS: usize = 9;
const BUILDINGS: usize = 83;

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PlayerStrength {
    /// +0xba0 base, +0x1124 override (0 = none).
    pub base: i32,
    pub over: i32,
    /// +0xba4 / +0xba8 / +0xbac: boards, stone, gold of the finished buildings; +0xbb0.. the same
    /// of the eye catchers.
    pub cost: [i32; 3],
    pub eye_cost: [i32; 3],
    /// +0xbbc own land, +0xbc0 foreign land.
    pub own: i32,
    pub foreign: i32,
    /// 0x14ef624: finished buildings per type.
    #[cfg_attr(feature = "serde", serde(with = "serde_big_array::BigArray"))]
    pub count: [i32; BUILDINGS],
    /// +0xa04 + 4·good: goods on the player's piles (`CPile::AdjustStatistic` 0x56fe80, the only
    /// writer; read by the GUI's goods pages).
    #[cfg_attr(feature = "serde", serde(default = "no_goods", with = "serde_big_array::BigArray"))]
    pub goods: [i32; GOODS],
    /// +0xab0: their total.
    #[cfg_attr(feature = "serde", serde(default))]
    pub goods_total: i32,
    /// +0x0 + 4·type: the player's settlers per type (`IncNumberOfSettler` 0x4f7d40 from
    /// `AddSettler` / `ChangeType`, `DecNumberOfSettler` 0x4f7db0 from `ChangeType` / the removal
    /// 0x582940); +0x10c their total; +0x220 those of warrior type 0 (civilians), +0x224 those of
    /// warrior types other than 0, 1, 6, 7 and 14 (soldiers).
    #[cfg_attr(feature = "serde", serde(default = "per_type"))]
    pub settlers: Vec<i32>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub settlers_total: i32,
    #[cfg_attr(feature = "serde", serde(default))]
    pub civilians: i32,
    #[cfg_attr(feature = "serde", serde(default))]
    pub soldiers: i32,
    /// `CMapObjectMgr::Kill(entity, attacker)` (0x56d030, attacker > 0): +0xc1c + 4·type the
    /// attacker's settler kills (type < 0x43), +0xd28 their total; +0xd2c the victim owner's
    /// losses, +0xe38 their total; the same for ships and land vehicles at +0xe40 / +0xf4c
    /// (kills) and +0xf50 / +0x105c (losses).
    #[cfg_attr(feature = "serde", serde(default = "per_type"))]
    pub kills: Vec<i32>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub kills_total: i32,
    #[cfg_attr(feature = "serde", serde(default = "per_type"))]
    pub losses: Vec<i32>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub losses_total: i32,
    #[cfg_attr(feature = "serde", serde(default = "per_type"))]
    pub vehicle_kills: Vec<i32>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub vehicle_kills_total: i32,
    #[cfg_attr(feature = "serde", serde(default = "per_type"))]
    pub vehicle_losses: Vec<i32>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub vehicle_losses_total: i32,
    /// +0xe3c: the player's military buildings destroyed (the role's +0x74, 0x549440).
    #[cfg_attr(feature = "serde", serde(default))]
    pub military_lost: i32,
    /// +0x374 + 4·type: buildings the player captured, +0x4c0 + 4·type: buildings it lost to a
    /// capture (`FirstSettlerEnter` 0x54871c / 0x548730; the writer 0x4f8ac0 has no callers).
    #[cfg_attr(feature = "serde", serde(default = "per_building"))]
    pub conquered: Vec<i32>,
    #[cfg_attr(feature = "serde", serde(default = "per_building"))]
    pub captured_from: Vec<i32>,
    /// The mushroom farms this player destroyed: not `CStatistic` but the player record
    /// 0x149b868 + 0x8c · p (`CDarkMushroomFarmRole` vtbl+0x2c 0x545050 adds one for the building's
    /// cause byte +0x25), which `Statistic.MushroomFarmsDestroyed` (0x5ca350) reads.
    #[cfg_attr(feature = "serde", serde(default))]
    pub mushroom_farms_destroyed: i32,
    /// +0x110 + 4·type: settlers made — a carrier at the barracks becoming its next type
    /// (0x5739c5), a dark temple's unit (0x54586a, counted whether it appeared or not), a
    /// residence's new carrier (0x54ddc6); +0x21c their total (0x4f7e50 has no callers).
    #[cfg_attr(feature = "serde", serde(default = "per_type"))]
    pub produced: Vec<i32>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub produced_total: i32,
    /// +0x228 + 4·type / +0x60c: the finished buildings now — they move with [`Self::count`] at
    /// the same four places (0x53635b, 0x53c46d, 0x53c744, 0x53f0b4 / 0x53f131), so they are
    /// [`Self::count`] and [`PlayerStrength::buildings_total`]. +0x610 + 4·type / +0x75c: the
    /// buildings finished ever, only in `BuildingDone` (0x53639c / 0x5363a9).
    #[cfg_attr(feature = "serde", serde(default = "per_building"))]
    pub built: Vec<i32>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub built_total: i32,
    /// +0xa00: the beds of the player's residences — `CResidenceBuildingRole::Init` adds them
    /// (0x54db94), its vtbl+0x2c (0x54df40, crushed or destroyed) takes them off. Read by the
    /// end screen.
    #[cfg_attr(feature = "serde", serde(default))]
    pub beds: i32,
    /// +0xab4 + 4·good / +0xb60: goods put on production piles (`CProductionPileRole::Increase`
    /// 0x571810, counted for the owner of the pile's tile).
    #[cfg_attr(feature = "serde", serde(default = "no_goods_vec"))]
    pub goods_made: Vec<i32>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub goods_made_total: i32,
    /// +0xbc4: boards + stone + gold of the finished buildings (0x4f8730).
    #[cfg_attr(feature = "serde", serde(default))]
    pub value: i32,
    /// +0xb9c: the player's share of the land in percent (0x4f8850: `CTiling` vtbl+0x3c · 100 /
    /// vtbl+0x34), +0x106c the highest share so far.
    #[cfg_attr(feature = "serde", serde(default))]
    pub land: i32,
    #[cfg_attr(feature = "serde", serde(default))]
    pub land_max: i32,
    /// +0x1060 / +0x1064: copies of the mana pool's mana and total gained (0x4f87f0, every
    /// `Update`).
    #[cfg_attr(feature = "serde", serde(default))]
    pub mana: i32,
    #[cfg_attr(feature = "serde", serde(default))]
    pub mana_gained: i32,
    /// +0x1078 + 4·good: the goods as they were when the start snapshot froze (mirrored by
    /// `AdjustStatistic` until then). Saved in chunk 133; nothing live reads it (its getter
    /// 0x4f7cd0 has no callers).
    #[cfg_attr(feature = "serde", serde(default = "no_goods_vec"))]
    pub goods_start: Vec<i32>,
    /// +0x1070 / +0x1074: the highest own-land / foreign-land strength so far (0x4f8450).
    #[cfg_attr(feature = "serde", serde(default))]
    pub own_max: i32,
    #[cfg_attr(feature = "serde", serde(default))]
    pub foreign_max: i32,
}

fn no_goods_vec() -> Vec<i32> {
    vec![0; GOODS]
}

/// Settler types counted per type (`< 0x43`).
pub const SETTLER_TYPES: usize = 67;

fn per_type() -> Vec<i32> {
    vec![0; SETTLER_TYPES]
}

fn per_building() -> Vec<i32> {
    vec![0; BUILDINGS]
}

/// Goods slots of the statistic (0..=42).
pub const GOODS: usize = 43;

#[cfg(feature = "serde")]
fn no_goods() -> [i32; GOODS] {
    [0; GOODS]
}

impl PlayerStrength {
    /// +0x60c: the finished buildings now (the sum of [`Self::count`]).
    pub fn buildings_total(&self) -> i32 {
        self.count.iter().sum()
    }
}

impl Default for PlayerStrength {
    fn default() -> Self {
        PlayerStrength {
            base: 0,
            over: 0,
            cost: [0; 3],
            eye_cost: [0; 3],
            own: 0,
            foreign: 0,
            count: [0; BUILDINGS],
            goods: [0; GOODS],
            goods_total: 0,
            settlers: per_type(),
            settlers_total: 0,
            civilians: 0,
            soldiers: 0,
            kills: per_type(),
            kills_total: 0,
            losses: per_type(),
            losses_total: 0,
            vehicle_kills: per_type(),
            vehicle_kills_total: 0,
            vehicle_losses: per_type(),
            vehicle_losses_total: 0,
            military_lost: 0,
            conquered: per_building(),
            captured_from: per_building(),
            mushroom_farms_destroyed: 0,
            produced: per_type(),
            produced_total: 0,
            built: per_building(),
            built_total: 0,
            beds: 0,
            goods_made: no_goods_vec(),
            goods_made_total: 0,
            value: 0,
            land: 0,
            land_max: 0,
            mana: 0,
            mana_gained: 0,
            goods_start: no_goods_vec(),
            own_max: 0,
            foreign_max: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Statistic {
    pub players: Vec<PlayerStrength>,
    /// The history (`CStatisticArchive`).
    #[cfg_attr(feature = "serde", serde(default))]
    pub archive: Archive,
    /// Byte 0x12941e7: the start snapshot is frozen.
    #[cfg_attr(feature = "serde", serde(default))]
    pub start_frozen: bool,
}

impl Default for Statistic {
    fn default() -> Self {
        Statistic { players: vec![PlayerStrength::default(); PLAYERS], archive: Archive::default(), start_frozen: false }
    }
}

impl Statistic {
    pub(crate) fn hash(&self, h: &mut StateHasher) {
        self.archive.hash(h);
        h.bool(self.start_frozen);
        for p in &self.players {
            p.goods_start.iter().for_each(|&v| h.u32(v as u32));
        }
        for p in &self.players {
            for v in [p.base, p.over, p.own, p.foreign, p.goods_total].iter().chain(&p.cost).chain(&p.eye_cost).chain(&p.count).chain(&p.goods) {
                h.u32(*v as u32);
            }
            let scalars = [
                p.settlers_total,
                p.civilians,
                p.soldiers,
                p.kills_total,
                p.losses_total,
                p.vehicle_kills_total,
                p.vehicle_losses_total,
                p.military_lost,
                p.mushroom_farms_destroyed,
                p.produced_total,
                p.built_total,
                p.beds,
                p.goods_made_total,
                p.value,
                p.land,
                p.land_max,
                p.mana,
                p.mana_gained,
                p.own_max,
                p.foreign_max,
            ];
            for v in scalars.iter().chain(&p.settlers).chain(&p.kills).chain(&p.losses).chain(&p.vehicle_kills).chain(&p.vehicle_losses).chain(&p.conquered).chain(&p.captured_from).chain(&p.produced).chain(&p.built).chain(&p.goods_made) {
                h.u32(*v as u32);
            }
        }
    }
}

/// 0x4f7ac0 / 0x4f86b0 at game start: bases for every player, then the strengths.
pub fn init(world: &mut World) {
    let n = world.players.iter().filter(|p| p.is_some()).count() as i32;
    world.statistic = Statistic::default();
    for p in 1..PLAYERS {
        let base = if world.race_of(p as u8) == 3 { 0x100 } else { 0x300 / (n + 4) };
        world.statistic.players[p].base = base;
        recompute(world, p as u8);
    }
}

/// `CPile::AdjustStatistic` 0x56fe80, the statistic half (after the eco-sector's goods count):
/// for the owner of the eco-sector, `delta > 0` adds to the good and the total; otherwise at most
/// the good's count is taken from both. While the start snapshot is not frozen (byte 0x12941e7)
/// the good's count is mirrored into it (+0x1078).
pub fn adjust_goods(world: &mut World, player: u8, good: u8, delta: i32) {
    let frozen = world.statistic.start_frozen;
    let Some(p) = world.statistic.players.get_mut(player as usize) else { return };
    let Some(g) = p.goods.get_mut(good as usize) else { return };
    let d = if delta > 0 { delta } else { -(delta.saturating_neg().min(*g)) };
    *g += d;
    p.goods_total += d;
    let now = *g;
    if !frozen && let Some(s) = p.goods_start.get_mut(good as usize) {
        *s = now;
    }
}

/// 0x4f8f80 (from `EventManagement` 0x5172f0) and the same copy in `Statistic::Update` 0x4f88d0
/// (once `GlobalGameState +0x2e0 > 0`, which the game setup 0x520c90 / 0x5265a0 sets for game
/// mode 5): once, every player's goods (+0xa04, 43 values) become the start snapshot (+0x1078)
/// and it is frozen. `EventManagement` calls it at 0x51754c and 0x517653, [inf] in the handling of a
/// player leaving the game (messages 0x78e / 0x8ce / 0x8d1; 0x517631 clears the player's present
/// flag 0x149b608 + 0x3c·p and 0x517650 removes its AI, vtbl+0x20). TODO(port): those network
/// paths — the snapshot only matters for chunk 133. The mode-5 flag: `tick::statistic_update`.
pub fn freeze_start(world: &mut World) {
    if world.statistic.start_frozen {
        return;
    }
    world.statistic.start_frozen = true;
    for p in world.statistic.players.iter_mut() {
        p.goods_start.copy_from_slice(&p.goods);
    }
}

/// 0x4f8730 `(player, race, type, sign)`: a finished building (dis)appears.
pub fn building(world: &mut World, player: u8, kind: u16, sign: i32) {
    let race = world.race_of(player);
    let r = world.rules.building(race, kind as u8);
    let (stone, boards, gold) = (r.cost_stone as i32 * sign, r.cost_boards as i32 * sign, r.cost_gold as i32 * sign);
    let Some(p) = world.statistic.players.get_mut(player as usize) else { return };
    p.cost[0] += boards;
    p.cost[1] += stone;
    p.cost[2] += gold;
    p.value += boards + stone + gold;
    if (64..76).contains(&kind) {
        p.eye_cost[0] += boards;
        p.eye_cost[1] += stone;
        p.eye_cost[2] += gold;
    }
    if let Some(c) = p.count.get_mut(kind as usize) {
        *c += sign;
    }
    recompute(world, player);
}

/// The formula of 0x4f8450: the foreign-land strength from the override `over` (+0x1124), the
/// base (+0xba0), the building sums `a` and `b`, the complete eye-catcher sets, the divisor and
/// game type 3 (`quarter`).
pub fn foreign_strength(over: i32, base: i32, a: i32, b: i32, sets: i32, div: i32, quarter: bool) -> i32 {
    let mut s = over;
    if s < 1 {
        s = 0x100;
        if base != 0x100 {
            s = ((a + b) * 0x100 + 0x7f) / div + base + sets * 0x1e;
            if s > 0x80 {
                let d = (s - 0x80) / 2;
                s = d + 0x80;
                if s > 0x100 {
                    let d = (d - 0x80) / 2;
                    s = d + 0x100;
                    if s > 0x140 {
                        s = ((d - 0x40) / 2 + 0x140).min(0x180);
                    }
                }
            }
        }
    }
    // Game type 3 (`GlobalGameStateObject +0x58`) quarters the strength, at least 1.
    if quarter {
        s = if s < 4 { 1 } else { s / 4 };
    }
    s
}

/// The own-land strength for the foreign-land strength `s` (0x4f8450).
pub fn own_strength(s: i32) -> i32 {
    if s < 0x101 { 0x100 } else { (s - 0x100) / 2 + 0x100 }
}

/// 0x4f8380 `(player, percent)` (`Game.SetFightingStrength`): a positive percent sets the
/// override +0x1124 to `percent · 256 / 100`, at least the base (+0xba0) and at most 384; 0 or
/// less clears it. Then 0x4f8450.
pub fn set_override(world: &mut World, player: u8, percent: i32) {
    if !(1..=8).contains(&player) {
        return;
    }
    let Some(p) = world.statistic.players.get_mut(player as usize) else { return };
    p.over = if percent <= 0 {
        0
    } else {
        let v = percent.wrapping_mul(256) / 100;
        if v < p.base { p.base } else { v.min(0x180) }
    };
    recompute(world, player);
}

/// 0x4f8450 `(player)`.
pub fn recompute(world: &mut World, player: u8) {
    let divisor = world.rules.logic.fighting_strength_divisor;
    // 0x4f8486..0x4f84bb: a computer player (control 2 / 3, 0x4f7a40) of an active AI (`CAIMain`
    // vtbl+0x10: +4) at level 2 or more (0x434600: the record's level for an AI player, else 0)
    // counts its buildings once more instead of the eye catchers twice.
    let control = world.setup.control.get(player as usize).copied().unwrap_or(0);
    let level = if world.ai.is_ai(player) { world.ai.vars.records.get(player as usize).map_or(0, |r| r.level) } else { 0 };
    let ai_double = matches!(control, 2 | 3) && world.ai.active && level >= 2;
    let quarter = world.setup.game_type == 3;
    let div = if divisor > 0 { divisor } else { 1000 };
    let Some(p) = world.statistic.players.get_mut(player as usize) else { return };
    let a = p.cost[1] + p.cost[2] * 2 + p.cost[0];
    let b = if ai_double { a } else { (p.eye_cost[1] + p.eye_cost[2] * 2 + p.eye_cost[0]) * 2 };
    let sets = (64..76).map(|k| p.count[k]).min().unwrap_or(0);
    let s = foreign_strength(p.over, p.base, a, b, sets, div, quarter);
    p.foreign = s;
    p.own = own_strength(s);
    // +0x1070 / +0x1074: the highest own / foreign strength so far.
    p.own_max = p.own_max.max(p.own);
    p.foreign_max = p.foreign_max.max(p.foreign);
}

/// The strength a soldier of `player` fights with on land of `land_owner` (0x589250: the own
/// value when both are of one team).
pub fn strength(world: &World, player: u8, land_owner: u8) -> i32 {
    let Some(p) = world.statistic.players.get(player as usize) else { return 0x100 };
    if crate::warmap::team(world, player) == crate::warmap::team(world, land_owner) { p.own } else { p.foreign }
}

/// The warrior-type class of 0x4f7d40 / 0x4f7db0 (table 0x12dbca0 = the warrior type): 0
/// civilians, 1 / 6 / 7 / 14 neither, the rest soldiers.
fn settler_class(world: &World, kind: u16) -> Option<bool> {
    match world.rules.warrior_type(kind) {
        0 => Some(false),
        1 | 6 | 7 | 14 => None,
        _ => Some(true),
    }
}

/// `IncNumberOfSettler(player, type)` (0x4f7d40).
pub fn settler_added(world: &mut World, player: u8, kind: u16) {
    if kind as usize >= SETTLER_TYPES {
        return;
    }
    let class = settler_class(world, kind);
    let Some(p) = world.statistic.players.get_mut(player as usize) else { return };
    p.settlers[kind as usize] += 1;
    p.settlers_total += 1;
    match class {
        Some(false) => p.civilians += 1,
        Some(true) => p.soldiers += 1,
        None => {}
    }
}

/// `DecNumberOfSettler(player, type)` (0x4f7db0): the counts it lowers stay at least 0.
pub fn settler_removed(world: &mut World, player: u8, kind: u16) {
    if kind as usize >= SETTLER_TYPES {
        return;
    }
    let class = settler_class(world, kind);
    let Some(p) = world.statistic.players.get_mut(player as usize) else { return };
    let dec = |v: &mut i32| {
        if *v > 0 {
            *v -= 1;
        }
    };
    dec(&mut p.settlers[kind as usize]);
    dec(&mut p.settlers_total);
    match class {
        Some(false) => dec(&mut p.civilians),
        Some(true) => dec(&mut p.soldiers),
        None => {}
    }
}

/// One counter update of [`killed`].
type Count = fn(&mut PlayerStrength, usize);

/// The statistic part of `CMapObjectMgr::Kill(entity, attacker)` (0x56d119..0x56d1aa): with an
/// attacker (> 0), a settler of type < 0x43 counts as the attacker's kill and its owner's loss;
/// a ship or land vehicle the same in the vehicle counts.
pub fn killed(world: &mut World, id: u16, attacker: u8) {
    if attacker == 0 {
        return;
    }
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let (obj, kind, owner) = (e.obj_type, e.kind as usize, e.owner as usize);
    let st = &mut world.statistic.players;
    let (kill, loss): (Count, Count) = match obj {
        crate::entity::ObjType::Settler => (
            |p, k| {
                p.kills[k] += 1;
                p.kills_total += 1;
            },
            |p, k| {
                p.losses[k] += 1;
                p.losses_total += 1;
            },
        ),
        crate::entity::ObjType::Ship | crate::entity::ObjType::LandVehicle => (
            |p, k| {
                p.vehicle_kills[k] += 1;
                p.vehicle_kills_total += 1;
            },
            |p, k| {
                p.vehicle_losses[k] += 1;
                p.vehicle_losses_total += 1;
            },
        ),
        _ => return,
    };
    // Settlers only below type 0x43; vehicles are indexed by their type unchecked (always small).
    if kind >= SETTLER_TYPES {
        return;
    }
    if let Some(a) = st.get_mut(attacker as usize) {
        kill(a, kind);
    }
    if let Some(v) = st.get_mut(owner) {
        loss(v, kind);
    }
}

/// `BuildingDone` (0x53639c / 0x5363a9): one more building of `kind` finished ever.
pub fn building_built(world: &mut World, player: u8, kind: u16) {
    let Some(p) = world.statistic.players.get_mut(player as usize) else { return };
    if let Some(v) = p.built.get_mut(kind as usize) {
        *v += 1;
    }
    p.built_total += 1;
}

/// +0xa00 of `player` += `n` (residence beds).
pub fn beds(world: &mut World, player: u8, n: i32) {
    if let Some(p) = world.statistic.players.get_mut(player as usize) {
        p.beds += n;
    }
}

/// `n` goods `good` made on a production pile of `player` (+0xab4 + 4·good, +0xb60).
pub fn goods_made(world: &mut World, player: u8, good: u8, n: i32) {
    let Some(p) = world.statistic.players.get_mut(player as usize) else { return };
    if let Some(v) = p.goods_made.get_mut(good as usize) {
        *v += n;
    }
    p.goods_made_total += n;
}

/// One settler of `kind` made for `player` (+0x110 + 4·kind and the total +0x21c).
pub fn settler_produced(world: &mut World, player: u8, kind: u16) {
    let Some(p) = world.statistic.players.get_mut(player as usize) else { return };
    if let Some(v) = p.produced.get_mut(kind as usize) {
        *v += 1;
    }
    p.produced_total += 1;
}

/// A capture of a building of `kind` (0x54871c / 0x548730): +0x374 + 4·kind of the new owner,
/// +0x4c0 + 4·kind of the old.
pub fn building_captured(world: &mut World, new: u8, old: u8, kind: u16) {
    let k = kind as usize;
    if let Some(v) = world.statistic.players.get_mut(new as usize).and_then(|p| p.conquered.get_mut(k)) {
        *v += 1;
    }
    if let Some(v) = world.statistic.players.get_mut(old as usize).and_then(|p| p.captured_from.get_mut(k)) {
        *v += 1;
    }
}

/// +0xe3c of `player`: one more military building destroyed (0x549440).
pub fn military_building_lost(world: &mut World, player: u8) {
    if let Some(p) = world.statistic.players.get_mut(player as usize) {
        p.military_lost += 1;
    }
}

/// 0x4f8850: for players 1..=`PlayerCount` the land share `nodes · 100 / land nodes` (+0xb9c)
/// and its maximum (+0x106c).
pub fn land_share(world: &mut World) {
    let total = world.tiling.land_nodes();
    for player in 1..=world.player_count() {
        let nodes = world.tiling.nodes_of(player);
        let share = if total != 0 { nodes.wrapping_mul(100) / total } else { 0 };
        if let Some(p) = world.statistic.players.get_mut(player as usize) {
            p.land = share;
            p.land_max = p.land_max.max(share);
        }
    }
}

/// 0x4f87f0: the mana pools of players 1..=`PlayerCount` copied in (mana and total gained).
pub fn copy_mana(world: &mut World) {
    for player in 1..=world.player_count() {
        let (mana, gained) = world.magic.pool(player).map_or((0, 0), |m| (m.mana, m.gained));
        if let Some(p) = world.statistic.players.get_mut(player as usize) {
            p.mana = mana;
            p.mana_gained = gained;
        }
    }
}

/// Categories of the statistic history per player (`CStatisticArchive` dimension 0x31).
pub const CATEGORIES: usize = 49;
/// Samples per ring.
pub const SAMPLES: usize = 16;

/// The goods of categories 7..=48 (0x4f9850 copies `+0xa04 + 4·good`; the order is the
/// release's).
pub const CATEGORY_GOODS: [u8; CATEGORIES - 7] = [
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38,
    40, 39, 42, 41,
];

/// One ring of [`Archive`]: `SAMPLES` slots per player and category.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Ring {
    /// Samples per slot step (0 and 1 alike: every sample).
    pub every: u32,
    /// The span the ring shows, in minutes (15 / 60 / 240; not read by the simulation).
    pub minutes: u32,
    /// Samples since the last step.
    pub calls: u32,
    /// The slot being written, −1 before the first sample.
    pub index: i32,
    /// `[(player · CATEGORIES + category) · SAMPLES + slot]`, players 0..=8.
    pub data: Vec<i32>,
}

impl Ring {
    fn new(every: u32, minutes: u32) -> Ring {
        Ring { every, minutes, calls: 0, index: -1, data: vec![0; PLAYERS * CATEGORIES * SAMPLES] }
    }

    /// The `SAMPLES` slots of `player`'s `category` as stored (slot [`Ring::index`] the newest).
    pub fn slots(&self, player: u8, category: usize) -> &[i32] {
        let i = (player as usize * CATEGORIES + category) * SAMPLES;
        self.data.get(i..i + SAMPLES).unwrap_or(&[])
    }

    /// 0x4f9330's step: the first ring steps every sample; the others after `every` samples or
    /// on the first one. The slot wraps at `SAMPLES`.
    fn step(&mut self, first_ring: bool) {
        self.calls += 1;
        if self.every <= self.calls || (!first_ring && self.index == -1) {
            self.index += 1;
            self.calls = 0;
            if self.index >= SAMPLES as i32 {
                self.index = 0;
            }
        }
    }
}

/// `CStatisticArchive` (.bss 0x14865d8; ctor 0x4f9070, `Store` 0x4f9240 / `Load` 0x4f9180 into
/// savegame chunk 137: version 1, dimensions 9 · 0x31 · 16 · 16 · 16, the three rings, then 0xc4
/// bytes nothing writes): the history the statistics screen draws. Three rings of 16 slots per
/// player and category: one sample per slot (15 minutes), one slot per 3 samples (an hour), one
/// per 15 (4 hours). The ring positions are not saved.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Archive {
    pub rings: [Ring; 3],
}

impl Default for Archive {
    fn default() -> Self {
        Archive { rings: [Ring::new(0, 15), Ring::new(3, 60), Ring::new(15, 240)] }
    }
}

impl Archive {
    pub(crate) fn hash(&self, h: &mut StateHasher) {
        for r in &self.rings {
            h.u32(r.calls);
            h.u32(r.index as u32);
            r.data.iter().for_each(|&v| h.u32(v as u32));
        }
    }
}

/// The value of `category` for player `p` that a sample takes (0x4f9330 / 0x4f9850): 0 land
/// share, 1 building value, 2 civilians, 3 soldiers, 4 kills, 5 losses, 6 military buildings
/// lost, 7..=48 goods ([`CATEGORY_GOODS`]).
pub fn category_value(p: &PlayerStrength, category: usize) -> i32 {
    match category {
        0 => p.land,
        1 => p.value,
        2 => p.civilians,
        3 => p.soldiers,
        4 => p.kills_total,
        5 => p.losses_total,
        6 => p.military_lost,
        c => CATEGORY_GOODS.get(c.wrapping_sub(7)).and_then(|&g| p.goods.get(g as usize)).copied().unwrap_or(0),
    }
}

/// `CStatisticArchive::TriggerStatistic` (0x4f9330, from the logic tick 0x4f2720 when `tick %
/// 840 == 0 || tick == 1`): the rings step, then for players 1..=`PlayerCount` the land share is
/// recomputed (0x4f8850) and every category written into the current slot of all three rings.
/// The release then sends the GUI event 0x277 (not simulation).
pub fn trigger(world: &mut World) {
    let a = &mut world.statistic.archive;
    for (k, r) in a.rings.iter_mut().enumerate() {
        r.step(k == 0);
    }
    for player in 1..=world.player_count() {
        land_share(world);
        let Some(p) = world.statistic.players.get(player as usize) else { continue };
        let values: Vec<i32> = (0..CATEGORIES).map(|c| category_value(p, c)).collect();
        for r in world.statistic.archive.rings.iter_mut() {
            let slot = r.index.max(0) as usize;
            for (c, &v) in values.iter().enumerate() {
                r.data[(player as usize * CATEGORIES + c) * SAMPLES + slot] = v;
            }
        }
    }
}
