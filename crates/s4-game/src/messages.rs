//! The in-game text messages ("You are under attack!", "This mine is out of minerals …"): the
//! release's `CTextMsgHandler` (0x529fe0..0x52b4d0, list at `messageHandleStartOffset`) and the
//! part of `CGuiEventHandler` (0x4805a0) that shows them and plays their sounds.
//!
//! # The list (`CTextMsgHandler`)
//! A message is a 0x24-byte record: text id (+0), player (+4), lifetime (+0xc), node x / y
//! (+0x10 / +0x14), icon (+0x18), "has a position" (+0x1c), "classified" (+0x1d), "read"
//! (+0x1e) and an optional formatted text (+0x20). The generators:
//! * 0x52a460 `(text, player, x, y)`, the logic's warnings: only if the text's class passes the
//!   player's filter ([`Classes::passes`], 0x52a660), lifetime `DefaultMessageTimeOut / 2`,
//!   icon by class, with a position, classified.
//! * 0x52a320 `(text, player | flags, x, y, filtered, keep_dup, …)`: the same with a formatted
//!   text, icon 0xa5; the filter only if `filtered`; player bit 0x10000000 = never expires
//!   (lifetime −1).
//! * Both drop the new message when one with the same text, player, x and y is in the list
//!   (0x52ac80), unless 0x52a320's `keep_dup`.
//! * 0x529fe0 appends; at [`CAPACITY`] messages the oldest goes first. It sends the GUI a
//!   refresh, and the generator sends another one: two refreshes per message (both
//!   `sub_12AAD0(0, 1, 1)`), so two bells.
//! * The 100 ms timer event 0x15 (`GameTick` 0x503390, wall clock): every 20th (2 s) every
//!   positive lifetime counts down, and a message at 0 is removed (0x52a530). So a warning stays
//!   `DefaultMessageTimeOut` = 120 s (`MSG_DISPLAYTIMES`, static init 0x423dd0).
//! * A click on a line (`ChatEvents` 0x499d80 code 3 → event 0x1771 → 0x52a800): the camera goes
//!   to its position (event 0x13) and, unless it never expires, the message is marked read and
//!   leaves the window.
//!
//! # The window (`CGuiEventHandler` 0x4805a0)
//! `sub_12AAD0(popup, new, refresh)` (0x52aad0) sends the unread messages' (text, icon) list to
//! the GUI with event 0x267 (`PopupAllMsgs` off), 0x268 (on), or with `refresh` clear (or the
//! last message a network wait / credits line) 0x27a / 0x279. The handler:
//! * 0x267 / 0x27a: a non-empty list sets the blink flag of the closed window's button (dialog
//!   0x3b, images 0x14a / 0x14b ↔ 0x148 / 0x149 every 5 timer events). 0x267 with `new` rings
//!   `SOUND_GUI_BELL01` (0x4c). An open window plays 7 (not for 0x27a) and redraws its lines.
//! * 0x268 / 0x279: an open window plays `SOUND_GUI_HEAL01` (0x4e, not for 0x279), then as
//!   0x267 (no bell). A closed one is opened (event 0x269), which redraws through 0x1773 →
//!   `sub_12AAD0(0, 0, 0)`: 0x27a / 0x279, silent.
//! * The button (`HiddenChatMenuCallback` 0x4797f0, link 0x37b) opens the window: 0x269, then
//!   0x1773 with 1 → `sub_12AAD0(0, 0, 1)` → 0x267 without `new` (plays 7) or 0x268 (0x4e, 7);
//!   the button itself plays 11 (code 3, [`crate::sound::gui`]).
//! * The window's close button (0x3c8) sends 0x26a: the window closes, the button returns with
//!   images 0x148 / 0x149 and the blink flag clear.

use std::collections::HashMap;

use s4_sim::event::SimEvent;

/// `DAT_011644bc`: the list's capacity, 50 from the static data. Only the in-game settings
/// dialog's apply (`EventManagement` 0x518553 → 0x52a900) sets it to `MsgHistory` (default 100).
pub const CAPACITY: usize = 50;
/// `MSG_DISPLAYTIMES` `DefaultMessageTimeOut` (static init 0x423dd0): seconds; the lifetime
/// counter is half of it, counted down every 2 s.
pub const TIMEOUT: i32 = 120;

/// Class icons (0x52a660 by the class's category byte; 0x52a320 always 0xa5).
pub const ICON_SYS: u16 = 0xa5;
pub const ICON_ECO: u16 = 0xa6;
pub const ICON_WAR: u16 = 0xa7;
pub const ICON_CONS: u16 = 0xa8;

/// The text ids of the messages the logic sends, with their `STRINGID` names (the name table at
/// 0x100db84), which `WarningTypes.cfg` classifies.
pub const NAMES: &[(u16, &str)] = &[
    (0x122, "AO_GUI_MSG_BALIST"),
    (0x139, "AO_GUI_WARN_BUILD_NOBOW"),
    (0x1bd, "AO_SAVEALERT"),
    (0x1fd, "AO_STORAGEAREA_READY"),
    (0x776, "GUI_MSG_CART"),
    (0x777, "GUI_MSG_CATAPULT"),
    (0x780, "GUI_MSG_FINDGOLD"),
    (0x781, "GUI_MSG_FINDIRON"),
    (0x782, "GUI_MSG_FINDSTONE"),
    (0x783, "GUI_MSG_FINDSULFUR"),
    (0x784, "GUI_MSG_FIRESPIT"),
    (0x785, "GUI_MSG_GOODSTRANSP"),
    (0x78f, "GUI_MSG_MINECOAL"),
    (0x790, "GUI_MSG_MINEGOLD"),
    (0x791, "GUI_MSG_MINEIRON"),
    (0x792, "GUI_MSG_MINESTONE"),
    (0x793, "GUI_MSG_MINESULFUR"),
    (0x7aa, "GUI_MSG_THOR"),
    (0x7ab, "GUI_MSG_WARSHIP"),
    (0x990, "GUI_WARN_ATTACK_BUILD"),
    (0x992, "GUI_WARN_ATTACK_GEN"),
    (0x995, "GUI_WARN_BUILD_NOAXE"),
    (0x996, "GUI_WARN_BUILD_NOHAMMER"),
    (0x997, "GUI_WARN_BUILD_NOPICK"),
    (0x998, "GUI_WARN_BUILD_NOROD"),
    (0x999, "GUI_WARN_BUILD_NOSAW"),
    (0x99a, "GUI_WARN_BUILD_NOSCYTHE"),
    (0x99b, "GUI_WARN_BUILD_NOSETTLER"),
    (0x99c, "GUI_WARN_BUILD_NOSHOVEL"),
    (0x99d, "GUI_WARN_BUILD_NOWAR"),
    (0x99e, "GUI_WARN_CAPTURE"),
    (0x9a0, "GUI_WARN_CART_HAMMER"),
    (0x9a2, "GUI_WARN_CART_PLANKS"),
    (0x9a3, "GUI_WARN_CART_STONE"),
    (0x9a6, "GUI_WARN_DESTROY_BLD"),
    (0x9a7, "GUI_WARN_DESTROY_SHIP"),
    (0x9de, "GUI_WARN_MINE_EMPTY"),
    (0x9e0, "GUI_WARN_NODONKEY"),
    (0x9ef, "GUI_WARN_STONECUT_NOSTONE"),
    (0x9f7, "GUI_WARN_TOWER_NO_CRUSH"),
    (0x9fa, "GUI_WARN_WOODCUT_NOWOOD"),
    (0xefd, "GUI_WARN_FISHERHUT_NOFISH"),
];

/// The `<WARNING_…>` values `WarningTypes.cfg` uses (the define table at 0x1019e78: value, name).
/// Each byte is a category (SYS, WAR, ECO, CONS from low to high); its bit 0 is the category's
/// filter bit, the others the type's.
const DEFINES: &[(&str, u32)] = &[
    ("WARNING_CONS_MISSING_RESOURCES", 0x300_0000),
    ("WARNING_CONS_FOUND_RESOURCE", 0x500_0000),
    ("WARNING_CONS_BUILDING_SITE", 0x900_0000),
    ("WARNING_ECO_MISSING_RESOURCES", 0x3_0000),
    ("WARNING_ECO_MISSING_TOOL", 0x5_0000),
    ("WARNING_ECO_CANNOT_OCCUPY_HOUSE", 0x9_0000),
    ("WARNING_WAR_BATTLE_STARTED", 0x300),
    ("WARNING_WAR_UNIT_LOST", 0x500),
    ("WARNING_WAR_AREA_LOST", 0x900),
    ("WARNING_WAR_ENEMY_SIGHTED", 0x1100),
    ("WARNING_WAR_THIEVES_SIGHTED", 0x2100),
    ("WARNING_WAR_HOUSE_SABOTAGED", 0x4100),
    ("WARNING_SYS_NOT_SAVED", 3),
    ("WARNING_SYS_NETWORK", 5),
];

/// `Config\WarningTypes.cfg` `[WARNINGMSG_CLASSIFICATION]`: text name → class.
#[derive(Clone, Debug, Default)]
pub struct Classes {
    by_text: HashMap<u16, u32>,
}

impl Classes {
    pub fn load() -> Classes {
        let path = s4_formats::path::resolve(s4_formats::game_dir().join("Config").join("WarningTypes.cfg"));
        match std::fs::read(&path) {
            Ok(b) => Classes::parse(&String::from_utf8_lossy(&b)),
            Err(e) => {
                tracing::warn!("messages: {}: {e}", path.display());
                Classes::default()
            }
        }
    }

    pub fn parse(text: &str) -> Classes {
        let cfg = s4_audio::cfg::Cfg::parse(text);
        let mut by_text = HashMap::new();
        for &(id, name) in NAMES {
            if let Some(v) = cfg.get("WARNINGMSG_CLASSIFICATION", name) {
                by_text.insert(id, value(v));
            }
        }
        Classes { by_text }
    }

    /// 0x52a660: whether a message with text `text` shows under the filter `mask`
    /// (`GAMESETTINGS` `MsgLevelMask`, `DAT_011b58f4`), and its icon. The class must share a
    /// bit with the mask, and the mask must have the filter bit of the class's lowest category.
    /// A text without a class (or class 0: `GUI_WARN_MINE_EMPTY=`, the bare
    /// `GUI_WARN_TOWER_NO_CRUSH`) never shows.
    pub fn passes(&self, text: u16, mask: u32) -> (bool, u16) {
        let Some(&c) = self.by_text.get(&text) else { return (false, 0) };
        let icon = if c & 0x100_0000 != 0 {
            ICON_CONS
        } else if c & 0x1_0000 != 0 {
            ICON_ECO
        } else if c & 0x100 != 0 {
            ICON_WAR
        } else if c & 1 != 0 {
            ICON_SYS
        } else {
            0
        };
        if c & mask == 0 {
            return (false, icon);
        }
        let category = if c & 0xff != 0 {
            1
        } else if c & 0xff00 != 0 {
            0x100
        } else if c & 0xff_0000 != 0 {
            0x1_0000
        } else {
            0x100_0000
        };
        (mask & category != 0, icon)
    }
}

/// A class value: `<DEFINE>` names (or numbers) joined by `|`.
fn value(v: &str) -> u32 {
    v.split('|')
        .map(|t| {
            let t = t.trim();
            if let Some(name) = t.strip_prefix('<').and_then(|t| t.strip_suffix('>')) {
                DEFINES.iter().find(|(n, _)| n.eq_ignore_ascii_case(name.trim())).map_or(0, |d| d.1)
            } else {
                t.parse::<i64>().map_or(0, |n| n as u32)
            }
        })
        .fold(0, |a, b| a | b)
}

/// One message (the 0x24-byte record).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    /// String id.
    pub text: u16,
    pub player: u32,
    /// Counts down every 2 s; removed at 0; −1 never.
    pub life: i32,
    pub x: i32,
    pub y: i32,
    pub icon: u16,
    pub has_pos: bool,
    pub classified: bool,
    /// Clicked: not shown any more.
    pub read: bool,
    /// +0x20: the formatted text shown instead of the string (a chat line, 0x52a060).
    pub formatted: Option<String>,
}

/// What the logic asked for (one 0x52a460 / 0x52a320 call).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    pub text: u16,
    pub player: u8,
    pub x: i32,
    pub y: i32,
    /// 0x52a460 (filtered, icon by class); false: 0x52a320 with `filtered` = 0 (icon 0xa5), as
    /// all its logic callers pass.
    pub filtered: bool,
}

/// A `sub_12AAD0` refresh sent to the GUI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refresh {
    /// 0x267, 0x268, 0x279 or 0x27a.
    pub event: u16,
    /// The `new` flag (the event's high word).
    pub new: bool,
    /// Unread messages.
    pub count: usize,
}

/// `CTextMsgHandler` and the window's state in `CGuiEventHandler`.
#[derive(Clone, Debug)]
pub struct Messages {
    pub list: Vec<Message>,
    pub classes: Classes,
    /// `MsgLevelMask`.
    pub mask: u32,
    /// `PopupAllMsgs`.
    pub popup_all: bool,
    /// The window is open (`CGuiEventHandler +0x28` set).
    pub window: bool,
    /// The closed window's button blinks (`+0x41`), and its current image pair (0x14a / 0x14b
    /// when set, else 0x148 / 0x149).
    pub blink: bool,
    pub blink_on: bool,
    blink_wait: i32,
    /// Timer events since the last lifetime step (`DAT_012988bc`).
    ticks: u32,
    /// The window's lines changed (redraw them).
    pub dirty: bool,
    /// 0x4c4e00's slots (.data 0x146c798): (x, y, expiry tick) of the last attacks reported.
    attacks: [Option<(i32, i32, u32)>; 15],
}

impl Messages {
    pub fn new(classes: Classes, mask: u32, popup_all: bool) -> Messages {
        Messages {
            list: Vec::new(),
            classes,
            mask,
            popup_all,
            window: false,
            blink: false,
            blink_on: false,
            blink_wait: 0,
            ticks: 0,
            dirty: false,
            attacks: [None; 15],
        }
    }

    /// Tick `tick`'s [`SimEvent::Message`]s and [`SimEvent::Attacked`]s: those for `local`
    /// (the original's call sites test the owner against the local player). Returns the GUI
    /// sounds.
    pub fn sim_events(&mut self, local: u8, tick: u32, events: &[SimEvent]) -> Vec<u16> {
        let mut snd = Vec::new();
        for e in events {
            match *e {
                SimEvent::Message { player, text, x, y, filtered } if player == local => {
                    snd.extend(self.add(Request { text, player, x, y, filtered }));
                }
                SimEvent::Attacked { player, x, y } if player == local && self.attack(tick, x, y) => {
                    snd.extend(self.add(Request { text: 0x992, player, x, y, filtered: true }));
                }
                _ => {}
            }
        }
        snd
    }

    /// 0x4c4e00 `(x, y)`: whether this attack is reported (GUI_WARN_ATTACK_GEN). Not within 26
    /// (hex distance) of a reported one younger than 420 ticks (30 s), and only while one of
    /// the 15 slots is free; the attack then takes the first free slot.
    fn attack(&mut self, tick: u32, x: i32, y: i32) -> bool {
        let mut free = None;
        for (i, s) in self.attacks.iter_mut().enumerate() {
            if let Some((sx, sy, until)) = *s {
                if until >= tick {
                    let (dx, dy) = (x - sx, y - sy);
                    let d = if (dx ^ dy) < 0 { dx.abs() + dy.abs() } else { dx.abs().max(dy.abs()) };
                    if d < 26 {
                        return false;
                    }
                    continue;
                }
                *s = None;
            }
            free.get_or_insert(i);
        }
        let Some(i) = free else { return false };
        self.attacks[i] = Some((x, y, tick + 420));
        true
    }

    /// A 0x52a460 / 0x52a320 call for the local player; returns the GUI sounds.
    pub fn add(&mut self, r: Request) -> Vec<u16> {
        let (pass, icon) = self.classes.passes(r.text, self.mask);
        if r.filtered && !pass {
            return Vec::new();
        }
        let m = Message {
            text: r.text,
            player: r.player as u32,
            life: TIMEOUT / 2,
            x: r.x,
            y: r.y,
            icon: if r.filtered { icon } else { ICON_SYS },
            has_pos: true,
            classified: r.filtered,
            read: false,
            formatted: None,
        };
        if self.is_listed(&m) {
            return Vec::new();
        }
        self.push(m)
    }

    /// 0x52ac80: a message with the same text, player, x and y is in the list.
    fn is_listed(&self, m: &Message) -> bool {
        self.list.iter().any(|o| (o.text, o.player, o.x, o.y) == (m.text, m.player, m.x, m.y))
    }

    /// 0x529fe0 (append; at [`CAPACITY`] the oldest goes) with its refresh, then the generator's
    /// `sub_12AAD0(1)`. Returns the GUI sounds.
    fn push(&mut self, m: Message) -> Vec<u16> {
        if self.list.len() >= CAPACITY {
            self.list.remove(0);
        }
        self.list.push(m);
        let mut snd = Vec::new();
        for _ in 0..2 {
            let ev = self.refresh(false, true, true);
            snd.extend(self.gui_event(ev));
        }
        snd
    }

    /// 0x52a1a0 `(text, player)` (ecx, edx), the system messages of `EventManagement` 0x5172f0
    /// and of the event manager's start 0x513190: no filter, no duplicate test, no position, icon
    /// 0xa5, lifetime `DefaultMessageTimeOut / 2`. `player` is the per-player record field the
    /// callers pass (0x149b5e8 + 0x3c · p). Returns the GUI sounds.
    pub fn system(&mut self, text: u16, player: u32) -> Vec<u16> {
        self.push(Message { text, player, life: TIMEOUT / 2, x: 0, y: 0, icon: ICON_SYS, has_pos: false, classified: false, read: false, formatted: None })
    }

    /// 0x52a210 `(text, player, filtered, keep_dup, args…)`: as [`Messages::system`] with the
    /// string `vsprintf`-ed into +0x20 (`line`: the caller formats it; `None` shows the string),
    /// the class filter (0x52a660) only if `filtered` (+0x1d), and the duplicate test 0x52ac80
    /// unless `keep_dup`. Returns (the original's result: false only when the filter refused,
    /// the GUI sounds).
    pub fn formatted(&mut self, text: u16, player: u32, filtered: bool, keep_dup: bool, line: Option<String>) -> (bool, Vec<u16>) {
        if filtered && !self.classes.passes(text, self.mask).0 {
            return (false, Vec::new());
        }
        let m = Message { text, player, life: TIMEOUT / 2, x: 0, y: 0, icon: ICON_SYS, has_pos: false, classified: filtered, read: false, formatted: line };
        if !keep_dup && self.is_listed(&m) {
            return (true, Vec::new());
        }
        (true, self.push(m))
    }

    /// `Game.ShowText(name, x, y)` (0x5c7a30): the STRINGID `name` resolved by 0x5bfcd0
    /// (`s4_script::text_key_id`; unknown → nothing), then 0x52a320 `(text, 0x10000000, x, y, 0,
    /// 1)`: player 0, never expires, with a position (a click centres on (x, y)), icon 0xa5, no
    /// filter, duplicates kept. Returns the GUI sounds.
    pub fn show_text(&mut self, text: u16, x: i32, y: i32) -> Vec<u16> {
        self.push(Message { text, player: 0, life: -1, x, y, icon: ICON_SYS, has_pos: true, classified: false, read: false, formatted: None })
    }

    /// The game decided for the local player `local` (not in a tutorial: 0x5172f0 tests game
    /// type 4 first), by event 0x37 (`end_event`; `Game.DefaultGameEndCheck` 0x5c8260 packs
    /// `lose << 16 | win` into its +0xc, `texts` = (win, lose)) or by the local player's own
    /// 0x38, which already posted GUI_SYS_GAME_LOST ([`Messages::player_lost`]) and adds nothing
    /// here. 0x37 to the local team (0x517440..0x51751c): with texts the win one (the low word,
    /// even if 0), else GUI_SYS_GAME_WON (0x8cf); to another team (0x5174ab..0x51751c): with
    /// texts the lose one (word +0xe), else GUI_MSG_LOST (0x78e). Both by 0x52a1a0; a text 0
    /// shows nothing here. Player: the record field 0x149b5e8 of the local player, [inf]
    /// `local − 1` as for chat lines. Returns the GUI sounds.
    pub fn game_decided(&mut self, won: bool, end_event: bool, local: u8, texts: (u16, u16)) -> Vec<u16> {
        if !won && !end_event {
            return Vec::new();
        }
        let given = texts != (0, 0);
        let text = match (won, given) {
            (true, true) => texts.0,
            (true, false) => 0x8cf,
            (false, true) => texts.1,
            (false, false) => 0x78e,
        };
        if text == 0 {
            return Vec::new();
        }
        self.system(text, (local as u32).wrapping_sub(1))
    }

    /// A save of a decided game loaded (0x513190, at 0x5137dc..0x5138b5, not in game type 5): the
    /// winner's team the local one → GUI_SYS_GAME_WON (0x8cf), else GUI_SYS_GAME_LOST (0x8ce),
    /// by 0x52a1a0 (no script texts). Player as in [`Messages::game_decided`]. Returns the GUI
    /// sounds.
    pub fn decided_on_load(&mut self, won: bool, local: u8) -> Vec<u16> {
        self.system(if won { 0x8cf } else { 0x8ce }, (local as u32).wrapping_sub(1))
    }

    /// Event 0x38 (player `p` lost; `EventManagement` 0x5172f0): the local player →
    /// GUI_SYS_GAME_LOST (0x8ce, 0x51756b, 0x52a1a0); another → 0x52a210(0x8d1
    /// GUI_SYS_PLAYER_DESTROYED, p's record field, not filtered, duplicates kept, p's name)
    /// (0x517620): `line` is the string formatted with the name. Player fields: [inf] `p − 1`.
    /// Returns the GUI sounds.
    pub fn player_lost(&mut self, p: u8, local: u8, line: Option<String>) -> Vec<u16> {
        let rec = (p as u32).wrapping_sub(1);
        if p == local {
            return self.system(0x8ce, rec);
        }
        self.formatted(0x8d1, rec, false, true, line).1
    }

    /// Event 0x39 (a network player `p` left; `EventManagement` 0x5172f0 at 0x517763):
    /// 0x52a210(0x8d2 GUI_SYS_PLAYER_LEFT, p's record field, not filtered, duplicates kept, p's
    /// name); `line` is the string formatted with the name. Player field as in
    /// [`Messages::player_lost`]. Returns the GUI sounds.
    pub fn player_left(&mut self, p: u8, line: Option<String>) -> Vec<u16> {
        self.formatted(0x8d2, (p as u32).wrapping_sub(1), false, true, line).1
    }

    /// 0x5a2a90 (a player's turn overdue past `NotReadyWarnDelay`): 0x52a210(0x8d5, the player's
    /// number, not filtered, duplicates kept, its name). Returns the GUI sounds.
    pub fn player_waited_for(&mut self, p: u8, line: Option<String>) -> Vec<u16> {
        self.formatted(0x8d5, p as u32, false, true, line).1
    }

    /// The save's messages (`EventManagement` 0x5172f0 event 0xfa8 / 0x29, 0x5193a8..0x519449):
    /// `kind` 0 (named), 1 (QuickSave), 2 (AutoSave). A failed `SaveGame` 0x4feef0:
    /// GUI_WARN_SAVE_FAIL_DISKFULL (0x9e4); a saved one: GUI_MSG_SAVE2 (0x7a5), an autosave first
    /// AO_SAVEALERT (0x1bd, 0x52a210 filtered, duplicates kept) and GUI_MSG_SAVE2 only when that
    /// passed the filter. Player 0. Returns the GUI sounds.
    pub fn saved(&mut self, kind: u8, ok: bool) -> Vec<u16> {
        if !ok {
            return self.system(0x9e4, 0);
        }
        let mut snd = Vec::new();
        if kind == 2 {
            let (posted, s) = self.formatted(0x1bd, 0, true, true, None);
            snd.extend(s);
            if !posted {
                return snd;
            }
        }
        snd.extend(self.system(0x7a5, 0));
        snd
    }

    /// 0x52a060 `(line, player)`: a chat line (text 0x2a7, never expires, no position, icon
    /// `sub_12AC10(player)`: 0x1f9 + player for 0..=7, else 0xa5), appended (0x529fe0, with its
    /// refresh) and refreshed (`sub_12AAD0(1)`). `player` is the sender's +0x14 record field
    /// (0x149b5e8, [inf] its index p − 1). Returns the GUI sounds.
    pub fn add_chat(&mut self, line: String, player: u32) -> Vec<u16> {
        self.add_line(0x2a7, line, player)
    }

    /// 0x52a100 `(line, player)`: the same with text 0xefe (the cheat console's "Cheats
    /// activated!" / "Cheats deactivated!", player 9).
    pub fn add_console(&mut self, line: String, player: u32) -> Vec<u16> {
        self.add_line(0xefe, line, player)
    }

    fn add_line(&mut self, text: u16, line: String, player: u32) -> Vec<u16> {
        let m = Message {
            text,
            player,
            life: -1,
            x: 0,
            y: 0,
            icon: if player < 8 { 0x1f9 + player as u16 } else { ICON_SYS },
            has_pos: false,
            classified: false,
            read: false,
            formatted: Some(line),
        };
        self.push(m)
    }

    /// `sub_12AAD0(popup, new, refresh)`: the event the GUI gets.
    fn refresh(&self, popup: bool, new: bool, refresh: bool) -> Refresh {
        let count = self.list.iter().filter(|m| !m.read).count();
        // The last message a network wait or the credits header: never the "refresh" event.
        let last_sys = self.list.last().is_some_and(|m| m.text == 0x8d5 || m.text == 0x2a7);
        let (plain, refresh_ev) = if !self.popup_all && !popup { (0x27a, 0x267) } else { (0x279, 0x268) };
        Refresh { event: if refresh && !last_sys { refresh_ev } else { plain }, new, count }
    }

    /// `CGuiEventHandler` 0x4805a0 for events 0x267 / 0x268 / 0x279 / 0x27a: the sounds.
    fn gui_event(&mut self, r: Refresh) -> Vec<u16> {
        let mut snd = Vec::new();
        if matches!(r.event, 0x268 | 0x279) {
            if !self.window {
                // Event 0x269 (param 0) opens the window; its 0x1773 refresh (0x279 / 0x27a) is
                // silent. `SendAMessage` (0x45a7c0) dispatches at once.
                self.open_window();
                return snd;
            }
            if r.event == 0x268 {
                snd.push(0x4e);
            }
        }
        if r.count != 0 {
            self.blink = true;
        }
        if r.event == 0x267 && r.new {
            snd.push(0x4c);
        }
        if self.window {
            if !matches!(r.event, 0x279 | 0x27a) {
                snd.push(7);
            }
            self.dirty = true;
        }
        snd
    }

    /// Event 0x269: the button goes, the window opens (and redraws).
    fn open_window(&mut self) {
        self.window = true;
        self.blink = false;
        self.dirty = true;
    }

    /// The button (0x37b, code 3): event 0x269 and 0x1773 with 1. Returns the sounds after the
    /// button's own.
    pub fn open(&mut self) -> Vec<u16> {
        if self.window {
            return Vec::new();
        }
        self.open_window();
        let ev = self.refresh(false, false, true);
        self.gui_event(ev)
    }

    /// The close button (0x3c8): event 0x26a.
    pub fn close(&mut self) {
        if self.window {
            self.window = false;
            self.blink = false;
            self.blink_on = false;
        }
    }

    /// The 100 ms timer event 0x15: the button's blink (every 5th, while the flag is set) and
    /// the lifetimes (every 20th). Returns the sounds: an expired unread message refreshes with
    /// `sub_12AAD0(0, 0, 1)` (0x52a5d3), so an open window plays 7 (0x268: 0x4e and 7, and a
    /// closed one opens).
    pub fn timer(&mut self) -> Vec<u16> {
        if !self.window && self.blink {
            self.blink_wait -= 1;
            if self.blink_wait < 1 {
                self.blink_on = !self.blink_on;
                self.blink_wait = 5;
            }
        }
        self.ticks += 1;
        if self.ticks < 20 {
            return Vec::new();
        }
        self.ticks = 0;
        // 0x52a530.
        let mut unread_gone = false;
        self.list.retain_mut(|m| {
            if m.life > 0 {
                m.life -= 1;
            }
            if m.life == 0 {
                unread_gone |= !m.read;
                return false;
            }
            true
        });
        if unread_gone {
            let ev = self.refresh(false, false, true);
            return self.gui_event(ev);
        }
        Vec::new()
    }

    /// The unread messages, oldest first: the window's lines.
    pub fn lines(&self) -> impl Iterator<Item = (usize, &Message)> {
        self.list.iter().enumerate().filter(|(_, m)| !m.read)
    }

    /// A click on unread message `n` (0x52a800): the node to centre the camera on (event 0x13)
    /// and the sounds (after the line's own 6): a message that expires is marked read and
    /// refreshes with `sub_12AAD0(0, 0, 1)` (0x52a8cf).
    pub fn click(&mut self, n: usize) -> (Option<(i32, i32)>, Vec<u16>) {
        let Some(i) = self.lines().nth(n).map(|l| l.0) else { return (None, Vec::new()) };
        let m = &mut self.list[i];
        let pos = m.has_pos.then_some((m.x, m.y));
        if m.life == -1 {
            return (pos, Vec::new());
        }
        m.read = true;
        let ev = self.refresh(false, false, true);
        (pos, self.gui_event(ev))
    }
}

/// The 100 ms wall-clock timer of `GameTick` (0x503390): at most one event per call; a lag of
/// 10 ms or more resynchronises to `now − 10 ms`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Timer {
    prev: Option<std::time::Duration>,
}

impl Timer {
    /// `now` = wall time since any fixed start. True when event 0x15 fires.
    pub fn poll(&mut self, now: std::time::Duration) -> bool {
        let Some(prev) = self.prev else {
            self.prev = Some(now);
            return false;
        };
        let ms = std::time::Duration::from_millis;
        if now.saturating_sub(prev) < ms(100) {
            return false;
        }
        self.prev = Some(if now - prev - ms(100) < ms(10) { prev + ms(100) } else { now - ms(10) });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classes() -> Classes {
        Classes::parse(
            "[WARNINGMSG_CLASSIFICATION]\n{\n GUI_WARN_ATTACK_GEN = <WARNING_WAR_BATTLE_STARTED>\n \
             GUI_WARN_BUILD_NOPICK = <WARNING_ECO_MISSING_TOOL>\n GUI_MSG_MINECOAL = <WARNING_CONS_FOUND_RESOURCE>\n \
             GUI_WARN_TOWER_NO_CRUSH\n GUI_WARN_MINE_EMPTY=\n}\n",
        )
    }

    fn req(text: u16, x: i32) -> Request {
        Request { text, player: 1, x, y: 7, filtered: true }
    }

    #[test]
    fn classification_and_filter() {
        let c = classes();
        assert_eq!(c.passes(0x992, !0), (true, ICON_WAR));
        assert_eq!(c.passes(0x997, !0), (true, ICON_ECO));
        assert_eq!(c.passes(0x78f, !0), (true, ICON_CONS));
        assert!(!c.passes(0x9f7, !0).0 && !c.passes(0x9de, !0).0);
        // The type bit without the category bit (0x100) does not pass.
        assert!(!c.passes(0x992, 0x200).0);
        assert!(c.passes(0x992, 0x300).0);
    }

    #[test]
    fn the_shipped_file_classifies_the_warnings() {
        let path = s4_formats::game_dir().join("Config").join("WarningTypes.cfg");
        if !path.exists() {
            return;
        }
        let c = Classes::load();
        assert_eq!(c.passes(0x992, !0), (true, ICON_WAR));
        assert_eq!(c.passes(0x99b, !0), (true, ICON_ECO));
        assert_eq!(c.passes(0x780, !0), (true, ICON_CONS));
        assert!(!c.passes(0x9de, !0).0, "GUI_WARN_MINE_EMPTY= has no class");
        assert!(!c.passes(0x9f7, !0).0, "GUI_WARN_TOWER_NO_CRUSH has no value");
    }

    #[test]
    fn add_rings_twice_and_drops_duplicates() {
        let mut m = Messages::new(classes(), !0, false);
        assert_eq!(m.add(req(0x992, 5)), vec![0x4c, 0x4c]);
        assert_eq!(m.add(req(0x992, 5)), Vec::<u16>::new());
        assert_eq!(m.add(req(0x9f7, 5)), Vec::<u16>::new());
        assert_eq!(m.list.len(), 1);
        assert!(m.blink);
        // Open window: bell and 7, twice.
        assert_eq!(m.open(), vec![7]);
        assert_eq!(m.add(req(0x992, 6)), vec![0x4c, 7, 0x4c, 7]);
        // Popup mode, window closed: the first refresh opens it (silently, `SendAMessage` is
        // synchronous), the second plays 0x4e and 7.
        let mut p = Messages::new(classes(), !0, true);
        assert_eq!(p.add(req(0x992, 5)), vec![0x4e, 7]);
        assert!(p.window);
        assert_eq!(p.add(req(0x992, 6)), vec![0x4e, 7, 0x4e, 7]);
    }

    #[test]
    fn capacity_lifetime_and_click() {
        let mut m = Messages::new(classes(), !0, false);
        for x in 0..60 {
            m.add(req(0x992, x));
        }
        assert_eq!(m.list.len(), CAPACITY);
        assert_eq!(m.list[0].x, 10);
        assert_eq!(m.click(1), (Some((11, 7)), Vec::new()));
        m.open();
        assert_eq!(m.click(1), (Some((12, 7)), vec![7]));
        assert_eq!(m.lines().count(), CAPACITY - 2);
        // 120 s = 1200 timer events.
        for _ in 0..1199 {
            m.timer();
        }
        assert_eq!(m.list.len(), CAPACITY);
        assert_eq!(m.timer(), vec![7]);
        assert!(m.list.is_empty());
    }

    #[test]
    fn attacks_are_throttled() {
        let mut m = Messages::new(classes(), !0, false);
        let hit = |x, y| [SimEvent::Attacked { player: 1, x, y }];
        assert_eq!(m.sim_events(1, 10, &hit(100, 100)), vec![0x4c, 0x4c]);
        // Near (hex distance 25) and young: no message; far: a message (new text position).
        assert!(m.sim_events(1, 11, &hit(125, 100)).is_empty());
        assert_eq!(m.sim_events(1, 12, &hit(113, 87)).len(), 2);
        // Opposite signs add up: (−13, +13) is 26 away.
        assert_eq!(m.list.len(), 2);
        // The slot holds until tick 430; (101, 100) is 25 from the second report.
        assert!(m.sim_events(1, 430, &hit(140, 100)).len() == 2);
        assert!(m.sim_events(1, 431, &hit(101, 100)).is_empty());
        assert_eq!(m.sim_events(1, 433, &hit(101, 100)).len(), 2);
        // Another player's entity.
        assert!(m.sim_events(2, 434, &hit(500, 500)).is_empty());
    }

    #[test]
    fn system_messages_and_saves() {
        let mut m = Messages::new(Classes::parse("[WARNINGMSG_CLASSIFICATION]\n{\n AO_SAVEALERT = <WARNING_SYS_NOT_SAVED>\n}\n"), !0, false);
        // 0x52a1a0: no duplicate test, no position, icon 0xa5.
        assert_eq!(m.system(0x8cf, 0), vec![0x4c, 0x4c]);
        assert_eq!(m.system(0x8cf, 0), vec![0x4c, 0x4c]);
        assert_eq!((m.list[0].icon, m.list[0].has_pos, m.list[0].life), (ICON_SYS, false, TIMEOUT / 2));
        assert_eq!(m.click(0).0, None);
        // Saves: named / quick → GUI_MSG_SAVE2; auto → AO_SAVEALERT (kept as a duplicate), then
        // GUI_MSG_SAVE2; failed → GUI_WARN_SAVE_FAIL_DISKFULL.
        m.list.clear();
        m.saved(1, true);
        m.saved(2, true);
        m.saved(2, true);
        m.saved(0, false);
        assert_eq!(m.list.iter().map(|x| (x.text, x.classified)).collect::<Vec<_>>(), [
            (0x7a5, false),
            (0x1bd, true),
            (0x7a5, false),
            (0x1bd, true),
            (0x7a5, false),
            (0x9e4, false)
        ]);
        // Filtered out (SYS bit clear): no AO_SAVEALERT and no GUI_MSG_SAVE2.
        m.list.clear();
        m.mask = !1;
        assert!(m.saved(2, true).is_empty() && m.list.is_empty());
        // 0x52a210 without `keep_dup` drops a duplicate but reports it posted.
        m.mask = !0;
        assert_eq!(m.formatted(0x8d1, 3, false, false, Some("x".into())).1.len(), 2);
        assert_eq!(m.formatted(0x8d1, 3, false, false, Some("x".into())), (true, Vec::new()));
    }

    #[test]
    fn script_and_end_messages() {
        let mut m = Messages::new(classes(), !0, false);
        // Game.ShowText: never expires, a click centres but does not mark it read; kept as a
        // duplicate.
        m.show_text(0x500, 10, 20);
        m.show_text(0x500, 10, 20);
        assert_eq!(m.click(0), (Some((10, 20)), Vec::new()));
        assert_eq!(m.lines().count(), 2);
        assert_eq!(m.list[0].life, -1);
        m.list.clear();
        // Another player lost: 0x8d1 with the formatted line (kept as a duplicate); the local
        // one: 0x8ce and nothing more for that loss. Won: the script's text, else 0x8cf; another
        // team won (0x37): the script's lose text, else 0x78e; a win text alone: nothing.
        m.player_lost(3, 1, Some("x".into()));
        m.player_lost(3, 1, Some("x".into()));
        m.player_lost(1, 1, None);
        m.game_decided(false, false, 1, (0, 0));
        m.game_decided(true, true, 1, (0, 0));
        m.game_decided(true, true, 1, (0x123, 0x124));
        m.game_decided(false, true, 1, (0x123, 0x124));
        m.game_decided(false, true, 1, (0, 0));
        m.game_decided(false, true, 1, (0x123, 0));
        // A decided game loaded: 0x8cf / 0x8ce, no script texts.
        m.decided_on_load(true, 1);
        m.decided_on_load(false, 1);
        assert_eq!(m.list.iter().map(|x| (x.text, x.player)).collect::<Vec<_>>(), [(0x8d1, 2), (0x8d1, 2), (0x8ce, 0), (0x8cf, 0), (0x123, 0), (0x124, 0), (0x78e, 0), (0x8cf, 0), (0x8ce, 0)]);
        assert_eq!(m.list[0].formatted.as_deref(), Some("x"));
    }

    #[test]
    fn timer_resyncs() {
        let ms = std::time::Duration::from_millis;
        let mut t = Timer::default();
        assert!(!t.poll(ms(0)));
        assert!(!t.poll(ms(99)));
        assert!(t.poll(ms(105)));
        assert!(!t.poll(ms(150)));
        assert!(t.poll(ms(200)));
        // A long frame: one event, then 100 ms from now − 10.
        assert!(t.poll(ms(1000)));
        assert!(!t.poll(ms(1089)));
        assert!(t.poll(ms(1090)));
    }
}
