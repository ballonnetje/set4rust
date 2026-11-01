//! D55, the unit groups sidebar the soldier and specialist dialogs open (own RE, release
//! addresses): D45's toggle 0x5a0 and D44's 0x5ba send UI event 0x1c2 (handler 0x4eba10: the
//! fill 0x56a510 into `CGroupSideBarInfo` 0x11aad7c, info code 0x19, then the provider 0x56ac20
//! refreshes it, event 0x25e); the opener 0x481260 puts dialog 0x37 on the sidebar surface.
//! * Fill 0x56a510: for groups 1..=10 (`CGroupMgr` lists 0x153fe64 + player · 0x8c4, 0xcc
//!   apart) with members, the first member's warrior type (entity +0x14 & 0xf): 2..=6 → 1
//!   (soldiers), 7 → 2 (specialists), 10 / 11 → 3; else 0.
//! * Update 0x479120 (also the procedure 0x479270's code 0): groups 1..=9 (links 882..=890,
//!   .rdata 0xffd2bc): 0 hidden, else shown with image 0x163 / 0x162 / 0x164 (types 1 / 2 / 3, 4).
//! * Procedure 0x479270: code 3 on the buttons 873..=881 (groups 1..=9) sends 0x1d6 (store the
//!   selection, 0x4ec630) with Ctrl or Shift held (the third argument's bit 0x10000, 0x675ba0),
//!   else 0x1d8 (recall, 0x4ec880); code 7 (right click) sends 0x1d9 (the camera to the group,
//!   0x4ec760) — group 2's right click sends 0x1d8 (kept).

use crate::guistate::GuiState;
use crate::population::Population;

/// The dialog slot.
pub const DIALOG: usize = 55;
/// The group buttons (groups 1..=9) and their type images.
pub const BUTTONS: std::ops::RangeInclusive<u16> = 873..=881;
const MARKS: u16 = 882;

/// 0x56a510: the type of groups 1..=10 of `player`.
pub fn fill(pop: &Population, player: u8) -> [u8; 10] {
    let groups = s4_sim::group::GROUPS;
    std::array::from_fn(|k| {
        let g = player as usize * groups + k + 1;
        let Some(&first) = pop.groups.get(g).and_then(|m| m.first()) else { return 0 };
        let flags = pop.settlers.iter().chain(&pop.vehicles).find(|u| u.id == first).map_or(0, |u| u.flags);
        match flags & 0xf {
            2..=6 => 1,
            7 => 2,
            10 | 11 => 3,
            _ => 0,
        }
    })
}

/// 0x479120.
pub fn update(st: &mut GuiState, types: &[u8; 10]) {
    for (k, &t) in types.iter().take(9).enumerate() {
        let link = MARKS + k as u16;
        let image = match t {
            0 => {
                st.set_hidden(DIALOG, link, true);
                continue;
            }
            1 => 0x163,
            2 => 0x162,
            3 | 4 => 0x164,
            _ => continue,
        };
        st.set_hidden(DIALOG, link, false);
        st.images.insert((DIALOG, link), (image, 0));
    }
}

/// The procedure's UI event for a click on `link` (`modifier`: Ctrl or Shift held) or a right
/// click (`right`): (event, group).
pub fn click(link: u16, modifier: bool, right: bool) -> Option<(u16, u8)> {
    if !BUTTONS.contains(&link) {
        return None;
    }
    let g = (link - BUTTONS.start() + 1) as u8;
    Some(match (right, modifier) {
        (true, _) if g == 2 => (0x1d8, g),
        (true, _) => (0x1d9, g),
        (false, true) => (0x1d6, g),
        (false, false) => (0x1d8, g),
    })
}
