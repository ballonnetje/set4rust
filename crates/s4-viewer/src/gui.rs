//! The in-game GUI panel (static): the dialogs of `Menu\GUISet<Race>.dat` composed the way
//! `IGuiEngine` arranges them, with the original fonts, drawn as a GPU overlay.
//!
//! # What is open
//! `CStateGame` calls `IGuiEngine::Init(…, dialog 0, GuiDlgMinimapProc, language)`: the minimap
//! frame with the six main tabs (slot 0, 281×210 at 0,0). Below it the build tab's submenu bar
//! (slot 3, `GuiDlgBuildSubmenuProc`, at 0,210) and the basic buildings page (slot 9,
//! `GuiDlgBuildBasicProc`, at 0,250). `CGuiEventHandler` adds the extension bars when the screen is
//! tall enough: slot 1 (goods overview, at 0,600) and slot 2 (at 0,768, plus a camera window).
//! The build tab is open at game start: the game loop's first frame (0x513c80, its `+0x101` flag
//! still clear) sends GUI event 0x27; `CGuiEventHandler` (0x4805a0) opens page 9
//! (`BaseBuildingTabCallback`) and then 0x481000 "clicks" dialog 0's control 0x25e (tab 606, while
//! it is not pressed), whose code-3 command (0x49b6b0) opens the build submenu, slot 3. The
//! extension thresholds (`sub_139F260(n, OutputHeight)`, not
//! decompiled) are taken from the dialog geometry: slot 1 when the screen is ≥ 768 px tall, slot 2
//! when ≥ 1024.
//!
//! # Selected controls at start
//! `GuiDlgBuildSubmenuProc(0)`: `SelectControl(3, 536)`; `UpdateGuiDlgBuildBasic`: `SelectControl(9,
//! 394)` while the "show all" flag is clear; main tab 606 (build), the tab event 0x27 clicks.
//! A selected (`effects & 1`) control shows its `pressed_image`.
//!
//! # Texts
//! `IGuiEngine::SetText` fills a control's text; the values here are the ones the original sets
//! from the game state at the start:
//! * slot 9: `SetBuildBasicProperties(count, building, link, link_plus)` per building of the basic
//!   page (`UpdateGuiDlgBuildBasic`: woodcutter 380/389, forester 376/388, sawmill 374/387,
//!   stonecutter 378/390, waterworks 402/403, residences 386/393, 384/391, 385/392 — the order set in
//!   `GuiDlgBuildSubmenuProc` for tab 536) as `"%u"` and `"+%u"` (hidden when 0). The count is the
//!   local player's finished buildings of that type **[inf: finished vs. ordered split]**, the plus
//!   value those under construction (0 at map start). `SetBasicBuildCosts(-1, 1)`: 398/399/400 = "0".
//! * Every other text (goods counts in slot 1, minimap statistics in slot 0) is set by update
//!   handlers that need the economy; they stay empty here.
//!
//! # Fonts (`DrawControlText`, 0x2f9cb10)
//! Text is GDI `DrawTextA` with one of 19 fonts created in `IGuiEngine::Init` (0x2f9fff0) from a
//! table of 84-byte records (release VA 0x1029178, copied in [`FONTS`]): `CreateFontA(height, 0, …,
//! weight, …, quality = record[0x20] ? ANTIALIASED : DEFAULT, "Arial")`. Each record also holds the
//! COLORREF text colour, a drop shadow (black, drawn first at +dx,+dy when record[0x21]) and the
//! offset applied while the control is pressed. The control's style byte (+32) indexes the
//! `DrawTextA` format table (release VA 0x1029138): left / centre / right, `DT_SINGLELINE|DT_VCENTER`
//! variants, and word-wrapping ones. The text rectangle is the control's, shrunk by 2 px left and
//! right. We rasterise the same face (the user's `Arial`/`Arial Bold` from the Windows font
//! directory) at the GDI cell height: em size = height · unitsPerEm / (winAscent + winDescent).

use std::collections::HashMap;
use std::path::PathBuf;

use s4_formats::gamedata::Race;
use s4_formats::gfx::GfxFile;
use s4_formats::gui::{Dialog, GuiSet};


use crate::population::Population;

/// One `IGuiEngine` font: (height, weight, COLORREF, shadow dx, dy, pressed dx, dy, shadow).
struct FontStyle {
    height: i32,
    weight: i32,
    color: u32,
    shadow: (i32, i32),
    pressed: (i32, i32),
    has_shadow: bool,
}

const fn f(height: i32, weight: i32, color: u32, sx: i32, sy: i32, has_shadow: bool) -> FontStyle {
    FontStyle { height, weight, color, shadow: (sx, sy), pressed: (1, 1), has_shadow }
}

/// The 19 font records of the shipped exe (release VA 0x1029178, 84 bytes each; every face is
/// "Arial", width 0, antialiased).
const FONTS: [FontStyle; 19] = [
    f(23, 700, 0xdbc175, 2, 2, true),
    f(20, 700, 0xfefefe, 2, 2, true),
    f(20, 700, 0x0000fe, 2, 2, true),
    f(18, 700, 0xfefefe, 2, 2, true),
    f(18, 500, 0xfefefe, 2, 2, true),
    f(16, 300, 0xfefe00, 2, 2, true),
    f(15, 800, 0xfefefe, 2, 2, false),
    f(15, 700, 0xfefefe, 2, 2, true),
    f(15, 400, 0x000000, 0, 0, false),
    f(15, 100, 0xfefefe, 2, 2, true),
    f(15, 100, 0x0000fe, 2, 2, true),
    f(14, 700, 0xfefefe, 2, 2, true),
    f(12, 700, 0x00fefe, 2, 2, true),
    f(14, 700, 0x0000fe, 2, 2, true),
    f(14, 700, 0xdbc175, 2, 2, true),
    f(25, 700, 0x404080, 2, 2, false),
    f(20, 700, 0x404080, 2, 2, false),
    f(29, 700, 0xdbc175, 2, 2, true),
    f(18, 700, 0x0000fe, 2, 2, true),
];

/// `DrawTextA` formats by the control's style byte (release VA 0x1029138, left-to-right column).
const FORMATS: [u32; 8] = [0x0, 0x1, 0x2, 0x24, 0x25, 0x26, 0x10, 0x11];
const DT_CENTER: u32 = 0x1;
const DT_RIGHT: u32 = 0x2;
const DT_VCENTER: u32 = 0x4;
const DT_WORDBREAK: u32 = 0x10;

/// Arial / Arial Bold OS/2 metrics: unitsPerEm 2048, usWinAscent 1854, usWinDescent 434.
const UNITS_PER_EM: f32 = 2048.0;
const WIN_ASCENT: f32 = 1854.0;
const WIN_DESCENT: f32 = 434.0;

/// GDI-like text drawing with the user's Arial.
pub struct TextEngine {
    regular: fontdue::Font,
    bold: fontdue::Font,
}

/// Arial (regular, bold) where Windows has none: Microsoft's core fonts, else Liberation Sans (same
/// metrics), else what fontconfig gives for Arial.
#[cfg(not(windows))]
fn unix_arial() -> Option<(PathBuf, PathBuf)> {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let dirs = [
        PathBuf::from("/usr/share/fonts/truetype/msttcorefonts"),
        PathBuf::from("/usr/share/fonts/msttcore"),
        PathBuf::from("/usr/share/fonts/TTF"),
        home.join(".local/share/fonts"),
        home.join(".fonts"),
        PathBuf::from("/Library/Fonts"),
        PathBuf::from("/System/Library/Fonts/Supplemental"),
        PathBuf::from("/usr/share/fonts/truetype/liberation2"),
        PathBuf::from("/usr/share/fonts/truetype/liberation"),
        PathBuf::from("/usr/share/fonts/liberation-sans"),
        PathBuf::from("/usr/share/fonts/liberation-sans-fonts"),
        PathBuf::from("/usr/share/fonts/liberation"),
    ];
    let pairs = [("arial.ttf", "arialbd.ttf"), ("Arial.ttf", "Arial_Bold.ttf"), ("Arial.ttf", "Arial Bold.ttf"), ("LiberationSans-Regular.ttf", "LiberationSans-Bold.ttf")];
    for d in &dirs {
        for (r, b) in pairs {
            let (r, b) = (d.join(r), d.join(b));
            if r.is_file() && b.is_file() {
                return Some((r, b));
            }
        }
    }
    let fc = |pattern: &str| {
        let o = std::process::Command::new("fc-match").args(["-f", "%{file}", pattern]).output().ok()?;
        let p = PathBuf::from(String::from_utf8_lossy(&o.stdout).trim());
        p.is_file().then_some(p)
    };
    Some((fc("Arial:style=Regular")?, fc("Arial:style=Bold")?))
}

impl TextEngine {
    /// The engine every GUI shares: the fonts parsed once (~80 ms), the first time (or by a warm
    /// at the app's start), not at every screen change.
    pub fn shared() -> anyhow::Result<std::sync::Arc<TextEngine>> {
        static SHARED: std::sync::OnceLock<Result<std::sync::Arc<TextEngine>, String>> = std::sync::OnceLock::new();
        SHARED.get_or_init(|| TextEngine::load().map(std::sync::Arc::new).map_err(|e| e.to_string())).clone().map_err(|e| anyhow::anyhow!(e))
    }

    pub fn load() -> anyhow::Result<Self> {
        let parse = |path: &std::path::Path| -> anyhow::Result<fontdue::Font> {
            let data = std::fs::read(path).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
            fontdue::Font::from_bytes(data, fontdue::FontSettings::default()).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))
        };
        #[cfg(windows)]
        let (regular, bold) = {
            let dir = std::env::var_os("WINDIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows")).join("Fonts");
            (dir.join("arial.ttf"), dir.join("arialbd.ttf"))
        };
        #[cfg(not(windows))]
        let (regular, bold) = unix_arial().ok_or_else(|| anyhow::anyhow!("no Arial or Liberation Sans font found (install fonts-liberation or ttf-mscorefonts-installer)"))?;
        Ok(TextEngine { regular: parse(&regular)?, bold: parse(&bold)? })
    }

    /// `GetTextExtentPoint32` of `text` in font `font` (px).
    fn width(&self, text: &str, font: usize) -> i32 {
        let fs = &FONTS[font.min(FONTS.len() - 1)];
        let face = if fs.weight >= 600 { &self.bold } else { &self.regular };
        let em = fs.height as f32 * UNITS_PER_EM / (WIN_ASCENT + WIN_DESCENT);
        text.chars().map(|c| face.metrics(c, em).advance_width.round() as i32).sum()
    }

    /// The lines `text` takes in a control `width` px wide (word-wrapping only with
    /// `DT_WORDBREAK`), and the line height.
    fn layout(&self, text: &str, width: i32, font: usize, style: usize) -> (Vec<String>, i32) {
        let fs = &FONTS[font.min(FONTS.len() - 1)];
        let fmt = FORMATS[style.min(7)];
        let face = if fs.weight >= 600 { &self.bold } else { &self.regular };
        let em = fs.height as f32 * UNITS_PER_EM / (WIN_ASCENT + WIN_DESCENT);
        let advance = |c: char| face.metrics(c, em).advance_width.round() as i32;
        let mut lines: Vec<String> = Vec::new();
        if fmt & DT_WORDBREAK != 0 {
            let mut cur = String::new();
            for word in text.split(' ') {
                let cand = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
                if !cur.is_empty() && cand.chars().map(advance).sum::<i32>() > width - 4 {
                    lines.push(std::mem::take(&mut cur));
                    cur = word.to_string();
                } else {
                    cur = cand;
                }
            }
            lines.push(cur);
        } else {
            lines.push(text.to_string());
        }
        (lines, fs.height)
    }

    /// `DrawControlText`: draw `text` into `img` (RGBA, `w` wide) inside `rect` (x, y, w, h).
    /// `font`, `style`: the control's font and style bytes. `scroll` = first line shown, for a
    /// scrolled multi-line text (`CScrollMultiline`): lines are then clipped to the rectangle.
    fn draw(&self, img: &mut [u8], w: usize, rect: (i32, i32, i32, i32), text: &str, (font, style, pressed): (usize, usize, bool), scroll: Option<usize>) {
        let fs = &FONTS[font.min(FONTS.len() - 1)];
        let fmt = FORMATS[style.min(7)];
        let (mut x0, mut y0, mut x1, mut y1) = (rect.0 + 2, rect.1, rect.0 + rect.2 - 2, rect.1 + rect.3);
        if pressed {
            x0 += fs.pressed.0;
            x1 += fs.pressed.0;
            y0 += fs.pressed.1;
            y1 += fs.pressed.1;
        }
        // GDI maps weights >= 600 to the bold face of Arial.
        let face = if fs.weight >= 600 { &self.bold } else { &self.regular };
        let em = fs.height as f32 * UNITS_PER_EM / (WIN_ASCENT + WIN_DESCENT);
        let ascent = (WIN_ASCENT * em / UNITS_PER_EM).round() as i32;
        let advance = |c: char| face.metrics(c, em).advance_width.round() as i32;
        let (mut lines, line_h) = self.layout(text, rect.2, font, style);
        if let Some(first) = scroll {
            lines.drain(..first.min(lines.len()));
            let fit = ((y1 - y0) / line_h.max(1)).max(0) as usize;
            lines.truncate(fit);
        }
        let block_h = line_h * lines.len() as i32;
        let top = if fmt & DT_VCENTER != 0 && fmt & 0x20 != 0 { y0 + (y1 - y0 - block_h) / 2 } else { y0 };
        let colorref = |c: u32| [(c & 0xff) as u8, (c >> 8 & 0xff) as u8, (c >> 16 & 0xff) as u8];
        let passes: Vec<((i32, i32), [u8; 3])> = if fs.has_shadow {
            vec![(fs.shadow, [0, 0, 0]), ((0, 0), colorref(fs.color))]
        } else {
            vec![((0, 0), colorref(fs.color))]
        };
        for (li, line) in lines.iter().enumerate() {
            let lw: i32 = line.chars().map(advance).sum();
            let lx = if fmt & DT_RIGHT != 0 {
                x1 - lw
            } else if fmt & DT_CENTER != 0 {
                x0 + (x1 - x0 - lw) / 2
            } else {
                x0
            };
            let baseline = top + li as i32 * line_h + ascent;
            for &((dx, dy), rgb) in &passes {
                let mut pen = lx + dx;
                for c in line.chars() {
                    let (m, cov) = face.rasterize(c, em);
                    let gx = pen + m.xmin;
                    let gy = baseline + dy - m.height as i32 - m.ymin;
                    for yy in 0..m.height {
                        for xx in 0..m.width {
                            let a = cov[yy * m.width + xx] as u32;
                            let (px, py) = (gx + xx as i32, gy + yy as i32);
                            // DrawTextA clips to the rectangle.
                            if a == 0 || px < x0 + dx.min(0) || px >= x1 + dx.max(0) || py < 0 || px < 0 || px as usize >= w {
                                continue;
                            }
                            let o = (py as usize * w + px as usize) * 4;
                            if o + 4 > img.len() {
                                continue;
                            }
                            for k in 0..3 {
                                img[o + k] = ((rgb[k] as u32 * a + img[o + k] as u32 * (255 - a)) / 255) as u8;
                            }
                            img[o + 3] = img[o + 3].max(a as u8);
                        }
                    }
                    pen += m.advance_width.round() as i32;
                }
            }
        }
    }
}

/// A composed panel: RGBA at the original's 1:1 pixel scale.
pub struct PanelImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

fn guiset_name(race: Race) -> &'static str {
    // CStateGame: 1 Viking, 2 Maya, 4 Trojan, else Roman.
    match race {
        Race::VIKING => "GUISetVikings.dat",
        Race::MAYA => "GUISetMayas.dat",
        Race::TROJAN => "GUISetTrojans.dat",
        _ => "GUISetRomans.dat",
    }
}

/// The in-game GUI: its state (open dialogs, control states, texts) and what draws it.
pub struct Gui {
    pub state: crate::guistate::GuiState,
    gfx: GfxFile,
    text: std::sync::Arc<TextEngine>,
    screen_h: u32,
    /// The last composed image (for hit-testing the non-transparent GUI area).
    last: Option<PanelImage>,
    /// The local player's race and id (the pages show their state).
    race: u8,
    pub local: u8,
    /// Building costs per (race, type): (boards, stone, gold).
    costs: HashMap<(u8, u8), (u8, u8, u8)>,
    /// `g_pStringEngine`, for the dialogs' titles.
    strings: Option<s4_formats::text::TextTable>,
    /// The building dialogs' info objects (building_dialog.rs).
    pub bdlg: crate::building_dialog::Infos,
    /// The building whose dialog is open and its info code.
    bdlg_shown: Option<(u16, usize)>,
    /// The open sidebar: (building, info code, the GUI event that asked for it) and the good its
    /// procedure has selected (D46 0x15aad24, D47 0x15aa4f8).
    sidebar: Option<(u16, usize, u16)>,
    side_selected: u8,
    /// D53's and D54's procedure globals (trade_sidebar.rs).
    trade: crate::trade_sidebar::TradeSend,
    foreign: crate::trade_sidebar::TradeForeign,
    /// D51's procedure statics (workshop_sidebar.rs).
    workshop: crate::workshop_sidebar::WorkshopSide,
    /// The selection dialogs' sidebar: the unit groups (D55, group_sidebar.rs) or the spells
    /// (D56), on the sidebar surface (CGuiEventHandler +0x10).
    pub sel_side: Option<usize>,
    /// 0x12938c7: the build pages' Ctrl + right click on a building with a sidebar asks the next
    /// update of D19 / D22 / D24 / D40 to press its sidebar toggle (s4_viewer::build_goto).
    pub deferred_press: bool,
    /// The GUI events of that press (`SelectControl`-like), for the game to run as if clicked.
    pub deferred_events: Vec<crate::guistate::UiEvent>,
    /// The eye catchers' sidebar rows (dialog 52, eyecatcher_sidebar.rs) while it is open.
    eyecatchers: [crate::eyecatcher_sidebar::Row; 12],
    /// D54's `PlayerCount` (the open sidebar's building facts).
    players: u8,
    /// The message window's lines and the dialog it is open in (msg_window.rs).
    pub msg: crate::msg_window::Lines,
    pub msg_dialog: Option<usize>,
    /// A tutorial game (`CGuiEventHandler +0x40`): the window slot (+0x28) holds the tutorial
    /// window instead of the message window.
    pub tutorial: bool,
    /// The open tutorial window (`ChatEvents2`, dialogs 66..=71).
    pub tut_dialog: Option<usize>,
    /// UI events of the tutorial window for `CTutorial::OnEvent` (0x275 / 0x276), oldest first.
    pub tut_events: Vec<u32>,
    /// What the client last took from the tutorial (s4-game msgwin.rs): the text sequence and a
    /// hash of the control bitsets; `None` before the first frame.
    pub tut_seen: Option<(u32, u64)>,
    /// The extended tooltip window (dialog 73).
    pub ext_tip: ExtTooltip,
    /// The short tooltip at the cursor.
    pub tip: ShortTooltip,
    /// The settings pages 36..39 (settings_pages.rs).
    pub settings: crate::settings_pages::Pages,
    /// The statistics graph pages 28, 75, 76 (stat_graphs.rs).
    pub graphs: crate::stat_graphs::Graphs,
    /// `GoodDistributionInfoHolder` (0x14540d0) holding the goods race fill 0x4f8da0: the tick
    /// it last ran (goods_race.rs).
    pub race_holder: Option<u32>,
    /// The camera window of dialog 2 (mini_view.rs).
    pub mini_view: crate::mini_view::MiniView,
    /// The clock, dialog 74 (clock.rs).
    pub clock: crate::clock::Clock,
    /// 0x11b589c (`Game.DisableLastNPlayersInStatistic` 0x5c81f0; 0 at the game start, 0x4f266b):
    /// the last players the statistics leave out.
    pub hidden_last: i32,
    /// The goods race info object 0x11ab0b0 as its fill 0x4f8da0 last left it (`None`: never
    /// filled, all 0); the goods race's end screen reads it.
    pub race_rows: Option<[crate::goods_race::Row; 7]>,
}

/// The GUI engine's short tooltip (object 0x11f7060: surface 0, text buffer `a3` that
/// `SetTooltip` 0x676a70 fills, +0x134 rect, +0x150 shown 0x11f71b0, +0x151 held back 0x11f71b1;
/// the shown time 0x1608da8, the last left press 0x1608dac, the leave time 0x11ff59c).
/// * The GUI timer (0x672f90, 0x673797..0x67386a), with the cursor on a control: shown, it hides
///   8000 ms after it appeared and is held back; hidden and not held back, when more than 700 ms
///   passed since the last left press or less than 400 ms since a shown tooltip was left, the
///   control's procedure gets code 8 with its tooltip id (+0x14) and calls
///   `SetTooltip(GetString(id))` ("" for -1), then `sub_278DA0` (0x678da0) shows a non-empty text
///   at the cursor + (0, 18): the text's extent in font 8 (`GetTextExtentPoint32A`) + 4 square,
///   kept on the screen, filled (255, 255, 180) (`sub_25F0E0`), a black frame (`sub_2784A0`), the
///   text at (2, 2) as a kind-14 control (no inset) with font 8, format 4.
/// * The cursor leaves its control (to another or none): hidden, no longer held back, the leave
///   time noted when it was shown.
/// * A left press (0x673893): the press time noted; a shown tooltip hides and is held back.
#[derive(Clone, Debug, Default)]
pub struct ShortTooltip {
    pub shown: bool,
    pub text: String,
    /// Panel px: top left and size.
    pub rect: (i32, i32, i32, i32),
    held: bool,
    shown_at: u64,
    pressed_at: u64,
    left_at: u64,
    hovered: Option<(usize, u16)>,
}

/// The GUI engine's extended tooltip window (object 0x15a9b50: surface 7, the text buffer
/// 0x15a9b58 that `SetTooltipExt` 0x676b40 fills, +4 the source dialog 0x15a9b54, +0x150 shown
/// 0x15a9ca0, +0x152 enabled 0x15a9ca2; the hide time 0x11ff598).
/// * Enabled by `sub_276BB0` (0x676bb0) from the extended tooltips setting (`GameManager
///   +0x144`); switched off it hides at once.
/// * The cursor enters a control (0x672f90 event 5): its procedure gets code 9 with the
///   control's extended tooltip id and calls `SetTooltipExt(GetString(id))` ("" for -1). Hidden,
///   the window shows (`sub_279210` 0x679210) when enabled and the text is not empty; shown, an
///   empty text blanks it and hides it 700 ms later (the timer, 0x67371e), a text replaces the
///   old one (`sub_2794F0` 0x6794f0) and cancels the timer.
/// * The cursor leaves every control: shown, the window is blanked and hides 700 ms later.
/// * `CloseDialog` (0x67654e) hides it when the closed dialog is its source.
/// * Placed by `ToggleUISurfaceVisibility` (0x488650, with `GameManager +0x144`): dialog 73
///   (296 x 98, flags 7, no controls) at the bottom right, (W - 1 - 296, H - 1 - 98).
///   `sub_279210` draws it there: black on every other pixel (`sub_2788D0`), then the text at
///   (4, 2), w - 4 x h, with font 6 and style 6 as a kind-15 control (`DrawText` 0x672c80 reads
///   the buffer for kind 15).
#[derive(Clone, Debug, Default)]
pub struct ExtTooltip {
    pub enabled: bool,
    pub shown: bool,
    pub text: String,
    source: Option<usize>,
    hide_at: Option<u64>,
    hovering: bool,
}

/// The extended tooltip window's dialog.
pub const EXT_TIP_DIALOG: usize = 73;

impl Gui {
    /// The GUI as a game starts, for the local player's race, on a screen `screen_h` (panel px)
    /// tall (see the module doc).
    pub fn new(race: Race, pop: Option<&Population>, local_player: u8, screen_h: u32) -> anyhow::Result<Self> {
        let game = s4_formats::game_dir();
        let name = guiset_name(race);
        let set = GuiSet::open(game.join("Menu").join(name))?;
        let gfx = GfxFile::open(game.join("Gfx"), s4_formats::gui::gfx_file_for(name).unwrap_or(9))?;
        let mut state = crate::guistate::GuiState::new(set);
        let mut ev = Vec::new();
        state.open(0, &mut ev);
        if screen_h >= 768 {
            state.open(1, &mut ev);
        }
        if screen_h >= 1024 {
            state.open(2, &mut ev);
        }
        // Event 0x27 on the first frame (0x4805a0): page 9 and a click on tab 606, which opens
        // the submenu (whose init selects tab 536 and so page 9).
        state.select(0, 606, &mut ev);
        state.open(3, &mut ev);
        let mut gui = Gui {
            state,
            gfx,
            text: TextEngine::shared()?,
            screen_h,
            last: None,
            race: race as u8,
            local: local_player,
            costs: crate::gui_pages::costs(),
            strings: None,
            bdlg: Default::default(),
            bdlg_shown: None,
            sidebar: None,
            side_selected: 0,
            trade: Default::default(),
            foreign: Default::default(),
            workshop: Default::default(),
            sel_side: None,
            deferred_press: false,
            deferred_events: Vec::new(),
            eyecatchers: Default::default(),
            players: 0,
            msg: Default::default(),
            msg_dialog: None,
            tutorial: false,
            tut_dialog: None,
            tut_events: Vec::new(),
            tut_seen: None,
            ext_tip: Default::default(),
            tip: Default::default(),
            settings: Default::default(),
            graphs: Default::default(),
            race_holder: None,
            mini_view: Default::default(),
            clock: Default::default(),
            hidden_last: 0,
            race_rows: None,
        };
        // Event 0x27 also opens the message button (dialog 59) when it is not open.
        gui.msg_button();
        gui.set_language(0);
        // Dialog 2's procedure code 0 (0x47c870): the camera window at the local player's start.
        if gui.state.open.contains(&crate::mini_view::DIALOG) {
            let start = pop.and_then(|p| p.starts.get(local_player as usize)).map_or((0, 0), |&(x, y)| (x as i32, y as i32));
            gui.mini_view.init(start);
        }
        if let Some(p) = pop {
            gui.update_pages(p, crate::gui_pages::Centre::default());
        }
        gui.hover_costs();
        Ok(gui)
    }

    /// Any GUI set of `Menu\` (e.g. `GUISetStartscreens.dat`) with the dialogs `open` opened, for a
    /// screen `screen_h` panel px tall.
    pub fn from_set(file_name: &str, screen_h: u32, open: &[usize]) -> anyhow::Result<Self> {
        let game = s4_formats::game_dir();
        let set = GuiSet::open(game.join("Menu").join(file_name))?;
        let gfx = GfxFile::open(game.join("Gfx"), s4_formats::gui::gfx_file_for(file_name).unwrap_or(0))?;
        let mut state = crate::guistate::GuiState::new(set);
        let mut ev = Vec::new();
        for &d in open {
            state.open(d, &mut ev);
        }
        Ok(Gui {
            state,
            gfx,
            text: TextEngine::shared()?,
            screen_h,
            last: None,
            race: 0,
            local: 0,
            costs: HashMap::new(),
            strings: None,
            bdlg: Default::default(),
            bdlg_shown: None,
            sidebar: None,
            side_selected: 0,
            trade: Default::default(),
            foreign: Default::default(),
            workshop: Default::default(),
            sel_side: None,
            deferred_press: false,
            deferred_events: Vec::new(),
            eyecatchers: Default::default(),
            players: 0,
            msg: Default::default(),
            msg_dialog: None,
            tutorial: false,
            tut_dialog: None,
            tut_events: Vec::new(),
            tut_seen: None,
            ext_tip: Default::default(),
            tip: Default::default(),
            settings: Default::default(),
            graphs: Default::default(),
            race_holder: None,
            mini_view: Default::default(),
            clock: Default::default(),
            hidden_last: 0,
            race_rows: None,
        })
    }

    /// `CScrollMultiline::ScrollUp` / `ScrollDown` on the text of control `link` of `dialog`:
    /// move its first shown line by `delta`, keeping the last page full. Returns whether it moved.
    pub fn scroll_text(&mut self, dialog: usize, link: u16, delta: i32) -> bool {
        let Some(c) = self.state.set.dialog(dialog).and_then(|d| d.controls.iter().find(|c| c.link == link)).cloned() else { return false };
        let Some(t) = self.state.texts.get(&(dialog, link)) else { return false };
        let font = self.state.fonts.get(&(dialog, link)).copied().unwrap_or(c.font);
        let (lines, line_h) = self.text.layout(t, c.width as i32, font as usize, c.text_style as usize);
        let fit = (c.height as i32 / line_h.max(1)).max(1) as usize;
        let max = lines.len().saturating_sub(fit);
        let cur = self.state.scroll.get(&(dialog, link)).copied().unwrap_or(0);
        let new = (cur as i32 + delta).clamp(0, max as i32) as usize;
        self.state.scroll.insert((dialog, link), new);
        new != cur
    }

    /// Load the texts of language `lang` (`Txt\s4_texts.dat<lang>`).
    pub fn set_language(&mut self, lang: u32) {
        self.strings = s4_formats::text::TextTable::open(s4_formats::game_dir(), lang).inspect_err(|e| tracing::warn!("texts: {e}")).ok();
    }

    /// A text of the game's string table (language set with [`Gui::set_language`]).
    pub fn string(&self, id: usize) -> String {
        self.strings.as_ref().and_then(|t| t.get(id)).unwrap_or("").to_string()
    }

    /// The win/loss notice: GUI event 0x261 opens dialog 72 (release 0x4807ca) and its update
    /// (0x46cbd0) sets link 1795 = string 1964 ("You have won!") or 1934 ("You have lost!").
    /// 0x4807f8 then moves the dialog (its record's x / y) to `x = (W − 210) / 2 − (w + 1) / 2 +
    /// 210`, `y = 10`, centred over the map view right of the 210 px panel: `W` the output width
    /// (`screen_w`, panel px here), `w` the dialog width (`(int)(width · scale + 0.5)`, scale 1),
    /// C integer division.
    pub fn show_win_loss(&mut self, won: bool, screen_w: u32) {
        let mut ev = Vec::new();
        if let Some(d) = self.state.set.dialogs.get_mut(72).and_then(|d| d.as_mut()) {
            let w = d.width as i32;
            let x = (screen_w as i32 - 210) / 2 - (w + 1) / 2 + 210;
            d.x = x.max(0) as u16;
            d.y = 10;
        }
        self.state.open(72, &mut ev);
        let t = self.string(if won { 1964 } else { 1934 });
        self.state.texts.insert((72, 1795), t);
    }

    /// The tutorial marker (`CGuiEventHandler` 0x4805a0, UI event 0x26e from `Tutorial.SetMarker`
    /// / `ClearMarker`): `Some((x, y))` opens dialog 57 (0x39, the 30 x 30 arrow; procedure
    /// 0x4ad910 only shows tooltips) and moves it to panel px (x, y) (the dialog record's x / y, then 0x664540 to the GUI origin + (x, y); the origin
    /// is (0, 0) from `ResetUIEngine` 0x675c50); `None` closes it. Idempotent: called every frame;
    /// returns whether the panel changed.
    /// [inf] Not modelled: the handler's flag +0x40 it requires and its dialog slot +0x38 (one
    /// such dialog at a time).
    pub fn tutorial_marker(&mut self, at: Option<(u16, u16)>) -> bool {
        const MARKER: usize = 0x39;
        let mut ev = Vec::new();
        let open = self.state.open.contains(&MARKER);
        match at {
            Some((x, y)) => {
                let mut moved = false;
                if let Some(d) = self.state.set.dialogs.get_mut(MARKER).and_then(|d| d.as_mut()) {
                    moved = (d.x, d.y) != (x, y);
                    d.x = x;
                    d.y = y;
                }
                if !open {
                    self.state.open(MARKER, &mut ev);
                }
                moved || self.state.open.contains(&MARKER) != open
            }
            None if open => {
                self.state.close(MARKER, &mut ev);
                true
            }
            None => false,
        }
    }

    /// Show the selection `ids`: open its context dialog in place of the main menu
    /// (`HandleMsgNewDialog`) and fill it. `change` says which of the original's paths changed
    /// it ([`crate::selection::Shown`]). Returns the GUI events.
    pub fn show_selection(&mut self, pop: &Population, ids: &[u16], change: crate::selection::Shown) -> Vec<crate::guistate::UiEvent> {
        use crate::selection::Shown;
        let mut ev = Vec::new();
        if change == Shown::Request && !ids.is_empty() {
            // `RequestDialog` 0x4ee730: GUI event 0x260 closes the sidebar; a building's role
            // fill runs with 0 (event 0x25a, `CreateUITab`): its dialog is created anew, also for
            // the building already shown (the crush confirmation D43 gives way to it).
            self.close_sidebar();
            self.bdlg_shown = None;
            // It also puts the dialog's fill into the holder.
            self.race_holder = None;
        }
        if ids.is_empty() && change == Shown::Reset {
            // The leave handler's 0x260 and event 0x25b (`ResetMainMenu`).
            self.close_sidebar();
            self.state.reset_main_menu(&mut ev);
            self.bdlg_shown = None;
            return ev;
        }
        let context = crate::gui_pages::context_dialog(pop, ids);
        match context {
            Some(d) => {
                for other in [crate::vehicle_group::GROUP, crate::vehicle_group::FERRY, 44, 45] {
                    if other != d {
                        self.state.close(other, &mut ev);
                    }
                }
                self.state.open_context(d, &mut ev);
                self.bdlg_shown = None;
                // `CreateUITab` case 0x14: the group dialog's title, then its update.
                if d == crate::vehicle_group::GROUP {
                    let strings = self.strings.take();
                    let s = |id: u32| strings.as_ref().and_then(|t| t.get(id as usize)).unwrap_or("").to_string();
                    crate::vehicle_group::group_title(&mut self.state, &s);
                    self.strings = strings;
                }
                self.fill_selection(pop, ids);
            }
            None if ids.is_empty() => {
                // `sub_EE690` alone (the main and submenu tabs' 0x192, the search page): the
                // tab's own command has replaced the dialog.
                self.bdlg_shown = None;
            }
            None => {}
        }
        ev
    }

    /// The building dialog shown: (building, info code).
    pub fn building_shown(&self) -> Option<(u16, usize)> {
        self.bdlg_shown
    }

    /// Show the selected building `f` (`CInputProcessor::RequestDialog` → the role's vtable
    /// +0x58): its role fills its info object; a new building or code opens the dialog
    /// (`CreateUITab`, event 0x25a), else it is refilled (update 0x488e00, event 0x25c). While the
    /// crush confirmation (D43) is open only the info object follows. Returns whether the GUI
    /// changed.
    pub fn show_building(&mut self, f: &crate::building_dialog::Facts) -> bool {
        use crate::building_dialog::{DIALOGS, dialog_of, fill, update};
        let Some(code) = fill(&mut self.bdlg, f) else { return false };
        if self.bdlg_shown.is_some_and(|(id, c)| id == f.id && c == 10) {
            return false;
        }
        let before = (self.state.open.clone(), self.state.texts.clone(), self.state.effects.clone(), self.state.bars.clone(), self.state.images.clone(), self.state.fonts.clone());
        if self.bdlg_shown != Some((f.id, code)) || !self.state.open.contains(&dialog_of(code)) {
            self.create_tab(code);
            self.bdlg_shown = Some((f.id, code));
            self.race_holder = None;
        }
        let strings = self.strings.take();
        let s = |id: u32| strings.as_ref().and_then(|t| t.get(id as usize)).unwrap_or("").to_string();
        update(code, &self.bdlg, &mut self.state, &s);
        self.strings = strings;
        self.deferred_toggle(dialog_of(code));
        // The sidebar's provider refills it (event 0x25e) while it is open.
        if let Some((id, side, event)) = self.sidebar {
            if id == f.id && self.state.open.contains(&DIALOGS[side]) {
                if let Some(c) = crate::building_dialog::sidebar_fill(&mut self.bdlg, f, event) {
                    if c == 0xe {
                        self.trade.update(&mut self.state, &self.bdlg.obj[c]);
                    } else if c == 0xd {
                        let p = self.foreign_players(f.player_count);
                        self.foreign.update(&mut self.state, &self.bdlg.obj[c], &p);
                    } else if c == 0x11 {
                        let mut out = Vec::new();
                        self.workshop.update(&self.bdlg.obj[c], &mut out);
                        crate::workshop_sidebar::WorkshopSide::apply(&mut self.state, &out);
                    } else {
                        crate::building_dialog::sidebar_update(c, &self.bdlg, &mut self.state, false);
                    }
                }
            } else {
                self.sidebar = None;
            }
        }
        before != (self.state.open.clone(), self.state.texts.clone(), self.state.effects.clone(), self.state.bars.clone(), self.state.images.clone(), self.state.fonts.clone())
    }

    /// A sidebar button (GUI events 0x1c3 / 0x1c4 / 0x1c5 / 0x1c9, see
    /// [`crate::building_dialog::sidebar_fill`]) for building `f`: the sidebar opened next to its
    /// dialog (the opener 0x481260 on event 0x25f) and its procedure's init run (D46 0x4af090: the
    /// knobs, percentages and tab 1775; D47 0x468d50: tab 228; D48 0x467970: tab 140; D50: the
    /// race table). 0x1cc closes
    /// it. Returns whether it opened.
    pub fn open_sidebar(&mut self, f: &crate::building_dialog::Facts, event: u16) -> bool {
        use crate::building_dialog::{DIALOGS, sidebar_fill, sidebar_update};
        let mut ev = Vec::new();
        if event == 0x1cc {
            self.close_sidebar();
            return true;
        }
        let Some(code) = sidebar_fill(&mut self.bdlg, f, event) else { return false };
        for d in &DIALOGS[0xb..] {
            self.state.close(*d, &mut ev);
        }
        self.bdlg.gui_race = self.race;
        self.state.open(DIALOGS[code], &mut ev);
        self.sidebar = Some((f.id, code, event));
        sidebar_update(code, &self.bdlg, &mut self.state, true);
        // D53's procedure init (0x477950 code 0): its records by race, group tab 0x338.
        if code == 0xe {
            let info = self.bdlg.obj[code];
            self.trade.init(&mut self.state, &info, self.race);
        }
        // D54's (0x478d50 code 0): its records, group tab 0x34a.
        if code == 0xd {
            let info = self.bdlg.obj[code];
            let p = self.foreign_players(f.player_count);
            self.foreign.init(&mut self.state, &info, self.race, &p);
        }
        // D51's (0x469540 code 0): the layout by the first record, radio 0xfd, the update.
        if code == 0x11 {
            let mut out = Vec::new();
            self.workshop.init(&self.bdlg.obj[code], self.race, &mut out);
            crate::workshop_sidebar::WorkshopSide::apply(&mut self.state, &out);
        }
        // The inits' tab: D46 1775 (good 0x22), D47 228 (good 0x12), D48 140 (type 0x1d,
        // 0x467970).
        let tab = match code {
            0xb => Some((1775, 0x22)),
            0xc => Some((228, 0x12)),
            0x10 => Some((140, 0x1d)),
            _ => None,
        };
        if let Some((link, good)) = tab {
            let mut more = Vec::new();
            self.state.select(DIALOGS[code], link, &mut more);
            self.side_selected = good;
        }
        true
    }

    /// What D54's update reads: `PlayerCount`, the local player and `PlayerColorIndex` ([inf]
    /// player p has colour p − 1: the client keeps no player colours yet).
    fn foreign_players(&mut self, count: u8) -> crate::trade_sidebar::Players {
        self.players = count;
        crate::trade_sidebar::Players { count, local: self.local, colours: std::array::from_fn(|i| i as u8) }
    }

    /// Close the sidebar (event 0x1cc → 0x4ebce0 clears the provider).
    pub fn close_sidebar(&mut self) {
        let mut ev = Vec::new();
        if let Some((_, code, _)) = self.sidebar.take() {
            self.state.close(crate::building_dialog::DIALOGS[code], &mut ev);
        }
        if let Some(d) = self.sel_side.take() {
            self.state.close(d, &mut ev);
        }
    }

    /// UI event 0x1c2 (handler 0x4eba10, from D45's 0x5a0 / D44's 0x5ba): the unit groups
    /// sidebar D55 filled (0x56a510) and opened (0x481260 code 0x19). Returns whether it opened.
    pub fn open_groups_sidebar(&mut self, pop: &Population) -> bool {
        use crate::group_sidebar::{DIALOG, fill, update};
        self.close_sidebar();
        let mut ev = Vec::new();
        self.state.open(DIALOG, &mut ev);
        if !self.state.open.contains(&DIALOG) {
            return false;
        }
        self.sel_side = Some(DIALOG);
        update(&mut self.state, &fill(pop, self.local));
        true
    }

    /// UI event 0x1ca (handler 0x4ebc60, the town page's toggle 0x224): the eye catchers' fill
    /// 0x53d800 and its sidebar opened (0x481260 case 0x1b: dialog 52, procedure 0x46ecf0, whose
    /// code 0 runs the update 0x46e880 and zeroes the costs). Returns whether it opened.
    pub fn open_eyecatchers(&mut self, pop: &Population) -> bool {
        use crate::eyecatcher_sidebar as ec;
        self.close_sidebar();
        let mut ev = Vec::new();
        self.state.open(ec::DIALOG, &mut ev);
        if !self.state.open.contains(&ec::DIALOG) {
            return false;
        }
        self.sel_side = Some(ec::DIALOG);
        self.eyecatchers = ec::fill(pop, self.local, self.race, &self.costs);
        ec::update(&mut self.state, &self.eyecatchers);
        ec::costs(&mut self.state, &self.eyecatchers, None);
        true
    }

    /// UI event 0x1c1 (handler 0x4eba40, D45's 0x5a4): the spells sidebar D56 filled (0x56a670)
    /// and opened (0x481260 case 0x1a; the procedure's code 0 runs the update 0x47ba30). Returns
    /// whether it opened.
    pub fn open_spells_sidebar(&mut self, pop: &Population) -> bool {
        use crate::spell_sidebar as sp;
        self.close_sidebar();
        let mut ev = Vec::new();
        self.state.open(sp::DIALOG, &mut ev);
        if !self.state.open.contains(&sp::DIALOG) {
            return false;
        }
        self.sel_side = Some(sp::DIALOG);
        let rows = self.spell_rows(pop);
        sp::update(&mut self.state, &rows);
        true
    }

    /// UI event 0x1b4 (handler 0x4e9c20, from the extras' 929 in game type 3): the goods race
    /// fill 0x4f8da0 kept by the holder, and its 0x25a → `CreateUITab` case 0x24 (see
    /// [`Gui::goods_race_tab`]).
    pub fn open_goods_race(&mut self, pop: &Population) {
        self.race_holder = Some(pop.tick);
        self.goods_race_tab(pop);
    }

    /// `CreateUITab` 0x4841e0 case 0x24 after the fill 0x4f8da0: the page dialog closed, dialog
    /// 77 opened, its procedure's code 0 (init 0x474b20, update 0x474b70).
    fn goods_race_tab(&mut self, pop: &Population) {
        use crate::goods_race as gr;
        let mut ev = Vec::new();
        for s in crate::guistate::PAGES {
            self.state.close(s, &mut ev);
        }
        self.state.open(gr::DIALOG, &mut ev);
        let count = (1..9).rev().find(|&p| pop.teams[p] != 0).unwrap_or(0) as u8;
        let players = &pop.economy.players;
        let rows = gr::fill(pop.economy.race_goods, count, &pop.teams, |p, g| players.get(p as usize).and_then(|i| i.goods_start.get(g as usize)).copied().unwrap_or(0));
        self.race_rows = Some(rows);
        let strings = self.strings.take();
        let s = |id: u32| strings.as_ref().and_then(|t| t.get(id as usize)).unwrap_or("").to_string();
        gr::init(&mut self.state, &s);
        self.strings = strings;
        gr::update(&mut self.state, &rows);
    }

    /// The fill 0x56a670 for the local player.
    fn spell_rows(&self, pop: &Population) -> [crate::spell_sidebar::Row; 8] {
        let p = pop.economy.players.get(self.local as usize).cloned().unwrap_or_default();
        let count = (1..9).rev().find(|&q| pop.teams[q] != 0).unwrap_or(0) as u8;
        crate::spell_sidebar::fill(p.mana, &p.spells, self.local >= 1 && self.local <= count)
    }

    /// A click in the open sidebar: the GUI event its procedure sends.
    pub fn sidebar_click(&mut self, dialog: usize, link: u16, effects: u8) -> Option<(u16, u32, u32)> {
        let (_, code, _) = self.sidebar?;
        if crate::building_dialog::DIALOGS[code] != dialog {
            return None;
        }
        // D46 0x4af090 code 3 on a knob: 0x4aea40, then UI event 0x1cf with the four values
        // (0x15aad28 | 0x15aad2c << 8 | 0x15aad30 << 16 | 0x15aad34 << 24).
        if let (46, 0x6f7 | 0x6f8 | 0x6f9 | 0x6fb) = (dialog, link) {
            let v = self.state.weapon_percentages();
            return Some((0x1cf, u32::from_le_bytes(v), 0));
        }
        if dialog == crate::trade_sidebar::DIALOG {
            let info = self.bdlg.obj[code];
            return self.trade.click(&mut self.state, &info, link, effects);
        }
        if dialog == crate::workshop_sidebar::DIALOG {
            let mut out = Vec::new();
            self.workshop.command(link, effects & 1 != 0, &mut out);
            crate::workshop_sidebar::WorkshopSide::apply(&mut self.state, &out);
            return out.into_iter().find_map(|c| match c {
                crate::workshop_sidebar::Call::Event(id, w, l) => Some((id, w, l as u32)),
                _ => None,
            });
        }
        if dialog == crate::trade_sidebar::FOREIGN_DIALOG {
            let info = self.bdlg.obj[code];
            let p = self.foreign_players(self.players);
            return self.foreign.click(&mut self.state, &info, link, effects, &p);
        }
        crate::building_dialog::sidebar_event(dialog, link, effects, &mut self.side_selected, self.race)
    }

    /// `CreateUITab` (0x4841e0) for info code `code`: the open context dialog closed, then the
    /// code's dialog opened, after `ResetMainMenuForContext` (0x481110) except for code 10.
    fn create_tab(&mut self, code: usize) {
        use crate::building_dialog::{DIALOGS, dialog_of};
        let mut ev = Vec::new();
        for d in DIALOGS.iter().chain(&[44, 45, crate::vehicle_dialog::DIALOG, crate::vehicle_group::GROUP, crate::vehicle_group::FERRY]) {
            self.state.close(*d, &mut ev);
        }
        // D16's procedure init (0x49b090) and D41's update take the local player's race.
        if code == 4 || dialog_of(code) == crate::vehicle_dialog::DIALOG {
            self.bdlg.gui_race = self.race;
        }
        if code == 10 {
            self.state.open(DIALOGS[code], &mut ev);
        } else {
            self.state.open_context(dialog_of(code), &mut ev);
        }
    }

    /// The crush button (UI event 0x194 → 0x536850): the crush confirmation (D43) for building
    /// `f`, unless its role refuses. Returns whether it opened.
    pub fn crush_request(&mut self, f: &crate::building_dialog::Facts) -> bool {
        let Some(code) = crate::building_dialog::crush_request(&mut self.bdlg, f) else { return false };
        self.create_tab(code);
        self.bdlg_shown = Some((f.id, code));
        let strings = self.strings.take();
        let s = |id: u32| strings.as_ref().and_then(|t| t.get(id as usize)).unwrap_or("").to_string();
        crate::building_dialog::update(code, &self.bdlg, &mut self.state, &s);
        self.strings = strings;
        true
    }

    /// Forget the shown building dialog, so the next [`Gui::show_building`] opens it again
    /// (`RequestDialog` after the crush confirmation's "no", 0x4ee730).
    pub fn rerequest_building(&mut self) {
        self.bdlg_shown = None;
    }

    /// Refill the open selection dialog from a new snapshot. Returns whether anything changed.
    pub fn refill_selection(&mut self, pop: &Population, ids: &[u16]) -> bool {
        let before = (self.state.texts.clone(), self.state.effects.clone(), self.state.bars.clone());
        self.fill_selection(pop, ids);
        (&self.state.texts, &self.state.effects, &self.state.bars) != (&before.0, &before.1, &before.2)
    }

    /// The tail of the updates of D19 / D22 / D24 / D40 (0x49f8e0 / 0x46c230 / 0x4a5100 /
    /// 0x4ae560): with 0x12938c7 set, the sidebar toggle is pressed unless it is (`SelectControl`
    /// 0x677430: the radio clicked), and the flag cleared.
    fn deferred_toggle(&mut self, dialog: usize) {
        let Some(link) = crate::build_goto::deferred_toggle(dialog) else { return };
        if std::mem::take(&mut self.deferred_press) {
            let ev = self.state.select_control(dialog, link);
            self.deferred_events.extend(ev);
        }
    }

    /// UI event 0x1a6 (0x4eccc0, D42's 0x2e6): the group fill 0x5972b0 for the selection `ids`,
    /// event 0x25a → `CreateUITab` case 0x14: D40 opened in place of the context dialog, titled
    /// and filled.
    pub fn show_group_dialog(&mut self, pop: &Population, ids: &[u16]) {
        use crate::vehicle_group as vg;
        let mut ev = Vec::new();
        for d in [vg::FERRY, 44, 45] {
            self.state.close(d, &mut ev);
        }
        self.state.open_context(vg::GROUP, &mut ev);
        self.bdlg_shown = None;
        let strings = self.strings.take();
        let s = |id: u32| strings.as_ref().and_then(|t| t.get(id as usize)).unwrap_or("").to_string();
        vg::group_title(&mut self.state, &s);
        self.strings = strings;
        self.fill_selection(pop, ids);
    }

    /// Fill the open selection dialog: the vehicle group dialog (0x5972b0 → 0x4ae560), the ferry's
    /// (0x58e450 → 0x474e80), else the soldiers' / specialists' ([`crate::gui_pages::fill_selection`]).
    fn fill_selection(&mut self, pop: &Population, ids: &[u16]) {
        use crate::vehicle_group as vg;
        if self.state.open.contains(&vg::GROUP) {
            // 0x5972b0 counts every selected entity by its type (+0xc), whatever its class.
            let kinds = ids.iter().filter_map(|&id| {
                pop.vehicles.iter().chain(&pop.settlers).find(|u| u.id == id).map(|u| u.kind).or_else(|| pop.buildings.iter().find(|b| b.id == id).map(|b| b.kind as u16))
            });
            vg::group_update(&vg::group_fill(kinds), &mut self.state);
            self.deferred_toggle(vg::GROUP);
        } else if self.state.open.contains(&vg::FERRY) {
            // 0x58e450 for the first selected (the single ferry `RequestDialog` asked).
            let counts = ids.first().and_then(|id| pop.economy.ferries.get(id)).copied().unwrap_or_default();
            let strings = self.strings.take();
            let s = |id: u32| strings.as_ref().and_then(|t| t.get(id as usize)).unwrap_or("").to_string();
            vg::ferry_update(&vg::FerryInfo(counts.map(|c| c as u32)), &mut self.state, &s);
            self.strings = strings;
        } else {
            let titles = (self.string(885), self.string(886));
            crate::gui_pages::fill_selection(&mut self.state, pop, self.local, ids, (&titles.0, &titles.1));
        }
    }

    /// Refresh the open pages from `pop` (`CLogic::SetFillDialogInfos`, see `gui_pages`);
    /// `at` = the eco-sectors around the screen centre. Returns whether anything changed.
    pub fn update_pages(&mut self, pop: &Population, at: crate::gui_pages::Centre) -> bool {
        let before = (self.state.texts.clone(), self.state.effects.clone(), self.state.images.clone(), self.state.sliders.clone());
        // The logic tick 0x4f2720 runs the holder's goods race fill at every `tick & 0xf == 0xe`
        // (the first such tick after the last run).
        self.state.goods_race = pop.economy.game_type == 3;
        if let Some(last) = self.race_holder
            && ((last + 2) | 0xf) - 1 <= pop.tick
        {
            self.race_holder = Some(pop.tick);
            self.goods_race_tab(pop);
        }
        let strings = &self.strings;
        let text = |id: usize| strings.as_ref().and_then(|t| t.get(id)).unwrap_or("").to_string();
        crate::gui_pages::update(&mut self.state, pop, self.local, at, &text);
        self.settings.frame(&mut self.state, &|id| text(id as usize));
        // The eye catchers' provider 0x53d800 refills theirs (event 0x25e).
        if self.sel_side == Some(crate::eyecatcher_sidebar::DIALOG) {
            self.eyecatchers = crate::eyecatcher_sidebar::fill(pop, self.local, self.race, &self.costs);
            crate::eyecatcher_sidebar::update(&mut self.state, &self.eyecatchers);
        }
        // The spells sidebar's provider 0x56ac40 refills it (event 0x25e).
        if self.sel_side == Some(crate::spell_sidebar::DIALOG) {
            let rows = self.spell_rows(pop);
            crate::spell_sidebar::update(&mut self.state, &rows);
        }
        // The groups sidebar's provider 0x56ac20 refills it (event 0x25e).
        if self.sel_side == Some(crate::group_sidebar::DIALOG) {
            crate::group_sidebar::update(&mut self.state, &crate::group_sidebar::fill(pop, self.local));
        }
        // [inf] Player p has colour p − 1 (the client keeps no player colours); no team game; the
        // players `DisableLastNPlayersInStatistic` hides are not known (the script call is
        // unported).
        let input = crate::stat_graphs::Input {
            archive: pop.economy.history.clone(),
            player_count: (1..9).rev().find(|&p| pop.teams[p] != 0).unwrap_or(0) as u8,
            local: self.local,
            team_game: false,
            teams: pop.teams.map(i32::from),
            colours: std::array::from_fn(|p| p.saturating_sub(1) as u8),
            names: self.state.page_info.chat.names.clone(),
            hidden_last: self.hidden_last,
        };
        self.graphs.frame(&mut self.state, input, &|id| text(id as usize));
        (&self.state.texts, &self.state.effects, &self.state.images, &self.state.sliders) != (&before.0, &before.1, &before.2, &before.3)
    }

    /// A GUI event of a settings page (code 3, or code 2 of its sliders): the event for
    /// `EventManagement` (see [`crate::settings_pages`]).
    pub fn settings_event(&mut self, e: &crate::guistate::UiEvent) -> Option<crate::settings_pages::Event> {
        use crate::guistate::UiEvent;
        match *e {
            UiEvent::Command { dialog: d @ 36..=39, link, effects } => self.settings.command(&mut self.state, d, link, effects),
            UiEvent::Slider { dialog: d @ 37..=38, link, value } => self.settings.slider(d, link, value),
            _ => None,
        }
    }

    /// The statistics graph pages' code 3 and code 10 (stat_graphs.rs) and D51's codes 5 / 6
    /// (workshop_sidebar.rs) and the tutorial window's codes 3 / 5 / 6. Returns whether one was
    /// theirs.
    pub fn graph_events(&mut self, ev: &[crate::guistate::UiEvent]) -> bool {
        let tut = ev.iter().fold(false, |a, e| self.tutorial_event(e) | a);
        let strings = &self.strings;
        let text = |id: u32| strings.as_ref().and_then(|t| t.get(id as usize)).unwrap_or("").to_string();
        let mut any = false;
        for e in ev {
            any |= self.graphs.event(&mut self.state, e, &text);
            if let crate::guistate::UiEvent::Highlight { dialog: crate::workshop_sidebar::DIALOG, link, on } = *e
                && self.sidebar.is_some_and(|s| s.1 == 0x11)
            {
                let mut out = Vec::new();
                self.workshop.highlight(&self.bdlg.obj[0x11], link, on, &mut out);
                crate::workshop_sidebar::WorkshopSide::apply(&mut self.state, &out);
                any = true;
            }
        }
        any | tut
    }

    /// `CGuiEventHandler` event 0x26f (0x4805a0, while dialog 0 is open) →
    /// `SetMinimapButtonState` 0x49b630: `SelectControl(0, link, bit)` of the four minimap
    /// toggles (kind 1: the pressed bit and code 1 only, no code 3) — 0x266 bit 0, 0x268 bit 1,
    /// 0x267 bit 2, 0x269 bit 3 — and `MinimapViewFlags` (0x15aabbc) = `flags`, which the caller
    /// gives the minimap layer (s4-viewer client `minimap_toggles`). [unk] No sender of 0x26f was
    /// found in the exe (no immediate 0x26f event outside library code).
    pub fn set_minimap_buttons(&mut self, flags: u32) -> u32 {
        if self.state.open.contains(&0) {
            for (link, bit) in [(0x266, 0), (0x268, 1), (0x267, 2), (0x269, 3)] {
                let on = flags >> bit & 1 != 0;
                if self.state.is_pressed_link(0, link) != on {
                    self.state.set_pressed_link(0, link, on);
                }
            }
        }
        flags
    }

    /// `EventManagement` 0x1f's `SetSettings` and the page refilled (0x26b).
    pub fn settings_refill(&mut self) {
        let strings = &self.strings;
        let text = |id: u32| strings.as_ref().and_then(|t| t.get(id as usize)).unwrap_or("").to_string();
        if let Some(d) = (36..=39).find(|d| self.state.open.contains(d)) {
            self.settings.fill(d, &mut self.state, &text);
        }
    }

    /// The build pages' costs of the button under the cursor (codes 5 / 6). Returns whether the
    /// texts changed.
    pub fn hover_costs(&mut self) -> bool {
        let before = self.state.texts.clone();
        let h = self.state.hovered();
        crate::gui_pages::hover(&mut self.state, &self.costs, self.race, h);
        if self.sel_side == Some(crate::eyecatcher_sidebar::DIALOG) {
            let link = h.filter(|h| h.0 == crate::eyecatcher_sidebar::DIALOG).map(|h| h.1);
            crate::eyecatcher_sidebar::costs(&mut self.state, &self.eyecatchers, link);
        }
        self.state.texts != before
    }

    /// `IsNonTransparentGuiArea`: is panel point (x, y) on a drawn GUI pixel?
    /// The message button (dialog 59) opened (event 0x27 / 0x26a) and placed by
    /// `ToggleUISurfaceVisibility` (0x488650): x = 0xd1 = 209, y = H - 1 - height.
    pub fn msg_button(&mut self) {
        let mut ev = Vec::new();
        if let Some(d) = self.state.set.dialogs.get_mut(crate::msg_window::BUTTON).and_then(|d| d.as_mut()) {
            d.x = 209;
            d.y = (self.screen_h as i32 - 1 - d.height as i32).max(0) as u16;
        }
        self.state.open(crate::msg_window::BUTTON, &mut ev);
    }

    /// The button's images (the blink: `SetImages(0x3b, 0x37b, ...)`). Returns whether they
    /// changed.
    pub fn msg_button_lit(&mut self, lit: bool) -> bool {
        let im = if lit { crate::msg_window::IMAGES_LIT } else { crate::msg_window::IMAGES };
        self.state.images.insert((crate::msg_window::BUTTON, crate::msg_window::BUTTON_LINK), im) != Some(im)
    }

    /// Event 0x269: the button closed, the window's dialog (`SelectChatSize` for a screen
    /// `screen_w` panel px wide) opened at (209, H - 1 - height) and initialised (`sub_99820`).
    pub fn msg_open(&mut self, screen_w: u32, extended_tooltips: bool) {
        use crate::msg_window::*;
        if self.msg_dialog.is_some() || self.tut_dialog.is_some() {
            return;
        }
        let mut ev = Vec::new();
        self.state.close(BUTTON, &mut ev);
        let dlg = dialog_for(screen_w, extended_tooltips);
        if let Some(d) = self.state.set.dialogs.get_mut(dlg).and_then(|d| d.as_mut()) {
            d.x = 209;
            d.y = (self.screen_h as i32 - 1 - d.height as i32).max(0) as u16;
        }
        self.state.open(dlg, &mut ev);
        self.msg_dialog = Some(dlg);
        self.state.set_hidden(dlg, CHAT_INPUT, true);
        self.msg_show(0);
    }

    /// GUI event 0x263 (`CGuiEventHandler` 0x4805a0, `Tutorial.ShowText`): without a window,
    /// event 0x269 (1) opens the tutorial window first (the button closed, `SelectChatSize`'s
    /// tutorial dialog at (209, H - 1 - height), `ChatEvents2`'s init 0x4ad470: fonts 0xb / 0xe on
    /// the lines, then UI event 0x276); then the line `line` (0 → 0x6af, else 0x6b0) gets `text`.
    /// [not ported] the sound 7 it plays.
    pub fn tutorial_text(&mut self, line: usize, text: String, screen_w: u32, extended_tooltips: bool) {
        use crate::msg_window::*;
        if self.tut_dialog.is_none() && self.msg_dialog.is_none() {
            let mut ev = Vec::new();
            self.state.close(BUTTON, &mut ev);
            let dlg = tutorial_dialog_for(screen_w, extended_tooltips);
            if let Some(d) = self.state.set.dialogs.get_mut(dlg).and_then(|d| d.as_mut()) {
                d.x = 209;
                d.y = (self.screen_h as i32 - 1 - d.height as i32).max(0) as u16;
            }
            self.state.open(dlg, &mut ev);
            self.tut_dialog = Some(dlg);
            self.state.fonts.insert((dlg, TUTORIAL_LINES[0]), 0xb);
            self.state.fonts.insert((dlg, TUTORIAL_LINES[1]), 0xe);
            self.tut_events.push(0x276);
        }
        let Some(dlg) = self.tut_dialog.or(self.msg_dialog) else { return };
        self.state.texts.insert((dlg, TUTORIAL_LINES[(line != 0) as usize]), text);
    }

    /// `ChatEvents2` (0x4ad610) codes 3 / 5 / 6: a click on line 1711 sends UI event 0x276, on
    /// 1712 0x275 (with sound 6, [not ported]), and the line's font goes back (0x4ad5a0: 0xb /
    /// 0xe); the cursor on a line gives it font 0xd, off it the normal one. Returns whether the
    /// event was the window's.
    fn tutorial_event(&mut self, e: &crate::guistate::UiEvent) -> bool {
        use crate::guistate::UiEvent;
        use crate::msg_window::TUTORIAL_LINES;
        let Some(dlg) = self.tut_dialog else { return false };
        let (link, highlight) = match *e {
            UiEvent::Command { dialog, link, .. } if dialog == dlg && TUTORIAL_LINES.contains(&link) => {
                self.tut_events.push(if link == TUTORIAL_LINES[0] { 0x276 } else { 0x275 });
                (link, false)
            }
            UiEvent::Highlight { dialog, link, on } if dialog == dlg && TUTORIAL_LINES.contains(&link) => (link, on),
            _ => return false,
        };
        let font = if highlight {
            0xd
        } else if link == TUTORIAL_LINES[0] {
            0xb
        } else {
            0xe
        };
        self.state.fonts.insert((dlg, link), font);
        true
    }

    /// Code 9 for the control the cursor entered in `dialog` (`tooltip`: its extended tooltip
    /// string id, `None` for -1), at `now` ms (see [`ExtTooltip`]). Returns whether the window
    /// changed.
    pub fn ext_tip_enter(&mut self, dialog: usize, tooltip: Option<u16>, now: u64) -> bool {
        let text: String = tooltip.map(|id| self.string(id as usize)).unwrap_or_default().chars().take(299).collect();
        let before = (self.ext_tip.shown, self.ext_tip.text.clone());
        let t = &mut self.ext_tip;
        t.hovering = true;
        t.text = text;
        if !t.shown {
            t.source = Some(dialog);
            t.shown = t.enabled && !t.text.is_empty();
        } else if t.text.is_empty() {
            t.source = None;
            t.hide_at = Some(now + 700);
        } else {
            t.source = Some(dialog);
            t.hide_at = None;
        }
        before != (self.ext_tip.shown, self.ext_tip.text.clone())
    }

    /// Each frame: the window's place for a `screen_w` panel px wide screen, the setting
    /// (`enabled`), the cursor off every control, the hide timer and a closed source dialog, at
    /// `now` ms. Returns whether the window changed.
    pub fn ext_tip_frame(&mut self, screen_w: u32, enabled: bool, now: u64) -> bool {
        let screen_h = self.screen_h;
        if enabled && let Some(d) = self.state.set.dialogs.get_mut(EXT_TIP_DIALOG).and_then(|d| d.as_mut()) {
            d.x = (screen_w as i32 - 1 - d.width as i32).max(0) as u16;
            d.y = (screen_h as i32 - 1 - d.height as i32).max(0) as u16;
        }
        let before = (self.ext_tip.shown, self.ext_tip.text.clone());
        let hovered = self.state.hovered().is_some();
        let source_open = self.ext_tip.source.is_none_or(|d| self.state.open.contains(&d));
        let t = &mut self.ext_tip;
        t.enabled = enabled;
        if !enabled {
            t.shown = false;
        }
        if t.hovering && !hovered {
            t.hovering = false;
            if t.shown {
                t.text.clear();
                t.source = None;
                t.hide_at = Some(now + 700);
            }
        }
        if t.shown && t.hide_at.is_some_and(|h| now > h) {
            t.shown = false;
            t.hide_at = None;
        }
        if t.shown && !source_open {
            t.shown = false;
        }
        before != (self.ext_tip.shown, self.ext_tip.text.clone())
    }

    /// The GUI timer for the short tooltip (see [`ShortTooltip`]): `cursor` in panel px, the
    /// screen `screen_w` panel px wide, at `now` ms. Returns whether it changed.
    pub fn short_tip_frame(&mut self, cursor: Option<(i32, i32)>, screen_w: u32, now: u64) -> bool {
        let before = (self.tip.shown, self.tip.rect, self.tip.text.clone());
        let hovered = self.state.hovered();
        let t = &mut self.tip;
        if hovered != t.hovered {
            if t.hovered.is_some() {
                if t.shown {
                    t.left_at = now;
                }
                t.shown = false;
                t.held = false;
            }
            t.hovered = hovered;
        }
        let Some((dialog, link)) = hovered else { return before != (self.tip.shown, self.tip.rect, self.tip.text.clone()) };
        if t.shown {
            if now > t.shown_at + 8000 {
                t.shown = false;
                t.held = true;
            }
        } else if !t.held && (now > t.pressed_at + 700 || now < t.left_at + 400) {
            let id = self.state.index_of(dialog, link).and_then(|i| self.state.set.dialog(dialog).map(|d| d.controls[i].tooltip)).unwrap_or(0xffff);
            let id = self.state.tooltips.get(&(dialog, link)).map_or(id, |t| t.0);
            let text: String = if id == 0xffff { String::new() } else { self.string(id as usize) }.chars().take(299).collect();
            if let Some((cx, cy)) = cursor.filter(|_| !text.is_empty()) {
                let (tw, th) = (self.text.width(&text, 8) + 4, FONTS[8].height + 4);
                let (mut x, mut y) = (cx, cy + 18);
                if x + tw > screen_w as i32 {
                    x = screen_w as i32 - tw;
                }
                x = x.max(0);
                if y + th > self.screen_h as i32 {
                    y = self.screen_h as i32 - th;
                }
                self.tip.rect = (x, y, tw, th);
                self.tip.shown = true;
            }
            self.tip.text = text;
            self.tip.shown_at = now;
        }
        before != (self.tip.shown, self.tip.rect, self.tip.text.clone())
    }

    /// A left press (0x673893): noted; a shown short tooltip hides and is held back. Returns
    /// whether it was shown.
    pub fn short_tip_press(&mut self, now: u64) -> bool {
        let t = &mut self.tip;
        t.pressed_at = now;
        let was = t.shown;
        if t.shown {
            t.shown = false;
            t.held = true;
        }
        was
    }

    /// `UIChatInteraction` (0x499980, Enter in the game: `IngameKeybindings` 0x4890b0 after
    /// event 0x269): three line slots (`sub_99220(3, …)`: 958..960 and their icons), the lines
    /// again, the chat input 969 shown and focused (`SomethingUIAndChatRelevant` 0x6756a0).
    pub fn chat_open(&mut self) {
        use crate::msg_window::*;
        let Some(dlg) = self.msg_dialog else { return };
        self.msg.visible = 3;
        self.msg_show(self.msg.last);
        self.state.set_hidden(dlg, CHAT_INPUT, false);
        self.state.focus = Some((dlg, CHAT_INPUT));
    }

    /// `sub_99B90` (0x499b90, after the input's return or loss of focus): the input emptied and
    /// hidden, four line slots again (`sub_99220(4, …)`), the lines again.
    pub fn chat_close(&mut self) {
        use crate::msg_window::*;
        let Some(dlg) = self.msg_dialog else { return };
        self.state.texts.insert((dlg, CHAT_INPUT), String::new());
        self.state.set_hidden(dlg, CHAT_INPUT, true);
        if self.state.focus == Some((dlg, CHAT_INPUT)) {
            self.state.focus = None;
        }
        self.msg.visible = 4;
        self.msg_show(self.msg.last);
    }

    /// The chat input's text (`GetText(window, 0x3c9)`).
    pub fn chat_text(&self) -> String {
        self.msg_dialog.and_then(|d| self.state.texts.get(&(d, crate::msg_window::CHAT_INPUT)).cloned()).unwrap_or_default()
    }

    /// Event 0x26a: the window closed, the button back with images 0x148 / 0x149.
    pub fn msg_close(&mut self) {
        let Some(dlg) = self.msg_dialog.take() else { return };
        let mut ev = Vec::new();
        self.state.close(dlg, &mut ev);
        self.msg_button();
        self.msg_button_lit(false);
    }

    /// `ChatCallback` for a refresh: the list rebuilt from (text, icon) pairs, the newest shown.
    pub fn msg_set(&mut self, messages: Vec<(String, u16)>) {
        if self.msg_dialog.is_none() {
            return;
        }
        let n = messages.len() as i32;
        self.msg.messages = messages;
        self.msg_show(n - 1);
    }

    /// `HandleChatMessageLines(last)` into the open window's controls.
    pub fn msg_show(&mut self, last: i32) {
        use crate::msg_window::*;
        let Some(dlg) = self.msg_dialog else { return };
        let Some(d) = self.state.set.dialog(dlg) else { return };
        let w = d.controls.iter().find(|c| c.link == LINES[0]).map_or(0, |c| c.width as i32);
        let text = &self.text;
        let measure = |t: &str| text.width(t, FONT as usize);
        show(&mut self.msg, last, &|t| fit_in(t, w, &measure));
        for (k, s) in self.msg.slots.clone().into_iter().enumerate() {
            self.state.set_hidden(dlg, LINES[k], s.text.is_none());
            if let Some(t) = s.text {
                self.state.texts.insert((dlg, LINES[k]), t);
            }
            self.state.set_hidden(dlg, ICONS[k], s.icon.is_none());
            if let Some(i) = s.icon {
                self.state.images.insert((dlg, ICONS[k]), (i, 0));
            }
        }
    }

    /// Codes 5 / 6 on a line (`sub_99CC0`): every slot showing its message gets font 10 / 9.
    pub fn msg_highlight(&mut self, link: u16, on: bool) {
        use crate::msg_window::*;
        let (Some(dlg), Some(k)) = (self.msg_dialog, LINES.iter().position(|&l| l == link)) else { return };
        let m = self.msg.slots[k].message;
        for (s, l) in self.msg.slots.iter().zip(LINES) {
            if s.message == m {
                self.state.fonts.insert((dlg, l), if on { FONT_HIGHLIGHT } else { FONT });
            }
        }
    }

    /// Code 10 (wheel): the four slots' messages back to font 9, then one message up (`delta`
    /// > 0) or down (< 0).
    pub fn msg_wheel(&mut self, delta: i32) {
        use crate::msg_window::*;
        for l in LINES {
            self.msg_highlight(l, false);
        }
        match delta.signum() {
            1 => self.msg_show(self.msg.last - 1),
            -1 => self.msg_show(self.msg.last + 1),
            _ => {}
        }
    }

    /// The message index of line `link` (the click's event 0x1771).
    pub fn msg_line(&self, link: u16) -> Option<i32> {
        let k = crate::msg_window::LINES.iter().position(|&l| l == link)?;
        Some(self.msg.slots[k].message)
    }

    pub fn over_gui(&self, x: i32, y: i32) -> bool {
        // A dialog without a background bitmap (flags 2) is opaque in its whole rectangle.
        let mut solid = self.state.open.iter().filter_map(|&s| self.state.set.dialog(s)).filter(|d| d.flags & 2 != 0);
        if solid.any(|d| x >= d.x as i32 && y >= d.y as i32 && x < d.x as i32 + d.width as i32 && y < d.y as i32 + d.height as i32) {
            return true;
        }
        self.last.as_ref().is_some_and(|p| {
            x >= 0 && y >= 0 && (x as u32) < p.width && (y as u32) < p.height && p.rgba[((y as u32 * p.width + x as u32) * 4 + 3) as usize] > 0
        })
    }

    /// Compose and remember the image (see [`Gui::compose`]).
    pub fn render(&mut self) -> anyhow::Result<&PanelImage> {
        let img = self.compose()?;
        Ok(self.last.insert(img))
    }

    /// Compose the open dialogs into one image (`DrawControl` order: background, then each
    /// control's image and text).
    pub fn compose(&self) -> anyhow::Result<PanelImage> {
        let st = &self.state;
        let dialogs: Vec<&Dialog> = st.open.iter().filter_map(|&s| st.set.dialog(s)).collect();
        let tip = st.set.dialog(EXT_TIP_DIALOG).filter(|_| self.ext_tip.shown);
        let short = self.tip.shown.then_some(self.tip.rect);
        let width = dialogs.iter().chain(tip.iter()).map(|d| d.x as u32 + d.width as u32).chain(short.map(|r| (r.0 + r.2).max(0) as u32)).max().unwrap_or(1);
        let height = dialogs.iter().chain(tip.iter()).map(|d| d.y as u32 + d.height as u32).chain(short.map(|r| (r.1 + r.3).max(0) as u32)).max().unwrap_or(1).min(self.screen_h.max(1));
        let mut img = vec![0u8; (width * height * 4) as usize];
        let w = width as usize;
        let gfx = &self.gfx;
        let blit = |img: &mut Vec<u8>, ox: i32, oy: i32, index: u16, colour_key: bool, greyed: bool| -> anyhow::Result<()> {
            let im = gfx.decode(index as usize)?;
            let ix = gfx.decode_indexed(index as usize)?;
            for y in 0..im.height as i32 {
                for x in 0..im.width as i32 {
                    let (px, py) = (ox + x, oy + y);
                    if px < 0 || py < 0 || px as u32 >= width || py as u32 >= height {
                        continue;
                    }
                    let i = (y as u32 * im.width + x as u32) as usize;
                    let s = &im.rgba[i * 4..i * 4 + 4];
                    // Raw backgrounds: index 0 is the colour key (IsNonTransparentGuiArea).
                    if s[3] == 0 || (colour_key && ix.raw && ix.pixels[i] == 0) {
                        continue;
                    }
                    let o = (py as usize * w + px as usize) * 4;
                    let rgb = if greyed { grey(s) } else { [s[0], s[1], s[2]] };
                    img[o..o + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
                }
            }
            Ok(())
        };
        for d in &dialogs {
            let (dx, dy) = (d.x as i32, d.y as i32);
            if d.flags & 2 == 0 {
                blit(&mut img, dx, dy, d.background, true, false)?;
            } else {
                // `RenderGui` (0x6721e0) -> `sub_2788D0`: black on every other pixel, starting
                // at the dialog's first pixel and alternating per row; the rest stays the colour
                // key (transparent).
                for y in 0..d.height as i32 {
                    for x in ((y & 1)..d.width as i32).step_by(2) {
                        let (px, py) = (dx + x, dy + y);
                        if px < 0 || py < 0 || px as u32 >= width || py as u32 >= height {
                            continue;
                        }
                        let o = (py as usize * w + px as usize) * 4;
                        img[o..o + 4].copy_from_slice(&[0, 0, 0, 255]);
                    }
                }
            }
            for (i, c) in d.controls.iter().enumerate() {
                let fx = st.effects.get(&(d.slot, i)).copied().unwrap_or(c.effects);
                let pressed = fx & 1 != 0;
                if c.kind == 10 {
                    if fx & 8 == 0 {
                        let pos = st.bars.get(&(d.slot, c.link)).copied().unwrap_or(0);
                        draw_bar(&mut img, (width, height), (dx + c.x as i32, dy + c.y as i32, c.width as i32, c.height as i32), pos);
                    }
                    continue;
                }
                let (img_n, img_p) = st.images.get(&(d.slot, c.link)).copied().unwrap_or((c.image, c.pressed_image));
                // `DrawGUIElement` 0x672600 asks `CGfxManager` vtbl+0 (0x461c70) for the image and
                // skips only a missing one, for every kind it draws: image 0 is an image (the start
                // screens' red colour icon; MCD2 / MCD3's briefing back gem, 1803 / 2915, 43×65).
                // A kind-0 control whose image 0 has another size fails the size check below.
                if c.has_image() && fx & 8 == 0 {
                    let image = if pressed && img_p != 0 && img_p != 0xffff { img_p } else { img_n };
                    if control_image_drawn(gfx, d, c, image)? {
                        blit(&mut img, dx + c.x as i32, dy + c.y as i32, image, false, fx & 4 != 0)?;
                    }
                }
                if fx & 8 == 0
                    && let Some((ow, oh, px)) = st.owner_images.get(&(d.slot, c.link))
                {
                    for y in 0..*oh as i32 {
                        for x in 0..*ow as i32 {
                            let (tx, ty) = (dx + c.x as i32 + x, dy + c.y as i32 + y);
                            let s = ((y as u32 * ow + x as u32) * 4) as usize;
                            if tx < 0 || ty < 0 || tx as u32 >= width || ty as u32 >= height || px[s + 3] == 0 {
                                continue;
                            }
                            let o = (ty as usize * w + tx as usize) * 4;
                            img[o..o + 4].copy_from_slice(&px[s..s + 4]);
                        }
                    }
                }
                if fx & 8 == 0
                    && let Some(t) = st.texts.get(&(d.slot, c.link))
                {
                    let rect = (dx + c.x as i32, dy + c.y as i32, c.width as i32, c.height as i32);
                    let font = st.fonts.get(&(d.slot, c.link)).copied().unwrap_or(c.font);
                    let scroll = st.scroll.get(&(d.slot, c.link)).copied();
                    // [inf] The focused edit field shows a caret after its text.
                    let shown = if st.focus == Some((d.slot, c.link)) { format!("{t}|") } else { t.clone() };
                    self.text.draw(&mut img, w, rect, &shown, (font as usize, c.text_style as usize, pressed), scroll);
                }
            }
        }
        if let Some(d) = tip {
            let (dx, dy) = (d.x as i32, d.y as i32);
            for y in 0..d.height as i32 {
                for x in ((y & 1)..d.width as i32).step_by(2) {
                    let (px, py) = (dx + x, dy + y);
                    if px < 0 || py < 0 || px as u32 >= width || py as u32 >= height {
                        continue;
                    }
                    let o = (py as usize * w + px as usize) * 4;
                    img[o..o + 4].copy_from_slice(&[0, 0, 0, 255]);
                }
            }
            if !self.ext_tip.text.is_empty() {
                self.text.draw(&mut img, w, (dx + 4, dy + 2, d.width as i32 - 4, d.height as i32), &self.ext_tip.text, (6, 6, false), None);
            }
        }
        if let Some((x, y, tw, th)) = short {
            for py in y..y + th {
                for px in x..x + tw {
                    if px < 0 || py < 0 || px as u32 >= width || py as u32 >= height {
                        continue;
                    }
                    let edge = px == x || py == y || px == x + tw - 1 || py == y + th - 1;
                    let o = (py as usize * w + px as usize) * 4;
                    img[o..o + 4].copy_from_slice(if edge { &[0, 0, 0, 255] } else { &[255, 255, 180, 255] });
                }
            }
            // A kind-14 control at (2, 2), text extent wide: `draw` insets by 2 on both sides.
            self.text.draw(&mut img, w, (x, y + 2, tw, th - 4), &self.tip.text, (8, 4, false), None);
        }
        Ok(PanelImage { width, height, rgba: img })
    }
}

/// Whether `DrawGUIElement` (0x672600) blits control `c`'s `image`: not when the control reaches
/// past its dialog ("Control exceeds container surface", 0x672a2f), not for a raw image (flag
/// 0x20: that branch only blits palette images, 0x672a4e), and not when the image's size is not
/// the control's ("Control size mismatch or illegal gfx data", 0x672ae0..0x672b06). The sets'
/// own images all fit (none raw, none of another size); images set at run time may not (the
/// victory screen's 618×442 race pictures on its 89×388 side panel, 0x4906c0).
fn control_image_drawn(gfx: &GfxFile, d: &Dialog, c: &s4_formats::gui::Control, image: u16) -> anyhow::Result<bool> {
    if c.x as u32 + c.width as u32 > d.width as u32 || c.y as u32 + c.height as u32 > d.height as u32 {
        return Ok(false);
    }
    let ix = gfx.decode_indexed(image as usize)?;
    Ok(!ix.raw && (ix.header.width, ix.header.height) == (c.width, c.height))
}

/// `UnpackGfxTransparentGrayed565` (a disabled control, effects bit 4): the RGB565 pixel's
/// `r5 + g6 / 2 + b5`, divided by 3 (`InitTables`: `byte_4727330[k] = k / 3`), as a 5-bit grey
/// written to all three channels (`grey · 0x841`).
fn grey(s: &[u8]) -> [u8; 3] {
    let (r5, g6, b5) = (s[0] as u32 >> 3, s[1] as u32 >> 2, s[2] as u32 >> 3);
    let g = (r5 + (g6 >> 1) + b5) / 3;
    let (c5, c6) = ((g << 3 | g >> 2) as u8, ((g << 1) << 2 | (g << 1) >> 4) as u8);
    [c5, c6, c5]
}

/// `DrawControl` for a bar (kind 10, e.g. the selection dialogs' health bars): the filled part
/// (`CalcSliderPosition(y + 1, y + h − 1, pos, 1)` down to the bottom) in `g_uShadeTable2`, the
/// rest in `g_uShadeTable1`, one table entry per column (`16 / (w − 2)` steps), then a bevel:
/// left and top `g_iRightShadeColor` (50, 41, 45), right and bottom `g_iLeftShadeColor`
/// (199, 178, 111). Colours as `IGuiEngine::InitShadeTables` makes them, through RGB565.
/// [inf] `FastVLine` / `FastHLine` include both end points.
fn draw_bar(img: &mut [u8], (iw, ih): (u32, u32), (x, y, w, h): (i32, i32, i32, i32), pos: u8) {
    let hicol = |r: i32, g: i32, b: i32| -> [u8; 4] {
        let q = |v: i32, bits: u32| {
            let m = ((v.clamp(0, 255) as u32) >> (8 - bits)) as u8;
            (m << (8 - bits)) | (m >> (2 * bits - 8))
        };
        [q(r, 5), q(g, 6), q(b, 5), 255]
    };
    let table = |base: (i32, i32, i32), i: usize| -> [u8; 4] {
        let k = if i == 0 { 1.0 } else { (i - 1) as f32 * 0.08 + 1.0 };
        let c = |v: i32| ((k * v as f32) as i32).min(255);
        hicol(c(base.0), c(base.1), c(base.2))
    };
    let mut put = |px: i32, py: i32, c: [u8; 4]| {
        if px >= 0 && py >= 0 && (px as u32) < iw && (py as u32) < ih {
            let o = ((py as u32 * iw + px as u32) * 4) as usize;
            img[o..o + 4].copy_from_slice(&c);
        }
    };
    let (top, bottom) = (y + 1, y + h - 1);
    let fill = bottom - ((pos as f32 * ((bottom - top) as f32 * 0.01)) + 0.5) as i32;
    let step = 16.0 / (w - 2).max(1) as f32;
    if pos > 0 {
        let mut t = 0.0f32;
        for px in x + 1..x + w - 1 {
            for py in fill..=bottom {
                put(px, py, table((10, 150, 3), t as usize));
            }
            t += step;
        }
    }
    if pos < 100 {
        let mut t = 0.0f32;
        for px in x + 1..x + w - 1 {
            for py in top..=fill {
                put(px, py, table((150, 4, 12), t as usize));
            }
            t += step;
        }
    }
    let (dark, light) = (hicol(50, 41, 45), hicol(199, 178, 111));
    for py in y..y + h {
        put(x, py, dark);
        put(x + w - 1, py, light);
    }
    for px in x..x + w {
        put(px, y, dark);
        put(px, y + h - 1, light);
    }
}

// ------------------------------------------------------------------------------------------------
// GPU overlay

/// One textured rectangle of the overlay, placed in panel pixels.
struct Layer {
    texture: wgpu::Texture,
    bind: wgpu::BindGroup,
    uniforms: wgpu::Buffer,
    size: (u32, u32),
    pos: (i32, i32),
}

/// The GUI drawn over the game view: the minimap (below the panel; it shows through the
/// colour-keyed hole of the minimap frame) and the composed panel.
pub struct GuiOverlay {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    panel: Layer,
    minimap: Option<Layer>,
    /// Hover highlight (not in the original: its only feedback is the cursor shape).
    marker: Option<Layer>,
    marker_at: Option<(i32, i32)>,
    /// The selection box (see [`GuiOverlay::set_selection_box`]): its four 1 px edges, in screen
    /// px.
    edges: Vec<Layer>,
    selection_box: Option<[(i32, i32, i32, i32); 4]>,
}

impl GuiOverlay {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat, panel: &PanelImage) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gui"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gui"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        // Nearest: the original draws the GUI 1:1; scaled up (HiDPI) it keeps its pixels.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor { label: Some("gui"), ..Default::default() });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gui"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("gui"),
            layout: Some(&pl),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() },
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        let layer = Self::layer(device, &layout, &sampler, (panel.width, panel.height), (0, 0));
        Self::upload(queue, &layer, &panel.rgba);
        GuiOverlay { pipeline, layout, sampler, panel: layer, minimap: None, marker: None, marker_at: None, edges: Vec::new(), selection_box: None }
    }

    fn layer(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, sampler: &wgpu::Sampler, size: (u32, u32), pos: (i32, i32)) -> Layer {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("gui layer"),
            size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // The bytes are sRGB-encoded art; sampling decodes, the sRGB target re-encodes.
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gui layer"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("gui layer"),
            layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&texture.create_view(&Default::default())),
                },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(sampler) },
            ],
        });
        Layer { texture, bind, uniforms, size, pos }
    }

    fn upload(queue: &wgpu::Queue, l: &Layer, rgba: &[u8]) {
        queue.write_texture(
            l.texture.as_image_copy(),
            rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(l.size.0 * 4), rows_per_image: Some(l.size.1) },
            wgpu::Extent3d { width: l.size.0, height: l.size.1, depth_or_array_layers: 1 },
        );
    }

    /// Replace the panel image (after the GUI state changed).
    pub fn set_panel(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, panel: &PanelImage) {
        if self.panel.size != (panel.width, panel.height) {
            self.panel = Self::layer(device, &self.layout, &self.sampler, (panel.width, panel.height), (0, 0));
        }
        Self::upload(queue, &self.panel, &panel.rgba);
    }

    /// Replace the minimap image (`size` RGBA) drawn at panel position `pos`.
    pub fn set_minimap(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, size: (u32, u32), pos: (i32, i32), rgba: &[u8]) {
        if self.minimap.as_ref().is_none_or(|m| m.size != size || m.pos != pos) {
            self.minimap = Some(Self::layer(device, &self.layout, &self.sampler, size, pos));
        }
        if let Some(m) = &self.minimap {
            Self::upload(queue, m, rgba);
        }
    }

    /// Show the hover marker centred on screen point `at` (screen px), or hide it.
    pub fn set_marker(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, at: Option<(f32, f32)>, scale: f32) {
        // A node's hexagon outline at zoom 1: the six triangles around it span ±24 px x, ±12 px y.
        const MW: u32 = 49;
        const MH: u32 = 25;
        if self.marker.is_none() {
            let l = Self::layer(device, &self.layout, &self.sampler, (MW, MH), (0, 0));
            let mut img = vec![0u8; (MW * MH * 4) as usize];
            let hex = [(12.0f32, 0.0f32), (36.0, 0.0), (48.0, 12.0), (36.0, 24.0), (12.0, 24.0), (0.0, 12.0)];
            for k in 0..6 {
                let (a, b) = (hex[k], hex[(k + 1) % 6]);
                for t in 0..=48 {
                    let f = t as f32 / 48.0;
                    let (x, y) = ((a.0 + (b.0 - a.0) * f).round() as u32, (a.1 + (b.1 - a.1) * f).round() as u32);
                    let o = ((y.min(MH - 1) * MW + x.min(MW - 1)) * 4) as usize;
                    img[o..o + 4].copy_from_slice(&[255, 255, 255, 200]);
                }
            }
            Self::upload(queue, &l, &img);
            self.marker = Some(l);
        }
        self.marker_at = at.map(|(x, y)| ((x / scale) as i32 - (MW as i32) / 2, (y / scale) as i32 - (MH as i32) / 2));
    }

    /// The selection box's four edges as drawn (x, y, width, height), if shown.
    pub fn selection_box(&self) -> Option<[(i32, i32, i32, i32); 4]> {
        self.selection_box
    }

    /// The selection box while the left button drags on the map (`EventManagement` 0x5172f0 sets
    /// it with `sub_25A6D0(rect, GetColorFromRGB(0xff, 0xff, 0xff))`; the object renderer
    /// 0x660070 draws it at 0x661887 after the objects): corners `(x0, y0, x1, y1)` in screen px,
    /// any order, put in order and clamped to the screen, a 1 px white outline including both
    /// corners. `None` hides it.
    pub fn set_selection_box(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, screen: (u32, u32), rect: Option<(i32, i32, i32, i32)>) {
        if self.edges.is_empty() {
            for _ in 0..4 {
                let l = Self::layer(device, &self.layout, &self.sampler, (1, 1), (0, 0));
                Self::upload(queue, &l, &[255, 255, 255, 255]);
                self.edges.push(l);
            }
        }
        self.selection_box = rect.map(|(ax, ay, bx, by)| {
            let (w, h) = (screen.0 as i32, screen.1 as i32);
            let (x0, x1) = (ax.min(bx).max(0), ax.max(bx).min(w - 1));
            let (y0, y1) = (ay.min(by).max(0), ay.max(by).min(h - 1));
            let (rw, rh) = ((x1 - x0 + 1).max(1), (y1 - y0 + 1).max(1));
            [(x0, y0, rw, 1), (x0, y1, rw, 1), (x0, y0, 1, rh), (x1, y0, 1, rh)]
        });
    }

    /// Anchored at the top-left of the screen, `scale` screen px per panel px.
    pub fn draw(&self, queue: &wgpu::Queue, pass: &mut wgpu::RenderPass<'_>, screen: (u32, u32), scale: f32, origin: (f32, f32)) {
        let (sw, sh) = (screen.0.max(1) as f32, screen.1.max(1) as f32);
        pass.set_pipeline(&self.pipeline);
        // Under the GUI: the selection box, in screen px.
        if let Some(edges) = &self.selection_box {
            for (l, &(x, y, w, h)) in self.edges.iter().zip(edges) {
                let u: Vec<u8> = [w as f32 * 2.0 / sw, h as f32 * 2.0 / sh, x as f32 * 2.0 / sw, y as f32 * 2.0 / sh].iter().flat_map(|v| v.to_le_bytes()).collect();
                queue.write_buffer(&l.uniforms, 0, &u);
                pass.set_bind_group(0, &l.bind, &[]);
                pass.draw(0..4, 0..1);
            }
        }
        let marker = self.marker.as_ref().zip(self.marker_at);
        for (l, pos) in marker.into_iter().chain(self.minimap.iter().chain([&self.panel]).map(|l| (l, l.pos))) {
            let u: Vec<u8> = [
                l.size.0 as f32 * scale * 2.0 / sw,
                l.size.1 as f32 * scale * 2.0 / sh,
                (pos.0 as f32 * scale + origin.0) * 2.0 / sw,
                (pos.1 as f32 * scale + origin.1) * 2.0 / sh,
            ]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
            queue.write_buffer(&l.uniforms, 0, &u);
            pass.set_bind_group(0, &l.bind, &[]);
            pass.draw(0..4, 0..1);
        }
    }
}

const SHADER: &str = r#"
struct U { size: vec2<f32>, pos: vec2<f32> };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var tex: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

struct Out { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };

@vertex
fn vs(@builtin(vertex_index) vi: u32) -> Out {
    let c = vec2<f32>(f32(vi & 1u), f32(vi >> 1u));
    var o: Out;
    o.pos = vec4<f32>(-1.0 + u.pos.x + c.x * u.size.x, 1.0 - u.pos.y - c.y * u.size.y, 0.0, 1.0);
    o.uv = c;
    return o;
}

@fragment
fn fs(i: Out) -> @location(0) vec4<f32> {
    return textureSample(tex, samp, i.uv);
}
"#;
