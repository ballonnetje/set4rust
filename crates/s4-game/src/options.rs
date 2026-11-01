//! The settings pages' events (s4_viewer::settings_pages) as `EventManagement` (0x5172f0) handles
//! them, on this client's settings:
//! * `SetSettings` (0x514d30): the pages' record from [`Settings`] ([`record`]);
//! * `SaveSettingsManager` (0x515160): the record back into the settings, saved to the file
//!   ([`store`]);
//! * 0x1f (graphics): +0x118 / +0x120 set, saved, the page refilled; 0x20 (sound): saved,
//!   `PlayMusic` — the volumes applied; 0x21 (game): saved — `sub_276BB0` (the extended
//!   tooltips), `ToggleUISurfaceVisibility`, the camera's border scrolling and step; 0x22
//!   (messages): saved, the list refiltered (0x52a9a0: a classified message whose class no longer
//!   passes the mask goes). [not ported] 0x52a900's list capacity (`MsgHistory`, not on a page).

use s4_viewer::gpu::Gpu;
use s4_viewer::guistate::UiEvent;
use s4_viewer::settings_pages::Record;

use crate::messages::Messages;
use crate::settings::Settings;

/// The graphics settings as a game state's start applies them: every state change re-initialises
/// the engine (0x5051a0 → 0x504b30 → 0x659770 → `ReinitializeEngine` 0x65e350 → the device setup
/// 0x65c980), which sets the landscape's texture filter from `Filtering` (gfx flag 8). The graphics
/// page's event 0x1f only stores the value, so a change shows from the next game.
pub fn apply_graphics(gpu: &mut Gpu, st: &Settings) {
    use s4_viewer::terrain_gpu::Filter;
    if let Some(t) = gpu.terrain.as_mut() {
        t.filter = if st.filtering { Filter::Linear } else { Filter::Point };
    }
}

/// `SetSettings`. [inf] `AILevel` is 0: the client has no AI level setting.
pub fn record(st: &Settings) -> Record {
    Record {
        texture: st.texture_quality as i32,
        filtering: st.filtering as i32,
        sound_volume: st.sound_volume as i32,
        music_volume: st.music_volume as i32,
        voice_volume: st.voice_volume as i32,
        micro: st.voice_micro as i32,
        music_on: st.music_enabled,
        sound_on: st.sound_enabled,
        voice_on: st.voice_chat,
        mp3: st.mp3_user_folder,
        ext_tooltip: st.extended_tooltip as i32,
        border_scroll: st.border_scroll,
        scroll_step: st.scroll_step,
        popup_all: st.popup_all_msgs,
        msg_mask: st.msg_level_mask,
        ai_level: 0,
    }
}

/// `SaveSettingsManager`: the record into the settings, written to the file when `persist`
/// (the headless runs leave the user's file alone).
pub fn store(r: &Record, st: &mut Settings, persist: bool) {
    st.texture_quality = (r.texture != 0) as u32;
    st.filtering = r.filtering != 0;
    st.sound_enabled = r.sound_on;
    st.sound_volume = r.sound_volume.clamp(0, 100) as u32;
    st.music_enabled = r.music_on;
    st.music_volume = r.music_volume.clamp(0, 100) as u32;
    st.voice_chat = r.voice_on;
    st.voice_volume = r.voice_volume.clamp(0, 100) as u32;
    st.voice_micro = r.micro.clamp(0, 100) as u32;
    st.mp3_user_folder = r.mp3;
    st.extended_tooltip = r.ext_tooltip != 0;
    st.border_scroll = r.border_scroll;
    st.scroll_step = r.scroll_step;
    st.popup_all_msgs = r.popup_all;
    st.msg_level_mask = r.msg_mask;
    if persist {
        st.save();
    }
}

/// Load the settings into the pages' record (at the game start).
pub fn init(gpu: &mut Gpu, st: &Settings) {
    if let Some(ui) = gpu.ui.as_mut() {
        ui.settings.rec = record(st);
    }
}

/// What changed for the caller to apply.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Changed {
    /// 0x20: the volumes (`PlayMusic`).
    pub volumes: bool,
    /// 0x21: the extended tooltips and the camera's border scrolling / step.
    pub game: bool,
}

/// The settings pages' GUI events: the record changed by the procedures, the events handled.
pub fn ui(events: &[UiEvent], gpu: &mut Gpu, st: &mut Settings, msgs: Option<&mut Messages>, persist: bool) -> Changed {
    let mut changed = Changed::default();
    let mut msgs = msgs;
    for e in events {
        let Some(ui) = gpu.ui.as_mut() else { break };
        let Some((ev, w, l)) = ui.settings_event(e) else { continue };
        tracing::info!("settings: event {ev:#x} ({w}, {l})");
        match ev {
            0x1f => {
                if w == 0 {
                    ui.settings.rec.texture = l as i32;
                } else {
                    ui.settings.rec.filtering = l as i32;
                }
                store(&ui.settings.rec, st, persist);
                ui.settings.rec = record(st);
                ui.settings_refill();
            }
            0x20 => {
                store(&ui.settings.rec, st, persist);
                changed.volumes = true;
            }
            0x21 => {
                store(&ui.settings.rec, st, persist);
                changed.game = true;
            }
            0x22 => {
                store(&ui.settings.rec, st, persist);
                if let Some(m) = msgs.as_deref_mut() {
                    refilter(m, st.msg_level_mask);
                    m.popup_all = st.popup_all_msgs;
                }
            }
            _ => {}
        }
    }
    if changed.game
        && let Some(m) = msgs
    {
        m.popup_all = st.popup_all_msgs;
    }
    if changed != Changed::default() {
        s4_viewer::client::recompose(gpu);
    }
    changed
}

/// 0x52a9a0: the list without the classified messages whose class does not pass `mask`.
pub fn refilter(m: &mut Messages, mask: u32) {
    m.mask = mask;
    let before = m.list.len();
    let classes = m.classes.clone();
    m.list.retain(|x| !x.classified || classes.passes(x.text, mask).0);
    if m.list.len() != before {
        m.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `SetSettings` and `SaveSettingsManager` are inverse on the settings the pages show.
    #[test]
    fn record_round_trip() {
        let st = Settings { music_volume: 37, sound_enabled: false, border_scroll: false, scroll_step: 41, msg_level_mask: 0x0100_0103, filtering: false, ..Settings::default() };
        let mut back = Settings::default();
        store(&record(&st), &mut back, false);
        back.player_name = st.player_name.clone();
        assert_eq!(back, st);
    }

    /// The sound page's music toggle (0x4c2) and volume (0x4c0, code 2) through the GUI events: the
    /// settings change, 0x20 asks for the volumes.
    #[test]
    fn sound_page_events_change_the_settings() {
        if !s4_formats::game_dir().join("Menu").exists() {
            return;
        }
        let mut gpu = Gpu::headless(1280, 1024).unwrap();
        gpu.ui = Some(s4_viewer::gui::Gui::new(s4_formats::gamedata::Race::ROMAN, None, 1, 1024).unwrap());
        let mut st = Settings::default();
        init(&mut gpu, &st);
        let ev = [UiEvent::Command { dialog: 37, link: 0x4c2, effects: 0 }, UiEvent::Slider { dialog: 37, link: 0x4c0, value: 25 }];
        let ch = ui(&ev, &mut gpu, &mut st, None, false);
        assert!(ch.volumes);
        assert!(!st.music_enabled);
        assert_eq!(st.music_volume, 25);
        assert_eq!(st.music_level(), 0);
    }
}
