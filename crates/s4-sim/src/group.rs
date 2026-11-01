//! `CGroupMgr` (vtable 0x105a308, global 0x153fd30): the players' unit groups (command 5041,
//! `DefineGroup`) and sending selected units somewhere (command 5014, the right-click move). Own
//! reverse engineering of the shipped exe (release VAs).
//!
//! **Groups** (reset vtbl+8 0x56a1a0, store 0x56ab10 / load 0x56aa40): 9 players × 11 groups,
//! each a flag byte (+4 + 11·player + group; bit 0: a squad leader is in it) and a list of up to
//! 100 entity ids (+0x68 + 0xcc·(11·player + group): count, then u16 ids). An entity keeps its
//! memberships in the group word of `ISelectableSettlerRole` (+0x2c; entity vtbl +0x54 get /
//! +0x58 set / +0x5c or / +0x60 clear bits = role vtbl +0x5c..+0x68, 0x57ffb0 / 0x57ffc0 /
//! 0x57ffe0 / 0x580000): bit `g` for group `g`. A squad leader (settler type 0x2c) holds
//! `g << 12 | 1 << g`; the members of its group at the time it joins get bit 0x800, the
//! critical-hit bonus of [`crate::jobs::warrior::hit`], and lose it when it leaves. Roles
//! without the word (carriers, workers, tower soldiers, …: base 0x572d30 returns 0) can not
//! join.

use crate::World;
use crate::entity::{EntityData, ObjType};
use crate::grid::Y16X16;
use crate::hash::StateHasher;
use crate::jobs::state::{EntityEvent, role};

const PLAYERS: usize = 9;
/// Groups per player.
pub const GROUPS: usize = 11;
/// Entities per group. [deviation] The release keeps at most 100 (+0x68 lists of 0xcc bytes);
/// s4rs selects up to 1000 units (the user's request, s4-viewer `selection::MAX`) and a group
/// takes them all.
pub const MEMBERS: usize = 1000;
/// The release's group / formation size.
const RELEASE_MEMBERS: usize = 100;
/// The squad leader's settler type (`(entity +0xa & 1) & (type == 0x2c)` in 0x56a230 / 0x56a340).
const SQUAD_LEADER: u16 = 0x2c;
/// The group-word bit of the members of a group with a squad leader.
pub const LEADER_BONUS: u16 = 0x800;

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Groups {
    /// +4: flag byte per player and group (bit 0: has a squad leader).
    flags: Vec<u8>,
    /// +0x68: the member lists.
    members: Vec<Vec<u16>>,
}

impl Default for Groups {
    fn default() -> Self {
        Groups { flags: vec![0; PLAYERS * GROUPS], members: vec![Vec::new(); PLAYERS * GROUPS] }
    }
}

fn slot(player: u8, group: usize) -> Option<usize> {
    ((player as usize) < PLAYERS && group < GROUPS).then(|| player as usize * GROUPS + group)
}

impl Groups {
    /// vtbl+0xc (0x56a1f0): the number of members.
    pub fn count(&self, player: u8, group: usize) -> usize {
        slot(player, group).map_or(0, |k| self.members[k].len())
    }

    /// vtbl+0x10 (0x56a210): the members in list order.
    pub fn members(&self, player: u8, group: usize) -> &[u16] {
        slot(player, group).map_or(&[], |k| &self.members[k])
    }

    /// vtbl+0x24 (0x56a4f0): a squad leader is in the group.
    pub fn has_leader(&self, player: u8, group: usize) -> bool {
        slot(player, group).is_some_and(|k| self.flags[k] & 1 != 0)
    }

    pub(crate) fn hash(&self, h: &mut StateHasher) {
        for (f, m) in self.flags.iter().zip(&self.members) {
            h.u8(*f);
            h.u32(m.len() as u32);
            h.u16s(m);
        }
    }
}

/// The roles derived from `ISelectableSettlerRole` (the vtables holding 0x57ffb0 at +0x5c):
/// soldier 7, priest 9, saboteur 10, pioneer 11, geologist 12, gardener 13, shaman 16, thief 19,
/// squad leader 23. A settler whose role is not ported yet counts by its settler info role.
fn has_group_word(world: &World, id: u16) -> bool {
    let Some(e) = world.entities.by_index(id as u32) else { return false };
    let EntityData::Settler(s) = &e.data else { return false };
    let r = if s.job.role != role::NONE { s.job.role } else { world.rules.settler_role(e.race, e.kind) };
    matches!(r, 7 | 9..=13 | 16 | 19 | 23)
}

/// Entity vtbl+0x54: the group word (0 for entities without one).
pub fn word(world: &World, id: u16) -> u16 {
    match world.entities.by_index(id as u32).map(|e| &e.data) {
        Some(EntityData::Settler(s)) if has_group_word(world, id) => s.job.groups,
        _ => 0,
    }
}

/// Entity vtbl+0x58 / +0x5c / +0x60 (set, or, clear bits): the new word (0 without one).
fn update_word(world: &mut World, id: u16, f: impl FnOnce(u16) -> u16) -> u16 {
    if !has_group_word(world, id) {
        return 0;
    }
    match world.entities.by_index_mut(id as u32).map(|e| &mut e.data) {
        Some(EntityData::Settler(s)) => {
            s.job.groups = f(s.job.groups);
            s.job.groups
        }
        _ => 0,
    }
}

fn is_leader(world: &World, id: u16) -> bool {
    world.entities.by_index(id as u32).is_some_and(|e| e.obj_type == ObjType::Settler && e.kind == SQUAD_LEADER)
}

/// vtbl+0x14 (0x56a230) `Add(player, group, id)`: the player's entity joins unless it is a
/// member already or the group is full. A squad leader joins only a group without one and only
/// while in no group at all: its word becomes `group << 12 | bit`, every member so far gets
/// [`LEADER_BONUS`] (members joining later do not) and the group is flagged. Anyone else ORs
/// in the bit (fails without a group word).
pub fn add(world: &mut World, player: u8, group: usize, id: u16) -> bool {
    let Some(k) = slot(player, group) else { return false };
    if world.entities.by_index(id as u32).is_none_or(|e| e.owner != player) {
        return false;
    }
    let bit = 1u16 << group;
    let cur = word(world, id);
    if cur & bit != 0 || world.groups.members[k].len() >= MEMBERS {
        return false;
    }
    if is_leader(world, id) {
        if world.groups.flags[k] & 1 != 0 || cur != 0 {
            return false;
        }
        update_word(world, id, |_| (group as u16) << 12 | bit);
        for m in world.groups.members[k].clone() {
            update_word(world, m, |w| w | LEADER_BONUS);
        }
        world.groups.flags[k] |= 1;
    } else if update_word(world, id, |w| w | bit) & bit == 0 {
        return false;
    }
    world.groups.members[k].push(id);
    true
}

/// vtbl+0x18 (0x56a340) `Remove(player, group, id)`: for a member of the player: out of the
/// list (0x56ac80: the last id takes its place); a squad leader's word becomes 0, the group
/// loses its flag and every remaining member [`LEADER_BONUS`]; anyone else clears only its
/// bit (a member that leaves keeps 0x800).
pub fn remove(world: &mut World, player: u8, group: usize, id: u16) -> bool {
    let Some(k) = slot(player, group) else { return false };
    if world.entities.by_index(id as u32).is_none_or(|e| e.owner != player) {
        return false;
    }
    let bit = 1u16 << group;
    if word(world, id) & bit == 0 {
        return false;
    }
    let list = &mut world.groups.members[k];
    if let Some(at) = list.iter().position(|&m| m == id) {
        list.swap_remove(at);
    }
    if !is_leader(world, id) {
        update_word(world, id, |w| w & !bit);
        return true;
    }
    update_word(world, id, |_| 0);
    world.groups.flags[k] &= !1;
    for m in world.groups.members[k].clone() {
        update_word(world, m, |w| w & !LEADER_BONUS);
    }
    true
}

/// vtbl+0x1c (0x56a430) `DetachEntityFromAllGroups(id)`: `Remove` from groups 1..=10 of the
/// entity's owner (never group 0), then the word is set to 0. Called by `Kill` (0x56d030, when
/// the word is not 0), `CSettler::SetRole` (0x580c20), the military buildings' soldier orders
/// (0x582ca0, 0x582ea0) and specialists' / vehicles' boarding (not ported).
pub fn remove_all(world: &mut World, id: u16) {
    let Some(owner) = world.entities.by_index(id as u32).map(|e| e.owner) else { return };
    let w = word(world, id);
    for g in 1..GROUPS {
        if w & (1 << g) != 0 {
            remove(world, owner, g, id);
        }
    }
    update_word(world, id, |_| 0);
}

/// vtbl+0x20 (0x56a4a0) `Clear(player, group)`: `Remove` from the last member on; a member that
/// is not removed (another owner, bit not set) is dropped from the count anyway.
pub fn clear(world: &mut World, player: u8, group: usize) {
    let Some(k) = slot(player, group) else { return };
    while let Some(&last) = world.groups.members[k].last() {
        let n = world.groups.members[k].len();
        remove(world, player, group, last);
        if world.groups.members[k].len() >= n {
            world.groups.members[k].truncate(n - 1);
        }
    }
}

/// Command 5041 (`CNetInputProcessor::Process` 0x4f6240 case 0x13b1): with a non-empty id list
/// the player's group is cleared, then every READY entity of the player leaves all its groups
/// and joins this one, in list order. The original does not range-check the group; here groups
/// beyond 10 are ignored. An empty list is taken to be the original's null data (nothing
/// happens; inferred).
pub fn define(world: &mut World, player: u8, group: usize, ids: &[u16]) {
    if ids.is_empty() || group >= GROUPS {
        return;
    }
    clear(world, player, group);
    for &id in ids {
        let ok = world.entities.by_index(id as u32).is_some_and(|e| e.flags & crate::entity::flag::READY != 0 && e.owner == player);
        if ok {
            remove_all(world, id);
            add(world, player, group, id);
        }
    }
}

/// The target positions 0x56a000 fills: `positions[0]` is the target, then `positions[1..]` the
/// further places of a group (at most 99).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Formation {
    pub positions: Vec<Y16X16>,
}

/// The sector of a node: the normal tile (0x569f90, tile ids 0x15630c4), or with flag 2 the
/// second tile-id map (0x569fc0, 0x12f1a1c, ids ≥ 0x8000 count as none; taken to be the catapult
/// tiling — inferred from the layout, not verified).
fn sector(world: &World, x: i32, y: i32, flags: u32) -> u16 {
    let Some(n) = world.grid.checked_index(x, y) else { return 0 };
    if flags & 2 == 0 {
        return world.tiling.sector_of(n);
    }
    let id = world.tiling.catapult[n];
    let id = if id < 0x8000 { id } else { 0 };
    world.tiling.tiles.get(id as usize).map_or(0, |t| t.sector)
}

/// 0x56a000 `(x, y, n, flags, from)`. The target is `(x, y)`; when its sector is not that of
/// `from` (and `from` lies in a sector), with flag 1 the first node of the spiral `0..=Last(15)`
/// around it in `from`'s sector becomes the target (none: the target stays, no further places).
/// For `n > 1` the further places are the nodes `target + 2·(spiral entry k)`, k = 1..=100, in
/// the target's sector (entries 1..=99, up to .data 0x1169af8), until `min(n, 100) − 1` are
/// found. [deviation] For more than 100 units (s4rs selects up to 1000) the spiral goes on past
/// entry 99 until each has its place.
pub fn formation(world: &World, x: i32, y: i32, n: usize, flags: u32, from: i32) -> Formation {
    let mut f = Formation { positions: vec![Y16X16::pack(x, y)] };
    let (mut x, mut y) = (x, y);
    let mut target_sector = sector(world, x, y, flags);
    let from_sector = if from > 0 {
        let p = Y16X16(from as u32);
        sector(world, p.x(), p.y(), flags) as i32
    } else {
        0
    };
    if target_sector < 1 || target_sector as i32 != from_sector {
        if from_sector < 1 || flags & 1 == 0 {
            return f;
        }
        let Some((nx, ny)) = crate::spiral::walk(world.grid, x, y, 15).find(|&(px, py)| sector(world, px, py, flags) as i32 == from_sector) else {
            return f;
        };
        (x, y) = (nx, ny);
        target_sector = from_sector as u16;
        f.positions[0] = Y16X16::pack(x, y);
    }
    let n = n.min(MEMBERS);
    let entries = if n <= RELEASE_MEMBERS { RELEASE_MEMBERS } else { crate::spiral::LEN };
    if n > 1 {
        for k in 1..entries {
            if f.positions.len() >= n {
                break;
            }
            let (px, py) = (x + 2 * crate::spiral::dx(k), y + 2 * crate::spiral::dy(k));
            if world.grid.in_world(px, py) && sector(world, px, py, flags) == target_sector {
                f.positions.push(Y16X16::pack(px, py));
            }
        }
    }
    f
}

/// vtbl+0x28 (0x56a800) `SendTo(count, ids, x, y, mode)`, command 5014. From the first entity
/// that is READY and not flagged 0x8000 on, every such entity gets event `{0x11, 0xd, mode,
/// place, (x, y)}`, the places of [`formation`] (for all `count` ids, flag 3 when the first one
/// is a land vehicle, else 1, from its position) taken in turn and repeated. With mode bit 3 the
/// event carries no positions. The net handler (0x4f6240 case 0x1396) checks no owner.
pub fn send_to(world: &mut World, ids: &[u16], x: i32, y: i32, mode: u32) {
    const READY_NOT_8000: u32 = 0x200_8000;
    let ok = |w: &World, id: u16| w.entities.by_index(id as u32).is_some_and(|e| e.flags & READY_NOT_8000 == 0x200_0000);
    let Some(first) = ids.iter().position(|&id| ok(world, id)) else { return };
    let click = Y16X16::pack(x, y).0 as i32;
    let f = (mode & 8 == 0).then(|| {
        let e = world.entities.by_index(ids[first] as u32).expect("checked");
        let flags = if e.obj_type == ObjType::LandVehicle { 3 } else { 1 };
        formation(world, x, y, ids.len(), flags, e.pos.0 as i32)
    });
    let mut k = 0;
    for &id in &ids[first..] {
        if !ok(world, id) {
            continue;
        }
        let ev = match &f {
            Some(f) => {
                let place = f.positions[k].0 as i32;
                k = if k + 1 >= f.positions.len() { 0 } else { k + 1 };
                EntityEvent { event: 0x11, kind: 0xd, data: [mode as i32, place, click] }
            }
            None => EntityEvent { event: 0x11, kind: 0xd, data: [mode as i32, 0, 0] },
        };
        // Entity vtbl+0x50: settlers queue the event.
        match world.entities.by_index(id as u32).map(|e| e.obj_type) {
            Some(ObjType::Settler) => crate::jobs::core::set_event(world, id, ev),
            Some(ObjType::LandVehicle | ObjType::Ship) => crate::vehicles::logic::set_event(world, id, ev),
            _ => world.events.push(crate::event::SimEvent::Unported { what: "SendTo an animal", entity: id }),
        }
    }
}
