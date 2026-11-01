//! `CSettlerMgr`'s lists of warriors per player and settler type (release .data 0x1544cec,
//! `[player * 67 + type]`, heads of intrusive lists through `IEntity +0x28/+0x2a`, i.e.
//! [`crate::entity::Entity::prev`] / [`crate::entity::Entity::next`]), and their lengths
//! (0x1545fc4).
//!
//! `AttachSettler` (0x582b40) is called by `AddSettler` (0x582600) for every type with a warrior
//! type (`m_uSettlerWarriorTypes`, non-zero for soldiers, specialists, donkeys, …): the settler
//! goes to the **head** of its list, entity flag OFFERED (0x40) is cleared and 0x40000000 set.
//! `DetachSettler` (0x582bb0) unlinks it and clears both flags. Searches over the lists (the
//! military buildings' soldier search 0x582ca0) therefore see the newest settler first.

use crate::entity::{ObjType, flag};
use crate::hash::StateHasher;
use crate::world::World;

/// `SETTLER_MAX`.
const TYPES: usize = 67;
const PLAYERS: usize = 9;

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SettlerLists {
    heads: Vec<u16>,
    counts: Vec<i32>,
    /// 0x1545658 `[player * 67 + type]`: every settler of the player and type, counted from
    /// `AddSettler` (0x582723) to its removal (0x582940 from `Kill`, never below 0) and moved by
    /// `ChangeType` (0x580db5 / 0x580efa); saved with the settlers (0x582040 / 0x582380). Read by
    /// `ChangeSpecialistWish` (0x4d5100), the AI and the script.
    #[cfg_attr(feature = "serde", serde(default = "per_player_type"))]
    settlers: Vec<i32>,
}

fn per_player_type() -> Vec<i32> {
    vec![0; PLAYERS * TYPES]
}

impl Default for SettlerLists {
    fn default() -> Self {
        SettlerLists { heads: vec![0; PLAYERS * TYPES], counts: vec![0; PLAYERS * TYPES], settlers: per_player_type() }
    }
}

fn slot(player: u8, kind: u16) -> Option<usize> {
    ((player as usize) < PLAYERS && (kind as usize) < TYPES).then(|| player as usize * TYPES + kind as usize)
}

impl SettlerLists {
    /// 0x582b20: the head of `player`'s list of `kind` (0 = empty).
    pub fn head(&self, player: u8, kind: u16) -> u16 {
        slot(player, kind).map_or(0, |i| self.heads[i])
    }

    /// 0x1545fc4: the number of settlers attached to the list.
    pub fn count(&self, player: u8, kind: u16) -> i32 {
        slot(player, kind).map_or(0, |i| self.counts[i])
    }

    /// 0x1545658: `player`'s settlers of `kind` (`CSettlerMgr::GetNumberOfSettlers`).
    pub fn settlers(&self, player: u8, kind: u16) -> i32 {
        slot(player, kind).map_or(0, |i| self.settlers.get(i).copied().unwrap_or(0))
    }

    /// One more settler of `kind` for `player` (0x582c40 without the statistic).
    pub(crate) fn settler_added(&mut self, player: u8, kind: u16) {
        if let Some(c) = slot(player, kind).and_then(|i| self.settlers.get_mut(i)) {
            *c += 1;
        }
    }

    /// One less, never below 0 (0x582c70 without the statistic).
    pub(crate) fn settler_removed(&mut self, player: u8, kind: u16) {
        if let Some(c) = slot(player, kind).and_then(|i| self.settlers.get_mut(i))
            && *c > 0
        {
            *c -= 1;
        }
    }

    pub(crate) fn hash(&self, h: &mut StateHasher) {
        h.u16s(&self.heads);
        self.counts.iter().for_each(|&c| h.u32(c as u32));
        self.settlers.iter().for_each(|&c| h.u32(c as u32));
    }
}

/// The settlers of `player`'s list of `kind`, head first.
pub fn iter(world: &World, player: u8, kind: u16) -> impl Iterator<Item = u16> + '_ {
    let mut cur = world.settler_lists.head(player, kind);
    std::iter::from_fn(move || {
        if cur == 0 {
            return None;
        }
        let id = cur;
        cur = world.entities.by_index(id as u32).map_or(0, |e| e.next);
        Some(id)
    })
}

/// `CSettlerMgr::AttachSettler` (0x582b40).
pub fn attach(world: &mut World, settler: u16) {
    let Some(e) = world.entities.by_index_mut(settler as u32) else { return };
    e.flags = (e.flags & !flag::OFFERED) | flag::GLOBAL_OFFERED;
    let Some(i) = slot(e.owner, e.kind) else { return };
    let old = world.settler_lists.heads[i];
    world.settler_lists.counts[i] += 1;
    world.settler_lists.heads[i] = settler;
    e.prev = 0;
    e.next = old;
    if old != 0
        && let Some(o) = world.entities.by_index_mut(old as u32).filter(|o| o.obj_type == ObjType::Settler)
    {
        o.prev = settler;
    }
}

/// `CSettlerMgr::DetachSettler` (0x582bb0).
pub fn detach(world: &mut World, settler: u16) {
    let Some(e) = world.entities.by_index_mut(settler as u32) else { return };
    e.flags &= !(flag::OFFERED | flag::GLOBAL_OFFERED);
    let (prev, next) = (e.prev, e.next);
    e.prev = 0;
    e.next = 0;
    let Some(i) = slot(e.owner, e.kind) else { return };
    if world.settler_lists.counts[i] > 0 {
        world.settler_lists.counts[i] -= 1;
    }
    if prev == 0 {
        world.settler_lists.heads[i] = next;
    } else if let Some(p) = world.entities.by_index_mut(prev as u32) {
        p.next = next;
    }
    if next != 0
        && let Some(n) = world.entities.by_index_mut(next as u32)
    {
        n.prev = prev;
    }
}
