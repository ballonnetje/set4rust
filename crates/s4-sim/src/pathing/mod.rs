//! Path finding (`Pathing/CAStarTiling`, `CAStar64*`, `CWaypoints`, AStar.cpp).
//!
//! Two levels, as in the original: [`astar_tiling`] finds a chain of tiles (coarse waypoints =
//! tile centres) on the tiling; the node-level search ([`astar64`]) then walks a 64×64 window
//! between consecutive waypoints; [`walking`] is the per-entity state machine driving both.
//!
//! The helpers `CAStarTiling::FindPath` calls through function pointers are not in the IDA
//! decompilation; they were read from the shipped `S4_Main.exe` with capstone (FindPath at VA
//! 0x5ac2f0, helpers 0x5abd00..0x5ac250, heap 0x5ad8d0/0x5ad930; listings in the session
//! scratchpad, findings in docs/SIM.md §11).

pub mod node_astar;
mod scratch;
pub mod tiling_astar;
pub mod walking;

pub use node_astar::{DirCache, Variant, astar64};
pub use tiling_astar::{Alliances, astar_tiling};

use crate::grid::Y16X16;

/// `Grid::Distance` on packed positions (`Y16X16::DistanceFast`, VA 0x58b0c0).
pub fn distance_fast(a: Y16X16, b: Y16X16) -> i32 {
    crate::spiral::grid_distance(b.x() - a.x(), b.y() - a.y())
}

/// `CWaypoints`: the goal plus a ring of the last 16 pushed points.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Waypoints {
    /// +0 `m_iGoal` (-1 = none).
    pub goal: i32,
    /// +4 `m_iGoalDistance` (the G value the rebuild passes; not reset by `Init`).
    pub goal_value: i32,
    /// +8 `m_iTotalWaypoints`: pushes since `Init`.
    pub total: u32,
    /// +0xc `m_iCachedWaypointsCount` (≤ 16).
    pub count: u32,
    /// +0x10 `m_iCurrentPointer`.
    pub head: u8,
    /// +0x14 ring.
    pub ring: [u32; 16],
    /// +0x54 `m_bFullyCached`.
    pub fully_cached: bool,
}

impl Default for Waypoints {
    fn default() -> Self {
        Waypoints { goal: -1, goal_value: 0, total: 0, count: 0, head: 15, ring: [0; 16], fully_cached: false }
    }
}

impl Waypoints {
    /// `CWaypoints::Init` (`+4` untouched).
    pub fn init(&mut self) {
        self.goal = -1;
        self.total = 0;
        self.count = 0;
        self.head = 15;
        self.fully_cached = false;
    }
    /// `CWaypoints::PushGoal`.
    pub fn push_goal(&mut self, xy: Y16X16, value: i32) {
        self.goal = xy.0 as i32;
        self.goal_value = value;
        self.total = 1;
        self.count = 1;
        self.head = 0;
        self.ring[0] = xy.0;
        self.fully_cached = true;
    }
    /// `CWaypoints::PushBack`: older points beyond 16 fall off.
    pub fn push_back(&mut self, xy: Y16X16) {
        let fits = self.count < 16;
        self.total += 1;
        self.count += fits as u32;
        self.head = (self.head + 1) & 15;
        self.ring[self.head as usize] = xy.0;
        self.fully_cached = fits;
    }
    /// `CWaypoints::Back`.
    pub fn back(&self) -> Y16X16 {
        Y16X16(self.ring[self.head as usize])
    }
    /// `CWaypoints::PopBack`.
    pub fn pop_back(&mut self) {
        if self.count > 0 {
            self.count -= 1;
            self.head = self.head.wrapping_sub(1) & 15;
        }
    }
    /// `CWaypoints::GoalCached`.
    pub fn goal_cached(&self) -> bool {
        self.fully_cached
    }
}
