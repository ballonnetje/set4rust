//! `CCart` (vtable 0x1059710, 0x55a940..0x55e100, decompile `tools/re/vehicles/cart.c`; load
//! ctor 0x55afd0, store 0x55b370). A cart is made in the vehicle hall and pulled by a donkey. It
//! carries goods between two trading buildings (six slots of 8, +0xc8) or, morphed into a
//! *foundation cart* (+0xbc, command 5012), pioneers, carriers and building goods to found a
//! settlement (`FoundationCart` config, static vars 0x424110..0x424430).
//!
//! * **Hitching** (vtbl+0xa0 0x55dc50, instead of `CVehicle` 0x594830): a cart whose build
//!   progress is complete calls the nearest free donkey of its owner on its sector (0x4fa050)
//!   with `CDonkeyRole` 0x576f00 (list 237 `JOB_DONKEY_COME_TO_VEHICLEHALL`, walk to a node
//!   behind the cart 0x55af20, +0x3c the cart, state 5) and tells the local player (0x776);
//!   without one it says so (0x9e0) and asks again in 16 ticks. The donkey's arrival (0x576d80)
//!   hitches it (0x55dd50): the donkey leaves the map and is deleted, the cart is finished
//!   (0x594830).
//! * **Update** (vtbl+0x90 0x55bca0): +0xbd "on own land" again, the flags (0x55de00), a
//!   standing trade cart idles (`IdleWalk`); a moving one goes on (0x595510) and, when its walk
//!   just ended, tells the trading building it drove to (0x55cc80); a foundation cart that stands
//!   or waits (0x1b) calls its settlers (0x55ce00) and goods (0x55d660) every 16 ticks.
//! * **Next task** (vtbl+0xbc 0x55b6d0): a GO_TO_POS walks unless the cart is a foundation cart
//!   that is not yet full (pioneers and goods).
//! * **Events** (vtbl+0x6c 0x55c170): its target / source building gone (events 7 / 9: 0x55caf0 /
//!   0x55cc10), a move order keeps where it stood (+0x114), then `CWheeler` 0x59a200.
//! * **Delete** (vtbl+0x10 0x55dbc0): the source's transport record, the load drops (0x55b7d0)
//!   or, on board a ferry, the passengers die (0x55dfc0).
//! * **Foundation cart** (+0xbc): command 5012 ([`create_foundation_cart`] 0x55c360) needs its
//!   owner's land and 4 boards, 8 stones, 2 hammers offered in the eco-sector; the cart waits
//!   (task 0x1b) and every 16 ticks calls pioneers (0x583130: the nearest free one of its owner
//!   on its sector) and carriers (the eco-sector's nearest offered one), event 0x18, which walk
//!   behind it ([`exit_place`]), attach (vtbl+0xa4) and check in ([`settler_entered`] 0x55d0c0),
//!   and orders the goods (0x55d660 → 0x4d5a30: a carrier per good, event 11, list 153, LOAD_GOOD
//!   → [`good_arrived`]). Full (8 pioneers, 14 goods) it can be driven and is SELECTABLE; 5011
//!   ([`create_settlement`] 0x55c130) claims the land around it, lets the settlers out, puts the
//!   goods down and leaves a pile of 5 boards; 5013 ([`create_normal_cart`] 0x55c020) lets them
//!   out without settling.
//! * **Trade** (with `buildings::trading`): a trading building orders the cart on its ensign or
//!   the nearest free one (0x5562b0: +0x11c itself, +0x118 the target, state 2, a drive to its
//!   ensign); there the building loads it (0x556c70 → 0x556520, state 4; 0x5565e0 → [`load`]
//!   0x55c690, one pile per update); when nothing more comes it sends it to the target (0x5567a0
//!   → state 1, [`drive_to_target`] 0x55ccf0) or, without a path, home; the target unloads it
//!   (0x556580 state 3, 0x556980 → [`unload`]) and sends it home ([`go_home`] 0x55b8a0: back to
//!   +0x114, the rest of the load dropped when that is in the same eco-sector). A loaded trade
//!   cart is VULNERABLE unless foreign (+0x125).
//!
//! Not simulation: the draw (0x55ba20 / 0x55bbd0: GFX file race + 0x1e, the loaded variant when
//! 0x55ca10, the build frame 0x2e) and the dialog refresh (vtbl+0x94 0x55bdb0: message 0x25a /
//! 0x25c with the slots or the foundation lists, see `docs/VEHICLES.md`).

use super::{get, get_mut, kind};
use crate::World;
use crate::entity::{ObjType, flag};
use crate::grid::Y16X16;
use crate::hash::StateHasher;
use crate::jobs::state::EntityEvent;

/// `FoundationCart` config (static `CStaticConfigVarInt`s, release 0x424110..0x424430):
/// AmountOfPioneers 0x11cdadc, AmountOfCarrier 0x11cdae8, AmountOfBoards 0x11cdaf4,
/// AmountOfStones 0x11cdb00, AmountOfHammer 0x11cdb0c, AmountOfRecycledBoard 0x11cdb18.
pub mod config {
    pub const PIONEERS: u8 = 8;
    pub const CARRIERS: u8 = 2;
    pub const BOARDS: i32 = 4;
    pub const STONES: i32 = 8;
    pub const HAMMERS: i32 = 2;
    pub const RECYCLED_BOARDS: u8 = 5;
}

/// Goods (`GOOD_*`) and settler types the cart uses.
pub mod good {
    pub const BOARD: i32 = 7;
    pub const HAMMER: i32 = 0x12;
    pub const STONE: i32 = 0x20;
}
pub const SETTLER_CARRIER: i32 = 1;
pub const SETTLER_PIONEER: i32 = 0x2f;
/// Goods per slot (0x55c690).
pub const SLOT_SIZE: i32 = 8;
/// `JOB_DONKEY_COME_TO_VEHICLEHALL`.
pub const LIST_DONKEY_COME: u16 = 237;
/// The foundation cart's waiting task (0x55c360, 0x55e020).
pub const TASK_WAIT: u8 = 0x1b;

/// One settler of a foundation cart (+0xa0, 16 bytes): +0 called, +4 inside, +8 id, +0xc type.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CartSettler {
    pub called: i32,
    pub inside: bool,
    pub id: i32,
    pub kind: i32,
}

/// One good of a foundation cart (+0xac, 20 bytes): +0 needed, +4 good, +8 arrived, +0xc ordered,
/// +0x10 the tick of the last order (not stored).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CartGood {
    pub needed: i32,
    pub good: i32,
    pub arrived: i32,
    pub ordered: i32,
    pub tick: i32,
}

/// The `CCart` members (+0xa0..+0x125).
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CartData {
    /// +0xa0 the foundation cart's settlers, +0xac its goods.
    pub settlers: Vec<CartSettler>,
    pub goods: Vec<CartGood>,
    /// +0xb8 / +0xb9 pioneers / carriers inside, +0xba / +0xbb still to call.
    pub pioneers: u8,
    pub carriers: u8,
    pub pioneers_due: u8,
    pub carriers_due: u8,
    /// +0xbc a foundation cart, +0xbd it stands on its owner's land (eco-sector).
    pub foundation: bool,
    pub own_land: bool,
    /// +0xbe goods arrived, +0xbf goods needed.
    pub goods_in: u8,
    pub goods_needed: u8,
    /// +0xc4 (cleared by the ctor, stored).
    pub xc4: i32,
    /// +0xc8 six slots `{good, amount, +8}` (12 bytes each; +8 is never written — the saves hold
    /// uninitialised values there).
    pub slots: [[i32; 3]; 6],
    /// +0x110 driving to the source, +0x111, +0x112 driving to the target.
    pub to_source: bool,
    pub x111: bool,
    pub to_target: bool,
    /// +0x114 where it stood when a trade order or a move order came (−1 none).
    pub home: i32,
    /// +0x118 / +0x11c the trading buildings it carries to / from (0x5562b0), +0x120 the trade
    /// state (2 to the source, 4 loading, 1 to the target, 3 unloading, 0 none).
    pub target: i32,
    pub source: i32,
    pub state: i32,
    /// +0x124 the task at the end of the last update.
    pub last_task: u8,
    /// +0x125 a foreign trade transport.
    pub foreign: bool,
}

impl Default for CartData {
    fn default() -> Self {
        CartData {
            settlers: Vec::new(),
            goods: Vec::new(),
            pioneers: 0,
            carriers: 0,
            pioneers_due: 0,
            carriers_due: 0,
            foundation: false,
            own_land: false,
            goods_in: 0,
            goods_needed: 0,
            xc4: 0,
            slots: [[0; 3]; 6],
            to_source: false,
            x111: false,
            to_target: false,
            home: -1,
            target: 0,
            source: 0,
            state: 0,
            last_task: 0,
            foreign: false,
        }
    }
}

impl CartData {
    pub(crate) fn hash(&self, h: &mut StateHasher) {
        h.u32(self.settlers.len() as u32);
        for s in &self.settlers {
            for v in [s.called, s.inside as i32, s.id, s.kind] {
                h.u32(v as u32);
            }
        }
        h.u32(self.goods.len() as u32);
        for g in &self.goods {
            for v in [g.needed, g.good, g.arrived, g.ordered, g.tick] {
                h.u32(v as u32);
            }
        }
        for v in [self.pioneers, self.carriers, self.pioneers_due, self.carriers_due, self.foundation as u8, self.own_land as u8, self.goods_in, self.goods_needed] {
            h.u8(v);
        }
        for s in &self.slots {
            s.iter().for_each(|&v| h.u32(v as u32));
        }
        for v in [self.to_source, self.x111, self.to_target, self.foreign] {
            h.bool(v);
        }
        for v in [self.xc4, self.home, self.target, self.source, self.state] {
            h.u32(v as u32);
        }
        h.u8(self.last_task);
    }

    /// The goods of `good` in the slots (the sum 0x55b7d0 / 0x5567a0 / 0x556980 compute).
    pub fn amount_of(&self, good: i32) -> i32 {
        self.slots.iter().filter(|s| s[0] == good).map(|s| s[1]).sum()
    }

    /// 0x55ca10: something is on board: a slot amount, an arrived good, a settler inside.
    pub fn has_cargo(&self) -> bool {
        self.slots.iter().any(|s| s[1] != 0) || self.goods.iter().any(|g| g.arrived > 0) || self.settlers.iter().any(|s| s.inside)
    }

    /// The foundation cart has all its pioneers and goods (0x55b6d0 / 0x55de00 / 0x55bdb0).
    pub fn is_full(&self) -> bool {
        self.pioneers >= config::PIONEERS && self.goods_needed <= self.goods_in
    }
}

pub fn data(world: &World, id: u16) -> Option<&CartData> {
    get(world, id)?.cart.as_deref()
}

pub fn data_mut(world: &mut World, id: u16) -> Option<&mut CartData> {
    get_mut(world, id)?.cart.as_deref_mut()
}

fn is_cart(world: &World, id: u16) -> bool {
    world.entities.by_index(id as u32).is_some_and(|e| e.obj_type == ObjType::LandVehicle && e.kind == kind::CART)
}

fn is_building(world: &World, id: i32) -> bool {
    id > 0 && world.entities.by_index(id as u32).is_some_and(|e| e.obj_type == ObjType::Building)
}

fn is_vehicle(world: &World, id: i32) -> bool {
    id > 0 && world.entities.by_index(id as u32).is_some_and(|e| matches!(e.obj_type, ObjType::Ship | ObjType::LandVehicle))
}

fn pos_of(world: &World, id: u16) -> Y16X16 {
    world.entities.by_index(id as u32).map_or(Y16X16(0), |e| e.pos)
}

/// `IAnimatedEntity::SetEvent` (vtbl+0x50) on any entity.
pub(super) fn send(world: &mut World, id: u16, ev: EntityEvent) {
    match world.entities.by_index(id as u32).map(|e| e.obj_type) {
        Some(ObjType::Settler) => crate::jobs::core::set_event(world, id, ev),
        Some(ObjType::Building) => crate::buildings::set_event(world, id, ev),
        Some(ObjType::Pile) => crate::pile::set_event(world, id, ev),
        Some(ObjType::Ship | ObjType::LandVehicle) => super::logic::set_event(world, id, ev),
        _ => {}
    }
}

/// The owner of the eco-sector at `pos` (0 on unowned land), and whether there is one.
fn eco_at(world: &World, pos: Y16X16) -> u16 {
    world.grid.checked_index(pos.x(), pos.y()).map_or(0, |n| crate::eco::eco_of(world, n))
}

/// +0xbd (ctor 0x55aae0, 0x55bca0): the cart stands in an eco-sector of its owner.
fn on_own_land(world: &World, id: u16) -> bool {
    let Some(e) = world.entities.by_index(id as u32) else { return false };
    let eco = eco_at(world, e.pos);
    eco != 0 && world.economy.owner(eco) == e.owner
}

/// `CCart` ctor (0x55aae0) after the `CWheeler` one: the members, a finished cart READY and
/// VISIBLE, +0xbd, VULNERABLE off, +0x124 the task, the flags (0x55de00); the trade variant
/// (`AddVehicle` type 0x406) becomes a filled foundation cart (0x55e020).
pub fn init(world: &mut World, id: u16, under_construction: bool, filled: bool) {
    let own = on_own_land(world, id);
    let task = get(world, id).map_or(0, |v| v.task);
    if let Some(e) = world.entities.by_index_mut(id as u32) {
        if !under_construction {
            e.flags |= 0x0200_0100;
        }
        e.flags &= !flag::VULNERABLE_MASK;
    }
    if let Some(v) = get_mut(world, id) {
        v.cart = Some(Box::new(CartData { own_land: own, last_task: task, ..CartData::default() }));
    }
    update_flags(world, id);
    if filled {
        fill(world, id);
    }
}

/// vtbl+0xac (0x55de00) with vtbl+0xb0 (0x55deb0): a finished cart loses SELECTABLE (or 0x4000
/// when 0x400 is set) and 0x400; a full foundation cart that is not busy with a trade and not on
/// a ferry (+0x98) is SELECTABLE; otherwise it gets 0x4000, and 0x4400 when 0x400 was set.
pub fn update_flags(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let old = e.flags;
    if old & flag::BIRTH != 0 {
        return;
    }
    let (full, busy, on_ferry) = match get(world, id) {
        Some(v) => {
            let c = v.cart.as_deref().cloned().unwrap_or_default();
            (c.foundation && c.is_full(), c.to_source || c.x111 || c.to_target, v.target_entity != 0)
        }
        None => return,
    };
    let Some(e) = world.entities.by_index_mut(id as u32) else { return };
    e.flags &= if e.flags & 0x400 != 0 { !0x4400 } else { !0x2400 };
    if full && !busy && !on_ferry {
        e.flags |= flag::SELECTABLE;
        return;
    }
    let f = e.flags;
    e.flags = f | 0x4000;
    if (old >> 10) & 1 != 0 {
        e.flags = f | 0x4400;
    }
}

/// vtbl+0x8c / vtbl+0xe4 (0x55ae60 / 0x55af20): the first node behind the cart (from the
/// opposite of its direction, clockwise) of the ring-2 nodes `8 + 2·k` / the entries `0x11 + 2·k`
/// of the spiral table that holds no map object or a real entity (the footprint placeholders
/// 1..=6 do not count); 0 when none. 0x55ae60 answers only for a finished cart.
fn place_behind(world: &World, id: u16, base: usize) -> i32 {
    let Some(e) = world.entities.by_index(id as u32) else { return 0 };
    let dir = get(world, id).map_or(0, |v| v.dir as i32);
    for d in dir..dir + 6 {
        let i = (((d.rem_euclid(6)) + 3) % 6) as usize * 2 + base;
        let (x, y) = (e.pos.x() + crate::spiral::dx(i), e.pos.y() + crate::spiral::dy(i));
        let Some(n) = world.grid.checked_index(x, y) else { continue };
        let m = world.layers.map_object[n];
        if m == 0 || world.entities.by_index(m as u32).is_some_and(|o| o.obj_type != ObjType::None) {
            return Y16X16::pack(x, y).0 as i32;
        }
    }
    0
}

/// vtbl+0x8c (0x55ae60).
pub fn exit_place(world: &World, id: u16) -> i32 {
    let ready = world.entities.by_index(id as u32).is_some_and(|e| e.flags & flag::READY != 0);
    if ready { place_behind(world, id, 8) } else { 0 }
}

/// vtbl+0xa0 (0x55dc50): the cart is built; a donkey must come and pull it.
pub fn finish(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let (owner, pos) = (e.owner, e.pos);
    let donkey = crate::jobs::donkey::nearest_free(world, owner, pos.x(), pos.y());
    if donkey != 0 {
        let place = place_behind(world, id, 0x11);
        crate::jobs::donkey::come_to_vehicle(world, donkey, Y16X16(place as u32), id);
        world.events.push(crate::event::SimEvent::Message { player: owner, text: 0x776, x: pos.x(), y: pos.y(), filtered: false });
        return;
    }
    world.events.push(crate::event::SimEvent::warning(owner, 0x9e0, pos));
    world.register_for_logic_update(id, 16);
}

/// 0x55dd50 `(donkey)` (from the donkey's arrival 0x576d80): the donkey leaves the map (war map,
/// map object, ON_BOARD, invisible, unregistered) and is deleted; the cart loses VULNERABLE and
/// is finished (0x594830).
pub fn hitch(world: &mut World, cart: u16, donkey: u16) {
    if world.warmap.contains(donkey) {
        crate::warmap::remove(world, donkey);
    }
    let pos = pos_of(world, donkey);
    if let Some(n) = world.grid.checked_index(pos.x(), pos.y()) {
        world.layers.map_object[n] = 0;
    }
    if let Some(e) = world.entities.by_index_mut(donkey as u32) {
        e.flags = (e.flags | flag::ON_BOARD) & !flag::VISIBLE;
    }
    world.unregister_from_logic_update(donkey);
    world.kill(donkey, 0);
    if let Some(e) = world.entities.by_index_mut(cart as u32) {
        e.flags &= !flag::VULNERABLE_MASK;
    }
    super::finish(world, cart);
}

/// vtbl+0x90 (0x55bca0).
pub fn continue_task(world: &mut World, id: u16) {
    let own = on_own_land(world, id);
    if let Some(c) = data_mut(world, id) {
        c.own_land = own;
    }
    update_flags(world, id);
    let Some(v) = get(world, id) else { return };
    let (task, foundation, last) = (v.task, v.cart.as_ref().is_some_and(|c| c.foundation), v.cart.as_ref().map_or(0, |c| c.last_task));
    match task {
        super::logic::task::IDLE if !foundation => {
            super::logic::continue_task(world, id);
            return;
        }
        super::logic::task::IDLE => {}
        TASK_WAIT if !foundation => return,
        TASK_WAIT => {}
        _ => {
            let ready = world.entities.by_index(id as u32).is_some_and(|e| e.flags & flag::READY != 0);
            if ready {
                super::logic::continue_task(world, id);
            }
            let now = get(world, id).map_or(task, |v| v.task);
            if now != last && last == super::logic::task::GO {
                arrived_at_building(world, id);
            }
            if let Some(c) = data_mut(world, id) {
                c.last_task = now;
            }
            return;
        }
    }
    call_settlers(world, id);
    order_goods(world, id);
    world.register_for_logic_update(id, 16);
}

/// 0x55cc80: the walk ended. Driving to the source, a source that is still a building learns
/// that the cart is there (`CTradingBuildingRole` 0x556c70: it loads); driving to the target
/// likewise (it unloads), which ends the drive.
pub fn arrived_at_building(world: &mut World, id: u16) {
    let Some(c) = data(world, id).cloned() else { return };
    if c.to_source && is_building(world, c.source) {
        crate::buildings::trading::cart_arrived(world, c.source as u16, id);
    }
    let Some(c) = data(world, id).cloned() else { return };
    if c.to_target && is_building(world, c.target) {
        crate::buildings::trading::cart_arrived(world, c.target as u16, id);
        if let Some(c) = data_mut(world, id) {
            c.to_target = false;
        }
    }
}

/// Whether the cart may walk a GO_TO_POS (0x55b6d0): not a foundation cart, or a full one.
pub fn may_move(world: &World, id: u16) -> bool {
    data(world, id).is_none_or(|c| !c.foundation || c.is_full())
}

/// vtbl+0x6c (0x55c170) before `CWheeler` 0x59a200: its source / target gone (events 9 / 7 with
/// the building in data 0), a move order (0x11 / 0xd) keeps where the cart stood.
pub fn convert_event(world: &mut World, id: u16, ev: &EntityEvent) {
    if ev.event == 9 || ev.event == 7 {
        if data(world, id).is_some_and(|c| ev.data[0] == c.target) {
            target_gone(world, id);
        }
        if data(world, id).is_some_and(|c| ev.data[0] == c.source) {
            source_gone(world, id);
        }
    }
    if ev.event == 0x11 && ev.kind == 0xd {
        let pos = pos_of(world, id);
        if let Some(c) = data_mut(world, id) {
            c.home = pos.0 as i32;
        }
    }
}

/// vtbl+0x74 (0x55df70): free for a trade order: standing (task 0x11 or 0), READY, not a
/// foundation cart.
pub fn is_free(world: &World, id: u16) -> bool {
    let ready = world.entities.by_index(id as u32).is_some_and(|e| e.flags & flag::READY != 0);
    let Some(v) = get(world, id) else { return false };
    ready && matches!(v.task, super::logic::task::IDLE | 0) && v.cart.as_ref().is_none_or(|c| !c.foundation)
}

/// 0x4fa050 for vehicles (list `[player · 6 + 5]`, head first): the nearest cart free for a
/// trade order (vtbl+0x74) on the sector of (x, y), by hex distance below 0xffff; ties keep the
/// first.
pub fn nearest_free(world: &World, player: u8, x: i32, y: i32) -> u16 {
    let Some(n) = world.grid.checked_index(x, y) else { return 0 };
    let sector = world.tiling.sector_of(n);
    let (mut best, mut best_d) = (0u16, 0xffff);
    for id in super::iter(world, player, kind::CART) {
        let Some(e) = world.entities.by_index(id as u32) else { continue };
        let Some(en) = world.grid.checked_index(e.x(), e.y()) else { continue };
        if world.tiling.sector_of(en) != sector || !is_free(world, id) {
            continue;
        }
        let d = crate::spiral::grid_distance(x - e.x(), y - e.y());
        if d < best_d {
            best_d = d;
            best = id;
        }
    }
    best
}

/// 0x55c690 `(good, n)`: load up to `n` of `good`: first into the slots of the good, then into
/// empty slots (8 each); afterwards the cart is VULNERABLE when it has cargo and is not foreign.
/// Returns what did not fit.
pub fn load(world: &mut World, id: u16, good: i32, n: i32) -> i32 {
    let mut n = n;
    if let Some(c) = data_mut(world, id) {
        for s in c.slots.iter_mut().filter(|s| s[0] == good) {
            let k = n.min(SLOT_SIZE - s[1]);
            n -= k;
            s[1] += k;
        }
        for s in c.slots.iter_mut() {
            if s[0] == 0 && n != 0 {
                s[0] = good;
                let k = n.min(SLOT_SIZE);
                s[1] += k;
                n -= k;
            }
        }
    }
    let (cargo, foreign) = data(world, id).map_or((false, false), |c| (c.has_cargo(), c.foreign));
    if let Some(e) = world.entities.by_index_mut(id as u32) {
        if cargo && !foreign {
            e.flags |= flag::VULNERABLE_MASK;
        } else {
            e.flags &= !flag::VULNERABLE_MASK;
        }
    }
    n
}

/// A move order to `pos` (event 0x11 / 0xd, vtbl+0x50).
fn move_to(world: &mut World, id: u16, pos: i32) {
    super::logic::set_event(world, id, EntityEvent { event: 0x11, kind: 0xd, data: [0, pos, 0] });
}

/// 0x55ccf0: with a target, drive to its ensign (+0x110 off, +0x112 on).
pub fn drive_to_target(world: &mut World, id: u16) {
    let Some(c) = data(world, id) else { return };
    let target = c.target;
    if target == 0 {
        return;
    }
    let ensign = match world.entities.by_index(target as u32).map(|e| &e.data) {
        Some(crate::entity::EntityData::Building(d)) => d.ensign.0 as i32,
        _ => 0,
    };
    if let Some(c) = data_mut(world, id) {
        c.to_source = false;
        c.to_target = true;
    }
    move_to(world, id, ensign);
}

/// 0x55b8a0: back to +0x114 (where it stood when ordered; else where it is). In the same
/// eco-sector: the cargo drops (0x55b7d0), the drive there, and it forgets the trade (+0x110,
/// +0x112, +0x118, +0x11c); else only the drive, and +0x114 is cleared (−1).
pub fn go_home(world: &mut World, id: u16) {
    let Some(c) = data(world, id) else { return };
    let pos = pos_of(world, id);
    let home = if c.home < 1 { pos } else { Y16X16(c.home as u32) };
    if eco_at(world, pos) == eco_at(world, home) {
        if data(world, id).is_some_and(CartData::has_cargo) {
            drop_all(world, id);
        }
        move_to(world, id, home.0 as i32);
        if let Some(c) = data_mut(world, id) {
            c.to_source = false;
            c.to_target = false;
            c.target = 0;
            c.source = 0;
        }
        return;
    }
    move_to(world, id, home.0 as i32);
    if let Some(c) = data_mut(world, id) {
        c.home = -1;
    }
}

/// 0x55caf0: the target (+0x118) is gone or the transport is called off. The target forgets
/// it; with a source the cart turns back to it (now its target, recorded there unless already,
/// [`drive_to_target`]); with a source that is no building it drops its cargo; then (no source)
/// it is free (state 0) and goes home.
pub fn target_gone(world: &mut World, id: u16) {
    use crate::buildings::trading;
    let Some(c) = data(world, id).cloned() else { return };
    if is_building(world, c.target) {
        trading::remove_transport(world, c.target as u16, id, trading::transport::CART);
    }
    if let Some(m) = data_mut(world, id) {
        m.target = 0;
    }
    if c.source != 0 {
        let building = is_building(world, c.source);
        if let Some(m) = data_mut(world, id) {
            m.to_target = false;
            m.source = 0;
            m.target = c.source;
        }
        if building {
            trading::add_transport(world, c.source as u16, id, trading::transport::CART, c.foreign);
            drive_to_target(world, id);
            return;
        }
        if data(world, id).is_some_and(CartData::has_cargo) {
            drop_all(world, id);
        }
        if let Some(m) = data_mut(world, id) {
            m.target = 0;
        }
    }
    if let Some(m) = data_mut(world, id) {
        m.state = 0;
    }
    go_home(world, id);
}

/// 0x55cc10: the source (+0x11c) is gone or the transport is called off. Driving there, the
/// source forgets it, the target is cleared, the cart is free and goes home.
pub fn source_gone(world: &mut World, id: u16) {
    use crate::buildings::trading;
    let Some(c) = data(world, id).cloned() else { return };
    if let Some(m) = data_mut(world, id) {
        m.source = 0;
    }
    if !c.to_source {
        return;
    }
    if is_building(world, c.source) {
        trading::remove_transport(world, c.source as u16, id, trading::transport::CART);
    }
    if let Some(m) = data_mut(world, id) {
        m.target = 0;
        m.state = 0;
    }
    go_home(world, id);
}

/// 0x55c920 `(good, n)`: take up to `n` of `good` out of the slots (a slot left empty forgets its
/// good); without cargo VULNERABLE goes off. Returns what was not there.
pub fn unload(world: &mut World, id: u16, good: i32, n: i32) -> i32 {
    let mut n = n;
    if let Some(c) = data_mut(world, id) {
        for s in c.slots.iter_mut().filter(|s| s[0] == good) {
            let k = s[1].min(n);
            s[1] -= k;
            if s[1] == 0 {
                s[0] = 0;
            }
            n -= k;
        }
    }
    if !data(world, id).is_some_and(CartData::has_cargo)
        && let Some(e) = world.entities.by_index_mut(id as u32)
    {
        e.flags &= !flag::VULNERABLE_MASK;
    }
    n
}

/// 0x55b7d0: the whole load goes down where the cart stands (per good, 8 at a time,
/// `SearchSpaceForGoods`), the foundation goods (0x55d910) and settlers (0x55d460) too.
pub fn drop_all(world: &mut World, id: u16) {
    let pos = pos_of(world, id);
    for g in 0..0x2b {
        let mut n = data(world, id).map_or(0, |c| c.amount_of(g));
        while n != 0 {
            let k = n.min(SLOT_SIZE);
            crate::pile::search_space_for_goods(world, pos.x(), pos.y(), g as u8, k as u8);
            unload(world, id, g, k);
            n -= k;
        }
    }
    drop_goods(world, id);
    release_settlers(world, id);
    if let Some(e) = world.entities.by_index_mut(id as u32) {
        e.flags &= !flag::VULNERABLE_MASK;
    }
}

/// 0x55d910: every foundation good that arrived and is needed goes down — its *needed* amount,
/// 8 at a time — and the list is cleared.
fn drop_goods(world: &mut World, id: u16) {
    let pos = pos_of(world, id);
    let goods = data(world, id).map(|c| c.goods.clone()).unwrap_or_default();
    for g in goods {
        if g.arrived != 0 && g.needed != 0 {
            let mut done = 0;
            while done < g.needed {
                let k = (g.needed - done).min(SLOT_SIZE);
                crate::pile::search_space_for_goods(world, pos.x(), pos.y(), g.good as u8, k as u8);
                done += k;
            }
        }
    }
    if let Some(c) = data_mut(world, id) {
        c.goods.clear();
    }
    if let Some(e) = world.entities.by_index_mut(id as u32) {
        e.flags &= !flag::VULNERABLE_MASK;
    }
}

/// 0x55d460: settlers on their way get event 9 (the cart is gone; a carrier is deleted when the
/// cart stands on unowned land, else 0x4d4090 gives it back to the eco-sector); those inside
/// get out at the node 8 + (dir + 3) % 6 of the spiral table (event 0x19, then a move order).
/// The list is cleared.
fn release_settlers(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let pos = e.pos;
    let dir = get(world, id).map_or(0, |v| v.dir as i32);
    let i = 8 + ((dir + 3).rem_euclid(6)) as usize;
    let exit = Y16X16::pack(pos.x() + crate::spiral::dx(i), pos.y() + crate::spiral::dy(i)).0 as i32;
    let settlers = data(world, id).map(|c| c.settlers.clone()).unwrap_or_default();
    for s in settlers {
        let sid = s.id as u16;
        if !s.inside {
            send(world, sid, EntityEvent { event: 9, kind: 0, data: [id as i32, 0, 0] });
            let carrier = world.entities.by_index(sid as u32).is_some_and(|e| e.kind == SETTLER_CARRIER as u16);
            if carrier {
                let eco = eco_at(world, pos);
                if eco == 0 {
                    world.kill(sid, 0);
                } else {
                    crate::eco::set_settler_offer(world, eco, SETTLER_CARRIER as u16, sid);
                }
            }
        } else {
            send(world, sid, EntityEvent { event: 0x19, kind: 0, data: [0, exit, 0] });
            send(world, sid, EntityEvent { event: 0x11, kind: 0xd, data: [0, exit, 0] });
        }
    }
    if let Some(c) = data_mut(world, id) {
        c.settlers.clear();
    }
}

/// vtbl+0x10 (0x55dbc0) before `CVehicle` 0x594a10: a source (+0x11c) trading building forgets
/// the transport (0x552e10 kind 2) and its +0x280 / +0x284 / +0x288 (the original clears the
/// donkey slot fields there); the load goes down (0x55b7d0), or on board (0x8000) the passengers die
/// (0x55dfc0); a ferry (+0x98) drops the cart (vtbl+0x7c, ships: not ported).
pub fn on_delete(world: &mut World, id: u16) {
    if !is_cart(world, id) {
        return;
    }
    let Some(c) = data(world, id) else { return };
    let source = c.source;
    if is_building(world, source) {
        crate::buildings::trading::cart_deleted(world, source as u16, id);
    }
    let on_board = world.entities.by_index(id as u32).is_some_and(|e| e.flags & flag::ON_BOARD != 0);
    if on_board {
        let inside: Vec<u16> = data(world, id).map(|c| c.settlers.iter().filter(|s| s.inside).map(|s| s.id as u16).collect()).unwrap_or_default();
        for s in inside {
            world.kill(s, 0);
        }
        if let Some(c) = data_mut(world, id) {
            c.settlers.clear();
        }
    } else {
        drop_all(world, id);
    }
    let ferry = get(world, id).map_or(0, |v| v.target_entity);
    if is_vehicle(world, ferry) {
        super::remove_passenger(world, ferry as u16, id);
    }
}

/// vtbl+0x20 (0x55c1d0) `(damage)`: a trade cart hit with cargo drops it and stops trading
/// (0x55b7d0, 0x55caf0); else VULNERABLE goes off. A foundation cart loses hit points and dies
/// at 0 (`Kill(id, attacker)` with the global attacker 0x129b0b8: 0 here).
pub fn hit(world: &mut World, id: u16, damage: i32) {
    let Some(c) = data(world, id) else { return };
    if !c.foundation {
        if c.has_cargo() {
            drop_all(world, id);
            if let Some(e) = world.entities.by_index_mut(id as u32) {
                e.flags &= !flag::VULNERABLE_MASK;
            }
            target_gone(world, id);
            return;
        }
        if let Some(e) = world.entities.by_index_mut(id as u32) {
            e.flags &= !flag::VULNERABLE_MASK;
        }
        return;
    }
    if damage <= 0 {
        return;
    }
    let Some(v) = get_mut(world, id) else { return };
    if damage >= v.health as i32 {
        v.health = 0;
        world.kill(id, 0);
    } else {
        v.health -= damage as u8;
    }
}

// ---- passengers (the vehicle's observer list +0x8c) ----

/// 0x56e0a0 (`CVehicle` vtbl+0xa4 0x5955e0): `settler` joins the vehicle's observer list
/// (+0x8c, at the end) and the vehicle becomes its home (`SetObserverTarget(0, vehicle)`,
/// ATTACHED; a land vehicle keeps it as `CWheeler` +0x98, 0x59a7d0).
pub fn attach(world: &mut World, vehicle: u16, settler: u16) {
    if let Some(v) = get_mut(world, vehicle) {
        v.passengers.push(settler);
    }
    set_home(world, settler, vehicle);
}

/// `SetObserverTarget(0, home)` (vtbl+0x44): a settler's role +0x20 (0x586ff0) or a land
/// vehicle's +0x98 (0x59a7d0), ATTACHED with a home.
fn set_home(world: &mut World, entity: u16, home: u16) {
    let land = world.entities.by_index(entity as u32).is_some_and(|e| e.obj_type == ObjType::LandVehicle);
    if land {
        if let Some(v) = get_mut(world, entity) {
            v.target_entity = home as i32;
        }
    } else if let Some(j) = crate::jobs::core::job_mut(world, entity) {
        j.home = home;
    }
    if let Some(e) = world.entities.by_index_mut(entity as u32) {
        if home != 0 {
            e.flags |= flag::ATTACHED;
        } else {
            e.flags &= !flag::ATTACHED;
        }
    }
}

/// 0x56e270: off the observer list (the first entry of the settler).
fn observer_remove(world: &mut World, vehicle: u16, settler: u16) {
    if let Some(v) = get_mut(world, vehicle)
        && let Some(k) = v.passengers.iter().position(|&p| p == settler)
    {
        v.passengers.remove(k);
    }
}

/// `SetObserverTarget(0, 0)`: no home, ATTACHED off.
fn clear_home(world: &mut World, settler: u16) {
    set_home(world, settler, 0);
}

/// `CVehicle` 0x595600's part: off the observer list, no home.
pub(crate) fn detach_observer(world: &mut World, vehicle: u16, settler: u16) {
    observer_remove(world, vehicle, settler);
    clear_home(world, settler);
}

/// vtbl+0x40 (0x55d310): the settler is detached: out of the settler list (vtbl+0x7c), off the
/// observer list, no home (vtbl+0xc0 is empty).
pub fn detach_settler(world: &mut World, cart: u16, settler: u16) {
    remove_settler(world, cart, settler);
    observer_remove(world, cart, settler);
    clear_home(world, settler);
}

/// vtbl+0x7c (0x55d350): a called settler that is not inside yet leaves the list; one more
/// pioneer (type 0x2f) or carrier (type 1) is to be called.
pub(super) fn remove_settler(world: &mut World, cart: u16, settler: u16) {
    let Some(c) = data_mut(world, cart) else { return };
    let Some(k) = c.settlers.iter().position(|s| s.id == settler as i32) else { return };
    let s = c.settlers[k];
    if s.called == 0 || s.inside {
        return;
    }
    if c.settlers.iter().any(|x| x.id == settler as i32 && x.kind == SETTLER_PIONEER) {
        c.pioneers_due = c.pioneers_due.wrapping_add(1);
    }
    if c.settlers.iter().any(|x| x.id == settler as i32 && x.kind == SETTLER_CARRIER) {
        c.carriers_due = c.carriers_due.wrapping_add(1);
    }
    c.settlers.retain(|x| x.id != settler as i32);
}

/// vtbl+0x80 (0x55d0c0): a called settler arrived (its CHECKIN): inside, counted, off the war
/// map and the map (ON_BOARD); a carrier leaves its eco-sector's count (0x4d4250); off the
/// observer list, no home. Only for a foundation cart.
pub fn settler_entered(world: &mut World, cart: u16, settler: u16) {
    let Some(c) = data(world, cart) else { return };
    if !c.foundation || !c.settlers.iter().any(|s| s.id != 0 && s.id == settler as i32) {
        return;
    }
    let cart_pos = pos_of(world, cart);
    if let Some(c) = data_mut(world, cart) {
        for s in c.settlers.iter_mut().filter(|s| s.id == settler as i32) {
            s.inside = true;
        }
        if c.settlers.iter().any(|s| s.id == settler as i32 && s.kind == SETTLER_PIONEER) {
            c.pioneers = c.pioneers.wrapping_add(1);
        }
        if c.settlers.iter().any(|s| s.id == settler as i32 && s.kind == SETTLER_CARRIER) {
            c.carriers = c.carriers.wrapping_add(1);
        }
    }
    if world.warmap.contains(settler) {
        crate::warmap::remove(world, settler);
    }
    let Some(e) = world.entities.by_index(settler as u32) else { return };
    let (pos, kind) = (e.pos, e.kind);
    if let Some(n) = world.grid.checked_index(pos.x(), pos.y()) {
        world.layers.map_object[n] = 0;
    }
    if let Some(e) = world.entities.by_index_mut(settler as u32) {
        e.flags |= flag::ON_BOARD;
    }
    let carrier = data(world, cart).is_some_and(|c| c.settlers.iter().any(|s| s.id == settler as i32 && s.kind == SETTLER_CARRIER));
    let eco = eco_at(world, cart_pos);
    if carrier && eco != 0 {
        crate::eco::change_nr_of_settler(world, eco, kind, -1);
    }
    observer_remove(world, cart, settler);
    clear_home(world, settler);
}

/// vtbl+0x78 (0x55d760): `n` more of `good` ordered (+0xc of its entry).
pub fn good_ordered(world: &mut World, cart: u16, good: i32, n: i32) {
    if let Some(c) = data_mut(world, cart)
        && let Some(g) = c.goods.iter_mut().find(|g| g.good == good)
    {
        g.ordered += n;
    }
}

/// vtbl+0x84 (0x55d7e0): `n` of `good` arrived: counted (+8 and +0xbe) while not more than
/// needed have arrived.
pub fn good_arrived(world: &mut World, cart: u16, good: i32, n: i32) {
    if let Some(c) = data_mut(world, cart)
        && let Some(k) = c.goods.iter().position(|g| g.good == good)
        && c.goods[k].arrived <= c.goods[k].needed
    {
        c.goods[k].arrived += n;
        c.goods_in = c.goods_in.wrapping_add(n as u8);
    }
}

/// vtbl+0x88 (0x55d880): an order of `n` of `good` fell through: +0xc and +0xbe go down (the
/// original lowers the arrived count +0xbe too).
pub fn good_cancelled(world: &mut World, cart: u16, good: i32, n: i32) {
    if let Some(c) = data_mut(world, cart)
        && let Some(k) = c.goods.iter().position(|g| g.good == good)
    {
        c.goods[k].ordered -= n;
        c.goods_in = c.goods_in.wrapping_sub(n as u8);
    }
}

// ---- the foundation cart ----

/// 0x4d4280: the goods of `good` the eco-sector offers (the amounts along its offer list).
fn offered_goods(world: &World, eco: u16, good: i32) -> i32 {
    let Some(e) = world.economy.get(eco) else { return 0 };
    let mut id = e.good_offer.get(good as usize).copied().unwrap_or(0);
    let mut n = 0;
    while let Some(p) = crate::pile::get(world, id) {
        n += p.amount as i32;
        id = world.entities.by_index(id as u32).map_or(0, |e| e.next);
    }
    n
}

/// 0x55c2d0: the cart stands in an eco-sector that offers 4 boards, 8 stones and 2 hammers.
pub fn sector_has_goods(world: &World, id: u16) -> bool {
    let eco = eco_at(world, pos_of(world, id));
    eco != 0 && offered_goods(world, eco, good::BOARD) >= config::BOARDS && offered_goods(world, eco, good::STONE) >= config::STONES && offered_goods(world, eco, good::HAMMER) >= config::HAMMERS
}

/// The three goods entries of a foundation cart `{needed, good, arrived, ordered, 0}`.
fn foundation_goods(filled: bool) -> Vec<CartGood> {
    [(config::BOARDS, good::BOARD), (config::STONES, good::STONE), (config::HAMMERS, good::HAMMER)]
        .into_iter()
        .map(|(n, g)| CartGood { needed: n, good: g, arrived: if filled { n } else { 0 }, ordered: if filled { n } else { 0 }, tick: 0 })
        .collect()
}

/// Command 5012 `CreateFoundationCart` (0x55c360): a trade cart on its owner's land whose
/// eco-sector offers the goods becomes a foundation cart: 8 pioneers and 2 carriers to call,
/// the goods entries (4 boards, 8 stones, 2 hammers; +0xbf 14), VULNERABLE, waiting (task 0x1b)
/// and updated in 16 ticks. A missing good tells the owner which (0x52a460 at the cart: 0x9a2
/// boards, 0x9a3 stones, 0x9a0 hammers; the first missing one).
pub fn create_foundation_cart(world: &mut World, id: u16) -> bool {
    let Some(c) = data(world, id) else { return false };
    if c.foundation || !c.own_land {
        return false;
    }
    let pos = pos_of(world, id);
    let eco = eco_at(world, pos);
    if eco == 0 {
        return false;
    }
    let owner = world.entities.by_index(id as u32).map_or(0, |e| e.owner);
    let missing = [(good::BOARD, config::BOARDS, 0x9a2), (good::STONE, config::STONES, 0x9a3), (good::HAMMER, config::HAMMERS, 0x9a0)].into_iter().find(|&(g, n, _)| offered_goods(world, eco, g) < n);
    if let Some((_, _, text)) = missing {
        world.events.push(crate::event::SimEvent::warning(owner, text, pos));
        return false;
    }
    if let Some(c) = data_mut(world, id) {
        c.pioneers_due = config::PIONEERS;
        c.carriers_due = config::CARRIERS;
        c.goods_in = 0;
        c.foundation = true;
        c.settlers.clear();
        c.goods = foundation_goods(false);
        c.goods_needed = (config::BOARDS + config::STONES + config::HAMMERS) as u8;
    }
    if let Some(e) = world.entities.by_index_mut(id as u32) {
        e.flags |= flag::VULNERABLE_MASK;
    }
    world.register_for_logic_update(id, 16);
    if let Some(v) = get_mut(world, id) {
        v.task = TASK_WAIT;
    }
    true
}

/// The `AddVehicle` type 0x406 part of the ctor (0x55e020): a foundation cart that comes full:
/// its goods arrived, 10 pioneers (`CreateSettler(1, 1, owner, 0x2f)`, off the map at once,
/// invisible, not selectable, not updated) inside; +0xba / +0xbb stay 8 / 2.
fn fill(world: &mut World, id: u16) {
    let owner = world.entities.by_index(id as u32).map_or(0, |e| e.owner);
    if let Some(c) = data_mut(world, id) {
        c.pioneers_due = config::PIONEERS;
        c.carriers_due = config::CARRIERS;
        c.foundation = true;
        c.settlers.clear();
        c.goods = foundation_goods(true);
        c.goods_needed = (config::BOARDS + config::STONES + config::HAMMERS) as u8;
        c.goods_in = c.goods_needed;
        c.pioneers = 0;
    }
    if let Some(e) = world.entities.by_index_mut(id as u32) {
        e.flags |= flag::VULNERABLE_MASK;
    }
    world.register_for_logic_update(id, 1);
    if let Some(v) = get_mut(world, id) {
        v.task = TASK_WAIT;
    }
    for _ in 0..(config::PIONEERS + config::CARRIERS) {
        let Some(s) = crate::create::add_settler(world, 1, 1, owner, SETTLER_PIONEER as u16) else { return };
        crate::jobs::core::take_wait_list(world, s);
        let spos = pos_of(world, s);
        if let Some(n) = world.grid.checked_index(spos.x(), spos.y()) {
            world.layers.map_object[n] = 0;
        }
        world.unregister_from_logic_update(s);
        if let Some(e) = world.entities.by_index_mut(s as u32) {
            e.flags = (e.flags | flag::ON_BOARD) & !0x2100;
        }
        if let Some(c) = data_mut(world, id) {
            c.settlers.push(CartSettler { called: 1, inside: true, id: s as i32, kind: SETTLER_PIONEER });
            c.pioneers = c.pioneers.wrapping_add(1);
        }
    }
}

/// 0x583130: the nearest pioneer of the cart's owner that is free (flag 0x2000) on the cart's
/// sector is called: 0x2000 off, event 0x18 with the cart.
fn call_pioneer(world: &mut World, cart: u16) -> u16 {
    let Some(e) = world.entities.by_index(cart as u32) else { return 0 };
    let (owner, pos) = (e.owner, e.pos);
    let Some(n) = world.grid.checked_index(pos.x(), pos.y()) else { return 0 };
    let sector = world.tiling.sector_of(n);
    let (mut best, mut best_d) = (0u16, u32::MAX);
    for s in crate::settler_lists::iter(world, owner, SETTLER_PIONEER as u16) {
        let Some(p) = world.entities.by_index(s as u32) else { continue };
        if p.flags & flag::SELECTABLE == 0 {
            continue;
        }
        let Some(pn) = world.grid.checked_index(p.x(), p.y()) else { continue };
        if world.tiling.sector_of(pn) != sector {
            continue;
        }
        let d = crate::spiral::grid_distance(pos.x() - p.x(), pos.y() - p.y()) as u32;
        if d < best_d {
            best_d = d;
            best = s;
        }
    }
    if best == 0 {
        return 0;
    }
    if let Some(p) = world.entities.by_index_mut(best as u32) {
        p.flags &= !flag::SELECTABLE;
    }
    send(world, best, EntityEvent { event: 0x18, kind: 0, data: [cart as i32, 0, 0] });
    best
}

/// 0x55ce00: call the pioneers still to come (0x583130) and then the carriers (the nearest
/// offered one of the eco-sector the cart stands in, 0x4d4af0: out of the offer, event 0x18,
/// 0x2200 off). Each loop runs while its counter is below the number still due, which goes down
/// with every call (so about half of them per update). Without an eco-sector at the cart: false.
fn call_settlers(world: &mut World, cart: u16) -> bool {
    let mut k = 0;
    while data(world, cart).is_some_and(|c| k < c.pioneers_due as i32) {
        let p = call_pioneer(world, cart);
        if p > 0
            && let Some(c) = data_mut(world, cart)
        {
            c.pioneers_due -= 1;
            c.settlers.push(CartSettler { called: 1, inside: false, id: p as i32, kind: SETTLER_PIONEER });
        }
        k += 1;
    }
    let mut k = 0;
    while data(world, cart).is_some_and(|c| k < c.carriers_due as i32) {
        let pos = pos_of(world, cart);
        let eco = eco_at(world, pos);
        if eco == 0 || world.economy.get(eco).is_none() {
            return false;
        }
        let c = crate::eco::get_nearest_settler(world, eco, SETTLER_CARRIER as u16, pos.x(), pos.y());
        if c > 0 {
            crate::eco::get_settler_out_of_offer(world, eco, c);
            if world.entities.by_index(c as u32).is_none_or(|e| e.obj_type != ObjType::Settler) {
                return false;
            }
            send(world, c, EntityEvent { event: 0x18, kind: 0, data: [cart as i32, 0, 0] });
            if let Some(e) = world.entities.by_index_mut(c as u32) {
                e.flags &= !0x2200;
            }
            if let Some(cd) = data_mut(world, cart) {
                cd.carriers_due -= 1;
                cd.settlers.push(CartSettler { called: 1, inside: false, id: c as i32, kind: SETTLER_CARRIER });
            }
        }
        k += 1;
    }
    true
}

/// 0x4d5a30 `(cart, good)`: the eco-sector's nearest pile offering `good` with goods not all
/// leaving (`GetNearestPile`), the nearest offered carrier to it; the carrier leaves the offer,
/// the pile reserves one good for it (`AttachAndIncAmountLeaving(1, 2)`), the cart becomes its
/// home (vtbl+0xa4) and it gets event 11 `[1, pile, cart]`. Returns the carrier or −1.
fn request_good(world: &mut World, eco: u16, cart: u16, good: i32) -> i32 {
    let pos = pos_of(world, cart);
    let head = world.economy.get(eco).and_then(|e| e.good_offer.get(good as usize).copied()).unwrap_or(0);
    let pile = crate::pile::get_nearest_pile(world, head, pos.x(), pos.y());
    if pile == 0 {
        return -1;
    }
    let pp = pos_of(world, pile);
    let c = crate::eco::get_nearest_settler(world, eco, SETTLER_CARRIER as u16, pp.x(), pp.y());
    if c == 0 {
        return -1;
    }
    crate::eco::get_settler_out_of_offer(world, eco, c);
    crate::pile::attach_and_inc_amount_leaving(world, pile, c, 1, crate::pile::target::PILE_SRC);
    attach(world, cart, c);
    send(world, c, EntityEvent { event: 11, kind: 0, data: [1, pile as i32, cart as i32] });
    c as i32
}

/// 0x55d660: on a sector, each goods entry ordered less than needed asks the eco-sector of the
/// cart for the rest, one transport each (0x4d5a30), and notes the tick.
fn order_goods(world: &mut World, cart: u16) -> bool {
    let pos = pos_of(world, cart);
    let Some(n) = world.grid.checked_index(pos.x(), pos.y()) else { return false };
    if world.tiling.eco_sector_of(n) == 0 {
        return false;
    }
    let eco = eco_at(world, pos);
    let goods = data(world, cart).map(|c| c.goods.clone()).unwrap_or_default();
    for (k, g) in goods.iter().enumerate() {
        // The original re-reads `needed − ordered` per request; requests do not change it.
        for _ in 0..(g.needed - g.ordered).max(0) {
            request_good(world, eco, cart, g.good);
            let tick = world.tick as i32;
            if let Some(c) = data_mut(world, cart)
                && let Some(x) = c.goods.get_mut(k)
            {
                x.tick = tick;
            }
        }
    }
    true
}

/// 0x55da00: room for a settlement: more than 27 of the nodes within spiral radius 3 on the
/// cart's sector belong to its owner or nobody, are not blocked land and hold no map object.
pub fn room_for_settlement(world: &World, id: u16) -> bool {
    let Some(e) = world.entities.by_index(id as u32) else { return false };
    let (owner, pos) = (e.owner, e.pos);
    area(world, pos)
        .into_iter()
        .filter(|&n| {
            let o = world.owner_of(n);
            (o == owner || o == 0) && world.layers.flags[n] & crate::grid::flag::BLOCKED_LAND == 0 && world.layers.map_object[n] == 0
        })
        .count()
        > 0x1b
}

/// The nodes of the area iterator 0x431270: spiral entries up to radius 3 around `pos` that lie
/// on the sector of `pos`.
fn area(world: &World, pos: Y16X16) -> Vec<usize> {
    let Some(c) = world.grid.checked_index(pos.x(), pos.y()) else { return Vec::new() };
    let sector = world.tiling.sector_of(c);
    crate::spiral::walk(world.grid, pos.x(), pos.y(), 3).map(|(x, y)| world.grid.index(x as u32, y as u32)).filter(|&n| world.tiling.sector_of(n) == sector).collect()
}

/// 0x4f6190: the commands 5011..=5013 act on a READY cart (a vehicle that casts to `CCart`).
pub fn command_target(world: &World, id: u32) -> Option<u16> {
    let id = u16::try_from(id).ok()?;
    let e = world.entities.by_index(id as u32)?;
    (is_cart(world, id) && e.flags & flag::READY != 0).then_some(id)
}

/// Command 5011 `CreateSettlement` (0x55c130): a foundation cart with room (0x55da00) claims
/// the area for its owner (0x55dae0: nodes of the owner or nobody, `CTiling` vtbl+0x1c), lets
/// its settlers out (0x55d460), puts its goods down (0x55d910) and turns into a pile of 5 boards
/// (0x55c250: `AddPile(x, y, 7, 5, 3)`, invisible, deleted).
pub fn create_settlement(world: &mut World, id: u16) -> bool {
    if !data(world, id).is_some_and(|c| c.foundation) || !room_for_settlement(world, id) {
        return false;
    }
    let Some(e) = world.entities.by_index(id as u32) else { return false };
    let (owner, pos) = (e.owner, e.pos);
    for n in area(world, pos) {
        let o = world.owner_of(n);
        if o == owner || o == 0 {
            let s = world.grid.size() as usize;
            world.change_owner((n % s) as i32, (n / s) as i32, owner);
        }
    }
    release_settlers(world, id);
    drop_goods(world, id);
    crate::pile::add_pile(world, pos.x(), pos.y(), good::BOARD as u8, config::RECYCLED_BOARDS, crate::pile::role::ANYWHERE);
    if let Some(e) = world.entities.by_index_mut(id as u32) {
        e.flags &= !flag::VISIBLE;
    }
    super::delete(world, id);
    true
}

/// Command 5013 `CreateNormalCart` (0x55c020): on a foundation cart the called settlers not
/// inside yet get event 9 with the cart, then the settlers (0x55d460) and goods (0x55d910) come
/// out. +0xbc stays set (the original does not clear it), so the cart goes on calling the
/// settlers still due.
pub fn create_normal_cart(world: &mut World, id: u16) -> bool {
    let Some(c) = data(world, id) else { return false };
    if !c.foundation {
        return false;
    }
    let called: Vec<u16> = c.settlers.iter().filter(|s| !s.inside && s.called != 0).map(|s| s.id as u16).collect();
    for s in called {
        send(world, s, EntityEvent { event: 9, kind: 0, data: [id as i32, 0, 0] });
    }
    release_settlers(world, id);
    drop_goods(world, id);
    true
}

// ---- the dialog (client) ----

/// What the cart's dialog shows (vtbl+0x94 0x55bdb0: message 0x25a, 0x25c when it opens, with
/// the packet at 0x11aa918, type 0x15). The client sends it; the sim only answers the queries.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CartDialog {
    /// 0x11aaa10: a foundation cart with room for a settlement (0x55da00) — VILLAGE (5011).
    pub can_settle: bool,
    /// 0x11aaa11: +0xbc.
    pub foundation: bool,
    /// 0x11aaa12: a trade cart on its owner's land whose eco-sector offers the goods (0x55c2d0) —
    /// MORPH2FOUNDATIONCART (5012).
    pub can_morph: bool,
    /// 0x11aaa14 / 0x11aaa18: settlers inside (+0xb8 + +0xb9) and needed (8 + 2); 0 for a
    /// trade cart.
    pub settlers_in: u32,
    pub settlers_needed: u32,
    /// From 0x11aa920, 12 bytes each: a trade cart's six slots `{good, amount, _}`, a
    /// foundation cart's goods `{good, arrived, ordered}` (the copy loop reads each 20-byte
    /// record at +4, +8, +0xc).
    pub rows: Vec<[i32; 3]>,
}

/// The fields of 0x55bdb0 for cart `id`.
pub fn dialog(world: &World, id: u16) -> Option<CartDialog> {
    let c = data(world, id)?;
    if !c.foundation {
        return Some(CartDialog {
            can_settle: false,
            foundation: false,
            can_morph: c.own_land && sector_has_goods(world, id),
            settlers_in: 0,
            settlers_needed: 0,
            rows: c.slots.iter().map(|s| [s[0], s[1], 0]).collect(),
        });
    }
    Some(CartDialog {
        can_settle: room_for_settlement(world, id),
        foundation: true,
        can_morph: false,
        settlers_in: c.pioneers as u32 + c.carriers as u32,
        settlers_needed: (config::PIONEERS + config::CARRIERS) as u32,
        rows: c.goods.iter().map(|g| [g.good, g.arrived, g.ordered]).collect(),
    })
}
