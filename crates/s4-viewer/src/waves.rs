//! Water animation: wave sprites placed per water node, animated by the game tick.
//!
//! The landscape textures themselves are static. The water moves because `RenderObjectLayer`
//! calls a per-node helper (debug `sub_2F6F620`, not decompiled; release rel 0x663af0, read from
//! disassembly) right after the border stone and before the node's object, which blits a frame of
//! one of the wave animations through `g_pfBlitWave` (`BlitWave`, release 0x661fa0).
//!
//! # Which animation (`CGfxManager::MoveWavesToGfxEngine`)
//! Wave slot `w` = file 5 job [`WAVE_JOBS`]`[w]`, direction 0, all frames (`g_uWaveFrames[w]` =
//! `GetObjectFrameCount(job)`). Jobs 517..=532 are the objects `WAVE_LAKE24X12`..`WAVE48X18_V3`
//! (object id + 261). Slot 13 repeats job 0x208: the original registers `WAVE_LAKE37X19` twice and
//! never `WAVE_LAKE48X19`. The per-node helper only uses slots 0..=8.
//!
//! # Per node (release rel 0x663af0)
//! Arguments: the node's screen anchor `(X16, Y16)` *without* the height offset, column `x`, row
//! `y`, ground type `t`. Nothing for `x < 4`, `y < 2`, `x > size − 7`, `y > size − 9`.
//! `srand(y·size + x)` then one `rand()` (MSVC LCG) picks the slot, so the choice is fixed per node.
//! A node is *visible* if `GetObjectFog(fadeStep, oldFog, newFog) > 58`.
//! * `t = 1` (shallowest water but one): skipped where `x & 2 && y & 3 == 3`; slot
//!   `4 + rand() % 5`; drawn at `(X16 − W, Y16 − H)`.
//! * `t = 2..=4`: only where `x & 2 && y & 2`; slot `max(rand() % 6, 2)`; drawn at
//!   `(X16 − W, Y16 − H)`, `(X16 − 3W, …)` for slot 3; needs node `x − 3` visible (if `x > 3`).
//! * `t = 5..=7` (deep): only where `x & 2 && y & 3 == 3`; slot `rand() % 6`; drawn at
//!   `(X16 − 3W, Y16 − H)`. Slot 0 (the 216×99 wave) needs `x ≤ size − 13`, `y ≤ size − 8` and
//!   nodes `x − 6` (if `x > 6`), `x + 8` (if `x < size − 8`) and `y + 2` (if `y < size − 2`)
//!   visible; other slots need `x − 4` (if `x > 4`), `x + 3` (if `x < size − 3`) and `y + 1`
//!   (if `y < size − 1`) visible.
//! * Every case needs the node itself visible and bit 0x20 of its byte 2 clear.
//!
//! Frame = `(*tick + y) % g_uWaveFrames[w]`, where the tick pointer is the game's virtual tick
//! (`IGfxEngine::SetTickCounterAdress(&m_sGameData.m_uVirtualTick)` in `CStateGame`): one frame
//! per logic tick, 71 ms at game speed 1.
//!
//! `BlitWave` subtracts the image's `left/top` *unshifted* from the 16.16 anchor (the object
//! blitters shift them by 16 first), so a wave's top-left is the anchor itself, one pixel up/left
//! when the offset is non-zero. The fog value darkens the wave like any object (`sub_2F6FD80`).

/// File 5 job of each wave slot (`MoveWavesToGfxEngine`).
pub const WAVE_JOBS: [usize; 16] = [525, 524, 529, 528, 527, 526, 530, 531, 532, 517, 518, 519, 520, 520, 522, 523];

/// The per-node helper only picks slots below this.
pub const WAVE_SLOTS_USED: usize = 9;

/// Game tick length at speed 1 (`CStateGame` frame pacing), in milliseconds.
pub const TICK_MS: u64 = 71;

/// MSVC `rand()` after `srand(seed)`: the first value.
fn msvc_first_rand(seed: u32) -> u32 {
    (seed.wrapping_mul(214013).wrapping_add(2531011) >> 16) & 0x7fff
}

/// A wave chosen for a node: slot and horizontal offset in triangle widths (−1 or −3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wave {
    pub slot: u8,
    pub dx: i8,
}

/// The per-node decision of rel 0x663af0. `ground(i)`, `byte2(i)` read node `i` (row-major),
/// `visible(i)` is `GetObjectFog(..) > 58`.
pub fn wave_at(
    size: usize,
    x: usize,
    y: usize,
    ground: impl Fn(usize) -> u8,
    byte2: impl Fn(usize) -> u8,
    visible: impl Fn(usize) -> bool,
) -> Option<Wave> {
    let s = size as isize;
    let (xi, yi) = (x as isize, y as isize);
    if xi < 4 || yi < 2 || xi > s - 7 || yi > s - 9 {
        return None;
    }
    let i = y * size + x;
    let t = ground(i);
    let r = msvc_first_rand(i as u32);
    let here = || visible(i) && byte2(i) & 0x20 == 0;
    match t {
        1 => {
            if x & 2 != 0 && y & 3 == 3 {
                return None;
            }
            here().then_some(Wave { slot: (4 + r % 5) as u8, dx: -1 })
        }
        2..=4 => {
            if x & 2 == 0 || y & 2 == 0 {
                return None;
            }
            let slot = (r % 6).max(2) as u8;
            if !here() || (xi > 3 && !visible(i - 3)) {
                return None;
            }
            Some(Wave { slot, dx: if slot == 3 { -3 } else { -1 } })
        }
        5..=7 => {
            if x & 2 == 0 || y & 3 != 3 {
                return None;
            }
            let slot = (r % 6) as u8;
            if !here() {
                return None;
            }
            let ok = if slot == 0 {
                xi <= s - 13
                    && yi <= s - 8
                    && (xi <= 6 || visible(i - 6))
                    && (xi >= s - 8 || visible(i + 8))
                    && (yi >= s - 2 || visible(i + 2 * size))
            } else {
                (xi <= 4 || visible(i - 4)) && (xi >= s - 3 || visible(i + 3)) && (yi >= s - 1 || visible(i + size))
            };
            ok.then_some(Wave { slot, dx: -3 })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn msvc_rand_matches_crt() {
        // srand(1); rand() == 41 with the MSVC CRT.
        assert_eq!(msvc_first_rand(1), 41);
        assert_eq!(msvc_first_rand(0), 38);
    }

    #[test]
    fn deep_water_pattern() {
        let size = 64;
        let w = |x, y| wave_at(size, x, y, |_| 7, |_| 0, |_| true);
        // Only x & 2 and y & 3 == 3.
        assert!(w(10, 11).is_some());
        assert!(w(9, 11).is_none());
        assert!(w(10, 10).is_none());
        assert_eq!(w(10, 11).unwrap().dx, -3);
        // Shallow type 1 everywhere except the deep-water grid points.
        let s = |x, y| wave_at(size, x, y, |_| 1, |_| 0, |_| true);
        assert!(s(9, 10).is_some());
        assert!(s(10, 11).is_none());
        assert!((4..=8).contains(&s(9, 10).unwrap().slot));
    }
}
