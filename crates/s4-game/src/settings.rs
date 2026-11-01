//! User settings, persisted as TOML in the user's config directory
//! (`%APPDATA%\s4rs\settings.toml`, or `$XDG_CONFIG_HOME/s4rs` / `~/.config/s4rs`).
//! A modern replacement for the original's registry / `Config\*.cfg` values.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Settings {
    /// The plain window's size (logical px; the original's `WindowWidth` / `WindowHeight`).
    pub width: u32,
    pub height: u32,
    /// `GAMESETTINGS` `Fullscreen` (default 1: static init 0x4219bb, the shipped
    /// GameSettings.cfg): a frameless window covering its monitor, else the plain window
    /// (main.rs `window_mode`; Alt+Enter switches, `--fullscreen` / `--windowed` for one run).
    pub fullscreen: bool,
    /// Text language (`Txt\s4_texts.dat<n>`: 0 English, 1 German, …).
    pub language: u32,
    /// Game speed at the start (SIM.md §2.1).
    pub speed: u32,
    /// Optional modern effects (all off = the original's look).
    pub effects: Effects,
    /// `GAMESETTINGS` `IsAutosaveEnabled` (default 1) and `AutoSaveInterval` (minutes, default
    /// 15; release static init 0x42263a). The autosave runs when the seconds since the last save
    /// (ticks / 14) exceed `max(60 · interval, 60)` (release 0x5158c0).
    pub autosave: bool,
    pub autosave_interval: u32,
    /// The player name shown in network games (dialog 2's 2312; the original showed the
    /// Ubisoft account name there).
    pub player_name: String,
    /// `GAMESETTINGS` `BorderScroll`, `ScrollStepValue` (the border speed: 72 = the key speed,
    /// camera.rs) and `LegacyMouse` (kept in the file; the camera has one drag mode since
    /// 2026-10-23). Defaults: the HE config tool's 1 / 72 and the exe's 1.
    pub border_scroll: bool,
    pub scroll_step: i32,
    pub legacy_mouse: bool,
    /// `GAMESETTINGS` `MusicEnabled` / `MusicVolume` and `SoundFXEnabled` / `SoundFXVolume`
    /// (defaults 1, 100, 1, 100: release static init 0x42277f, 0x4226df, 0x4228bf, 0x42281f).
    /// The game start (0x5155b0) sets the music volume to the volume if enabled, else 0, and
    /// the same for effects.
    pub music_enabled: bool,
    pub music_volume: u32,
    pub sound_enabled: bool,
    pub sound_volume: u32,
    /// `GAMESETTINGS` `MsgLevelMask` (default −1, static init 0x421fa0) and `PopupAllMsgs`
    /// (default 0, 0x4224a0): which message classes show, and whether a new message opens the
    /// message window (messages.rs).
    pub msg_level_mask: u32,
    pub popup_all_msgs: bool,
    /// `GAMESETTINGS` `ExtendedTooltip` (default 1, static init 0x4221fb): `GameManager +0x144`
    /// (`SetSettings` 0x514d30), which also picks the narrower message window (msgwin.rs).
    pub extended_tooltip: bool,
    /// `GAMESETTINGS` `TextureQuality` / `Filtering` (defaults 1, 1: static init 0x421a5b /
    /// 0x421afb; the graphics page 36). `Filtering` picks the landscape's texture filter at a game's
    /// start ([`crate::options::apply_graphics`]); `TextureQuality` is kept for the page.
    pub texture_quality: u32,
    pub filtering: bool,
    /// `VoiceChatEnabled` / `VoiceChatVolume` / `VoiceChatMicro` / `MP3UserFolder` (defaults 1,
    /// 100, 100, 0: static inits 0x42297d, 0x422a1d, 0x422abd, 0x422b5d; the sound page 37).
    /// Kept for the page; no voice chat or own music here.
    pub voice_chat: bool,
    pub voice_volume: u32,
    pub voice_micro: u32,
    pub mp3_user_folder: bool,
    /// `ADVGAMESETTINGS` `ShowVideos` (default 1: static init 0x421e40; read by the release from
    /// `Config\Video.cfg`, tested `== 1` at every video, docs/VIDEO.md). Ours lives here; the
    /// install's Video.cfg is not read.
    pub show_videos: bool,
    /// [ours] The internet relay (s4-relay) as `host:port` or `ip:port` (dialog 2's 2313 in
    /// internet mode; the release's internet games went through Ubisoft's lobby servers
    /// instead); default [`DEFAULT_RELAY`], empty: none configured, so no internet games.
    /// `--relay host:port` overrides it for one run (not saved).
    pub relay_server: String,
    /// The game's install folder as chosen at the first start (s4-formats `install`: tried
    /// after `S4_DIR`, before the registry); `None` = detect.
    pub game_dir: Option<PathBuf>,
}

/// The relay server a fresh settings file names (the user's, 2026-10-24).
pub const DEFAULT_RELAY: &str = "set4.online:7720";

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Effects {
    pub ao: bool,
    pub sharpen: bool,
    pub smooth_shadows: bool,
    pub hidpi: bool,
    pub clouds: bool,
}

impl Default for Settings {
    fn default() -> Self {
        let player_name = std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_else(|_| "Player".into());
        Settings {
            width: 1280,
            height: 1024,
            fullscreen: true,
            language: 0,
            speed: 1,
            effects: Effects::default(),
            player_name,
            autosave: true,
            autosave_interval: 15,
            border_scroll: true,
            scroll_step: 72,
            legacy_mouse: true,
            music_enabled: true,
            music_volume: 100,
            sound_enabled: true,
            sound_volume: 100,
            msg_level_mask: u32::MAX,
            popup_all_msgs: false,
            extended_tooltip: true,
            texture_quality: 1,
            filtering: true,
            voice_chat: true,
            voice_volume: 100,
            voice_micro: 100,
            mp3_user_folder: false,
            show_videos: true,
            relay_server: DEFAULT_RELAY.into(),
            game_dir: None,
        }
    }
}

#[cfg(test)]
thread_local! {
    /// Tests that save settings write here instead of the user's file.
    static TEST_PATH: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// Point this thread's [`Settings::load`] / [`Settings::save`] at `p` (tests).
#[cfg(test)]
pub fn use_test_file(p: Option<PathBuf>) {
    TEST_PATH.with(|t| *t.borrow_mut() = p);
}

fn path() -> Option<PathBuf> {
    #[cfg(test)]
    if let Some(p) = TEST_PATH.with(|t| t.borrow().clone()) {
        return Some(p);
    }
    Some(user_dir()?.join("settings.toml"))
}

/// The user's folder: settings, campaign progress, `saves`, `logs` (`%APPDATA%\s4rs`, or
/// `$XDG_CONFIG_HOME/s4rs` / `~/.config/s4rs`). `S4RS_HOME` replaces it (the portable build keeps
/// everything next to itself); unset, nothing changes.
pub fn user_dir() -> Option<PathBuf> {
    if let Some(h) = std::env::var_os("S4RS_HOME") {
        return Some(h.into());
    }
    let dir = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(dir.join("s4rs"))
}

/// `--relay` for this run ([`Settings::relay`]).
static RELAY_OVERRIDE: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Use `server` as the relay for this run (main.rs `--relay`), whatever the settings say.
pub fn override_relay(server: &str) {
    let _ = RELAY_OVERRIDE.set(server.to_string());
}

impl Settings {
    /// [ours] The internet relay to use: `--relay`, else [`Settings::relay_server`]; `None`
    /// when neither names one.
    pub fn relay(&self) -> Option<String> {
        RELAY_OVERRIDE.get().cloned().or_else(|| Some(self.relay_server.trim().to_string())).filter(|s| !s.is_empty())
    }

    /// The saved settings, or the defaults (a broken file is reported and ignored).
    pub fn load() -> Settings {
        let Some(p) = path() else { return Settings::default() };
        match std::fs::read_to_string(&p) {
            Ok(s) => toml::from_str(&s).unwrap_or_else(|e| {
                tracing::warn!("{}: {e}", p.display());
                Settings::default()
            }),
            Err(_) => Settings::default(),
        }
    }

    pub fn save(&self) {
        let Some(p) = path() else { return };
        let res = p.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|_| {
            std::fs::write(&p, toml::to_string_pretty(self).map_err(|e| std::io::Error::other(e.to_string()))?)
        });
        if let Err(e) = res {
            tracing::warn!("{}: {e}", p.display());
        }
    }

    /// The effects volume the game start passes to `SetSoundVolume` (0 when disabled).
    pub fn sound_level(&self) -> i32 {
        if self.sound_enabled { self.sound_volume.min(100) as i32 } else { 0 }
    }

    /// The music volume the game start passes to `SetMusicVolume` (0 when disabled).
    pub fn music_level(&self) -> i32 {
        if self.music_enabled { self.music_volume.min(100) as i32 } else { 0 }
    }

    pub fn viewer_effects(&self) -> s4_viewer::effects::Effects {
        let e = self.effects;
        s4_viewer::effects::Effects { ao: e.ao, sharpen: e.sharpen, smooth_shadows: e.smooth_shadows, hidpi: e.hidpi, clouds: e.clouds }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_partial_files() {
        let s = Settings { width: 1600, effects: Effects { ao: true, ..Default::default() }, ..Default::default() };
        let t = toml::to_string_pretty(&s).unwrap();
        assert_eq!(toml::from_str::<Settings>(&t).unwrap(), s);
        // Missing keys take their defaults.
        let p: Settings = toml::from_str("language = 1\n[effects]\nsharpen = true\n").unwrap();
        assert_eq!((p.width, p.language, p.effects.sharpen, p.effects.ao), (1280, 1, true, false));
        // A file from before `fullscreen` existed starts borderless fullscreen (`Fullscreen` 1).
        assert!(p.fullscreen && Settings::default().fullscreen);
        assert!(!toml::from_str::<Settings>("fullscreen = false").unwrap().fullscreen);
    }
}
