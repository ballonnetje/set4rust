//! The boundary to pathing: what the job code needs from `CWalking` (Pathing/Walking*.cpp).
//!
//! The job code only calls [`Pathing`] through [`ActivePathing`], implemented by the pathing port
//! (`pathing::walking`: `CWalkingBase::Walk` with `CWalkingNormal` / `CWalkingWorker`).
//!
//! The step word ([`Pathing::next_step`], `CWalking` vtbl+12) is what `CSettler::WalkDir`
//! consumes: low nibble = direction 0..5 (≥ 6 = no step this time), `0x10` = do not wear a
//! path (`IncreaseWalkCount` skipped), `0x20` = arrived (the role takes its next job), `0x40` =
//! failed (the role gives up: `SetFree`), `0x88` is or-ed in by `WalkDir` when the next node is
//! blocked.

use crate::World;
use crate::grid::Y16X16;

/// `Y16X16::g_iNeighborY16X16Modifiers` (release exe .rdata 0x101bd58, found by content):
/// direction → `(dx, dy)`. 0 = +x, 1 = +x+y, 2 = +y, 3 = −x, 4 = −x−y, 5 = −y.
pub const NEIGHBOR: [(i32, i32); 6] = [(1, 0), (1, 1), (0, 1), (-1, 0), (-1, -1), (0, -1)];

/// `ArrivedFlag`: the walk reached its target (handled by `ISettlerRole::Go`).
pub const WALK_ARRIVED: u32 = 0x20;
/// The walk failed.
pub const WALK_FAILED: u32 = 0x40;
/// Or-ed into the result by `CSettler::WalkDir` when the next node is not free.
pub const WALK_BLOCKED: u32 = 0x88;
/// `CSettler::Walk` without a `CWalking`: direction 8, no flags.
pub const WALK_NONE: u32 = 72;

/// The `CWalkingBase` data, ported in `pathing::walking` (part of the settler's state and hash).
pub use crate::pathing::walking::WalkState;

/// The calls the job code makes into `CWalking`.
pub trait Pathing {
    /// `CWalking::Create(kind, owner)` (role `InitWalking`).
    fn create(world: &mut World, settler: u16, kind: u8);
    /// vtbl+8, `IMovingEntity::WalkToXY(target, flags)`. A target of −1 cancels the walk.
    fn walk_to_xy(world: &mut World, settler: u16, target: Y16X16, flags: i32);
    /// vtbl+12: the next step word for the settler at its current position (see module docs).
    fn next_step(world: &mut World, settler: u16) -> u32;
    /// vtbl+16, `CWalkingBase::IdleWalk(currentXY, flags)`: a step word for standing about.
    fn idle_step(world: &mut World, settler: u16, flags: u32) -> u32;
    /// vtbl+20, `IMovingEntity::IsNotBlocked(node)`.
    fn is_not_blocked(world: &World, settler: u16, node: usize) -> bool;
    /// vtbl+24, `IMovingEntity::IsNotOccupied(node)`.
    fn is_not_occupied(world: &World, settler: u16, node: usize) -> bool;
}

/// The implementation the job code uses: the `CWalkingBase` port.
pub type ActivePathing = crate::pathing::walking::OriginalPathing;
