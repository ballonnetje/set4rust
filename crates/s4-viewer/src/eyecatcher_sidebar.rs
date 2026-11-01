//! The eye catchers' sidebar (dialog 52) of the town build page, as the shipped exe runs it (own
//! RE; `tools/re/gui-2026-10-22/{extras3.c,eyecatch.c,eyecatch2.c}`).
//!
//! * The town page's toggle 0x224 (`TransportBuildingTabCallback` 0x473430 code 3): pressed → info
//!   object 0x11ab130 (+4 code 0x1b) with twelve rows of the types 0x40..=0x4b, UI event 0x1ca
//!   (handler 0x4ebc60, states 0 and 4 → 0: the build mode left): 0x4ef270, then the fill
//!   0x53d800 ([`fill`]) with event 0x25f → the sidebar opener 0x481260 case 0x1b: dialog 52 with
//!   the procedure 0x46ecf0 on the sidebar surface; the fill stays the sidebar's provider
//!   (refreshed with 0x25e). Released → 0x1cc, GUI event 0x260 and 0x191.
//! * The fill 0x53d800 per row: +0xc the local tribe's finished buildings of the type (0x14ef624,
//!   incremented when a site is done, 0x536340), +0x10 its sites (0x14ef620), +0x14..+0x16 the
//!   local race's costs (building info bytes +3 boards, +2 stone, +4 gold).
//! * The procedure 0x46ecf0: code 0 → [`update`] (0x46e880) and zero costs; code 3 on a button
//!   (pressed) → 0x1b5 with its type (the build mode), released → 0x191 (as the build pages, so
//!   the buttons are in `guistate::build_button`); code 5 on a button → [`costs`] (0x46e7d0, row
//!   = type − 0x40), code 6 / elsewhere → zeros; code 7 on a button → 0x1b6 with its type (no
//!   Ctrl variant, s4_viewer::build_goto).

use crate::guistate::GuiState;

/// The dialog slot.
pub const DIALOG: usize = 52;
/// The town page's toggle that opens it (0x224).
pub const TOGGLE: (usize, u16) = (12, 0x224);

/// (type, count link, "+n" link) as 0x46e880 writes them.
const COUNTS: [(u8, u16, u16); 12] = [
    (0x40, 0x2a5, 0x2ae),
    (0x41, 0x2a3, 0x2ad),
    (0x42, 0x2a1, 0x2ac),
    (0x43, 0x2b5, 0x2b6),
    (0x44, 0x2ab, 0x2b1),
    (0x45, 0x2a9, 0x2af),
    (0x46, 0x2aa, 0x2b0),
    (0x47, 0x2b3, 0x2b4),
    (0x48, 0x2b8, 0x2b9),
    (0x49, 0x2c4, 0x2c5),
    (0x4a, 0x2c6, 0x2c7),
    (0x4b, 0x2c8, 0x2c9),
];

/// (button link, type) of the procedure's code 3 / 5 / 7.
pub const BUTTONS: [(u16, u8); 12] = [
    (0x2a0, 0x41),
    (0x2a2, 0x44),
    (0x2a4, 0x40),
    (0x2a6, 0x42),
    (0x2a7, 0x45),
    (0x2a8, 0x46),
    (0x2b2, 0x47),
    (0x2b7, 0x43),
    (0x2ba, 0x48),
    (0x2c1, 0x49),
    (0x2c2, 0x4a),
    (0x2c3, 0x4b),
];

/// The cost links (boards, stone, gold) of 0x46e7d0.
const COST_LINKS: [u16; 3] = [0x2be, 0x2bf, 0x2c0];

/// One row of the info object.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Row {
    pub kind: u8,
    pub done: u32,
    pub sites: i32,
    /// (boards, stone, gold) as signed bytes.
    pub costs: (i8, i8, i8),
}

/// The fill 0x53d800 for the `local` player of race `race`: `costs` = building info costs by
/// (race, type) (`gui_pages::costs`).
pub fn fill(pop: &crate::population::Population, local: u8, race: u8, costs: &std::collections::HashMap<(u8, u8), (u8, u8, u8)>) -> [Row; 12] {
    std::array::from_fn(|i| {
        let kind = 0x40 + i as u8;
        let mine = pop.buildings.iter().filter(|b| b.id != 0 && b.owner == local && b.kind == kind);
        let (done, sites) = mine.fold((0, 0), |(d, s), b| if b.site.is_some() { (d, s + 1) } else { (d + 1, s) });
        let (b, s, g) = costs.get(&(race, kind)).copied().unwrap_or_default();
        Row { kind, done, sites, costs: (b as i8, s as i8, g as i8) }
    })
}

/// The update 0x46e880: per row its count ("%u") and sites ("+%u", shown when above 0).
pub fn update(st: &mut GuiState, rows: &[Row; 12]) {
    for r in rows {
        let Some(&(_, count, plus)) = COUNTS.iter().find(|c| c.0 == r.kind) else { continue };
        st.texts.insert((DIALOG, count), r.done.to_string());
        st.texts.insert((DIALOG, plus), format!("+{}", r.sites as u32));
        st.set_hidden(DIALOG, plus, r.sites <= 0);
    }
}

/// 0x46e7d0: the costs of the button `hovered` (the cursor's link in dialog 52), else zeros.
pub fn costs(st: &mut GuiState, rows: &[Row; 12], hovered: Option<u16>) {
    let row = hovered.and_then(|l| BUTTONS.iter().find(|b| b.0 == l)).map(|b| rows[(b.1 - 0x40) as usize]);
    let (b, s, g) = row.map_or((0, 0, 0), |r| (r.costs.0 as i32, r.costs.1 as i32, r.costs.2 as i32));
    for (l, v) in COST_LINKS.iter().zip([b, s, g]) {
        st.texts.insert((DIALOG, *l), (v as u32).to_string());
    }
}
