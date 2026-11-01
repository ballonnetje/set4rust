//! The manakopters' rotor sounds: the end of `CManakopter::LogicUpdate` (0x56b4e0, from
//! 0x56b71a), which the sim reports as `SimEvent::Rotor`. The entity keeps two `PlaySound`
//! handles, +0x80 (`SOUND_MAGIC_MANAKOPTERROTOR`, 100) and +0x84 (`SOUND_MAGIC_MANAKOPTERTURN`,
//! 99), both −1 from the ctor. Every update, at the node it has just taken from its moving effect:
//! * `CEffects` vtbl+0x5c (0x4e41e0) on its shadow effect (+0x64, type 11) answers whether the
//!   effect was drawn since the last call and clears that: the draw pass's fill of a type-11
//!   effect (0x4e2eb0, record +0x34 from 0x4e300e) sets record +0x18. Not drawn: each handle
//!   that still plays (0x50d6a0) is stopped (`StopSample`) and set to −1.
//! * Drawn, with an order (state +0x4c bit 1): if +0x84 does not play, +0x80 is stopped (whether
//!   it plays or not) and set to −1, and `PlaySound(99, ..)` at the node (0x50d6f0, last
//!   argument 0: once) goes to +0x84. Then it returns.
//! * Drawn, no order (landing, landed): if +0x80 does not play, +0x84 is stopped and set to −1,
//!   and `PlaySound(100, .., 1)` goes to +0x80: looped (`EndOfSoundCallback` 0x69ac20 restarts
//!   the sample while its record holds the handle).
//!
//! So a flying manakopter repeats the turn sound whenever the last one has ended and its update
//! comes (every tick while climbing or sinking, at each node of the flight), and a landed one
//! loops the rotor. `Delete` (0x56b330) stops both (`SimEvent::RotorStop`).
//!
//! The shadow effect sits at the node of the manakopter's last update: it follows the moving
//! effect only through vtbl+0x58 (0x4e4160: unlink, the parent's position, link), which the
//! update calls when it starts a hop, from the node it reports. Its drawn flag is the client's:
//! each frame sets it when the draw pass iterates that node's effect square
//! ([`super::DrawRange::effect_drawn`]). [inf] Before the first update the client has no
//! position: the first event tests the square in the current view instead.

use std::collections::HashMap;

use s4_audio::VoiceId;

/// `SOUND_MAGIC_MANAKOPTERTURN`.
pub const TURN: u16 = 99;
/// `SOUND_MAGIC_MANAKOPTERROTOR`.
pub const ROTOR: u16 = 100;

#[derive(Clone, Copy, Debug, Default)]
struct Track {
    /// The node of the last update (the shadow effect's).
    pos: (i32, i32),
    /// Effect record +0x18.
    drawn: bool,
    /// +0x80, the rotor loop.
    rotor: Option<VoiceId>,
    /// +0x84, the turn sound.
    turn: Option<VoiceId>,
}

/// What [`Rotors::update`] asks of the audio side, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Stop(VoiceId),
    /// `PlaySound(sound, ..)` at the update's node; `looped` for [`ROTOR`]. The caller answers
    /// with [`Rotors::played`].
    Play { sound: u16, looped: bool },
}

/// The client's side of every manakopter's handles and drawn flag.
#[derive(Default)]
pub struct Rotors {
    map: HashMap<u16, Track>,
}

impl Rotors {
    /// One drawn frame: every shadow effect in an iterated square is drawn.
    pub fn draw(&mut self, effect_drawn: impl Fn(i32, i32) -> bool) {
        for t in self.map.values_mut() {
            if effect_drawn(t.pos.0, t.pos.1) {
                t.drawn = true;
            }
        }
    }

    /// `SimEvent::Rotor`: the sound part of 0x56b4e0. `drawn_now` tests a node's square in the
    /// current view (for a manakopter seen for the first time), `playing` is 0x50d6a0.
    pub fn update(&mut self, id: u16, pos: (i32, i32), ordered: bool, drawn_now: impl Fn(i32, i32) -> bool, playing: impl Fn(VoiceId) -> bool) -> Vec<Act> {
        let t = self.map.entry(id).or_insert_with(|| Track { pos, drawn: drawn_now(pos.0, pos.1), ..Track::default() });
        t.pos = pos;
        let drawn = std::mem::take(&mut t.drawn);
        let plays = |v: Option<VoiceId>| v.is_some_and(&playing);
        let mut acts = Vec::new();
        if !drawn {
            for v in [&mut t.rotor, &mut t.turn] {
                if plays(*v) {
                    acts.extend(v.take().map(Act::Stop));
                }
            }
            return acts;
        }
        if ordered {
            if !plays(t.turn) {
                acts.extend(t.rotor.take().map(Act::Stop));
                acts.push(Act::Play { sound: TURN, looped: false });
            }
            return acts;
        }
        if !plays(t.rotor) {
            acts.extend(t.turn.take().map(Act::Stop));
            acts.push(Act::Play { sound: ROTOR, looped: true });
        }
        acts
    }

    /// The handle of the [`Act::Play`] just made (`None`: `PlaySound` returned 0).
    pub fn played(&mut self, id: u16, sound: u16, v: Option<VoiceId>) {
        if let Some(t) = self.map.get_mut(&id) {
            if sound == TURN {
                t.turn = v;
            } else {
                t.rotor = v;
            }
        }
    }

    /// `SimEvent::RotorStop` (0x56b330): both handles stopped; the entity is gone.
    pub fn delete(&mut self, id: u16) -> Vec<VoiceId> {
        self.map.remove(&id).map(|t| [t.turn, t.rotor].into_iter().flatten().collect()).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turn_while_ordered_rotor_loop_after_and_silence_off_screen() {
        let mut r = Rotors::default();
        let v = |n| VoiceId(n);
        // First update, on screen, ordered: the turn sound.
        let a = r.update(7, (10, 10), true, |_, _| true, |_| true);
        assert_eq!(a, vec![Act::Play { sound: TURN, looped: false }]);
        r.played(7, TURN, Some(v(1)));
        // Not drawn since: the playing turn sound stops.
        let a = r.update(7, (11, 10), true, |_, _| true, |_| true);
        assert_eq!(a, vec![Act::Stop(v(1))]);
        // Drawn again, the turn sound over: a new one.
        r.draw(|_, _| true);
        let a = r.update(7, (12, 10), true, |_, _| false, |_| false);
        assert_eq!(a, vec![Act::Play { sound: TURN, looped: false }]);
        r.played(7, TURN, Some(v(2)));
        // Landing (no order): the turn sound is stopped, the rotor loops.
        r.draw(|_, _| true);
        let a = r.update(7, (12, 10), false, |_, _| false, |h| h == v(2));
        assert_eq!(a, vec![Act::Stop(v(2)), Act::Play { sound: ROTOR, looped: true }]);
        r.played(7, ROTOR, Some(v(3)));
        // Still playing: nothing.
        r.draw(|_, _| true);
        assert!(r.update(7, (12, 10), false, |_, _| false, |h| h == v(3)).is_empty());
        // A drawn frame elsewhere does not count.
        r.draw(|x, _| x != 12);
        assert_eq!(r.update(7, (12, 10), false, |_, _| true, |h| h == v(3)), vec![Act::Stop(v(3))]);
        r.played(7, ROTOR, Some(v(4)));
        assert_eq!(r.delete(7), vec![v(4)]);
        assert!(r.delete(7).is_empty());
    }
}
