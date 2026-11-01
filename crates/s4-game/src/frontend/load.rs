//! The load type chooser, dialog 5 (`CStateLoadType`: factory 0x51af20, event handler 0x51b060,
//! procedure 0x48cee0, init 0x48ce50), and the save filter of the load list, dialog 6
//! (`CStateLoadGame` 0x51a2e0, the list 0x51a5c0 with the filter 0x51abb0).
//!
//! The main menu's event 0x2e (load game, 0x522da6) enters `CStateLoadType`. Its buttons send
//! 0x5a / 0x5b / 0x5c, which enter `CStateLoadGame` (0x51a250) with type 1 / 2 / 3; the type
//! sets the game kind the list keeps (`+4`: 3, 1, 2; `+8`: 1, 0, 0, which nothing in the list
//! reads). Event 0x59 (type 0: kind 3, `+8` = 4): no `CreateEvent(0x59, ..)` in the exe (gui_calls.py),
//! [inf] unused.
//! Back (0x955) and Esc (event 0xb, key 0x1b) send 0x5d: the main menu (0x5224f0). The load
//! list's back (0x923) and Esc send 0x5e: this dialog again (0x51a4e3).

use crate::save::Header;
use s4_viewer::guistate::GuiState;

pub const DIALOG: usize = 5;
pub const TITLE: u16 = 0x954;
pub const BACK: u16 = 0x955;
/// Event 0x5b: the single player saves (kind 1).
pub const LOCAL: u16 = 0x956;
/// Event 0x5c: the network saves (kind 2).
pub const NETWORK: u16 = 0x957;
/// Event 0x5a: the campaign saves (kind 3).
pub const CAMPAIGN: u16 = 0x95f;

/// The game kind (`GlobalGameState +0x2a8`) the list keeps after `link` of dialog 5.
pub fn kind_of_button(link: u16) -> Option<u32> {
    match link {
        LOCAL => Some(1),
        NETWORK => Some(2),
        CAMPAIGN => Some(3),
        _ => None,
    }
}

/// The init 0x48ce50: the title (string 0x4d0), the network button enabled unless
/// `UPlayAdapter +0x98` (vtable +0x20, 0x60f610) is set, and that button's tooltip 0x4ea (the
/// control record's +0x14, replacing the set's 2292). [inf] `+0x98` is the Ubisoft offline
/// state; our network games are LAN games, so the button stays enabled.
pub fn init(st: &mut GuiState, title: String) {
    st.texts.insert((DIALOG, TITLE), title);
    st.tooltips.insert((DIALOG, NETWORK), (0x4ea, 0));
}

/// The kind of game a save was made in. The release keeps `GlobalGameState +0x2a8` in the save's
/// general chunk (+0x10: 2 in the 121 network autosaves of the 123 saves, 3 in the two roman02
/// saves) and the list filter 0x51abb0 compares it. Our header has no such field and save.rs
/// VERSION stays, so [inf] it is derived from the header: a network save is named
/// `<name> - player n` (sim.rs `net_save`), a campaign mission's map lies in `Map\Campaign`, a
/// tutorial's in `Map\Tutorial` (kind 4, which no load type lists); else a single player game.
pub fn kind_of(h: &Header) -> u32 {
    if h.name.rsplit_once(" - player ").is_some_and(|(_, n)| n.parse::<u8>().is_ok()) {
        return 2;
    }
    let dir = std::path::Path::new(&h.map.replace('\\', "/")).parent().and_then(|d| d.file_name()).map(|d| d.to_string_lossy().to_ascii_lowercase());
    match dir.as_deref() {
        Some("campaign") => 3,
        Some("tutorial") => 4,
        _ => 1,
    }
}

/// The filter 0x51abb0 for load type kind `want`: kinds 1, 2 (and 4) keep saves of that kind
/// whose general chunk +8 is 0; kind 3 keeps campaign saves, those with +8 in 5..=10 only when
/// `GlobalMissionCD` (vtable +4) says so. [inf] +8 is the campaign (roman02's saves: 1), 0 in
/// other games, and the mission CD's campaigns are installed when their maps are, so here the
/// kind alone decides.
pub fn keeps(h: &Header, want: u32) -> bool {
    kind_of(h) == want
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(name: &str, map: &str) -> Header {
        Header { magic: crate::save::MAGIC.into(), version: crate::save::VERSION, name: name.into(), saved_at: 0, map: map.into(), map_hash: 0, tick: 0, rules: 0, thumb: Vec::new(), world_hash: 0 }
    }

    #[test]
    fn kinds_of_saves() {
        assert_eq!(kind_of(&header("mp - player 1", r"C:\S4\Map\Multiplayer\x.map")), 2);
        assert_eq!(kind_of(&header("mp - player two", r"C:\S4\Map\Multiplayer\x.map")), 1);
        assert_eq!(kind_of(&header("a", r"C:\S4\Map\Campaign\roman02.map")), 3);
        assert_eq!(kind_of(&header("a", "Campaign/MCD2_maya1.map")), 3);
        assert_eq!(kind_of(&header("a", r"C:\S4\Map\Tutorial\Tutorial01.map")), 4);
        assert_eq!(kind_of(&header("a", r"C:\Campaign\S4\Map\User\x.map")), 1);
        assert!(keeps(&header("a", "Campaign/roman02.map"), 3) && !keeps(&header("a", "Campaign/roman02.map"), 1));
        assert_eq!([LOCAL, NETWORK, CAMPAIGN, BACK].map(kind_of_button), [Some(1), Some(2), Some(3), None]);
    }
}
