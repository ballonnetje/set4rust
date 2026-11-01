//! `CManakopterHallRole` (house kind 21, vtable 0x10593d0, the Dark Tribe's manakopter hall).
//! Own reverse engineering of the shipped exe; decompiles in
//! `tools/re/decompiled/script/manakopter/hall.c`, `hall2.c` and `script/ai/dtagents.c`.
//!
//! The hall keeps three stocks (+0x17c mana, +0x180 boards, +0x184 iron) and needs
//! `DTManakopterRequired{Mana, Boards, Iron}` of them for one manakopter
//! ([`crate::magic::DarkMagicCfg::manakopter_required`]). `Init` (0x547990) starts it full, in
//! state 2. In state 2 the `LogicUpdate` (0x547a20, every 31 ticks) adds one mana while the mana
//! is below the boards, the iron and the mana required. Boards and iron come only from the
//! shaman's teleport (0x547d70 / 0x547ce0). [`start`] (0x547dc0, the AI's hall agent 0x430c20)
//! turns to state 3 with full stocks and no manakopter of the player; state 3 takes one unit
//! per update from the stock with the most still to use (ties: iron over mana, boards over
//! both), each update `3 · frames(slot 5) − 1` ticks after the last, and when all are used up
//! creates the manakopter at the ensign (0x569ca0) and returns to state 2.

use crate::World;
use crate::hash::StateHasher;

/// `CManakopterHallRole` +4 and +0x17c..+0x194.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct State {
    /// +4: 2 collecting, 3 building a manakopter.
    pub state: u8,
    /// +0x17c / +0x180 / +0x184: mana, boards (good 7), iron (good 20) in stock.
    pub stock: [i32; 3],
    /// +0x188 / +0x18c / +0x190: what the manakopter being built has used of each.
    pub used: [i32; 3],
    /// +0x194: the frame of the build picture (patch slot 3): `(frames − 2) · used / required`.
    pub progress: i32,
    /// The patch slot whose animation runs (+0x100 / +0x114 set, the release's slots 5 and 6):
    /// true in state 3 between units. Drawing only.
    pub animating: bool,
}

impl State {
    pub(crate) fn hash(&self, h: &mut StateHasher) {
        h.u8(self.state);
        self.stock.iter().chain(&self.used).for_each(|&v| h.u32(v as u32));
        h.u32(self.progress as u32);
        h.bool(self.animating);
    }
}

fn st(world: &mut World, hall: u16) -> Option<&mut State> {
    match world.buildings.roles.get_mut(&hall).map(|r| &mut r.kind) {
        Some(super::RoleKind::ManakopterHall(s)) => Some(s),
        _ => None,
    }
}

/// The hall's state, if `hall` is a manakopter hall.
pub fn get(world: &World, hall: u16) -> Option<&State> {
    match world.buildings.roles.get(&hall).map(|r| &r.kind) {
        Some(super::RoleKind::ManakopterHall(s)) => Some(s),
        _ => None,
    }
}

/// The player's manakopter hall (`CBuildingMgr` list head 0x14eea74 + (player · 0x53 + 80) · 4,
/// read at 0x4440d0 as 0x14eebb4 + player · 0x14c; one hall per player): here the first live
/// building of the player with this role.
pub fn hall_of(world: &World, player: u8) -> Option<u16> {
    world.buildings.roles.iter().find_map(|(&id, r)| {
        let ok = matches!(r.kind, super::RoleKind::ManakopterHall(_)) && world.entities.by_index(id as u32).is_some_and(|e| e.owner == player && !e.is_dead());
        ok.then_some(id)
    })
}

/// 0x547d70 `(good, amount)`: boards and iron are counted; any other good is only traced
/// ("Shaman teleported %d invalid goods %s to ManakopterHall."). (0x547ce0, vtbl+0x44, is the
/// same for one good.)
pub fn add_goods(world: &mut World, hall: u16, good: u8, amount: i32) {
    let Some(s) = st(world, hall) else { return };
    match good {
        7 => s.stock[1] += amount,
        20 => s.stock[2] += amount,
        _ => {}
    }
}

fn required(world: &World) -> [i32; 3] {
    world.rules.magic.dark.manakopter_required
}

/// Frames of the hall's patch slot `k` (`GetJobFrameCount(10 + race, job, 0)`).
fn slot_frames(world: &World, hall: u16, k: usize) -> i32 {
    let race = world.entities.by_index(hall as u32).map_or(3, |e| e.race) as usize;
    world.rules.dark.hall_patch_frames.get(race).map_or(0, |f| f[k] as i32)
}

/// `Init` (vtbl+0x18, 0x547990), after `InitCommon`: state 2, full stocks, nothing used, the
/// first update in 14 ticks.
pub fn init(world: &mut World, hall: u16) {
    let req = required(world);
    if let Some(s) = st(world, hall) {
        *s = State { state: 2, stock: req, ..Default::default() };
    }
    world.register_for_logic_update(hall, 0xe);
}

/// `LogicUpdate` (vtbl+0xc, 0x547a20); see the module doc.
pub fn logic_update(world: &mut World, hall: u16) {
    let req = required(world);
    let Some(s) = st(world, hall) else { return };
    match s.state {
        2 => {
            if s.stock[0] < s.stock[1] && s.stock[0] < s.stock[2] && s.stock[0] < req[0] {
                s.stock[0] += 1;
            }
        }
        3 => {
            let mut k = 0;
            let mut left = req[0] - s.used[0];
            if left <= req[2] - s.used[2] {
                k = 2;
                left = req[2] - s.used[2];
            }
            if left <= req[1] - s.used[1] {
                k = 1;
                left = req[1] - s.used[1];
            }
            if left > 0 {
                s.used[k] += 1;
                s.stock[k] -= 1;
                s.animating = true;
                let used: i32 = s.used.iter().sum();
                let (f3, f5) = (slot_frames(world, hall, 3), slot_frames(world, hall, 5));
                let sum = req.iter().sum::<i32>();
                let progress = if sum != 0 { (f3 - 2) * used / sum } else { 0 };
                if let Some(s) = st(world, hall) {
                    s.progress = progress;
                }
                world.register_for_logic_update(hall, f5 * 3 - 1);
                return;
            }
            let Some(e) = world.entities.by_index(hall as u32) else { return };
            let (owner, ensign) = (e.owner, match &e.data {
                crate::entity::EntityData::Building(b) => b.ensign,
                _ => e.pos,
            });
            crate::flying::add_manakopter(world, ensign.x(), ensign.y(), owner);
            if let Some(s) = st(world, hall) {
                s.progress = 0;
                s.used = [0; 3];
                s.animating = false;
                s.state = 2;
            }
        }
        _ => return,
    }
    world.register_for_logic_update(hall, 0x1f);
}

/// 0x547dc0: already building → true; with full stocks and no manakopter of the owner: state 3,
/// an update next tick, true. Otherwise false.
pub fn start(world: &mut World, hall: u16) -> bool {
    let req = required(world);
    let owner = world.entities.by_index(hall as u32).map_or(0, |e| e.owner);
    let has = !crate::flying::list(world, owner, crate::flying::MANAKOPTER).is_empty();
    let Some(s) = st(world, hall) else { return false };
    if s.state == 3 {
        return true;
    }
    if s.stock[0] >= req[0] && s.stock[1] >= req[1] && s.stock[2] >= req[2] && !has {
        s.state = 3;
        world.register_for_logic_update(hall, 1);
        return true;
    }
    false
}

/// 0x547e30: with no manakopter, a stock short and not building, the shaman AI (`CAIShaman`
/// vtbl+0 0x4440d0, [`crate::ai::shaman::target`], the owner's enemies, same sector) is asked
/// for a target from the ensign; when it finds none the stocks are filled to what is required
/// and [`start`] runs (true). Otherwise false.
pub fn fill_for_ai(world: &mut World, hall: u16) -> bool {
    let req = required(world);
    let Some(e) = world.entities.by_index(hall as u32) else { return false };
    let (owner, ensign) = (e.owner, match &e.data {
        crate::entity::EntityData::Building(b) => b.ensign,
        _ => e.pos,
    });
    let has = !crate::flying::list(world, owner, crate::flying::MANAKOPTER).is_empty();
    let Some(s) = get(world, hall) else { return false };
    let short = s.stock[0] < req[0] || s.stock[1] < req[1] || s.stock[2] < req[2];
    if has || !short || s.state == 3 {
        return false;
    }
    let enemies = world.alliances.enemy_bits[owner.min(8) as usize] as u32;
    if crate::ai::shaman::target(world, owner, enemies, ensign.x(), ensign.y(), false).is_some() {
        return false;
    }
    if let Some(s) = st(world, hall) {
        s.stock = req;
    }
    start(world, hall);
    true
}
