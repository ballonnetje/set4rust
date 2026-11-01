//! The statistics submenu's graph pages (own RE; the tabs 0x618..0x61a of dialog 6): 28 the
//! warriors (`SomethingGraph` 0x4abe80, draw 0x4aa920, rows 0x4ab4c0), 75 the land (0x4aa0d0,
//! `DrawGraph` 0x4a8b80, rows 0x4a9720) and 76 the goods (0x4a8230, draw 0x4a7620). They draw
//! `CStatisticArchive` (`s4_sim::statistic::Archive`) into the owner image of a 300 × 180 control
//! (kind 9) with `LineDDA`. The procedures are checked against unicorn runs
//! (`tools/re/gui-2026-10-21/statgraph_emu.py`, `tests/statgraph_vectors.rs`).
//!
//! The state is the release's statics (0x15aac00.. / 0x15aabd5..): zero before the first open and
//! kept between opens, so an init's first draw shows the previous session's ring and category.
//!
//! * Init (code 0): the guard set, the maximum 100 (`"%d"` into the max text, scale 178 / max, a
//!   draw), the rows set up (28 / 75), ring 0, category 0, x step 19.93; `SelectControl` of the
//!   first ring radio, the first category tab and (28 / 75) the first row's checkbox, whose code 3
//!   arrive with the guard set; the guard cleared, the ring's texts (cursor 15), a draw.
//! * Rows (0x4ab4c0 / 0x4a9720): the players 1..=`PlayerCount` but the last
//!   `DisableLastNPlayersInStatistic` ones (0x11b589c), or in a team game (`GlobalGameStateObject
//!   +0x74`) the players of the local player's team (0x12dcd0c); per row (name, checkbox, value)
//!   links (0xfff740 / 0xfff6a0): the name text, the checkbox's images by the player's colour
//!   (0xfff700 / 0xfff660), the three shown; the checkbox clicked (`UIEventMethod`: its code 3
//!   toggles the row on) unless pressed; the rows after the last hidden.
//! * Draw: the owner image restored (the dialog's background), a grid in RGB (50, 100, 50) (x
//!   20, 40, .. 280 and y 20, 40, .. 160), then per shown row the ring's 16 slots from the newest
//!   (slot `index`) back, x = ⌊pos · step⌋ for pos 15 down to 0, y = ⌊178 − value · scale⌋, each
//!   segment twice (y and y + 1) in the player's colour; at pos = cursor the value goes to the
//!   row's value text and a white line is drawn at x. Ring 1 writes the newest value to the text
//!   first. With the rescale flag (any row or category click but the goods page's first tab) the
//!   maximum becomes the largest drawn value (at least 10), its text set, and it draws again.
//!   Only y > 0 is plotted (the plot callbacks 0x4ab490 / 0x4a96f0 / 0x4a8080).
//! * Zoom: max ∓ max / 10 within 10..=9999, then scale, text, draw. The wheel (code 10, delta in
//!   the high word): delta > 10 like the shrinking button, < 10 like the
//!   growing one (10 itself does nothing).
//!
//! [inf] `LineDDA` is user32's; its points here are Wine's algorithm (as in the emulator runs).
//! [inf] The pixels are RGB565 (`GetColorFromRGB` 0x65a410 with `rgb565Interpolation` 0); 0
//! stands for "not drawn" (no colour drawn is black).

use s4_sim::statistic::{Archive, SAMPLES};

use crate::guistate::GuiState;

/// The owner image's size (the kind-9 control).
pub const W: usize = 300;
pub const H: usize = 180;

/// The player colours (static 0x1026bf8, copied to 0x1563a9c; `IGfxEngine::GetPlayerColor`
/// 0x65a340).
pub const PLAYER_RGB: [(i32, i32, i32); 8] =
    [(240, 21, 21), (69, 62, 210), (0, 200, 54), (250, 238, 0), (190, 75, 255), (253, 160, 36), (33, 197, 181), (230, 255, 255)];

/// The checkbox images by colour (0xfff700 / 0xfff660, the same values): (pressed, normal).
const ROW_IMAGES: [(u16, u16); 8] = [(154, 153), (152, 151), (150, 149), (148, 147), (146, 145), (144, 143), (142, 141), (140, 136)];

/// The goods page's tabs and categories (0x4a8230 code 3); 0x5cc (category 0x1c) sets no rescale.
const GOODS: [(u16, usize); 42] = [
    (0x5cc, 0x1c), (0x5cd, 0x26), (0x5ce, 0xd), (0x5cf, 0x1a), (0x5d0, 0x14), (0x5d1, 0xf), (0x5d2, 0x28), (0x5d3, 0x18),
    (0x5d4, 0xc), (0x5d5, 0x10), (0x5d8, 0x15), (0x5d9, 0x1b), (0x5da, 0xe), (0x5db, 9), (0x5dc, 0x27), (0x5dd, 0x13),
    (0x5de, 0x11), (0x5df, 0x1e), (0x5e0, 0x16), (0x5e1, 0x12), (0x5e2, 7), (0x5e3, 0x29), (0x5e4, 0x25), (0x5e5, 0x22),
    (0x5e6, 0x23), (0x5e7, 0x1f), (0x5e8, 0x21), (0x5e9, 10), (0x5ed, 0x17), (0x5ee, 0x2a), (0xb2e, 8), (0xb2f, 0x24),
    (0xb30, 0x2b), (0xb43, 0x2e), (0xb44, 0x2c), (0xb45, 0x2d), (0xb46, 0x30), (0xb47, 0x2f), (0xb5c, 0xb), (0xb5d, 0x20),
    (0xb5e, 0x1d), (0xb5f, 0x19),
];

/// The x step (0x419f7777).
const STEP: f32 = 19.933332;

/// What the pages read of the game.
pub struct Facts<'a> {
    pub archive: &'a Archive,
    /// `PlayerCount` (0x1294828).
    pub player_count: u8,
    /// `CurrentTribeOffset` (0x129482c): the local player.
    pub local: u8,
    /// `GlobalGameStateObject +0x74` [inf] a team game: the rows are the local team's players.
    pub team_game: bool,
    /// 0x12dcd0c: the team per player id.
    pub teams: [i32; 9],
    /// The player record's +0x14 (stride 60 from 0x149b5d4): the colour index (< 8).
    pub colours: [u8; 9],
    pub names: &'a [String; 9],
    /// 0x11b589c (`Game.DisableLastNPlayersInStatistic`).
    pub hidden_last: i32,
}

/// What a procedure does to the GUI, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Call {
    Text(u16, String),
    /// `SelectControl(dialog, link, 1)` of an unpressed tab / radio / checkbox: pressed (the
    /// group released); its code 3 has run already.
    Select(u16),
    /// `UIEventMethod` of an unpressed checkbox: pressed; its code 3 has run already.
    Click(u16),
    Hide(u16, bool),
    /// (link, image, pressed image).
    Images(u16, u16, u16),
}

/// One page's links.
struct Links {
    dialog: usize,
    graph: u16,
    /// The ring radios (3).
    rings: u16,
    /// The texts the ring radios set (0x639..0x63b).
    ring_texts: u16,
    max_text: u16,
    /// The cursor buttons (+1, −1) and zoom buttons (grow, shrink).
    cursor: (u16, u16),
    zoom: (u16, u16),
    /// The row links (name, checkbox, value) for 8 rows; the checkboxes are `toggles..+8`.
    rows: Option<(u16, u16, u16)>,
}

const L28: Links = Links {
    dialog: 28,
    graph: 0x61b,
    rings: 0x61c,
    ring_texts: 0x639,
    max_text: 0x637,
    cursor: (0x641, 0x642),
    zoom: (0x644, 0x643),
    rows: Some((0x627, 0x61f, 0x62f)),
};
const L75: Links = Links {
    dialog: 75,
    graph: 0x5ef,
    rings: 0x5f0,
    ring_texts: 0x60d,
    max_text: 0x60b,
    cursor: (0x611, 0x612),
    zoom: (0x616, 0x617),
    rows: Some((0x5fb, 0x5f3, 0x603)),
};
const L76: Links = Links {
    dialog: 76,
    graph: 0x5bf,
    rings: 0x5c0,
    ring_texts: 0x5c6,
    max_text: 0x5c4,
    cursor: (0x5ca, 0x5cb),
    zoom: (0x5d7, 0x5d6),
    rows: None,
};
/// The goods page's value text.
const GOODS_VALUE: u16 = 0x5c3;

/// One page's statics.
#[derive(Clone, Debug)]
pub struct Graph {
    pub page: usize,
    /// 0x15aac02 / 0x15aac00 / 0x15aabd6: the init runs (the code 3 of its selects only set).
    guard: bool,
    /// 0x15aac01 / 0x15aabd7 / 0x15aabd5.
    rescale: bool,
    /// The rows: their players (0x15aac58 / 0x15aac0c) and on flags (0x15aac50 / 0x15aac04).
    rows: usize,
    players: [i32; 8],
    shown: [bool; 8],
    pub category: usize,
    pub ring: usize,
    step: f32,
    scale: f32,
    pub max: f32,
    pub cursor: i32,
    /// The owner image, RGB565 (0 = not drawn).
    pub image: Vec<u16>,
}

impl Graph {
    pub fn new(page: usize) -> Graph {
        Graph {
            page,
            guard: false,
            rescale: false,
            rows: 0,
            players: [0; 8],
            shown: [false; 8],
            category: 0,
            ring: 0,
            step: 0.0,
            scale: 0.0,
            max: 0.0,
            cursor: 0,
            image: vec![0; W * H],
        }
    }

    fn links(&self) -> &'static Links {
        match self.page {
            28 => &L28,
            75 => &L75,
            _ => &L76,
        }
    }

    /// Code 0.
    pub fn init(&mut self, f: &Facts, strings: &dyn Fn(u32) -> String, out: &mut Vec<Call>) {
        let l = self.links();
        self.guard = true;
        self.max = 100.0;
        if l.rows.is_some() {
            self.rows = 0;
        }
        self.set_scale(f, out);
        match l.rows {
            Some(_) => self.set_rows(f, out),
            None => {
                // 0x4a80b0.
                out.push(Call::Text(0x5c5, "0".into()));
                out.push(Call::Text(0x5c9, " ".into()));
            }
        }
        self.ring = 0;
        self.category = 0;
        self.step = STEP;
        let tab = match self.page {
            28 => 0x63d,
            75 => 0x613,
            _ => 0x5cc,
        };
        for link in [l.rings, tab] {
            out.push(Call::Select(link));
            self.command(f, link, strings, out);
        }
        // The first row's checkbox: `SelectControl` does nothing when the rows' setup pressed it.
        if let Some((_, toggles, _)) = l.rows {
            out.push(Call::Select(toggles));
            if self.rows == 0 {
                self.command(f, toggles, strings, out);
            }
        }
        self.guard = false;
        self.ring_texts(strings, out);
        self.draw(f, out);
    }

    /// 0x4ab4c0 / 0x4a9720.
    fn set_rows(&mut self, f: &Facts, out: &mut Vec<Call>) {
        let l = self.links();
        let Some((names, toggles, values)) = l.rows else { return };
        if self.page == 28 {
            out.push(Call::Text(0x638, "0".into()));
            out.push(Call::Text(0x63c, "%".into()));
        } else {
            out.push(Call::Text(0x60c, "0".into()));
        }
        self.rows = 0;
        self.shown = [false; 8];
        self.players = [-1; 8];
        let own = f.teams.get(f.local as usize).copied().unwrap_or(0);
        for p in 1..=f.player_count as usize {
            let row = self.rows;
            let take = if f.team_game { f.teams[p] == own } else { (p as i32 - 1) < f.player_count as i32 - f.hidden_last };
            if !take || row >= 8 {
                continue;
            }
            self.rows += 1;
            self.players[row] = p as i32;
            let r = row as u16;
            out.push(Call::Text(names + r, f.names[p].clone()));
            let (pressed, image) = ROW_IMAGES[f.colours[p] as usize % 8];
            out.push(Call::Images(toggles + r, image, pressed));
            for link in [names + r, toggles + r, values + r] {
                out.push(Call::Hide(link, false));
            }
        }
        for row in 0..8u16 {
            if (row as usize) < self.rows {
                // `UIEventMethod` (kind 3): pressed, code 3 (the dialog's controls are released
                // when it opens).
                out.push(Call::Click(toggles + row));
                self.shown[row as usize] = !self.shown[row as usize];
                self.rescale = true;
            } else {
                for link in [names + row, toggles + row, values + row] {
                    out.push(Call::Hide(link, true));
                }
            }
        }
    }

    /// 0x4abda0 / 0x4a9ff0 / 0x4a8150: the ring's texts; cursor 15.
    fn ring_texts(&mut self, strings: &dyn Fn(u32) -> String, out: &mut Vec<Call>) {
        let t = self.links().ring_texts;
        let (mid, right) = match self.ring {
            0 => (" ".to_string(), 0x39d),
            1 => (strings(0x3a0), 0x39f),
            2 => (strings(0x3a1), 0x39e),
            _ => return,
        };
        out.push(Call::Text(t, "0".into()));
        out.push(Call::Text(t + 1, mid));
        out.push(Call::Text(t + 2, strings(right)));
        self.cursor = 15;
        self.step = STEP;
    }

    /// 0x4abd30 / 0x4a9f80 / 0x4a80e0: scale, max text, draw.
    fn set_scale(&mut self, f: &Facts, out: &mut Vec<Call>) {
        self.scale = 178.0 / self.max;
        out.push(Call::Text(self.links().max_text, format!("{}", self.max as i32)));
        self.draw(f, out);
    }

    fn zoom(&mut self, grow: bool, f: &Facts, out: &mut Vec<Call>) {
        if grow {
            self.max += self.max / 10.0;
            if self.max > 9999.0 {
                self.max = 9999.0;
            }
        } else {
            self.max -= self.max / 10.0;
            if self.max < 10.0 {
                self.max = 10.0;
            }
        }
        self.set_scale(f, out);
    }

    /// Code 3 (`link` = the low word of the second argument).
    pub fn command(&mut self, f: &Facts, link: u16, strings: &dyn Fn(u32) -> String, out: &mut Vec<Call>) {
        let l = self.links();
        if (l.rings..l.rings + 3).contains(&link) {
            self.ring = (link - l.rings) as usize;
            if !self.guard {
                self.ring_texts(strings, out);
                self.draw(f, out);
            }
            return;
        }
        if link == l.cursor.0 || link == l.cursor.1 {
            self.cursor = if link == l.cursor.0 { (self.cursor + 1).min(15) } else { (self.cursor - 1).max(0) };
            self.draw(f, out);
            return;
        }
        if link == l.zoom.0 || link == l.zoom.1 {
            self.zoom(link == l.zoom.0, f, out);
            return;
        }
        let category = match self.page {
            28 => match link {
                0x61f..=0x626 => {
                    let i = (link - 0x61f) as usize;
                    self.shown[i] = !self.shown[i];
                    self.rescale = true;
                    None
                }
                0x63d..=0x640 => Some((link - 0x63d + 3) as usize),
                _ => return,
            },
            75 => match link {
                0x5f3..=0x5fa => {
                    let i = (link - 0x5f3) as usize;
                    self.shown[i] = !self.shown[i];
                    self.rescale = true;
                    None
                }
                0x613..=0x615 => {
                    out.push(Call::Text(0x610, if link == 0x613 { "%".into() } else { String::new() }));
                    Some([0, 2, 1][(link - 0x613) as usize])
                }
                _ => return,
            },
            _ => {
                let Some(&(_, c)) = GOODS.iter().find(|g| g.0 == link) else { return };
                if link == 0x5cc {
                    self.category = c;
                    if !self.guard {
                        self.draw(f, out);
                    }
                    return;
                }
                // The goods tabs draw even inside the init.
                self.category = c;
                self.rescale = true;
                self.draw(f, out);
                return;
            }
        };
        if let Some(c) = category {
            self.rescale = true;
            self.category = c;
        }
        if !self.guard {
            self.draw(f, out);
        }
    }

    /// Code 10: the wheel, `delta` the high word of the second argument.
    pub fn wheel(&mut self, f: &Facts, delta: i16, out: &mut Vec<Call>) {
        if delta > 10 {
            self.zoom(false, f, out);
        } else if delta < 10 {
            self.zoom(true, f, out);
        }
    }

    /// The draw (codes 8 and 0xb, and after the changes above).
    pub fn draw(&mut self, f: &Facts, out: &mut Vec<Call>) {
        self.image.fill(0);
        grid(&mut self.image);
        let white = rgb565(255, 255, 255);
        let ring = &f.archive.rings[self.ring.min(2)];
        let mut lines: Vec<(u8, u16)> = Vec::new();
        match self.links().rows {
            Some((_, _, values)) => {
                for row in 0..self.rows.min(8) {
                    if self.shown[row] {
                        lines.push((self.players[row].clamp(0, 8) as u8, values + row as u16));
                    }
                }
            }
            None => lines.push((f.local, GOODS_VALUE)),
        }
        let mut top = 0i32;
        if ring.index >= 0 {
            let idx = ring.index as usize;
            for (p, text) in lines {
                let (r, g, b) = PLAYER_RGB[f.colours.get(p as usize).copied().unwrap_or(0) as usize % 8];
                let colour = rgb565(r, g, b);
                let slots = ring.slots(p, self.category.min(48));
                let at = |s: usize| slots.get(s).copied().unwrap_or(0);
                let y_of = |v: i32| (178.0f32 - v as f32 * self.scale) as i32;
                if self.ring == 1 {
                    out.push(Call::Text(text, at(idx).to_string()));
                }
                let mut x0 = (self.step * 15.0) as i32;
                let mut y0 = y_of(at(idx));
                let mut pos = 15;
                for s in (0..=idx).rev().chain((idx + 1..SAMPLES).rev()) {
                    let v = at(s);
                    top = top.max(v);
                    let y = y_of(v);
                    let x = (pos as f32 * self.step) as i32;
                    line_dda(&mut self.image, (x0, y0), (x, y), colour);
                    line_dda(&mut self.image, (x0, y0 + 1), (x, y + 1), colour);
                    (x0, y0) = (x, y);
                    if pos == self.cursor {
                        out.push(Call::Text(text, v.to_string()));
                        line_dda(&mut self.image, (x, 0), (x, 0xb3), white);
                    }
                    pos -= 1;
                }
            }
        }
        if self.rescale {
            self.rescale = false;
            self.max = top as f32;
            if self.max < 10.0 {
                self.max = 10.0;
            }
            self.scale = 178.0 / self.max;
            out.push(Call::Text(self.links().max_text, format!("{}", self.max as i32)));
            self.draw(f, out);
        }
    }

    /// The image as RGBA (alpha 0 where nothing is drawn).
    pub fn rgba(&self) -> Vec<u8> {
        let mut px = vec![0u8; W * H * 4];
        for (i, &c) in self.image.iter().enumerate() {
            if c != 0 {
                let (r, g, b) = ((c >> 11) as u32 & 31, (c >> 5) as u32 & 63, c as u32 & 31);
                px[i * 4..i * 4 + 4].copy_from_slice(&[(r * 255 / 31) as u8, (g * 255 / 63) as u8, (b * 255 / 31) as u8, 255]);
            }
        }
        px
    }

    /// The calls applied to the GUI (the dialog's procedure has heard their code 3 already).
    pub fn apply(&self, st: &mut GuiState, calls: &[Call]) {
        let d = self.links().dialog;
        for c in calls {
            match c {
                Call::Text(link, t) => {
                    st.texts.insert((d, *link), t.clone());
                }
                Call::Select(link) | Call::Click(link) => {
                    st.release_group(d, *link);
                    st.press(d, *link);
                }
                Call::Hide(link, on) => st.set_hidden(d, *link, *on),
                Call::Images(link, i, p) => {
                    st.images.insert((d, *link), (*i, *p));
                }
            }
        }
        st.owner_images.insert((d, self.links().graph), (W as u32, H as u32, self.rgba()));
    }
}

/// `GetColorFromRGB` (0x65a410) in RGB565: each channel · 31 / 255 (63 / 255 for green) in
/// single precision, rounded to even (`cvtss2si`).
pub fn rgb565(r: i32, g: i32, b: i32) -> u16 {
    const F5: f32 = 0.121_568_63;
    const F6: f32 = 0.247_058_82;
    let c = |v: i32, f: f32| (v as f32 * f).round_ties_even() as i32;
    (((c(r, F5) << 6) + c(g, F6)) << 5).wrapping_add(c(b, F5)) as u16
}

/// 0x4aa8a0 / `DrawBackgroundLines` 0x4a8b00 / 0x4a75a0.
fn grid(img: &mut [u16]) {
    let c = rgb565(50, 100, 50);
    let mut x = 20;
    while (x as f32) < 300.0 {
        line_dda(img, (x, 0), (x, 0xb3), c);
        x += 20;
    }
    let mut y = 20;
    while (y as f32) < 179.0 {
        line_dda(img, (0, y), (299, y), c);
        y += 20;
    }
}

/// `LineDDA` (Wine's points: from the start up to, not including, the end) with the plot
/// callbacks' rule (only y > 0). [inf] Points outside the image are dropped (the release writes
/// them past the control).
fn line_dda(img: &mut [u16], (x1, y1): (i32, i32), (x2, y2): (i32, i32), c: u16) {
    let mut plot = |x: i32, y: i32| {
        if y > 0 && (0..W as i32).contains(&x) && y < H as i32 {
            img[y as usize * W + x as usize] = c;
        }
    };
    let (mut dx, mut dy) = (x2 - x1, y2 - y1);
    let (mut xadd, mut yadd) = (1, 1);
    if dx < 0 {
        dx = -dx;
        xadd = -1;
    }
    if dy < 0 {
        dy = -dy;
        yadd = -1;
    }
    let (mut x, mut y) = (x1, y1);
    if dx > dy {
        let (mut err, erradd) = (2 * dy - dx, 2 * dy - 2 * dx);
        for _ in 0..dx {
            plot(x, y);
            if err > 0 {
                y += yadd;
                err += erradd;
            } else {
                err += 2 * dy;
            }
            x += xadd;
        }
    } else {
        let (mut err, erradd) = (2 * dx - dy, 2 * dx - 2 * dy);
        for _ in 0..dy {
            plot(x, y);
            if err > 0 {
                x += xadd;
                err += erradd;
            } else {
                err += 2 * dx;
            }
            y += yadd;
        }
    }
}

/// [`Facts`] owned: what the client hands the pages each frame.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Input {
    pub archive: Archive,
    pub player_count: u8,
    pub local: u8,
    pub team_game: bool,
    pub teams: [i32; 9],
    pub colours: [u8; 9],
    pub names: [String; 9],
    pub hidden_last: i32,
}

impl Input {
    pub fn facts(&self) -> Facts<'_> {
        Facts {
            archive: &self.archive,
            player_count: self.player_count,
            local: self.local,
            team_game: self.team_game,
            teams: self.teams,
            colours: self.colours,
            names: &self.names,
            hidden_last: self.hidden_last,
        }
    }
}

/// The three pages and the input they last saw.
#[derive(Clone, Debug)]
pub struct Graphs {
    pub pages: [Graph; 3],
    /// The page whose init has run.
    open: Option<usize>,
    input: Input,
}

impl Default for Graphs {
    fn default() -> Self {
        Graphs { pages: [Graph::new(28), Graph::new(75), Graph::new(76)], open: None, input: Input::default() }
    }
}

impl Graphs {
    /// Per frame: the init of a page that opened; a redraw of the open one when the input
    /// changed [inf] (the release redraws on code 0xb, which the GUI event 0x277 after a sample
    /// sends, and on every code 8).
    pub fn frame(&mut self, st: &mut GuiState, input: Input, strings: &dyn Fn(u32) -> String) {
        let changed = input != self.input;
        self.input = input;
        let open = [28, 75, 76].into_iter().find(|d| st.open.contains(d));
        let f = self.input.facts();
        let mut out = Vec::new();
        let Some(g) = open.and_then(|d| self.pages.iter_mut().find(|g| g.page == d)) else {
            self.open = None;
            return;
        };
        if open != self.open {
            g.init(&f, strings, &mut out);
        } else if changed {
            g.draw(&f, &mut out);
        } else {
            return;
        }
        g.apply(st, &out);
        self.open = open;
    }

    /// Code 3 / code 10 of an open page. Returns whether it was one.
    pub fn event(&mut self, st: &mut GuiState, e: &crate::guistate::UiEvent, strings: &dyn Fn(u32) -> String) -> bool {
        use crate::guistate::UiEvent;
        let (d, cmd) = match *e {
            UiEvent::Command { dialog: d @ (28 | 75 | 76), link, .. } => (d, Ok(link)),
            UiEvent::Wheel { dialog: d @ (28 | 75 | 76), delta } => (d, Err(delta as i16)),
            _ => return false,
        };
        let f = self.input.facts();
        let Some(g) = self.pages.iter_mut().find(|g| g.page == d) else { return false };
        let mut out = Vec::new();
        match cmd {
            Ok(link) => g.command(&f, link, strings, &mut out),
            Err(delta) => g.wheel(&f, delta, &mut out),
        }
        g.apply(st, &out);
        true
    }
}
