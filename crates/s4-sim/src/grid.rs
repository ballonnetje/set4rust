//! The node grid (`CWorldManager`, World.cpp) and its per-node layers.
//!
//! The original keeps every layer as a static array of `size * size` elements indexed by
//! `x + y * size` (`CWorldManager::Index`); maps are square, `0 < size <= 1024`
//! (`CWorldManager::Construct`). Here each layer is one `Vec` in [`Layers`].

use crate::hash::StateHasher;

/// Map geometry. Square, side `size` (`CWorldManager::m_iWidthHeight`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Grid {
    size: u32,
}

impl Grid {
    /// `CWorldManager::Construct` asserts `0 < size <= 1024`.
    pub fn new(size: u32) -> Option<Grid> {
        (1..=1024).contains(&size).then_some(Grid { size })
    }

    pub fn size(self) -> u32 {
        self.size
    }

    /// `m_iWorldIdxMax`.
    pub fn len(self) -> usize {
        self.size as usize * self.size as usize
    }

    pub fn is_empty(self) -> bool {
        false
    }

    /// `CWorldManager::Index`: `x + y * size`. Caller guarantees [`Grid::in_world`].
    pub fn index(self, x: u32, y: u32) -> usize {
        x as usize + y as usize * self.size as usize
    }

    /// `CWorldManager::X`.
    pub fn x(self, i: usize) -> u32 {
        (i % self.size as usize) as u32
    }

    /// `CWorldManager::Y`.
    pub fn y(self, i: usize) -> u32 {
        (i / self.size as usize) as u32
    }

    /// `CWorldManager::InWorld`: unsigned compare, so negative coordinates are outside.
    pub fn in_world(self, x: i32, y: i32) -> bool {
        (x as u32) < self.size && (y as u32) < self.size
    }

    /// `InInnerWorld1/2/16`: at least `border` nodes away from every edge.
    pub fn in_inner_world(self, x: i32, y: i32, border: i32) -> bool {
        let s = self.size as i32;
        x >= border && y >= border && x < s - border && y < s - border
    }

    /// Index of `(x, y)` if it lies on the map.
    pub fn checked_index(self, x: i32, y: i32) -> Option<usize> {
        self.in_world(x, y).then(|| self.index(x as u32, y as u32))
    }
}

/// A position packed as `y << 16 | x` (`Y16X16`, globals.cpp): `PackXYFast(x, y) = x + (y << 16)`,
/// `UnpackXFast` = low 16 bits (zero-extended), `UnpackYFast` = arithmetic `>> 16`.
/// Packing is plain wrapping addition, so a negative `x` borrows from `y` as in the original.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Y16X16(pub u32);

impl Y16X16 {
    pub fn pack(x: i32, y: i32) -> Y16X16 {
        Y16X16(x.wrapping_add(y.wrapping_shl(16)) as u32)
    }
    pub fn x(self) -> i32 {
        (self.0 & 0xFFFF) as i32
    }
    pub fn y(self) -> i32 {
        (self.0 as i32) >> 16
    }
}

/// Node flag bits (`m_cFlagMap`). Only the observed ones are named.
pub mod flag {
    /// `FLAG_BLOCKED_LAND`; set/cleared only through `Set/ClearBlockedLandEx`.
    pub const BLOCKED_LAND: u8 = 0x01;
    /// Set by `CWorldManager::SetPileId` for a non-zero pile id (and by chunk 6 for blocked nodes).
    pub const BLOCKED_OBJECT: u8 = 0x02;
    /// From chunk 13 gradient bit 0x40 (`LoadGfxData`). Meaning not researched.
    pub const GRADIENT_40: u8 = 0x04;
    /// Set by `CBuildingMgr::CheckForBuild` on nodes a building site will level (inferred).
    pub const LEVELING: u8 = 0x10;
}

/// Every per-node layer of `CWorldManager`, one `Vec` per field (structure of arrays).
///
/// Ownership is not a node layer: as in the original it lives in the tiling (`m_cNormalTileIdMap`,
/// `OwnerId(i) = Tile(NormalTileId(i)).OwnerId()`), see [`crate::World::owner_of`].
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Layers {
    /// `T_GFX_MAP_ELEMENT` +0 (`GroundHeight`).
    pub height: Vec<u8>,
    /// +1 (`Ground`; `IsWater` = `<= 8`).
    pub ground: Vec<u8>,
    /// +2 (`ShadingGradient` = low nibble).
    pub gradient: Vec<u8>,
    /// +3 (fog bits 0-2 old, 3-5 new).
    pub gfx_bits: Vec<u8>,
    /// `m_cFlagMap`, see [`flag`].
    pub flags: Vec<u8>,
    /// `m_cMapObjectMap`: entity index of the settler/building/vehicle on the node (0 = none).
    pub map_object: Vec<u16>,
    /// `m_cDecoObjectMap`: entity index of the deco object or pile on the node (0 = none).
    /// Shared static deco instances make several nodes hold the same index.
    pub deco_object: Vec<u16>,
    /// `m_cResourceMap`: high nibble type, low nibble amount.
    pub resource: Vec<u8>,
    /// `m_cWalkCount5MoveCost3Map`: bits 0-2 move cost, bits 3-7 walk count.
    pub walk_move: Vec<u8>,
}

impl Layers {
    pub fn new(grid: Grid) -> Layers {
        let n = grid.len();
        Layers {
            height: vec![0; n],
            ground: vec![0; n],
            gradient: vec![0; n],
            gfx_bits: vec![0; n],
            flags: vec![0; n],
            map_object: vec![0; n],
            deco_object: vec![0; n],
            resource: vec![0; n],
            walk_move: vec![0; n],
        }
    }

    /// `RefreshShading` (release 0x66d7b0, on the map elements the gfx engine shares with the
    /// world manager: `SetWorld(size, 0x15630d4)` at 0x514b90): the shading nibble of node `n`
    /// and of the nodes above and below (`n ∓ size`), `((h(n − size) − h(n + size)) >> 1) + 8`
    /// (arithmetic shift, low 4 bits; the node's own height where the row does not exist), and
    /// bit 0x40 of byte 3 on each (a redraw mark for the renderer; no simulation reader found).
    pub fn refresh_shading(&mut self, n: usize, size: usize) {
        let total = self.height.len();
        let mut one = |n: usize| {
            let h = self.height[n] as i32;
            let up = if n >= size { self.height[n - size] as i32 } else { h };
            let down = if n + size < total { self.height[n + size] as i32 } else { h };
            let shade = (((up - down) >> 1) + 8) as u8 & 0x0F;
            self.gradient[n] = (self.gradient[n] & 0xF0) | shade;
            self.gfx_bits[n] |= 0x40;
        };
        if n >= total {
            return;
        }
        one(n);
        if n >= size {
            one(n - size);
        }
        if n + size < total {
            one(n + size);
        }
    }

    /// `CLandscapeProperties::IsWater`.
    pub fn is_water(&self, i: usize) -> bool {
        self.ground[i] <= 8
    }

    /// `CWorldManager::IsBlockedLand`.
    pub fn is_blocked_land(&self, i: usize) -> bool {
        self.flags[i] & flag::BLOCKED_LAND != 0
    }

    /// `CWorldManager::MoveCostsBits`.
    pub fn move_cost(&self, i: usize) -> u8 {
        self.walk_move[i] & 7
    }

    /// `CWorldManager::SetMoveCostsBits`.
    pub fn set_move_cost(&mut self, i: usize, cost: u8) {
        self.walk_move[i] = (self.walk_move[i] & !7) | (cost & 7);
    }

    /// `CWorldManager::MoveCount`.
    pub fn walk_count(&self, i: usize) -> u8 {
        self.walk_move[i] >> 3
    }

    /// `CWorldManager::SetMoveCount`.
    pub fn set_walk_count(&mut self, i: usize, count: u8) {
        self.walk_move[i] = (self.walk_move[i] & 7) | (count << 3);
    }

    /// `CWorldManager::ResourceType` (`& 0xF0`).
    pub fn resource_type(&self, i: usize) -> u8 {
        self.resource[i] & 0xF0
    }

    /// `CWorldManager::SetResource(i, type, amount)` = `type | amount`.
    pub fn set_resource(&mut self, i: usize, kind: u8, amount: u8) {
        self.resource[i] = kind | amount;
    }

    /// `CWorldManager::SetPileId`: the deco slot takes the pile; a pile blocks the node
    /// (flag 0x02, move cost 7), removing it restores move cost 2.
    pub fn set_pile_id(&mut self, i: usize, id: u16) {
        self.deco_object[i] = id;
        if id > 0 {
            self.set_move_cost(i, 7);
            self.flags[i] |= flag::BLOCKED_OBJECT;
        } else {
            self.set_move_cost(i, 2);
            self.flags[i] &= !flag::BLOCKED_OBJECT;
        }
    }

    /// `CWorldManager::IsPositionFreeForSettler`: not blocked land and no map object.
    pub fn is_free_for_settler(&self, i: usize) -> bool {
        !self.is_blocked_land(i) && self.map_object[i] == 0
    }

    pub(crate) fn hash(&self, h: &mut StateHasher) {
        h.bytes(&self.height);
        h.bytes(&self.ground);
        h.bytes(&self.gradient);
        h.bytes(&self.gfx_bits);
        h.bytes(&self.flags);
        h.u16s(&self.map_object);
        h.u16s(&self.deco_object);
        h.bytes(&self.resource);
        h.bytes(&self.walk_move);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn y16x16_matches_original_arithmetic() {
        let p = Y16X16::pack(5, 7);
        assert_eq!(p.0, 5 + (7 << 16));
        assert_eq!((p.x(), p.y()), (5, 7));
        // x = -1 borrows from y, as `x + (y << 16)` does in the original.
        let q = Y16X16::pack(-1, 7);
        assert_eq!((q.x(), q.y()), (0xFFFF, 6));
    }

    #[test]
    fn grid_bounds() {
        assert!(Grid::new(0).is_none() && Grid::new(1025).is_none());
        let g = Grid::new(256).unwrap();
        assert!(g.in_world(0, 255) && !g.in_world(-1, 0) && !g.in_world(256, 0));
        assert_eq!(g.index(3, 2), 3 + 2 * 256);
        assert_eq!((g.x(515), g.y(515)), (3, 2));
        assert!(g.in_inner_world(16, 16, 16) && !g.in_inner_world(15, 16, 16));
    }
}
