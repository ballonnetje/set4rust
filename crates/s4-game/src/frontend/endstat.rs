//! The end-of-game statistics screen (`CStateEndStatistic`), as the shipped exe runs it (own RE;
//! decompiles `tools/re/gui-2026-10-21/endstat*.c`, `tools/re/gui-2026-10-24/endstat_a.c`;
//! `tools/re/gui-2026-10-24/endstat_emu.py` runs the original fill and table in unicorn,
//! `tests/endstat_vectors.rs` checks this port against them).
//!
//! * `CStateGame::EndGame` → `EndGameGoToStatistics` 0x514450: the `CEndStatistic` record
//!   0x147c940 is filled (0x4e4660, [`EndRecord`]) and, unless the game is a tutorial
//!   (`GlobalGameStateObject +0x3c` = 4), the state 0x512c00 (game type 3: 0x5128e0, the goods
//!   race's, race_end.rs) follows; after it the main menu (0x5224f0).
//! * The state's constructor 0x512c50 opens dialog 13 (start-screen set) with the procedure
//!   0x491a30: code 0 sets the mode 0x15aabad = 1, runs the fill 0x490fe0 ([`fill`]) and
//!   `SelectControl(13, 0xa19)`; code 3: 0xa07 (OK) → event 0x74 (the state ends), 0xa08..=0xa0f →
//!   event 0x73 with the player 1..=8 (dialog 21, [`detail`]), 0xa18 / 0xa19 → mode 0 / 1 and
//!   the table 0x490850 ([`table`]), 0xa71 → the map's name (0xa65) to the clipboard (shown for
//!   random maps only).
//! * Esc (key event 0xb, 0x1b; 0x512e00): event 0x74, or 0x75 (back to 13) from dialog 21.

use s4_viewer::guistate::GuiState;

/// The dialog slot of the overview.
pub const DIALOG: usize = 13;
/// OK (the players' buttons are 0xa08..=0xa0f), the two mode radios.
pub const OK: u16 = 0xa07;
pub const MODE0: u16 = 0xa18;
pub const MODE1: u16 = 0xa19;
/// The random map's save button.
pub const SAVE_MAP: u16 = 0xa71;

/// A player's `CStatistic` record (0x1128 bytes) as dwords; the screens read it by offset
/// ([`PlayerStats::at`]). What the sim keeps is placed by `sim.rs` `end_record`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerStats(pub Vec<i32>);

impl Default for PlayerStats {
    fn default() -> Self {
        PlayerStats(vec![0; RECORD / 4])
    }
}

/// The record's size.
pub const RECORD: usize = 0x1128;

impl PlayerStats {
    /// The dword at byte offset `off`.
    pub fn at(&self, off: usize) -> i32 {
        self.0.get(off / 4).copied().unwrap_or(0)
    }
    pub fn set(&mut self, off: usize, v: i32) {
        if let Some(d) = self.0.get_mut(off / 4) {
            *d = v;
        }
    }
}

/// The sim's `CStatistic` player record (s4_sim::statistic) at its offsets, with the player's
/// vehicles made / finished per type (`world.vehicles.stat_made` / `stat_finished` at
/// `player · 6 + type`: +0xb64 / +0xb80, their sums +0xb7c / +0xb98).
pub fn record_of(p: &s4_sim::statistic::PlayerStrength, vehicles_made: &[i32], vehicles_finished: &[i32]) -> PlayerStats {
    let mut r = PlayerStats::default();
    let mut array = |base: usize, v: &[i32]| {
        for (k, &x) in v.iter().enumerate() {
            r.set(base + 4 * k, x);
        }
    };
    array(0x0, &p.settlers);
    array(0xa04, &p.goods);
    array(0xba4, &p.cost);
    array(0xbb0, &p.eye_cost);
    array(0xc1c, &p.kills);
    array(0xd2c, &p.losses);
    // Buildings captured / lost to a capture, per type (`FirstSettlerEnter` 0x54871c / 0x548730).
    array(0x374, &p.conquered);
    array(0x4c0, &p.captured_from);
    array(0xe40, &p.vehicle_kills);
    array(0xf50, &p.vehicle_losses);
    array(0x1078, &p.goods_start);
    // Settlers made (+0x110), buildings now (+0x228, `count`) and finished ever (+0x610), goods
    // made (+0xab4), vehicles made / finished (+0xb64 / +0xb80).
    array(0x110, &p.produced);
    array(0x228, &p.count);
    array(0x610, &p.built);
    array(0xab4, &p.goods_made);
    array(0xb64, vehicles_made);
    array(0xb80, vehicles_finished);
    for (off, v) in [
        (0x21c, p.produced_total), (0x60c, p.buildings_total()), (0x75c, p.built_total), (0xa00, p.beds), (0xb60, p.goods_made_total),
        (0xb7c, vehicles_made.iter().sum()), (0xb98, vehicles_finished.iter().sum()),
    ] {
        r.set(off, v);
    }
    for (off, v) in [
        (0x10c, p.settlers_total), (0x220, p.civilians), (0x224, p.soldiers), (0xab0, p.goods_total), (0xb9c, p.land), (0xba0, p.base),
        (0xbbc, p.own), (0xbc0, p.foreign), (0xbc4, p.value), (0xd28, p.kills_total), (0xe38, p.losses_total), (0xe3c, p.military_lost),
        (0xf4c, p.vehicle_kills_total), (0x105c, p.vehicle_losses_total), (0x1060, p.mana), (0x1064, p.mana_gained), (0x106c, p.land_max),
        (0x1070, p.own_max), (0x1074, p.foreign_max), (0x1124, p.over),
    ] {
        r.set(off, v);
    }
    r
}

/// `CEndStatistic` 0x147c940 as 0x4e4660 fills it at the end of a game.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EndRecord {
    /// 0x14864ac: `*GlobalGameStateObject`, the game tick.
    pub tick: u32,
    /// 0x14864a8: `PlayerCount`; 0x14863ac: the last players left out (+0x9a6c =
    /// `Game.DisableLastNPlayersInStatistic`, 0x11b589c).
    pub players: i32,
    pub removed: i32,
    /// 0x14863b0 / 0x14863b4 (0x4e46f7): `GlobalGameStateObject +0x34`, the game was decided,
    /// and +0x38, the winning team (only then). [inf] The object is `CStateGame +0x30`: its +0x64
    /// / +0x68 are what event 0x37 sets (0x517401: decided, the event's team), and
    /// `EndGameGoToStatistics` reads the game kind as `CStateGame +0x6c` = `GlobalGameStateObject
    /// +0x3c`.
    pub decided: bool,
    pub winner: i32,
    /// 0x14863b8: `GlobalGameState +0x18`, the map's path (as 0x5069b0 got it).
    pub path: String,
    /// Per player 0..=8: the name (0x14863d0 + 0x18 i), race (0x14864b0), colour (0x14864d4,
    /// `PlayerColorIndex`), team (0x14864f8, 0x12dcd0c).
    pub names: [String; 9],
    pub race: [i32; 9],
    pub colour: [i32; 9],
    pub team: [i32; 9],
    /// The warrior type of each settler type (0x12dbca0, the game's rules).
    pub warrior: Vec<u8>,
    /// The `CStatistic` copy (+4), player 0..=8.
    pub stats: Vec<PlayerStats>,
}

/// .rdata 0x102d6e4..: the images by team (0..=8, entry 0 is the end of another table), by race
/// (+9) and by colour (+14), contiguous; indices past them read on into the next tables.
const IMAGES: [u16; 40] = [
    2609, 115, 116, 117, 118, 119, 120, 121, 122, 33, 34, 32, 35, 38, 0, 2, 4, 6, 8, 10, 12, 14, 2594, 2595, 2596, 2597, 2598, 2599, 2600,
    2601, 2586, 2587, 2588, 2589, 2590, 2591, 2592, 2593, 2576, 2577,
];

fn image(i: i32) -> u16 {
    usize::try_from(i).ok().and_then(|i| IMAGES.get(i)).copied().unwrap_or(0)
}

/// Row `i` (player 1..=8) links: name button, colour, race, team, result images.
fn row(i: usize) -> [u16; 5] {
    let k = i as u16 - 1;
    [0xa08 + k, 2576 + k, 2586 + k, 2594 + k, 2602 + k]
}

/// The six value columns (.rdata 0x102d858, 0x102d838, 0x102d818, 0x102d7f8, 0x102d7d8,
/// 0x102d7b8) of row `i`.
fn columns(i: usize) -> [u16; 6] {
    let k = i as u16 - 1;
    [2610 + k, 2618 + k, 2626 + k, 2650 + k, 2634 + k, 2642 + k]
}

/// The control's tooltips as the fills write them: one dword at `+0x14`, so the extended
/// tooltip (`+0x16`) becomes 0.
pub(crate) fn tooltip_in(st: &mut GuiState, dialog: usize, link: u16, tip: u16) {
    st.tooltips.insert((dialog, link), (tip, 0));
}

fn tooltip(st: &mut GuiState, link: u16, tip: u16) {
    tooltip_in(st, DIALOG, link, tip);
}

fn text(st: &mut GuiState, link: u16, t: String) {
    st.texts.insert((DIALOG, link), t);
}

/// The game time `tick · 71` (u32, wrapping) as seconds (f64 → f32, / 1000, truncated).
pub fn time(tick: u32) -> String {
    let ms = tick.wrapping_mul(71) as f64;
    let s = (ms as f32 / 1000.0) as u32;
    format!("{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

/// The map's text (0x491541..): from after the first '\' to the next '.', or the whole path
/// without a '\'.
pub fn map_name(path: &str) -> String {
    match path.split_once('\\') {
        None => path.to_string(),
        Some((_, rest)) => rest.split('.').next().unwrap_or("").to_string(),
    }
}

/// The fill 0x490fe0 (`strings`: the string table; `random_map`: the map is a random one,
/// `RandomMap` vtbl+0x24), then the table in mode `mode1`.
pub fn fill(st: &mut GuiState, rec: &EndRecord, strings: &dyn Fn(u32) -> String, random_map: bool, mode1: bool) {
    text(st, 0xa66, time(rec.tick));
    for (link, tip) in [(MODE0, 0x356), (MODE1, 0x361), (OK, 0x353), (0xa66, 0x35d), (0xa62, 0x36b)] {
        tooltip(st, link, tip);
    }
    text(st, 0xa62, strings(0x36b));
    text(st, 0xa63, strings(0x360));
    text(st, 0xa64, strings(0x35d));
    tooltip(st, 0xa65, 0x360);
    text(st, 0xa65, map_name(&rec.path));
    if !random_map {
        st.set_hidden_notify(DIALOG, SAVE_MAP, true);
    }
    let shown = rec.players.wrapping_sub(rec.removed);
    for i in 1..9 {
        let links = row(i);
        if (i as i32) > shown {
            for l in links {
                st.set_hidden_notify(DIALOG, l, true);
            }
            continue;
        }
        for (l, tip) in links[..4].iter().zip([0x368, 0x355, 0x369, 0x36a]) {
            tooltip(st, *l, tip);
        }
        text(st, links[0], rec.names[i].clone());
        st.images.insert((DIALOG, links[1]), (image(14 + rec.colour[i]), 0));
        st.images.insert((DIALOG, links[2]), (image(9 + rec.race[i]), 0));
        st.images.insert((DIALOG, links[3]), (image(rec.team[i]), 0));
        let (img, tip) = if !rec.decided {
            (0xfb, 0x35e)
        } else if rec.team[i] == rec.winner {
            (0xfd, 0x36c)
        } else {
            (0xfc, 0x35f)
        };
        st.images.insert((DIALOG, links[4]), (img, 0));
        tooltip(st, links[4], tip);
    }
    table(st, rec, mode1);
}

/// Mode 1's score (0x490850): `(5·kills (+0xd28) + 2·(warriors made + +0xaec + made +0x21c) +
/// +0xb38 + +0xb14 + +0xb08 + +0xaf0 + +0xae0 + +0xadc + +0xad8 + mana +0x1064 + finished +0x75c)
/// / 10`, i32 wrapping.
pub fn score(rec: &EndRecord, p: usize) -> i32 {
    let s = &rec.stats[p];
    let sum = [0xb38, 0xb14, 0xb08, 0xaf0, 0xae0, 0xadc, 0xad8, 0x1064, 0x75c].iter().fold(0i32, |a, &o| a.wrapping_add(s.at(o)));
    let made = warriors_made(rec, p).wrapping_add(s.at(0xaec)).wrapping_add(s.at(0x21c));
    s.at(0xd28).wrapping_mul(5).wrapping_add(made.wrapping_mul(2)).wrapping_add(sum) / 10
}

/// The settlers made (+0x110 + 4·type) of the types whose warrior type (0x12dbca0) is 1..=6.
fn warriors_made(rec: &EndRecord, p: usize) -> i32 {
    (0..0x43).filter(|&t| rec.warrior.get(t).is_some_and(|&w| w.wrapping_sub(1) < 6)).fold(0i32, |a, t| a.wrapping_add(rec.stats[p].at(0x110 + 4 * t)))
}

/// The table 0x490850: mode 0 the economy (made, finished, land %, mana, two goods sums), mode 1
/// the military (warriors made, kills, foreign strength %, the score — font 0 for the best,
/// else 1 —, own strength %, the military buildings 46..=48 finished).
pub fn table(st: &mut GuiState, rec: &EndRecord, mode1: bool) {
    let (tips, images): ([u16; 6], [u16; 6]) = if mode1 {
        ([0x367, 0x362, 0x364, 0x363, 0x365, 0xf03], [0xf6, 0xf1, 0xf3, 0xf2, 0xf4, 0x14d])
    } else {
        ([0x35c, 0x357, 0x359, 0x35b, 0x358, 0x35a], [0xf0, 0xeb, 0xed, 0xef, 0xec, 0xee])
    };
    for (k, (tip, img)) in tips.iter().zip(images).enumerate() {
        let link = 0xa67 + k as u16;
        tooltip(st, link, *tip);
        st.images.insert((DIALOG, link), (img, 0));
    }
    let shown = rec.players.wrapping_sub(rec.removed);
    let pct = |v: i32| format!("{}%", v.wrapping_mul(100) >> 8);
    let scores: Vec<i32> = (1..9).map(|p| score(rec, p)).collect();
    let best = scores.iter().copied().fold(0, i32::max);
    for i in 1..9 {
        if i as i32 > shown {
            continue;
        }
        let s = &rec.stats[i];
        let c = columns(i);
        let sum = |offs: &[usize]| offs.iter().fold(0i32, |a, &o| a.wrapping_add(s.at(o))).to_string();
        if mode1 {
            text(st, c[0], warriors_made(rec, i).to_string());
            text(st, c[1], s.at(0xd28).to_string());
            text(st, c[2], pct(s.at(0x1074)));
            st.fonts.insert((DIALOG, c[3]), if scores[i - 1] == best { 0 } else { 1 });
            text(st, c[3], scores[i - 1].to_string());
            text(st, c[4], pct(s.at(0x1070)));
            text(st, c[5], sum(&[0x6c8, 0x6cc, 0x6d0]));
        } else {
            text(st, c[0], s.at(0x21c).to_string());
            text(st, c[1], s.at(0x75c).to_string());
            text(st, c[2], format!("{}%", s.at(0x106c)));
            st.fonts.insert((DIALOG, c[3]), 3);
            text(st, c[3], s.at(0x1064).to_string());
            text(st, c[4], sum(&[0xb38, 0xb08, 0xaf0, 0xadc]));
            text(st, c[5], sum(&[0xb14, 0xae0, 0xad8]));
        }
    }
}

/// The detail dialog (event 0x73: `CloseDialog(13)`, `OpenDialog(0x15, 0x480250)`, the init
/// 0x47e030 for the player).
pub const DETAIL: usize = 21;
/// Its back button (0x83a → event 0x75: dialog 13 again) and the scroll buttons (0x857 down,
/// 0x858 up).
pub const BACK: u16 = 0x83a;
pub const DOWN: u16 = 0x857;
pub const UP: u16 = 0x858;
/// The rows shown: labels 0x843.., values 0x84d.. (0x47de90).
const ROWS: u16 = 10;

/// .rdata 0x102d5c0..: the detail's images by team (entry 0 is the "%" of another string), by
/// race (+9; the Trojans read the next table's 0) and by colour (+14).
const DETAIL_IMAGES: [u16; 24] = [37, 115, 116, 117, 118, 119, 120, 121, 122, 33, 34, 32, 35, 0, 1, 3, 5, 7, 9, 11, 13, 15, 2125, 2126];

fn detail_image(i: i32) -> u16 {
    usize::try_from(i).ok().and_then(|i| DETAIL_IMAGES.get(i)).copied().unwrap_or(0)
}

/// One row of the table 0x146b2b8 (entries of 0x34: label, value, font +0x30).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Row {
    pub label: String,
    pub value: String,
    pub font: u8,
}

/// The rows 0x47e030 builds for player `p` (the table's 100 entries reset to "", "", font 1
/// first). Read off the original by setting each record dword alone
/// (`tools/re/gui-2026-10-24/detail_emu.py derive RACE`, `detail_derive_r*.txt`): the economy
/// (made, finished, land, goods produced, mana), then per race the military: the Dark Tribe's
/// own list with rows for +0x1e8 / +0x1e0 / +0x1ec only when above 0.
pub fn detail_rows(rec: &EndRecord, p: usize, strings: &dyn Fn(u32) -> String) -> Vec<Row> {
    let s = &rec.stats[p];
    let mut rows = Vec::new();
    let sum = |offs: &[usize]| offs.iter().fold(0i32, |a, &o| a.wrapping_add(s.at(o)));
    let head = |rows: &mut Vec<Row>, id: u32, font: u8| rows.push(Row { label: strings(id), value: String::new(), font });
    let dashes = "-".repeat(0x76);
    let line = |rows: &mut Vec<Row>, id: u32, offs: &[usize]| rows.push(Row { label: strings(id), value: sum(offs).to_string(), font: 1 });
    let blank = |rows: &mut Vec<Row>| rows.push(Row { font: 1, ..Default::default() });
    head(&mut rows, 0x931, 0x11);
    rows.push(Row { label: dashes.clone(), value: String::new(), font: 0xc });
    line(&mut rows, 0x933, &[0x21c]);
    line(&mut rows, 0x935, &[0x75c]);
    rows.push(Row { label: strings(0x937), value: format!("{}%", s.at(0x106c)), font: 1 });
    for (id, offs) in [
        (0x939, &[0xadc, 0xaf0, 0xb08, 0xb38][..]), (0x93b, &[0xadc]), (0x93d, &[0xb08]), (0x943, &[0xaf0]), (0x941, &[0xb38]),
        (0x945, &[0xad8, 0xae0, 0xb14]), (0x949, &[0xae0]), (0x947, &[0xad8]), (0x94b, &[0xb14]), (0x94d, &[0x1064]),
    ] {
        line(&mut rows, id, offs);
    }
    blank(&mut rows);
    head(&mut rows, 0x951, 0x11);
    rows.push(Row { label: dashes, value: String::new(), font: 0xc });
    if rec.race[p] == 3 {
        line(&mut rows, 0x967, &[0x6d4]);
        blank(&mut rows);
        line(&mut rows, 0x971, &[0x184]);
        blank(&mut rows);
        line(&mut rows, 0x975, &[0x190]);
        blank(&mut rows);
        for (id, off) in [(0x930, 0x1e8), (0x932, 0x1e0), (0x934, 0x1ec)] {
            if s.at(off) > 0 {
                line(&mut rows, id, &[off]);
            }
        }
        blank(&mut rows);
    } else {
        let race = match rec.race[p] {
            0 => (0x92e, [0x19c, 0x1a0, 0x1a4]),
            2 => (0x92c, [0x1b4, 0x1b8, 0x1bc]),
            4 => (0x11a, [0x204, 0x208, 0x20c]),
            _ => (0x977, [0x1a8, 0x1ac, 0x1b0]),
        };
        let groups: [(u32, [(u32, usize); 3]); 4] = [
            (0x957, [(0x955, 0x6c8), (0x959, 0x6cc), (0x95d, 0x6d0)]),
            (0x95f, [(0x961, 0x660), (0x963, 0x65c), (0x965, 0x668)]),
            (0x969, [(0x96b, 0x184), (0x96d, 0x188), (0x96f, 0x18c)]),
            (0x973, [(0x96b, 0x190), (0x96d, 0x194), (0x96f, 0x198)]),
        ];
        for (h, items) in groups {
            head(&mut rows, h, 0);
            for (id, off) in items {
                line(&mut rows, id, &[off]);
            }
            blank(&mut rows);
        }
        head(&mut rows, race.0, 0);
        for (id, off) in [(0x96b, race.1[0]), (0x96d, race.1[1]), (0x96f, race.1[2])] {
            line(&mut rows, id, &[off]);
        }
        line(&mut rows, 0x936, &[0x1c0]);
        blank(&mut rows);
    }
    head(&mut rows, 0x938, 0);
    for (id, off) in [(0x93a, 0xb84), (0x93c, 0xb8c), (0x93e, 0xb90)] {
        line(&mut rows, id, &[off]);
    }
    blank(&mut rows);
    line(&mut rows, 0x944, &[0xd28]);
    line(&mut rows, 0x942, &[0xe38]);
    blank(&mut rows);
    line(&mut rows, 0x946, &[0xf4c]);
    line(&mut rows, 0x94a, &[0x105c]);
    blank(&mut rows);
    line(&mut rows, 0x948, &[0x42c, 0x430, 0x434]);
    line(&mut rows, 0x94c, &[0x578, 0x57c, 0x580]);
    rows
}

/// The detail of one player: its rows and the first shown (0x12938d0; count 0x12938cc).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Detail {
    pub player: usize,
    pub rows: Vec<Row>,
    pub top: usize,
}

/// The init 0x47e030 for player `p`: the titles (0x47dfd0), the rows, the name and the time,
/// the tooltips, the colour / race / team images, the first ten rows.
pub fn detail(st: &mut GuiState, rec: &EndRecord, p: usize, strings: &dyn Fn(u32) -> String) -> Detail {
    let set = |st: &mut GuiState, link: u16, t: String| {
        st.texts.insert((DETAIL, link), t);
    };
    set(st, 0x83b, strings(0x92b));
    set(st, 0x83d, strings(0x92d));
    set(st, 0x83c, strings(0x92f));
    let d = Detail { player: p, rows: detail_rows(rec, p, strings), top: 0 };
    set(st, 0x83e, rec.names.get(p).cloned().unwrap_or_default());
    set(st, 0x83f, time(rec.tick));
    for (link, tip) in [(BACK, 0x352), (0x83f, 0x35d), (0x83b, 0x36b), (0x85b, 0x355), (0x859, 0x369), (0x85a, 0x36a)] {
        tooltip_in(st, DETAIL, link, tip);
    }
    st.images.insert((DETAIL, 0x85b), (detail_image(14 + rec.colour[p]), 0));
    st.images.insert((DETAIL, 0x859), (detail_image(9 + rec.race[p]), 0));
    st.images.insert((DETAIL, 0x85a), (detail_image(rec.team[p]), 0));
    show(st, &d);
    d
}

/// 0x47de90: rows `top..top + 10` into the labels (with their fonts) and values.
pub fn show(st: &mut GuiState, d: &Detail) {
    let empty = Row { font: 1, ..Default::default() };
    for i in 0..ROWS {
        let r = d.rows.get(d.top + i as usize).unwrap_or(&empty);
        st.texts.insert((DETAIL, 0x843 + i), r.label.clone());
        st.fonts.insert((DETAIL, 0x843 + i), r.font);
        st.texts.insert((DETAIL, 0x84d + i), r.value.clone());
    }
}

/// 0x480210 (`down`) / 0x4801e0: one row on while there is one; returns whether it moved (it
/// plays sound 11 then).
pub fn scroll(st: &mut GuiState, d: &mut Detail, down: bool) -> bool {
    if down && d.top + ROWS as usize >= d.rows.len() || !down && d.top == 0 {
        return false;
    }
    if down {
        d.top += 1;
    } else {
        d.top -= 1;
    }
    show(st, d);
    true
}

/// Code 3 of the procedure 0x491a30 on `link`: the state's next step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Event 0x74: the screen ends.
    Done,
    /// Event 0x73 with the player: dialog 21.
    Player(usize),
    /// The table in the other mode.
    Mode(bool),
}

pub fn command(link: u16) -> Option<Step> {
    match link {
        OK => Some(Step::Done),
        0xa08..=0xa0f => Some(Step::Player((link - 0xa07) as usize)),
        MODE0 => Some(Step::Mode(false)),
        MODE1 => Some(Step::Mode(true)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// sim's end-screen counts land at the record's offsets (SIM.md §22).
    #[test]
    fn record_offsets() {
        let mut p = s4_sim::statistic::PlayerStrength::default();
        p.produced[3] = 5;
        p.produced_total = 6;
        p.count[2] = 1;
        p.count[7] = 2;
        p.built[2] = 4;
        p.built_total = 7;
        p.beds = 8;
        p.goods_made[1] = 9;
        p.goods_made_total = 10;
        let r = record_of(&p, &[1, 0, 0, 0, 0, 2], &[0, 3, 0, 0, 0, 0]);
        let at = |o: usize| r.at(o);
        assert_eq!((at(0x110 + 12), at(0x21c), at(0x228 + 8), at(0x60c)), (5, 6, 1, 3));
        assert_eq!((at(0x610 + 8), at(0x75c), at(0xa00), at(0xab4 + 4), at(0xb60)), (4, 7, 8, 9, 10));
        assert_eq!((at(0xb64), at(0xb64 + 20), at(0xb7c), at(0xb80 + 4), at(0xb98)), (1, 2, 3, 3, 3));
        // The neighbours stay theirs.
        assert_eq!((at(0x374), at(0xa04)), (0, 0));
    }

    #[test]
    fn time_and_map_name() {
        assert_eq!(time(0), "00:00:00");
        assert_eq!(time(15_451), "00:18:17");
        assert_eq!(map_name("Map\\Singleplayer\\Aeneas.map"), "Singleplayer\\Aeneas");
        assert_eq!(map_name("plain"), "plain");
        assert_eq!(map_name("Map\\User\\x.y.map"), "User\\x");
    }
}
