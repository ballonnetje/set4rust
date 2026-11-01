//! The local player's fog of war, as the renderer reads it. The simulation owns the fog
//! (`s4_sim::fog`, `CFogging`, updated inside the tick at `CTiling::Update`'s position); this is
//! the per-tick copy that travels in the snapshot.

use std::sync::Arc;

use s4_sim::World;

/// What the renderer needs of the fog after a tick.
#[derive(Clone)]
pub struct FogFrame {
    /// Byte 3 fog bits per node (bits 0..2 old level, 3..5 new level).
    pub gfx: Arc<Vec<u8>>,
    /// `g_iFogFadeStep`.
    pub fade: u32,
    /// `g_iFadeYBorder`.
    pub border: i32,
    /// What `CFogging::IsPositionVisible` reads besides the gfx bits.
    pub vis: s4_sim::fog::Visibility,
}

impl FogFrame {
    /// `CFogging::IsPositionVisible(x, y)` (release 0x5aee70) after the tick; `size` is the map
    /// side in nodes.
    pub fn is_position_visible(&self, size: usize, x: i32, y: i32) -> bool {
        self.vis.is_visible(&self.gfx, size, x, y)
    }
}

/// The snapshot side of the world's fog: the gfx bits are shared until they change.
pub struct GameFog {
    frame: FogFrame,
}

impl GameFog {
    /// `CFogging::Start` for `local` in the world (a new game; a loaded save would pass its
    /// chunk 135).
    pub fn start(world: &mut World, local: u8) -> GameFog {
        world.start_fog(local, None);
        let mut g = GameFog { frame: FogFrame { gfx: Arc::new(Vec::new()), fade: 0, border: -50000, vis: Default::default() } };
        g.tick(world);
        g
    }

    /// A loaded world that kept its fog (the explored area survives a save, chunk 135); a
    /// world without one starts it.
    pub fn resume(world: &mut World, local: u8) -> GameFog {
        if world.fog.is_none() {
            return GameFog::start(world, local);
        }
        let mut g = GameFog { frame: FogFrame { gfx: Arc::new(Vec::new()), fade: 0, border: -50000, vis: Default::default() } };
        g.tick(world);
        g
    }

    pub fn frame(&self) -> FogFrame {
        self.frame.clone()
    }

    /// After a tick: copy what changed.
    pub fn tick(&mut self, world: &World) {
        let Some(f) = world.fog.as_ref() else { return };
        if f.gfx() != self.frame.gfx.as_slice() {
            self.frame.gfx = Arc::new(f.gfx().to_vec());
        }
        self.frame.fade = f.fade();
        self.frame.border = f.border();
        self.frame.vis = f.visibility();
    }
}
