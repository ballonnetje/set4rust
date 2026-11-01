//! The credits (`CStateCredits`, main menu button 2550 → event 49 → state 0x5118a0), as the
//! shipped exe runs them (own RE; `tools/re/gui-2026-10-24/credits*.c`, `misc_procs.c`).
//!
//! * The state's constructor (0x511a00 → 0x511cf0) starts music track 5 and opens the
//!   start-screen dialogs 14 (the background, procedure 0x47d6b0), 15, 16 and 17 (the moving
//!   panel with the texts 0x81c heading, 0x81d / 0x81e the names); it empties 0x81c / 0x81e and
//!   shows the first entry (0x47d0d0 with the entry −1).
//! * Every shown frame (0x511c90: frames at least 16 ms apart) runs 0x47d5c0 once ([`Credits::step`]):
//!   a pause counts down; else the panel (y = `pos`) moves by `(int)speed · dir` px, the speed 20
//!   for the first 14 frames after a turn, then 1.3 less a frame down to 1; it turns at the top
//!   (pos 1) or wherever 100 frames have passed (pausing half the entry's frames), and at the
//!   bottom (0x1b8: a pause of 20 frames and the next entry).
//! * 0x47d330 places dialog 16 at (w · 0.125, (pos + 20) / 600 · h), 17 at (w · 0.15375,
//!   (pos + 74) / 600 · h) and 15 at (w · 0.10875, h · 0.85) of dialog 14's w × h.
//! * The entries (.data 0x11637b0, 0x9e of {line 1, line 2, heading, pause}): MCD2's 0..0x17,
//!   the Add-On's 0x18..0x3e, the Mission CD's 0x3f..0x64, the game's 0x65..0x9d, the ones of
//!   packs not installed skipped (0x12947b0 / 0x12947ac / `GlobalMissionCD` vtbl+4); the History
//!   Edition has all three.
//! * A mouse button (events 8 / 10) or Esc (key event 0xb, 0x1b) → event 0x6d: the main menu.

use std::sync::LazyLock;

use s4_viewer::guistate::GuiState;

/// The dialogs: the background, then in their surfaces' order the panel (1), the top rod (2)
/// and the bottom roll (3).
pub const BACKGROUND: usize = 14;
pub const DIALOGS: [usize; 4] = [14, 17, 16, 15];

/// .data 0x11637b0: {line 1, line 2, heading, pause frames} (string ids; < 1 = none), read from
/// the player's exe.
static ENTRIES: LazyLock<Vec<(i32, i32, i32, i32)>> = LazyLock::new(|| {
    let bytes = s4_formats::exe::table(0x11637b0, 0x9e * 16).unwrap_or_else(|| vec![0; 0x9e * 16]);
    let int = |b: &[u8]| i32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    bytes.as_chunks::<16>().0.iter().map(|e| (int(&e[0..]), int(&e[4..]), int(&e[8..]), int(&e[12..]))).collect()
});

/// The credits' globals (0x15aab90.., 0x11feee0).
#[derive(Clone, Debug, PartialEq)]
pub struct Credits {
    /// 0x15aab90: the panel's y (1..=0x1b8); 0x15aab94: its direction (±2); 0x15aab98: frames
    /// since the last turn; 0x15aab9c: the turn limit 100; 0x15aaba0: the pause; 0x15aaba4: the
    /// entry.
    pub pos: i32,
    pub dir: i32,
    pub counter: i32,
    pub limit: i32,
    pub pause: i32,
    pub entry: i32,
    /// 0x11feee0 (f32, 20 at the start).
    pub speed: f32,
}

impl Default for Credits {
    fn default() -> Self {
        Credits { pos: 0x1b8, dir: -2, counter: 0, limit: 100, pause: 0x3c, entry: 0, speed: 20.0 }
    }
}

impl Credits {
    /// 0x511cf0 after the dialogs are open: dialog 14's init (0x47d6b0 code 0: the pause 60,
    /// the panel at the bottom moving up, the limit 100; 0x47d330), then 0x81c / 0x81e emptied
    /// and the first entry (0x47d0d0 from −1).
    pub fn open(st: &mut GuiState, strings: &dyn Fn(u32) -> String) -> Credits {
        let mut ev = Vec::new();
        // [inf] Opened in their surfaces' order so the rods cover the parchment as the surfaces
        // stack.
        for d in DIALOGS {
            st.open(d, &mut ev);
        }
        let mut c = Credits::default();
        c.layout(st);
        st.texts.insert((17, 0x81c), String::new());
        st.texts.insert((17, 0x81e), String::new());
        c.entry = -1;
        c.next(st, strings);
        c
    }

    /// 0x47d0d0: the next entry (wrapping to the first of the installed packs at 0x9e) and its
    /// texts: 0x81d line 1 in font 0xf (or "" in font 0x10), 0x81e line 2, 0x81c the heading.
    pub fn next(&mut self, st: &mut GuiState, strings: &dyn Fn(u32) -> String) {
        // [inf] All three packs present (the History Edition): the first entry is 0.
        let first = 0;
        self.entry = if self.entry < 0 { first } else { self.entry + 1 };
        // With the Mission CD installed its first entry follows the Add-On's last but one: 0x3e
        // is never shown.
        if self.entry == 0x3e {
            self.entry = 0x3f;
        } else if self.entry > 0x9d {
            self.entry = first;
        }
        let (a, b, heading, _) = ENTRIES[self.entry as usize];
        let line = |id: i32| if id < 1 { String::new() } else { strings(id as u32) };
        st.fonts.insert((17, 0x81d), if a < 1 { 0x10 } else { 0xf });
        st.texts.insert((17, 0x81d), line(a));
        st.texts.insert((17, 0x81e), line(b));
        st.texts.insert((17, 0x81c), strings(heading as u32));
    }

    /// 0x47d5c0: one frame.
    pub fn step(&mut self, st: &mut GuiState, strings: &dyn Fn(u32) -> String) {
        if self.pause > 0 {
            self.pause -= 1;
            self.layout(st);
            return;
        }
        self.counter += 1;
        if self.counter < 0xf {
            self.speed = 20.0;
        } else {
            self.speed -= 1.3;
            if self.speed < 1.0 {
                self.speed = 1.0;
            }
        }
        self.pos += self.speed as i32 * self.dir;
        let turn = if self.pos < 1 {
            self.pos = 1;
            self.counter >= self.limit
        } else if self.pos < 0x1b9 {
            self.counter >= self.limit
        } else {
            self.pos = 0x1b8;
            true
        };
        if turn {
            self.dir = -self.dir;
            self.counter = 0;
            self.pause = ENTRIES.get(self.entry as usize).map_or(0, |e| e.3 >> 1);
            if self.pos > 0x1b7 {
                self.pause = 0x14;
                self.next(st, strings);
            }
        }
        self.layout(st);
    }

    /// 0x47d330: the dialogs' places on dialog 14's rectangle (f32 with +0.5, truncated).
    pub fn layout(&self, st: &mut GuiState) {
        let Some(bg) = st.set.dialog(BACKGROUND) else { return };
        let (w, h) = (bg.width as f32, bg.height as f32);
        let at = |v: f32| (v + 0.5) as i32;
        let place = [
            (16, at(w * 0.125), at((self.pos + 0x14) as f32 / 600.0 * h)),
            (17, at(w * 0.15375), at((self.pos + 0x4a) as f32 / 600.0 * h)),
            (15, at(w * 0.10875), at(h * 0.85)),
        ];
        for (d, x, y) in place {
            if let Some(dlg) = st.set.dialogs.get_mut(d).and_then(|d| d.as_mut()) {
                dlg.x = x.clamp(0, u16::MAX as i32) as u16;
                dlg.y = y.clamp(0, u16::MAX as i32) as u16;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The panel's path: 60 frames still, up at 40 px a frame (the first 14 frames), held at
    /// the top until frame 100 since the turn, then down, the next entry at the bottom.
    #[test]
    fn panel_path() {
        if !s4_formats::game_dir().join("Gfx").exists() {
            return;
        }
        let mut c = Credits::default();
        let mut st = s4_viewer::gui::Gui::from_set(super::super::SET, 600, &[]).unwrap().state;
        let strings = |id: u32| format!("#{id}");
        for _ in 0..60 {
            c.step(&mut st, &strings);
        }
        assert_eq!((c.pos, c.pause), (0x1b8, 0));
        c.step(&mut st, &strings);
        assert_eq!(c.pos, 0x1b8 - 40);
        for _ in 0..10 {
            c.step(&mut st, &strings);
        }
        assert_eq!(c.pos, 1, "at the top after 11 frames");
        while c.dir < 0 {
            c.step(&mut st, &strings);
        }
        assert_eq!((c.counter, c.pause), (0, ENTRIES[0].3 >> 1));
        let entry = c.entry;
        while c.entry == entry {
            c.step(&mut st, &strings);
        }
        assert_eq!((c.entry, c.pos, c.pause, c.dir), (entry + 1, 0x1b8, 0x14, -2));
        assert_eq!(st.texts.get(&(17, 0x81c)).cloned(), Some(format!("#{}", ENTRIES[1].2)));
    }
}
