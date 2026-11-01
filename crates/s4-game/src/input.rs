//! `CInputProcessor`'s state machine: which UI events it acts on in which state. The release
//! registers 148 transitions (0x4e6bc0: state, next state, event, handler index into the table
//! 0x4e96d0 fills; `tools/re/gui-2026-10-21/trans_extract.py`), and the machine (0x631100) looks
//! (state, event) up: found → the handler runs and the state becomes the next one; not found →
//! the default handler 0x4e9bf0 (a debug print, nothing else) and the state stays. Handlers post
//! further events themselves (e.g. a click that selects nothing posts 0x191).
//!
//! The states this client has: 0 nothing selected, 1 a selection (0x1dd / 0x1de / 0x198 / 0x1b9
//! …), 3 the work-area mode (0x193), 4 the build mode (0x1b5, `TryBuild` 0x4ea250: +0x50 = 4),
//! 7 the trade-target mode (0x199), 9 the soldier dialog's target mode (0x19e, bdlg.rs), and on
//! top of these (`BuildingDialog::state_of`) 8 the spell mode (0x1c1 / 0x1da) and 10 the camera
//! window's pick mode (0x1e7).
//! Here the state is read off the client's own modes ([`state`]) rather than kept: the modes
//! change at the same events.
//!
//! Handler 1 (0x4edb10) is the "leave" handler: `sub_EDC10` (mode +0x50 = 0, the cursor shape,
//! the build markers off `sub_25A5D0(0)`, the selected building's dialog refilled), then GUI
//! events 0x260 (the sidebar closed) and 0x259. In state 3 most dialog buttons reach it instead of
//! their own handler: they only leave the work-area mode.

/// (state, next state, event, handler index), in the release's registration order.
pub const TRANSITIONS: [(u8, u8, u16, u8); 148] = [
    (0, 0, 0x191, 0x01), (0, 0, 0x1e0, 0x00), (0, 0, 0x192, 0x11), (0, 1, 0x1dd, 0x1c),
    (0, 1, 0x1de, 0x1b), (0, 1, 0x1aa, 0x1d), (0, 1, 0x198, 0x0c), (0, 1, 0x1e6, 0x0d),
    (0, 1, 0x1ab, 0x0e), (0, 1, 0x1e4, 0x03), (0, 0, 0x1e2, 0x14), (0, 0, 0x1b4, 0x38),
    (0, 0, 0x1ac, 0x10), (0, 4, 0x1b5, 0x04), (0, 0, 0x1b6, 0x0f), (0, 0, 0x1ad, 0x2c),
    (0, 0, 0x1ae, 0x2d), (0, 0, 0x1af, 0x2e), (0, 0, 0x1e5, 0x2f), (0, 1, 0x1b9, 0x30),
    (1, 1, 0x1b9, 0x30), (0, 0, 0x1ba, 0x31), (0, 0, 0x1e1, 0x4b), (0, 1, 0x1d8, 0x1e),
    (0, 1, 0x1d9, 0x21), (0, 0, 0x1b0, 0x32), (0, 0, 0x1b1, 0x33), (0, 0, 0x1b2, 0x34),
    (0, 0, 0x1bd, 0x36), (0, 0, 0x1be, 0x35), (0, 0, 0x1b3, 0x37), (0, 0, 0x1ca, 0x41),
    (0, 0, 0x1b8, 0x4a), (0, 10, 0x1e7, 0x53), (1, 0, 0x191, 0x01), (1, 1, 0x1dd, 0x1c),
    (1, 1, 0x1de, 0x1b), (1, 1, 0x1aa, 0x1d), (1, 1, 0x1e1, 0x4b), (1, 1, 0x1e0, 0x22),
    (1, 1, 0x19f, 0x28), (1, 1, 0x1a7, 0x29), (1, 1, 0x1d6, 0x20), (1, 1, 0x1d8, 0x1e),
    (1, 1, 0x1d9, 0x21), (1, 1, 0x1d7, 0x1f), (1, 1, 0x19b, 0x23), (1, 1, 0x19c, 0x24),
    (1, 9, 0x19e, 0x25), (9, 1, 0x191, 0x01), (9, 1, 0x1e0, 0x01), (9, 0, 0x192, 0x11),
    (9, 9, 0x1dd, 0x26), (1, 1, 0x1a6, 0x27), (1, 1, 0x1b3, 0x37), (1, 1, 0x1c0, 0x51),
    (1, 0, 0x192, 0x11), (1, 4, 0x1b5, 0x04), (1, 3, 0x193, 0x06), (1, 1, 0x194, 0x08),
    (1, 1, 0x195, 0x09), (1, 1, 0x196, 0x0a), (1, 1, 0x197, 0x0b), (1, 1, 0x198, 0x0c),
    (1, 1, 0x1e6, 0x0d), (1, 1, 0x1ab, 0x0e), (1, 1, 0x1cd, 0x44), (1, 1, 0x1ce, 0x45),
    (1, 1, 0x1cf, 0x46), (1, 1, 0x1d0, 0x47), (1, 1, 0x1d4, 0x48), (1, 1, 0x1d5, 0x49),
    (1, 1, 0x1bf, 0x52), (1, 7, 0x199, 0x15), (1, 1, 0x1d1, 0x17), (1, 1, 0x1d2, 0x18),
    (1, 1, 0x1d3, 0x19), (1, 1, 0x1e3, 0x1a), (1, 1, 0x1bb, 0x12), (1, 1, 0x1bc, 0x13),
    (1, 8, 0x1c1, 0x39), (1, 1, 0x1c2, 0x3a), (1, 1, 0x1c3, 0x3b), (1, 1, 0x1c4, 0x3c),
    (1, 1, 0x1c5, 0x3d), (1, 1, 0x1c7, 0x3e), (1, 1, 0x1c8, 0x3f), (1, 1, 0x1c9, 0x40),
    (1, 1, 0x1cb, 0x42), (1, 1, 0x1cc, 0x43), (1, 1, 0x1a3, 0x4c), (1, 1, 0x1a5, 0x4d),
    (1, 1, 0x1a4, 0x4e), (1, 1, 0x1a1, 0x4f), (1, 1, 0x1a2, 0x50), (1, 10, 0x1e7, 0x53),
    (4, 0, 0x191, 0x01), (4, 0, 0x1e0, 0x01), (4, 0, 0x192, 0x11), (4, 0, 0x1ac, 0x10),
    (4, 1, 0x1de, 0x1b), (4, 4, 0x1b5, 0x04), (4, 4, 0x1dd, 0x05), (4, 4, 0x1b6, 0x0f),
    (4, 0, 0x1ca, 0x41), (4, 10, 0x1e7, 0x53), (3, 1, 0x191, 0x01), (3, 1, 0x1e0, 0x01),
    (3, 0, 0x192, 0x11), (3, 1, 0x1de, 0x1b), (3, 3, 0x1dd, 0x07), (3, 1, 0x198, 0x01),
    (3, 1, 0x194, 0x01), (3, 1, 0x193, 0x01), (3, 1, 0x196, 0x01), (3, 1, 0x1c1, 0x01),
    (3, 1, 0x1c2, 0x01), (3, 1, 0x1c3, 0x01), (3, 1, 0x1c4, 0x01), (3, 1, 0x1c5, 0x01),
    (3, 1, 0x1c7, 0x01), (3, 1, 0x1c8, 0x01), (3, 1, 0x1c9, 0x01), (3, 1, 0x1cb, 0x01),
    (3, 10, 0x1e7, 0x53), (7, 1, 0x191, 0x01), (7, 1, 0x1e0, 0x01), (7, 0, 0x192, 0x11),
    (7, 7, 0x1dd, 0x16), (7, 10, 0x1e7, 0x53), (1, 8, 0x1da, 0x2a), (8, 8, 0x1da, 0x2a),
    (8, 8, 0x1e0, 0x2b), (8, 0, 0x192, 0x11), (8, 1, 0x1dd, 0x1c), (8, 0, 0x191, 0x01),
    (8, 1, 0x1cc, 0x01), (8, 1, 0x1de, 0x1b), (8, 8, 0x1d6, 0x20), (8, 8, 0x1d8, 0x1e),
    (8, 8, 0x1d9, 0x21), (8, 1, 0x19b, 0x23), (8, 1, 0x19c, 0x24), (8, 9, 0x19e, 0x25),
    (8, 1, 0x19f, 0x28), (8, 10, 0x1e7, 0x53), (10, 1, 0x1dd, 0x54), (10, 1, 0x1e0, 0x01),
];

/// The handler index of the "leave" handler (0x4edb10).
pub const LEAVE: u8 = 1;

/// (next state, handler index) for `event` in `state`; `None`: no transition (dropped).
pub fn transition(state: u8, event: u16) -> Option<(u8, u8)> {
    TRANSITIONS.iter().find(|t| t.0 == state && t.2 == event).map(|t| (t.1, t.3))
}

/// The state the client's modes stand for.
pub fn state(build_mode: bool, work_area: bool, trade_target: bool, target: bool, selected: bool) -> u8 {
    if build_mode {
        4
    } else if work_area {
        3
    } else if trade_target {
        7
    } else if target {
        9
    } else if selected {
        1
    } else {
        0
    }
}

/// The controls whose procedures send UI event 0x192 (handler 0x4edac0 in every state: the
/// selection emptied, the sidebar provider cleared, 0x4edb10, state 0) before their own action:
/// the main tabs (`UiMainSidebarSurfaceCallbacks` 0x49b6b0, 606..=611) and the submenus' tabs:
/// build 0x472750 (536..=540), settlers 0x475150 (745..=748), production 0x49ce30
/// (1067..=1069), statistics 0x4aa5e0 (1560..=1562), settings 0x49c040 (1021..=1024), load / save
/// 0x47b4e0 (928..=931).
pub fn sends_192(dialog: usize, link: u16) -> bool {
    matches!((dialog, link), (0, 606..=611) | (3, 536..=540) | (4, 745..=748) | (5, 1067..=1069) | (6, 1560..=1562) | (7, 1021..=1024) | (8, 928..=931))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table() {
        // The free carrier buttons (0x1ba) only without a selection; the specialists (0x1e1) also
        // with one; a dialog button in the work-area mode leaves it.
        assert_eq!(transition(0, 0x1ba), Some((0, 0x31)));
        assert_eq!(transition(1, 0x1ba), None);
        assert_eq!(transition(1, 0x1e1), Some((1, 0x4b)));
        assert_eq!(transition(3, 0x198), Some((1, LEAVE)));
        assert_eq!(transition(7, 0x198), None);
        assert_eq!(transition(4, 0x192), Some((0, 0x11)));
        // Every (state, event) once.
        for (i, a) in TRANSITIONS.iter().enumerate() {
            assert!(TRANSITIONS[..i].iter().all(|b| (b.0, b.2) != (a.0, a.2)));
        }
    }
}
