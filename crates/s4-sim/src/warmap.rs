//! `CWarMap`: where the warriors are (release 0x5bb260 init, 0x5bb330 `AddEntity`, 0x5bb440
//! `RemoveEntity`, 0x5bb560 re-add, 0x5bb5a0 `NotifyMove`; own reverse engineering of the
//! shipped exe).
//!
//! The map is cut into 16×16 cells (64×64 for the largest map). Each cell keeps five intrusive
//! lists of entities by category (`IEntity +0x1c` previous / `+0x1e` next, heads and counts at
//! .bss 0x142f290): 0 settlers, 1 ships and land vehicles, 2 buildings, 3 animals (0x88 | 0x86),
//! 4 piles and flying objects (0x50); decos are never added. New entries go to the head. The
//! position an entity was registered with is kept (0x15b45b8).
//!
//! An entity with a warrior type also adds its **strength** to the cell grid 0x12dd498 (nine u16
//! per cell: the total and one per team; one margin row above and below, the memset at 0x5bb260
//! covers them): settlers by type (.rdata 0x101c990: levels 1/2/3 = 2/3/4, the squad leader 7,
//! specialists, donkeys and the door settler 1), vehicles by type (0x101ca80: 1), flying objects 1.
//! `0x5afcc0` adds a strength `s` to the cell (×8 when |s| ≥ 2) and `s` to its eight
//! neighbours (columns clipped to the map, rows not — they land in the margin rows).
//!
//! Queries (teams and enemy masks from `World::alliances`, [`crate::alliances`]):
//! * [`foreign_strength`] (0x431380 / behaviour table entry 0x5983f0): total − own team strength
//!   of the node's cell;
//! * [`enemy_near`] (0x5b06d0): a cell of the 5×5 around the node within 20 has more strength
//!   than the own team's;
//! * [`enemy_warriors_near`] (0x5b0a20): an enemy settler of warrior type 2..5 within `r`, looked
//!   for only in cells (of the 5×5 within `r`) whose foreign strength exceeds 15.

use crate::entity::ObjType;
use crate::hash::StateHasher;
use crate::world::World;

/// Cells per side (`MapSize >> 4` of the largest map).
pub const SIDE: usize = 64;
const CATS: usize = 5;
/// `IEntity +0x1c` while the entity is not in the war map.
const OUT: u16 = 0xFFFF;

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct WarMap {
    /// `[cell * 5 + category]`: (head, count).
    lists: Vec<(u16, u16)>,
    /// 0x15b45b8 per entity id: the registered packed position, −1 = none.
    pos: Vec<i32>,
    /// `IEntity +0x1c` / `+0x1e`.
    prev: Vec<u16>,
    next: Vec<u16>,
    /// 0x12dd498 − 0x480: `[(row + 1) * 64 + column][0 total, 1..=8 team]`.
    strength: Vec<[u16; 9]>,
}

impl Default for WarMap {
    fn default() -> Self {
        WarMap {
            lists: vec![(0, 0); SIDE * SIDE * CATS],
            pos: vec![-1; 0x10000],
            prev: vec![OUT; 0x10000],
            next: vec![0; 0x10000],
            strength: vec![[0; 9]; (SIDE + 2) * SIDE],
        }
    }
}

impl WarMap {
    pub(crate) fn hash(&self, h: &mut StateHasher) {
        for &(a, b) in &self.lists {
            h.u16(a);
            h.u16(b);
        }
        for (id, &p) in self.pos.iter().enumerate() {
            if p >= 0 || self.prev[id] != OUT {
                h.u32(id as u32);
                h.u32(p as u32);
                h.u16(self.prev[id]);
                h.u16(self.next[id]);
            }
        }
        for c in &self.strength {
            c.iter().for_each(|&v| h.u16(v));
        }
    }

    /// Whether entity `id` is in a cell list (`IEntity +0x1c != 0xffff`).
    pub fn contains(&self, id: u16) -> bool {
        self.prev[id as usize] != OUT
    }

    /// The entities of one cell list, head first.
    pub fn cell(&self, cx: usize, cy: usize, cat: usize) -> impl Iterator<Item = u16> + '_ {
        let mut cur = self.lists.get((cy * SIDE + cx) * CATS + cat).map_or(0, |l| l.0);
        std::iter::from_fn(move || {
            if cur == 0 {
                return None;
            }
            let id = cur;
            cur = self.next[id as usize];
            Some(id)
        })
    }

    /// Total and team strength of a cell (0 outside).
    pub fn cell_strength(&self, cx: i32, cy: i32) -> [u16; 9] {
        if !(0..SIDE as i32).contains(&cx) || !(-1..=SIDE as i32).contains(&cy) {
            return [0; 9];
        }
        self.strength[((cy + 1) as usize) * SIDE + cx as usize]
    }

    fn add_strength(&mut self, cx: i32, cy: i32, team: usize, v: i16) {
        if !(0..SIDE as i32).contains(&cx) || !(-1..=SIDE as i32).contains(&cy) {
            return;
        }
        let c = &mut self.strength[((cy + 1) as usize) * SIDE + cx as usize];
        c[0] = c[0].wrapping_add(v as u16);
        if let Some(t) = c.get_mut(team) {
            *t = t.wrapping_add(v as u16);
        }
    }

    /// 0x5afcc0 `(cx, cy, player, s)`.
    fn spread(&mut self, cx: i32, cy: i32, team: usize, s: i32) {
        let center = if s.abs() < 2 { s } else { s * 8 };
        self.add_strength(cx, cy, team, center as i16);
        self.add_strength(cx, cy - 1, team, s as i16);
        self.add_strength(cx, cy + 1, team, s as i16);
        if cx > 0 {
            for dy in -1..=1 {
                self.add_strength(cx - 1, cy + dy, team, s as i16);
            }
        }
        if cx < SIDE as i32 - 1 {
            for dy in -1..=1 {
                self.add_strength(cx + 1, cy + dy, team, s as i16);
            }
        }
    }
}

/// .rdata 0x101c990: settler type → strength.
const SETTLER_STRENGTH: [(u16, i32); 29] = [
    (29, 2), (30, 3), (31, 4), (32, 2), (33, 3), (34, 4), (35, 2), (36, 3), (37, 4), (38, 2), (39, 3), (40, 4), (41, 2), (42, 3), (43, 4),
    (44, 7), (45, 1), (46, 1), (47, 1), (48, 1), (49, 1), (50, 1), (52, 1), (54, 1), (60, 1), (61, 2), (62, 3), (63, 4), (69, 1),
];

/// A settler type's strength (.rdata 0x101c990, copied to 0x1443790 by the war map's init).
pub fn settler_strength(kind: u16) -> i32 {
    SETTLER_STRENGTH.iter().find(|&&(k, _)| k == kind).map_or(0, |&(_, s)| s)
}

/// `CAlliances` team of a player (0x12dcd0c, [`crate::alliances`]).
pub fn team(world: &World, player: u8) -> usize {
    world.alliances.team(player)
}

/// Whether `b`'s units are enemies of `a` (0x12dcd54 bit `b − 1`).
pub fn hostile(world: &World, a: u8, b: u8) -> bool {
    world.alliances.hostile(a, b)
}

/// Whether `b`'s units are allies of `a`, `a` included (0x12dcd30 bit `b − 1`; medics heal them).
pub fn allied(world: &World, a: u8, b: u8) -> bool {
    world.alliances.allied(a, b)
}

fn strength_of(world: &World, id: u16) -> i32 {
    let Some(e) = world.entities.by_index(id as u32) else { return 0 };
    if e.flags & 0xF == 0 {
        return 0;
    }
    let t = e.obj_type as u8;
    if t & 1 != 0 {
        settler_strength(e.kind)
    } else if t & 6 != 0 {
        // .rdata 0x101ca80: vehicle types 1..5 → 1.
        if (1..=5).contains(&e.kind) { 1 } else { 0 }
    } else if t & 0x40 != 0 {
        1
    } else {
        0
    }
}

fn category(t: u8) -> usize {
    (if t & 0x88 != 0 { 2 } else { 0 }) | (t & 0x86 != 0) as usize | (if t & 0x50 != 0 { 4 } else { 0 })
}

fn cell_of(pos: i32) -> (i32, i32) {
    ((pos >> 4) & 0xfff, pos >> 20)
}

fn listed(world: &World, id: u16) -> bool {
    world.entities.by_index(id as u32).is_some_and(|e| (e.obj_type as u8) & 0xdf != 0)
}

/// 0x5bb330 `AddEntity(entity, pos)`: at the entity's position.
pub fn add(world: &mut World, id: u16) {
    let Some(pos) = world.entities.by_index(id as u32).map(|e| e.pos.0 as i32) else { return };
    add_at(world, id, pos);
}

fn add_at(world: &mut World, id: u16, pos: i32) {
    if !listed(world, id) {
        return;
    }
    if world.warmap.pos[id as usize] >= 0 && world.warmap.contains(id) {
        remove(world, id);
    }
    world.warmap.pos[id as usize] = pos;
    let (cx, cy) = cell_of(pos);
    let s = strength_of(world, id);
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let (owner, t) = (e.owner, e.obj_type as u8);
    if s != 0 {
        world.warmap.spread(cx, cy, team(world, owner), s);
    }
    let l = (cy as usize * SIDE + cx as usize) * CATS + category(t);
    let Some(&(head, _)) = world.warmap.lists.get(l) else { return };
    world.warmap.prev[id as usize] = 0;
    world.warmap.next[id as usize] = head;
    world.warmap.lists[l].0 = id;
    if head != 0 {
        world.warmap.prev[head as usize] = id;
    }
    world.warmap.lists[l].1 = world.warmap.lists[l].1.wrapping_add(1);
}

/// 0x5bb440 `RemoveEntity(entity)`: from the cell of its registered position.
pub fn remove(world: &mut World, id: u16) {
    if !listed(world, id) {
        return;
    }
    let pos = world.warmap.pos[id as usize];
    if pos < 0 {
        return;
    }
    world.warmap.pos[id as usize] = -1;
    let (cx, cy) = cell_of(pos);
    let s = strength_of(world, id);
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let (owner, t) = (e.owner, e.obj_type as u8);
    if s != 0 {
        world.warmap.spread(cx, cy, team(world, owner), -s);
    }
    let l = (cy as usize * SIDE + cx as usize) * CATS + category(t);
    if let Some(c) = world.warmap.lists.get_mut(l) {
        c.1 = c.1.wrapping_sub(1);
    }
    let (prev, next) = (world.warmap.prev[id as usize], world.warmap.next[id as usize]);
    if prev == 0 {
        if let Some(c) = world.warmap.lists.get_mut(l) {
            c.0 = next;
        }
    } else if prev != OUT {
        world.warmap.next[prev as usize] = next;
    }
    if next != 0 {
        world.warmap.prev[next as usize] = prev;
    }
    world.warmap.prev[id as usize] = OUT;
    world.warmap.next[id as usize] = 0;
}

/// 0x5bb560: out (if in) and in again at the current position.
pub fn readd(world: &mut World, id: u16) {
    if world.warmap.contains(id) {
        remove(world, id);
    }
    add(world, id);
}

/// 0x5bb5a0 `NotifyMove`: an entity in the map that moved into another cell is re-added;
/// otherwise only its registered position changes. Unregistered: added.
pub fn moved(world: &mut World, id: u16) {
    if !listed(world, id) || !world.warmap.contains(id) {
        return;
    }
    let Some(pos) = world.entities.by_index(id as u32).map(|e| e.pos.0 as i32) else { return };
    let old = world.warmap.pos[id as usize];
    if old < 0 {
        add(world, id);
        return;
    }
    if cell_of(old) != cell_of(pos) {
        remove(world, id);
        add(world, id);
        return;
    }
    world.warmap.pos[id as usize] = pos;
}

/// 0x431380: foreign strength (total − own team) in the cell of `(x, y)`.
pub fn foreign_strength(world: &World, x: i32, y: i32, player: u8) -> i32 {
    let c = world.warmap.cell_strength(x >> 4, y >> 4);
    c[0] as i32 - c.get(team(world, player)).copied().unwrap_or(0) as i32
}

/// 0x5b0310 `(x, y, r)`: the cells of the 5×5 around `(x, y)`'s cell (rows outer, −2..2) that
/// lie in the map and whose nearest point is within `r` (`Grid::Distance`).
pub fn cells_within(world: &World, x: i32, y: i32, r: i32) -> Vec<(i32, i32)> {
    let side = (world.grid.size() >> 4) as i32;
    let (cx, cy) = (x >> 4, y >> 4);
    let (px, py) = (x & 0xf, y & 0xf);
    let mut out = Vec::new();
    for dy in -2..=2 {
        for dx in -2..=2 {
            let (nx, ny) = (cx + dx, cy + dy);
            // 0x5b01b0: the columns / rows beyond the map edge.
            if nx < 0 || ny < 0 || nx > side - 1 || ny > side - 1 {
                continue;
            }
            let ix = match dx {
                0 => px,
                d if d < 0 => d * 16 + 15,
                d => d * 16,
            };
            let iy = match dy {
                0 => py,
                d if d < 0 => d * 16 + 15,
                d => d * 16,
            };
            if crate::spiral::grid_distance(ix - px, iy - py) <= r {
                out.push((nx, ny));
            }
        }
    }
    out
}

/// 0x5b06d0 `(x, y, player)`: a cell within 20 with more strength than the player's team has.
pub fn enemy_near(world: &World, x: i32, y: i32, player: u8) -> bool {
    cells_within(world, x, y, 20).into_iter().any(|(cx, cy)| {
        let c = world.warmap.cell_strength(cx, cy);
        c[0] > c.get(team(world, player)).copied().unwrap_or(0)
    })
}

/// 0x5b0a20 `(x, y, r, player)`: an enemy settler of warrior type 2..5 within `r`.
pub fn enemy_warriors_near(world: &World, x: i32, y: i32, r: i32, player: u8) -> bool {
    if r <= 16 && foreign_strength(world, x, y, player) == 0 {
        return false;
    }
    for (cx, cy) in cells_within(world, x, y, r) {
        let c = world.warmap.cell_strength(cx, cy);
        if (c[0] as i32 - c.get(team(world, player)).copied().unwrap_or(0) as i32) <= 15 {
            continue;
        }
        for id in world.warmap.cell(cx as usize, cy as usize, 0) {
            let Some(e) = world.entities.by_index(id as u32) else { continue };
            let wt = e.warrior_type();
            if e.obj_type == ObjType::Settler
                && hostile(world, player, e.owner)
                && (2..=5).contains(&wt)
                && crate::spiral::grid_distance(e.x() - x, e.y() - y) <= r
            {
                return true;
            }
        }
    }
    false
}

/// 0x5b0dd0 `(x, y, r, players, &pos)`: on the sector of `(x, y)` (none: 0), the settlers
/// without a warrior type, READY and VISIBLE, of a player in the bit mask `players` (bit
/// `p − 1`), in the cells within `r` ([`cells_within`]; the distance itself is not checked):
/// their count, and the position of the nearest (hex distance below 0x4000, the first of
/// equals; −1 without one).
pub fn civilians_near(world: &World, x: i32, y: i32, r: i32, players: u32) -> (i32, i32) {
    let sector_at = |x: i32, y: i32| world.grid.checked_index(x, y).map_or(0, |n| world.tiling.sector_of(n));
    let sector = sector_at(x, y);
    let (mut count, mut best, mut pos) = (0, 0x4000, -1);
    if sector == 0 {
        return (0, -1);
    }
    for (cx, cy) in cells_within(world, x, y, r) {
        for id in world.warmap.cell(cx as usize, cy as usize, 0) {
            let Some(e) = world.entities.by_index(id as u32) else { continue };
            if e.flags & 0xf != 0 || e.flags & 0x0200_0100 != 0x0200_0100 || players & crate::alliances::player_bit(e.owner) as u32 == 0 {
                continue;
            }
            if sector_at(e.x(), e.y()) != sector {
                continue;
            }
            count += 1;
            let d = crate::spiral::grid_distance(e.x() - x, e.y() - y);
            if d < best {
                best = d;
                pos = e.pos.0 as i32;
            }
        }
    }
    (count, pos)
}

/// Consistency check for tests: every listed entity sits in the list of its registered
/// position's cell and category, the counts match, and the strength grid equals the sum over the
/// listed entities. Returns a description of the first mismatch.
pub fn check(world: &World) -> Result<(), String> {
    let wm = &world.warmap;
    let mut grid = WarMap::default();
    for cy in 0..SIDE {
        for cx in 0..SIDE {
            for cat in 0..CATS {
                let ids: Vec<u16> = wm.cell(cx, cy, cat).take(0x10000).collect();
                if ids.len() != wm.lists[(cy * SIDE + cx) * CATS + cat].1 as usize {
                    return Err(format!("cell ({cx}, {cy}) category {cat}: {} listed, count {}", ids.len(), wm.lists[(cy * SIDE + cx) * CATS + cat].1));
                }
                for id in ids {
                    let p = wm.pos[id as usize];
                    if cell_of(p) != (cx as i32, cy as i32) {
                        return Err(format!("entity {id} listed in ({cx}, {cy}) but registered at {p:#x}"));
                    }
                    let Some(e) = world.entities.by_index(id as u32) else { return Err(format!("listed entity {id} does not exist")) };
                    if category(e.obj_type as u8) != cat {
                        return Err(format!("entity {id} in category {cat}"));
                    }
                    let s = strength_of(world, id);
                    if s != 0 {
                        grid.spread(cx as i32, cy as i32, team(world, e.owner), s);
                    }
                }
            }
        }
    }
    if grid.strength != wm.strength {
        let i = grid.strength.iter().zip(&wm.strength).position(|(a, b)| a != b).unwrap_or(0);
        return Err(format!("strength of cell {} (row {}): recomputed {:?}, kept {:?}", i % SIDE, i / SIDE, grid.strength[i], wm.strength[i]));
    }
    Ok(())
}
