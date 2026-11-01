//! The settings submenu's pages (dialog 7's tabs 1021..=1024 → UI events 0x1a..0x1d →
//! `EventManagement` → GUI event 0x26c with the `CStateGame` record → 0x483010 cases 0..3):
//! 36 graphics (procedure 0x4a18d0, texts 0x4a1730, fill 0x4a0cf0), 37 sound (0x4a3840, 0x4a3780,
//! `SetSoundSettingsState` 0x4a36a0), 38 game (0x4a09d0, 0x4a0900, 0x4a0840) and 39 messages
//! (`MessageSettingTabCallback` 0x4a30e0, 0x4a2f40, 0x4a1b50). Own RE; the procedures are checked
//! against unicorn runs (`tools/re/gui-2026-10-21/settings_emu.py`, `tests/settings_vectors.rs`).
//!
//! The record (`SetSettings` 0x514d30 fills it from the `GAMESETTINGS` values,
//! `SaveSettingsManager` 0x515160 writes it back): +0x118 `TextureQuality`, +0x120 `Filtering`,
//! +0x12c / +0x130 the effects / music volume, +0x134 / +0x138 the voice chat volume / microphone,
//! +0x13c..+0x13f music, effects, voice chat, `MP3UserFolder` on, +0x144 `ExtendedTooltip`, +0x148
//! `BorderScroll`, +0x14c `ScrollStepValue`, +0x150 `PopupAllMsgs`, +0x158 `MsgLevelMask`; the
//! game page shows `AILevel` (0x12987ec).
//!
//! The fills run with the page's guard set (0x15aabc4 / 0x15aabc6 / 0x15aabc3 / 0x15aabc5), so the
//! clicks they simulate send nothing. The procedures change the record and send the events that
//! `EventManagement` handles ([`Event`]); the client applies and saves them.

use crate::guistate::GuiState;

/// The record's values (see the module doc).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Record {
    pub texture: i32,
    pub filtering: i32,
    pub sound_volume: i32,
    pub music_volume: i32,
    pub voice_volume: i32,
    pub micro: i32,
    pub music_on: bool,
    pub sound_on: bool,
    pub voice_on: bool,
    pub mp3: bool,
    pub ext_tooltip: i32,
    pub border_scroll: bool,
    pub scroll_step: i32,
    pub popup_all: bool,
    pub msg_mask: u32,
    /// `AILevel` (0x12987ec), shown on the game page.
    pub ai_level: i32,
}

/// What a procedure sends `EventManagement` (event id, w, l): 0x1f (0 / 1, value) the graphics
/// settings (+0x118 / +0x120 set there, then `SaveSettingsManager`, event 0x11, `SetSettings`,
/// the page refilled), 0x20 save and `PlayMusic` (the volumes), 0x21 save, 0x22 save and the
/// message list refiltered (0x52a900, 0x52a9a0).
pub type Event = (u32, u32, u32);

/// The message page's radios and the mask bits their clicks set or clear (0x4a30e0); the fill
/// tests the same bits but 0x1100 for 0x4b1 (0x4a22ff).
const MSG_BITS: [(u16, u32, u32); 13] = [
    (0x4aa, 0x900_0000, 0x900_0000),
    (0x4ab, 0x500_0000, 0x500_0000),
    (0x4ac, 0x9_0000, 0x9_0000),
    (0x4ad, 0x5_0000, 0x5_0000),
    (0x4ae, 0x3_0000, 0x3_0000),
    (0x4af, 0x4100, 0x4100),
    (0x4b0, 0x300_0000, 0x300_0000),
    (0x4b1, 0x3100, 0x1100),
    (0x4b2, 0x900, 0x900),
    (0x4b3, 0x500, 0x500),
    (0x4b4, 0x300, 0x300),
    (0x4b5, 5, 5),
    (0x4b6, 3, 3),
];

/// The pages' state: the record and which page's fill has run.
#[derive(Clone, Debug, Default)]
pub struct Pages {
    pub rec: Record,
    filled: Option<usize>,
    guard: bool,
}

fn press(st: &mut GuiState, d: usize, link: u16, on: bool) {
    st.release_group(d, link);
    if on {
        st.press(d, link);
    }
}

impl Pages {
    /// Each frame: the init (code 0) of a settings page that has just opened.
    pub fn frame(&mut self, st: &mut GuiState, strings: &dyn Fn(u32) -> String) {
        let open = (36..=39).find(|d| st.open.contains(d));
        if open != self.filled {
            self.filled = open;
            if let Some(d) = open {
                self.init(d, st, strings);
            }
        }
    }

    /// The procedure's code 0: texts, then the fill.
    pub fn init(&mut self, d: usize, st: &mut GuiState, strings: &dyn Fn(u32) -> String) {
        let texts: &[(u16, u32)] = match d {
            36 => &[(1152, 1996), (1153, 879), (1154, 877), (1155, 878), (1156, 897), (1157, 899), (1177, 903), (1161, 898), (1162, 1724), (1163, 2124), (1158, 900), (1159, 1724), (1160, 2124), (1164, 896)],
            37 => &[(1220, 921), (1221, 920), (1222, 923), (1231, 919), (1232, 924), (1230, 922)],
            38 => &[(1140, 890), (1141, 888), (1147, 894), (1151, 895)],
            39 => &[(1180, 910), (1181, 915), (1182, 913), (1183, 904), (1184, 918), (1185, 909), (1186, 908), (1188, 906), (1189, 911), (1190, 912), (1191, 914), (1192, 916), (1193, 781), (1187, 917)],
            _ => return,
        };
        for &(link, id) in texts {
            st.texts.insert((d, link), strings(id));
        }
        self.fill(d, st, strings);
    }

    /// The fill (0x4a0cf0 / 0x4a36a0 / 0x4a0900 + 0x4a0840 / 0x4a1b50).
    pub fn fill(&mut self, d: usize, st: &mut GuiState, strings: &dyn Fn(u32) -> String) {
        self.guard = true;
        let r = self.rec.clone();
        match d {
            36 => {
                for l in [0x499, 0x485, 0x484, 0x490, 0x491, 0x489, 0x48a, 0x495, 0x486, 0x487, 0x493, 0x48c, 0x496] {
                    st.set_greyed(36, l, false);
                }
                // Resolution, the software choices and their texts hidden; the hardware radios
                // clicked (`UIEventMethod`).
                for l in [0x48b, 0x494, 0x488, 0x492, 0x480, 0x483, 0x48f, 0x482, 0x48e, 0x481, 0x48d] {
                    st.set_hidden(36, l, true);
                }
                press(st, 36, 0x495, true);
                press(st, 36, 0x493, true);
                press(st, 36, if r.texture == 0 { 0x490 } else { 0x491 }, true);
                press(st, 36, 0x496, r.filtering != 0);
            }
            37 => {
                press(st, 37, 0x4c2, r.music_on);
                press(st, 37, 0x4c3, r.sound_on);
                press(st, 37, 0x4c9, r.voice_on);
                for (l, v) in [(0x4c0, r.music_volume), (0x4c1, r.sound_volume), (0x4c8, r.voice_volume), (0x4cd, r.micro)] {
                    st.set_slider(37, l, v as u32);
                }
                press(st, 37, 0x4d1, r.mp3);
            }
            38 => {
                st.texts.insert((38, 0x475), strings(888));
                st.set_greyed(38, 0x47d, !r.border_scroll);
                st.set_greyed(38, 0x47c, !r.border_scroll);
                press(st, 38, 0x473, r.ext_tooltip != 0);
                press(st, 38, 0x47a, r.border_scroll);
                press(st, 38, 0x47e, r.popup_all);
                st.set_slider(38, 0x47d, r.scroll_step as u32);
                st.texts.insert((38, 0x476), strings(if r.ai_level == 0 { 889 } else { 891 }));
            }
            39 => {
                for &(link, _, test) in &MSG_BITS {
                    press(st, 39, link, r.msg_mask & test != 0);
                }
            }
            _ => {}
        }
        self.guard = false;
    }

    /// Code 3 on `link` of page `d` with the control's `effects`: the record changed, the event.
    pub fn command(&mut self, st: &mut GuiState, d: usize, link: u16, effects: u8) -> Option<Event> {
        if self.guard {
            return None;
        }
        let on = effects & 1 != 0;
        let r = &mut self.rec;
        match (d, link) {
            (36, 0x490) => (r.texture != 0).then_some((0x1f, 0, 0)),
            (36, 0x491) => (r.texture != 1).then_some((0x1f, 0, 1)),
            (36, 0x496) => Some((0x1f, 1, on as u32)),
            (37, 0x4c0 | 0x4c1 | 0x4c8 | 0x4cd) => Some((0x20, 0, 0)),
            (37, 0x4c2) => {
                r.music_on = on;
                Some((0x20, 0, 0))
            }
            (37, 0x4c3) => {
                r.sound_on = on;
                Some((0x20, 0, 0))
            }
            (37, 0x4c9) => {
                r.voice_on = on;
                Some((0x20, 0, 0))
            }
            (37, 0x4d1) => {
                r.mp3 = on;
                Some((0x20, 0, 0))
            }
            (38, 0x473) => {
                r.ext_tooltip = on as i32;
                Some((0x21, 0, 0))
            }
            (38, 0x47a) => {
                // The scroll speed slider (area and knob) greyed with it.
                r.border_scroll = on;
                st.set_greyed(38, 0x47d, !on);
                st.set_greyed(38, 0x47c, !on);
                Some((0x21, 0, 0))
            }
            (38, 0x47d) => Some((0x21, 0, 0)),
            (38, 0x47e) => {
                r.popup_all = on;
                Some((0x21, 0, 0))
            }
            (39, _) => {
                let &(_, bits, _) = MSG_BITS.iter().find(|m| m.0 == link)?;
                r.msg_mask = if on { r.msg_mask | bits } else { r.msg_mask & !bits };
                Some((0x22, 0, 0))
            }
            _ => None,
        }
    }

    /// Code 2 (a slider's value while it moves): sound page volumes (then event 0x20), the game
    /// page's scroll speed (no event).
    pub fn slider(&mut self, d: usize, link: u16, value: i32) -> Option<Event> {
        if self.guard {
            return None;
        }
        let r = &mut self.rec;
        match (d, link) {
            (37, 0x4c0) => r.music_volume = value,
            (37, 0x4c1) => r.sound_volume = value,
            (37, 0x4c8) => r.voice_volume = value,
            (37, 0x4cd) => r.micro = value,
            (38, 0x47d) => {
                r.scroll_step = value;
                return None;
            }
            _ => return None,
        }
        Some((0x20, 0, 0))
    }
}
