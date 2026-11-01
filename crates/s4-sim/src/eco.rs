//! The economy: `CEcoSectorMgr` / `CEcoSector` (Logic/EcoSector/EcoSectorMgr.cpp, EcoSector.cpp).
//!
//! An eco-sector is the connected land of one owner (the tiling decides which nodes; see
//! `tiling::EcoNote`). Its record keeps per-type settler counts, the offer lists of free settlers
//! and offered piles (heads here, the links in the entities' `next` / `prev`), goods counts and
//! the transport bookkeeping. Ids are the tiling's eco-sector ids (the tiling's slot allocation is
//! `CEcoSectorMgr::GetFreeSlot`); the node count is the tiling's `eco_sizes` (both are kept by the
//! same per-node notifications in the original).
//!
//! Checked against the shipped exe where the structure matters: `ConnectEconomy` (release
//! 0x4defc0) keys the moved pile offers by `IEntity::Type`, which is 0 for piles (the `CPile`
//! constructor 0x56efb0 never sets `m_nType`), so merged pile offers land in list 0 — copied.

use std::collections::VecDeque;

use crate::entity::{EntityData, ObjType, flag};
use crate::hash::StateHasher;
use crate::tiling::EcoNote;
use crate::world::World;

/// `TILING_ECO_SECTOR_MAX`.
pub const ECO_MAX: usize = 0x4000;
/// `GOOD_MAX`.
pub const GOODS: usize = 43;
/// `SETTLER_MAX`.
pub const SETTLER_TYPES: usize = 67;
/// `ISettlerRole` type whose settlers are not counted (`GetRoleType() != 18` checks).
pub const ROLE_UNCOUNTED: u8 = 18;
/// `IEntity` flag 0x80, set on a non-warrior whose node left every eco-sector.
pub const FLAG_LEFT_ECO: u32 = 0x80;

/// `CEcoSector` (offsets: the release layout where verified against the shipped exe (ctor
/// 0x4d2590); others are unverified debug-build hints).
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EcoSector {
    /// +12 `m_iUniqueId` (from 0x10000000).
    pub unique_id: u32,
    /// +16 eco-sector id.
    pub id: u16,
    /// +18 owner.
    pub owner: u8,
    /// +19 (stored, use unknown).
    pub byte19: u8,
    /// +20 building sites with priority.
    pub sites_with_prio: u8,
    /// +21 / +22 sites with digger / builder priority.
    pub digger_prio_sites: i8,
    pub builder_prio_sites: i8,
    /// +24 / +26 diggers / builders those sites need.
    pub needed_prio_diggers: i16,
    pub needed_prio_builders: i16,
    /// +28 / +29 sites with board / stone priority.
    pub prio_boards: u8,
    pub prio_stone: u8,
    /// +30 extra carriers kept back from recruiting (0..=99).
    pub extra_min_carrier: u8,
    /// +32 carriers never recruited (5..=999).
    pub min_carrier: u16,
    /// +34 carriers on their way to become a specialist / worker.
    pub carriers_with_job_order: u16,
    /// +36 settlers per type.
    #[cfg_attr(feature = "serde", serde(with = "serde_big_array::BigArray"))]
    pub settlers: [u16; SETTLER_TYPES],
    /// +170 offer list heads of free settlers per type.
    #[cfg_attr(feature = "serde", serde(with = "serde_big_array::BigArray"))]
    pub settler_offer: [u16; SETTLER_TYPES],
    /// +304 offer list heads of piles per good.
    #[cfg_attr(feature = "serde", serde(with = "serde_big_array::BigArray"))]
    pub good_offer: [u16; GOODS],
    /// Buildings waiting for a good (`RequestGood` queues, per good).
    pub requests: Vec<VecDeque<u16>>,
    /// +472 supply counters per building type (`InitTransport`).
    #[cfg_attr(feature = "serde", serde(with = "serde_big_array::BigArray"))]
    pub supply_counter: [u16; 42],
    /// Specialist wishes, worker deltas (offsets unverified, debug-build hint).
    pub specialist_wish: [i8; 5],
    pub worker_delta: [i8; 2],
    /// `GetFullPile` stacks, per good.
    pub full_piles: Vec<Vec<u16>>,
    /// +0x290 goods lying on piles, +0x2e6 goods carried, per good (release: ctor 0x4d2590,
    /// `GetNumberOfGoods` 0x4df820).
    #[cfg_attr(feature = "serde", serde(with = "serde_big_array::BigArray"))]
    pub goods_on_piles: [u16; GOODS],
    #[cfg_attr(feature = "serde", serde(with = "serde_big_array::BigArray"))]
    pub goods_on_carriers: [u16; GOODS],
    /// +0x33c soldier wishes (ten soldier types, table 0x1164b10; `ChangeSoldierWish` 0x4d68a0).
    pub soldier_orders: [i8; 10],
    /// +0x348 tool order counts and +0x354 the tools (the toolsmith's (21) product piles in
    /// building-info order); +0x360 weapon auto-production (100 = on, the ctor default; 0 off);
    /// +0x364 weapon order counts, +0x370 the weapons (the weaponsmith's (22) products),
    /// +0x37c weapon percentages (50/50/0(/0)), +0x388..+0x394 weapons made by percentage.
    /// A count of 100 (`'d'`) means unlimited.
    pub tool_orders: Vec<u8>,
    pub tools: Vec<u8>,
    pub weapon_auto: u8,
    pub weapon_orders: Vec<u8>,
    pub weapons: Vec<u8>,
    pub weapon_percent: Vec<u8>,
    pub weapons_made: [u32; 4],
    /// +572 `CGoodTransportPriority`: rank per good (1 = first, 0 = not transported), the number of
    /// ranks, and its +96 flag.
    #[cfg_attr(feature = "serde", serde(with = "serde_big_array::BigArray"))]
    pub transport_prio: [u16; GOODS],
    pub transport_prio_count: i32,
    pub transport_prio_flag: bool,
    /// +556 `CBuildingSupplyPriority`: building type → good → share.
    pub supply: SupplyMap,
    /// +424 free storage piles (building, pile), +440 storage piles holding a good (building,
    /// pile, good), +456 goods a storage building takes (building, good). Unregistering moves
    /// the last entry into the hole.
    pub free_storage_piles: Vec<(u16, u16)>,
    pub good_storage_piles: Vec<(u16, u16, u8)>,
    pub storage_goods: Vec<(u16, u8)>,
}

/// `CBuildingSupplyPriority`'s `map<int, map<int, int>>`: building type → good → share (%).
pub type SupplyMap = std::collections::BTreeMap<u8, std::collections::BTreeMap<u8, i32>>;

impl EcoSector {
    /// `CEcoSector::CEcoSector(owner, id)` (release 0x4d2590): `weapons` / `tools` are the
    /// weaponsmith's and toolsmith's product goods of the owner's race.
    fn new(owner: u8, id: u16, unique_id: u32, race: u8, supply: SupplyMap, weapons: Vec<u8>, tools: Vec<u8>) -> EcoSector {
        let (transport_prio, transport_prio_count) = default_transport_priority(race);
        EcoSector {
            unique_id,
            id,
            owner,
            byte19: 0,
            sites_with_prio: 0,
            digger_prio_sites: 0,
            builder_prio_sites: 0,
            needed_prio_diggers: 0,
            needed_prio_builders: 0,
            prio_boards: 0,
            prio_stone: 0,
            extra_min_carrier: 0,
            min_carrier: 5,
            carriers_with_job_order: 0,
            settlers: [0; SETTLER_TYPES],
            settler_offer: [0; SETTLER_TYPES],
            good_offer: [0; GOODS],
            requests: vec![VecDeque::new(); GOODS],
            supply_counter: [0; 42],
            specialist_wish: [0; 5],
            worker_delta: [0; 2],
            full_piles: Vec::new(),
            goods_on_piles: [0; GOODS],
            goods_on_carriers: [0; GOODS],
            soldier_orders: [0; 10],
            tool_orders: vec![0; tools.len()],
            tools,
            weapon_auto: 100,
            weapon_orders: vec![0; weapons.len()],
            weapon_percent: (0..weapons.len()).map(|i| [50, 50, 0, 0].get(i).copied().unwrap_or(0)).collect(),
            weapons,
            weapons_made: [0; 4],
            transport_prio,
            transport_prio_count,
            transport_prio_flag: true,
            supply,
            free_storage_piles: Vec::new(),
            good_storage_piles: Vec::new(),
            storage_goods: Vec::new(),
        }
    }

    fn hash(&self, h: &mut StateHasher) {
        h.u32(self.unique_id);
        h.u16(self.id);
        for v in [self.owner, self.byte19, self.sites_with_prio, self.digger_prio_sites as u8, self.builder_prio_sites as u8] {
            h.u8(v);
        }
        h.u16(self.needed_prio_diggers as u16);
        h.u16(self.needed_prio_builders as u16);
        for v in [self.prio_boards, self.prio_stone, self.extra_min_carrier] {
            h.u8(v);
        }
        h.u16(self.min_carrier);
        h.u16(self.carriers_with_job_order);
        h.u16s(&self.settlers);
        h.u16s(&self.settler_offer);
        h.u16s(&self.good_offer);
        for q in &self.requests {
            h.u32(q.len() as u32);
            q.iter().for_each(|&b| h.u16(b));
        }
        h.u16s(&self.supply_counter);
        self.specialist_wish.iter().chain(&self.worker_delta).chain(&self.soldier_orders).for_each(|&v| h.u8(v as u8));
        h.u16s(&self.transport_prio);
        h.u32(self.transport_prio_count as u32);
        h.bool(self.transport_prio_flag);
        h.u32(self.free_storage_piles.len() as u32);
        self.free_storage_piles.iter().for_each(|&(b, p)| {
            h.u16(b);
            h.u16(p);
        });
        h.u32(self.good_storage_piles.len() as u32);
        self.good_storage_piles.iter().for_each(|&(b, p, g)| {
            h.u16(b);
            h.u16(p);
            h.u8(g);
        });
        h.u32(self.storage_goods.len() as u32);
        self.storage_goods.iter().for_each(|&(b, g)| {
            h.u16(b);
            h.u8(g);
        });
        for (&b, m) in &self.supply {
            h.u8(b);
            for (&g, &v) in m {
                h.u8(g);
                h.u32(v as u32);
            }
        }
        h.u32(self.full_piles.len() as u32);
        self.full_piles.iter().for_each(|v| h.u16s(v));
        h.u16s(&self.goods_on_piles);
        h.u16s(&self.goods_on_carriers);
        for v in [&self.tool_orders, &self.tools, &self.weapon_orders, &self.weapons, &self.weapon_percent] {
            h.u32(v.len() as u32);
            v.iter().for_each(|&b| h.u8(b));
        }
        h.u8(self.weapon_auto);
        self.weapons_made.iter().for_each(|&m| h.u32(m));
    }
}

/// `CEcoSectorMgr`.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Economy {
    /// +20 records by id.
    pub sectors: Vec<Option<Box<EcoSector>>>,
    /// +0 set by `NotifyEcoSectorMerge` until `ConnectEconomy`.
    pub merging: bool,
    /// +8 / +12 last used / highest ever used id.
    pub last_used: u16,
    pub max_used: u16,
    /// +16 next unique id.
    pub next_unique: u32,
    /// `CPlayerGameData` eco-sector id lists, per player (index 0 unused).
    pub player_ecos: Vec<Vec<u16>>,
    /// +65556 `Update` calls since the last `UpdateStrikeSettlers` (every 32).
    pub strike_counter: i32,
    /// +65560 `GetNrOfInitialFreeBeds` per player (`CalculateInitialFreeBeds`).
    pub initial_free_beds: [i32; 9],
    /// +65596 the striking carriers per player (index 0 unused).
    pub strikers: Vec<Vec<u16>>,
}

impl Default for Economy {
    fn default() -> Self {
        Economy {
            sectors: vec![None; ECO_MAX],
            merging: false,
            last_used: 0,
            max_used: 0,
            next_unique: 0x1000_0000,
            player_ecos: vec![Vec::new(); 9],
            strike_counter: 0,
            initial_free_beds: [0; 9],
            strikers: vec![Vec::new(); 9],
        }
    }
}

impl Economy {
    /// `CEcoSectorMgr::GetEcoSectorPtr` (also guards id 0 and out-of-range ids).
    pub fn get(&self, id: u16) -> Option<&EcoSector> {
        self.sectors.get(id as usize)?.as_deref()
    }

    pub fn get_mut(&mut self, id: u16) -> Option<&mut EcoSector> {
        self.sectors.get_mut(id as usize)?.as_deref_mut()
    }

    /// `CEcoSector::Owner` (0 for no eco-sector).
    pub fn owner(&self, id: u16) -> u8 {
        self.get(id).map_or(0, |e| e.owner)
    }

    pub(crate) fn hash(&self, h: &mut StateHasher) {
        h.bool(self.merging);
        h.u16(self.last_used);
        h.u16(self.max_used);
        h.u32(self.next_unique);
        for (i, e) in self.sectors.iter().enumerate() {
            if let Some(e) = e {
                h.u16(i as u16);
                e.hash(h);
            }
        }
        self.player_ecos.iter().for_each(|v| h.u16s(v));
        h.u32(self.strike_counter as u32);
        self.initial_free_beds.iter().for_each(|&b| h.u32(b as u32));
        self.strikers.iter().for_each(|v| h.u16s(v));
    }

    /// `CEcoSectorMgr::NewEcoSector(owner)` at the slot the tiling chose (+ `RegisterEcoSector`,
    /// `CPlayerGameData::AddEcoSectorId`).
    fn create(&mut self, id: u16, owner: u8, race: u8, supply: SupplyMap, weapons: Vec<u8>, tools: Vec<u8>) {
        let uid = self.next_unique;
        self.next_unique += 1;
        if let Some(s) = self.sectors.get_mut(id as usize) {
            *s = Some(Box::new(EcoSector::new(owner, id, uid, race, supply, weapons, tools)));
        }
        self.last_used = self.last_used.max(id);
        self.max_used = self.max_used.max(id);
        if let Some(v) = self.player_ecos.get_mut(owner as usize) {
            v.push(id);
        }
    }

    /// `CEcoSectorMgr::DeleteEcoSector` → `DestroyEcoSector` + `DeleteEcoSectorId`.
    fn delete(&mut self, id: u16) {
        let Some(e) = self.sectors.get_mut(id as usize).and_then(Option::take) else { return };
        let mut last = self.last_used;
        while last > 0 && self.sectors[last as usize].is_none() {
            last -= 1;
        }
        self.last_used = last;
        if let Some(v) = self.player_ecos.get_mut(e.owner as usize) {
            v.retain(|&x| x != id);
        }
    }
}

/// `ITiling::EcoSectorId(node)` (0 off the map or on unowned land).
pub fn eco_of(w: &World, node: usize) -> u16 {
    let t = w.tiling.normal.get(node).copied().unwrap_or(0);
    w.tiling.tiles.get(t as usize).map_or(0, |t| t.eco_sector)
}

/// The tiling's queued notifications, in order (see `tiling::EcoNote`).
pub fn apply_tiling_notes(w: &mut World) {
    let notes = std::mem::take(&mut w.tiling.eco_notes);
    for n in notes {
        match n {
            EcoNote::Create { id, owner } => {
                let race = w.race_of(owner);
                let supply = w.rules.supply.get(race as usize).cloned().unwrap_or_default();
                let products = |t: u8| -> Vec<u8> { w.rules.building(race, t).piles.iter().filter(|p| p.kind == 0).map(|p| p.good).collect() };
                let (weapons, tools) = (products(22), products(21));
                w.economy.create(id, owner, race, supply, weapons, tools);
                // `NewEcoSector` (0x4dec50): AI notice 0xd (owner, id, unique id).
                let uid = w.economy.get(id).map_or(0, |e| e.unique_id);
                crate::ai::post_event(w, 0xd, owner as i32, id as i32, uid as i32);
            }
            EcoNote::Delete { id } => {
                // `DeleteEcoSector` (0x4ded30): AI notice 0xe once it is gone.
                let info = w.economy.get(id).map(|e| (e.owner, e.unique_id));
                w.economy.delete(id);
                if let Some((owner, uid)) = info {
                    crate::ai::post_event(w, 0xe, owner as i32, id as i32, uid as i32);
                }
            }
            EcoNote::Node { node, old, new } => notify_change(w, node as usize, old, new),
            EcoNote::MergeStart { .. } => w.economy.merging = true,
            EcoNote::Merged { keep, gone } => connect_economy(w, keep, gone),
            EcoNote::Split { old, new } => separate_economy(w, old, new),
        }
    }
}

/// The settler (`CWorldManager::SettlerId`) on a node.
fn settler_at(w: &World, node: usize) -> u16 {
    let id = w.layers.map_object.get(node).copied().unwrap_or(0);
    match w.entities.by_index(id as u32) {
        Some(e) if e.obj_type == ObjType::Settler => id,
        _ => 0,
    }
}

/// `CWorldManager::PileId` on a node.
fn pile_at(w: &World, node: usize) -> u16 {
    let id = w.layers.deco_object.get(node).copied().unwrap_or(0);
    match w.entities.by_index(id as u32) {
        Some(e) if e.obj_type == ObjType::Pile => id,
        _ => 0,
    }
}

/// (type, warrior type, flags, role type, next settler type) of a settler.
fn settler_info(w: &World, s: u16) -> Option<(u16, u8, u32, u8, u8)> {
    let e = w.entities.by_index(s as u32)?;
    let EntityData::Settler(d) = &e.data else { return None };
    Some((e.kind, e.warrior_type(), e.flags, d.job.role, d.job.carrier.next_type))
}

/// `CEcoSectorMgr::NotifyChangeEcoSector(node, old, new)` for a node holding something.
fn notify_change(w: &mut World, node: usize, old: u16, new: u16) {
    let pile = pile_at(w, node);
    if new != 0 {
        if old == 0 {
            if pile != 0 {
                crate::pile::occupied(w, pile, new);
            }
            return;
        }
        let old_owner = w.economy.owner(old);
        if old_owner != w.economy.owner(new) {
            return;
        }
        if pile != 0 {
            crate::pile::notify_change_eco_sector(w, pile, old, new);
        }
        if w.economy.merging {
            return; // merge: ConnectEconomy moves the offers afterwards
        }
        // Split: the settler moves to the new eco-sector.
        let s = settler_at(w, node);
        let Some((kind, warrior, flags, role, next_type)) = settler_info(w, s) else { return };
        let owner = w.entities.by_index(s as u32).map_or(0, |e| e.owner);
        if owner != old_owner || warrior != 0 {
            return;
        }
        if role != ROLE_UNCOUNTED {
            change_nr_of_settler(w, new, kind, 1);
            change_nr_of_settler(w, old, kind, -1);
        }
        if next_type != 0 {
            if let Some(e) = w.economy.get_mut(old) {
                e.carriers_with_job_order = e.carriers_with_job_order.saturating_sub(1);
            }
            if let Some(e) = w.economy.get_mut(new) {
                e.carriers_with_job_order += 1;
            }
        }
        if flags & flag::OFFERED != 0 {
            get_settler_out_of_offer(w, old, s);
            set_settler_offer(w, new, kind, s);
        }
        return;
    }
    if pile != 0 {
        crate::pile::set_free(w, pile, old);
    }
    let s = settler_at(w, node);
    let Some((kind, warrior, flags, role, next_type)) = settler_info(w, s) else { return };
    if warrior != 0 {
        return;
    }
    if flags & flag::OFFERED != 0 {
        get_settler_out_of_offer(w, old, s);
    }
    if role != ROLE_UNCOUNTED {
        change_nr_of_settler(w, old, kind, -1);
    }
    if next_type != 0 {
        reset_carrier_next_settler_type_and_decrement_order(w, old, s);
    }
    if let Some(e) = w.entities.by_index_mut(s as u32) {
        e.flags |= FLAG_LEFT_ECO;
    }
}

/// `CEcoSectorMgr::ConnectEconomy(keep, gone)` (release 0x4defc0): `gone` was merged into `keep`.
/// `keep`'s soldier wishes, weapon and tool orders (index by index) are raised to `gone`'s where
/// those are larger, through the change functions with the difference; for tools the original
/// also calls with difference 0 when both are equal, which clears the count (kept).
fn connect_economy(w: &mut World, keep: u16, gone: u16) {
    w.economy.merging = false;
    // AI notice 0xf (keep's owner, keep, gone).
    let owner = w.economy.owner(keep);
    crate::ai::post_event(w, 0xf, owner as i32, keep as i32, gone as i32);
    let Some(g) = w.economy.get(gone).cloned() else { return };
    if let Some(k) = w.economy.get_mut(keep) {
        k.sites_with_prio = k.sites_with_prio.wrapping_add(g.sites_with_prio);
        k.digger_prio_sites = k.digger_prio_sites.wrapping_add(g.digger_prio_sites);
        k.needed_prio_diggers = k.needed_prio_diggers.wrapping_add(g.needed_prio_diggers);
        k.builder_prio_sites = k.builder_prio_sites.wrapping_add(g.builder_prio_sites);
        k.needed_prio_builders = k.needed_prio_builders.wrapping_add(g.needed_prio_builders);
        k.prio_boards = k.prio_boards.wrapping_add(g.prio_boards);
        k.prio_stone = k.prio_stone.wrapping_add(g.prio_stone);
    }
    connect_orders(w, keep, &g);
    // Offers: walk gone's lists, re-offer each entity in keep under its `IEntity::Type`.
    for (pile, heads) in [(false, &g.settler_offer[..]), (true, &g.good_offer[..])] {
        for &head in heads {
            let mut id = head;
            while id != 0 {
                let Some(e) = w.entities.by_index_mut(id as u32) else { break };
                let next = e.next;
                e.flags &= !flag::OFFERED;
                // 0x4df242: the slot is `IEntity +0xc`, a pile's good type (our pile entities
                // keep the good in their data, kind 0).
                let kind = match &e.data {
                    EntityData::Pile(p) => p.good as u16,
                    _ => e.kind,
                };
                link_offer(w, keep, id, kind as usize, pile);
                id = next;
            }
        }
    }
    if let Some(k) = w.economy.get_mut(keep) {
        for i in 0..SETTLER_TYPES {
            k.settlers[i] = k.settlers[i].wrapping_add(g.settlers[i]);
        }
    }
}

/// The order moves of `ConnectEconomy` (release 0x4df048..0x4df184), run after the counters.
fn connect_orders(w: &mut World, keep: u16, g: &EcoSector) {
    let Some(k) = w.economy.get(keep).cloned() else { return };
    for i in 0..10 {
        let (kv, gv) = (k.soldier_orders[i] as i32, g.soldier_orders[i] as i32);
        if kv.max(gv) == gv && gv != kv {
            let t = SOLDIER_WISH_TYPES[if i == 9 { 15 } else { i }].0;
            change_soldier_wish(w, keep, t, gv - kv);
        }
    }
    for (i, &t) in k.weapons.iter().enumerate() {
        let (kv, gv) = (k.weapon_orders[i] as i32, g.weapon_orders.get(i).copied().unwrap_or(0) as i32);
        if kv.max(gv) == gv && gv != kv {
            change_weapon_order(w, keep, t, gv - kv);
        }
    }
    for (i, &t) in k.tools.iter().enumerate() {
        let (kv, gv) = (k.tool_orders[i] as i32, g.tool_orders.get(i).copied().unwrap_or(0) as i32);
        if kv.max(gv) == gv {
            change_tool_order(w, keep, t, gv - kv);
        }
    }
}

/// The soldier-wish table at .data 0x1164b10: (settler type, wish slot at +0x33c), ended by
/// (0, -1).
pub const SOLDIER_WISH_TYPES: [(u16, u8); 19] = [
    (29, 0), (30, 1), (31, 2), (32, 3), (33, 4), (34, 5), (35, 6), (36, 7), (37, 8), (38, 6), (39, 7), (40, 8),
    (41, 6), (42, 7), (43, 8), (44, 9), (61, 6), (62, 7), (63, 8),
];

/// `CEcoSector::ChangeSoldierWish(type, delta)` (release 0x4d68a0): type 0x43 with delta 0
/// clears all ten; a listed type moves its slot like an order count (0 clears, 100 unlimited,
/// clamped to 0..=100). (An unlisted type writes the byte before the wishes in the original;
/// ignored here.)
pub fn change_soldier_wish(w: &mut World, eco: u16, kind: u16, delta: i32) {
    let Some(e) = w.economy.get_mut(eco) else { return };
    if kind == 0x43 {
        if delta == 0 {
            e.soldier_orders = [0; 10];
        }
        return;
    }
    let Some(&(_, slot)) = SOLDIER_WISH_TYPES.iter().find(|&&(t, _)| t == kind) else { return };
    let c = &mut e.soldier_orders[slot as usize];
    *c = if delta == 0 {
        0
    } else if delta == 100 {
        100
    } else if delta < 1 {
        if (*c as i32) + delta < 0 { 0 } else { (*c as i32 + delta) as i8 }
    } else if (*c as i32) + delta > 100 {
        100
    } else {
        (*c as i32 + delta) as i8
    };
}

/// A construction site's priority flags as `SeparateEconomy` reads them from the site role
/// (release offsets): +0x1b8 prioritised, +0x1b9 digger priority (needing +0x184 diggers),
/// +0x1ba builder priority (needing info +7 builders), +0x1bb boards, +0x1bc stone priority.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SitePriority {
    pub prio: bool,
    pub diggers: Option<i16>,
    pub builders: Option<i16>,
    pub boards: bool,
    pub stone: bool,
}

/// The priority flags of a site (`build::order::site_priority`).
fn site_priority(w: &World, site: u16) -> SitePriority {
    crate::build::order::site_priority(w, site)
}

/// `CEcoSectorMgr::SeparateEconomy(old, new)` (release 0x4ded90), after the tiling split `new`
/// off `old`. If `old` counted prioritised sites, its priority counters are cleared and
/// recounted from the owner's construction sites (BIRTH set, building types 1..=82) whose
/// ensign now lies in `old` or `new`.
/// Then, always, `new` gets `old`'s soldier wishes (the first ten entries of the soldier table:
/// slot 9 goes to type 38, i.e. slot 6, as in the original), weapon and tool orders through the
/// change functions. First of all the AI notice 0x10 (old's owner, new << 16 | old, new's
/// unique id).
fn separate_economy(w: &mut World, old: u16, new: u16) {
    let (owner, uid) = (w.economy.owner(old), w.economy.get(new).map_or(0, |e| e.unique_id));
    crate::ai::post_event(w, 0x10, owner as i32, ((new as i32) << 16) | old as i32, uid as i32);
    recount_site_priorities(w, old, new);
    let Some(o) = w.economy.get(old).cloned() else { return };
    for (&(t, _), &v) in SOLDIER_WISH_TYPES.iter().zip(&o.soldier_orders) {
        change_soldier_wish(w, new, t, v as i32);
    }
    for (i, &g) in o.weapons.iter().enumerate() {
        change_weapon_order(w, new, g, o.weapon_orders[i] as i32);
    }
    for (i, &g) in o.tools.iter().enumerate() {
        change_tool_order(w, new, g, o.tool_orders[i] as i32);
    }
}

fn recount_site_priorities(w: &mut World, old: u16, new: u16) {
    let Some(o) = w.economy.get_mut(old) else { return };
    if o.sites_with_prio == 0 {
        return;
    }
    let owner = o.owner;
    o.sites_with_prio = 0;
    o.digger_prio_sites = 0;
    o.builder_prio_sites = 0;
    o.needed_prio_diggers = 0;
    o.needed_prio_builders = 0;
    o.prio_boards = 0;
    o.prio_stone = 0;
    for t in 1..83u16 {
        let sites: Vec<(u16, usize)> = w
            .entities
            .iter()
            .filter(|(_, e)| e.obj_type == ObjType::Building && e.owner == owner && e.kind == t && e.flags & flag::BIRTH != 0)
            .filter_map(|(i, e)| match &e.data {
                EntityData::Building(b) => w.grid.checked_index(b.ensign.x(), b.ensign.y()).map(|n| (i, n)),
                _ => None,
            })
            .collect();
        for (site, node) in sites {
            let e = eco_of(w, node);
            if e != old && e != new {
                continue;
            }
            let p = site_priority(w, site);
            let Some(x) = w.economy.get_mut(e) else { continue };
            if p.prio {
                x.sites_with_prio = x.sites_with_prio.wrapping_add(1);
            }
            if let Some(n) = p.diggers {
                x.digger_prio_sites = x.digger_prio_sites.wrapping_add(1);
                x.needed_prio_diggers = x.needed_prio_diggers.wrapping_add(n);
            }
            if let Some(n) = p.builders {
                x.builder_prio_sites = x.builder_prio_sites.wrapping_add(1);
                x.needed_prio_builders = x.needed_prio_builders.wrapping_add(n);
            }
            if p.boards {
                x.prio_boards = x.prio_boards.wrapping_add(1);
            }
            if p.stone {
                x.prio_stone = x.prio_stone.wrapping_add(1);
            }
        }
    }
}

/// Head insertion into an offer list (`SetSettlerOffer` / `SetGoodOffer` body).
fn link_offer(w: &mut World, eco: u16, id: u16, slot: usize, pile: bool) {
    let Some(e) = w.economy.get_mut(eco) else { return };
    let heads: &mut [u16] = if pile { &mut e.good_offer } else { &mut e.settler_offer };
    let Some(head) = heads.get_mut(slot) else { return };
    let old = *head;
    *head = id;
    if let Some(x) = w.entities.by_index_mut(id as u32) {
        x.prev = 0;
        x.next = old;
        x.flags |= flag::OFFERED;
    }
    if old != 0
        && let Some(x) = w.entities.by_index_mut(old as u32)
    {
        x.prev = id;
    }
}

/// Unlink from an offer list (`GetSettlerOutOfOffer` / `GetPileOutOfOffer` body).
fn unlink_offer(w: &mut World, eco: u16, id: u16, slot: usize, pile: bool) {
    let Some(e) = w.entities.by_index(id as u32) else { return };
    let (prev, next) = (e.prev, e.next);
    if prev != 0 {
        if let Some(p) = w.entities.by_index_mut(prev as u32) {
            p.next = next;
        }
    } else {
        // The release (0x4d4180 / 0x4d4360) writes the head of `eco` unchecked. A pile can be
        // in another eco-sector's list: its node changes owner (A → 0 → B) before its building
        // is destroyed, and neither a production nor a delivery pile reacts to the owner change
        // (IPileRole vtbl+0x30 / +0x34 do nothing), so `SubjectDie` then cancels the offer in
        // B. [deviation] Unchecked, that cross-links the lists (the release defect; here it hung
        // `InitTransport`): the head written is the one of the list that has the entity at its
        // head (none: no head is written).
        let has = |e: &EcoSector| (if pile { &e.good_offer[..] } else { &e.settler_offer[..] }).get(slot) == Some(&id);
        let target = if w.economy.get(eco).is_some_and(has) { Some(eco) } else { (1..ECO_MAX as u16).find(|&e| w.economy.get(e).is_some_and(has)) };
        if let Some(s) = target.and_then(|t| w.economy.get_mut(t)) {
            let heads: &mut [u16] = if pile { &mut s.good_offer } else { &mut s.settler_offer };
            heads[slot] = next;
        }
    }
    if next != 0
        && let Some(n) = w.entities.by_index_mut(next as u32)
    {
        n.prev = prev;
    }
    if let Some(x) = w.entities.by_index_mut(id as u32) {
        x.next = 0;
        x.prev = 0;
        x.flags &= !flag::OFFERED;
    }
}

/// `CEcoSector::SetGoodOffer(good, pile)`.
pub fn set_good_offer(w: &mut World, eco: u16, good: u8, pile: u16) {
    link_offer(w, eco, pile, good as usize, true);
}

/// `CEcoSector::GetPileOutOfOffer(good, pile)`: unlinks; the head slot cleared is the pile's
/// current good (`GetGoodType`), as in the original.
pub fn get_pile_out_of_offer(w: &mut World, eco: u16, pile: u16) {
    let good = match w.entities.by_index(pile as u32).map(|e| &e.data) {
        Some(EntityData::Pile(p)) => p.good,
        _ => return,
    };
    unlink_offer(w, eco, pile, good as usize, true);
}

/// `CEcoSector::SetSettlerOffer(type, settler)`.
pub fn set_settler_offer(w: &mut World, eco: u16, kind: u16, settler: u16) {
    link_offer(w, eco, settler, kind as usize, false);
}

/// `CEcoSector::GetSettlerOutOfOffer(settler)`.
pub fn get_settler_out_of_offer(w: &mut World, eco: u16, settler: u16) {
    let Some(kind) = w.entities.by_index(settler as u32).map(|e| e.kind) else { return };
    unlink_offer(w, eco, settler, kind as usize, false);
}

/// `CEcoSector::ChangeNrOfSettler(type, delta)` (clamped to 0..=0xFFFF).
pub fn change_nr_of_settler(w: &mut World, eco: u16, kind: u16, delta: i32) {
    if let Some(e) = w.economy.get_mut(eco)
        && let Some(c) = e.settlers.get_mut(kind as usize)
    {
        *c = (*c as i32 + delta).clamp(0, 0xFFFF) as u16;
    }
}

/// `CEcoSector::ResetCarrierNextSettlerTypeAndDecrementOrder(settler)`.
pub fn reset_carrier_next_settler_type_and_decrement_order(w: &mut World, eco: u16, settler: u16) {
    if let Some(e) = w.entities.by_index_mut(settler as u32)
        && let EntityData::Settler(d) = &mut e.data
    {
        d.job.carrier.next_type = 0;
    }
    if let Some(e) = w.economy.get_mut(eco) {
        e.carriers_with_job_order = e.carriers_with_job_order.saturating_sub(1);
    }
}

/// `CEcoSector::ChangeNumberOfGoodsPile(good, delta)` (clamped at 0).
pub fn change_number_of_goods_pile(w: &mut World, eco: u16, good: u8, delta: i32) {
    if let Some(e) = w.economy.get_mut(eco)
        && (1..GOODS).contains(&(good as usize))
    {
        let c = &mut e.goods_on_piles[good as usize];
        *c = (*c as i32 + delta).max(0) as u16;
    }
}

/// `CEcoSector::ChangeNumberOfGoodsCarrier(good, delta)` (clamped at 0).
pub fn change_number_of_goods_carrier(w: &mut World, eco: u16, good: u8, delta: i32) {
    if let Some(e) = w.economy.get_mut(eco)
        && (1..GOODS).contains(&(good as usize))
    {
        let c = &mut e.goods_on_carriers[good as usize];
        *c = (*c as i32 + delta).max(0) as u16;
    }
}

/// `CEcoSector::RequestGood(building, good)`: queue the building's need (served by
/// `InitTransport`).
pub fn request_good(w: &mut World, eco: u16, building: u16, good: u8) {
    crate::debug::requested(w, eco, good, building);
    if let Some(e) = w.economy.get_mut(eco)
        && let Some(q) = e.requests.get_mut(good as usize)
    {
        q.push_back(building);
    }
}

/// `CEcoSector::RequestSpace(pile)`: a carrier takes a good of the pile to the nearest storage
/// pile that takes it.
pub fn request_space(w: &mut World, eco: u16, pile: u16) -> bool {
    if pile == 0 {
        return false;
    }
    let target = get_nearest_storage_pile(w, eco, pile);
    target != 0 && transport_good(w, eco, pile, target)
}

/// `CEcoSector::Register*` / `Unregister*`: no duplicates; the last entry fills the hole.
fn register<T: PartialEq + Copy>(v: &mut Vec<T>, x: T) -> bool {
    if v.contains(&x) {
        return false;
    }
    v.push(x);
    true
}

fn unregister<T: PartialEq + Copy>(v: &mut Vec<T>, x: T) -> bool {
    let Some(i) = v.iter().position(|&y| y == x) else { return false };
    v.swap_remove(i);
    true
}

pub fn register_free_storage_pile(w: &mut World, eco: u16, building: u16, pile: u16) -> bool {
    w.economy.get_mut(eco).is_some_and(|e| register(&mut e.free_storage_piles, (building, pile)))
}

pub fn unregister_free_storage_pile(w: &mut World, eco: u16, building: u16, pile: u16) -> bool {
    w.economy.get_mut(eco).is_some_and(|e| unregister(&mut e.free_storage_piles, (building, pile)))
}

pub fn register_good_storage_pile(w: &mut World, eco: u16, building: u16, pile: u16, good: u8) -> bool {
    w.economy.get_mut(eco).is_some_and(|e| register(&mut e.good_storage_piles, (building, pile, good)))
}

pub fn unregister_good_storage_pile(w: &mut World, eco: u16, building: u16, pile: u16, good: u8) -> bool {
    w.economy.get_mut(eco).is_some_and(|e| unregister(&mut e.good_storage_piles, (building, pile, good)))
}

pub fn register_good_storage(w: &mut World, eco: u16, building: u16, good: u8) -> bool {
    w.economy.get_mut(eco).is_some_and(|e| register(&mut e.storage_goods, (building, good)))
}

pub fn unregister_good_storage(w: &mut World, eco: u16, building: u16, good: u8) -> bool {
    w.economy.get_mut(eco).is_some_and(|e| unregister(&mut e.storage_goods, (building, good)))
}

/// `CEcoSector::GetNearestStoragePile(pile)`: over the storage buildings taking the pile's good,
/// the nearest pile of theirs with that good and room, else their first free pile; a free pile
/// chosen is reserved for the good (re-registered as a good pile, trade role 2).
pub fn get_nearest_storage_pile(w: &mut World, eco: u16, pile: u16) -> u16 {
    let Some(good) = crate::pile::get(w, pile).map(|p| p.good) else { return 0 };
    let (px, py) = pos_of(w, pile);
    let regs = w.economy.get(eco).map(|e| e.storage_goods.clone()).unwrap_or_default();
    let (mut best_d, mut best, mut best_building) = (0xFFFF, 0u16, 0u16);
    for (b, g) in regs {
        if g != good {
            continue;
        }
        let mut next = crate::buildings::pile_with_good(w, b, good);
        if next == 0 {
            next = crate::buildings::storage::next_free_pile(w, b);
        }
        if next != 0 {
            let (nx, ny) = pos_of(w, next);
            let d = crate::spiral::grid_distance(nx - px, ny - py);
            if d < best_d {
                best_d = d;
                best = next;
                best_building = b;
            }
        }
    }
    let free = w.economy.get(eco).is_some_and(|e| e.free_storage_piles.contains(&(best_building, best)));
    if best == 0 || !free {
        return best;
    }
    unregister_free_storage_pile(w, eco, best_building, best);
    register_good_storage_pile(w, eco, best_building, best, good);
    crate::buildings::storage::reserve_free_pile(w, best_building, good, best);
    best
}

// ---- transport and recruiting (piece P3) ----

/// `CGoodTransportPriority::Create*Priorities` (release 0x4e59c0 / 0x4e5b10 / 0x4e5c70 /
/// 0x4e5de0, values from the `TransportPriority` config defaults): rank per good, rank count.
/// Dark Tribe gets none.
pub fn default_transport_priority(race: u8) -> ([u16; GOODS], i32) {
    let table: &[(u8, u16)] = match race {
        0 => &[
            (2, 17), (3, 16), (4, 20), (7, 1), (8, 15), (9, 7), (10, 6), (11, 8), (12, 10), (14, 25), (15, 26), (16, 11), (18, 19),
            (20, 4), (21, 5), (22, 3), (24, 9), (25, 21), (27, 23), (28, 22), (29, 24), (30, 12), (31, 18), (32, 2), (33, 28),
            (34, 14), (36, 13), (37, 27),
        ],
        1 => &[
            (3, 17), (4, 20), (5, 16), (7, 1), (8, 15), (9, 7), (10, 6), (11, 8), (12, 10), (14, 25), (15, 26), (16, 11), (18, 19),
            (19, 28), (20, 4), (21, 5), (22, 3), (23, 27), (24, 9), (25, 21), (26, 12), (27, 23), (28, 22), (29, 24), (31, 18),
            (32, 2), (33, 29), (34, 14), (36, 13),
        ],
        2 => &[
            (1, 30), (3, 17), (4, 22), (6, 16), (7, 1), (8, 15), (9, 7), (10, 6), (11, 8), (12, 10), (13, 12), (14, 27), (15, 28), (16, 11),
            (17, 18), (18, 21), (20, 4), (21, 5), (22, 3), (24, 9), (25, 23), (27, 25), (28, 24), (29, 26), (31, 20), (32, 2),
            (33, 19), (34, 14), (35, 29), (36, 13),
        ],
        4 => &[
            (3, 17), (4, 22), (7, 1), (8, 15), (9, 7), (10, 6), (11, 8), (12, 10), (14, 27), (15, 28), (16, 11), (18, 21), (20, 4),
            (21, 5), (22, 3), (24, 9), (25, 23), (27, 25), (28, 24), (29, 26), (31, 20), (32, 2), (33, 19), (34, 14), (36, 13),
            (38, 16), (39, 12), (40, 18), (41, 29), (42, 30),
        ],
        _ => &[],
    };
    let mut p = [0u16; GOODS];
    for &(g, r) in table {
        p[g as usize] = r;
    }
    let count = match race {
        0 => 28,
        1 => 29,
        2 | 4 => 30,
        _ => 0,
    };
    (p, count)
}

/// `CBuildingSupplyPriority::Create*Priorities`: which building types share which good, with the
/// shares from `Config\Transport.cfg [BUILDINGSUPPLYPRIORITY]` (column 0 for the all-races mines,
/// mill and vehicle hall, the race's column for the rest). A later entry for the same building
/// replaces the earlier one, as `map::operator=` does.
pub fn default_supply(cfg: &crate::cfg::Cfg, race: u8) -> SupplyMap {
    const S: &str = "BUILDINGSUPPLYPRIORITY";
    let mut m = SupplyMap::new();
    let mut set = |b: u8, goods: &[(u8, &str)], col: usize| {
        m.insert(b, goods.iter().map(|&(g, k)| (g, cfg.int_array::<6>(S, k)[col])).collect());
    };
    // CreateAllRacesPriorities (column 0).
    for (b, mine) in [(17, "COALMINE"), (15, "IRONMINE"), (16, "GOLDMINE"), (14, "STONEMINE"), (18, "SULFURMINE")] {
        let (bread, meat, fish) = (format!("BREAD_{mine}"), format!("MEAT_{mine}"), format!("FISH_{mine}"));
        set(b, &[(9, bread.as_str()), (24, meat.as_str()), (11, fish.as_str())], 0);
    }
    set(9, &[(16, "GRAIN_MILL")], 0);
    set(23, &[(20, "IRONBAR_VEHICLEHALL")], 0);
    let col = match race {
        0 => 1,
        1 => 2,
        2 => 3,
        4 => 5,
        _ => return m,
    };
    match race {
        1 => {
            set(39, &[(36, "WATER_MEADMAKERHUT")], col);
            set(25, &[(22, "LOG_CHARCOALMAKER")], col);
            set(3, &[(22, "LOG_SAWMILL")], col);
        }
        2 => set(28, &[(10, "COAL_AMMOMAKERHUT")], col),
        4 => {
            set(28, &[(22, "LOG_AMMOMAKERHUT"), (10, "COAL_AMMOMAKERHUT")], col);
            set(3, &[(22, "LOG_SAWMILL")], col);
        }
        _ => {}
    }
    set(31, &[(20, "IRONBAR_SHIPYARD")], col);
    set(19, &[(10, "COAL_GOLDMELT")], col);
    set(20, &[(10, "COAL_IRONMELT")], col);
    set(21, &[(10, "COAL_TOOLSMITH"), (20, "IRONBAR_TOOLSMITH")], col);
    set(22, &[(10, "COAL_WEAPONSMITH"), (20, "IRONBAR_WEAPONSMITH")], col);
    set(10, &[(36, "WATER_BAKERY")], col);
    set(12, &[(36, "WATER_ANIMALFARM"), (16, "GRAIN_ANIMALRANCH")], col);
    set(13, &[(36, "WATER_DONKEYFARM"), (16, "GRAIN_DONKEYFARM")], col);
    if race == 2 {
        // (the config key is spelt TEQUILAMAKEHUT)
        set(37, &[(36, "WATER_TEQUILAMAKEHUT")], col);
    }
    m
}

fn pos_of(w: &World, id: u16) -> (i32, i32) {
    w.entities.by_index(id as u32).map_or((0, 0), |e| (e.pos.x(), e.pos.y()))
}

fn event(w: &mut World, settler: u16, event: i32, data: [i32; 3]) {
    crate::jobs::core::set_event(w, settler, crate::jobs::state::EntityEvent { event, kind: 0, data });
}

/// `CEcoSector::GetNearestSettler(type, x, y)`: along the type's offer list, the nearest
/// (first on ties) that is not striking (`CSettler::Strike`: flag 0x1000 clear).
pub fn get_nearest_settler(w: &World, eco: u16, kind: u16, x: i32, y: i32) -> u16 {
    let Some(e) = w.economy.get(eco) else { return 0 };
    let mut id = e.settler_offer.get(kind as usize).copied().unwrap_or(0);
    let (mut best, mut best_d) = (0, u32::MAX);
    while let Some(s) = w.entities.by_index(id as u32) {
        let d = crate::spiral::grid_distance(x - s.pos.x(), y - s.pos.y()) as u32;
        if s.flags & 0x1000 != 0 && d < best_d {
            best_d = d;
            best = id;
        }
        id = s.next;
    }
    best
}

/// `CEcoSector::TransportGood(src, dst)`: the nearest free carrier takes one good from `src` to
/// `dst` (event 10, then the reservations).
pub fn transport_good(w: &mut World, eco: u16, src: u16, dst: u16) -> bool {
    if src == dst || src == 0 || dst == 0 {
        return false;
    }
    let (x, y) = pos_of(w, src);
    let c = get_nearest_settler(w, eco, 1, x, y);
    if c == 0 {
        return false;
    }
    get_settler_out_of_offer(w, eco, c);
    event(w, c, 10, [0, src as i32, dst as i32]);
    crate::pile::attach_and_inc_amount_leaving(w, src, c, 1, crate::pile::target::PILE_SRC);
    crate::pile::attach_and_inc_amount_coming(w, dst, c);
    true
}

/// `CEcoSector::GetCarrierOutOfOfferAndSetNextSettlerType(carrier, type)`.
fn get_carrier_out_of_offer_and_set_next_type(w: &mut World, eco: u16, carrier: u16, kind: u16) {
    get_settler_out_of_offer(w, eco, carrier);
    if let Some(e) = w.entities.by_index_mut(carrier as u32)
        && let EntityData::Settler(d) = &mut e.data
    {
        d.job.carrier.next_type = kind as u8;
    }
    if let Some(e) = w.economy.get_mut(eco) {
        e.carriers_with_job_order += 1;
    }
}

/// `CEcoSector::CallWorker(building, type)`: the nearest free settler of the type comes.
pub fn call_worker(w: &mut World, eco: u16, building: u16, kind: u16) -> u16 {
    if w.rules.warrior_type(kind) != 0 {
        return 0;
    }
    let (x, y) = pos_of(w, building);
    let s = get_nearest_settler(w, eco, kind, x, y);
    if s != 0 {
        get_settler_out_of_offer(w, eco, s);
        crate::jobs::core::call_worker(w, s, building);
    }
    s
}

fn carriers_free_for_orders(w: &World, eco: u16) -> bool {
    w.economy.get(eco).is_some_and(|e| (e.min_carrier as i32) < e.settlers[1] as i32 - e.carriers_with_job_order as i32)
}

/// `CEcoSector::OrderWorker(building, type)`: a free settler of the type, else a carrier that
/// fetches the type's tool (event 23) or simply becomes it (event 22). Returns the settler.
/// The owner's warnings ([`order_warning`]): no carrier, no tool.
pub fn order_worker(w: &mut World, eco: u16, building: u16, kind: u16) -> u16 {
    if w.rules.warrior_type(kind) != 0 {
        return 0;
    }
    let (bx, by) = pos_of(w, building);
    let s = get_nearest_settler(w, eco, kind, bx, by);
    if s != 0 {
        get_settler_out_of_offer(w, eco, s);
        crate::jobs::core::attach_to_building(w, s, building);
        event(w, s, 1, [building as i32, 0, 0]);
        return s;
    }
    if !carriers_free_for_orders(w, eco) {
        // Not for diggers and builders (0x4d4f50).
        if kind != 2 && kind != 3 {
            order_warning(w, building, 0x99b);
        }
        return 0;
    }
    let race = w.entities.by_index(building as u32).map_or(0, |e| e.race);
    let tool = w.rules.settler_tool(race, kind);
    if tool != 0 {
        let head = w.economy.get(eco).map_or(0, |e| e.good_offer[tool as usize]);
        let pile = crate::pile::get_nearest_pile(w, head, bx, by);
        if pile == 0 {
            // 0x4d4ff0 by tool: GUI_WARN_BUILD_NOAXE, AO_GUI_WARN_BUILD_NOBOW, …NOHAMMER (not
            // for a builder), …NOPICK, …NOROD, …NOSAW, …NOSCYTHE, …NOSHOVEL (not for a digger).
            let text = match (tool, kind) {
                (4, _) => 0x995,
                (8, _) => 0x139,
                (0x12, k) if k != 3 => 0x996,
                (0x19, _) => 0x997,
                (0x1b, _) => 0x998,
                (0x1c, _) => 0x999,
                (0x1d, _) => 0x99a,
                (0x1f, k) if k != 2 => 0x99c,
                _ => 0,
            };
            if text != 0 {
                order_warning(w, building, text);
            }
            return 0;
        }
        let (px, py) = pos_of(w, pile);
        let c = get_nearest_settler(w, eco, 1, px, py);
        if c == 0 {
            return 0;
        }
        get_carrier_out_of_offer_and_set_next_type(w, eco, c, kind);
        crate::pile::attach_and_inc_amount_leaving(w, pile, c, 1, crate::pile::target::PILE_SRC);
        crate::jobs::core::attach_to_building(w, c, building);
        event(w, c, 23, [kind as i32, pile as i32, building as i32]);
        return c;
    }
    let c = get_nearest_settler(w, eco, 1, bx, by);
    if c != 0 {
        get_carrier_out_of_offer_and_set_next_type(w, eco, c, kind);
        crate::jobs::core::attach_to_building(w, c, building);
        event(w, c, 22, [kind as i32, building as i32, 0]);
    } else {
        order_warning(w, building, 0x99b);
    }
    c
}

/// The owner's warning about `building` from `OrderWorker` (0x4d4d91, 0x4d4df7, 0x4d5057):
/// GUI_WARN_BUILD_NOSETTLER or a missing tool.
fn order_warning(w: &mut World, building: u16, text: u16) {
    if let Some(b) = w.entities.by_index(building as u32) {
        let ev = crate::event::SimEvent::warning(b.owner, text, b.pos);
        w.events.push(ev);
    }
}

/// `CEcoSector::ChangeValue(count, current, delta)`: 0 resets, 100 means 99, never below
/// −count, at most 99.
fn change_value(count: i32, current: i32, delta: i32) -> i32 {
    match delta {
        0 => 0,
        100 => 99,
        d if d <= 0 => {
            if d + current + count >= 0 {
                d + current
            } else {
                -count
            }
        }
        d => (d + current).min(99),
    }
}

/// Specialist types of `m_cSpecialistWishes` (+672; release .data 0x1164214): thief,
/// geologist, pioneer, saboteur, gardener.
pub const SPECIALISTS: [u16; 5] = [48, 49, 47, 46, 50];
/// Worker types of `m_cWorkerDeltaAmount` (+677; .data 0x1164244): builder, digger.
pub const WORKERS: [u16; 2] = [3, 2];


/// `CEcoSector::ChangeSpecialistWish(type, delta)`.
pub fn change_specialist_wish(w: &mut World, eco: u16, kind: u16, delta: i32) {
    let Some(i) = SPECIALISTS.iter().position(|&t| t == kind) else { return };
    let owner = w.economy.owner(eco);
    let n = number_of_settlers(w, owner, kind);
    if let Some(e) = w.economy.get_mut(eco) {
        e.specialist_wish[i] = change_value(n, e.specialist_wish[i] as i32, delta) as i8;
    }
}

/// `CEcoSector::ChangeWorkerDeltaAmount(type, delta)` (counts of the eco-sector).
pub fn change_worker_delta_amount(w: &mut World, eco: u16, kind: u16, delta: i32) {
    let Some(i) = WORKERS.iter().position(|&t| t == kind) else { return };
    if let Some(e) = w.economy.get_mut(eco) {
        let n = e.settlers[kind as usize] as i32;
        e.worker_delta[i] = change_value(n, e.worker_delta[i] as i32, delta) as i8;
    }
}

/// `CEcoSector::ChangeMinMaxValues(type, delta)` (command `SetUnitReservation`): carriers kept
/// (5..=999, at most the carriers there are), or a worker delta.
pub fn change_min_max_values(w: &mut World, eco: u16, kind: u16, delta: i32) {
    if kind == 1 {
        if let Some(e) = w.economy.get_mut(eco) {
            let v = e.min_carrier as i32 + delta;
            e.min_carrier = if v <= 5 {
                5
            } else if v >= 999 {
                999
            } else {
                v.min(e.settlers[1] as i32) as u16
            };
        }
    } else if (2..=3).contains(&kind) {
        change_worker_delta_amount(w, eco, kind, delta);
    }
}

/// The tool pile a carrier at `c` would fetch for becoming `kind` (`None`: the type needs a tool
/// and none is offered; `Some(0)`: no tool needed).
fn tool_pile_for(w: &World, eco: u16, c: u16, kind: u16) -> Option<u16> {
    let race = w.entities.by_index(c as u32).map_or(0, |e| e.race);
    let tool = w.rules.settler_tool(race, kind);
    if tool == 0 {
        return Some(0);
    }
    let head = w.economy.get(eco).map_or(0, |e| e.good_offer[tool as usize]);
    let (x, y) = pos_of(w, c);
    let p = crate::pile::get_nearest_pile(w, head, x, y);
    (p != 0).then_some(p)
}

/// `CEcoSector::RecruiteSpecialist` (0x4d51d0): wished specialists are made from free carriers
/// (fetching the tool first: event 21; thieves without: event 20). A negative wish turns the
/// owner's specialists of the type back into carriers: over the owner's list of the type
/// (0x1544cec), each one standing in this eco-sector without flag 0x8000
/// (`CSettler::ChangeType(1, …, 0)`) until the wish is met; then the wish is 0. TODO(port): the
/// script game event 18.
pub fn recruite_specialist(w: &mut World, eco: u16) {
    for (i, &kind) in SPECIALISTS.iter().enumerate() {
        let wish = w.economy.get(eco).map_or(0, |e| e.specialist_wish[i]);
        if wish < 0 {
            let owner = w.economy.owner(eco);
            let mut left = wish;
            let mut id = w.settler_lists.head(owner, kind);
            while id != 0 && left < 0 {
                let Some(e) = w.entities.by_index(id as u32) else { break };
                let (next, flags) = (e.next, e.flags);
                let here = w.grid.checked_index(e.x(), e.y()).is_some_and(|n| eco_of(w, n) == eco);
                if here && flags & 0x8000 == 0 {
                    crate::jobs::core::change_type(w, id, 1, true, false);
                    left += 1;
                }
                id = next;
            }
            if let Some(e) = w.economy.get_mut(eco) {
                e.specialist_wish[i] = 0;
            }
            continue;
        }
        for _ in 0..wish.max(0) {
            if !carriers_free_for_orders(w, eco) {
                return;
            }
            let first = w.economy.get(eco).map_or(0, |e| e.settler_offer[1]);
            if first == 0 {
                continue;
            }
            let Some(pile) = tool_pile_for(w, eco, first, kind) else { break };
            if pile != 0 {
                crate::pile::attach_and_inc_amount_leaving(w, pile, first, 1, crate::pile::target::PILE_SRC);
            }
            get_carrier_out_of_offer_and_set_next_type(w, eco, first, kind);
            if matches!(kind, 46 | 47 | 49 | 50) {
                event(w, first, 21, [kind as i32, pile as i32, 0]);
            } else {
                event(w, first, 20, [kind as i32, 0, 0]);
            }
            // The script's `SETTLER_CHANGE_TYPE` with the new type (0x4d5597).
            w.script_events.send(crate::event::ScriptEvents::SETTLER_CHANGE_TYPE, [kind as i32, 0, 0]);
            change_specialist_wish(w, eco, kind, -1);
        }
    }
}

/// `CEcoSector::RecruiteWorker` (0x4d57d0): one builder / digger per call from a free carrier
/// (with the tool: event 21), or a surplus one (the first offered) back to carrier
/// (`CSettler::ChangeType(1, …, 0)`). Only the recruiting posts the script's game event 18
/// (0x4d59a4); the way back to carrier posts none.
pub fn recruite_worker(w: &mut World, eco: u16) {
    for (i, &kind) in WORKERS.iter().enumerate() {
        let delta = w.economy.get(eco).map_or(0, |e| e.worker_delta[i]);
        if delta > 0 {
            if !carriers_free_for_orders(w, eco) {
                return;
            }
            let c = w.economy.get(eco).map_or(0, |e| e.settler_offer[1]);
            if c == 0 {
                continue;
            }
            let Some(pile) = tool_pile_for(w, eco, c, kind) else { continue };
            if pile != 0 {
                crate::pile::attach_and_inc_amount_leaving(w, pile, c, 1, crate::pile::target::PILE_SRC);
            }
            get_carrier_out_of_offer_and_set_next_type(w, eco, c, kind);
            event(w, c, 21, [kind as i32, pile as i32, 0]);
            // The script's `SETTLER_CHANGE_TYPE` with the new type (0x4d59a4).
            w.script_events.send(crate::event::ScriptEvents::SETTLER_CHANGE_TYPE, [kind as i32, 0, 0]);
            change_worker_delta_amount(w, eco, kind, -1);
        } else if delta < 0 {
            let s = w.economy.get(eco).map_or(0, |e| e.settler_offer[kind as usize]);
            if s != 0 {
                // 0x4d5849: out of the offer, back to carrier (wait list, no offer).
                get_settler_out_of_offer(w, eco, s);
                crate::jobs::core::change_type(w, s, 1, true, false);
                change_worker_delta_amount(w, eco, kind, 1);
            }
        }
    }
}

/// `CEcoSector::CleanUpBuildingNeed(building)`: the building's first request per good goes.
pub fn clean_up_building_need(w: &mut World, eco: u16, building: u16) {
    if let Some(e) = w.economy.get_mut(eco) {
        for q in &mut e.requests {
            if let Some(i) = q.iter().position(|&b| b == building) {
                q.remove(i);
            }
        }
    }
}

/// `CEcoSector::InitTransport` (release 0x4d5d30): for each transport rank from 1, the good of
/// that rank: among the buildings asking for it, pick a building type by supply share (served
/// count × 256 + 128, per share; lowest wins, if such a building asks), then the asking
/// building of that type (or any, without shares; ports and marketplaces always) whose own pile
/// is most urgent per distance to the nearest offered pile gets one carrier. The queue is then
/// emptied; coal (good 10) gets a second round per rank. At most one transport per rank.
pub fn init_transport(w: &mut World, eco: u16) {
    const COAL: usize = 10;
    let (mut rank, mut good, mut second) = (1u16, 0usize, false);
    while (rank as usize) < GOODS {
        let mut supplied = false;
        if good != COAL || !second {
            good = 1;
        }
        let Some(e) = w.economy.get(eco) else { return };
        while good < GOODS && e.transport_prio[good] != rank {
            good += 1;
        }
        if good < GOODS && !e.requests[good].is_empty() {
            let queue: Vec<u16> = e.requests[good].iter().copied().collect();
            // GetNumberOfSupplyBuildings(good): (building type, share) in map order.
            let supply: Vec<(u8, i32)> = e.supply.iter().filter_map(|(&b, m)| m.get(&(good as u8)).map(|&s| (b, s))).collect();
            let counters = e.supply_counter;
            let mut chosen: i32 = -1;
            let mut best = u32::MAX;
            for &(btype, share) in &supply {
                supplied = true;
                let score: i32 = if share == 0 {
                    -1
                } else {
                    (((counters.get(btype as usize).copied().unwrap_or(0) as i32) << 8) + 128) / share
                };
                if (score as u32) < best && score != -1 {
                    let asks = queue.iter().any(|&b| {
                        w.entities.by_index(b as u32).is_some_and(|x| (btype == 84 || x.kind == btype as u16) && x.flags & 0x1000 != 0)
                    });
                    if asks {
                        best = score as u32;
                        chosen = btype as i32;
                    }
                }
            }
            let mut candidates = Vec::new();
            for &b in &queue {
                if candidates.len() == 100 {
                    break;
                }
                let Some(x) = w.entities.by_index(b as u32).filter(|x| x.obj_type == ObjType::Building) else {
                    return; // (the original returns from InitTransport here)
                };
                let (t, ready) = (x.kind as i32, x.flags & 0x1000 != 0);
                if (chosen == -1 && !supplied) || ((t == 33 || t == 32 || chosen == 84 || t == chosen) && ready) {
                    candidates.push(b);
                }
            }
            if (good != COAL || second)
                && let Some(e) = w.economy.get_mut(eco)
            {
                e.requests[good].clear();
            }
            let head = w.economy.get(eco).map_or(0, |e| e.good_offer[good]);
            let (mut best, mut from, mut to_building) = (0u32, 0u16, 0u16);
            for &b in &candidates {
                let own = crate::buildings::pile_with_good(w, b, good as u8);
                if own == 0 {
                    continue;
                }
                let (bx, by) = pos_of(w, b);
                let n = crate::pile::get_nearest_pile(w, head, bx, by);
                if n == 0 {
                    continue;
                }
                let (px, py) = pos_of(w, n);
                let d = crate::spiral::grid_distance(bx - px, by - py).max(1); // (0 divides by zero in the original)
                let finished = w.entities.by_index(b as u32).is_some_and(|e| e.flags & crate::entity::flag::BIRTH == 0);
                let (amount, coming) = crate::pile::get(w, own).map_or((0, 0), |p| (p.amount, p.coming));
                let score = transport_score(finished, amount, coming, d);
                if score > best {
                    if crate::buildings::pile_with_need_for_good(w, b, good as u8) == n {
                        continue; // "Ilegal transport from pile A to pile A found"
                    }
                    best = score;
                    from = n;
                    to_building = b;
                }
            }
            if best != 0 {
                let dst = crate::buildings::pile_with_need_for_good(w, to_building, good as u8);
                let room = crate::pile::get(w, dst).is_some_and(|p| p.amount + p.coming < 8);
                if room && transport_good(w, eco, from, dst) {
                    if good == COAL
                        && !second
                        && let Some(e) = w.economy.get_mut(eco)
                        && let Some(i) = e.requests[good].iter().position(|&b| b == to_building)
                    {
                        e.requests[good].remove(i);
                    }
                    if (0..=0x29).contains(&chosen)
                        && let Some(e) = w.economy.get_mut(eco)
                    {
                        e.supply_counter[chosen as usize] = e.supply_counter[chosen as usize].wrapping_add(1);
                    }
                }
            }
        }
        if good == COAL && !second {
            second = true;
        } else {
            if good == COAL
                && let Some(e) = w.economy.get_mut(eco)
            {
                e.requests[COAL].clear();
            }
            rank += 1;
            second = false;
        }
    }
}

/// `InitTransport`'s score of a requesting building (release 0x4d638b): a finished building
/// (BIRTH clear) scores `CPile::CalcUrgent << 16` over the distance to the nearest offer, a site
/// 0x10000. `CalcUrgent` = 4000 · (16 − (2·amount + coming)); the shifted product wraps in 32
/// bits (an empty pile gives 0xfa000000), the division is signed, and the scores are compared
/// unsigned (`jbe` at 0x4d6429), so wrapped (negative) values rank above 0x10000.
fn transport_score(finished: bool, amount: u8, coming: u8, d: i32) -> u32 {
    if !finished {
        return 0x10000;
    }
    let urgent = 4000i32.wrapping_mul(16 - (coming as i32 + 2 * amount as i32));
    (urgent.wrapping_shl(16) / d.max(1)) as u32
}

#[test]
fn transport_score_wraps_and_ranks_unsigned() {
    // Empty pile, distance 5: 0xfa000000 / 5 signed, then unsigned: above a site's 0x10000.
    assert_eq!(transport_score(true, 0, 0, 5), (0xfa00_0000u32 as i32 / 5) as u32);
    assert!(transport_score(true, 0, 0, 5) > transport_score(false, 0, 0, 5));
    // A fuller pile scores lower (closer to 0 from the top of the unsigned range).
    assert!(transport_score(true, 3, 1, 5) < transport_score(true, 0, 0, 5));
    // 2·amount + coming = 16: CalcUrgent 0.
    assert_eq!(transport_score(true, 8, 0, 3), 0);
}

/// `CEcoSectorMgr::InitTransport` / `RecruiteSpecialist` / `RecruiteWorker`: every eco-sector
/// in id order (`tick & 15 == 15`).
pub fn for_each_sector(w: &mut World, f: fn(&mut World, u16)) {
    for id in 1..ECO_MAX as u16 {
        if w.economy.get(id).is_some() {
            f(w, id);
        }
    }
}

/// `CGoodTransportPriority::ChangePriority(good, delta)` (command 5029): 0 moves the good to
/// rank 1, 100 to the last rank, otherwise it swaps with the good `delta` ranks away.
pub fn change_transport_priority(w: &mut World, eco: u16, good: u16, delta: i32) {
    let Some(e) = w.economy.get_mut(eco) else { return };
    let g = good as usize;
    if g == 0 || g >= GOODS {
        return;
    }
    let p = &mut e.transport_prio;
    let cur = p[g];
    match delta {
        100 => {
            for x in p.iter_mut() {
                if *x > cur {
                    *x -= 1;
                }
            }
            p[g] = e.transport_prio_count as u16;
        }
        0 => {
            for x in p.iter_mut() {
                if *x != 0 && *x < cur {
                    *x += 1;
                }
            }
            p[g] = 1;
        }
        d => {
            let want = (cur as i32 + d) as u16;
            if let Some(j) = (0..GOODS).find(|&j| p[j] == want)
                && j > 0
            {
                p[j] = cur;
                p[g] = want;
            }
        }
    }
}

/// `CBuildingSupplyPriority::ChangePriority(good, building, delta)` (command 5028): the share,
/// clamped to 0..=100, if the building type has one for the good.
pub fn change_building_supply_priority(w: &mut World, eco: u16, good: u16, building: u16, delta: i8) {
    let Some(e) = w.economy.get_mut(eco) else { return };
    if let Some(m) = e.supply.get_mut(&(building as u8))
        && let Some(v) = m.get_mut(&(good as u8))
    {
        *v = (*v + delta as i32).clamp(0, 100);
    }
}

/// `CSettlerMgr::GetNumberOfSettlers(player, type)`: 0x1545658, counted from `AddSettler` to the
/// removal in `Kill` (0x582940), under the current type (`ChangeType` moves it); see
/// [`crate::settler_lists::SettlerLists::settlers`].
pub fn number_of_settlers(w: &World, player: u8, kind: u16) -> i32 {
    w.settler_lists.settlers(player, kind)
}

/// `CEcoSectorMgr::CalculateInitialFreeBeds` (release 0x4ddec0, after the map is loaded): per player
/// 10 * ceil(carriers / 10) + 10.
pub fn calculate_initial_free_beds(w: &mut World) {
    for p in 1..=w.number_of_players().clamp(0, 8) as u8 {
        let carriers = number_of_settlers(w, p, 1);
        w.economy.initial_free_beds[p as usize] = 10 * ((carriers + 9) / 10) + 10;
    }
}

/// `CEcoSectorMgr::GetNrOfCurrentTotalBeds(player)` (release 0x4df730): the initial free beds
/// plus info +0x1f0 (beds) times the player's ready residences (small 40, medium 41, big 42) as
/// `CBuildingMgr` counts them (0x14ef624 + 8 · type). `CBuilding::Ready` (0x5362f0) counts a
/// building before its role's `Init` runs (and sets 0x10000 after it), so a new residence's
/// `Init` already sees its own beds; here: READY (0x2000000, set by `InitCommon` first) and not
/// dead.
pub fn current_total_beds(w: &World, player: u8) -> i32 {
    if !(1..=8).contains(&player) {
        return 0;
    }
    let race = w.race_of(player);
    // The ready residences 40..=42 counted in one pass over the entities.
    let mut ready = [0i32; 3];
    for (_, e) in w.entities.iter() {
        if e.obj_type == ObjType::Building && e.owner == player && (40..=42).contains(&e.kind) && e.flags & flag::READY != 0 && e.flags & flag::DIED == 0 {
            ready[(e.kind - 40) as usize] += 1;
        }
    }
    let homes: i32 = [40u8, 41, 42].iter().zip(ready).map(|(&t, n)| w.rules.building(race, t).beds * n).sum();
    w.economy.initial_free_beds[player as usize] + homes
}

/// `CEcoSectorMgr::Update` (release 0x4ddea0, once per tick): every 32nd call
/// `UpdateStrikeSettlers`.
pub fn update(w: &mut World) {
    w.economy.strike_counter += 1;
    if w.economy.strike_counter < 32 {
        return;
    }
    update_strike_settlers(w);
    w.economy.strike_counter = 0;
}

/// `CEcoSectorMgr::UpdateStrikeSettlers` (release 0x4df2f0): per player (1..=`NumberOfPlayers`), the carriers beyond the beds
/// strike. More carriers than beds + strikers: free carriers (the head of each eco-sector's
/// carrier offer, the player's eco-sectors in turn, round after round) get event 27 (data 1),
/// leave the offer and join the strikers. Fewer: the last strikers get event 27 (data 0) and
/// rejoin the carrier offer of the eco-sector they stand on.
pub(crate) fn update_strike_settlers(w: &mut World) {
    for p in 1..=w.number_of_players().clamp(0, 8) as u8 {
        let pi = p as usize;
        let mut excess = number_of_settlers(w, p, 1) - current_total_beds(w, p) - w.economy.strikers[pi].len() as i32;
        if excess > 0 {
            let mut progress = true;
            while progress && excess != 0 {
                progress = false;
                let ecos = w.economy.player_ecos[pi].clone();
                for e in ecos {
                    if excess == 0 {
                        break;
                    }
                    if w.economy.owner(e) != p {
                        continue;
                    }
                    let s = w.economy.get(e).map_or(0, |x| x.settler_offer[1]);
                    if s == 0 || w.entities.by_index(s as u32).is_none() {
                        continue;
                    }
                    event(w, s, 27, [1, 0, 0]);
                    get_settler_out_of_offer(w, e, s);
                    w.economy.strikers[pi].push(s);
                    excess -= 1;
                    progress = true;
                }
            }
        } else if excess < 0 {
            let mut want = -excess;
            let n = want.min(w.economy.strikers[pi].len() as i32);
            for _ in 0..n {
                if want == 0 {
                    break;
                }
                let Some(&s) = w.economy.strikers[pi].last() else { break };
                let Some(node) = w.entities.by_index(s as u32).and_then(|e| w.grid.checked_index(e.x(), e.y())) else {
                    continue; // (a missing settler is skipped and stays on the list, as in the original)
                };
                event(w, s, 27, [0, 0, 0]);
                let e = eco_of(w, node);
                if w.economy.get(e).is_some() {
                    set_settler_offer(w, e, 1, s);
                }
                w.economy.strikers[pi].pop();
                want -= 1;
            }
        }
    }
}

/// `CEcoSectorMgr::RemoveCarrierFromStrikeSettlers(player, settler)` (called by
/// `CCarrierRole::SetFree`; not yet located in the release): the last striker fills the hole.
pub fn remove_carrier_from_strike_settlers(w: &mut World, player: u8, settler: u16) {
    if let Some(v) = w.economy.strikers.get_mut(player as usize)
        && let Some(i) = v.iter().position(|&s| s == settler)
    {
        v.swap_remove(i);
    }
}

/// `CEcoSectorMgr::GetNrStrikingCarriers(player)`.
pub fn striking_carriers(w: &World, player: u8) -> usize {
    w.economy.strikers.get(player as usize).map_or(0, Vec::len)
}

/// The order-count rule of 0x4d6960 / 0x4d6a90: `delta` 0 clears, 100 means unlimited, else the
/// count moves by `delta` within 0..=100 (beyond 100 it becomes 100, below 0 it becomes 0).
fn change_count(count: u8, delta: i32) -> u8 {
    if delta == 0 {
        0
    } else if delta == 100 {
        100
    } else if delta > 0 {
        if count as i32 + delta > 100 { 100 } else { (count as i32 + delta) as u8 }
    } else if count as i32 + delta >= 0 {
        (count as i32 + delta) as u8
    } else {
        0
    }
}

/// `CEcoSector::ChangeWeaponOrder(weapon, delta)` (release 0x4d6960): the count of the first
/// listed weapon equal to `good`. (A good not in the list reads one past the list's end in the
/// original; nothing changes here.)
pub fn change_weapon_order(w: &mut World, eco: u16, good: u8, delta: i32) {
    let Some(e) = w.economy.get_mut(eco) else { return };
    if let Some(i) = e.weapons.iter().position(|&g| g == good)
        && e.weapons[i] != 0
    {
        e.weapon_orders[i] = change_count(e.weapon_orders[i], delta);
    }
}

/// `CEcoSector::ChangeToolOrder(tool, delta)` (release 0x4d6a90): as for weapons, for a nonzero
/// listed tool.
pub fn change_tool_order(w: &mut World, eco: u16, good: u8, delta: i32) {
    let Some(e) = w.economy.get_mut(eco) else { return };
    if good == 0 {
        return;
    }
    if let Some(i) = e.tools.iter().position(|&g| g == good) {
        e.tool_orders[i] = change_count(e.tool_orders[i], delta);
    }
}

/// `CEcoSector::SetWeaponPercentage(packed)` (release 0x4d6a10): bytes 0, 1, 2 (and 3 when four
/// weapons are listed) of `packed` become the percentages.
pub fn set_weapon_percentage(w: &mut World, eco: u16, packed: u32) {
    let Some(e) = w.economy.get_mut(eco) else { return };
    let n = if e.weapon_percent.len() == 4 { 4 } else { 3 };
    for k in 0..n.min(e.weapon_percent.len()) {
        e.weapon_percent[k] = (packed >> (8 * k)) as u8;
    }
}

/// The product choice of `CProductionBuildingRole` (release 0x54ce20) for a toolsmith (21) or
/// weaponsmith (22) whose products (role +0x198) are `products`; `last` / `product` are the
/// role's +0x17d / +0x17c. Round-robin from the index after `last`, the first product with a
/// nonzero order count is made (the count drops unless it is 100). A weaponsmith without orders
/// and with auto-production on (+0x360 == 100) makes the first weapon whose made-counter is at
/// most its percentage of all weapons made (the 4th one, when there are four, without the
/// comparison); its counter goes up. Returns whether the building works (the weaponsmith on
/// auto-production always does, keeping `product` when no percentage applies).
pub fn choose_product(w: &mut World, eco: u16, building_type: u16, products: &[u8], last: &mut u8, product: &mut u8) -> bool {
    let Some(e) = w.economy.get_mut(eco) else { return false };
    let n = products.len();
    let orders = match building_type {
        21 => &mut e.tool_orders,
        22 => &mut e.weapon_orders,
        _ => return false,
    };
    for k in 1..=n {
        let i = (*last as usize + k) % n;
        if let Some(c) = orders.get_mut(i)
            && *c != 0
        {
            *product = products[i];
            if *c != 100 {
                *c -= 1;
            }
            *last = i as u8;
            return true;
        }
    }
    if building_type != 22 || e.weapon_auto != 100 {
        return false;
    }
    let total: u32 = e.weapons_made.iter().fold(0u32, |a, &m| a.wrapping_add(m));
    for i in 0..3 {
        let pct = e.weapon_percent.get(i).copied().unwrap_or(0) as u32;
        if pct != 0 && e.weapons_made[i] <= pct.wrapping_mul(total) / 100 {
            *product = products.get(i).copied().unwrap_or(*product);
            e.weapons_made[i] = e.weapons_made[i].wrapping_add(1);
            return true;
        }
    }
    if n == 4 && e.weapon_percent.get(3).is_some_and(|&p| p != 0) {
        *product = products[3];
        e.weapons_made[3] = e.weapons_made[3].wrapping_add(1);
    }
    true
}

/// Consistency of every eco-sector's offer lists (a debugging aid for soaks and tests; the
/// original has none): each list ends, every member exists, carries OFFERED, links back to its
/// predecessor and is in no other list. (A pile may stand outside its list's eco-sector: a
/// building's pile whose node changed owner keeps its offer, in the release too.) The first
/// problem found, as text.
pub fn check_offer_lists(w: &World) -> Result<(), String> {
    let mut seen: std::collections::HashMap<u16, (u16, &str, usize)> = std::collections::HashMap::new();
    for eco in 1..ECO_MAX as u16 {
        let Some(e) = w.economy.get(eco) else { continue };
        let lists = e.good_offer.iter().enumerate().map(|(g, &h)| ("good", g, h)).chain(e.settler_offer.iter().enumerate().map(|(k, &h)| ("settler", k, h)));
        for (what, slot, head) in lists {
            let (mut id, mut prev, mut n) = (head, 0u16, 0usize);
            while id != 0 {
                let Some(x) = w.entities.by_index(id as u32) else {
                    return Err(format!("eco {eco} {what} list {slot}: member {id} does not exist (after {prev})"));
                };
                if x.flags & flag::OFFERED == 0 {
                    return Err(format!("eco {eco} {what} list {slot}: member {id} (type {:?} kind {}) is not OFFERED", x.obj_type, x.kind));
                }
                if x.prev != prev {
                    return Err(format!("eco {eco} {what} list {slot}: member {id} has prev {} instead of {prev}", x.prev));
                }
                if let Some((e2, w2, s2)) = seen.insert(id, (eco, what, slot)) {
                    return Err(format!("member {id} is in eco {eco} {what} list {slot} and in eco {e2} {w2} list {s2}"));
                }
                n += 1;
                if n > crate::entity::MAX_ENTRIES as usize {
                    return Err(format!("eco {eco} {what} list {slot}: loops (at {id})"));
                }
                prev = id;
                id = x.next;
            }
        }
    }
    Ok(())
}
