//! The Add-On's story picture between missions, `CStateAOSplash` (factory 0x5101c0, event handler
//! 0x510330): dialog 8 of `GUISetAOStartscreens.DAT` (opened by 0x467520 with procedure 0x46bc40,
//! init 0x46b900).
//!
//! `EndGameGoToStatistics` 0x514450 queues it after the statistics for a won mission of campaign
//! type 15 at level 1 / 3 / 6 / 8 / 9 (0x1294830 = the level) and type 12 at level 1 (−1); the
//! campaign's screen follows (`OpenMainMenu` codes 0xe / 0xd).
//! * The init: the title 0x81 hidden, the picture 0x7f and the story text (into the text scroller
//!   0x1185058 → 124) by the level, the speech `Snd\Campaign\ao_picture%d.mp3` (1..=5) or
//!   `ao_picture4V.mp3` (−1); a missing speech file shows the message box with string 0x184.
//! * Code 3: 0x7d / 0x7e scroll the text a line (0x4b0c80 / 0x4b0cb0), 0x80 → sound 11, the speech
//!   stopped, event 0x1f57: the state ends. Code 10: the wheel as 0x7d / 0x7e. Esc (event 0xb,
//!   0x1b) stops the speech and ends it too.

use s4_viewer::guistate::GuiState;

pub const DIALOG: usize = 8;
pub const TEXT: u16 = 0x7c;
pub const UP: u16 = 0x7d;
pub const DOWN: u16 = 0x7e;
pub const PICTURE: u16 = 0x7f;
pub const OK: u16 = 0x80;
pub const TITLE: u16 = 0x81;

/// The init's table by 0x1294830: (picture, story string, speech number; 0 = `ao_picture4V`).
pub fn content(param: i32) -> Option<(u16, u32, u32)> {
    Some(match param {
        1 => (0x8f, 0x1f7, 1),
        3 => (0x90, 0x1f8, 2),
        6 => (0x91, 0x1f9, 3),
        8 => (0x92, 0x1fa, 4),
        9 => (0x93, 0x1fb, 5),
        -1 => (0x92, 0x1fc, 0),
        _ => return None,
    })
}

/// The speech file of `content`'s number.
pub fn speech(game: &std::path::Path, n: u32) -> std::path::PathBuf {
    let name = if n == 0 { "ao_picture4V.mp3".to_string() } else { format!("ao_picture{n}.mp3") };
    s4_formats::path::resolve(game.join("Snd").join("Campaign").join(name))
}

/// The init 0x46b900 without the speech: title hidden, picture and story.
pub fn init(st: &mut GuiState, param: i32, story: String) {
    st.set_hidden(DIALOG, TITLE, true);
    if let Some((image, _, _)) = content(param) {
        st.images.insert((DIALOG, PICTURE), (image, 0));
        st.texts.insert((DIALOG, TEXT), story);
        st.scroll.insert((DIALOG, TEXT), 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table() {
        assert_eq!(content(9), Some((0x93, 0x1fb, 5)));
        assert_eq!(content(-1), Some((0x92, 0x1fc, 0)));
        assert_eq!(content(2), None);
        let game = s4_formats::game_dir();
        if game.join("Snd").join("Campaign").exists() {
            for n in 0..=5 {
                assert!(speech(&game, n).exists(), "{}", speech(&game, n).display());
            }
        }
    }
}
