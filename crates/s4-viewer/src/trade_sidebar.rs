//! D53, the trading building's "goods to send" sidebar (info code 0xe, info object 0x11aae6c),
//! as the shipped exe runs it (own RE; `tools/re/gui-2026-10-21/d53_emu.py` runs the original
//! functions in unicorn, `tests/d53_vectors.rs` checks this port against them).
//!
//! * Fill (`CTradingBuildingRole` 0x553300, through the provider 0x53d7a0 that the handler of UI
//!   event 0x1c7, 0x4ebb70, registers): +8 + good = the amount still to send (+0x1ab) of each
//!   good whose flag byte (+0x1d6) has bit 1, else 0 ([`crate::building_dialog::sidebar_fill`]).
//! * The procedure 0x477950: init (code 0) builds the per-good records (0x4773a0: .bss 0x15aa600 +
//!   16 · good = {group, text link, tab link, value}, by the local race) and selects the first
//!   group tab 0x338. Code 3: the group tabs 0x338..0x33b set the group (0x15aa8b4: 0x338 → 0,
//!   0x339 → 1, 0x33a → 2, 0x33b → 3 while pressed, else 0), lay out its good tabs (0x476b90),
//!   select the first (0x33d) and set the good (0x15aa8b0) to the group's first (22, 9, 18, 34),
//!   then run the update; a good tab 0x33d..0x345 sets the good of its record in the group
//!   (0x477910) or 0 when released; the buttons 0x325..0x32a send UI event 0x1d1 (good, 100 / 0 /
//!   5 / −5 / 1 / −1) when a good is chosen. 0x1d1's handler 0x4eb0f0 sends net 0x138e = 5006
//!   `TransportGood` (selected building << 16 | good, amount).
//! * The update 0x476e90 (0x488e00 case 0xe): per good 1..=42 with an amount (any non-zero
//!   byte) its group is marked; in the shown group the good's text is "0" (below 1), "%u" (below
//!   100) or "E" and shown, else hidden; the four group markers 818..821 show the groups with
//!   amounts.
//!
//! Quirks kept: the init resets only the records' tab link (to −1, goods 0..=41), so a good the
//! race's table leaves out stays in group 0 with text link 0: an amount for it marks group 0.

use crate::guistate::GuiState;

/// The dialog slot.
pub const DIALOG: usize = 53;

/// The records 0x4773a0 writes, per race: (good, group, text link, tab link).
const RECORDS: [&[(u8, u8, u16, u16)]; 5] = [
    // race 0 (also the Dark Tribe, 3: the init's default branch)
    &[(2, 3, 822, 836), (3, 3, 814, 832), (4, 2, 813, 831), (7, 0, 812, 830), (8, 3, 812, 830), (9, 1, 811, 829), (10, 0, 822, 836), (11, 1, 812, 830), (12, 1, 814, 832), (14, 0, 814, 832), (15, 0, 817, 835), (16, 1, 815, 833), (18, 2, 811, 829), (20, 0, 815, 833), (21, 0, 816, 834), (22, 0, 811, 829), (24, 1, 813, 831), (25, 2, 816, 834), (27, 2, 817, 835), (28, 2, 814, 832), (29, 2, 815, 833), (31, 2, 812, 830), (32, 0, 813, 831), (33, 3, 817, 835), (34, 3, 811, 829), (36, 1, 816, 834), (37, 1, 817, 835)],
    // race 1
    &[(3, 3, 814, 832), (4, 2, 813, 831), (5, 3, 813, 831), (7, 0, 812, 830), (8, 3, 812, 830), (9, 1, 811, 829), (10, 0, 822, 836), (11, 1, 812, 830), (12, 1, 814, 832), (14, 0, 814, 832), (15, 0, 817, 835), (16, 1, 815, 833), (18, 2, 811, 829), (19, 1, 822, 836), (20, 0, 815, 833), (21, 0, 816, 834), (22, 0, 811, 829), (23, 1, 817, 835), (24, 1, 813, 831), (25, 2, 816, 834), (27, 2, 817, 835), (28, 2, 814, 832), (29, 2, 815, 833), (31, 2, 812, 830), (32, 0, 813, 831), (33, 3, 817, 835), (34, 3, 811, 829), (36, 1, 816, 834)],
    // race 2
    &[(1, 1, 822, 836), (3, 3, 814, 832), (4, 2, 813, 831), (6, 3, 813, 831), (7, 0, 812, 830), (8, 3, 812, 830), (9, 1, 811, 829), (10, 0, 822, 836), (11, 1, 812, 830), (12, 1, 814, 832), (14, 0, 814, 832), (15, 0, 817, 835), (16, 1, 815, 833), (17, 3, 822, 836), (18, 2, 811, 829), (20, 0, 815, 833), (21, 0, 816, 834), (22, 0, 811, 829), (24, 1, 813, 831), (25, 2, 816, 834), (27, 2, 817, 835), (28, 2, 814, 832), (29, 2, 815, 833), (31, 2, 812, 830), (32, 0, 813, 831), (33, 3, 817, 835), (34, 3, 811, 829), (35, 1, 817, 835), (36, 1, 816, 834)],
    // race 3
    &[(2, 3, 822, 836), (3, 3, 814, 832), (4, 2, 813, 831), (7, 0, 812, 830), (8, 3, 812, 830), (9, 1, 811, 829), (10, 0, 822, 836), (11, 1, 812, 830), (12, 1, 814, 832), (14, 0, 814, 832), (15, 0, 817, 835), (16, 1, 815, 833), (18, 2, 811, 829), (20, 0, 815, 833), (21, 0, 816, 834), (22, 0, 811, 829), (24, 1, 813, 831), (25, 2, 816, 834), (27, 2, 817, 835), (28, 2, 814, 832), (29, 2, 815, 833), (31, 2, 812, 830), (32, 0, 813, 831), (33, 3, 817, 835), (34, 3, 811, 829), (36, 1, 816, 834), (37, 1, 817, 835)],
    // race 4
    &[(3, 3, 814, 832), (4, 2, 813, 831), (7, 0, 812, 830), (8, 3, 812, 830), (9, 1, 811, 829), (10, 0, 822, 836), (11, 1, 812, 830), (12, 1, 814, 832), (14, 0, 814, 832), (15, 0, 817, 835), (16, 1, 815, 833), (18, 2, 811, 829), (20, 0, 815, 833), (21, 0, 816, 834), (22, 0, 811, 829), (24, 1, 813, 831), (25, 2, 816, 834), (27, 2, 817, 835), (28, 2, 814, 832), (29, 2, 815, 833), (31, 2, 812, 830), (32, 0, 813, 831), (33, 3, 817, 835), (34, 3, 811, 829), (36, 1, 816, 834), (38, 3, 813, 831), (40, 3, 822, 836), (41, 1, 817, 835), (42, 1, 822, 836)],
];

/// 0x476b90 hides these first: the nine good texts and the nine good tabs.
const TEXTS: [u16; 9] = [811, 812, 813, 814, 815, 816, 817, 822, 823];
const TABS: [u16; 9] = [829, 830, 831, 832, 833, 834, 835, 836, 837];
/// The group markers (0x15aa5e0 + 8 · group drives them).
const MARKERS: [u16; 4] = [818, 819, 820, 821];
/// The group tabs 0x338..0x33b and the good a group starts with.
const GROUPS: [(u16, i32); 4] = [(0x338, 22), (0x339, 9), (0x33a, 18), (0x33b, 34)];
/// The buttons 0x325..0x32a: the amount of UI event 0x1d1.
const AMOUNTS: [i32; 6] = [100, 0, 5, -5, 1, -1];

#[derive(Clone, Copy, Debug, Default)]
struct Record {
    group: i32,
    text: u16,
    /// −1 = none.
    tab: i32,
}

/// The procedure's globals.
#[derive(Clone, Debug)]
pub struct TradeSend {
    /// 0x15aa8b4, 0x15aa8b0.
    pub group: i32,
    pub good: i32,
    recs: [Record; 43],
}

impl Default for TradeSend {
    fn default() -> Self {
        TradeSend { group: 0, good: 0, recs: [Record::default(); 43] }
    }
}

fn hide(st: &mut GuiState, link: u16, hidden: bool) {
    st.set_hidden(DIALOG, link, hidden);
}

impl TradeSend {
    /// The procedure's code 0 (0x477950 → 0x4773a0, then `SelectControl(53, 0x338)`); `info` is
    /// the info object (0x11aae6c), which the group tab's update reads.
    pub fn init(&mut self, st: &mut GuiState, info: &[u8], race: u8) {
        for r in self.recs.iter_mut().take(42) {
            r.tab = -1;
        }
        for &(good, group, text, tab) in RECORDS[(race as usize).min(4)] {
            self.recs[good as usize] = Record { group: group as i32, text, tab: tab as i32 };
        }
        self.select(st, info, 0x338);
    }

    /// `IGuiEngine::SelectControl(53, link, 1)`: when the tab is not pressed yet, the engine
    /// presses it and runs the procedure's code 3 inside the call.
    fn select(&mut self, st: &mut GuiState, info: &[u8], link: u16) {
        let mut ev = Vec::new();
        st.select(DIALOG, link, &mut ev);
        if let Some(&crate::guistate::UiEvent::Command { effects, .. }) = ev.iter().find(|e| matches!(e, crate::guistate::UiEvent::Command { dialog: DIALOG, link: l, .. } if *l == link)) {
            self.click(st, info, link, effects);
        }
    }

    /// The procedure's code 3 for `link` with the control's `effects` after the click: the GUI
    /// event it sends (0x1d1, good, amount).
    pub fn click(&mut self, st: &mut GuiState, info: &[u8], link: u16, effects: u8) -> Option<(u16, u32, u32)> {
        let pressed = (effects & 1) as i32;
        match link {
            0x325..=0x32a => (self.good != 0).then(|| (0x1d1, self.good as u32, AMOUNTS[(link - 0x325) as usize] as u32)),
            0x338..=0x33b => {
                let k = (link - 0x338) as usize;
                self.group = match k {
                    0 => 0,
                    1 => pressed,
                    2 => pressed * 2,
                    _ => pressed * 3,
                };
                self.lay_out(st);
                self.select(st, info, 0x33d);
                self.good = GROUPS[k].1;
                self.update(st, info);
                None
            }
            0x33d..=0x345 => {
                // 0x477910: the record of the group whose tab is `link`, numbered from 1.
                self.good = if pressed != 0 { (1..43).find(|&g| self.recs[g].group == self.group && self.recs[g].tab == link as i32).unwrap_or(0) as i32 } else { 0 };
                None
            }
            _ => None,
        }
    }

    /// 0x476b90: the group's good tabs: all texts and tabs hidden, then per good of the group its
    /// text emptied and shown, its tab shown with the good's images and tooltips (.rdata
    /// 0xffcb98 + 20 · (good − 1)).
    fn lay_out(&self, st: &mut GuiState) {
        for (&t, &b) in TEXTS.iter().zip(&TABS) {
            hide(st, t, true);
            hide(st, b, true);
        }
        for g in 1..43 {
            let r = self.recs[g];
            if r.group != self.group {
                continue;
            }
            st.texts.insert((DIALOG, r.text), String::new());
            hide(st, r.text, false);
            let Ok(tab) = u16::try_from(r.tab) else { continue };
            hide(st, tab, false);
            let (img, pressed, tip, ext) = crate::gui_pages::TRANSPORT_GOOD[g];
            st.images.insert((DIALOG, tab), (img, pressed));
            st.tooltips.insert((DIALOG, tab), (tip, ext));
        }
    }

    /// The update 0x476e90 with the info object's bytes (+8 + good).
    pub fn update(&mut self, st: &mut GuiState, info: &[u8]) {
        let mut marked = [false; 4];
        for g in 1..43 {
            let v = info.get(8 + g).copied().unwrap_or(0) as i8;
            let r = self.recs[g];
            if v != 0
                && let Some(m) = marked.get_mut(r.group as usize)
            {
                *m = true;
            }
            if r.group == self.group {
                if v != 0 {
                    let t = if v < 1 {
                        "0".to_string()
                    } else if v < 100 {
                        v.to_string()
                    } else {
                        "E".to_string()
                    };
                    st.texts.insert((DIALOG, r.text), t);
                    hide(st, r.text, false);
                } else {
                    hide(st, r.text, true);
                }
            }
        }
        for (&m, &link) in marked.iter().zip(&MARKERS) {
            hide(st, link, !m);
        }
    }
}

// ------------------------------------------------------------------------------------------------
// D54

/// D54, the trading building's foreign trade sidebar (info code 0xd, info object 0x11aae30;
/// `tools/re/gui-2026-10-21/d54_emu.py`, `tests/d54_vectors.rs`).
///
/// * Fill (0x5531b0 through the provider 0x53d7c0, UI event 0x1c8 → 0x4ebbc0; only with the
///   role's `CInternationalTrader` handle +0x2a4, which every trading role has): +8..+0x10
///   cleared, +8 + p (p = 1..=`PlayerCount`) the owner's "trades with p" (+0x14 + p), +0x11 + good
///   the building's trader record state (0 / 4 import / 8 export; 0 without a record).
/// * The procedure 0x478d50: init (0x4786e0) builds the records (0x15aa8d8 + 16 · good = {group,
///   mark link, tab link, state}; the same goods and groups as D53's, marks 854..861 where D53 has
///   its texts, tabs 864..871) and selects group tab 0x34a. Code 3: the group tabs 0x34a..0x34d set
///   the group (0x15aab88, as D53's), lay the goods out (0x4783d0: marks and tabs hidden, the
///   group's shown with the good's images and tooltips from .rdata 0xffcf44) and run the update;
///   the player radios 0x34e..0x355 (players 8..1) send UI event 0x1e3 (player, pressed) unless
///   an update runs (0x15aa5c5); a good tab 0x360..0x368 sends 0x1d3 (the good of its record in
///   the group, 0x478c50). 0x1e3 → 0x4eb300 → net 0x13ba = 5050 (selected << 16 | player,
///   pressed); 0x1d3 → 0x4eb250 → 5008 (selected, good).
/// * The update 0x477d50: per good 1..=42 with a state (a signed byte ≥ 1) its group is marked; in
///   the shown group a good with a state gets mark image 0xa1 (bit 4, import) or 0xa0 (exactly 8,
///   export) and its mark shown, else hidden; the group markers 838..841; per player radio (853 −
///   i for player i + 1) below `PlayerCount` and not the local player: the colour's images
///   (.rdata 0xffcf00 by `PlayerColorIndex`: +4 image, +0 pressed), pressed as +8 + p, shown; the
///   others hidden.
#[derive(Clone, Debug)]
pub struct TradeForeign {
    /// 0x15aab88.
    pub group: i32,
    recs: [Record; 43],
    updating: bool,
}

impl Default for TradeForeign {
    fn default() -> Self {
        TradeForeign { group: 0, recs: [Record::default(); 43], updating: false }
    }
}

/// D54's dialog slot.
pub const FOREIGN_DIALOG: usize = 54;
const MARKS: [u16; 9] = [854, 855, 856, 857, 858, 859, 860, 861, 862];
const FOREIGN_TABS: [u16; 9] = [864, 865, 866, 867, 868, 869, 870, 871, 872];
const FOREIGN_MARKERS: [u16; 4] = [838, 839, 840, 841];
/// .rdata 0xffcf00: (pressed image, image) per player colour.
const COLOURS: [(u16, u16); 8] = [(154, 153), (152, 151), (150, 149), (148, 147), (146, 145), (144, 143), (142, 141), (140, 136)];

/// What D54's update reads besides its info object: `PlayerCount`, the local player and
/// `PlayerColorIndex` of players 1..=8.
#[derive(Clone, Copy, Debug, Default)]
pub struct Players {
    pub count: u8,
    pub local: u8,
    pub colours: [u8; 8],
}

impl TradeForeign {
    /// The procedure's code 0 (0x4786e0, then `SelectControl(54, 0x34a)`, whose code 3 runs
    /// inside it); `info` is the info object (0x11aae30).
    pub fn init(&mut self, st: &mut GuiState, info: &[u8], race: u8, players: &Players) {
        for r in self.recs.iter_mut().take(42) {
            r.tab = -1;
        }
        for &(good, group, text, tab) in RECORDS[(race as usize).min(4)] {
            let mark = if text == 822 { 861 } else { text + 43 };
            self.recs[good as usize] = Record { group: group as i32, text: mark, tab: tab as i32 + 35 };
        }
        let mut ev = Vec::new();
        st.select(FOREIGN_DIALOG, 0x34a, &mut ev);
        if let Some(&crate::guistate::UiEvent::Command { effects, .. }) = ev.iter().find(|e| matches!(e, crate::guistate::UiEvent::Command { dialog: FOREIGN_DIALOG, link: 0x34a, .. })) {
            self.click(st, info, 0x34a, effects, players);
        }
    }

    /// The procedure's code 3 for `link` with the control's `effects` after the click: the GUI
    /// event it sends.
    pub fn click(&mut self, st: &mut GuiState, info: &[u8], link: u16, effects: u8, players: &Players) -> Option<(u16, u32, u32)> {
        let pressed = (effects & 1) as i32;
        match link {
            0x34a..=0x34d => {
                self.group = match link - 0x34a {
                    0 => 0,
                    1 => pressed,
                    2 => pressed * 2,
                    _ => pressed * 3,
                };
                self.lay_out(st);
                self.update(st, info, players);
                None
            }
            0x34e..=0x355 if !self.updating => Some((0x1e3, (0x355 - link + 1) as u32, pressed as u32)),
            0x360..=0x368 => (1..43).find(|&g| self.recs[g].group == self.group && self.recs[g].tab == link as i32).map(|g| (0x1d3, g as u32, 0)),
            _ => None,
        }
    }

    /// 0x4783d0.
    fn lay_out(&self, st: &mut GuiState) {
        for (&m, &t) in MARKS.iter().zip(&FOREIGN_TABS) {
            st.set_hidden(FOREIGN_DIALOG, m, true);
            st.set_hidden(FOREIGN_DIALOG, t, true);
        }
        for g in 0..43 {
            let r = self.recs[g];
            let Ok(tab) = u16::try_from(r.tab) else { continue };
            if r.group != self.group {
                continue;
            }
            st.set_hidden(FOREIGN_DIALOG, r.text, false);
            st.set_hidden(FOREIGN_DIALOG, tab, false);
            let (img, pressed, tip, ext) = crate::gui_pages::TRANSPORT_GOOD[g];
            st.images.insert((FOREIGN_DIALOG, tab), (img, pressed));
            st.tooltips.insert((FOREIGN_DIALOG, tab), (tip, ext));
        }
    }

    /// The update 0x477d50 with the info object's bytes.
    pub fn update(&mut self, st: &mut GuiState, info: &[u8], players: &Players) {
        self.updating = true;
        let mut marked = [false; 4];
        for g in 1..43 {
            let v = info.get(0x11 + g).copied().unwrap_or(0) as i8;
            let r = self.recs[g];
            if v >= 1
                && let Some(m) = marked.get_mut(r.group as usize)
            {
                *m = true;
            }
            if r.group != self.group {
                continue;
            }
            if v < 1 {
                st.set_hidden(FOREIGN_DIALOG, r.text, true);
                continue;
            }
            if v & 4 != 0 {
                st.images.insert((FOREIGN_DIALOG, r.text), (0xa1, 0));
            } else if v == 8 {
                st.images.insert((FOREIGN_DIALOG, r.text), (0xa0, 0));
            }
            st.set_hidden(FOREIGN_DIALOG, r.text, false);
        }
        for (&m, &link) in marked.iter().zip(&FOREIGN_MARKERS) {
            st.set_hidden(FOREIGN_DIALOG, link, !m);
        }
        for i in 0..8u8 {
            let link = 0x355 - i as u16;
            if i < players.count && players.local != i + 1 {
                let (pressed, img) = COLOURS[(players.colours[i as usize] as usize).min(7)];
                st.images.insert((FOREIGN_DIALOG, link), (img, pressed));
                // `SelectControl(54, link, +8 + p)`: the radio is its own group; its code 3 is
                // ignored while the update runs.
                st.release_group(FOREIGN_DIALOG, link);
                if info.get(9 + i as usize).copied().unwrap_or(0) != 0 {
                    st.press(FOREIGN_DIALOG, link);
                }
                st.set_hidden(FOREIGN_DIALOG, link, false);
            } else {
                st.set_hidden(FOREIGN_DIALOG, link, true);
            }
        }
        self.updating = false;
    }
}
