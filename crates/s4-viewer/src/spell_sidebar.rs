//! The priests' spells sidebar D56, as the shipped exe opens, fills and runs it (own RE;
//! `tools/re/gui-2026-10-22/{d45.c,d56.c}`).
//!
//! * D45's toggle 0x5a4 (1444) sends UI event 0x1c1 (state 1 → 8, handler 0x4eba40): the chosen
//!   spell (`CInputProcessor +0x14`) = −1, the fill 0x56a670 (`CMagicSpellSideBarInfo` 0x11aada8,
//!   code 0x1a) with event 0x25f → the opener 0x481260 case 0x1a: dialog 56, procedure 0x47bbc0
//!   (code 0 → the update 0x47ba30); the provider 0x56ac40 refreshes it (0x25e). Released → 0x1cc
//!   (state 8 → 1) and GUI event 0x260.
//! * The fill per spell i (0..8, row +8 + 16·i): +0 i, +4 the casts the mana pays for (0x4f5a50,
//!   `s4_sim::magic::affordable`), +8 `cost · 100 / mana` (0 without mana; unread), +0xc the
//!   current cost (at least 1; 1 for a player past `PlayerCount`).
//! * The update 0x47ba30: per row the casts at 0x3ac + i and the cost at 0x3b4 + i ("%u").
//! * The procedure's code 3 on 0x3a4 + i ([`click`]): pressed → UI 0x1da with w = i (states 1 /
//!   8 → 8, handler 0x4eb860: +0x14 = w), released → 0x1da with w = −1. Sound 6 / 7.
//! * In state 8 a right click on the map (0x1e0, handler 0x4eb880; the middle button excepted,
//!   key flag 0x10) with a chosen spell sends net 5048 `CastSpell` (priest << 16 | spell,
//!   y << 16 | x) for the selected priest (type 45) nearest to the node by hex distance (the first
//!   on ties); without a chosen spell it is `SendTo` (0x4ec960). A left click selects as usual
//!   (state 1).

use crate::guistate::GuiState;

/// The dialog slot.
pub const DIALOG: usize = 56;
/// D45's toggle that opens it.
pub const TOGGLE: (usize, u16) = (45, 0x5a4);

/// One row of the fill 0x56a670.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Row {
    pub casts: i32,
    pub percent: i32,
    pub cost: i32,
}

/// The fill 0x56a670 from the local player's mana and (casts, cost) per spell; `in_game` =
/// `local − 1 < PlayerCount`.
pub fn fill(mana: i32, spells: &[(i32, i32); 8], in_game: bool) -> [Row; 8] {
    std::array::from_fn(|i| {
        let cost = if in_game { spells[i].1.max(1) } else { 1 };
        let percent = if mana < 1 { 0 } else { cost.wrapping_mul(100) / mana };
        Row { casts: spells[i].0, percent, cost }
    })
}

/// The update 0x47ba30.
pub fn update(st: &mut GuiState, rows: &[Row; 8]) {
    for (i, r) in rows.iter().enumerate() {
        st.texts.insert((DIALOG, 0x3ac + i as u16), (r.casts as u32).to_string());
        st.texts.insert((DIALOG, 0x3b4 + i as u16), (r.cost as u32).to_string());
    }
}

/// The procedure 0x47bbc0's code 3 on `link`: the spell UI event 0x1da's w (−1 = none chosen).
pub fn click(link: u16, pressed: bool) -> Option<i32> {
    (0x3a4..=0x3ab).contains(&link).then(|| if pressed { (link - 0x3a4) as i32 } else { -1 })
}

/// 0x4eb880's caster: of the selected `priests` (id, x, y), the one nearest to `(x, y)` by hex
/// distance, the first on ties.
pub fn caster(priests: &[(u16, u16, u16)], x: u16, y: u16) -> Option<u16> {
    let mut best: Option<(u32, u16)> = None;
    for &(id, px, py) in priests {
        let dy = y as i32 - py as i32;
        let dx = x as i32 - px as i32;
        let d = if (dx ^ dy) < 0 { dx.unsigned_abs() + dy.unsigned_abs() } else { dx.unsigned_abs().max(dy.unsigned_abs()) };
        if best.is_none_or(|b| d < b.0) {
            best = Some((d, id));
        }
    }
    best.map(|b| b.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_and_caster() {
        let rows = fill(50, &[(2, 20), (0, 0), (1, 60), (0, 5), (0, 5), (0, 5), (0, 5), (0, 5)], true);
        assert_eq!(rows[0], Row { casts: 2, percent: 40, cost: 20 });
        assert_eq!(rows[1].cost, 1, "at least 1");
        assert_eq!(fill(0, &[(0, 7); 8], true)[0].percent, 0);
        assert_eq!(fill(10, &[(0, 7); 8], false)[0].cost, 1);
        // Same signs: the larger difference; opposite: the sum.
        assert_eq!(caster(&[(1, 10, 10), (2, 13, 7), (3, 12, 12)], 10, 13), Some(1));
        assert_eq!(caster(&[(2, 13, 7), (1, 10, 10)], 11, 11), Some(1));
        assert_eq!(click(0x3a6, true), Some(2));
        assert_eq!(click(0x3a6, false), Some(-1));
    }
}
