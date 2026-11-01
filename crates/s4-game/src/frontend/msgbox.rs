//! The start screens' message box, dialog 20 (procedure 0x48f1d0, init 0x48f170): OK (0x9ce,
//! string 0x90d), the title 0x9cf (string 0x94e), the text 0x9d7 from the string 0x146c750 that
//! the opener filled. OK sends event 0x72; the state that opened the box handles it by opening
//! its own screen again (`CStateMainMenu` 0x522b70: the main menu; `CStateTutorial` 0x527c10:
//! dialog 0xb; `CStateBriefing` 0x510970: dialog 10).
//!
//! Openers ported: the tutorial's missing map (string 0x966), the campaign briefing's missing map
//! (0x95a), the main menu's manual (event 0x32: `ShowHtml` 0x5053a0 failing, 0x952 / 0x956 /
//! 0x958), and ours: the internet game list's unreachable server (net.rs). [not ported] The
//! Ubisoft network checks of event 0x2c (strings at 0x1057188...; our multiplayer has no Ubisoft
//! login) and the add-on briefings' missing map (0xcdb), which switch the GUI set first.

use s4_viewer::guistate::GuiState;

pub const DIALOG: usize = 20;
pub const OK: u16 = 0x9ce;
pub const TITLE: u16 = 0x9cf;
pub const TEXT: u16 = 0x9d7;

/// The init 0x48f170 with `strings` (the text table) and the box's text.
pub fn init(st: &mut GuiState, strings: &dyn Fn(u32) -> String, text: String) {
    st.texts.insert((DIALOG, OK), strings(0x90d));
    st.texts.insert((DIALOG, TITLE), strings(0x94e));
    st.texts.insert((DIALOG, TEXT), text);
}

/// `ShowHtml` 0x5053a0 (`ShellExecuteA(hWnd, "open", path)` relative to the game folder): `Ok`, or
/// the string id of the message box text: 0x958 "file not found" (ShellExecute 2 / 3), 0x952
/// (0x1f, no program for the file type), 0x956 (any other failure). [inf] Ours checks the file
/// first and reports a failed start of the system's opener as 0x952.
pub fn show_html(path: &std::path::Path) -> Result<(), u32> {
    if !path.exists() {
        return Err(0x958);
    }
    if cfg!(test) {
        return Ok(());
    }
    #[cfg(windows)]
    let opened = std::process::Command::new("cmd").args(["/C", "start", ""]).arg(path).spawn();
    #[cfg(target_os = "macos")]
    let opened = std::process::Command::new("open").arg(path).spawn();
    #[cfg(not(any(windows, target_os = "macos")))]
    let opened = std::process::Command::new("xdg-open").arg(path).spawn();
    opened.map(|_| ()).map_err(|_| 0x952)
}

/// The manual (event 0x32, 0x502470) or the tips (0x33, 0x5024d0) of text language `language`:
/// `MiscData1.cfg` `MISCDATA1` `ManualURL_DE` / `_FR` / `_EN` (`TipsURL_...`), read at the start
/// (0x500910); German for language 1, French for 2, else English.
pub fn manual(game: &std::path::Path, language: u32, tips: bool) -> Option<std::path::PathBuf> {
    let suffix = match language {
        1 => "DE",
        2 => "FR",
        _ => "EN",
    };
    let key = format!("{}URL_{suffix}", if tips { "Tips" } else { "Manual" });
    let cfg = std::fs::read(s4_formats::path::resolve(game.join("Config").join("MiscData1.cfg"))).ok()?;
    let cfg = String::from_utf8_lossy(&cfg);
    let value = cfg.lines().find_map(|l| {
        let (k, v) = l.split_once('=')?;
        (k.trim().eq_ignore_ascii_case(&key)).then(|| v.trim().trim_matches('"').to_string())
    })?;
    Some(s4_formats::path::resolve(game.join(value.replace('\\', std::path::MAIN_SEPARATOR_STR))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_paths() {
        let game = s4_formats::game_dir();
        if !game.join("Config").join("MiscData1.cfg").exists() {
            return;
        }
        let en = manual(&game, 0, false).unwrap();
        assert!(en.ends_with("Handbook/english/The Settlers IV Gold Manual.pdf"), "{}", en.display());
        assert!(manual(&game, 1, false).unwrap().ends_with("Handbuch/Die Siedler IV Gold Handbuch.pdf"));
        assert_eq!(show_html(&game.join("no such manual.pdf")), Err(0x958));
    }
}
