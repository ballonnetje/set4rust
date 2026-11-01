//! The in-game load / save submenu (dialog 8): tab 928 "Save" (string 2204) → page 33, 929
//! "Briefing" → page 35, 930 "Quit" → page 34.
//!
//! Verified in the shipped exe: the page-33 init (release 0x47ac70: 927 hidden, 914 text cleared,
//! 912 disabled, 913 hidden, top row 0, 914's edit properties 0x0410 = 16 characters, the rows from
//! the table at file offset 0xbfc780 = 915..924) and page 34 (init 0x49d16e: SetText(34, 1070,
//! 1983); the proc sends 0x17 for 1071 and 0x25B for 1072). The rest follows the GUI set's layout
//! and tooltips (912 "Save Game" 1847, 913 "Delete saved game" 1843, 925 / 926 scroll).
//!
//! Additions of ours (UI conveniences, not mechanics): each row shows the save's date; saving
//! over an existing name, deleting (913, enabled for a selected row) and loading (a second click
//! on the selected row) ask for confirmation on page 34's yes / no layout.
//!
//! The saves are ours (save.rs), in `%APPDATA%\s4rs\saves`.

use std::path::PathBuf;

use s4_viewer::gpu::Gpu;
use s4_viewer::guistate::UiEvent;

pub const SAVE_PAGE: usize = 33;
pub const QUIT_PAGE: usize = 34;
const ROWS: u16 = 10;

/// What the page asks of the game.
pub enum MenuAction {
    /// Save under this name.
    Save(String),
    /// End this game and continue the saved one.
    Load(PathBuf),
    /// End the game (page 34, yes).
    Quit,
}

/// A question asked on page 34 instead of "end the game?".
enum Confirm {
    Overwrite(String),
    Delete(PathBuf),
    Load(PathBuf),
    /// Only shown: a save this build cannot load (either answer returns to the save page).
    Refused,
}

/// The save page's state.
#[derive(Default)]
pub struct SaveMenu {
    /// Saved games, newest first.
    pub saves: Vec<(PathBuf, crate::save::Header)>,
    top: usize,
    selected: Option<usize>,
    confirm: Option<Confirm>,
    /// Sounds the page played (the wheel's scroll), for the game's sound to take.
    pub sounds: Vec<u16>,
}

/// Every savegame in the save folder, newest first.
pub fn list_saves() -> Vec<(PathBuf, crate::save::Header)> {
    let Some(dir) = crate::save::dir() else { return Vec::new() };
    list_saves_in(&dir)
}

/// Every savegame in `dir`, newest first (unreadable files left out; old versions listed).
pub fn list_saves_in(dir: &std::path::Path) -> Vec<(PathBuf, crate::save::Header)> {
    let mut v: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "s4rs"))
        .filter_map(|p| crate::save::read_header(&p).ok().map(|h| (p, h)))
        .collect();
    v.sort_by_key(|s| std::cmp::Reverse(s.1.saved_at));
    v
}

fn set_text(gpu: &mut Gpu, d: usize, l: u16, t: &str) {
    if let Some(ui) = gpu.ui.as_mut() {
        ui.state.texts.insert((d, l), t.to_string());
    }
}

fn set_effect(gpu: &mut Gpu, d: usize, l: u16, bit: u8, on: bool) {
    let Some(ui) = gpu.ui.as_mut() else { return };
    let Some(i) = ui.state.set.dialog(d).and_then(|x| x.controls.iter().position(|c| c.link == l)) else { return };
    if let Some(e) = ui.state.effects.get_mut(&(d, i)) {
        *e = if on { *e | bit } else { *e & !bit };
    }
}

fn visible(gpu: &mut Gpu, d: usize, l: u16, v: bool) {
    set_effect(gpu, d, l, 8, !v);
}

fn enabled(gpu: &mut Gpu, d: usize, l: u16, v: bool) {
    set_effect(gpu, d, l, 4, !v);
}

/// Swap the page in the page slot (33 and 34 share a surface).
fn show(gpu: &mut Gpu, page: usize) {
    if let Some(ui) = gpu.ui.as_mut() {
        let mut ev = Vec::new();
        for p in [SAVE_PAGE, QUIT_PAGE] {
            ui.state.close(p, &mut ev);
        }
        ui.state.open(page, &mut ev);
    }
}

/// `dd.mm. hh:mm` (UTC) of a save time.
fn short_date(secs: u64) -> String {
    let d = crate::frontend::date(secs);
    // "YYYY-MM-DD HH:MM" → "DD.MM. HH:MM"
    format!("{}.{}. {}", &d[8..10], &d[5..7], &d[11..16])
}

impl SaveMenu {
    fn name(gpu: &Gpu) -> String {
        gpu.ui.as_ref().and_then(|u| u.state.texts.get(&(SAVE_PAGE, 914)).cloned()).unwrap_or_default()
    }

    /// `InitGuiDlgLoadSave`.
    pub fn init(&mut self, gpu: &mut Gpu) {
        visible(gpu, SAVE_PAGE, 927, false);
        set_text(gpu, SAVE_PAGE, 914, "");
        enabled(gpu, SAVE_PAGE, 912, false);
        visible(gpu, SAVE_PAGE, 913, true);
        enabled(gpu, SAVE_PAGE, 913, false);
        self.top = 0;
        self.selected = None;
        self.saves = list_saves();
        self.fill(gpu);
    }

    /// `GuiDlgLoadSaveFillList`, rows with the date added.
    fn fill(&self, gpu: &mut Gpu) {
        let more = self.saves.len() > ROWS as usize;
        enabled(gpu, SAVE_PAGE, 925, more);
        enabled(gpu, SAVE_PAGE, 926, more);
        for i in 0..ROWS {
            let l = 915 + i;
            let k = self.top + i as usize;
            match self.saves.get(k) {
                Some((_, h)) => {
                    // Ours: a save this build cannot load says why (save.rs policy).
                    let text = match crate::save::compatibility(h, crate::save::current_rules()) {
                        Ok(()) => format!("{}  {}", h.name, short_date(h.saved_at)),
                        Err(why) => format!("{}  ({why})", h.name),
                    };
                    set_text(gpu, SAVE_PAGE, l, &text);
                    visible(gpu, SAVE_PAGE, l, true);
                    // Never pressed: a pressed button takes no click (guistate `left_down`), so the
                    // second click that loads would be lost. The name field 914 shows the
                    // selected save, as in the release.
                    set_effect(gpu, SAVE_PAGE, l, 1, false);
                }
                None => visible(gpu, SAVE_PAGE, l, false),
            }
        }
        enabled(gpu, SAVE_PAGE, 913, self.selected.is_some());
    }

    fn ask(&mut self, gpu: &mut Gpu, c: Confirm) {
        let name = |p: &PathBuf| self.saves.iter().find(|s| &s.0 == p).map_or(String::new(), |s| s.1.name.clone());
        let text = match &c {
            Confirm::Overwrite(n) => format!("Overwrite the saved game \"{n}\"?"),
            Confirm::Delete(p) => format!("Delete the saved game \"{}\"?", name(p)),
            Confirm::Load(p) => format!("Load the saved game \"{}\"? This game ends.", name(p)),
            Confirm::Refused => String::new(),
        };
        self.confirm = Some(c);
        show(gpu, QUIT_PAGE);
        set_text(gpu, QUIT_PAGE, 1070, &text);
    }

    fn save_or_ask(&mut self, gpu: &mut Gpu) -> Option<MenuAction> {
        let name = Self::name(gpu).trim().to_string();
        if name.is_empty() {
            return None;
        }
        if self.saves.iter().any(|s| s.1.name == name) {
            self.ask(gpu, Confirm::Overwrite(name));
            None
        } else {
            Some(MenuAction::Save(name))
        }
    }

    /// The pages' GUI events. Returns what the game has to do.
    pub fn events(&mut self, gpu: &mut Gpu, events: &[UiEvent]) -> Option<MenuAction> {
        let mut out = None;
        for e in events {
            match *e {
                UiEvent::DialogOpened(SAVE_PAGE) => self.init(gpu),
                UiEvent::DialogOpened(QUIT_PAGE) if self.confirm.is_none() => {
                    let t = gpu.ui.as_ref().map(|u| u.string(1983)).unwrap_or_default();
                    set_text(gpu, QUIT_PAGE, 1070, &t);
                }
                UiEvent::Command { dialog: SAVE_PAGE, link, .. } => match link {
                    912 => out = self.save_or_ask(gpu),
                    913 => {
                        if let Some(p) = self.selected.and_then(|k| self.saves.get(k)).map(|s| s.0.clone()) {
                            self.ask(gpu, Confirm::Delete(p));
                        }
                    }
                    915..=924 => {
                        let k = self.top + (link - 915) as usize;
                        if let Some((p, h)) = self.saves.get(k).cloned() {
                            if self.selected == Some(k) {
                                // A second click on the selected row: load it (ours), unless
                                // this build cannot.
                                if let Err(why) = crate::save::compatibility(&h, crate::save::current_rules()) {
                                    show(gpu, QUIT_PAGE);
                                    set_text(gpu, QUIT_PAGE, 1070, &format!("\"{}\" cannot be loaded: made with {why}.", h.name));
                                    self.confirm = Some(Confirm::Refused);
                                } else {
                                    self.ask(gpu, Confirm::Load(p));
                                }
                            } else {
                                // GuiDlgLoadSaveNameSelected: its name into 914.
                                self.selected = Some(k);
                                set_text(gpu, SAVE_PAGE, 914, &h.name);
                                enabled(gpu, SAVE_PAGE, 912, true);
                                self.fill(gpu);
                            }
                        }
                    }
                    925 if self.top > 0 => {
                        self.top -= 1;
                        self.fill(gpu);
                    }
                    926 if self.top + (ROWS as usize) < self.saves.len() => {
                        self.top += 1;
                        self.fill(gpu);
                    }
                    _ => {}
                },
                // Code 10 (0x47af10): the wheel scrolls as 925 / 926, with sound 11 when it moves.
                UiEvent::Wheel { dialog: SAVE_PAGE, delta } => {
                    let moved = if delta > 0 && self.top > 0 {
                        self.top -= 1;
                        true
                    } else if delta < 0 && self.top + (ROWS as usize) < self.saves.len() {
                        self.top += 1;
                        true
                    } else {
                        false
                    };
                    if moved {
                        self.sounds.push(11);
                        self.fill(gpu);
                    }
                }
                UiEvent::EditDone { dialog: SAVE_PAGE, link: 914 } => out = self.save_or_ask(gpu),
                UiEvent::Command { dialog: QUIT_PAGE, link: 1071, .. } => match self.confirm.take() {
                    None => out = Some(MenuAction::Quit),
                    Some(Confirm::Overwrite(n)) => {
                        show(gpu, SAVE_PAGE);
                        self.init(gpu);
                        out = Some(MenuAction::Save(n));
                    }
                    Some(Confirm::Delete(p)) => {
                        if let Err(e) = std::fs::remove_file(&p) {
                            tracing::warn!("delete {}: {e}", p.display());
                        }
                        show(gpu, SAVE_PAGE);
                        self.init(gpu);
                    }
                    Some(Confirm::Load(p)) => out = Some(MenuAction::Load(p)),
                    Some(Confirm::Refused) => {
                        show(gpu, SAVE_PAGE);
                        self.init(gpu);
                    }
                },
                UiEvent::Command { dialog: QUIT_PAGE, link: 1072, .. } => match self.confirm.take() {
                    None => {
                        if let Some(ui) = gpu.ui.as_mut() {
                            ui.state.reset_main_menu(&mut Vec::new());
                        }
                    }
                    Some(_) => {
                        show(gpu, SAVE_PAGE);
                        self.init(gpu);
                    }
                },
                _ => {}
            }
        }
        // Code 2 on 914: the save button follows the name.
        if gpu.ui.as_ref().is_some_and(|u| u.state.open.contains(&SAVE_PAGE)) {
            let has = !Self::name(gpu).trim().is_empty();
            enabled(gpu, SAVE_PAGE, 912, has);
        }
        out
    }
}

/// A file name for a save name (letters, digits, `-`, `_`, space kept).
pub fn file_for(name: &str) -> Option<PathBuf> {
    let stem: String = name.chars().map(|c| if c.is_alphanumeric() || matches!(c, '-' | '_' | ' ') { c } else { '_' }).collect();
    crate::save::dir().map(|d| d.join(format!("{}.s4rs", stem.trim())))
}
