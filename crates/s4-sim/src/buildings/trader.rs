//! `CInternationalTrader` (vtable 0x10303ec: +0 load 0x4efea0, +4 store 0x4f0080): the foreign
//! trade's per-player table (release .data 0x14865b0). Own reverse engineering; decompiles in
//! `tools/re/trade/foreign.c` / `foreign2.c`.
//!
//! Every `CTradingBuildingRole` holds a handle (+0x2a4, ctor 0x5517a0: `{vtbl, player, 1}`) to
//! this table, made once per game (0x4efd50, flag 0x12941e5) with one entry per player
//! `0..=PlayerCount`. An entry (0x20 bytes) has two lists of building records (ports and
//! marketplaces), the player (+0x10) and nine "trade with player p" bools (+0x14 + p). A record
//! (0x160 bytes) is `{building, type 32 / 33, 43 × (good, state)}`; the state of a good is 0,
//! [`state::IMPORT`] (4) or [`state::EXPORT`] (8), the same bits as the role's +0x1d6 flags
//! (command 5008, 0x5530f0). `Init` (0x5524d0 → 0x4f0460) appends the building's record, the
//! crush (0x5570b0 → 0x4f0530) removes it. The table is saved in chunk 167 ([`parse_chunk`]).

use crate::World;
use crate::entity::{EntityData, ObjType};
use crate::hash::StateHasher;

/// Goods per record.
pub const GOODS: usize = 43;
/// `BUILDING_PORT` / `BUILDING_MARKETPLACE`.
const PORT: u16 = 32;
const MARKET: u16 = 33;

/// The per-good states of a record.
pub mod state {
    pub const IMPORT: u8 = 4;
    pub const EXPORT: u8 = 8;
}

/// One building record (0x160 bytes).
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Record {
    pub building: u16,
    pub kind: u16,
    /// `(good, state)` per good index; the good field is only written by 5008 (0 before).
    #[cfg_attr(feature = "serde", serde(with = "serde_big_array::BigArray"))]
    pub goods: [(u8, u8); GOODS],
}

impl Record {
    fn new(building: u16, kind: u16) -> Record {
        Record { building, kind, goods: [(0, 0); GOODS] }
    }
}

/// One player's entry.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Trader {
    /// +0x10.
    pub player: u8,
    /// +0x14 + p: the player trades with player p (commands 5007 / 5050).
    pub trade_with: [bool; 9],
    /// The two lists, in insertion order.
    pub ports: Vec<Record>,
    pub markets: Vec<Record>,
}

impl Trader {
    fn list(&self, kind: u16) -> &[Record] {
        match kind {
            PORT => &self.ports,
            MARKET => &self.markets,
            _ => &[],
        }
    }

    /// 0x4f0630: the record of building `b` (of type `kind`).
    pub fn record(&self, b: u16, kind: u16) -> Option<&Record> {
        self.list(kind).iter().find(|r| r.building == b)
    }

    fn record_mut(&mut self, b: u16, kind: u16) -> Option<&mut Record> {
        match kind {
            PORT => self.ports.iter_mut().find(|r| r.building == b),
            MARKET => self.markets.iter_mut().find(|r| r.building == b),
            _ => None,
        }
    }

    /// 0x4f06a0: `(building, good index)` of the export goods of building `b`.
    pub fn exports_of(&self, b: u16, kind: u16) -> Vec<(u16, u8)> {
        self.record(b, kind).map_or(Vec::new(), |r| (0..GOODS).filter(|&g| r.goods[g].1 == state::EXPORT).map(|g| (b, r.goods[g].0)).collect())
    }

    /// 0x4f0770 / 0x4f0870: `(building, good index)` of every record of type `kind` with the good
    /// in `st` (the original gives the index, not the record's good field).
    pub fn all_in_state(&self, kind: u16, st: u8) -> Vec<(u16, u8)> {
        let mut v = Vec::new();
        for r in self.list(kind) {
            for g in 0..GOODS {
                if r.goods[g].1 == st {
                    v.push((r.building, g as u8));
                }
            }
        }
        v
    }

    /// Any of the nine trade-with bools.
    pub fn trades_with_anyone(&self) -> bool {
        self.trade_with.iter().any(|&b| b)
    }
}

/// The table and its "made" flag (0x12941e5).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Traders {
    pub made: bool,
    /// Index = player `0..=PlayerCount`.
    pub players: Vec<Trader>,
}

impl Traders {
    pub fn get(&self, player: u8) -> Option<&Trader> {
        self.players.get(player as usize)
    }

    pub fn get_mut(&mut self, player: u8) -> Option<&mut Trader> {
        self.players.get_mut(player as usize)
    }

    pub(crate) fn hash(&self, h: &mut StateHasher) {
        h.bool(self.made);
        h.u32(self.players.len() as u32);
        for t in &self.players {
            h.u8(t.player);
            t.trade_with.iter().for_each(|&b| h.bool(b));
            for l in [&t.ports, &t.markets] {
                h.u32(l.len() as u32);
                for r in l {
                    h.u16(r.building);
                    h.u16(r.kind);
                    for (g, s) in r.goods {
                        h.u8(g);
                        h.u8(s);
                    }
                }
            }
        }
    }
}

/// `PlayerCount`: the highest player id in the game.
fn player_count(world: &World) -> u8 {
    world.players.iter().enumerate().filter(|(_, p)| p.is_some()).map(|(i, _)| i as u8).max().unwrap_or(0)
}

/// 0x4efd50: make the table once (entries `0..=PlayerCount`, player = index, nothing allowed).
pub fn ensure(world: &mut World) {
    if world.buildings.traders.made {
        return;
    }
    let n = player_count(world);
    world.buildings.traders.players = (0..=n).map(|p| Trader { player: p, ..Trader::default() }).collect();
    world.buildings.traders.made = true;
}

fn building(world: &World, id: u16) -> Option<(u8, u16)> {
    let e = world.entities.by_index(id as u32)?;
    (e.obj_type == ObjType::Building).then_some((e.owner, e.kind))
}

/// 0x4f0460 (from `Init` / the owner change 0x5520c0): the owner's entry gets a record of the
/// building (ports and marketplaces only), all goods 0.
pub fn add_building(world: &mut World, id: u16) {
    let Some((owner, kind)) = building(world, id) else { return };
    let Some(t) = world.buildings.traders.get_mut(owner) else { return };
    t.player = owner;
    match kind {
        PORT => t.ports.push(Record::new(id, kind)),
        MARKET => t.markets.push(Record::new(id, kind)),
        _ => {}
    }
}

/// 0x4f0530 (crush): the owner's entry forgets the building's record.
pub fn remove_building(world: &mut World, id: u16) {
    let Some((owner, kind)) = building(world, id) else { return };
    let Some(t) = world.buildings.traders.get_mut(owner) else { return };
    match kind {
        PORT => {
            if let Some(k) = t.ports.iter().position(|r| r.building == id) {
                t.ports.remove(k);
            }
        }
        MARKET => {
            if let Some(k) = t.markets.iter().position(|r| r.building == id) {
                t.markets.remove(k);
            }
        }
        _ => {}
    }
}

/// The owner's entry of building `id` (`table[owner]`).
pub fn trader_of(world: &World, id: u16) -> Option<&Trader> {
    let (owner, _) = building(world, id)?;
    world.buildings.traders.get(owner)
}

/// 0x4f0630 + the 5008 write: the record of building `id` takes `(good, st)` for the good.
pub fn set_good_state(world: &mut World, id: u16, good: usize, st: u8) {
    let Some((owner, kind)) = building(world, id) else { return };
    if good >= GOODS {
        return;
    }
    if let Some(r) = world.buildings.traders.get_mut(owner).and_then(|t| t.record_mut(id, kind)) {
        r.goods[good] = (good as u8, st);
    }
}

/// 0x4ef7b0: the export goods of building `b` in its owner's entry.
pub fn exports_of(world: &World, b: u16) -> Vec<(u16, u8)> {
    let Some((_, kind)) = building(world, b) else { return Vec::new() };
    trader_of(world, b).map_or(Vec::new(), |t| t.exports_of(b, kind))
}

/// 0x4f02f0: two buildings of the same type whose places are in the same sector (a port's
/// working area +0x4c, a marketplace's ensign +0x48).
pub fn same_sector(world: &World, a: u16, b: u16) -> bool {
    let place = |id: u16| -> Option<(u16, crate::grid::Y16X16)> {
        let e = world.entities.by_index(id as u32)?;
        let EntityData::Building(d) = &e.data else { return None };
        Some((e.kind, if e.kind == PORT { d.working_area } else { d.ensign }))
    };
    let (Some((ka, pa)), Some((kb, pb))) = (place(a), place(b)) else { return false };
    if ka != kb {
        return false;
    }
    let sector = |p: crate::grid::Y16X16| world.grid.checked_index(p.x(), p.y()).map(|n| world.tiling.sector_of(n));
    sector(pa).is_some() && sector(pa) == sector(pb)
}

/// 0x4efaa0: building `a` exports a good the owner of `b` imports (in a record of `a`'s type).
pub fn still_trades(world: &World, a: u16, b: u16) -> bool {
    let (Some((_, kind)), Some(_)) = (building(world, a), building(world, b)) else { return false };
    let Some(tb) = trader_of(world, b) else { return false };
    let imports = tb.all_in_state(kind, state::IMPORT);
    exports_of(world, a).iter().any(|e| imports.iter().any(|i| i.1 == e.1))
}

/// 0x4efbf0: as [`still_trades`] for the one good `good`.
pub fn trades_good(world: &World, a: u16, b: u16, good: u8) -> bool {
    let (Some((_, kind)), Some(_)) = (building(world, a), building(world, b)) else { return false };
    let Some(tb) = trader_of(world, b) else { return false };
    let imports = tb.all_in_state(kind, state::IMPORT);
    exports_of(world, a).iter().any(|e| e.1 == good && imports.iter().any(|i| i.1 == good))
}

// ---- chunk 167 ----

/// Chunk 167 (`CInternationalTrader::Store` 0x4f0080, written at 0x4ff2f2): magic 0x2d6ebb9b,
/// version 1, the entry count, then per entry the nine bools, the port records and the
/// marketplace records (each a count and 352-byte records). `None` unless it reads exactly.
pub fn parse_chunk(d: &[u8]) -> Option<Vec<Trader>> {
    let u32_at = |o: usize| -> Option<u32> { Some(u32::from_le_bytes(d.get(o..o + 4)?.try_into().ok()?)) };
    if u32_at(0)? != 0x2d6e_bb9b || u32_at(4)? != 1 {
        return None;
    }
    let n = u32_at(8)? as usize;
    let mut o = 12;
    let mut out = Vec::new();
    for p in 0..n {
        let mut t = Trader { player: p as u8, ..Trader::default() };
        for (k, b) in d.get(o..o + 9)?.iter().enumerate() {
            t.trade_with[k] = *b != 0;
        }
        o += 9;
        for list in 0..2 {
            let c = u32_at(o)? as usize;
            o += 4;
            for _ in 0..c {
                let mut r = Record::new(u32_at(o)? as u16, u32_at(o + 4)? as u16);
                for g in 0..GOODS {
                    r.goods[g] = (u32_at(o + 8 + 8 * g)? as u8, u32_at(o + 12 + 8 * g)? as u8);
                }
                o += 352;
                if list == 0 { &mut t.ports } else { &mut t.markets }.push(r);
            }
        }
        out.push(t);
    }
    (o == d.len()).then_some(out)
}
