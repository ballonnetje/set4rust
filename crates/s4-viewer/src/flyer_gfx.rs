//! How a flying object (the Dark Tribe's manakopter, s4-sim `flying.rs`) is drawn. Own reverse
//! engineering of the release; decompiles in `tools/re/viewer-2026-10-23/manako/`.
//!
//! `CManakopter::GetGfxInfo` (vtbl+0x14 0x56bb30) returns 0: the entity draws nothing itself. It is
//! three `CEffects` records:
//! * **the shadow**, its moving effect (+0x44, record type 8, effect 0x6e = `Gfx\37.gfx` job 10,
//!   16 directions of one frame: an ellipse on the ground). Draw routine 0x4e2b70: direction +5
//!   (0x4e1f30 of the last move's screen vector, set by 0x4e28e0; 0 before the first move), frame
//!   `(+7 + age) % +6`, at `from + (to − from) · min(age, ticks) / ticks` of the move's screen
//!   positions (integer division, `from` while `ticks` or `age` is below 1), `age` = the game
//!   tick − the move's start (+8);
//! * **the body** (+0x64, effect 0x6d = job 9, 16 directions of one frame) and **the rotor**
//!   (+0x68, effect 0x6f = job 11, one direction of 6 frames), records of type 11 attached to the
//!   shadow (0x4e4090 → 0x4e2f60; the ctor 0x56ae30 makes the rotor first with flag 0, then the
//!   body with flag 1). Flag 1 takes the shadow's direction count and direction (`+5 % +4`), flag
//!   0 one direction. `CEffects` +8 (0x4e3580) draws an attached record by running its shadow's
//!   routine first and then its own (0x4e2eb0): its own image (direction, frame `age % +6` with
//!   its own start, the attach tick), the shadow's position plus its offset (+0x20 / +0x24,
//!   vtbl+0x54 0x4e4110 ← the manakopter's +0x78 / +0x7c: −10 / −50 on the ground, height −200
//!   in the air).
//!
//! Records sit in the row of their node (+0x10 / +0x14, 0x4e38d0): the shadow at its move's
//! `from` node, the attached ones at the same node one row further (0x4e2f60 / 0x4e2f20: +0x14 + 1
//! below 0x400), re-synced after every move (vtbl+0x58 0x4e4160: rotor, then body). Each list is
//! drawn head first and new or re-linked records go to the head, so the body comes before the
//! rotor. All three set +0x2c9 = 0xc0: they are drawn from the deferred list, after every row.
//! The colour byte +0x2cb is never set: no player colour.

use crate::population::{Flyer, Population};
use crate::sprites::{AmbientSprite, aim};

/// The shadow, the moving effect (`CManakopter` +0x44, created by 0x569100).
pub const SHADOW: u8 = 0x6e;
/// The body: attached with flag 1 (the shadow's direction).
pub const BODY: u8 = 0x6d;
/// The rotor: attached with flag 0 (one direction, 6 frames).
pub const ROTOR: u8 = 0x6f;

/// The three effects of `f` at game tick `now`, in the order they are drawn.
pub fn flyer_sprites(f: &Flyer, now: u32) -> [AmbientSprite; 3] {
    let m = &f.flight;
    // 0x4e2b70: min(age, +3) of the way between the screen positions.
    let age = if m.start <= now { (now - m.start).min(i32::MAX as u32) as i32 } else { 0 };
    let n = m.ticks as i32;
    let u = age.min(n);
    let px = if n == 0 || u < 1 {
        m.screen_from
    } else {
        (m.screen_from.0 + (m.screen_to.0 - m.screen_from.0) * u / n, m.screen_from.1 + (m.screen_to.1 - m.screen_from.1) * u / n)
    };
    // +5: 0x4e1f30 of the last move's vector (0x4e28e0), 0 from the ctor.
    let (dx, dy) = (m.screen_to.0 - m.screen_from.0, m.screen_to.1 - m.screen_from.1);
    let dir = if (dx, dy) == (0, 0) { 0 } else { aim(dx as f32, dy as f32) };
    let (x, y) = (m.from.0.clamp(0, u16::MAX as i32) as u16, m.from.1.clamp(0, u16::MAX as i32) as u16);
    let up = if (y as i32) < 0x400 { y + 1 } else { y };
    let above = (px.0 + f.offset.0, px.1 + f.offset.1);
    // [inf] The attached records' start (+8) is the manakopter's creation tick, which the
    // simulation does not keep: the rotor's frame is taken from the game tick alone.
    let rotor_frame = now % 6;
    [
        AmbientSprite::Effect { gfx: SHADOW, dir, frame: 0, x, y, px },
        AmbientSprite::Effect { gfx: BODY, dir, frame: 0, x, y: up, px: above },
        AmbientSprite::Effect { gfx: ROTOR, dir: 0, frame: rotor_frame, x, y: up, px: above },
    ]
}

/// Every flying object of `p` at game tick `now`.
pub fn sprites(p: &Population, now: u32) -> Vec<AmbientSprite> {
    p.flyers.iter().flat_map(|f| flyer_sprites(f, now)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use s4_sim::flying::Flight;

    /// A move of 10 ticks from screen (100, 200) to (160, 230): the shadow steps a tenth of the
    /// way per tick (truncated), stands at the start before it and at the end after it, faces the
    /// move; body and rotor sit at the offset above it, one row further, the body in the
    /// shadow's direction, the rotor in direction 0 with its 6 frames.
    #[test]
    fn shadow_body_and_rotor_follow_the_moving_effect() {
        let f = Flyer {
            id: 9,
            owner: 1,
            flight: Flight { from: (40, 50), to: (41, 50), screen_from: (100, 200), screen_to: (160, 230), start: 100, ticks: 10, speed: 60 },
            offset: (-10, -200),
        };
        let at = |now| match flyer_sprites(&f, now) {
            [AmbientSprite::Effect { gfx: SHADOW, dir, x: 40, y: 50, px, .. }, AmbientSprite::Effect { gfx: BODY, dir: bd, y: 51, px: bp, .. }, AmbientSprite::Effect { gfx: ROTOR, dir: 0, frame, y: 51, px: rp, .. }] => {
                assert_eq!((bd, bp), (dir, rp));
                assert_eq!(bp, (px.0 - 10, px.1 - 200));
                (px, dir, frame)
            }
            s => panic!("{s:?}"),
        };
        assert_eq!(at(99).0, (100, 200));
        assert_eq!(at(100).0, (100, 200));
        assert_eq!(at(103).0, (100 + 60 * 3 / 10, 200 + 30 * 3 / 10));
        assert_eq!(at(110).0, (160, 230));
        assert_eq!(at(400).0, (160, 230));
        assert_eq!(at(105).1, aim(60.0, 30.0));
        assert_eq!((0..12).map(|t| at(100 + t).2).collect::<Vec<_>>(), [4, 5, 0, 1, 2, 3, 4, 5, 0, 1, 2, 3]);
        // Standing since the ctor: direction 0, at its node.
        let still = Flyer { flight: Flight { from: (40, 50), to: (40, 50), screen_from: (100, 200), screen_to: (100, 200), start: 100, ticks: 0, speed: 60 }, ..f };
        assert!(matches!(flyer_sprites(&still, 130)[0], AmbientSprite::Effect { dir: 0, px: (100, 200), .. }));
    }
}
