//! Fish. In the original they are not entities: the fish a fisher catches are the resource layer's
//! fish amounts (resource type 0, `SearchFish`), and the fish one sees are jumping-fish effects
//! that `CAnimalMgr::Update` (0x52edf0) starts near the screen centre on every third update
//! (0x5301c0). That pass reads the local camera and draws the manager's own RNG, so it is client
//! side; [`jump`] is its decision, for the client to call.
//!
//! 0x5301c0, with the screen centre's map node `(x, y)`:
//! * two RNG draws pick a cell around the centre's cell (`(x >> 4) + (r & 3) − 2`, likewise y);
//!   it must be on the map and have fish (the AI cell grid's fish sum, built at game start);
//! * its 256 nodes are walked row by row, one RNG draw each, until one holds more fish than
//!   `FISH_MIN` (Animals.cfg, default 2) and is not dark land;
//! * there an animal effect starts (`CEffects` vtbl+0x20, 0x4e3c10 → 0x4e2780): gfx file 8, job
//!   [`FISH_JOB`], direction = that node's draw % 6, lasting the job's frame
//!   count (direction 2) from now; drawn (0x4e2b20) with frame `age % frames` at the node.

use crate::rng::Random16;
use crate::world::World;

/// The first gfx job of `ANIMAL_FISH` (type 15) in `Gfx\8.gfx`: `ANIMAL_FISH_SWIM`. [inf] The
/// release reads it from its first-job-per-animal-type table (GUIElementEngine +0x77c, entry 15);
/// the job defines are in type order and fish has only this job.
pub const FISH_JOB: u16 = 36;

/// One jumping fish: node and direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Jump {
    pub x: i32,
    pub y: i32,
    pub dir: u8,
}

/// 0x5301c0 for the screen-centre node `(x, y)` with the client's copy of the manager's RNG
/// (seeded like [`World::animal_rng`]). Call it on the updates where `world.animals.ticks % 3 == 0`
/// (after the tick, the counter as `CAnimalMgr::Update` left it).
pub fn jump(world: &World, rng: &mut Random16, x: i32, y: i32) -> Option<Jump> {
    let m = &world.animals;
    let cx = (x >> 4) + (rng.rand() & 3) as i32 - 2;
    let cy = (y >> 4) + (rng.rand() & 3) as i32 - 2;
    if cx < 0 || cy < 0 || cx >= m.cells_w as i32 || cy >= m.cells_h as i32 || m.cells.get(cx, cy).resources[0] == 0 {
        return None;
    }
    let fish_min = world.rules.animals.fish_min;
    for row in 0..16 {
        for col in 0..16 {
            let r16 = rng.rand();
            let (nx, ny) = (cx * 16 + col, cy * 16 + row);
            let n = world.grid.checked_index(nx, ny)?;
            let r = world.layers.resource[n];
            let fish = if r & 0xf0 == 0 { (r & 0xf) as i32 } else { 0 };
            if fish > fish_min && world.layers.flags[n] & 0x04 == 0 {
                return Some(Jump { x: nx, y: ny, dir: (r16 % 6) as u8 });
            }
        }
    }
    None
}
