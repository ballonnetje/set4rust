//! The clock, dialog 74 (procedure `DrawClockCallback` 0x474a80, text `DrawClock` 0x474960), as
//! the shipped exe runs it (own RE; `tools/re/gui-2026-10-23/clock.c`, `guievh.c`).
//!
//! * GUI event 0x273 (`CGuiEventHandler` 0x4805a0): unless open (+0x3c), `OpenDialog(0x4a,
//!   DrawClockCallback)` and `ToggleUISurfaceVisibility` (0x488650) puts it in the top right
//!   corner: x = W − 1 − width, y = 0. Its init (code 0): the countdown off and its seconds 0
//!   (0x15aa5c4 / 0x15aa5c0), the text control 0x8b's font 7. Event 0x274 closes it.
//! * Senders: `Game.ShowClock(seconds)` (0x5c7920): event 0x273 (dispatched at once), then the
//!   countdown on for seconds ≠ 0 with those seconds, and the same flag at `GUIEventHandler +0x42`;
//!   F7 (`HandleF6_F7_Esc` 0x489440, key 0x76): nothing while +0x42 is set, else 0x274 when open,
//!   0x273 when closed (then countdown off, seconds 0, +0x42 clear).
//! * The 100 ms timer event 0x15: while open, when the game tick (`GlobalGameManager +0x30`) is
//!   more than 13 past the last drawn one (+8), `DrawClock(tick)` ([`text`]) into 0x8b.

use crate::guistate::GuiState;

/// The dialog slot.
pub const DIALOG: usize = 74;
/// The text control.
pub const LINK: u16 = 0x8b;
/// Seconds per game tick (.rdata 0x1070d90, f32).
const SECONDS_PER_TICK: f32 = 0.071;

/// The clock's globals.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Clock {
    /// 0x15aa5c4: a countdown, not the game time.
    pub countdown: bool,
    /// 0x15aa5c0: the countdown's end, seconds of game time.
    pub seconds: i32,
    /// `GUIEventHandler +0x42`: a script's countdown, which F7 leaves alone.
    pub locked: bool,
    /// `GUIEventHandler +8`: the tick last drawn.
    pub last: u32,
}

/// `DrawClock(tick)` (SSE single precision): the game time `tick · 0.071` s as "hh:mm:ss", or for
/// a countdown the seconds left `max(0, seconds − tick · 0.071)` (truncated), below 60 as "%d"
/// with font 2 (set on the control and kept until the next init).
pub fn text(tick: u32, countdown: bool, seconds: i32) -> (String, Option<u8>) {
    let t = tick as i32 as f32 * SECONDS_PER_TICK;
    let s = if countdown {
        let left = (seconds as f32 - t) as i32;
        let left = left.max(0);
        if left < 0x3c {
            return (format!("{left}"), Some(2));
        }
        left
    } else {
        t as i32
    };
    (format!("{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60), None)
}

impl Clock {
    /// Event 0x273 on a `screen_w` × `screen_h` panel: opened unless it is (init: countdown off,
    /// font 7).
    pub fn open(&mut self, st: &mut GuiState, screen_w: u32) {
        if st.open.contains(&DIALOG) {
            return;
        }
        if let Some(d) = st.set.dialogs.get_mut(DIALOG).and_then(|d| d.as_mut()) {
            d.x = (screen_w as i32 - 1 - d.width as i32).max(0) as u16;
            d.y = 0;
        }
        let mut ev = Vec::new();
        st.open(DIALOG, &mut ev);
        self.countdown = false;
        self.seconds = 0;
        st.fonts.insert((DIALOG, LINK), 7);
    }

    /// Event 0x274.
    pub fn close(&mut self, st: &mut GuiState) {
        let mut ev = Vec::new();
        st.close(DIALOG, &mut ev);
    }

    /// `Game.ShowClock(seconds)` (0x5c7920).
    pub fn show(&mut self, st: &mut GuiState, screen_w: u32, seconds: i32) {
        self.open(st, screen_w);
        self.countdown = seconds != 0;
        self.seconds = seconds;
        self.locked = self.countdown;
    }

    /// F7 (0x48948f).
    pub fn key(&mut self, st: &mut GuiState, screen_w: u32) {
        if self.locked {
            return;
        }
        if st.open.contains(&DIALOG) {
            self.close(st);
        } else {
            self.open(st, screen_w);
            self.countdown = false;
            self.seconds = 0;
            self.locked = false;
        }
    }

    /// The 100 ms timer event 0x15 at game tick `tick`. Returns whether the text changed.
    pub fn timer(&mut self, st: &mut GuiState, tick: u32) -> bool {
        if !st.open.contains(&DIALOG) || tick.wrapping_sub(self.last) <= 0xd {
            return false;
        }
        self.last = tick;
        let (t, font) = text(tick, self.countdown, self.seconds);
        if let Some(f) = font {
            st.fonts.insert((DIALOG, LINK), f);
        }
        st.texts.insert((DIALOG, LINK), t.clone()) != Some(t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_texts() {
        assert_eq!(text(0, false, 0), ("00:00:00".into(), None));
        // 14 ticks ≈ 0.994 s: still 0.
        assert_eq!(text(14, false, 0).0, "00:00:00");
        assert_eq!(text(15, false, 0).0, "00:00:01");
        assert_eq!(text(50_704, false, 0).0, "00:59:59");
        assert_eq!(text(50_705, false, 0).0, "01:00:00");
        // Countdown to 5400 s (dark04): from 60 s left the plain seconds with font 2.
        assert_eq!(text(0, true, 5400), ("01:30:00".into(), None));
        assert_eq!(text(75_211, true, 5400), ("00:01:00".into(), None));
        assert_eq!(text(75_212, true, 5400), ("59".into(), Some(2)));
        assert_eq!(text(80_000, true, 5400), ("0".into(), Some(2)));
    }
}
