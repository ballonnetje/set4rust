//! What follows a campaign mission's statistics (`EndGameGoToStatistics` 0x514450, after it
//! queued `CStateEndStatistic` / `CStateEcoStatistic`), and the victory screen, dialog 12
//! (`CStateVictoryScreen`: factory 0x528050, ctor 0x5280a0, procedure 0x490720, init 0x4906c0,
//! event handler 0x528180).
//!
//! The release queues, for `GlobalGameState +0x2d4` = campaign type c and +0x2d8 = level l, with
//! "won" = the winning team (`CStateGame +0x68`) is the local player's (0x12dcd0c):
//! * the Dark Tribe's last mission (c 4, l 11) won: video 6 (`CStateVideo` 0x528360, when
//!   0x11b58e8 = 1, which is 1 from the start), then the main menu;
//! * the three races' last missions (c 1..=3, l 2) won: the victory screen of race c, then the
//!   main menu;
//! * c 12, l 1 won: the Add-On splash (`CStateAOSplash` 0x5101c0); c 15 won: videos 9 (l 4) / 10
//!   (l 11) and the splash at levels 1, 3, 6, 8, 9; then their campaign screen;
//! * else the main menu with the submenu of c (`OpenMainMenu` 0x522540 codes: c 1..=3 → 2 the
//!   three races, 4 → 1 the Dark Tribe, 5 / 6 / 7 / 8, 9 → 6 / 8 / 7 / 9, 11 / 12 / 13 / 14 / 15 →
//!   0xb / 0xd / 0xc / 0xf / 0xe, 17..=20 → 0x10, 21..=24 → 0x11; 10 and 16 → 0, the menu).
//!
//! The videos are left out (the user's decision): `CStateVideo` is a skipped step, the queue goes
//! on as if it had ended. The Add-On splash is splash.rs.

use s4_viewer::guistate::GuiState;

pub const DIALOG: usize = 12;
/// The picture control the init 0x4906c0 sets (`SetImages(12, 0x9ff, image, 0)`).
pub const PICTURE: u16 = 0x9ff;

/// How a campaign mission ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub campaign: u8,
    pub level: u32,
    pub won: bool,
}

/// The screen after the statistics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum After {
    /// Dialog 12 for race 1..=3, then the main menu.
    Victory(u8),
    /// The main menu (code 0).
    Menu,
    /// The campaign's selection screen.
    Campaign(u8),
    /// The Add-On splash (splash.rs) with 0x1294830 = the second value, then the campaign's
    /// screen.
    Splash(u8, i32),
}

pub fn after(o: Outcome) -> After {
    match (o.campaign, o.level, o.won) {
        (4, 11, true) => After::Menu,
        (c @ 1..=3, 2, true) => After::Victory(c),
        (12, 1, true) => After::Splash(12, -1),
        (15, l @ (1 | 3 | 6 | 8 | 9), true) => After::Splash(15, l as i32),
        (10 | 16, _, _) => After::Menu,
        (c, _, _) => After::Campaign(c),
    }
}

/// The video `EndGameGoToStatistics` 0x514450 queues after a won campaign mission's statistics,
/// before [`after`]'s state: Extro (6) after the Dark Tribe's last (0x5145c7), TroScen5 (9) /
/// TroExtro (10) after the Trojans' 5th / 12th (0x514643 / 0x514668).
pub fn video(o: Outcome) -> Option<u8> {
    match (o.campaign, o.level, o.won) {
        (4, 11, true) => Some(6),
        (15, 4, true) => Some(9),
        (15, 11, true) => Some(10),
        _ => None,
    }
}

/// The init 0x4906c0: the picture of race `campaign` (0x12987f4, the factory's argument): images
/// 254 / 256 / 255 for 1 / 2 / 3.
pub fn init(st: &mut GuiState, campaign: u8) {
    let image = match campaign {
        1 => 0xfe,
        2 => 0x100,
        3 => 0xff,
        _ => return,
    };
    st.images.insert((DIALOG, PICTURE), (image, 0));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes() {
        let o = |campaign, level, won| after(Outcome { campaign, level, won });
        assert_eq!(o(2, 2, true), After::Victory(2));
        assert_eq!(o(2, 2, false), After::Campaign(2));
        assert_eq!(o(2, 1, true), After::Campaign(2));
        assert_eq!(o(4, 11, true), After::Menu);
        assert_eq!(o(4, 10, true), After::Campaign(4));
        assert_eq!(o(10, 0, true), After::Menu);
        assert_eq!(o(5, 4, true), After::Campaign(5));
        assert_eq!(o(12, 1, true), After::Splash(12, -1));
        assert_eq!(o(15, 6, true), After::Splash(15, 6));
        assert_eq!(o(15, 4, true), After::Campaign(15));
        assert_eq!(o(15, 6, false), After::Campaign(15));
        let v = |campaign, level, won| video(Outcome { campaign, level, won });
        assert_eq!((v(4, 11, true), v(4, 11, false), v(4, 10, true)), (Some(6), None, None));
        assert_eq!((v(15, 4, true), v(15, 11, true), v(15, 6, true), v(12, 1, true)), (Some(9), Some(10), None, None));
    }
}
