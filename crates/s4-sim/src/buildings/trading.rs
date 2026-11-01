//! `CTradingBuildingRole` (release vtable 0x105958c, house kind 13): the marketplace (33) and the
//! port (32). Own reverse engineering of the shipped exe; decompiles in `tools/re/trade/`
//! (`role.c`: the 97 functions 0x551690..0x558210, `helpers.c`).
//!
//! **What it does.** The player picks a trade target (command 5005, `SetTradeTarget` 0x552a90)
//! and how many goods of each type to send (5006, `TransportGood` 0x552f60: +0x180 / +0x1ab, flag
//! bit 1 in +0x1d6). Every 14 ticks (`LogicUpdate` 0x5527d0 → 0x5547b0) the market fills its
//! eight trade piles (`CTradePileRole`): piles that received goods from another market are
//! re-traded, piles already collecting a good expect more, free piles start collecting a good
//! the eco-sector offers (0x553660); carriers bring the goods. With goods to send and a target
//! that works, it orders a transport (0x554170): a donkey (transport type 2) between markets, a
//! trade cart (type 3, vehicle kind 5) for 17 goods and more, a ship (type 1) between two ports.
//! The transport loads at the source (0x5551b0 / 0x555b60 / 0x5565e0), leaves when nothing more
//! can be loaded (0x5543b0), and unloads into the target's piles (0x556d10), which offer the goods
//! to the eco-sector (trade role 2).
//!
//! **Fields** (store 0x551dc0, version 4; the savegame oracle `tests/sim_trade_save.rs`):
//! +0x17c target, +0x180 / +0x1ab goods still to collect / to send per good, +0x257.. 8 bools,
//! +0x1d6 per-good flags, +0x260 piles, +0x270.., the loading / unloading transport per kind
//! (+0x274/+0x278 ship, +0x279/+0x27c ship unloading, +0x280/+0x284 donkey, +0x285/+0x288,
//! +0x28c/+0x290 cart, +0x291/+0x294), +0x298 transports `{entity, kind, foreign}`, +0x17e the
//! foreign partner (`CInternationalTrader`, +0x2a4), +0x201 / +0x22c its amounts, +0x2a8 ships
//! under way.
//!
//! **Donkeys** (transport kind 2, the `CDonkeyRole` side in `jobs::donkey`): ordered by 0x5558b0,
//! they arrive at the ensign (0x5561f0), load (0x555b60), leave (0x555e40), unload at the target
//! (0x555ed0 → 0x556d10) and are freed there; switching off / crushing calls them off (0x554950,
//! 0x554b10, 0x554ca0), a new target redirects them (0x553f30).
//!
//! **Foreign trade** (`CInternationalTrader`, [`super::trader`]): commands 5007 / 5008 / 5050,
//! the matching 0x4ef5d0 each update (exports topped up to 16 in +0x201 / +0x22c, a partner
//! +0x17e found by 0x4ef7e0), foreign collecting into trade piles of role 3 (0x557a40), and the
//! donkey's two legs: there with this building's exports (reserving the partner's goods, role
//! 4, and free piles for the goods coming, role 5), back with the partner's (0x5572e0).
//!
//! Trade carts: see [`crate::vehicles::cart`] (0x5562b0 order, 0x556c70 arrival, 0x5565e0 load,
//! 0x5567a0 leave, 0x556980 unload). Ships between ports: see [`crate::vehicles::transport`]
//! (0x554da0 order, 0x555800 arrival, 0x5551b0 load, 0x5554d0 leave, 0x5555e0 unload, 0x557420
//! the foreign way back).

use crate::World;
use crate::eco;
use crate::entity::{EntityData, flag};
use crate::hash::StateHasher;
use crate::jobs::state::EntityEvent;
use crate::pile;
use crate::vehicles::cart;
use crate::vehicles::transport as ship;

/// `BUILDING_PORT` / `BUILDING_MARKETPLACE`.
pub const PORT: u16 = 32;
pub const MARKETPLACE: u16 = 33;
/// Goods per table (`0x2b`).
const N: usize = eco::GOODS;
/// "Unlimited" amount (`'d'`).
pub const ENDLESS: i8 = 100;
/// +0x1d6 bits: domestic trade on (bit 1), foreign trade on (bit 3), foreign trade value 0 (bit 2).
pub mod good_flag {
    pub const DOMESTIC: u8 = 0x02;
    pub const FOREIGN_ZERO: u8 = 0x04;
    pub const FOREIGN: u8 = 0x08;
}
/// Transport kinds in +0x298.
pub mod transport {
    pub const SHIP: u32 = 1;
    pub const DONKEY: u32 = 2;
    pub const CART: u32 = 3;
}

/// The loading / unloading slot of one transport kind: `(entity, busy)`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Slot {
    pub entity: u32,
    pub on: bool,
}

/// One +0x298 record.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Transport {
    pub entity: u32,
    pub kind: u32,
    pub foreign: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct State {
    /// Role +4: 1 after `Init`, 3 once running (`LogicUpdate`); +5 cleared in state 3.
    pub state: u8,
    pub sub: u8,
    /// +0x17c the trade target, +0x17e the foreign partner.
    pub target: u16,
    pub partner: u16,
    /// +0x180 goods still to collect, +0x1ab goods still to send (100 = endless).
    #[cfg_attr(feature = "serde", serde(with = "serde_big_array::BigArray"))]
    pub collect: [i8; N],
    #[cfg_attr(feature = "serde", serde(with = "serde_big_array::BigArray"))]
    pub send: [i8; N],
    /// +0x1d6 per good ([`good_flag`]).
    #[cfg_attr(feature = "serde", serde(with = "serde_big_array::BigArray"))]
    pub flags: [u8; N],
    /// +0x201 / +0x22c the same for the foreign trade.
    #[cfg_attr(feature = "serde", serde(with = "serde_big_array::BigArray"))]
    pub foreign_collect: [i8; N],
    #[cfg_attr(feature = "serde", serde(with = "serde_big_array::BigArray"))]
    pub foreign_send: [i8; N],
    /// +0x257..=+0x25f: set when the owner is player 1 (`Init`; only 8 are stored).
    pub player_one: [bool; 9],
    /// +0x260 the trade piles, in buildingInfo order.
    pub piles: Vec<u16>,
    /// +0x270 (stored, never read).
    pub x270: u32,
    /// +0x274/+0x278 ship loading, +0x27c/+0x279 ship unloading, +0x280/+0x284 donkey loading,
    /// +0x288/+0x285 donkey unloading, +0x28c/+0x290 cart loading, +0x294/+0x291 cart unloading.
    pub ship_load: Slot,
    pub ship_unload: Slot,
    pub donkey_load: Slot,
    pub donkey_unload: Slot,
    pub cart_load: Slot,
    pub cart_unload: Slot,
    /// +0x298.
    pub transports: Vec<Transport>,
    /// +0x2a8 ships sent and not back.
    pub ships_out: i32,
}

impl Default for State {
    fn default() -> Self {
        State {
            state: 0,
            sub: 0,
            target: 0,
            partner: 0,
            collect: [0; N],
            send: [0; N],
            flags: [0; N],
            foreign_collect: [0; N],
            foreign_send: [0; N],
            player_one: [false; 9],
            piles: Vec::new(),
            x270: 0,
            ship_load: Slot::default(),
            ship_unload: Slot::default(),
            donkey_load: Slot::default(),
            donkey_unload: Slot::default(),
            cart_load: Slot::default(),
            cart_unload: Slot::default(),
            transports: Vec::new(),
            ships_out: 0,
        }
    }
}

impl State {
    pub(crate) fn hash(&self, h: &mut StateHasher) {
        h.u8(self.state);
        h.u8(self.sub);
        h.u16(self.target);
        h.u16(self.partner);
        for t in [&self.collect, &self.send, &self.foreign_collect, &self.foreign_send] {
            t.iter().for_each(|&v| h.u8(v as u8));
        }
        h.bytes(&self.flags);
        self.player_one.iter().for_each(|&b| h.bool(b));
        h.u16s(&self.piles);
        h.u32(self.x270);
        for s in [self.ship_load, self.ship_unload, self.donkey_load, self.donkey_unload, self.cart_load, self.cart_unload] {
            h.u32(s.entity);
            h.bool(s.on);
        }
        h.u32(self.transports.len() as u32);
        for t in &self.transports {
            h.u32(t.entity);
            h.u32(t.kind);
            h.bool(t.foreign);
        }
        h.u32(self.ships_out as u32);
    }
}

pub fn state(world: &World, id: u16) -> Option<&State> {
    match &super::role(world, id)?.kind {
        super::RoleKind::Trading(s) => Some(&**s),
        _ => None,
    }
}

fn state_mut(world: &mut World, id: u16) -> Option<&mut State> {
    match &mut super::role_mut(world, id)?.kind {
        super::RoleKind::Trading(s) => Some(&mut **s),
        _ => None,
    }
}

/// 0x572215: a marketplace pile's goods expected go back to the goods still to collect (+0x180,
/// a byte add) when the pile changes its good (`CTradePileRole::ChangeGoodType` 0x5721a0).
pub fn collect_back(world: &mut World, id: u16, good: u8, expected: i32) {
    if let Some(s) = state_mut(world, id)
        && let Some(c) = s.collect.get_mut(good as usize)
    {
        *c = c.wrapping_add(expected as i8);
    }
}

/// `SETTLER_DONKEY`.
pub const DONKEY_SETTLER: u16 = 60;

/// Entity flag 0x1000: the building works (not switched off).
pub const WORKING: u32 = 0x1000;

fn building_flags(world: &World, id: u16) -> u32 {
    world.entities.by_index(id as u32).map_or(0, |e| e.flags)
}

fn is_trading(world: &World, id: u16) -> bool {
    state(world, id).is_some()
}

fn piles(world: &World, id: u16) -> Vec<u16> {
    state(world, id).map(|s| s.piles.clone()).unwrap_or_default()
}

/// `CTradingBuildingRole::Init` (0x5524d0), after `IBuildingRole::InitCommon`: the bools
/// +0x257..+0x25f become "owner is player 1", the per-good tables and +0x17c/+0x17e are cleared,
/// the buildingInfo piles are created as trade piles (amount 0; a patch pile, appearance 1, is
/// invisible) and attached, the working area is the position plus the info `workingpos`, role
/// bytes +0x14..+0x1c are cleared, state 1, the first update in 14 ticks; the owner's
/// international trader learns the building (0x4f0460, not ported).
pub fn init(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let (x, y, race, owner) = (e.x(), e.y(), e.race, e.owner);
    let type_ex = match &e.data {
        EntityData::Building(b) => b.type_ex,
        _ => return,
    };
    let info = world.rules.building(race, type_ex);
    let (rules, working) = (info.piles.clone(), info.working);
    if let Some(s) = state_mut(world, id) {
        *s = State { player_one: [owner == 1; 9], state: 1, ..State::default() };
    }
    for p in rules {
        let Some(pid) = pile::add_pile_ex(world, x + p.x as i32, y + p.y as i32, 0, 0, p.kind, p.patch, p.appearance, p.pixel) else {
            continue;
        };
        super::attach(world, id, pid);
        if p.appearance == 1
            && let Some(e) = world.entities.by_index_mut(pid as u32)
        {
            e.flags &= !flag::VISIBLE;
        }
        if let Some(s) = state_mut(world, id) {
            s.piles.push(pid);
        }
    }
    if let Some(EntityData::Building(d)) = world.entities.by_index_mut(id as u32).map(|e| &mut e.data) {
        d.working_area = crate::grid::Y16X16::pack(x + working.0 as i32, y + working.1 as i32);
    }
    world.register_for_logic_update(id, 14);
    // The role's ctor (0x5517a0) makes the trader table (0x4efd50); Init records the building.
    super::trader::ensure(world);
    super::trader::add_building(world, id);
}

// ---- foreign trade commands ----

/// 5007 `TradeWith(p)` (0x5530a0): the owner's trader toggles "trade with player `p`".
pub fn trade_with(world: &mut World, id: u16, p: u32) {
    let owner = owner_of(world, id);
    if let Some(t) = world.buildings.traders.get_mut(owner)
        && let Some(b) = t.trade_with.get_mut(p as usize)
    {
        *b = !*b;
    }
}

/// 5008 `TradeGood(good)` (0x5530f0): the good's foreign state (+0x1d6 bits above 1) cycles
/// 0 → 4 (import) → 8 (export) → 0 (which clears the whole flag byte); the trader record takes
/// the new state. An export switched off gives its foreign collecting piles back (0x557bd0).
pub fn trade_good(world: &mut World, id: u16, good: u32) {
    let g = good as usize;
    let Some(s) = state_mut(world, id) else { return };
    // The original writes +0x1d6 + good for any index; goods above 0x2b only the clear.
    let Some(&v) = s.flags.get(g) else { return };
    let old = v & 0xfc;
    let mut f = v & !old;
    if old < 8 {
        if g <= 0x2b {
            f |= if old == 0 { good_flag::FOREIGN_ZERO } else { good_flag::FOREIGN };
        }
    } else {
        f = 0;
    }
    s.flags[g] = f;
    super::trader::set_good_state(world, id, g, f & 0xfc);
    if old != 0 {
        foreign_good_off(world, id, good as u8, old);
    }
}

/// 0x557bd0: when an export (8) is switched off, its foreign collecting piles (role 3) drop
/// their observers (0x56fac0) and turn receiving (2); +0x22c / +0x201 and the expectation go to
/// 0.
fn foreign_good_off(world: &mut World, id: u16, good: u8, old: u8) {
    if old != 8 {
        return;
    }
    for p in piles(world, id) {
        let Some(d) = pile::get(world, p) else { continue };
        if d.trade_type != 3 || d.good != good {
            continue;
        }
        pile::notify_target_die_and_detach_all(world, p);
        pile::set_trade_type(world, p, 2);
        if let Some(s) = state_mut(world, id) {
            s.foreign_send[good as usize] = 0;
            s.foreign_collect[good as usize] = 0;
        }
        pile::set_expected(world, p, 0);
    }
}

/// 5050 `ChangeTradeStatus(p, on)` (0x557250): the owner's trader sets "trade with player `p`";
/// with nobody left the foreign partner is dropped (0x557890); the building updates next tick.
pub fn set_trade_status(world: &mut World, id: u16, p: u16, on: bool) {
    let owner = owner_of(world, id);
    let Some(t) = world.buildings.traders.get_mut(owner) else { return };
    if let Some(b) = t.trade_with.get_mut(p as usize) {
        *b = on;
    }
    if !t.trades_with_anyone() {
        drop_partner(world, id);
    }
    world.register_for_logic_update(id, 1);
}

/// `CTradingBuildingRole::LogicUpdate` (0x5527d0). The selection part (vtbl+0x58 dialog refresh,
/// the route drawing 0x552b70) is the client's. State 1 (or 2) becomes 3 — a building with an
/// inhabitant type first orders it (vtbl+0x78) — and state 3 clears +5; those states update
/// again in 14 ticks. A working building (flag 0x1000) runs the trade (0x5547b0).
pub fn logic_update(world: &mut World, id: u16) {
    let Some(s) = state(world, id) else { return };
    let st = s.state;
    let mut again = false;
    match st {
        1 => {
            let has_inhabitant = inhabitant_type(world, id) != 0;
            if !has_inhabitant || super::work::order_inhabitant(world, id) {
                set_state(world, id, 3, 0);
            }
            again = true;
        }
        2 => {
            set_state(world, id, 3, 0);
            again = true;
        }
        3 => {
            if let Some(s) = state_mut(world, id) {
                s.sub = 0;
            }
            again = true;
        }
        _ => {}
    }
    if again {
        world.register_for_logic_update(id, 14);
    }
    if building_flags(world, id) & WORKING != 0 {
        foreign_match(world, id);
        trade_update(world, id);
    }
}

// ---- foreign trade: matching and partners ----

/// 0x4ef5d0 (each `LogicUpdate` of a working building, before the trade): with export goods
/// (0x4f06a0) and any trade-with flag, each export is topped up for collecting: +0x201 = 16 −
/// foreign stock (0x553b10) − +0x22c, flag bit 3, +0x22c += that. Then for the players this
/// owner trades with and who trade with it, the first partner 0x4ef7e0 finds: both buildings
/// take each other as partner (0x5575a0, only when they have none) and a foreign transport is
/// ordered (0x554170).
fn foreign_match(world: &mut World, id: u16) {
    let owner = owner_of(world, id);
    let kind = kind_of_building(world, id);
    let Some(t) = world.buildings.traders.get(owner) else { return };
    let exports = t.exports_of(id, kind);
    if exports.is_empty() {
        return;
    }
    if t.trades_with_anyone() {
        for &(_, g) in &exports {
            let gi = g as usize;
            let st = foreign_stock(world, id, g);
            let Some(s) = state_mut(world, id) else { return };
            let Some(&fs) = s.foreign_send.get(gi) else { continue };
            if st + (fs as i32) < 16 {
                let c = (16 - (st + fs as i32)) as i8;
                s.foreign_collect[gi] = c;
                if gi <= 0x2b {
                    s.flags[gi] |= good_flag::FOREIGN;
                }
                s.foreign_send[gi] = fs.wrapping_add(c);
            }
        }
    }
    let n = world.buildings.traders.players.len();
    for p in 1..n {
        let traders = &world.buildings.traders;
        let Some(t) = traders.get(owner) else { return };
        if p > 8 || p == t.player as usize || !t.trade_with[p] {
            continue;
        }
        let Some(other) = traders.get(p as u8) else { continue };
        if !other.trade_with.get(owner as usize).copied().unwrap_or(false) {
            continue;
        }
        let partner = find_partner(world, t, other, id);
        if partner != 0 {
            set_partner(world, id, partner);
            order_transport(world, id, true);
            set_partner(world, partner, id);
            return;
        }
    }
}

/// 0x4ef7e0: for an export good of `b` that the other player imports, the first building in
/// the other player's export list (of `b`'s type) — provided the importer of that good is in
/// `b`'s sector (0x4f02f0), `b` has 16 or more of its good and the importer at least as many of
/// the listed export good. Too few free piles (role 0, 0x557830; 8 goods each) on either side
/// ends the search with none.
fn find_partner(world: &World, ours: &super::trader::Trader, other: &super::trader::Trader, b: u16) -> u16 {
    use super::trader::state as st;
    let kind = kind_of_building(world, b);
    let imports = other.all_in_state(kind, st::IMPORT);
    for (_, g) in ours.exports_of(b, kind) {
        if !imports.iter().any(|i| i.1 == g) {
            continue;
        }
        for (xb, xg) in other.all_in_state(kind, st::EXPORT) {
            let Some(&(pb, _)) = imports.iter().find(|i| i.1 == g) else { continue };
            if !super::trader::same_sector(world, b, pb) {
                continue;
            }
            let s1 = stock(world, pb, xg);
            let s2 = stock(world, b, g);
            if s2 < 16 || s1 < s2 {
                continue;
            }
            if free_piles(world, b) * 8 < s2 || free_piles(world, pb) * 8 < s1 {
                return 0;
            }
            return xb;
        }
    }
    0
}

/// 0x557830: the trade piles with trade role 0.
fn free_piles(world: &World, id: u16) -> i32 {
    trade_piles(world, id).iter().filter(|(_, d)| d.trade_type == 0).count() as i32
}

/// 0x5575a0: take `partner` as the foreign partner unless one is set (the route drawing of a
/// selected building is the client's).
fn set_partner(world: &mut World, id: u16, partner: u16) {
    if let Some(s) = state_mut(world, id)
        && s.partner == 0
    {
        s.partner = partner;
    }
}

/// 0x557a40: collect `good` for the foreign trade while +0x201 asks for more than the foreign
/// piles (role 3) expect or have coming: free piles (role 0, no good) whose eco-sector offers the
/// good take it, role 3 and the expectation, and update next tick. Returns whether one did.
fn foreign_collect(world: &mut World, id: u16, good: u8) -> bool {
    let gi = good as usize;
    let Some(s) = state(world, id) else { return false };
    if gi >= N || s.foreign_collect[gi] == 0 {
        return false;
    }
    let mut need = s.foreign_collect[gi] as i32 - expected(world, id, good, true) - coming(world, id, good, true);
    if need <= 0 {
        return false;
    }
    let mut found = false;
    for p in piles(world, id) {
        let Some(d) = pile::get(world, p) else { continue };
        if d.trade_type != 0 || !eco_offers(world, p, good) {
            continue;
        }
        let n = (8 - d.coming as i32 - d.expected - d.amount as i32).min(need);
        if n != 0 && d.good == 0 {
            let expected = d.expected;
            pile::set_good_type(world, p, good);
            pile::set_trade_type(world, p, 3);
            pile::set_expected(world, p, expected + n);
            world.register_for_logic_update(p, 1);
            need -= n;
            found = true;
        }
    }
    found
}

fn set_state(world: &mut World, id: u16, st: u8, sub: u8) {
    if let Some(s) = state_mut(world, id) {
        s.state = st;
        s.sub = sub;
    }
}

/// buildingInfo +0x1de.
fn inhabitant_type(world: &World, id: u16) -> u16 {
    let Some(e) = world.entities.by_index(id as u32) else { return 0 };
    let EntityData::Building(d) = &e.data else { return 0 };
    world.rules.work.info(e.race, d.type_ex).map_or(0, |i| i.inhabitant)
}

fn kind_of_building(world: &World, id: u16) -> u16 {
    world.entities.by_index(id as u32).map_or(0, |e| e.kind)
}

/// The trade (0x5547b0), for a working building that has its inhabitant or is a marketplace:
/// per good with flags, collect (0x553660, and the foreign 0x557a40), and with goods here or
/// coming, a working target and no transport under way, order one (0x554170). Then the
/// transport loading here loads (0x5551b0 / 0x555b60 / 0x5565e0) and, once nothing more fits or
/// is left, leaves (0x5554d0 / 0x555e40 / 0x5567a0); the foreign partner is dropped when the
/// trader no longer allows it (0x557890); unloading transports unload (0x5555e0 / 0x555ed0 /
/// 0x556980); the piles are tidied (0x553e20).
fn trade_update(world: &mut World, id: u16) {
    let inhabited = super::role(world, id).is_some_and(|r| r.base.worker_in_house);
    if !inhabited && kind_of_building(world, id) != MARKETPLACE {
        return;
    }
    for g in 0..N as u8 {
        let Some(s) = state(world, id) else { return };
        if s.flags[g as usize] == 0 {
            continue;
        }
        collect(world, id, g);
        foreign_collect(world, id, g);
        let target = state(world, id).map_or(0, |s| s.target);
        if target != 0 {
            let here = stock(world, id, g);
            let coming = coming(world, id, g, false);
            if (here > 0 || coming > 0) && building_flags(world, target) & WORKING != 0 && !transport_under_way(world, id) {
                order_transport(world, id, false);
            }
        }
    }
    match loading_kind(world, id) {
        0 => {}
        transport::DONKEY => donkey_load_goods(world, id),
        transport::SHIP => ship_load_goods(world, id),
        _ => cart_load_goods(world, id),
    }
    if nothing_more_to_load(world, id) {
        match loading_kind(world, id) {
            0 => {}
            transport::DONKEY => donkey_leave(world, id),
            transport::SHIP => ship_leave(world, id),
            _ => cart_leave(world, id),
        }
    }
    // The partner must stay in the sector (0x4f02f0) and still import an export (0x4efaa0).
    let partner = state(world, id).map_or(0, |s| s.partner);
    if partner != 0 && !(super::trader::same_sector(world, id, partner) && super::trader::still_trades(world, id, partner)) {
        drop_partner(world, id);
    }
    let Some(s) = state(world, id) else { return };
    let (ship, donkey, cart) = (s.ship_unload.on, s.donkey_unload.on, s.cart_unload.on);
    if ship {
        ship_unload(world, id);
    }
    if donkey {
        donkey_unload(world, id);
    }
    if cart {
        cart_unload(world, id);
    }
    tidy_piles(world, id);
}

// ---- transport records (+0x298) ----

/// 0x552d40: a record `{entity, kind}` exists.
pub fn has_transport(world: &World, id: u16, entity: u16, kind: u32) -> bool {
    state(world, id).is_some_and(|s| s.transports.iter().any(|t| t.entity == entity as u32 && t.kind == kind))
}

/// 0x552dc0: record `{entity, kind, foreign}` unless `{entity, kind}` is recorded.
pub fn add_transport(world: &mut World, id: u16, entity: u16, kind: u32, foreign: bool) -> bool {
    if has_transport(world, id, entity, kind) {
        return false;
    }
    let Some(s) = state_mut(world, id) else { return false };
    s.transports.push(Transport { entity: entity as u32, kind, foreign });
    true
}

/// 0x552e10: with `{entity, kind}` recorded, the first record of the entity (whatever its kind)
/// is replaced by the last one.
pub fn remove_transport(world: &mut World, id: u16, entity: u16, kind: u32) -> bool {
    if !has_transport(world, id, entity, kind) {
        return false;
    }
    let Some(s) = state_mut(world, id) else { return false };
    if let Some(k) = s.transports.iter().position(|t| t.entity == entity as u32) {
        s.transports.swap_remove(k);
    }
    true
}

/// A cart whose target this is was deleted (`CCart` 0x55dbc0): the record of the cart as kind 2
/// goes (0x552e10 — the cart's own record is kind 3, so it stays) and the role clears +0x280,
/// +0x284 and +0x288, the donkey loading / unloading fields, as the original does.
pub fn cart_deleted(world: &mut World, id: u16, cart: u16) {
    remove_transport(world, id, cart, transport::DONKEY);
    if let Some(s) = state_mut(world, id) {
        s.donkey_load.entity = 0;
        s.donkey_load.on = false;
        s.donkey_unload.entity = 0;
    }
}

/// 0x552ee0: records of `kind` with the `foreign` flag.
fn count_transports(world: &World, id: u16, kind: u32, foreign: bool) -> usize {
    state(world, id).map_or(0, |s| s.transports.iter().filter(|t| t.kind == kind && t.foreign == foreign).count())
}

// ---- ships (vehicles V5c) ----

/// The working area (+0x4c) of a building: a port's water.
fn working_area(world: &World, id: u16) -> crate::grid::Y16X16 {
    match world.entities.by_index(id as u32).map(|e| &e.data) {
        Some(EntityData::Building(d)) => d.working_area,
        _ => crate::grid::Y16X16(0),
    }
}

/// 0x555050: the first vehicle of the war-map cell of the port's working area is a transport
/// ship.
fn transport_ship_at_spot(world: &World, id: u16) -> bool {
    ship::ship_in_cell(world, working_area(world, id)).is_some_and(|v| world.entities.by_index(v as u32).is_some_and(|e| e.kind == crate::vehicles::kind::TRANSPORT_SHIP))
}

/// The first vehicle of that cell if it is a ship (types 1..=3), else 0.
fn any_ship_at_spot(world: &World, id: u16) -> u16 {
    ship::ship_in_cell(world, working_area(world, id)).filter(|&v| world.entities.by_index(v as u32).is_some_and(|e| (1..=3).contains(&e.kind))).unwrap_or(0)
}

/// 0x554fc0: a ship loads or unloads here, or the cell's first vehicle is a ship.
fn ship_busy_at_spot(world: &World, id: u16) -> bool {
    let Some(s) = state(world, id) else { return false };
    s.ship_unload.entity != 0 || s.ship_load.entity != 0 || any_ship_at_spot(world, id) != 0
}

/// 0x554f30: the cell's first vehicle if it is a ship, else the unloading, else the loading ship.
fn ship_at_spot(world: &World, id: u16) -> u16 {
    match any_ship_at_spot(world, id) {
        0 => state(world, id).map_or(0, |s| if s.ship_unload.entity != 0 { s.ship_unload.entity as u16 } else { s.ship_load.entity as u16 }),
        v => v,
    }
}

/// 0x554da0 `(this port, target, foreign)`: order a transport ship. Not a port: done (true); a
/// foreign order while the target has ships out: done. A ship record of this kind here: nothing.
/// A transport ship at this port's water (0x555050) while one loads / unloads or a ship is there
/// (0x554fc0): that one (0x554f30), else the nearest free one of the owner (0x4fa050). It must
/// have no trade (state 0): +0x9c this port, +0x98 the target, state 2, recorded here, home where
/// it stands, sailing to the source (+0xa4), a move order to this port's working area, its
/// foreign flag (0x591f80), and this port has one more ship out (+0x2a8).
fn order_ship(world: &mut World, id: u16, target: u16, foreign: bool) -> bool {
    if kind_of_building(world, id) != PORT {
        return true;
    }
    if foreign && state(world, target).is_some_and(|s| s.ships_out > 0) {
        return true;
    }
    if count_transports(world, id, transport::SHIP, foreign) != 0 {
        return false;
    }
    let wa = working_area(world, id);
    let sh = if transport_ship_at_spot(world, id) && ship_busy_at_spot(world, id) {
        match ship_at_spot(world, id) {
            0 => return false,
            v => v,
        }
    } else {
        ship::nearest_free(world, owner_of(world, id), wa.x(), wa.y())
    };
    if sh == 0 || ship::data(world, sh).is_none_or(|t| t.state != 0) {
        return false;
    }
    let here = pos_of(world, sh);
    if let Some(t) = ship::data_mut(world, sh) {
        t.source = id;
        t.target = target;
        t.state = 2;
    }
    add_transport(world, id, sh, transport::SHIP, foreign);
    if let Some(t) = ship::data_mut(world, sh) {
        t.home = here.0 as i32;
        t.to_source = true;
        t.to_target = false;
    }
    crate::vehicles::logic::set_event(world, sh, EntityEvent { event: 0x11, kind: 0xd, data: [0, wa.0 as i32, 0] });
    ship::set_foreign(world, sh, foreign);
    if let Some(s) = state_mut(world, id) {
        s.ships_out += 1;
    }
    true
}

/// 0x555800 (from the ship's arrival 0x591950): the record goes; a ship coming to load (state 2)
/// loads here (0x5550f0: +0x274 / +0x278, state 4; a foreign one on its second leg counts one
/// ship out less), one coming to unload (state 1) unloads (0x555150: +0x27c / +0x279, state 3).
pub fn ship_arrived(world: &mut World, id: u16, sh: u16) {
    remove_transport(world, id, sh, transport::SHIP);
    let Some(t) = ship::data(world, sh) else { return };
    let (st, foreign, leg) = (t.state, t.foreign, t.leg);
    match st {
        2 => {
            if let Some(s) = state_mut(world, id) {
                if foreign && leg == 2 {
                    s.ships_out -= 1;
                }
                s.ship_load = Slot { entity: sh as u32, on: true };
            }
            if let Some(t) = ship::data_mut(world, sh) {
                t.state = 4;
            }
        }
        1 => {
            if let Some(s) = state_mut(world, id) {
                s.ship_unload = Slot { entity: sh as u32, on: true };
            }
            if let Some(t) = ship::data_mut(world, sh) {
                t.state = 3;
            }
        }
        _ => {}
    }
}

/// 0x5551b0: the loading ship takes goods from every pile (once each) whose good is still to be
/// sent (+0x1ab; foreign: +0x22c) and that the target (foreign: the partner) has room for, into a
/// slot that is empty or has the good with room, unless it is full (0x591660). Domestic:
/// collecting piles (role 1); +0x1ab counts down unless endless, and a pile that keeps goods
/// becomes role 3 (0x572230, a foreign collecting pile: the original's). Foreign: as the donkey's
/// (legs 1 / 2, piles 3 / 4, 0x4efbf0, 16 collected or the good aboard, 0x557710, 0x5575e0 /
/// 0x5576a0), without the donkey's turning a reserved pile back to role 3.
fn ship_load_goods(world: &mut World, id: u16) {
    let Some(s) = state(world, id) else { return };
    if !s.ship_load.on {
        return;
    }
    let sh = s.ship_load.entity as u16;
    if !is_vehicle(world, sh) {
        return;
    }
    for p in piles(world, id) {
        let Some(pd) = pile::get(world, p) else { continue };
        let (good, avail, trade, reserve) = (pd.good, pd.amount as i32 - pd.leaving as i32, pd.trade_type, pd.reserve);
        let gi = good as usize;
        let Some(t) = ship::data(world, sh).cloned() else { return };
        let Some(s) = state(world, id) else { return };
        let partner = s.partner;
        let (target, want) = if t.foreign { (s.partner, s.foreign_send.get(gi).copied().unwrap_or(0)) } else { (s.target, s.send.get(gi).copied().unwrap_or(0)) };
        if want <= 0 || avail <= 0 || target == 0 || room_for(world, target, good) == 0 {
            continue;
        }
        let ok = if !t.foreign {
            trade == 1
        } else {
            let leg_pile = (t.leg == 1 && trade == 3) || (t.leg == 2 && trade == 4);
            let exchange = super::trader::trades_good(world, id, target, good);
            let enough = foreign_stock(world, id, good) == 16 || reserved_stock(world, id, good) == 16 || t.has_good(good as i32);
            leg_pile && exchange && enough
        };
        if !ok || !t.has_room(good as i32) || t.is_full() {
            continue;
        }
        let mut n = (want as i32).min(avail);
        if trade == 4 {
            n = reserve;
        }
        if trade == 3 {
            n = n.min(partner_export_stock(world, id, partner));
        }
        let rest = ship::load(world, sh, good as i32, n);
        pile::decrease_unforeseen(world, p, n - rest);
        let k = (n - rest) as i8;
        let left = pile::get(world, p).is_some_and(|d| d.amount != 0);
        if !t.foreign {
            if left {
                pile::set_trade_type(world, p, 3);
            }
            if let Some(s) = state_mut(world, id)
                && s.send[gi] != ENDLESS
            {
                s.send[gi] -= k;
            }
            continue;
        }
        if let Some(s) = state_mut(world, id) {
            s.foreign_send[gi] = s.foreign_send[gi].wrapping_sub(k);
        }
        if t.leg == 1 {
            reserve_for_return(world, target, n);
            keep_free_piles(world, target, n);
            keep_free_piles(world, id, n);
        }
    }
}

/// 0x5554d0: the loading ship leaves for the target (foreign: the partner) if that is a building:
/// recorded there, state 1, its home is now this port's ensign (+0xa0 = +0x48), it sails
/// (0x5919d0); the loading slot is cleared.
fn ship_leave(world: &mut World, id: u16) {
    let Some(s) = state(world, id) else { return };
    let sh = s.ship_load.entity as u16;
    if sh == 0 {
        return;
    }
    let Some(t) = ship::data(world, sh) else { return };
    let foreign = t.foreign;
    let dest = if foreign { s.partner } else { s.target };
    if dest == 0 || !matches!(world.entities.by_index(dest as u32).map(|e| &e.data), Some(EntityData::Building(_))) {
        return;
    }
    add_transport(world, dest, sh, transport::SHIP, foreign);
    let ensign = ensign_of(world, id);
    if let Some(t) = ship::data_mut(world, sh) {
        t.state = 1;
        t.home = ensign.0 as i32;
    }
    ship::sail_to_target(world, sh);
    if let Some(s) = state_mut(world, id) {
        s.ship_load = Slot::default();
    }
}

/// 0x5555e0: the unloading ship puts its goods into this port's piles (0x556d10, foreign as the
/// ship). Empty: the slot is cleared; a domestic ship (or one of this port's owner) calls off
/// this port's loading transports (0x554950), state 0, one ship out less here, home; a foreign
/// one at the partner is forgotten, state 0, and loads here for the way back (0x557420). Goods
/// left that this port has no room for go down at its ensign, 8 at a time; empty then: state 0,
/// slot cleared, then domestic: forgotten and home; foreign: this port's trade runs (0x5547b0).
fn ship_unload(world: &mut World, id: u16) {
    let Some(s) = state(world, id) else { return };
    let sh = s.ship_unload.entity as u16;
    if sh == 0 {
        return;
    }
    let Some(t) = ship::data(world, sh) else { return };
    let foreign = t.foreign;
    let away = foreign && owner_of(world, id) != owner_of(world, sh);
    let total = |world: &World| ship::data(world, sh).map_or(0, |t| t.total());
    for g in 0..N as u8 {
        let n = ship::data(world, sh).map_or(0, |t| t.amount_of(g as i32));
        if n != 0 {
            let rest = unload_into_piles(world, id, g, n, foreign);
            ship::unload(world, sh, g as i32, n - rest);
        }
    }
    if total(world) == 0 {
        if let Some(s) = state_mut(world, id) {
            s.ship_unload = Slot::default();
        }
        if away {
            remove_transport(world, id, sh, transport::SHIP);
            if let Some(t) = ship::data_mut(world, sh) {
                t.state = 0;
            }
            ship_back(world, id, sh);
        } else {
            cancel_loading(world, id);
            if let Some(t) = ship::data_mut(world, sh) {
                t.state = 0;
            }
            if let Some(s) = state_mut(world, id) {
                s.ships_out -= 1;
            }
            ship::go_home(world, sh);
        }
        return;
    }
    let ens = ensign_of(world, id);
    for g in 0..N as u8 {
        let mut n = ship::data(world, sh).map_or(0, |t| t.amount_of(g as i32));
        if n != 0 && room_for(world, id, g) == 0 {
            while n != 0 {
                let k = n.min(ship::SLOT_SIZE);
                pile::search_space_for_goods(world, ens.x(), ens.y(), g, k as u8);
                ship::unload(world, sh, g as i32, k);
                n -= k;
            }
        }
    }
    if total(world) != 0 {
        return;
    }
    if let Some(t) = ship::data_mut(world, sh) {
        t.state = 0;
    }
    if let Some(s) = state_mut(world, id) {
        s.ship_unload = Slot::default();
    }
    if away {
        trade_update(world, id);
    } else {
        remove_transport(world, id, sh, transport::SHIP);
        ship::go_home(world, sh);
    }
}

/// 0x557420 `(ship)`: the foreign ship loads here for the way back: it is the loading ship; one
/// without trade (state 0) gets the old source as its target, this port as its source, state 2,
/// a record here and its next leg (0x591f80 with foreign).
fn ship_back(world: &mut World, id: u16, sh: u16) {
    if let Some(s) = state_mut(world, id) {
        s.ship_load = Slot { entity: sh as u32, on: true };
    }
    let Some(t) = ship::data(world, sh) else { return };
    if t.state != 0 {
        return;
    }
    let foreign = t.foreign;
    if let Some(t) = ship::data_mut(world, sh) {
        t.target = t.source;
        t.source = id;
        t.state = 2;
    }
    add_transport(world, id, sh, transport::SHIP, foreign);
    ship::set_foreign(world, sh, true);
}

// ---- donkeys ----

fn owner_of(world: &World, id: u16) -> u8 {
    world.entities.by_index(id as u32).map_or(0, |e| e.owner)
}

/// 0x5558b0: order a donkey from `id` to `target`: none while a donkey transport (of the
/// `foreign` kind) is recorded (true). The donkey on the ensign, else the nearest free one
/// (0x4fa050); a busy one does nothing (true). The donkey learns source, target, state 2 and
/// where it stands (+0x1c), the source records the transport and the donkey gets event 10.
fn order_donkey(world: &mut World, id: u16, target: u16, foreign: bool) -> bool {
    use crate::jobs::donkey;
    if count_transports(world, id, transport::DONKEY, foreign) != 0 {
        return true;
    }
    let d = match donkey_at_ensign(world, id) {
        0 => {
            let Some(e) = world.entities.by_index(id as u32) else { return false };
            let (owner, ens) = (e.owner, ensign_of(world, id));
            donkey::nearest_free(world, owner, ens.x(), ens.y())
        }
        d => d,
    };
    if d == 0 {
        return false;
    }
    if !donkey::is_free(world, d) {
        return true;
    }
    donkey::order(world, d, id, target);
    add_transport(world, id, d, transport::DONKEY, foreign);
    donkey::send_order_event(world, d, id, foreign);
    true
}

// ---- carts ----

/// `CAStarTiling::FindPath(from, to, flags 2)` as the cart code calls it: whether it returns
/// anything but 0.
fn path_exists(world: &World, from: crate::grid::Y16X16, to: crate::grid::Y16X16) -> bool {
    let mut wp = crate::pathing::Waypoints::default();
    let side = world.grid.size() as usize / 16;
    crate::pathing::astar_tiling(&world.tiling, &world.alliances, &world.warmap, side, from, to, 2, &mut wp) != 0
}

fn pos_of(world: &World, id: u16) -> crate::grid::Y16X16 {
    world.entities.by_index(id as u32).map_or(crate::grid::Y16X16(0), |e| e.pos)
}

/// 0x5562b0 `(this building, target, foreign)`: order a trade cart. Nothing while a cart
/// transport of this kind is recorded or a cart loads here. The cart on the ensign (0x5564a0),
/// else the nearest free one of the owner on the ensign's sector (0x4fa050); it must have no
/// trade (+0x120 0) and a path from it to this building or from this building to the target
/// (`FindPath` flags 2). Recorded here (kind 3), +0x11c this building, +0x118 the target, state 2;
/// a cart not on the ensign remembers where it stood (+0x114), drives (+0x110) to the ensign;
/// one already there arrives at once (0x55cc80). +0x125 = `foreign`.
fn order_cart(world: &mut World, id: u16, target: u16, foreign: bool) -> bool {
    if count_transports(world, id, transport::CART, foreign) != 0 || state(world, id).is_none_or(|s| s.cart_load.entity != 0) {
        return false;
    }
    let at = cart_on_ensign(world, id);
    let c = if at == 0 {
        let ens = ensign_of(world, id);
        cart::nearest_free(world, owner_of(world, id), ens.x(), ens.y())
    } else {
        at
    };
    if c == 0 || cart::data(world, c).is_none_or(|cd| cd.state != 0) {
        return false;
    }
    let reach = path_exists(world, pos_of(world, c), pos_of(world, id)) | path_exists(world, pos_of(world, id), pos_of(world, target));
    if !reach {
        return false;
    }
    add_transport(world, id, c, transport::CART, foreign);
    let here = pos_of(world, c);
    if let Some(cd) = cart::data_mut(world, c) {
        cd.source = id as i32;
        cd.target = target as i32;
        cd.state = 2;
    }
    if at == 0 {
        let ens = ensign_of(world, id);
        if let Some(cd) = cart::data_mut(world, c) {
            cd.home = here.0 as i32;
            cd.to_source = true;
            cd.to_target = false;
        }
        crate::vehicles::logic::set_event(world, c, EntityEvent { event: 0x11, kind: 0xd, data: [0, ens.0 as i32, 0] });
    } else {
        if let Some(cd) = cart::data_mut(world, c) {
            cd.to_source = false;
        }
        cart::arrived_at_building(world, c);
    }
    if let Some(cd) = cart::data_mut(world, c) {
        cd.foreign = foreign;
    }
    true
}

/// 0x556c70 (from the cart's 0x55cc80): the cart at this building's ensign. The record goes; a
/// cart coming to load (state 2) becomes the loading cart (0x556520: +0x28c / +0x290, state
/// 4), one coming to unload (state 1) the unloading one (0x556580: +0x294 / +0x291, state 3).
pub fn cart_arrived(world: &mut World, id: u16, c: u16) {
    remove_transport(world, id, c, transport::CART);
    let st = cart::data(world, c).map_or(0, |cd| cd.state);
    let (slot, next) = match st {
        2 => (true, 4),
        1 => (false, 3),
        _ => return,
    };
    if let Some(s) = state_mut(world, id) {
        let sl = Slot { entity: c as u32, on: true };
        if slot {
            s.cart_load = sl;
        } else {
            s.cart_unload = sl;
        }
    }
    if let Some(cd) = cart::data_mut(world, c) {
        cd.state = next;
    }
}

/// 0x5565e0: the loading cart (forgotten when it is no vehicle) takes goods from the first trade
/// pile (role 1) whose good is traded (flag 2) and still to send, that has goods not leaving,
/// for which the target has room and a slot of the cart fits: `min(send, available)`, what fit
/// leaves the pile (0x56f8d0) and the send amount (not when endless). One pile per update.
fn cart_load_goods(world: &mut World, id: u16) {
    let Some(s) = state(world, id) else { return };
    let c = s.cart_load.entity as u16;
    if !is_vehicle(world, c)
        && let Some(s) = state_mut(world, id)
    {
        s.cart_load.on = false;
    }
    let Some(s) = state(world, id) else { return };
    if !s.cart_load.on {
        return;
    }
    for p in piles(world, id) {
        let Some(pd) = pile::get(world, p) else { continue };
        let (good, avail, trade) = (pd.good, pd.amount as i32 - pd.leaving as i32, pd.trade_type);
        let gi = good as usize;
        let Some(s) = state(world, id) else { return };
        let (send, target) = (s.send.get(gi).copied().unwrap_or(0), s.target);
        if trade != 1 || s.flags.get(gi).copied() != Some(2) || send <= 0 || avail <= 0 || target == 0 || room_for(world, target, good) == 0 {
            continue;
        }
        let fits = cart::data(world, c).is_some_and(|cd| cd.slots.iter().any(|sl| sl[0] == 0 || (sl[0] == good as i32 && sl[1] < cart::SLOT_SIZE)));
        if !fits {
            continue;
        }
        let n = (send as i32).min(avail);
        let rest = cart::load(world, c, good as i32, n);
        pile::decrease_unforeseen(world, p, n - rest);
        if let Some(s) = state_mut(world, id)
            && s.send[gi] != ENDLESS
        {
            s.send[gi] -= (n - rest) as i8;
        }
        return;
    }
}

/// 0x5567a0: the loading cart leaves. With a path from it to the target's ensign (`FindPath`
/// flags 2): recorded at the target (unless already, kind 3, its +0x125), state 1, the drive
/// (0x55ccf0), its update next tick. Without: the goods this building has no room for go down
/// at the target's ensign, 8 at a time, and the cart goes home (0x55b8a0). The loading slot is
/// cleared.
fn cart_leave(world: &mut World, id: u16) {
    let Some(s) = state(world, id) else { return };
    let (c, target) = (s.cart_load.entity as u16, s.target);
    if c == 0 {
        return;
    }
    let target_ensign = ensign_of(world, target);
    if path_exists(world, pos_of(world, c), target_ensign) {
        let foreign = cart::data(world, c).is_some_and(|cd| cd.foreign);
        add_transport(world, target, c, transport::CART, foreign);
        if let Some(cd) = cart::data_mut(world, c) {
            cd.state = 1;
        }
        cart::drive_to_target(world, c);
        if world.entities.by_index(c as u32).is_some() {
            world.register_for_logic_update(c, 1);
        }
    } else {
        for g in 0..N as u8 {
            let mut n = cart::data(world, c).map_or(0, |cd| cd.amount_of(g as i32));
            if n != 0 && room_for(world, id, g) == 0 {
                while n != 0 {
                    let k = n.min(cart::SLOT_SIZE);
                    pile::search_space_for_goods(world, target_ensign.x(), target_ensign.y(), g, k as u8);
                    cart::unload(world, c, g as i32, k);
                    n -= k;
                }
            }
        }
        cart::go_home(world, c);
    }
    if let Some(s) = state_mut(world, id) {
        s.cart_load = Slot::default();
    }
}

/// 0x556980: the unloading cart (no longer foreign) puts its goods into this building's piles
/// (0x556d10); the goods there is no room for go down at the ensign, 8 at a time. Once empty
/// the cart has no target, no trade, the unloading slot is cleared and it goes home (0x55b8a0).
fn cart_unload(world: &mut World, id: u16) {
    let Some(s) = state(world, id) else { return };
    let c = s.cart_unload.entity as u16;
    if c == 0 || !is_vehicle(world, c) || cart::data(world, c).is_none() {
        return;
    }
    if let Some(cd) = cart::data_mut(world, c) {
        cd.foreign = false;
    }
    let total = |world: &World| cart::data(world, c).map_or(0, |cd| (0..N as i32).map(|g| cd.amount_of(g)).sum::<i32>());
    for g in 0..N as u8 {
        let n = cart::data(world, c).map_or(0, |cd| cd.amount_of(g as i32));
        if n != 0 {
            let rest = unload_into_piles(world, id, g, n, false);
            cart::unload(world, c, g as i32, n - rest);
        }
    }
    if total(world) != 0 {
        let ens = ensign_of(world, id);
        for g in 0..N as u8 {
            let mut n = cart::data(world, c).map_or(0, |cd| cd.amount_of(g as i32));
            if n != 0 && room_for(world, id, g) == 0 {
                while n != 0 {
                    let k = n.min(cart::SLOT_SIZE);
                    pile::search_space_for_goods(world, ens.x(), ens.y(), g, k as u8);
                    cart::unload(world, c, g as i32, k);
                    n -= k;
                }
            }
        }
        if total(world) != 0 {
            return;
        }
    }
    if let Some(cd) = cart::data_mut(world, c) {
        cd.target = 0;
        cd.state = 0;
    }
    if let Some(s) = state_mut(world, id) {
        s.cart_unload = Slot::default();
    }
    cart::go_home(world, c);
}

/// 0x5561f0 (from the donkey's arrival 0x576d80): the market forgets the transport; a donkey
/// coming to load (state 2) that stands on the ensign starts loading (0x555ac0: +0x284 / +0x280,
/// displacement 10, state 4), one coming to unload (state 1) starts unloading (0x555b10: +0x285 /
/// +0x288, state 3).
pub fn donkey_arrived(world: &mut World, id: u16, donkey: u16) {
    use crate::jobs::donkey::{self as dk, state as ds};
    remove_transport(world, id, donkey, transport::DONKEY);
    let st = dk::data(world, donkey).map_or(0, |d| d.state);
    let next = match st {
        ds::TO_SOURCE if donkey_at_ensign(world, id) == donkey => {
            if let Some(s) = state_mut(world, id) {
                s.donkey_load = Slot { entity: donkey as u32, on: true };
            }
            ds::LOADING
        }
        ds::TO_TARGET => {
            if let Some(s) = state_mut(world, id) {
                s.donkey_unload = Slot { entity: donkey as u32, on: true };
            }
            ds::UNLOADING
        }
        _ => return,
    };
    if let Some(j) = crate::jobs::core::job_mut(world, donkey) {
        j.displacement_costs = 10;
    }
    if let Some(d) = dk::data_mut(world, donkey) {
        d.state = next;
    }
}

/// 0x555b60: the loading donkey takes goods still to be sent (+0x1ab; foreign: +0x22c) whose good
/// the target (foreign: the partner) has room for, per pile at most once, into the first slot
/// that is empty or has the good and room. Domestic: from the collecting piles (role 1), +0x1ab
/// counts down unless endless. Foreign: on the way there (leg 1) from the foreign collecting
/// piles (3), back (leg 2) from the piles the partner reserved (4, their reserve), only goods
/// this building exports and the partner's owner imports (0x4efbf0) and only once 16 are
/// collected (0x553b10 / 0x553bd0) or the donkey already has the good; a role-3 pile gives at
/// most what the partner has of its exports (0x557710). +0x22c counts down; on leg 1 the
/// partner reserves as many of its goods for the way back (0x5575e0) and both sides keep free
/// piles for the goods coming (0x5576a0); a reserved pile with goods left is role 3 again.
fn donkey_load_goods(world: &mut World, id: u16) {
    use crate::jobs::donkey;
    let Some(s) = state(world, id) else { return };
    if !s.donkey_load.on {
        return;
    }
    let d = s.donkey_load.entity as u16;
    for p in piles(world, id) {
        let Some(pd) = pile::get(world, p) else { continue };
        let (good, avail, trade, reserve) = (pd.good, pd.amount as i32 - pd.leaving as i32, pd.trade_type, pd.reserve);
        let gi = good as usize;
        let Some(dd) = donkey::data(world, d).copied() else { return };
        let Some(s) = state(world, id) else { return };
        let (target, want) = if dd.foreign { (s.partner, s.foreign_send.get(gi).copied().unwrap_or(0)) } else { (s.target, s.send.get(gi).copied().unwrap_or(0)) };
        if want <= 0 || avail <= 0 || target == 0 || room_for(world, target, good) == 0 {
            continue;
        }
        let ok = if !dd.foreign {
            trade == 1
        } else {
            let leg_pile = (dd.leg == 1 && trade == 3) || (dd.leg == 2 && trade == 4);
            let exchange = super::trader::trades_good(world, id, target, good);
            let enough = foreign_stock(world, id, good) == 16 || reserved_stock(world, id, good) == 16 || dd.slots.iter().any(|&(g, n)| n > 0 && g == good);
            leg_pile && exchange && enough
        };
        if !ok {
            continue;
        }
        let fits = dd.slots.iter().any(|&(g, n)| g == 0 || (g == good && donkey::SLOT_SIZE - n > 0));
        if !fits {
            continue;
        }
        let mut n = (want as i32).min(avail);
        if trade == 4 {
            n = reserve;
        }
        if trade == 3 {
            n = n.min(partner_export_stock(world, id, target));
        }
        let rest = donkey::load(world, d, good, n);
        pile::decrease_unforeseen(world, p, n - rest);
        let k = (n - rest) as i8;
        let Some(s) = state_mut(world, id) else { return };
        if !dd.foreign {
            if s.send[gi] != ENDLESS {
                s.send[gi] -= k;
            }
            continue;
        }
        s.foreign_send[gi] = s.foreign_send[gi].wrapping_sub(k);
        if dd.leg == 1 {
            reserve_for_return(world, target, n);
            keep_free_piles(world, target, n);
            keep_free_piles(world, id, n);
        }
        if trade == 4 && pile::get(world, p).is_some_and(|d| d.amount > 0) {
            pile::set_trade_type(world, p, 3);
        }
    }
}

/// 0x553bd0: goods in the reserved piles (role 4) of `good`, for goods with the export bit.
fn reserved_stock(world: &World, id: u16, good: u8) -> i32 {
    let Some(s) = state(world, id) else { return 0 };
    trade_piles(world, id)
        .iter()
        .filter(|(_, d)| s.flags.get(d.good as usize).is_some_and(|f| f & good_flag::FOREIGN != 0) && d.trade_type == 4 && d.good == good)
        .map(|(_, d)| d.amount as i32)
        .sum()
}

/// 0x557710 (on this building, for the partner): the goods the partner has in its piles of the
/// goods it exports (its owner's record).
fn partner_export_stock(world: &World, id: u16, partner: u16) -> i32 {
    let _ = id;
    let exports = super::trader::exports_of(world, partner);
    let piles = trade_piles(world, partner);
    exports.iter().map(|&(_, g)| piles.iter().filter(|(_, d)| d.good == g).map(|(_, d)| d.amount as i32).sum::<i32>()).sum()
}

/// 0x5575e0 (on the partner): reserve `n` goods of its foreign collecting piles (role 3 with
/// goods) for the way back: they become role 4 and their reserve (+0x10) grows by what they have
/// beyond it.
fn reserve_for_return(world: &mut World, id: u16, n: i32) {
    let mut n = n;
    for p in piles(world, id) {
        let Some(d) = pile::get(world, p) else { continue };
        if d.trade_type != 3 || n <= 0 || d.amount == 0 {
            continue;
        }
        pile::set_trade_type(world, p, 4);
        let Some(d) = pile::get(world, p) else { continue };
        let r = d.reserve;
        let mut k = d.amount as i32 - r;
        if k < 1 {
            k = 0;
        } else if k < n {
            pile::set_reserve(world, p, r + k);
        } else {
            pile::set_reserve(world, p, r + n);
            k = n;
        }
        n -= k;
    }
}

/// 0x5576a0: free piles (role 0) are kept for the foreign goods coming (role 5), one per 8.
fn keep_free_piles(world: &mut World, id: u16, n: i32) {
    let mut n = n;
    for p in piles(world, id) {
        if n > 0 && pile::get(world, p).is_some_and(|d| d.trade_type == 0) {
            pile::set_trade_type(world, p, 5);
            n -= 8;
        }
    }
}

/// 0x5543b0: the loading transport may leave: it is full (0x576a70), or the collecting piles
/// of goods still to be sent that it could take (and the target has room for) have nothing
/// more: no goods, none coming, and no expectation the eco-sector can serve (the original
/// compares the expectation with the head of the eco-sector's offer list of the good, a pile
/// id). False without a loading transport.
fn nothing_more_to_load(world: &mut World, id: u16) -> bool {
    use crate::jobs::donkey;
    let Some(s) = state(world, id) else { return false };
    // The loader's slots `(good, amount)` and whether it is foreign: the loading ship (none when
    // it is no vehicle: then every pile fits), else the loading donkey, else the loading cart (one
    // that is no longer a vehicle is forgotten).
    let (slots, foreign): (Vec<(u8, i32)>, bool) = if s.ship_load.on {
        let sh = s.ship_load.entity as u16;
        match ship::data(world, sh).filter(|_| is_vehicle(world, sh)) {
            Some(t) if t.is_full() => return true,
            Some(t) => (t.slots.iter().map(|sl| (sl[0] as u8, sl[1])).collect(), t.foreign),
            None => (Vec::new(), false),
        }
    } else if s.donkey_load.on {
        let d = s.donkey_load.entity as u16;
        let dd = donkey::data(world, d).copied();
        if dd.is_some() && donkey::is_full(world, d) {
            return true;
        }
        match dd {
            Some(dd) => (dd.slots.to_vec(), dd.foreign),
            None => (Vec::new(), false),
        }
    } else if s.cart_load.on {
        let c = s.cart_load.entity as u16;
        let Some(cd) = cart::data(world, c).filter(|_| is_vehicle(world, c)).cloned() else {
            if let Some(s) = state_mut(world, id) {
                s.cart_load = Slot::default();
            }
            return false;
        };
        if cd.slots.iter().all(|sl| sl[0] != 0 && sl[1] >= cart::SLOT_SIZE) {
            return true;
        }
        (cd.slots.iter().map(|sl| (sl[0] as u8, sl[1])).collect(), cd.foreign)
    } else {
        return false;
    };
    let size = if s.ship_load.on {
        ship::SLOT_SIZE
    } else if s.donkey_load.on {
        donkey::SLOT_SIZE
    } else {
        cart::SLOT_SIZE
    };
    // A foreign loader checks the partner's room (the pile test stays the domestic one).
    let target = if foreign { s.partner } else { s.target };
    let mut sum = 0;
    for p in piles(world, id) {
        let Some(pd) = pile::get(world, p) else { continue };
        let good = pd.good as usize;
        let Some(s) = state(world, id) else { return false };
        if pd.trade_type != 1 || s.flags.get(good).is_none_or(|&f| f == 0) || s.send[good] == 0 {
            continue;
        }
        let fits = slots.is_empty() || slots.iter().any(|&(g, n)| g == 0 || (g == pd.good && n != size && n <= size));
        if !fits || target == 0 || room_for(world, target, pd.good) == 0 {
            continue;
        }
        sum += pd.amount as i32 + pd.coming as i32;
        if pd.expected != 0 {
            let head = world
                .entities
                .by_index(p as u32)
                .and_then(|e| world.grid.checked_index(e.x(), e.y()))
                .map(|n| eco::eco_of(world, n))
                .and_then(|e| world.economy.get(e))
                .map_or(0, |e| e.good_offer[good] as i32);
            if pd.expected <= head {
                sum += pd.expected;
            }
        }
    }
    sum == 0
}

/// 0x555e40: the loading donkey leaves for the target: +0x34 target, +0x38 this market, state
/// 1, its update next tick; the loading slot is cleared.
fn donkey_leave(world: &mut World, id: u16) {
    let Some(s) = state(world, id) else { return };
    let d = s.donkey_load.entity as u16;
    if d == 0 {
        return;
    }
    let target = if crate::jobs::donkey::data(world, d).is_some_and(|dd| dd.foreign) { s.partner } else { s.target };
    if let Some(dd) = crate::jobs::donkey::data_mut(world, d) {
        dd.target = target;
        dd.source = id;
        dd.state = crate::jobs::donkey::state::TO_TARGET;
    }
    if world.entities.by_index(d as u32).is_some() {
        world.register_for_logic_update(d, 1);
    }
    if let Some(s) = state_mut(world, id) {
        s.donkey_load = Slot::default();
    }
}

/// 0x555ed0: the unloading donkey puts its goods into this market's piles (0x556d10); the goods
/// this market has no room for go down where the donkey stands. Once it is empty the market
/// forgets it and frees it: no target, displacement 5, state 0, its update next tick, home
/// (0x577010).
fn donkey_unload(world: &mut World, id: u16) {
    use crate::jobs::donkey;
    let Some(s) = state(world, id) else { return };
    let d = s.donkey_unload.entity as u16;
    if d == 0 {
        return;
    }
    let Some(dd) = donkey::data(world, d).copied() else { return };
    // Another player's foreign donkey: emptied, it is not freed but loaded here for the way back.
    let foreign_other = dd.foreign && owner_of(world, id) != owner_of(world, d);
    let mut left = 0;
    for g in 0..N as u8 {
        let n = donkey::amount_of(world, d, g);
        if n != 0 {
            let rest = unload_into_piles(world, id, g, n, dd.foreign);
            donkey::unload(world, d, g, n - rest);
        }
        left += donkey::amount_of(world, d, g);
    }
    if left != 0 {
        let pos = world.entities.by_index(d as u32).map_or(crate::grid::Y16X16(0), |e| e.pos);
        for g in 0..N as u8 {
            let mut n = donkey::amount_of(world, d, g);
            if n != 0 && room_for(world, id, g) == 0 {
                while n != 0 {
                    let k = n.min(donkey::SLOT_SIZE);
                    pile::search_space_for_goods(world, pos.x(), pos.y(), g, k as u8);
                    donkey::unload(world, d, g, k);
                    n -= k;
                }
            }
        }
        if (0..N as u8).any(|g| donkey::amount_of(world, d, g) != 0) {
            return;
        }
        // Emptied by dropping: freed; a foreign one of another player makes this building run
        // its trade again (0x5547b0) instead of going home.
        free_unloaded(world, id, d);
        if foreign_other {
            trade_update(world, id);
        } else {
            donkey::go_home(world, d);
        }
        return;
    }
    free_unloaded(world, id, d);
    let partner = state(world, id).map_or(0, |s| s.partner);
    if foreign_other && partner != 0 {
        take_back_order(world, id, d);
    } else {
        donkey::go_home(world, d);
    }
}

/// The end of 0x555ed0: the market forgets the unloading donkey, which has no target, state 0,
/// displacement 5 and its update next tick.
fn free_unloaded(world: &mut World, id: u16, d: u16) {
    use crate::jobs::donkey;
    if let Some(s) = state_mut(world, id) {
        s.donkey_unload = Slot::default();
    }
    if let Some(dd) = donkey::data_mut(world, d) {
        dd.target = 0;
        dd.state = donkey::state::FREE;
    }
    if let Some(j) = crate::jobs::core::job_mut(world, d) {
        j.displacement_costs = 5;
    }
    if world.entities.by_index(d as u32).is_some() {
        world.register_for_logic_update(d, 1);
    }
}

/// 0x5572e0: the foreign donkey just unloaded here becomes this building's loading donkey and,
/// free, gets the order back: target = its old source, source = here, state 2, recorded here,
/// event 10, the next leg (0x577620).
fn take_back_order(world: &mut World, id: u16, d: u16) {
    use crate::jobs::donkey;
    if let Some(s) = state_mut(world, id) {
        s.donkey_load = Slot { entity: d as u32, on: true };
    }
    let Some(dd) = donkey::data(world, d).copied() else { return };
    if dd.state != donkey::state::FREE {
        return;
    }
    if let Some(m) = donkey::data_mut(world, d) {
        m.target = dd.source;
        m.source = id;
        m.state = donkey::state::TO_SOURCE;
    }
    add_transport(world, id, d, transport::DONKEY, dd.foreign);
    donkey::send_order_event(world, d, id, true);
}

/// 0x556d10: put `n` goods of `good` (converted to the owner's race) into this market's piles:
/// first a pile of the good with room `8 − expected − coming − amount` (domestic only), else an
/// empty free pile (role 0; for the foreign trade roles 1 / 5, or 0 without a partner), which
/// takes the good and turns receiving (role 2). One good at a time (`IncreaseUnforeseen`), the
/// pile's update next tick. Returns what did not fit.
fn unload_into_piles(world: &mut World, id: u16, good: u8, n: i32, foreign: bool) -> i32 {
    let owner = owner_of(world, id);
    let g = pile::convert_good_type(good, world.race_of(owner));
    let fill = |world: &mut World, p: u16, k: i32| {
        for _ in 0..k.max(0) {
            pile::increase_unforeseen(world, p, 1);
        }
        world.register_for_logic_update(p, 1);
    };
    for p in piles(world, id) {
        let Some(pd) = pile::get(world, p) else { continue };
        if pd.good != g || foreign {
            continue;
        }
        let room = (pile::MAX_AMOUNT as i32 - pd.expected - pd.coming as i32 - pd.amount as i32).min(n);
        if room != 0 {
            fill(world, p, room);
            return n - room;
        }
    }
    let n8 = n.min(pile::MAX_AMOUNT as i32);
    let partner = state(world, id).map_or(0, |s| s.partner);
    for p in piles(world, id) {
        let Some(pd) = pile::get(world, p) else { continue };
        let ok = if foreign { matches!(pd.trade_type, 1 | 5) || (partner == 0 && pd.trade_type == 0) } else { pd.trade_type == 0 };
        if ok && n8 <= pile::MAX_AMOUNT as i32 - pd.amount as i32 && pd.amount == 0 {
            let k = n.min(pile::MAX_AMOUNT as i32);
            if k != 0 {
                pile::set_good_type(world, p, g);
                pile::set_trade_type(world, p, 2);
                fill(world, p, k);
                return n - k;
            }
        }
    }
    n
}

/// 0x5571b0: which transport loads here: a cart (vehicle kind 5) on the ensign (3), a donkey on
/// the ensign (2), a port trading with a port (1), else 0. Only with a target or partner.
fn loading_kind(world: &World, id: u16) -> u32 {
    let Some(s) = state(world, id) else { return 0 };
    if s.target == 0 && s.partner == 0 {
        return 0;
    }
    if cart_on_ensign(world, id) != 0 {
        return transport::CART;
    }
    if donkey_at_ensign(world, id) != 0 {
        return transport::DONKEY;
    }
    let port = |b: u16| b != 0 && kind_of_building(world, b) == PORT;
    if kind_of_building(world, id) == PORT && (port(s.target) || port(s.partner)) {
        return transport::SHIP;
    }
    0
}

/// Building +0x48, the ensign.
fn ensign_of(world: &World, id: u16) -> crate::grid::Y16X16 {
    match world.entities.by_index(id as u32).map(|e| &e.data) {
        Some(EntityData::Building(d)) => d.ensign,
        _ => crate::grid::Y16X16(0),
    }
}

fn ensign_node(world: &World, id: u16) -> Option<usize> {
    let p = ensign_of(world, id);
    world.grid.checked_index(p.x(), p.y())
}

/// 0x555a60: the donkey (settler type 60) standing on the building's ensign, or 0.
pub fn donkey_at_ensign(world: &World, id: u16) -> u16 {
    let Some(n) = ensign_node(world, id) else { return 0 };
    let m = world.layers.map_object[n];
    match world.entities.by_index(m as u32) {
        Some(e) if e.obj_type == crate::entity::ObjType::Settler && e.kind == DONKEY_SETTLER => m,
        _ => 0,
    }
}

/// 0x5564a0: the loading cart (+0x28c), else a cart (vehicle kind 5) whose node is the ensign,
/// or 0.
fn cart_on_ensign(world: &World, id: u16) -> u16 {
    let Some(s) = state(world, id) else { return 0 };
    if s.cart_load.entity != 0 {
        return s.cart_load.entity as u16;
    }
    let Some(n) = ensign_node(world, id) else { return 0 };
    let m = world.layers.map_object[n];
    match world.entities.by_index(m as u32) {
        Some(e) if e.obj_type == crate::entity::ObjType::LandVehicle && e.kind == crate::vehicles::kind::CART => m,
        _ => 0,
    }
}

fn is_vehicle(world: &World, id: u16) -> bool {
    id != 0 && world.entities.by_index(id as u32).is_some_and(|e| matches!(e.obj_type, crate::entity::ObjType::Ship | crate::entity::ObjType::LandVehicle))
}

/// The trade piles with their pile data.
fn trade_piles(world: &World, id: u16) -> Vec<(u16, crate::entity::PileData)> {
    piles(world, id).into_iter().filter_map(|p| pile::get(world, p).map(|d| (p, d.clone()))).collect()
}

/// 0x553a70: goods here in the collecting piles (trade roles 1 / 3) of `good`.
pub fn stock(world: &World, id: u16, good: u8) -> i32 {
    trade_piles(world, id).iter().filter(|(_, d)| matches!(d.trade_type, 1 | 3) && d.good == good).map(|(_, d)| d.amount as i32).sum()
}

/// 0x553c90: goods on their way (+0x43) to the collecting piles (role 1, or 3 when `foreign`) of
/// `good`; 0 unless the good is still to be collected (+0x180 or +0x201).
pub fn coming(world: &World, id: u16, good: u8, foreign: bool) -> i32 {
    let Some(s) = state(world, id) else { return 0 };
    if good as usize >= N || (s.collect[good as usize] == 0 && s.foreign_collect[good as usize] == 0) {
        return 0;
    }
    let t = if foreign { 3 } else { 1 };
    trade_piles(world, id).iter().filter(|(_, d)| d.trade_type == t && d.good == good).map(|(_, d)| d.coming as i32).sum()
}

/// 0x553d40: goods expected (`CTradePileRole +0xc`) by the collecting piles of `good`.
pub fn expected(world: &World, id: u16, good: u8, foreign: bool) -> i32 {
    let t = if foreign { 3 } else { 1 };
    trade_piles(world, id).iter().filter(|(_, d)| d.trade_type == t && d.good == good).map(|(_, d)| d.expected).sum()
}

/// 0x556f90: collecting piles (roles 1 / 3) of `good`.
fn collecting_piles(world: &World, id: u16, good: u8) -> i32 {
    trade_piles(world, id).iter().filter(|(_, d)| matches!(d.trade_type, 1 | 3) && d.good == good).count() as i32
}

/// The eco-sector of a pile's node offers `good` (`CEcoSector +0x130`, the offer list head).
fn eco_offers(world: &World, p: u16, good: u8) -> bool {
    let Some(e) = world.entities.by_index(p as u32) else { return false };
    let Some(n) = world.grid.checked_index(e.x(), e.y()) else { return false };
    let eco = eco::eco_of(world, n);
    eco != 0 && world.economy.get(eco).is_some_and(|s| s.good_offer[good as usize] != 0)
}

/// 0x553660: collect `good` for the domestic trade while +0x180 asks for more than the piles
/// expect or have coming: (1) piles that received the good from another market (role 2) collect
/// it again (role 1, or 3 for the foreign flag), their goods counting against +0x180 unless it
/// is endless; (2) collecting piles of the good expect more, up to their room; (3) free piles
/// (role 0) whose eco-sector offers the good start collecting it. With the domestic flag, at most
/// three piles collect one good (`> 2` stops). Returns whether anything changed.
fn collect(world: &mut World, id: u16, good: u8) -> bool {
    let gi = good as usize;
    let Some(s) = state(world, id) else { return false };
    if gi >= N || s.collect[gi] == 0 {
        return false;
    }
    let _ = stock(world, id, good);
    let mut need = s.collect[gi] as i32 - expected(world, id, good, false) - coming(world, id, good, false);
    if need <= 0 {
        return false;
    }
    let domestic = |w: &World| state(w, id).is_some_and(|s| s.flags[gi] & good_flag::DOMESTIC != 0);
    let foreign = |w: &World| state(w, id).is_some_and(|s| s.flags[gi] & good_flag::FOREIGN != 0);
    let remaining = |w: &World| state(w, id).map_or(0, |s| s.collect[gi]);
    // (1) role 2 piles of the good.
    for p in piles(world, id) {
        let Some(d) = pile::get(world, p) else { continue };
        if d.trade_type != 2 || d.good != good || d.amount == 0 {
            continue;
        }
        let amount = d.amount as i32;
        if domestic(world) {
            pile::set_trade_type(world, p, 1);
        }
        if foreign(world) {
            pile::set_trade_type(world, p, 3);
        }
        let n = amount.min(need);
        if remaining(world) != ENDLESS {
            need -= n;
            if let Some(s) = state_mut(world, id) {
                s.collect[gi] -= n as i8;
            }
        }
        world.register_for_logic_update(p, 1);
        if domestic(world) && (collecting_piles(world, id, good) > 2 || remaining(world) == 0) {
            return true;
        }
    }
    if need == 0 {
        return true;
    }
    if domestic(world) && collecting_piles(world, id, good) >= 3 {
        return false;
    }
    // (2) collecting piles of the good.
    for p in piles(world, id) {
        let Some(d) = pile::get(world, p) else { continue };
        if !matches!(d.trade_type, 1 | 3) || d.good != good {
            continue;
        }
        let room = 8 - d.expected - d.coming as i32 - d.amount as i32;
        let n = room.min(need);
        if n != 0 {
            pile::set_expected(world, p, d.expected + n);
            world.register_for_logic_update(p, 1);
            need -= n;
        }
    }
    if need == 0 {
        return true;
    }
    if domestic(world) && collecting_piles(world, id, good) >= 3 {
        return false;
    }
    // (3) free piles where the eco-sector offers the good.
    let mut found = false;
    for p in piles(world, id) {
        let Some(d) = pile::get(world, p) else { continue };
        if d.trade_type != 0 || !eco_offers(world, p, good) {
            continue;
        }
        let room = 8 - d.coming as i32 - d.expected - d.amount as i32;
        let n = room.min(need);
        if n == 0 {
            continue;
        }
        let expected = d.expected;
        pile::set_good_type(world, p, good);
        pile::set_trade_type(world, p, if domestic(world) { 1 } else { 3 });
        pile::set_expected(world, p, expected + n);
        world.register_for_logic_update(p, 1);
        let before = remaining(world);
        found = true;
        if collecting_piles(world, id, good) > 2 {
            return true;
        }
        if before != ENDLESS {
            need -= n;
        }
        if remaining(world) == 0 {
            return true;
        }
    }
    found
}

/// 0x553e20 (each trade update of a building): an empty receiving pile (role 2) becomes free
/// (good 0); a free pile with goods turns receiving unless a foreign partner is set; a collecting
/// pile (role 1) with goods whose good is no longer to be sent turns receiving.
fn tidy_piles(world: &mut World, id: u16) {
    for p in piles(world, id) {
        let Some(d) = pile::get(world, p) else { continue };
        let (t, amount) = (d.trade_type, d.amount);
        if amount == 0 && t == 2 {
            pile::set_trade_type(world, p, 0);
            world.register_for_logic_update(p, 1);
            pile::set_good_type(world, p, 0);
        }
        let Some(d) = pile::get(world, p) else { continue };
        let partner = state(world, id).map_or(0, |s| s.partner);
        if d.trade_type == 0 && d.amount > 0 && partner == 0 {
            pile::set_trade_type(world, p, 2);
        }
        let Some(d) = pile::get(world, p) else { continue };
        let send = state(world, id).map_or(0, |s| s.send.get(d.good as usize).copied().unwrap_or(0));
        if d.amount != 0 && send == 0 && d.trade_type == 1 {
            pile::set_trade_type(world, p, 2);
            world.register_for_logic_update(p, 1);
        }
    }
}

/// 0x5542d0: a transport of the right kind is under way for the domestic trade: a ship between
/// two ports, else a cart or a donkey.
fn transport_under_way(world: &World, id: u16) -> bool {
    let Some(s) = state(world, id) else { return false };
    if s.target == 0 {
        return false;
    }
    let count = |k: u32| s.transports.iter().filter(|t| t.kind == k && !t.foreign).count();
    let (here, there) = (kind_of_building(world, id), kind_of_building(world, s.target));
    if there == PORT && here == PORT {
        return count(transport::SHIP) != 0;
    }
    if there == MARKETPLACE || here == MARKETPLACE {
        return count(transport::CART) != 0 || count(transport::DONKEY) != 0;
    }
    false
}

/// 0x557000: goods to send (+0x1ab, or the foreign ones), where a target exists only the goods
/// the target has room for (0x552340 on the target) and whose flag allows it (adding +0x1ab
/// also for the foreign trade, as the original).
pub fn goods_to_send(world: &World, id: u16, foreign: bool) -> i32 {
    let Some(s) = state(world, id) else { return 0 };
    let has_target = s.target != 0 && world.entities.by_index(s.target as u32).is_some_and(|e| e.obj_type == crate::entity::ObjType::Building);
    let mut total = 0;
    for g in 0..N {
        let v = if foreign { foreign_stock(world, id, g as u8) } else { s.send[g] as i32 };
        if v <= 0 {
            continue;
        }
        if !has_target {
            total += v;
        } else if room_for(world, s.target, g as u8) != 0 {
            let bit = if foreign { good_flag::FOREIGN } else { good_flag::DOMESTIC };
            if s.flags[g] & bit != 0 {
                total += s.send[g] as i32;
            }
        }
    }
    total
}

/// 0x553b10: goods in the foreign collecting piles (role 3) of goods with the foreign flag.
fn foreign_stock(world: &World, id: u16, good: u8) -> i32 {
    let Some(s) = state(world, id) else { return 0 };
    trade_piles(world, id)
        .iter()
        .filter(|(_, d)| s.flags.get(d.good as usize).is_some_and(|f| f & good_flag::FOREIGN != 0) && d.trade_type == 3 && d.good == good)
        .map(|(_, d)| d.amount as i32)
        .sum()
}

/// 0x552340: the room for `good` (converted to the owner's race) in the trade piles: per pile of
/// the good `8 − expected − amount − coming`, plus 8 per free (role 0) or role 5 pile.
pub fn room_for(world: &World, id: u16, good: u8) -> i32 {
    if good as usize >= N {
        return 0;
    }
    let g = pile::convert_good_type(good, world.race_of(owner_of(world, id)));
    let mut room = 0;
    for (_, d) in trade_piles(world, id) {
        if d.good == g {
            room += 8 - d.expected - d.amount as i32 - d.coming as i32;
        }
        if d.trade_type == 0 || d.trade_type == 5 {
            room += 8;
        }
    }
    room
}

/// 0x554170: order a transport for the domestic (`foreign` false) trade: between two ports a
/// ship (0x554da0, also foreign); else with a marketplace on either side, up to 16 goods a donkey
/// (0x5558b0) or else a cart (0x5562b0), from 17 a cart or else — unless a cart transport is
/// recorded — a donkey.
fn order_transport(world: &mut World, id: u16, foreign: bool) -> bool {
    if goods_to_send(world, id, foreign) < 1 {
        return false;
    }
    let Some(s) = state(world, id) else { return false };
    if !foreign && s.send.iter().map(|&v| v as i32).sum::<i32>() < 1 {
        return false;
    }
    let target = if foreign { s.partner } else { s.target };
    if target == 0 || world.entities.by_index(target as u32).is_none() {
        return false;
    }
    let (here, there) = (kind_of_building(world, id), kind_of_building(world, target));
    if here == PORT && there == PORT {
        return order_ship(world, id, target, foreign);
    }
    if here != MARKETPLACE && there != MARKETPLACE {
        return false;
    }
    if foreign {
        return order_donkey(world, id, target, true);
    }
    let n = goods_to_send(world, id, false);
    if n == 0 {
        return false;
    }
    if n < 17 {
        return order_donkey(world, id, target, false) || order_cart(world, id, target, false);
    }
    if order_cart(world, id, target, false) {
        return true;
    }
    if count_transports(world, id, transport::CART, false) != 0 {
        return false;
    }
    order_donkey(world, id, target, false)
}

/// `SetTradeTarget(target)` (0x552a90, command 5005, both buildings trading roles): the old
/// target forgets this building (building vtbl+0x40 `Detach`); its own id clears the target
/// (and cancels the loading transports, 0x554950), else the target is set, attached
/// (vtbl+0x74). The transports under way learn the new target (0x553f30). The route drawing of
/// a selected building is the client's.
pub fn set_trade_target(world: &mut World, id: u16, target: u16) {
    let old = state(world, id).map_or(0, |s| s.target);
    if old != 0 {
        super::detach(world, old, id);
    }
    if target == id {
        if let Some(s) = state_mut(world, id) {
            s.target = 0;
        }
        cancel_loading(world, id);
        retarget_transports(world, id, 0);
    } else {
        if let Some(s) = state_mut(world, id) {
            s.target = target;
        }
        retarget_transports(world, id, target);
        super::attach(world, target, id);
    }
}

/// 0x553f30: the domestic transports coming here to load (ships / donkeys / carts in state 2)
/// and the loading one get the new target.
fn retarget_transports(world: &mut World, id: u16, target: u16) {
    use crate::jobs::donkey;
    let Some(s) = state(world, id) else { return };
    let (records, ship, cart, dl) = (s.transports.clone(), s.ship_load.on, s.cart_load.on, s.donkey_load);
    for t in records {
        match t.kind {
            transport::DONKEY => {
                if let Some(d) = donkey::data_mut(world, t.entity as u16)
                    && !d.foreign
                    && d.state == donkey::state::TO_SOURCE
                {
                    d.target = target;
                }
            }
            // A cart that is no vehicle any more is forgotten; one coming to load gets the target.
            transport::CART => {
                let c = t.entity as u16;
                if !is_vehicle(world, c) {
                    remove_transport(world, id, c, transport::CART);
                } else if let Some(cd) = cart::data_mut(world, c)
                    && cd.state == 2
                {
                    cd.target = target as i32;
                }
            }
            // A domestic ship coming to load gets the target.
            _ => {
                if let Some(t) = ship::data_mut(world, t.entity as u16)
                    && !t.foreign
                    && t.state == 2
                {
                    t.target = target;
                }
            }
        }
    }
    if ship {
        let sh = state(world, id).map_or(0, |s| s.ship_load.entity as u16);
        if let Some(t) = ship::data_mut(world, sh) {
            t.target = target;
        }
    } else if cart {
        let c = state(world, id).map_or(0, |s| s.cart_load.entity as u16);
        if is_vehicle(world, c)
            && let Some(cd) = cart::data_mut(world, c)
        {
            cd.target = target as i32;
        }
    } else if dl.on
        && let Some(d) = donkey::data_mut(world, dl.entity as u16)
    {
        d.target = target;
    }
}

/// 0x554950: the transports recorded here that are on their way to load (donkey state 2) are
/// called off: the donkey forgets the source (0x576d10) and, without a source, the target
/// (0x576bb0): it goes home free; a cart (state 2) likewise (0x55cc10 / 0x55caf0), a ship
/// (0x591830 / 0x591740).
fn cancel_loading(world: &mut World, id: u16) {
    use crate::jobs::donkey;
    let mut k = 0;
    while let Some(t) = state(world, id).and_then(|s| s.transports.get(k).copied()) {
        k += 1;
        match t.kind {
            transport::DONKEY => {
                let d = t.entity as u16;
                let Some(dd) = donkey::data(world, d).copied() else { continue };
                if dd.state == donkey::state::TO_SOURCE {
                    donkey::source_gone(world, d);
                    donkey::target_gone(world, d);
                    remove_transport(world, id, d, transport::DONKEY);
                    k -= 1;
                }
                // A foreign donkey: forgotten, state 1, back the way it came (0x5770d0). (Both
                // cases at once step the index back twice in the original; here at most to 0.)
                if dd.foreign {
                    remove_transport(world, id, d, transport::DONKEY);
                    if let Some(m) = donkey::data_mut(world, d) {
                        m.state = donkey::state::TO_TARGET;
                    }
                    donkey::foreign_return(world, d);
                    k = k.saturating_sub(1);
                }
            }
            // A cart coming to load: called off at both ends (0x55cc10, 0x55caf0), forgotten.
            transport::CART => {
                let c = t.entity as u16;
                if is_vehicle(world, c) && cart::data(world, c).is_some_and(|cd| cd.state == 2) {
                    cart::source_gone(world, c);
                    cart::target_gone(world, c);
                    remove_transport(world, id, c, transport::CART);
                    k -= 1;
                }
            }
            // A ship coming to load: called off at both ends (0x591830, 0x591740), forgotten; a
            // foreign one once more (the index steps back twice, as the donkey's).
            _ => {
                let sh = t.entity as u16;
                if world.entities.by_index(sh as u32).is_none_or(|e| e.obj_type != crate::entity::ObjType::Ship) {
                    continue;
                }
                let Some(sd) = ship::data(world, sh) else { continue };
                let foreign = sd.foreign;
                if sd.state == 2 {
                    ship::source_gone(world, sh);
                    ship::target_gone(world, sh);
                    remove_transport(world, id, sh, transport::SHIP);
                    k = k.saturating_sub(1);
                }
                if foreign {
                    remove_transport(world, id, sh, transport::SHIP);
                    k = k.saturating_sub(1);
                }
            }
        }
    }
}

/// 0x554b10: the transports recorded here that come to unload (donkey state 1) take their
/// goods back (0x576bb0), carts and ships (state 1) turn back (0x55caf0 / 0x591740).
fn cancel_unloading(world: &mut World, id: u16) {
    use crate::jobs::donkey;
    let mut k = 0;
    while let Some(t) = state(world, id).and_then(|s| s.transports.get(k).copied()) {
        k += 1;
        match t.kind {
            transport::DONKEY => {
                let d = t.entity as u16;
                if donkey::data(world, d).is_some_and(|dd| dd.state == donkey::state::TO_TARGET) {
                    donkey::target_gone(world, d);
                    remove_transport(world, id, d, transport::DONKEY);
                    k -= 1;
                }
            }
            // A cart that is no vehicle any more is forgotten (the index is not stepped back); one
            // coming to unload (state 1) turns back (0x55caf0).
            transport::CART => {
                let c = t.entity as u16;
                if !is_vehicle(world, c) {
                    remove_transport(world, id, c, transport::CART);
                } else if cart::data(world, c).is_some_and(|cd| cd.state == 1) {
                    cart::target_gone(world, c);
                    remove_transport(world, id, c, transport::CART);
                    k -= 1;
                }
            }
            // A ship coming to unload (state 1) turns back (0x591740).
            _ => {
                let sh = t.entity as u16;
                if ship::data(world, sh).is_some_and(|sd| sd.state == 1) {
                    ship::target_gone(world, sh);
                    remove_transport(world, id, sh, transport::SHIP);
                    k -= 1;
                }
            }
        }
    }
}

/// 0x554ca0 (crush): the loading transport leaves when the target still lives (0x5554d0 /
/// 0x5567a0 / 0x555e40), else a ship or a cart goes home (0x591b40 / 0x55b8a0), a donkey drops
/// its load and waits (0x5771d0).
fn release_loading(world: &mut World, id: u16) {
    let Some(s) = state(world, id) else { return };
    let target_lives = s.target != 0 && world.entities.by_index(s.target as u32).is_some_and(|e| e.flags & flag::ALIVE_MASK != 0);
    if s.ship_load.on {
        if target_lives {
            ship_leave(world, id);
        } else {
            ship::go_home(world, s.ship_load.entity as u16);
        }
        return;
    }
    if s.cart_load.on {
        let c = s.cart_load.entity as u16;
        if target_lives {
            cart_leave(world, id);
        } else {
            cart::go_home(world, c);
        }
        return;
    }
    if !s.donkey_load.on {
        return;
    }
    if target_lives {
        donkey_leave(world, id);
    } else {
        crate::jobs::donkey::drop_and_wait(world, s.donkey_load.entity as u16);
    }
}

/// `TransportGood(good, amount)` (0x552f60, command 5006): 0 stops the good (flag bit 1 off,
/// both amounts 0; a working building drops the expectations of its piles of the good, 0x552ce0);
/// 100 makes it endless; a positive amount adds (capped at 100), a negative one takes off (below
/// 0: stopped as with 0).
pub fn transport_good(world: &mut World, id: u16, good: u16, amount: i32) {
    // Goods 0..=42 (the original writes any index; the UI sends 1..=42).
    let gi = good as usize;
    if gi >= N {
        return;
    }
    let working = building_flags(world, id) & WORKING != 0;
    let Some(s) = state_mut(world, id) else { return };
    let stop = |s: &mut State| {
        s.flags[gi] &= !good_flag::DOMESTIC;
        s.collect[gi] = 0;
        s.send[gi] = 0;
    };
    if amount == 0 {
        stop(s);
        if !working {
            drop_expectations(world, id, good as u8);
        }
        return;
    }
    if amount == 100 {
        s.flags[gi] |= good_flag::DOMESTIC;
        s.collect[gi] = ENDLESS;
        s.send[gi] = ENDLESS;
        return;
    }
    if amount > 0 {
        s.flags[gi] |= good_flag::DOMESTIC;
        let c = s.collect[gi] as i32;
        if c + amount > 100 {
            s.collect[gi] = ENDLESS;
            s.send[gi] = ENDLESS;
        } else {
            s.send[gi] = (s.send[gi] as i32 + amount) as i8;
            s.collect[gi] = (c + amount) as i8;
        }
        return;
    }
    let c = s.collect[gi] as i32;
    let drop = if c + amount < 0 {
        stop(s);
        !working
    } else {
        s.send[gi] = (s.send[gi] as i32 + amount) as i8;
        s.collect[gi] = (c + amount) as i8;
        !working && s.send[gi] == 0
    };
    if drop {
        drop_expectations(world, id, good as u8);
    }
}

/// 0x552ce0: piles still expecting goods drop the expectation when the good is no longer sent.
/// (The original tests `+0x1ab + good` of the role but reads every pile's expectation, whatever
/// its good.)
fn drop_expectations(world: &mut World, id: u16, good: u8) {
    let send = state(world, id).map_or(1, |s| s.send[good as usize]);
    for p in piles(world, id) {
        if pile::get(world, p).is_some_and(|d| d.expected != 0) && send == 0 {
            pile::set_expected(world, p, 0);
        }
    }
}

/// `GoodArrive(good)` (vtbl+0x44, 0x5522f0; a collecting pile got a good): one less to collect
/// for the domestic trade (unless endless or already 0), and for the foreign one.
pub fn good_arrive(world: &mut World, id: u16, good: u8) {
    let Some(s) = state_mut(world, id) else { return };
    let g = good as usize;
    if g >= N {
        return;
    }
    if s.flags[g] & good_flag::DOMESTIC != 0 && s.collect[g] > 0 && s.collect[g] != ENDLESS {
        s.collect[g] -= 1;
    }
    if s.flags[g] & good_flag::FOREIGN != 0 {
        s.foreign_collect[g] = s.foreign_collect[g].wrapping_sub(1);
    }
}

/// `GetPileIdWithGood(good)` (vtbl+0x38, 0x5521f0): the first trade pile of the good (converted
/// to the owner's race) that still expects goods.
pub fn pile_with_good(world: &World, id: u16, good: u8) -> u16 {
    if good as usize >= N {
        return 0;
    }
    let race = world.entities.by_index(id as u32).map_or(0, |e| e.race);
    let g = pile::convert_good_type(good, race);
    trade_piles(world, id).into_iter().find(|(_, d)| d.good == g && d.expected != 0).map_or(0, |(p, _)| p)
}

/// `SettlerEnter(settler)` (vtbl+0x4c, 0x552430): the settler is the inhabitant, inside.
pub fn settler_enter(world: &mut World, id: u16, settler: u16) -> bool {
    if let Some(r) = super::role_mut(world, id) {
        r.base.inhabitant = settler;
        r.base.worker_in_house = true;
    }
    true
}

/// `ConvertEventIntoGoal` (vtbl+0x5c, 0x557160): event 9 (`TARGET_DIE`) from the target clears
/// it, calls off the loading transports and drops the foreign partner (0x557890); then the next
/// update in 14 ticks.
pub fn convert_event_into_goal(world: &mut World, id: u16, ev: &crate::jobs::state::EntityEvent) {
    if ev.event == 9 && state(world, id).is_some_and(|s| s.target != 0 && ev.data[0] as u32 == s.target as u32) {
        if let Some(s) = state_mut(world, id) {
            s.target = 0;
        }
        cancel_loading(world, id);
        drop_partner(world, id);
    }
    world.register_for_logic_update(id, 14);
}

/// 0x557890: drop the foreign partner: the loading transport is released (0x554ca0), the
/// transports are called off (0x554950 / 0x554b10), no partner, the eco-sector's requests of the
/// building go (0x4d5b70); the foreign piles (roles 3 / 4 / 5) end: role 3 drops its observers
/// (0x56fac0), all become collecting (1) when they have goods, else free (0), and their good loses
/// the foreign bits 2 / 3 and its foreign amounts; no ships under way.
fn drop_partner(world: &mut World, id: u16) {
    if state(world, id).is_none_or(|s| s.partner == 0) {
        return;
    }
    release_loading(world, id);
    cancel_loading(world, id);
    cancel_unloading(world, id);
    if let Some(s) = state_mut(world, id) {
        s.partner = 0;
    }
    let e = super::storage::eco_of_building(world, id);
    eco::clean_up_building_need(world, e, id);
    for p in piles(world, id) {
        let Some(d) = pile::get(world, p) else { continue };
        let (t, amount, good) = (d.trade_type, d.amount, d.good as usize);
        if t == 3 {
            pile::notify_target_die_and_detach_all(world, p);
        }
        if matches!(t, 3..=5) {
            pile::set_trade_type(world, p, (amount > 0) as u8);
            if let Some(s) = state_mut(world, id)
                && good < N
            {
                s.flags[good] &= 0xf3;
                s.foreign_collect[good] = 0;
                s.foreign_send[good] = 0;
                s.ships_out = 0;
            }
        }
    }
}

/// `Switch` (vtbl+0x20, 0x552120, command 5003): the base switch (0x53ffd0); a building now off
/// calls off its transports (0x554950 / 0x554b10); the eco-sector drops the building's open
/// requests (0x4d5b70).
pub fn switch(world: &mut World, id: u16) {
    super::work::switch(world, id);
    if building_flags(world, id) & WORKING == 0 {
        cancel_loading(world, id);
        cancel_unloading(world, id);
    }
    let e = super::storage::eco_of_building(world, id);
    eco::clean_up_building_need(world, e, id);
}

/// vtbl+0x2c (0x5570b0, from `CBuilding::CrushBuilding`): the foreign partner (0x557890), the
/// transports (0x554950 / 0x554b10 / 0x554ca0), the international trader (0x4f0530), the
/// partner's link and the target (0x557520), the eco-sector's requests (0x4d5b70).
pub fn crush(world: &mut World, id: u16) {
    drop_partner(world, id);
    cancel_loading(world, id);
    cancel_unloading(world, id);
    release_loading(world, id);
    super::trader::remove_building(world, id);
    // 0x557520: a partner that names this building forgets it; no target.
    let partner = state(world, id).map_or(0, |s| s.partner);
    if partner != 0
        && let Some(p) = state_mut(world, partner)
        && p.partner == id
    {
        p.partner = 0;
    }
    if let Some(s) = state_mut(world, id) {
        s.target = 0;
    }
    let e = super::storage::eco_of_building(world, id);
    eco::clean_up_building_need(world, e, id);
}

/// Whether two buildings may trade (both trading roles, the target alive) — the command check
/// of 5005 (0x4f6240 case 0x138d).
pub fn can_target(world: &World, id: u16, target: u16) -> bool {
    is_trading(world, id) && is_trading(world, target) && world.entities.by_index(target as u32).is_some_and(|e| e.flags & flag::DIED == 0)
}
