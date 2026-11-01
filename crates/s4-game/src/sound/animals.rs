//! Animal sounds: the draw-time update of a land animal (`CAnimal` 0x52bb90, from its
//! `FillGfxInfo` 0x52bb30 each drawn frame). When the tick counter moved since the animal's last
//! update (`(u8)(tick − +0x2c)` ≠ 0) and it is not in state 5 (+0x58), an animal in task 0x10
//! (idle animation) or 0x1b (dying) whose type has a `SOUND` plays it at its node when
//! * `SOUND_INTERVAL` is 100, or CRT `rand() % 100 < SOUND_INTERVAL`,
//! * its previous sound (+0x74, the `PlaySound` handle) no longer plays (0x50d6a0), and
//! * the fog shows its node (`CFogging` vtbl+0x20),
//!
//! in that order (`PlayEnvironmentSound` 0x50c310), and keeps the new handle. Types and keys:
//! `Config\Animals.cfg` `[ANIMAL_*]` `SOUND` (−1 → none) and `SOUND_INTERVAL` (default 100), read
//! by `LoadAnimalData` 0x52f1d0 (type table +0x20 / +0x24). The install names sounds for BOAR,
//! DARKBUG(BIG), JAGUAR, DOLPHIN, WILDBEAR and BULL, all at interval 10, but `SOUND_ANIMAL_BOAR`
//! and `SOUND_ANIMAL_BULL` are no `SIV_SOUNDS` define (the exe has neither string): those two stay
//! silent, as an unknown define does here.
//!
//! Not ported: the ambient effects' sounds (birds, seagulls: the mover 0x52cb30), which need the
//! client-side `CEffects`.

use std::collections::HashMap;

use s4_audio::{SoundId, VoiceId};

/// `ANIMAL_*` 0..=17 (section names 0x10156dc).
pub const TYPES: [&str; 18] = [
    "ANIMAL_NO_ANIMAL",
    "ANIMAL_BOAR",
    "ANIMAL_DARKBUG",
    "ANIMAL_DARKBUGBIG",
    "ANIMAL_DEERFE",
    "ANIMAL_DEERMA",
    "ANIMAL_FOX",
    "ANIMAL_CHICKEN",
    "ANIMAL_JAGUAR",
    "ANIMAL_RABBIT",
    "ANIMAL_DOLPHIN",
    "ANIMAL_NESSY",
    "ANIMAL_SHARK",
    "ANIMAL_WILDBEAR",
    "ANIMAL_WOLF",
    "ANIMAL_FISH",
    "ANIMAL_RABBIT2",
    "ANIMAL_BULL",
];

/// A drawn animal as the snapshot shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Seen {
    pub id: u16,
    /// `ANIMAL_*`.
    pub kind: u16,
    /// The task type (`AnimalData::task_type`).
    pub task: u8,
    /// +0x58 = 5.
    pub state5: bool,
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug, Default)]
struct Track {
    tick: u32,
    voice: Option<VoiceId>,
}

#[derive(Clone, Debug, Default)]
pub struct AnimalSounds {
    /// Per type: (sound, interval).
    types: Vec<(u16, u8)>,
    tracks: HashMap<u16, Track>,
}

impl AnimalSounds {
    pub fn load() -> AnimalSounds {
        let path = s4_formats::path::resolve(s4_formats::game_dir().join("Config").join("Animals.cfg"));
        match std::fs::read(&path) {
            Ok(b) => AnimalSounds::parse(&String::from_utf8_lossy(&b)),
            Err(e) => {
                tracing::warn!("animal sounds: {}: {e}", path.display());
                AnimalSounds::default()
            }
        }
    }

    pub fn parse(text: &str) -> AnimalSounds {
        let cfg = s4_audio::cfg::Cfg::parse(text);
        let types = TYPES
            .iter()
            .map(|sec| {
                let sound = cfg.get(sec, "SOUND").map_or(0, |v| {
                    let v = v.trim();
                    v.strip_prefix('<').and_then(|v| v.strip_suffix('>')).and_then(SoundId::by_name).map_or(0, |s| s.0)
                });
                (sound, cfg.int(sec, "SOUND_INTERVAL", 100).clamp(0, 255) as u8)
            })
            .collect();
        AnimalSounds { types, tracks: HashMap::new() }
    }

    /// One drawn frame's update of `a` at tick `tick`. `roll(interval)` is the CRT roll,
    /// `playing` the handle test, `visible` the fog. Returns the sound to play now; the caller
    /// hands its voice to [`AnimalSounds::played`].
    pub fn update(&mut self, a: &Seen, tick: u32, roll: &mut impl FnMut(u8) -> bool, playing: impl Fn(VoiceId) -> bool, visible: impl Fn(i32, i32) -> bool) -> Option<u16> {
        let tr = self.tracks.entry(a.id).or_insert(Track { tick, voice: None });
        if (tick.wrapping_sub(tr.tick) & 0xff) == 0 || a.state5 {
            return None;
        }
        tr.tick = tick;
        if !matches!(a.task, 0x10 | 0x1b) {
            return None;
        }
        let &(sound, interval) = self.types.get(a.kind as usize)?;
        if sound == 0 || !roll(interval) || tr.voice.is_some_and(&playing) || !visible(a.x, a.y) {
            return None;
        }
        Some(sound)
    }

    /// The handle `PlaySound` returned for `id`'s sound.
    pub fn played(&mut self, id: u16, voice: Option<VoiceId>) {
        if let Some(t) = self.tracks.get_mut(&id) {
            t.voice = voice;
        }
    }

    /// Forget the animals not drawn any more.
    pub fn retain(&mut self, keep: impl Fn(u16) -> bool) {
        self.tracks.retain(|&id, _| keep(id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_or_dying_animals_sound_unless_still_playing() {
        let mut s = AnimalSounds::parse("[ANIMAL_JAGUAR]\n{\n SOUND = <SOUND_ANIMAL_JAGUAR>\n SOUND_INTERVAL = 10\n}\n");
        let boar = SoundId::by_name("SOUND_ANIMAL_JAGUAR").unwrap().0;
        let a = Seen { id: 7, kind: 8, task: 0x10, state5: false, x: 1, y: 2 };
        let mut yes = |_| true;
        assert_eq!(s.update(&a, 10, &mut yes, |_| false, |_, _| true), None, "first sight: no step");
        assert_eq!(s.update(&a, 11, &mut yes, |_| false, |_, _| true), Some(boar));
        s.played(7, Some(VoiceId(3)));
        assert_eq!(s.update(&a, 12, &mut yes, |_| true, |_, _| true), None, "the last one still plays");
        assert_eq!(s.update(&a, 13, &mut yes, |_| false, |_, _| false), None, "fogged");
        assert_eq!(s.update(&a, 14, &mut |i| i == 100, |_| false, |_, _| true), None, "interval 10: lost the roll");
        let walk = Seen { task: 0x11, ..a };
        assert_eq!(s.update(&walk, 15, &mut yes, |_| false, |_, _| true), None);
        let deer = Seen { kind: 4, ..a };
        assert_eq!(s.update(&deer, 16, &mut yes, |_| false, |_, _| true), None);
    }

    #[test]
    fn the_shipped_config() {
        let s = AnimalSounds::load();
        if s.types.is_empty() {
            return;
        }
        let id = |n: &str| SoundId::by_name(n).unwrap().0;
        assert_eq!(s.types[8], (id("SOUND_ANIMAL_JAGUAR"), 10));
        assert_eq!(s.types[13], (id("SOUND_ANIMAL_BEAR"), 10));
        assert_eq!(s.types[2], (id("SOUND_ANIMAL_BUG"), 10));
        // No such defines: silent.
        assert_eq!((s.types[1].0, s.types[17].0, s.types[4].0), (0, 0, 0));
    }
}
