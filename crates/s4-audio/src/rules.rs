//! The original's audio rules as plain integer/float functions, independent of any audio library.
//! Every item cites the function of the decompiled debug build (`Main\CSoundManager.cpp`,
//! `SoundEngine\CSoundSystem.cpp`, `SoundEngine\CPlaylist.cpp`, `Logic\CLogic.cpp`).
//! Volumes and pans are in Miles Sound System units, 0..=127 (pan 64 = centre).

/// `ISoundEngine::Init(dir, 22050, stereo, 16 bit, 28, 4)` in `CGameStateHandler`: 28 sample
/// handles and 4 stream handles. A sound that finds no free handle is dropped
/// (`CSoundSystem::PlaySoundFromMemory`: "No free sound handle"); nothing is stolen.
pub const MAX_SOUND_VOICES: usize = 28;
/// See [`MAX_SOUND_VOICES`]: music streams ("No free stream handle").
pub const MAX_STREAMS: usize = 4;
/// `ISoundEngine::SetTimerInterval(71)`: fades advance one step per 71 ms
/// (`CSoundSystem::Fade` / `UpdateFades`).
pub const FADE_STEP_MS: u32 = 71;
/// `CLogic::HandleSoundtracks`: fight music lasts until 32 logic ticks after the last war action.
pub const FIGHT_TICKS: i32 = 32;
/// `CSoundManager` constructor: `this[0] = 64`, the pan `PlaySoundFX(id)` passes.
pub const PAN_CENTER: i32 = 64;
/// The ambient loops of `CSoundManager::Update` (release 0x50d860): `(mask, SIV_SOUNDS id)`,
/// tested against `CAIResourceData::Flags1` (byte 39) of the 16×16-node resource square under the
/// screen centre. Tables at release 0x1018dd0 (masks) and 0x1018de8 (ids). Masks 0x01 and 0x10
/// have sound 0, i.e. no loop. Order as in the exe.
pub const AMBIENT: [(u8, u16); 6] = [(0x01, 0), (0x02, 35), (0x10, 0), (0x08, 80), (0x20, 61), (0x04, 36)];
/// Ambient loops fade in/out over 500 ms (`ISoundEngine::Fade(.., 500)`).
pub const AMBIENT_FADE_MS: i32 = 500;
/// Centre volume below this is treated as 0 for ambient loops (`if v < 5: v = 0`).
pub const AMBIENT_MIN_VOLUME: i32 = 5;
/// Volume setting default (`this[4]`, `this[5]` = 100) and mute factor (`this[6]`: 100 or 0).
pub const DEFAULT_VOLUME: i32 = 100;

/// `(int)double` as SSE2 `cvttsd2si`: NaN / out of range give `i32::MIN`.
pub fn cvttsd2si(n: f64) -> i32 {
    if n > -2_147_483_649.0 && n < 2_147_483_648.0 { n as i32 } else { i32::MIN }
}

/// `CSoundManager::CalcFinalVolume(a, b, c)` = `(int)(float)((float)(a*b*c*mute) * 1e-6)`,
/// clamped to 0..=127. `mute` is `this[6]` (100, or 0 when muted).
pub fn calc_final_volume(a: i32, b: i32, c: i32, mute: i32) -> i32 {
    let prod = a.wrapping_mul(b).wrapping_mul(c).wrapping_mul(mute);
    let v = cvttsd2si(((prod as f32) as f64 * 0.000001) as f32 as f64);
    v.clamp(0, 127)
}

/// Where a map position is, as `CSoundManager::CalcPanningAndVolume` sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenProbe {
    /// `g_pFogging->IsVisible(x, y)` (vtbl+32).
    pub visible: bool,
    /// `IGfxEngine::GetScreenOffsetsByMapIndices`: pixel position on the output, `None` when the
    /// position is outside the map. Off-screen positions are still `Some` (may be negative or
    /// beyond the output size).
    pub screen: Option<(i32, i32)>,
    /// Output size in pixels.
    pub width: i32,
    pub height: i32,
    /// `CStateGame::GetZoomFactor()` = `(m_uZoom − 14.0) · 100 / 58.0` for a triangle size of
    /// 14..=72 px (16.16 fixed): 0 = farthest out, 100 = closest in.
    pub zoom_factor: f32,
}

/// `CSoundManager::CalcPanningAndVolume(x, y, &vol, &pan)` with the sound setting `sound`
/// (`this[5]`) and mute factor `mute`.
///
/// Not visible, or left of / above the output (negative screen x or y): volume 0, pan 127.
/// Otherwise `vol = 100 − 100·d² / ((w² + h²) / 4)` (d = distance from the output centre, so 100
/// in the centre and 0 at the corners, negative beyond), then scaled by `(int)(zoom + 20) / 100`
/// (0.2 zoomed out .. 1.2 zoomed in), then `CalcFinalVolume(sound, vol, sound)`.
/// `pan = 127 · x / w`, which exceeds 127 right of the output. Positions right of or below the
/// output are therefore only faded by distance, while those left or above are silent: that
/// asymmetry is in the original.
pub fn calc_pan_and_volume(p: &ScreenProbe, sound: i32, mute: i32) -> (i32, i32) {
    let Some((sx, sy)) = p.screen.filter(|_| p.visible) else { return (0, 127) };
    if sx < 0 || sy < 0 {
        return (0, 127);
    }
    let dx = (p.width >> 1).wrapping_sub(sx);
    let dy = (p.height >> 1).wrapping_sub(sy);
    let d2 = dy.wrapping_mul(dy).wrapping_add(dx.wrapping_mul(dx));
    let r = (p.height.wrapping_mul(p.height).wrapping_add(p.width.wrapping_mul(p.width)) >> 2).max(1);
    let mut vol = 100 - 100i32.wrapping_mul(d2) / r;
    let zoom = cvttsd2si(p.zoom_factor as f64 + 20.0);
    vol = cvttsd2si(((zoom.wrapping_mul(vol) as f32) as f64 * 0.0099999998) as f32 as f64);
    let pan = if p.width > 0 { 127i32.wrapping_mul(sx) / p.width } else { PAN_CENTER };
    (calc_final_volume(sound, vol, sound, mute), pan)
}

/// `PlaySoundFX(id, call_vol, pan, ..)` final sample volume: `CalcFinalVolume(sound, id_vol,
/// call_vol)`. For `PlaySoundFX(id)` (GUI, non-positional) `call_vol` = 100;
/// for `PlayEnvironmentSound` it is the volume from [`calc_pan_and_volume`].
pub fn fx_volume(sound: i32, id_vol: i32, call_vol: i32, mute: i32) -> i32 {
    calc_final_volume(sound, id_vol, call_vol, mute)
}

/// `CSoundManager::SetMusicVolume` / `SetSoundVolume`: `setting · mute / 100`, clamped to 0..=127
/// (the value `ChangeStreamVolume` / `ChangeSoundVolume` apply).
pub fn setting_volume(setting: i32, mute: i32) -> i32 {
    (setting.wrapping_mul(mute) / 100).clamp(0, 127)
}

/// One playlist (`CPlaylist`): titles with their fade-in time (ms, from `..._FADETIME`, 0 if the
/// key is missing), the playing index and the stream position saved by `StopSample`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Playlist {
    pub titles: Vec<(String, i32)>,
    pub current: usize,
    pub stored_pos: f64,
}

impl Playlist {
    /// `CPlaylist::SetTitle`: out-of-range indices select title 0.
    pub fn set_title(&mut self, i: usize) {
        self.current = if i < self.titles.len() { i } else { 0 };
    }
    /// `CPlaylist::GetActiveTitle`.
    pub fn active(&self) -> Option<&str> {
        self.titles.get(self.current).map(|t| t.0.as_str())
    }
    /// `CPlaylist::GetFadeInTime(i)`.
    pub fn fade_in_ms(&self, i: usize) -> i32 {
        self.titles.get(i).map_or(0, |t| t.1)
    }
    /// `CPlaylist::TitleEnded`: advance by `rand() % (n−1) + 1` (never the same title again when
    /// there are at least 2), wrapping once. `r` is the `rand()` value.
    pub fn title_ended(&mut self, r: u32) {
        let n = self.titles.len();
        if n <= 1 {
            self.current += 1;
        } else {
            self.current += (r % (n as u32 - 1)) as usize + 1;
        }
        if self.current >= n {
            self.current -= n;
        }
    }
}

/// What a fade step did (`CSoundSystem::UpdateFades`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FadeStep {
    Continue,
    /// Rising fade reached its target: job removed.
    Done,
    /// Falling (or flat) fade reached its target: job removed **and the sound stopped**.
    Stop,
}

/// A fade job (`CSoundSystem::Fade`): linear in Miles volume units, one step per
/// [`FADE_STEP_MS`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fade {
    pub current: f64,
    pub target: f64,
    pub step: f64,
}

impl Fade {
    /// `Fade(id, target, ms)` starting from `current` (the running job's value if the sound
    /// already has one, else its volume). `step = (target − current) / (ms / 71)`; for `ms = 0`
    /// that is ±∞ (or NaN when `target == current`), reproduced as is.
    pub fn new(current: f64, target: i32, ms: i32) -> Fade {
        let target = target as f64;
        Fade { current, target, step: (target - current) / (ms as f64 / FADE_STEP_MS as f64) }
    }

    /// One `UpdateFades` step: returns the volume to set and whether the job ended.
    pub fn advance(&mut self) -> (i32, FadeStep) {
        self.current += self.step;
        let v = cvttsd2si(self.current).clamp(0, 127);
        let end = if self.step > 0.0 {
            if self.current >= self.target { FadeStep::Done } else { FadeStep::Continue }
        } else if self.target >= self.current {
            FadeStep::Stop
        } else {
            FadeStep::Continue
        };
        (v, end)
    }
}

/// `SIV_MUSICSTYLES` (the playlist index).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MusicStyle {
    Settle = 0,
    Fight = 1,
    Sea = 2,
    Dark = 3,
    StartMenu = 4,
    Credits = 5,
}

impl MusicStyle {
    pub const ALL: [MusicStyle; 6] = [Self::Settle, Self::Fight, Self::Sea, Self::Dark, Self::StartMenu, Self::Credits];
    /// The names `LoadRaceTitles` appends (`aacMusicStyles`).
    pub fn name(self) -> &'static str {
        ["SETTLE", "FIGHT", "SEA", "DARK", "STARTMENU", "CREDITS"][self as usize]
    }
}

/// The playlist owner: a race, or the menu set (`LoadRaceTitles` race 5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Race {
    Roman = 0,
    Viking = 1,
    Maya = 2,
    Dark = 3,
    Trojan = 4,
    Menu = 5,
}

impl Race {
    pub fn name(self) -> &'static str {
        ["ROMAN", "VIKING", "MAYA", "DARK", "TROJAN", "MENU"][self as usize]
    }
    pub fn from_index(i: u8) -> Option<Race> {
        [Self::Roman, Self::Viking, Self::Maya, Self::Dark, Self::Trojan, Self::Menu].get(i as usize).copied()
    }
}

/// `CLogic::HandleSoundtracks`, run once per logic tick (from `CLogic::LogicUpdate`, skipped when
/// "user MP3" is on). `CLogic::SetWarAction` (called by `ISettlerRole` on every animation step of
/// a settler whose warrior type is 2..=5, wherever it is on the map) sets the counter to 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StyleDirector {
    /// `CLogic + 0x40`: ticks since the last war action, 0 = none pending.
    pub war: i32,
    /// `CLogic + 0x44`: the style last requested.
    pub style: MusicStyle,
}

impl Default for StyleDirector {
    fn default() -> Self {
        StyleDirector { war: 0, style: MusicStyle::Settle }
    }
}

impl StyleDirector {
    pub fn war_action(&mut self) {
        self.war = 1;
    }

    /// One tick. `centre_dark`: `None` if the screen-centre world coordinate is outside the map,
    /// else whether its node has flag bit 4 (dark land). Returns the style to switch to
    /// (`ChangeMusicStyle`), if any. While a fight is pending the centre is not looked at; when
    /// it expires the original switches to SETTLE unconditionally (even on dark land, which then
    /// switches to DARK on the next tick).
    pub fn tick(&mut self, centre_dark: Option<bool>) -> Option<MusicStyle> {
        if self.war != 0 {
            self.war += 1;
            if self.war <= FIGHT_TICKS {
                if self.style != MusicStyle::Fight {
                    self.style = MusicStyle::Fight;
                    return Some(MusicStyle::Fight);
                }
                return None;
            }
            self.style = MusicStyle::Settle;
            self.war = 0;
            return Some(MusicStyle::Settle);
        }
        match centre_dark {
            Some(true) if self.style != MusicStyle::Dark => {
                self.style = MusicStyle::Dark;
                Some(MusicStyle::Dark)
            }
            Some(false) if self.style != MusicStyle::Settle => {
                self.style = MusicStyle::Settle;
                Some(MusicStyle::Settle)
            }
            _ => None,
        }
    }
}

/// A small deterministic stand-in for the CRT `rand()` the original shares across the UI
/// (variant choice, next title). The original's sequence depends on every other `rand()` caller,
/// so it is not reproducible anyway; this one is seeded so runs are repeatable.
#[derive(Clone, Copy, Debug)]
pub struct Rand(u32);

impl Rand {
    pub fn new(seed: u32) -> Rand {
        Rand(seed.max(1))
    }
    /// 0..=0x7fff like MSVC `rand()`.
    pub fn rand(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x >> 8) & 0x7fff
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(x: i32, y: i32, zoom: f32) -> ScreenProbe {
        ScreenProbe { visible: true, screen: Some((x, y)), width: 1024, height: 768, zoom_factor: zoom }
    }

    #[test]
    fn pan_and_volume() {
        // centre, closest zoom: 100 * 1.2 = 120
        assert_eq!(calc_pan_and_volume(&probe(512, 384, 100.0), 100, 100), (120, 63));
        // centre, farthest zoom: 100 * 0.2
        assert_eq!(calc_pan_and_volume(&probe(512, 384, 0.0), 100, 100), (20, 63));
        // left edge, middle row: d² = 512², r = (1024²+768²)/4 = 409600 -> 100 - 64 = 36; *0.7 (zoom 50)
        assert_eq!(calc_pan_and_volume(&probe(0, 384, 50.0), 100, 100), (25, 0));
        // right edge: same volume, pan 126
        assert_eq!(calc_pan_and_volume(&probe(1023, 384, 50.0), 100, 100).1, 126);
        // corner: 1; far off-screen right/below: clamped to 0 (pan beyond 127)
        assert_eq!(calc_pan_and_volume(&probe(1023, 767, 100.0), 100, 100).0, 1);
        assert_eq!(calc_pan_and_volume(&probe(2047, 384, 100.0), 100, 100), (0, 253));
        // off-screen left: silent, pan 127; fogged: same
        assert_eq!(calc_pan_and_volume(&probe(-1, 384, 100.0), 100, 100), (0, 127));
        let mut p = probe(512, 384, 100.0);
        p.visible = false;
        assert_eq!(calc_pan_and_volume(&p, 100, 100), (0, 127));
        // sound setting 50 enters twice here: 50*120*50*100/1e6 = 30
        assert_eq!(calc_pan_and_volume(&probe(512, 384, 100.0), 50, 100).0, 30);
        // mute
        assert_eq!(calc_pan_and_volume(&probe(512, 384, 100.0), 100, 0).0, 0);
    }

    #[test]
    fn final_volume() {
        assert_eq!(fx_volume(100, 100, 100, 100), 100);
        assert_eq!(fx_volume(100, 4, 100, 100), 4); // SOUND_AMB_BIRD = 4
        assert_eq!(fx_volume(100, 100, 120, 100), 120);
        assert_eq!(fx_volume(100, 200, 120, 100), 127);
        assert_eq!(setting_volume(100, 100), 100);
    }

    #[test]
    fn playlist_never_repeats() {
        let mut p = Playlist { titles: (0..4).map(|i| (format!("t{i}"), 2500)).collect(), ..Default::default() };
        let mut r = Rand::new(7);
        for _ in 0..100 {
            let before = p.current;
            p.title_ended(r.rand());
            assert_ne!(before, p.current);
            assert!(p.current < 4);
        }
        p.set_title(9);
        assert_eq!(p.current, 0);
    }

    #[test]
    fn fades() {
        // 0 -> 100 over 2500 ms: 2500/71 = 35.2 steps of 2.84
        let mut f = Fade::new(0.0, 100, 2500);
        let mut n = 0;
        while f.advance().1 == FadeStep::Continue {
            n += 1;
        }
        assert_eq!(n, 35);
        let mut f = Fade::new(100.0, 0, 2500);
        let mut last = (0, FadeStep::Continue);
        while last.1 == FadeStep::Continue {
            last = f.advance();
        }
        assert_eq!(last, (0, FadeStep::Stop));
        // fade time 0: +inf -> cvttsd2si gives i32::MIN -> volume 0, job done
        assert_eq!(Fade::new(0.0, 100, 0).advance(), (0, FadeStep::Done));
    }

    #[test]
    fn style_director() {
        let mut d = StyleDirector::default();
        assert_eq!(d.tick(Some(false)), None);
        assert_eq!(d.tick(Some(true)), Some(MusicStyle::Dark));
        d.war_action();
        assert_eq!(d.tick(Some(true)), Some(MusicStyle::Fight));
        for _ in 0..30 {
            assert_eq!(d.tick(Some(true)), None);
        }
        // 32nd tick after the war action: back to SETTLE, then DARK again
        assert_eq!(d.tick(Some(true)), Some(MusicStyle::Settle));
        assert_eq!(d.tick(Some(true)), Some(MusicStyle::Dark));
        assert_eq!(d.tick(None), None);
    }
}
