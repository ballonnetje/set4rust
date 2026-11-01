//! "Go to a building of a type": the build pages' right click, Ctrl + right click and the
//! building dialog's "next" button, as the shipped exe runs them (own RE;
//! `tools/re/gui-2026-10-22/{goto.c,goto2.c,goto3.c}`).
//!
//! * The build pages' procedures (`BaseBuildingTabCallback` 0x46d2b0, `MetalBuildingTabCallback`
//!   0x46fc50, `FoodBuildingTabCallback` 0x471250, `TransportBuildingTabCallback` 0x473430,
//!   `MilitaryBuildingTabCallback` 0x471f70) get code 7 for a right click on a control with
//!   `param_3` from 0x675ba0 over the mouse message's key flags: 0x10001 for Ctrl alone (Shift
//!   adds 4: 0x10004 / 0x10005, neither 0). A building button (the same link → type as code 3)
//!   sends UI event 0x1e6 with the type for 0x10001, else 0x1b6 ([`right_click`]). For the tool
//!   smith (21), weapon smith (22), vehicle hall (23) and barracks (24) the Ctrl click also sets
//!   0x12938c7: the next update of D19 / D22 / D24 / D40 that runs presses its sidebar toggle
//!   (1138 / 344 / 1381 / 1761, `SelectControl`-like: a radio is clicked) and clears it.
//! * `CBuildingMgr` keeps per (player, type) a list (heads 0x14eea74 + 4 · (83 · player + type),
//!   next at entity +0x2a, a new building at the head) and a cursor (0x14f0d78, same layout).
//!   0x53d0b0 `(tribe, type)` ([`advance`]): the cursor moves to its building's successor, or the
//!   head when it is not a building or has none; returns the cursor's entity.
//! * 0x1b6 (handler 0x4ea150; states 0 and 4, unchanged): the local player's cursor advanced, the
//!   view centred on that building (event 0x13); the selection stays.
//! * 0x1e6 (0x4e9fd0; states 0 and 1 → 1): the type (or with type 0 the first selected entity's,
//!   if any, else only 0x4edb10) — the selection dropped (0x4ee690), the modes left (0x4edb10:
//!   0x260, 0x259), then the cursor's building **without advancing** (advanced only when the
//!   cursor is 0); when its entity exists it is selected (`SetId`, `RequestDialog`, flag 0x200)
//!   and the view centred on it; else event 0x191.
//! * 0x198 (0x4e9e70, the building dialog's "next"): as 0x1e6 but advancing first.

/// `CBuildingMgr`'s list of (`player`, `kind`), head first (the sim's lists,
/// [`crate::population::Economy::building_lists`]).
pub fn building_list(pop: &crate::population::Population, player: u8, kind: u16) -> Vec<u16> {
    pop.economy.building_lists.get(&(player, kind)).cloned().unwrap_or_default()
}

/// 0x53d0b0: `cursor` moved to the successor of its building in `list` (0 when it is not in the
/// list or the last), then to the head when 0. Returns the new cursor (0: the list is empty).
pub fn advance(list: &[u16], cursor: &mut u16) -> u16 {
    let next = list.iter().position(|&id| id == *cursor).and_then(|i| list.get(i + 1)).copied().unwrap_or(0);
    *cursor = if next != 0 { next } else { list.first().copied().unwrap_or(0) };
    *cursor
}

/// The cursor 0x1e6 goes to (0x4e9fd0): the current one, advanced only when 0.
pub fn current(list: &[u16], cursor: &mut u16) -> u16 {
    if *cursor == 0 {
        advance(list, cursor);
    }
    *cursor
}

/// A build page's code 7 on building `kind` with Ctrl (`ctrl`) and Shift held: the UI event and
/// whether 0x12938c7 is set (the sidebar toggle pressed by the building's dialog).
pub fn right_click(kind: u8, ctrl: bool, shift: bool) -> (u16, bool) {
    if ctrl && !shift { (0x1e6, matches!(kind, 21..=24)) } else { (0x1b6, false) }
}

/// The sidebar toggle a dialog's update presses while 0x12938c7 is set (D22 0x46c230 → 0x158,
/// D19 0x49f8e0 → 0x472, D24 0x4a5100 → 0x565, D40 0x4ae560 → 0x6e1).
pub fn deferred_toggle(dialog: usize) -> Option<u16> {
    Some(match dialog {
        22 => 0x158,
        19 => 0x472,
        24 => 0x565,
        40 => 0x6e1,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_walks_the_list_and_wraps() {
        let list = [30, 20, 10];
        let mut c = 0;
        assert_eq!(current(&list, &mut c), 30);
        assert_eq!(current(&list, &mut c), 30, "0x1e6 stays");
        assert_eq!(advance(&list, &mut c), 20);
        assert_eq!(advance(&list, &mut c), 10);
        assert_eq!(advance(&list, &mut c), 30, "wraps to the head");
        let mut gone = 99;
        assert_eq!(advance(&list, &mut gone), 30, "not a building of the list: the head");
        let mut none = 5;
        assert_eq!(advance(&[], &mut none), 0);
    }

    #[test]
    fn code_7_by_keys() {
        assert_eq!(right_click(21, true, false), (0x1e6, true));
        assert_eq!(right_click(15, true, false), (0x1e6, false));
        assert_eq!(right_click(21, true, true), (0x1b6, false));
        assert_eq!(right_click(21, false, false), (0x1b6, false));
    }
}
