//! GUI interaction: which dialogs are open, each control's state, and the mouse handling of
//! `GuiEngine2_EventProc` (0x2f99880), turned into typed [`UiEvent`]s instead of calls into the
//! dialog procedures. No game commands are executed (the sim has no command handlers yet); the
//! procedures' *GUI* reactions (switching tabs and pages) are reproduced in [`GuiState::react`].
//!
//! # Control state (`SGuiControl::effects`)
//! 1 pressed/selected, 2 highlighted (hover), 4 greyed, 8 hidden. `FindControlUnderCursor`
//! (0x2f9da30) searches the open dialogs from surface 13 down to 0 and skips greyed and hidden
//! controls; a click only counts on a non-transparent GUI pixel (`IsNonTransparentGuiArea`).
//!
//! # Mouse (event ids as `GuiEngine2_EventProc` receives them)
//! * Move: the control under the cursor becomes the selected one (highlight on, the previous one
//!   off) and its procedure gets code 9 with the extended tooltip id.
//! * Left button down: type 0/13/19 buttons get pressed and become the drag control; type 1
//!   toggles flip and fire code 3; types 2/3 (tab, radio) go through `SelectRadioGroup`: the others
//!   of the same group (byte +28) in the dialog are released, the control is pressed, code 3 (a
//!   pressed type-3 radio fires code 3 and is released again; a pressed tab does nothing); types
//!   17/18 (repeat buttons) press and fire code 3.
//! * Left button up: a pressed type 0/13/19 is released and fires code 3.
//! * Right button down: types 0..3, 5..8, 10, 13, 19, 20 fire code 7.
//!
//! The procedure's `a2` is `effects << 16 | link`; we report both.
//!
//! # Reactions reproduced
//! * `GuiDlgMinimapProc` code 3: main tabs 606..=611 (0x25e..0x263) reset the open submenu and
//!   page (`MinimpapResetOpenDialogs`) and open the submenu dialog 3..=8.
//! * `GuiDlgBuildSubmenuProc` code 3: tabs 536..=540 select the build page; `CGuiEventHandler`
//!   event 0x1C opens page 0 → dialog 9 (basic), 1 → 11 (food), 2 → 10 (metal), 3 → 12 (town),
//!   4 → 13 (military). Tab order: 536 basic, 537 metal (page 2), 538 food (1), 539 town (3),
//!   540 military (4).
//! * Opening a submenu selects its first tab (`GuiDlgBuildSubmenuProc(0)`: `SelectControl(3, 536)`).
//!   **[unk]** The pages of submenus 4..=8 (figures, production, statistics, options, load/save) are
//!   opened through events not traced yet; for those only the submenu bar opens.

use std::collections::HashMap;

use s4_formats::gui::{Dialog, GuiSet};

mod slider;

/// A GUI event as the dialog procedures receive it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UiEvent {
    /// Code 3: a button, toggle, tab or radio was activated.
    Command { dialog: usize, link: u16, effects: u8 },
    /// Code 1: a control's pressed bit (or an edit field's focus bit 0x40) changed; `effects` is
    /// the new state. Only in [`GuiState::calls`].
    State { dialog: usize, link: u16, effects: u8 },
    /// Code 7: right click on a control.
    Context { dialog: usize, link: u16 },
    /// Code 2: a slider knob's value changed (`guistate::slider`).
    Slider { dialog: usize, link: u16, value: i32 },
    /// Code 9: the cursor entered a control; `tooltip` is its extended tooltip string id.
    Hover { dialog: usize, link: u16, tooltip: Option<u16> },
    DialogOpened(usize),
    DialogClosed(usize),
    /// The selection changed (`m_vSelection`): entity ids in selection order, empty = cleared.
    Selection(Vec<u16>),
    /// Enter pressed in a focused edit field (kind 20): its text is in `texts`.
    EditDone { dialog: usize, link: u16 },
    /// Code 2 of an edit field: a typed character was inserted or one deleted (0x672f90 case
    /// 0xd, the key event). Only in [`GuiState::calls`].
    Edit { dialog: usize, link: u16 },
    /// `CInputProcessor::SendTo` (right click on the map with a selection): net command 0x1396
    /// for the selected entities to node `(x, y)`; `mode` bit 0 = Ctrl (`wParam & 8`), bit 1 =
    /// Shift (`& 4`), bit 2 = middle button (`& 0x10`), 8 = stand ground (no target). Not sent:
    /// the sim has no military commands yet.
    SendTo { ids: Vec<u16>, x: u16, y: u16, mode: u8 },
    /// Codes 5 / 6: the cursor entered (`on`) or left a control of a highlighting kind (0..=3, 5,
    /// 7..=13, 17..=19; `AnotherEventHandler_F7` 0x672f90 event 5, effects bit 2).
    Highlight { dialog: usize, link: u16, on: bool },
    /// Code 10: the mouse wheel over a dialog (event 0xe), `delta` as WM_MOUSEWHEEL's high word.
    Wheel { dialog: usize, delta: i32 },
}

/// The control kinds whose highlight (effects bit 2) the procedure hears (codes 5 / 6).
fn highlights(kind: u8) -> bool {
    matches!(kind, 0..=3 | 5 | 7..=13 | 17..=19)
}

/// The building a build-page button stands for: `GuiDlgBuild{Basic,Metal,Food,Town,Military}Proc`
/// code 3 sends `CEvn_Logic(0x1B5, 0, type)` (→ `CInputProcessor::TryBuild`: build mode for
/// `type`) when the toggle is pressed, `0x191` (leave the mode) when it is released. Pages: dialog
/// 9 basic, 10 metal, 11 food, 12 town, 13 military; 52 the eye catchers' sidebar.
pub fn build_button(dialog: usize, link: u16) -> Option<u8> {
    const T: &[(usize, u16, u8)] = &[
        (9, 373, 3),
        (9, 375, 2),
        (9, 377, 4),
        (9, 379, 1),
        (9, 381, 40),
        (9, 382, 41),
        (9, 383, 42),
        (9, 401, 14),
        (10, 474, 15),
        (10, 476, 20),
        (10, 478, 17),
        (10, 480, 16),
        (10, 481, 19),
        (10, 482, 21),
        (10, 492, 22),
        (10, 498, 18),
        (10, 2904, 25),
        (11, 428, 12),
        (11, 430, 8),
        (11, 432, 11),
        (11, 434, 9),
        (11, 435, 6),
        (11, 436, 7),
        (11, 446, 5),
        (11, 451, 10),
        (11, 456, 36),
        (11, 457, 37),
        (11, 2035, 35),
        (11, 2876, 82),
        (11, 2877, 81),
        (11, 2900, 38),
        (11, 2901, 39),
        (12, 541, 33),
        (12, 543, 31),
        (12, 545, 34),
        (12, 547, 13),
        (12, 556, 32),
        (12, 557, 43),
        (12, 558, 44),
        (13, 505, 46),
        (13, 507, 48),
        (13, 509, 24),
        (13, 511, 47),
        (13, 512, 28),
        (13, 513, 27),
        (13, 525, 45),
        (13, 526, 23),
    ];
    if dialog == crate::eyecatcher_sidebar::DIALOG {
        // The eye catchers' sidebar (procedure 0x46ecf0) works as a build page.
        return crate::eyecatcher_sidebar::BUTTONS.iter().find(|b| b.0 == link).map(|b| b.1);
    }
    T.iter().find(|&&(d, l, _)| d == dialog && l == link).map(|&(_, _, t)| t)
}

/// `CInputProcessor +0x65` for the whole session (see [`GuiState::fill_global`]): true = 0 =
/// the player's totals.
pub static FILL_GLOBAL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// The page dialogs below the submenus (one is open at a time).
pub const PAGES: [usize; 24] = [9, 10, 11, 12, 13, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 58, 75, 76, 77];

/// The parts of the pages' info objects (static globals in the original) that a fill without an
/// own eco-sector leaves as they were.
#[derive(Clone, Debug, Default)]
pub struct PageInfo {
    /// Free carrier page (0x585190): the offered shovels and hammers (info +0x20 / +0x30).
    pub tools: (i32, i32),
    /// Specialist page (0x584fb0): per type (type, count, wish, tool offered, tools offered);
    /// `None` before the first fill with an own eco-sector.
    pub specialists: Option<[SpecialistRow; 5]>,
    /// Transport priority page (25).
    pub transport: TransportInfo,
    /// Goods distribution page (26).
    pub distribution: DistributionInfo,
    /// The message recipients page (58).
    pub chat: ChatInfo,
    /// The briefing page (35): string 880 and the mission goal (0x149be6c, s4-game briefing.rs).
    pub briefing: (String, String),
}

/// The message recipients page (dialog 58, procedure 0x474560, fill 0x473e90), which the extras
/// submenu's tab 931 opens (`ExtrasMenuCallback` 0x47b4e0: UI event 0x1e → `EventManagement`
/// 0x1e → GUI event 0x26c with `CStateGame +0x160` → 0x483010 case 4) and Esc with it
/// (`HandleF6_F7_Esc` 0x489440). Its record (`SetSettings` 0x514d30): +0x160 = 4, +0x168 the
/// players (`GlobalGameState +0x68`), +0x16c the local player − 1, per player (+0x170 + 0x24 i)
/// the name, +0x18 the colour (`PlayerColorIndex`), +0x1c the kind (2 human, 3 / 4 for the
/// player types 2 / 3: computer), +0x20 "receives my messages" (computers never; humans by the
/// recipients mask 0x11b5930, bit p − 1), +0x21 "another team" (0x12dcd0c).
#[derive(Clone, Debug)]
pub struct ChatInfo {
    pub slots: Vec<ChatSlot>,
    /// What the client knows of the players besides the population: the names and the human
    /// players (bit p − 1; the local player is always human). [unk] The single player game's
    /// computer names (`SetSettings`' `RetrieveData`) are not known: empty.
    pub names: [String; 9],
    pub humans: u32,
    pub local: usize,
    /// 0x11b5930: who receives the local player's messages (bit p − 1).
    pub mask: u32,
    /// 0x15aa5be: a fill runs (the radios' code 3 does nothing).
    pub filling: bool,
}

/// One player of [`ChatInfo`].
#[derive(Clone, Debug, Default)]
pub struct ChatSlot {
    pub name: String,
    pub colour: u8,
    pub kind: u8,
    pub team: u8,
}

impl Default for ChatInfo {
    fn default() -> Self {
        // [inf] The recipients mask's default (a config value) taken as everyone.
        ChatInfo { slots: Vec::new(), names: Default::default(), humans: 0, local: 0, mask: u32::MAX, filling: false }
    }
}

impl ChatInfo {
    /// `SetSettings` 0x514d30: the record from the population (players 1..= the highest one in
    /// the game) for local player `local`. [inf] Player p has colour p − 1 (the client keeps no
    /// player colours).
    pub fn set_players(&mut self, pop: &crate::population::Population, local: u8) {
        let count = (1..9).rev().find(|&p| pop.teams[p] != 0).unwrap_or(0);
        self.local = (local as usize).saturating_sub(1);
        self.slots = (1..=count)
            .map(|p| ChatSlot {
                name: self.names[p].clone(),
                colour: (p - 1) as u8,
                kind: if p == local as usize || self.humans & 1 << (p - 1) != 0 { 2 } else { 3 },
                team: pop.teams[p],
            })
            .collect();
    }

    /// +0x20: the slot receives the local player's messages.
    pub fn receives(&self, i: usize) -> bool {
        self.slots.get(i).is_some_and(|s| s.kind == 2) && self.mask & 1 << i != 0
    }

    /// `EventManagement` 0x24 (the buttons 0x24b / 0x24c / 0x24d: 0 all, 1 allies, 2 enemies):
    /// the mask of the other human players, all or those of the local team or of other teams.
    /// [inf] `sub_F7A40` (players it skips) taken as the computer players.
    pub fn select(&mut self, mode: u32) {
        let own = self.slots.get(self.local).map_or(0, |s| s.team);
        let mut mask = 0;
        for (i, s) in self.slots.iter().enumerate() {
            if i == self.local || s.kind != 2 {
                continue;
            }
            let ok = match mode {
                1 => s.team == own,
                2 => s.team != own,
                _ => true,
            };
            if ok {
                mask |= 1 << i;
            }
        }
        self.mask = mask;
    }
}

/// The goods distribution page's info (`GlobalGoodDistributionInfo` 0x11aaea0, filled by
/// 0x4df9d0) and the procedure's slider values (0x15aa5cc).
#[derive(Clone, Debug, Default)]
pub struct DistributionInfo {
    /// +0x3c (0x11aaedc): the good, set by the tabs' UI event 0x1b2 (0x4ea450).
    pub good: i32,
    /// +0x40: the eco-sector of the last fill that showed (-1 none; the tabs set -1 to force one).
    pub last_eco: i32,
    /// +8, +0xc: the number of buildings and the race.
    pub count: i32,
    pub race: i32,
    /// +0x10: (building type, share) of the eco-sector's supply priorities for the good.
    pub list: [(i32, i32); 5],
    /// The display (info event 0x25c → 0x476130) is due.
    pub show: bool,
    /// The display is the one a knob's release runs (code 3): the original runs it inside the
    /// release's callback, while the knob is still the drag control, so `SetSliderPosition`
    /// leaves the whole group alone (no move, a lock stays; 0x6772d0, 0x673f88..0x673fb6).
    pub release: bool,
    /// `goodDistributionSliderValues` (0x15aa5cc).
    pub values: [i32; 5],
}

/// `ProductionOrderMenuCallback` (0x476810) code 3: the goods distribution tabs and their goods.
pub const DISTRIBUTION_TABS: [(u16, i32); 8] = [(0x308, 10), (0x309, 20), (0x30a, 16), (0x30b, 36), (0x30c, 9), (0x30d, 24), (0x30e, 11), (0xb40, 22)];

/// The transport priority page's state: the info at 0x11aaa38 (filled by 0x4dfae0: +0xc the
/// number of ranks, +0x10 the goods in rank order) and the procedure's globals (0x15aac98: the
/// good shown by each link of [`TRANSPORT_LINKS`], 0x15aad20: the selected good).
#[derive(Clone, Debug)]
pub struct TransportInfo {
    pub count: i32,
    pub goods: [i32; 42],
    pub link_goods: [i32; 34],
    pub target: i32,
}

impl Default for TransportInfo {
    fn default() -> Self {
        TransportInfo { count: 0, goods: [0; 42], link_goods: [-1; 34], target: -1 }
    }
}

/// .rdata 0xfffb60: the transport priority page's good slots in rank order (`sub_ACCC0`); the
/// procedure (0x4acf20) maps a clicked link back to its slot. Entries 34.. lie past the 34 slots.
pub const TRANSPORT_LINKS: [u16; 42] = [
    0x687, 0x688, 0x689, 0x68a, 0x68b, 0x68c, 0x68d, 0x68e, 0x68f, 0x690, 0x696, 0x69f, 0x69e, 0x691, 0x692, 0x693, 0x694,
    0x695, 0x69a, 0x699, 0x698, 0x697, 0x6a2, 0x6a1, 0x6a0, 0x69b, 0x69c, 0x69d, 0x6a8, 0x6a9, 0x6aa, 0x6ab, 0x6ac, 0x6ad, 0,
    0, 0, 0, 0, 0x19b, 0x1f0, 0x1f1,
];

/// One specialist type of the page 0x584fb0 fills: (type, count, wish, tool offered, tools offered).
pub type SpecialistRow = (u16, i32, i32, bool, i32);

/// Open dialogs and per-control state over a GUI set.
pub struct GuiState {
    pub set: GuiSet,
    /// Open dialog slots (draw order).
    pub open: Vec<usize>,
    /// `effects` of every control, by (dialog slot, control index).
    pub effects: HashMap<(usize, usize), u8>,
    /// Texts set with `SetText`, by (dialog slot, link).
    pub texts: HashMap<(usize, u16), String>,
    /// Font templates set with `SetFontTemplate`, by (dialog slot, link).
    pub fonts: HashMap<(usize, u16), u8>,
    /// Owner-drawn images (`LockOwnerImage` … `UnlockOwnerImage`): (width, height, RGBA) drawn at
    /// the control's top-left, by (dialog slot, link).
    pub owner_images: HashMap<(usize, u16), (u32, u32, Vec<u8>)>,
    /// `SetSliderPosition` values (0..=100) of bar controls (kind 10), by (dialog slot, link).
    pub bars: HashMap<(usize, u16), u8>,
    /// First shown line of scrolled multi-line texts (`CScrollMultiline`), by (dialog, link).
    pub scroll: HashMap<(usize, u16), usize>,
    /// The edit field (kind 20) that takes the keyboard, by (dialog, link).
    pub focus: Option<(usize, u16)>,
    /// `IGuiEngine::SetImages`: (image, pressed image) replacing a control's own, by (dialog, link).
    pub images: HashMap<(usize, u16), (u16, u16)>,
    /// Tooltip ids written into a control (`SGuiControl` +0x14 / +0x16), by (dialog, link).
    pub tooltips: HashMap<(usize, u16), (u16, u16)>,
    /// Slider knobs (kinds 7 / 8): (value +0x1e, value saved at the last knob press +0x21), by
    /// (dialog slot, control index). See [`slider`].
    pub sliders: HashMap<(usize, usize), (i8, i8)>,
    /// The statistic pages' shared mode, `CInputProcessor +0x65` (0x148659d) = 0: the player's
    /// totals instead of the eco-sector at the screen centre (the fills, e.g. 0x584c40 / 0x53d5a0,
    /// take the eco-sector only when it is not 0). Toggled by 1350 (dialog 31), 323 (27) and 563
    /// (12), which all send event 0x1B8 (handler 0x4edaa0 flips it). `CInputProcessor` is a static
    /// whose constructor (0x4e6a70) and reset (0x4e95c0) never write it: global at the start, and
    /// it keeps its state from game to game ([`FILL_GLOBAL`]).
    pub fill_global: bool,
    /// `CInputProcessor`'s state as the client last set it (s4-game input.rs): the handlers of
    /// the procedures' UI events run only where it has a transition (here 0x1b8, state 0).
    pub input_state: u8,
    /// What the pages' info objects keep between fills (see [`PageInfo`]).
    pub page_info: PageInfo,
    /// The dialog procedures' code 1, 3 and 7 calls since the caller last drained it, in order
    /// ([`UiEvent::State`], [`UiEvent::Command`], [`UiEvent::Context`]): whatever path caused them (mouse,
    /// `SelectControl` from an init or a reaction). The procedures play their GUI sounds here.
    pub calls: Vec<UiEvent>,
    /// The GUI engine's control hook 0x146b054 (the tutorial's 0x4fa520, set by
    /// `SetupTutorialEvent` 0x4fa5b0): `(dialog, link, control kind, enabled, flags) → may be
    /// enabled`. `OpenDialog` (0x671ed0) asks it for every control before (flags 3) and after
    /// (flags 1) the init, `Resize` (0x675ea0, [`GuiState::rehook`]) with flags 4,
    /// `EnableControl` (0x676cd0) with flags 8 and the caller's wish; the answer sets or clears the
    /// greyed bit 4 (the wish itself is then ignored).
    pub hook: Option<ControlHook>,
    /// 0x11695d4: the dialog the hook leaves alone (0x24 while a tutorial runs).
    pub hook_exempt: Option<usize>,
    /// 0x15aa5c6: game type 3 (set by the extras submenu's init, `ExtrasMenuCallback` 0x47b4e0
    /// code 0): its 929 sends UI 0x1b4 (the goods race, `crate::goods_race`) instead of opening
    /// the briefing page 35.
    pub goods_race: bool,
    /// `GlobalGameState +0x2a8`, the kind of game: 1 a local free game (0x520efb), 2 a network
    /// game (0x51bd41 / 0x5a0337 / 0x520efb), 3 a campaign mission (the briefings 0x510d27,
    /// 0x50e46e, 0x5239f7, 0x524877, 0x52915a), 4 a tutorial (0x527e63); 0 before a game.
    pub game_kind: u32,
    selected: Option<(usize, usize)>,
    drag: Option<(usize, usize)>,
}

/// See [`GuiState::hook`].
pub type ControlHook = Box<dyn FnMut(u32, u32, u32, bool, u8) -> bool + Send>;

impl GuiState {
    pub fn new(set: GuiSet) -> Self {
        let mut effects = HashMap::new();
        for d in set.dialogs.iter().flatten() {
            for (i, c) in d.controls.iter().enumerate() {
                effects.insert((d.slot, i), c.effects);
            }
        }
        let fill_global = FILL_GLOBAL.load(std::sync::atomic::Ordering::Relaxed);
        GuiState { set, open: Vec::new(), effects, texts: HashMap::new(), fonts: HashMap::new(), owner_images: HashMap::new(), bars: HashMap::new(), scroll: HashMap::new(), focus: None, images: HashMap::new(), tooltips: HashMap::new(), sliders: HashMap::new(), fill_global, input_state: 0, page_info: PageInfo::default(), calls: Vec::new(), hook: None, hook_exempt: None, goods_race: false, game_kind: 0, selected: None, drag: None }
    }

    fn dialog(&self, slot: usize) -> Option<&Dialog> {
        self.set.dialog(slot)
    }

    /// `IGuiEngine::OpenDialog` (0x2f9b380): refused while the dialog's surface shows another one;
    /// clears the pressed bit of every control, then runs the procedure's init (code 0).
    pub fn open(&mut self, slot: usize, ev: &mut Vec<UiEvent>) {
        let Some(d) = self.dialog(slot) else { return };
        let surface = d.surface;
        let n = d.controls.len();
        if self.open.iter().any(|&s| self.dialog(s).map(|d| d.surface) == Some(surface)) {
            return;
        }
        for i in 0..n {
            self.set_effect((slot, i), 1, false);
        }
        self.apply_hook(slot, 3);
        self.open.push(slot);
        ev.push(UiEvent::DialogOpened(slot));
        self.init(slot, ev);
        self.apply_hook(slot, 1);
    }

    /// The hook over every control of `slot` (see [`GuiState::hook`]).
    fn apply_hook(&mut self, slot: usize, flags: u8) {
        if self.hook.is_none() || self.hook_exempt == Some(slot) {
            return;
        }
        let Some(controls) = self.dialog(slot).map(|d| d.controls.iter().map(|c| (c.link, c.kind)).collect::<Vec<_>>()) else { return };
        for (i, (link, kind)) in controls.into_iter().enumerate() {
            let enabled = self.effects.get(&(slot, i)).copied().unwrap_or(0) & 4 == 0;
            let ok = self.hook.as_mut().is_some_and(|h| h(slot as u32, link as u32, kind as u32, enabled, flags));
            self.set_effect((slot, i), 4, !ok);
        }
    }

    /// `IGuiEngine::Resize` (0x675ea0) as far as the hook goes: every open dialog's controls asked
    /// again (flags 4). The tutorial's control changes end with it.
    pub fn rehook(&mut self) {
        for slot in self.open.clone() {
            self.apply_hook(slot, 4);
        }
    }

    /// The procedures' init (code 0) as far as it touches the GUI.
    fn init(&mut self, slot: usize, ev: &mut Vec<UiEvent>) {
        let mut more = Vec::new();
        match slot {
            // The submenus' init (code 0) selects their first tab, whose command opens the page:
            // GuiDlgBuildSubmenuProc SelectControl(3, 536), GuiDlgFiguresSubmenuProc (4, 747),
            // GuiDlgProductionSubmenuProc (5, 1069), GuiDlgStatisticSubmenuProc (6, 1560),
            // GuiDlgOptionsSubmenuProc (7, 1021), GuiDlgLoadSaveSubmenuProc (8, 928).
            3 => self.select(3, 536, &mut more),
            4 => self.select(4, 747, &mut more),
            5 => self.select(5, 1069, &mut more),
            6 => self.select(6, 1560, &mut more),
            7 => self.select(7, 1021, &mut more),
            // ExtrasMenuCallback 0x47b4e0 code 0: then 929 enabled only with a mission goal
            // (0x149be6c not "", 0x47b9a0) or in game type 3 (0x15aa5c6, `goods_race`).
            8 => {
                self.select(8, 928, &mut more);
                let on = self.goods_race || !self.page_info.briefing.1.is_empty();
                self.set_greyed(8, 929, !on);
            }
            // The briefing page's init 0x46cce0: SetText(35, 369, string 880), (35, 370, goal).
            35 => {
                let (header, goal) = self.page_info.briefing.clone();
                self.texts.insert((35, 369), header);
                self.texts.insert((35, 370), goal);
            }
            // UpdateGuiDlgBuildBasic: SelectControl(9, 394, "show all" flag clear).
            9 => self.select(9, 394, &mut more),
            // GuiDlgSettlerProductionMenuProc code 0 (0x4a4ae0 → 0x4a4630): the recruiting target
            // 0; in a campaign mission (`GlobalGameStateObject +0x3c` = `GlobalGameState +0x2a8` = 3,
            // `game_kind`) the saboteur's row (1253, 1266, 1260, 1277, 1282) hidden; the thief's
            // tool icon and count (1267, 1281) hidden, then radio 1251 clicked (`UIEventMethod`, code
            // 3 → target 50) as it is not pressed (`OpenDialog` released every control).
            30 => {
                if self.game_kind == 3 {
                    for l in [1253, 1266, 1260, 1277, 1282] {
                        self.set_hidden(30, l, true);
                    }
                }
                for l in [1267, 1281] {
                    self.set_hidden(30, l, true);
                }
                self.select(30, 1251, &mut more);
            }
            // `FindUnitTabCallback` code 0 (0x4a3eb0 → 0x4a3cf0): the type 0, in a campaign mission
            // tab 1242 hidden, then tab 1240 clicked.
            29 => {
                if self.game_kind == 3 {
                    self.set_hidden(29, 1242, true);
                }
                self.select(29, 1240, &mut more);
            }
            // ProductionOrderMenuCallback code 0 (0x476810 → 0x476680): the slider values 0, the
            // knobs' images (0x11f, 0x151), SelectControl(26, 0x308).
            26 => {
                self.page_info.distribution.values = [0; 5];
                for l in 795..=799 {
                    self.images.insert((26, l), (0x11f, 0x151));
                }
                self.select(26, 0x308, &mut more);
            }
            // TransportPrioTabCallback code 0 (0x4acf20): no good selected, and `sub_ACEC0`
            // (0x4acec0) sets every slot's good to -1 (.rdata 0x1071710).
            25 => {
                self.page_info.transport.target = -1;
                self.page_info.transport.link_goods = [-1; 34];
            }
            // GuiDlgFreeCarrierMenuProc code 0 (0x475dc0): target 0, button 755 hidden,
            // SelectControl(32, 765, 1) (code 3 → target 1).
            32 => {
                self.set_hidden(32, 755, true);
                self.select(32, 765, &mut more);
            }
            _ => {}
        }
        for e in more.clone() {
            self.react(&e, &mut more);
        }
        ev.extend(more);
    }

    pub fn close(&mut self, slot: usize, ev: &mut Vec<UiEvent>) {
        if let Some(i) = self.open.iter().position(|&s| s == slot) {
            self.open.remove(i);
            ev.push(UiEvent::DialogClosed(slot));
        }
    }

    /// `PrintDistributionValues` (0x476070): per building of the info, the knob's value (-1 → 0)
    /// as the slider value and the text "n%" (800 + i).
    pub fn print_distribution(&mut self) {
        let n = self.page_info.distribution.count;
        for i in 0..n.clamp(0, 5) as usize {
            let v = self.knob_value(26, 795 + i as u16).map_or(-1, |v| v as i32).max(0);
            self.page_info.distribution.values[i] = v;
            self.texts.insert((26, 800 + i as u16), format!("{v}%"));
        }
    }

    /// 0x6775a0 (`ResetRadioGroup`): release every pressed control of `link`'s group (+0x1c),
    /// `link` included.
    pub fn release_group(&mut self, slot: usize, link: u16) {
        let Some((_, i)) = self.find_link(slot, link) else { return };
        let Some(d) = self.dialog(slot) else { return };
        let group = d.controls[i].group;
        let keys: Vec<usize> = d.controls.iter().enumerate().filter(|(_, c)| c.group == group).map(|(j, _)| j).collect();
        for j in keys {
            // Code 1 for each released member (0x6775a0).
            self.set_pressed((slot, j), false);
        }
    }

    /// `UIEventMethod` on a tab / radio while its procedure ignores the code 3 it gets: pressed.
    pub fn press(&mut self, slot: usize, link: u16) {
        if let Some(key) = self.find_link(slot, link) {
            self.set_pressed(key, true);
        }
    }

    /// `IGuiEngine::EnableControl(dialog, link, !greyed)`: effects bit 4.
    pub fn set_greyed(&mut self, slot: usize, link: u16, greyed: bool) {
        if let Some(key) = self.find_link(slot, link) {
            let kind = self.control(key).map_or(0, |c| c.kind);
            // With the hook (0x676cd0): its answer to the wish (flags 8), the wish ignored.
            let greyed = match self.hook.as_mut() {
                Some(h) => !h(slot as u32, link as u32, kind as u32, !greyed, 8),
                None => greyed,
            };
            self.set_effect(key, 4, greyed);
        }
    }

    /// `IGuiEngine::SetControlVisibility`: effects bit 8.
    pub fn set_hidden(&mut self, slot: usize, link: u16, hidden: bool) {
        if let Some(key) = self.find_link(slot, link) {
            self.set_effect(key, 8, hidden);
        }
    }

    /// The hidden bit set directly in the control record with the procedure's code 1 (as the
    /// end statistics' fill 0x490fe0 does): a call only when it changes.
    pub fn set_hidden_notify(&mut self, slot: usize, link: u16, hidden: bool) {
        if let Some(key) = self.find_link(slot, link) {
            self.set_notify(key, 8, hidden);
        }
    }

    pub fn is_pressed(&self, slot: usize, idx: usize) -> bool {
        self.effects.get(&(slot, idx)).is_some_and(|e| e & 1 != 0)
    }

    fn set_effect(&mut self, key: (usize, usize), bit: u8, on: bool) {
        if let Some(e) = self.effects.get_mut(&key) {
            *e = if on { *e | bit } else { *e & !bit };
        }
    }

    /// `set_effect` where the original notifies the procedure: on a change of `bit`, code 1 with
    /// the new state (`AnotherEventHandler_F7` 0x672f90, `UIEventMethod` 0x674c40,
    /// `SelectControl` 0x677430, the edit focus 0x6756a0).
    fn set_notify(&mut self, key: (usize, usize), bit: u8, on: bool) {
        let Some(&old) = self.effects.get(&key) else { return };
        self.set_effect(key, bit, on);
        let effects = self.effects.get(&key).copied().unwrap_or(old);
        if effects != old
            && let Some(c) = self.control(key)
        {
            self.calls.push(UiEvent::State { dialog: key.0, link: c.link, effects });
        }
    }

    /// The pressed bit of (dialog, link).
    pub fn is_pressed_link(&self, slot: usize, link: u16) -> bool {
        self.find_link(slot, link).is_some_and(|k| self.is_pressed(k.0, k.1))
    }

    /// Set the pressed bit of (dialog, link) without a command (`SelectControl` of a kind-1
    /// toggle, 0x677430).
    pub fn set_pressed_link(&mut self, slot: usize, link: u16, on: bool) {
        if let Some(k) = self.find_link(slot, link) {
            self.set_pressed(k, on);
        }
    }

    fn set_pressed(&mut self, key: (usize, usize), on: bool) {
        self.set_notify(key, 1, on);
    }

    fn control(&self, key: (usize, usize)) -> Option<&s4_formats::gui::Control> {
        self.dialog(key.0)?.controls.get(key.1)
    }

    /// The index of control `link` in dialog `slot`.
    pub fn index_of(&self, slot: usize, link: u16) -> Option<usize> {
        self.dialog(slot)?.controls.iter().position(|c| c.link == link)
    }

    fn find_link(&self, slot: usize, link: u16) -> Option<(usize, usize)> {
        let d = self.dialog(slot)?;
        d.controls.iter().position(|c| c.link == link).map(|i| (slot, i))
    }

    /// `IGuiEngine::SelectControl(dialog, link, true)` for tabs/radios (0x677430: nothing for a
    /// pressed control, a checkbox included).
    pub fn select(&mut self, slot: usize, link: u16, ev: &mut Vec<UiEvent>) {
        if let Some(key) = self.find_link(slot, link)
            && !self.is_pressed(key.0, key.1)
        {
            self.select_radio(key, ev);
        }
    }

    /// `FindControlUnderCursor`: (dialog slot, control index) at panel point (x, y).
    pub fn control_at(&self, x: i32, y: i32) -> Option<(usize, usize)> {
        let mut open: Vec<usize> = self.open.clone();
        open.sort_by_key(|&s| std::cmp::Reverse(self.dialog(s).map_or(0, |d| d.surface)));
        for s in open {
            let d = self.dialog(s)?;
            let (dx, dy) = (d.x as i32, d.y as i32);
            if x < dx || y < dy || x > dx + d.width as i32 || y > dy + d.height as i32 {
                continue;
            }
            for (i, c) in d.controls.iter().enumerate() {
                let fx = self.effects.get(&(s, i)).copied().unwrap_or(c.effects);
                if fx & 0xC != 0 {
                    continue;
                }
                let (cx, cy) = (dx + c.x as i32, dy + c.y as i32);
                if x >= cx && y >= cy && x <= cx + c.width as i32 && y <= cy + c.height as i32 {
                    // 0x6747c0: a slider area gives its knob when the point is on the knob.
                    if let Some(k) = self.knob_of(s, i) {
                        let n = &d.controls[k];
                        let (nx, ny) = (dx + n.x as i32, dy + n.y as i32);
                        if x >= nx && y >= ny && x <= nx + n.width as i32 && y <= ny + n.height as i32 {
                            return Some((s, k));
                        }
                    }
                    return Some((s, i));
                }
            }
            return None;
        }
        None
    }

    fn command(&mut self, key: (usize, usize), ev: &mut Vec<UiEvent>) {
        if let Some(c) = self.control(key) {
            let effects = self.effects.get(&key).copied().unwrap_or(0);
            let e = UiEvent::Command { dialog: key.0, link: c.link, effects };
            self.calls.push(e.clone());
            ev.push(e);
        }
    }

    /// `SelectRadioGroup` (types 2 and 3).
    fn select_radio(&mut self, key: (usize, usize), ev: &mut Vec<UiEvent>) {
        let Some(c) = self.control(key).cloned() else { return };
        if self.is_pressed(key.0, key.1) {
            if c.kind == 3 {
                // 0x674c40: code 3 with the pressed bit already clear in its effects
                // (`(+0x1b & 0xfe) << 16`), then the release with code 1.
                let effects = self.effects.get(&key).copied().unwrap_or(0) & !1;
                let e = UiEvent::Command { dialog: key.0, link: c.link, effects };
                self.calls.push(e.clone());
                ev.push(e);
                self.set_pressed(key, false);
            }
            return;
        }
        let n = self.dialog(key.0).map_or(0, |d| d.controls.len());
        for i in 0..n {
            let Some(o) = self.control((key.0, i)) else { continue };
            if matches!(o.kind, 2 | 3) && o.group == c.group && i != key.1 {
                self.set_pressed((key.0, i), false);
            }
        }
        self.set_pressed(key, true);
        self.command(key, ev);
    }

    /// `CGuiEventHandler::HandleMsgNewDialog` for a context dialog (soldiers 45, specialists 44,
    /// …): `ResetMainMenuForContext` (`ResetRadioGroup(0, 606)`: no main tab pressed; the open
    /// page and submenu closed), then the dialog opened in the page slot.
    pub fn open_context(&mut self, dialog: usize, ev: &mut Vec<UiEvent>) {
        if self.open.contains(&dialog) {
            return;
        }
        for l in 606..=611 {
            if let Some(k) = self.find_link(0, l) {
                self.set_pressed(k, false);
            }
        }
        let pages: Vec<usize> = self.open.iter().copied().filter(|&s| (3..=56).contains(&s) || matches!(s, 75..=77)).collect();
        for s in pages {
            self.close(s, ev);
        }
        self.open(dialog, ev);
    }

    /// `CGuiEventHandler` event 0x25b (603, `ResetMainMenu` 0x481000): the build tab (dialog 0,
    /// 606) is clicked unless it is pressed already; its command opens the build submenu again
    /// and closes the page slot (a building or selection dialog). A building dialog (0x481110)
    /// and a selection dialog unpress the main tabs, so after them it always runs.
    pub fn reset_main_menu(&mut self, ev: &mut Vec<UiEvent>) {
        let mut more = Vec::new();
        if self.find_link(0, 606).is_none_or(|k| self.is_pressed(k.0, k.1)) {
            return;
        }
        self.select(0, 606, &mut more);
        for e in more.clone() {
            self.react(&e, &mut more);
        }
        ev.extend(more);
    }

    /// `IGuiEngine::SelectControl(dialog, link, 1)` (0x677430) from outside the GUI (the
    /// tutorial's `Tutorial.PressButton`): nothing for a pressed control; a tab / radio /
    /// checkbox (kinds 2, 3) is clicked (`UIEventMethod`: the group released, pressed, code 3); a
    /// button (kinds 0, 13) gets code 3 unpressed; a toggle (kind 1) its pressed bit (code 1);
    /// an edit field (5, 20) the focus. The commands' reactions (pages opening) follow. Returns
    /// the procedures' events.
    pub fn select_control(&mut self, slot: usize, link: u16) -> Vec<UiEvent> {
        let mut ev = Vec::new();
        let Some(key) = self.find_link(slot, link) else { return ev };
        if self.is_pressed(key.0, key.1) {
            return ev;
        }
        match self.control(key).map(|c| c.kind) {
            Some(2 | 3) => self.select_radio(key, &mut ev),
            Some(0 | 13) => self.command(key, &mut ev),
            Some(1) => {
                self.set_pressed(key, true);
                let effects = self.effects.get(&key).copied().unwrap_or(0);
                self.calls.push(UiEvent::State { dialog: slot, link, effects });
            }
            Some(5 | 20) => self.focus = Some((slot, link)),
            _ => {}
        }
        for e in ev.clone() {
            self.react(&e, &mut ev);
        }
        ev
    }

    /// The (dialog slot, link) of the control under the cursor.
    pub fn hovered(&self) -> Option<(usize, u16)> {
        self.selected.and_then(|h| self.control(h).map(|c| (h.0, c.link)))
    }

    /// Mouse moved to panel point (x, y).
    pub fn mouse_move(&mut self, x: i32, y: i32) -> Vec<UiEvent> {
        let mut ev = Vec::new();
        let hit = self.control_at(x, y);
        if let Some(d) = self.drag {
            if self.control(d).is_some_and(|c| matches!(c.kind, 7 | 8)) {
                self.knob_drag(d, x, &mut ev);
            } else {
                // A pressed button follows the cursor: released while outside, pressed again inside.
                let inside = hit == Some(d);
                self.set_pressed(d, inside);
            }
        }
        if hit != self.selected {
            if let Some(s) = self.selected.take() {
                self.set_effect(s, 2, false);
                if let Some(c) = self.control(s).filter(|c| highlights(c.kind)) {
                    ev.push(UiEvent::Highlight { dialog: s.0, link: c.link, on: false });
                }
                // Leaving a held repeat button releases it (0x672f90 case 5, `UnknownGUIElement`).
                if self.control(s).is_some_and(|c| matches!(c.kind, 17 | 18)) {
                    self.set_pressed(s, false);
                }
            }
            if let Some(h) = hit {
                self.set_effect(h, 2, true);
                self.selected = Some(h);
                if let Some(c) = self.control(h).filter(|c| highlights(c.kind)) {
                    ev.push(UiEvent::Highlight { dialog: h.0, link: c.link, on: true });
                }
                if let Some(c) = self.control(h) {
                    let ext = self.tooltips.get(&(h.0, c.link)).map_or(c.tooltip_ext, |t| t.1);
                    let tooltip = (ext != 0xffff).then_some(ext);
                    ev.push(UiEvent::Hover { dialog: h.0, link: c.link, tooltip });
                }
            }
        }
        ev
    }

    /// The mouse wheel at panel point (x, y): code 10 for the top dialog there (0x672f90 event
    /// 0xe, `sub_274950`).
    pub fn wheel(&mut self, x: i32, y: i32, delta: i32) -> Option<UiEvent> {
        let mut open: Vec<usize> = self.open.clone();
        open.sort_by_key(|&s| std::cmp::Reverse(self.dialog(s).map_or(0, |d| d.surface)));
        let dialog = open.into_iter().find(|&s| {
            self.dialog(s).is_some_and(|d| x >= d.x as i32 && y >= d.y as i32 && x <= d.x as i32 + d.width as i32 && y <= d.y as i32 + d.height as i32)
        })?;
        Some(UiEvent::Wheel { dialog, delta })
    }

    /// Left button pressed; `over_gui` = the point is on a non-transparent GUI pixel.
    pub fn left_down(&mut self, x: i32, y: i32, over_gui: bool) -> Vec<UiEvent> {
        let mut ev = self.mouse_move(x, y);
        if !over_gui {
            return ev;
        }
        let Some(key) = self.selected else { return ev };
        let Some(c) = self.control(key).cloned() else { return ev };
        match c.kind {
            16 => self.area_down(key, x, &mut ev),
            7 | 8 => self.knob_down(key),
            0 | 13 | 19 => {
                if !self.is_pressed(key.0, key.1) {
                    self.set_pressed(key, true);
                    self.drag = Some(key);
                }
            }
            1 => {
                let p = self.is_pressed(key.0, key.1);
                self.set_pressed(key, !p);
                self.command(key, &mut ev);
            }
            2 | 3 => self.select_radio(key, &mut ev),
            17 | 18 => {
                self.set_pressed(key, true);
                self.command(key, &mut ev);
            }
            // An edit field takes the keyboard (`SelectEditControl`); gaining the focus (bit
            // 0x40, not kept in `effects`) is a code 1 (0x6756a0).
            20 => {
                if self.focus != Some((key.0, c.link)) {
                    let effects = self.effects.get(&key).copied().unwrap_or(0) | 0x40;
                    self.calls.push(UiEvent::State { dialog: key.0, link: c.link, effects });
                }
                self.focus = Some((key.0, c.link));
            }
            _ => {}
        }
        let commands: Vec<UiEvent> = ev.iter().filter(|e| matches!(e, UiEvent::Command { .. })).cloned().collect();
        for e in commands {
            self.react(&e, &mut ev);
        }
        ev
    }

    /// Typed text for the focused edit field (`SelectEditControl` / the edit key handling):
    /// printable characters are appended (up to `max` characters), backspace (`\u{8}`) deletes,
    /// return (`\r`) reports [`UiEvent::EditDone`]. Returns the events.
    pub fn type_text(&mut self, text: &str, max: usize) -> Vec<UiEvent> {
        let Some((d, l)) = self.focus else { return Vec::new() };
        if !self.open.contains(&d) {
            self.focus = None;
            return Vec::new();
        }
        let mut ev = Vec::new();
        let effects = self.index_of(d, l).and_then(|i| self.effects.get(&(d, i))).copied().unwrap_or(0);
        let t = self.texts.entry((d, l)).or_default();
        for c in text.chars() {
            match c {
                // A deleted or inserted character: code 2.
                '\u{8}' => {
                    if t.pop().is_some() {
                        self.calls.push(UiEvent::Edit { dialog: d, link: l });
                    }
                }
                '\r' | '\n' => {
                    // Return clears the focus bit (code 1), then code 3 (0x672f90 case 0xd).
                    self.calls.push(UiEvent::State { dialog: d, link: l, effects });
                    ev.push(UiEvent::EditDone { dialog: d, link: l });
                }
                c if !c.is_control() && t.chars().count() < max => {
                    t.push(c);
                    self.calls.push(UiEvent::Edit { dialog: d, link: l });
                }
                _ => {}
            }
        }
        ev
    }

    /// Left button released.
    pub fn left_up(&mut self, x: i32, y: i32) -> Vec<UiEvent> {
        let mut ev = Vec::new();
        let drag = self.drag.take();
        let hit = self.control_at(x, y);
        if let Some(key) = drag {
            if self.control(key).is_some_and(|c| matches!(c.kind, 7 | 8)) {
                self.knob_up(key, &mut ev);
            } else if self.is_pressed(key.0, key.1) && hit == Some(key) {
                self.set_pressed(key, false);
                self.command(key, &mut ev);
            } else {
                self.set_pressed(key, false);
            }
        }
        if let Some(key) = hit
            && let Some(c) = self.control(key)
            && matches!(c.kind, 17 | 18)
        {
            self.set_pressed(key, false);
        }
        let commands: Vec<UiEvent> = ev.iter().filter(|e| matches!(e, UiEvent::Command { .. })).cloned().collect();
        for e in commands {
            self.react(&e, &mut ev);
        }
        ev
    }

    /// Right button pressed.
    pub fn right_down(&mut self, x: i32, y: i32, over_gui: bool) -> Vec<UiEvent> {
        let mut ev = self.mouse_move(x, y);
        // Event 9: a linked slider knob under the cursor is locked / unlocked (guistate/slider.rs).
        if over_gui
            && let Some(hit) = self.control_at(x, y)
            && self.selected == Some(hit)
            && self.drag != Some(hit)
        {
            self.knob_lock(hit);
        }
        if over_gui
            && let Some(key) = self.selected
            && let Some(c) = self.control(key)
            && matches!(c.kind, 0..=3 | 5..=8 | 10 | 13 | 19 | 20)
        {
            let e = UiEvent::Context { dialog: key.0, link: c.link };
            self.calls.push(e.clone());
            ev.push(e);
        }
        ev
    }

    /// The dialog procedures' GUI reactions to a command (see the module doc).
    fn react(&mut self, e: &UiEvent, ev: &mut Vec<UiEvent>) {
        let UiEvent::Command { dialog, link, .. } = *e else { return };
        match (dialog, link) {
            // ProductionOrderMenuCallback code 3 (0x476810): a tab forces the next fill
            // (0x11aaee0 = -1) and sends 0x1b2 (good) → 0x4ea450: the good, then the fill.
            (26, 0x308..=0x30e | 0xb40) => {
                if let Some(&(_, g)) = DISTRIBUTION_TABS.iter().find(|t| t.0 == link) {
                    self.page_info.distribution.last_eco = -1;
                    self.page_info.distribution.good = g;
                }
            }
            // A released knob: PrintDistributionValues, the values into the info's shares, UI
            // event 0x1bd (s4-game sends the 5028s), then the display again.
            (26, 795..=799) => {
                self.print_distribution();
                let d = &mut self.page_info.distribution;
                for i in 0..(d.count.clamp(0, 5) as usize) {
                    d.list[i].1 = d.values[i];
                }
                d.show = true;
                d.release = true;
            }
            // The message recipients page (0x474560 code 3): a player's radio (0x243..0x24a) sets
            // its +0x20 (event 0x23: `SaveSettingsManager` [inf: the mask bit], `SetSettings`,
            // the page refilled); the buttons 0x24b..0x24d send event 0x24 with 0 / 1 / 2.
            (58, 0x243..=0x24a) if !self.page_info.chat.filling => {
                let i = (link - 0x243) as usize;
                let on = self.index_of(58, link).is_some_and(|k| self.is_pressed(58, k));
                let c = &mut self.page_info.chat;
                c.mask = if on { c.mask | 1 << i } else { c.mask & !(1 << i) };
            }
            (58, 0x24b..=0x24d) => self.page_info.chat.select((link - 0x24b) as u32),
            // TransportPrioTabCallback code 3 (0x4acf20): a good slot selects its good.
            (25, 0x687..=0x6a2 | 0x6a8..=0x6ad) => {
                if let Some(i) = TRANSPORT_LINKS.iter().position(|&l| l == link)
                    && let Some(&g) = self.page_info.transport.link_goods.get(i)
                {
                    self.page_info.transport.target = g;
                }
            }
            // UI event 0x1b8 → 0x4edaa0, which `CInputProcessor` runs in state 0 only.
            (31, 1350) | (27, 323) | (12, 563) if self.input_state == 0 => {
                self.fill_global = !self.fill_global;
                FILL_GLOBAL.store(self.fill_global, std::sync::atomic::Ordering::Relaxed);
            }
            (0, 606..=611) => {
                let sub = (link - 606) as usize + 3;
                if self.open.contains(&sub) {
                    return;
                }
                // MinimpapResetOpenDialogs: close the submenu bar and the page below it.
                let pages: Vec<usize> = self.open.iter().copied().filter(|&s| ((3..=56).contains(&s) || matches!(s, 75..=77)) && s != 1 && s != 2).collect();
                for s in pages {
                    self.close(s, ev);
                }
                self.open(sub, ev);
            }
            (3, 536..=540) => {
                // GuiDlgBuildSubmenuProc → event 0x1C page index → build page dialog.
                let page = match link {
                    536 => 9,
                    537 => 10,
                    538 => 11,
                    539 => 12,
                    _ => 13,
                };
                for s in 9..=13 {
                    if s != page {
                        self.close(s, ev);
                    }
                }
                self.open(page, ev);
            }
            // The other submenus' tabs: `g_pGuiMainEvents +12` holds the one open page; it is
            // closed, the tab's page opened (directly, or through the CInputProcessor action's
            // `CGuiEventHandler` page event).
            (4..=8, _) => {
                let page = match (dialog, link) {
                    (4, 745) => 30, // 0x2E9 → 0x1AF → InitSettlerProduction → GuiDlgSettlerProductionMenuProc
                    (4, 746) => 32, // 0x2EA → 0x1AE → InitFreeCarrier → GuiDlgFreeCarrierMenuProc
                    (4, 747) => 31, // 0x2EB → 0x1AD → InitSettlerAmount → GuiDlgSettlerStatisticMenuProc
                    (4, 748) => 29, // 0x2EC → GuiDlgSettlerFindMenuProc
                    (5, 1067) => 26, // 0x42B → GuiDlgGoodDistributionProc
                    (5, 1068) => 25, // 0x42C → 0x1B1 → InitTransportPrio → GuiDlgTransportpriorityProc
                    (5, 1069) => 27, // 0x42D → 0x1B0 → InitGoodAmount → GuiDlgAvailableGoodsProc
                    (6, 1560) => 28, // 0x618 → GuiDlgStatisticWarriorProc
                    (6, 1561) => 75, // 0x619 → GuiDlgStatisticLandProc
                    (6, 1562) => 76, // 0x61A → GuiDlgStatisticGoodsProc
                    // 0x49c040: UI events 0x1a..0x1d → `EventManagement` → 0x26c with the records +0x110 /
                    // +0x128 / +0x140 / +0x154 (ids 0..3, `SetSettings`) → 0x483010 → 36..39
                    // (settings_pages.rs). [inf] tab order = the pushes' order (0x49c18c..0x49c2b1).
                    (7, 1021) => 36,
                    (7, 1022) => 37,
                    (7, 1023) => 38,
                    (7, 1024) => 39,
                    // [inf] 928 → event 0x28 → the load/save page.
                    (8, 928) => 33,
                    // 0x47b4e0: in game type 3 0x3a1 → UI 0x192, 0x1b4 (goods_race.rs) instead.
                    (8, 929) if self.goods_race => return,
                    (8, 929) => 35, // GuiDlgBriefingProc
                    (8, 930) => 34, // GuiDlgQuitGameProc
                    // 0x47b4e0: 0x3a3 → UI 0x1e → EventManagement 0x1e → 0x26c → 0x483010 case 4.
                    (8, 931) => 58,
                    _ => return,
                };
                for s in PAGES {
                    if s != page {
                        self.close(s, ev);
                    }
                }
                self.open(page, ev);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    /// `EventManagement` 0x24 on the message recipients page: all / allies / enemies among the
    /// other human players (computers never).
    #[test]
    fn chat_recipients_by_mode() {
        let slot = |kind, team| ChatSlot { name: String::new(), colour: 0, kind, team };
        let mut c = ChatInfo { slots: vec![slot(2, 1), slot(2, 1), slot(2, 2), slot(3, 2)], local: 0, ..Default::default() };
        c.select(0);
        assert_eq!(c.mask, 0b0110);
        c.select(1);
        assert_eq!(c.mask, 0b0010);
        c.select(2);
        assert_eq!(c.mask, 0b0100);
        assert!(c.receives(2) && !c.receives(3) && !c.receives(1));
    }

    use super::*;

    fn roman() -> Option<GuiState> {
        let set = GuiSet::open(s4_formats::game_dir().join("Menu/GUISetRomans.dat")).ok()?;
        let mut s = GuiState::new(set);
        let mut ev = Vec::new();
        s.open(0, &mut ev);
        s.select(0, 606, &mut ev);
        s.open(3, &mut ev);
        Some(s)
    }

    /// Centre of a control in panel px.
    fn centre(s: &GuiState, slot: usize, link: u16) -> (i32, i32) {
        let d = s.set.dialog(slot).unwrap();
        let c = d.controls.iter().find(|c| c.link == link).unwrap();
        ((d.x + c.x + c.width / 2) as i32, (d.y + c.y + c.height / 2) as i32)
    }

    #[test]
    fn tabs_switch_dialogs_and_stay_exclusive() {
        let Some(mut s) = roman() else { return };
        assert_eq!(s.open, vec![0, 3, 9]);
        let (x, y) = centre(&s, 0, 607);
        let ev = s.left_down(x, y, true);
        assert!(ev.contains(&UiEvent::DialogOpened(4)) && ev.contains(&UiEvent::DialogClosed(9)));
        // Its init selects tab 747, which opens the settler statistic page.
        assert_eq!(s.open, vec![0, 4, 31]);
        // Exactly one of the main tabs is pressed.
        let pressed: Vec<u16> = (606..=611).filter(|&l| s.find_link(0, l).is_some_and(|k| s.is_pressed(k.0, k.1))).collect();
        assert_eq!(pressed, vec![607]);
        // Clicking the pressed tab again does nothing (SelectRadioGroup on a pressed type 2).
        let ev = s.left_down(x, y, true);
        assert!(!ev.iter().any(|e| matches!(e, UiEvent::Command { .. })));
        // Back to build: submenu init selects 536 and opens page 9.
        let (x, y) = centre(&s, 0, 606);
        s.left_down(x, y, true);
        assert_eq!(s.open, vec![0, 3, 9]);
        let (x, y) = centre(&s, 3, 540);
        s.left_down(x, y, true);
        assert_eq!(s.open, vec![0, 3, 13]);
    }

    /// The procedures' code 1 / code 3 calls in the original's order: a tab releases the pressed
    /// one, presses itself, then fires; the submenu it opens selects its first tab the same way.
    #[test]
    fn calls_are_code_1_then_code_3() {
        let Some(mut s) = roman() else { return };
        s.calls.clear();
        let (x, y) = centre(&s, 0, 607);
        s.left_down(x, y, true);
        let calls: Vec<(u8, usize, u16, bool)> = s
            .calls
            .iter()
            .map(|c| match *c {
                UiEvent::State { dialog, link, effects } => (1, dialog, link, effects & 1 != 0),
                UiEvent::Command { dialog, link, effects } => (3, dialog, link, effects & 1 != 0),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(&calls[..3], &[(1, 0, 606, false), (1, 0, 607, true), (3, 0, 607, true)]);
        assert!(calls[3..].contains(&(1, 4, 747, true)) && calls[3..].contains(&(3, 4, 747, true)));
        // The press on a tab already pressed changes nothing: no call.
        s.calls.clear();
        s.left_down(x, y, true);
        s.left_up(x, y);
        assert!(s.calls.is_empty());
    }
}
