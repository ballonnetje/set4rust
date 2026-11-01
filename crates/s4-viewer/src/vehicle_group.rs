//! The vehicle group dialog D40 and the ferry dialog D42, as the shipped exe opens, fills and runs
//! them (own RE; `tools/re/gui-2026-10-22/{d40.c,warmach.c,d42.c}`; `d40_emu.py` / `d42_emu.py` run
//! the original updates in unicorn, `tests/d40_vectors.rs` / `tests/d42_vectors.rs` check this port
//! against them).
//!
//! * Opening (`CInputProcessor::RequestDialog` 0x4ee730, by the first selected entity, see
//!   [`request_dialog`]): a warship (ship type 1), a war machine (land vehicle type 4) or a ferry
//!   (ship type 2) with two or more selected → the group fill 0x5972b0 (info object 0x11aa8f8, code
//!   0x14, event 0x25a; `GoodDistributionInfoHolder` keeps the fill to refresh it with 0x25c);
//!   `CreateUITab` (0x4841e0) case 0x14: `ResetMainMenuForContext`, dialog 40 opened with the
//!   procedure 0x4ae790, title 0x6d9 = string 0x377, the update 0x4ae560. A single ferry → its
//!   vtbl+0x94, `CFerryShip` 0x58e450 (info 0x11ab078, code 0x17) → `CreateUITab` case 0x17:
//!   `OpenDialog(42, 0x474f80)`, whose code 0 runs the update 0x474e80. Carts (land type 5) and
//!   transport ships (ship type 3) get their vtbl+0x94 too: D41 ([`crate::vehicle_dialog`]).
//! * The group fill 0x5972b0 counts the selected entities by type (entity +0xc, whatever their
//!   class): (4, war machines), (1, warships), (2, ferries) at +8 / +0x10 / +0x18.
//! * The procedure 0x4ae790 code 3: 0x6e1 (toggle) pressed → UI 0x1c2 (the groups sidebar D55),
//!   released → 0x1cc and GUI event 0x260; 0x6e2 → 0x1a0 (no `CInputProcessor` transition in any
//!   state: nothing happens); 0x6e3 → 0x1a1 (net 5009 `VehicleUnload` for each selected, 0x4ed0f0);
//!   0x6e4 → 0x1a2 (5010 `CatapultLoad` for each selected, 0x4ed1d0); 0x6e5 → 0x1c0 (5049
//!   `DestroyVehicle` for each selected, then deselect and 0x25b, 0x4ee940). Sound 0xb after
//!   each; code 1 sounds 6 / 7.
//! * The ferry procedure 0x474f80 code 3: 0x2ca → UI 0x1ab (0x4e9cc0: the next vehicle of the
//!   type, [`next_vehicle`]); 0x2da → 0x1a1; 0x2e6 → 0x1a6 (0x4eccc0: GUI event 0x260, then the
//!   group fill 0x5972b0 for the selection: D40 replaces D42).

use crate::guistate::GuiState;

/// The group dialog's slot.
pub const GROUP: usize = 40;
/// The ferry dialog's slot.
pub const FERRY: usize = 42;

/// `RequestDialog` 0x4ee730 for a first selected vehicle of object class `obj` (2 ship, 4 land
/// vehicle) and type `kind`, `count` entities selected: the dialog it opens here (D40 / D42), or
/// `None` (D41 through the vehicle's vtbl+0x94, or nothing).
pub fn request_dialog(obj: u8, kind: u16, count: usize) -> Option<usize> {
    match (obj, kind) {
        (2, 1) | (4, 4) => Some(GROUP),
        (2, 2) if count >= 2 => Some(GROUP),
        (2, 2) => Some(FERRY),
        _ => None,
    }
}

/// The group fill 0x5972b0's info object (0x11aa8f8 +8): three (type, count) rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GroupInfo {
    pub rows: [(u32, i32); 3],
}

impl Default for GroupInfo {
    fn default() -> Self {
        GroupInfo { rows: [(4, 0), (1, 0), (2, 0)] }
    }
}

/// 0x5972b0 over the selected entities' types (entity +0xc).
pub fn group_fill(kinds: impl IntoIterator<Item = u16>) -> GroupInfo {
    let mut info = GroupInfo::default();
    for k in kinds {
        match k {
            1 => info.rows[1].1 += 1,
            2 => info.rows[2].1 += 1,
            4 => info.rows[0].1 += 1,
            _ => {}
        }
    }
    info
}

/// The group dialog's title (`CreateUITab` case 0x14 and the procedure's code 0: 0x6d9 = string
/// 0x377).
pub fn group_title(st: &mut GuiState, strings: &dyn Fn(u32) -> String) {
    st.texts.insert((GROUP, 0x6d9), strings(0x377));
}

/// The update 0x4ae560: the three buttons greyed, then per row: warships (1) → text 0x6de and
/// 0x6e2 enabled when any; ferries (2) → 0x6df, 0x6e3; war machines (4) → 0x6dd, 0x6e4, and the
/// destroy button 0x6e5 shown only for exactly one. The counts as "%u". [not ported] Its tail
/// presses the toggle 0x6e1 when 0x12938c7 is set (only the build pages' go-to sets it).
pub fn group_update(info: &GroupInfo, st: &mut GuiState) {
    let d = GROUP;
    for link in [0x6e2, 0x6e3, 0x6e4] {
        st.set_greyed(d, link, true);
    }
    for &(kind, n) in &info.rows {
        let (text, button) = match kind {
            1 => (0x6de, 0x6e2),
            2 => (0x6df, 0x6e3),
            4 => (0x6dd, 0x6e4),
            _ => continue,
        };
        if n > 0 {
            st.set_greyed(d, button, false);
        }
        if kind == 4 {
            st.set_hidden(d, 0x6e5, n != 1);
        }
        st.texts.insert((d, text), (n as u32).to_string());
    }
}

/// The procedure 0x4ae790's code 3 on `link` (`pressed`: the toggle's new state): the UI event.
/// The toggle released also sends GUI event 0x260 (the sidebar closed).
pub fn group_event(link: u16, pressed: bool) -> Option<u16> {
    Some(match link {
        0x6e1 if pressed => 0x1c2,
        0x6e1 => 0x1cc,
        0x6e2 => 0x1a0,
        0x6e3 => 0x1a1,
        0x6e4 => 0x1a2,
        0x6e5 => 0x1c0,
        _ => return None,
    })
}

/// The ferry fill 0x58e450's info object (0x11ab078 +8, twelve dwords): specialists of types 50,
/// 49, 48, 47, 46; soldiers 0x1d..=0x1f, 0x20..=0x22, the third type (0x23..=0x2b, 0x3d..=0x3f),
/// 44, 45; war machines (4), carts (5) — the passengers aboard, `s4_sim::vehicles::ferry::dialog`
/// (population `economy.ferries`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FerryInfo(pub [u32; 12]);

/// The ferry dialog's text links per info dword (.rdata 0xffcb0c, 0xffcb34, 0xffcafc; stride 8,
/// the second word of each pair is the icon beside it).
const FERRY_TEXTS: [u16; 12] = [731, 732, 733, 734, 735, 736, 737, 738, 739, 743, 740, 741];

/// The update 0x474e80: title 0x2cb = string 0x507, then the twelve counts as "%u".
pub fn ferry_update(info: &FerryInfo, st: &mut GuiState, strings: &dyn Fn(u32) -> String) {
    st.texts.insert((FERRY, 0x2cb), strings(0x507));
    for (&link, &n) in FERRY_TEXTS.iter().zip(&info.0) {
        st.texts.insert((FERRY, link), n.to_string());
    }
}

/// The procedure 0x474f80's code 3 on `link`: the UI event.
pub fn ferry_event(link: u16) -> Option<u16> {
    Some(match link {
        0x2ca => 0x1ab,
        0x2da => 0x1a1,
        0x2e6 => 0x1a6,
        _ => return None,
    })
}

/// UI event 0x1ab (0x4e9cc0, D41's 0x6b1 and D42's 0x2ca) with the first selected vehicle
/// `first` of type `kind`: the local player's list of that type (`CVehicleMgr` 0x1549bd0, `list`
/// head first, linked through `IEntity +0x2a`) walked from the vehicle's successor, wrapping to the
/// head, past vehicles with flag 0x8000, until back at `first`. The vehicle reached (`first` itself
/// when no other qualifies) is selected alone, its dialog requested and the view centred on it.
/// `flags` gives an id's entity flags.
pub fn next_vehicle(list: &[u16], first: u16, flags: &dyn Fn(u16) -> u32) -> Option<u16> {
    let at = list.iter().position(|&id| id == first)?;
    (1..=list.len()).map(|k| list[(at + k) % list.len()]).find(|&id| id == first || flags(id) & 0x8000 == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_dialog_by_class_type_and_count() {
        assert_eq!(request_dialog(4, 4, 1), Some(GROUP));
        assert_eq!(request_dialog(2, 1, 1), Some(GROUP));
        assert_eq!(request_dialog(2, 2, 1), Some(FERRY));
        assert_eq!(request_dialog(2, 2, 2), Some(GROUP));
        assert_eq!(request_dialog(2, 3, 1), None);
        assert_eq!(request_dialog(4, 5, 1), None);
    }

    #[test]
    fn group_fill_counts_by_type() {
        assert_eq!(group_fill([4, 4, 2, 5, 1, 30]).rows, [(4, 2), (1, 1), (2, 1)]);
    }

    #[test]
    fn next_vehicle_wraps_and_skips_flag_0x8000() {
        let flags = |id: u16| if id == 7 { 0x8000 } else { 0 };
        assert_eq!(next_vehicle(&[9, 7, 5], 9, &flags), Some(5));
        assert_eq!(next_vehicle(&[9, 7, 5], 5, &flags), Some(9));
        assert_eq!(next_vehicle(&[9, 7], 9, &flags), Some(9));
        assert_eq!(next_vehicle(&[9], 3, &flags), None);
    }
}
