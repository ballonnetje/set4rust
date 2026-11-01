//! Path wear (`CWorldManager` 0x60ca10 / 0x60cad0) and `CLogic`'s world step (0x4f2d00). Own
//! reverse engineering of the shipped exe (release VAs).
//!
//! * **Walk count** (bits 3..7 of `m_cWalkCount5MoveCost3Map`, [`crate::grid::Layers::walk_count`]):
//!   only on grass-family ground (`ground & 0xf0 == 0x10`). A settler's step onto a node adds
//!   one (0x60ca10, `IncreaseWalkCount`, from `ISettlerRole::Go`); the world step takes one off
//!   (0x60cad0). Reaching 10 sets move cost 1 and, if every neighbour may border it
//!   (`CanChangeGround` 0x66d240), ground 0x1c (path). Counting up to 15 sets move cost 0 and
//!   ground 0x1d (road); counting down to 6 sets move cost 2 and ground 0x10 (grass). The count
//!   is 5 bits: above 31 it wraps to 0 (`(count + 1) · 8` in a byte). The changed node then gets
//!   0x65a530: its shading refreshed (0x66d7b0, shared map elements); the row marks are the
//!   client's.
//! * **World step** (0x4f2d00, from `Logic::Update` 0x4f2720 after the archive trigger): every
//!   tick `size² / 2000` steps of a cursor (`CLogic +0x28 / +0x2c`, 0 at game start, ctor
//!   0x4f2480) that advances 13 nodes, wrapping into the next row and back to the top. On each
//!   node: the walk count decays; fish (resource type 0, amount > 0) move: one game-RNG draw
//!   `(r & 0xff) % 6` picks a neighbour, and a water neighbour (ground < 9) that has fish already
//!   gets one more while the node has one less; then one draw `r`: below 0x11eb a static deco of
//!   type 193..=208 leaves the node (only the node's deco slot is cleared), below 0x1eb8 the
//!   node's plant ages (0x564430 `(node, 0)`, [`crate::magic::spells::plant_step`]).

use crate::hash::StateHasher;
use crate::world::World;

/// `CLogic +0x28 / +0x2c`: the world step's cursor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct WorldStep {
    pub x: i32,
    pub y: i32,
}

impl WorldStep {
    pub(crate) fn hash(&self, h: &mut StateHasher) {
        h.u32(self.x as u32);
        h.u32(self.y as u32);
    }
}

const PATH: u8 = 0x1c;
const ROAD: u8 = 0x1d;
const GRASS: u8 = 0x10;

fn grass_family(world: &World, n: usize) -> bool {
    world.layers.ground[n] & 0xf0 == 0x10
}

/// 0x65b0f0 + the ground write: the node becomes `ground` when its neighbours allow it.
fn change_ground(world: &mut World, n: usize, ground: u8) {
    let size = world.grid.size() as i32;
    let (x, y) = (n as i32 % size, n as i32 / size);
    if crate::build::site::can_change_ground(world, x, y, ground) {
        world.layers.ground[n] = ground;
        // 0x65a530: the shading (0x66d7b0) and the renderer's row marks (client).
        let size = world.grid.size() as usize;
        world.layers.refresh_shading(n, size);
    }
}

/// `CWorldManager::IncreaseWalkCount(node)` (0x60ca10): a settler stepped onto `n`.
pub fn increase_walk_count(world: &mut World, n: usize) {
    if n >= world.layers.ground.len() || !grass_family(world, n) {
        return;
    }
    let b = world.layers.walk_move[n];
    // `bVar1 < 0xf8` always holds for a 5-bit count: it wraps from 31 to 0.
    world.layers.walk_move[n] = ((b >> 3).wrapping_add(1) << 3) | (b & 7);
    if world.layers.walk_move[n] & 0xf8 == 0x50 {
        world.layers.walk_move[n] = (world.layers.walk_move[n] & 0xf9) | 1;
        change_ground(world, n, PATH);
    }
    if world.layers.walk_move[n] & 0xf8 == 0x78 {
        world.layers.walk_move[n] &= 0xf8;
        change_ground(world, n, ROAD);
    }
}

/// 0x60cad0 `(node)`: the walk count decays by one.
pub fn decrease_walk_count(world: &mut World, n: usize) {
    if !grass_family(world, n) {
        return;
    }
    let b = world.layers.walk_move[n];
    if b >> 3 == 0 {
        return;
    }
    world.layers.walk_move[n] = (((b >> 3) - 1) << 3) | (b & 7);
    if world.layers.walk_move[n] & 0xf8 == 0x50 {
        world.layers.walk_move[n] = (world.layers.walk_move[n] & 0xf9) | 1;
        change_ground(world, n, PATH);
    }
    if world.layers.walk_move[n] & 0xf8 == 0x30 {
        world.layers.walk_move[n] = (world.layers.walk_move[n] & 0xfa) | 2;
        change_ground(world, n, GRASS);
    }
}

/// 0x4f2d00: `size² / 2000` steps of the cursor (see the module doc).
pub fn world_step(world: &mut World) {
    let size = world.grid.size() as i32;
    for _ in 0..size * size / 2000 {
        let c = &mut world.world_step;
        c.x += 13;
        if c.x >= size {
            c.y += 1;
            c.x -= size;
            if c.y >= size {
                c.y -= size;
            }
        }
        let (x, y) = (c.x, c.y);
        let n = (y * size + x) as usize;
        decrease_walk_count(world, n);
        let r = world.layers.resource[n];
        if r & 0xf0 == 0 && r & 0xf != 0 {
            let d = (world.rng.rand() & 0xff) % 6;
            let nx = x + matches!(d, 0 | 1) as i32 - matches!(d, 3 | 4) as i32;
            let ny = y + matches!(d, 1 | 2) as i32 - matches!(d, 4 | 5) as i32;
            // Unsigned bounds (a −1 fails), then a water neighbour with fish.
            if (nx as u32) < size as u32 && (ny as u32) < size as u32 {
                let m = (ny * size + nx) as usize;
                let nr = world.layers.resource[m];
                if world.layers.ground[m] < 9 && nr & 0xf0 == 0 && nr & 0xf != 0 {
                    // Byte arithmetic as in the release (15 + 1 becomes type 1, amount 0).
                    world.layers.resource[m] = nr.wrapping_add(1);
                    world.layers.resource[n] = (r & 0xf).wrapping_sub(1);
                }
            }
        }
        let r = world.rng.rand();
        if r < 0x11eb {
            let id = world.layers.deco_object[n];
            if world.entities.by_index(id as u32).is_some_and(|e| e.obj_type == crate::entity::ObjType::Deco && (193..=208).contains(&e.kind)) {
                world.layers.deco_object[n] = 0;
            }
        }
        if r < 0x1eb8 {
            crate::magic::spells::plant_step(world, x, y, false);
        }
    }
}
