//! The goods race dialog D77 of game type 3 (the economy mode), as the shipped exe opens, fills
//! and runs it (own RE; `tools/re/gui-2026-10-23/{d77.c,createuitab.c,goodsrace_setup.c}`,
//! `tools/re/gui-2026-10-22/{extras2.c,extras3.c,goodsrace.c}`).
//!
//! * The goods: the single-player lobby (0x51c1a0, game type 3 and no savegame) draws seven into
//!   `GlobalGameState +0x2fc` (0x51f5c0, [`draw`]); the game copies them to
//!   `GlobalGameStateObject +0x78` (0x460540, `s4_sim::world::GameSetup::race_goods`).
//! * The extras submenu's 929 (0x3a1, `ExtrasMenuCallback` 0x47b4e0): with game type 3 (0x15aa5c6,
//!   set by the submenu's init) UI events 0x192 and 0x1b4 instead of the briefing page 35. The
//!   handler 0x4e9c20 (state 0) runs the fill 0x4f8da0 into the info object 0x11ab0b0 (code
//!   0x24) and puts it into `GoodDistributionInfoHolder` (0x14540d0 +4 / +8 / +0xc), which the
//!   logic tick 0x4f2720 calls again at every tick with `tick & 0xf == 0xe`; 0x192's handler
//!   0x4edac0 clears it. The fill always sends event 0x25a: `CreateUITab` 0x4841e0 closes the
//!   page dialog (`CGuiEventHandler +0xc`) and opens dialog 77 (surface 2) anew, procedure
//!   0x474d90 (code 0: the init 0x474b20, then the update 0x474b70; codes 1 / 8 / 9 sounds and
//!   tooltips).
//! * The fill: per entry i < 7 the good (byte +0x78 + i) and over the players 1..=`PlayerCount`
//!   the sums of `CStatistic +0x1078 + 4 · good` for alliance 1 (0x12dcd0c) and for all others.
//! * The init: 646 = string 0x7fa, 670 = "I", 671 = "II". The update per row i: the two sums
//!   ("%u") at 649 + i and 656 + i, the larger in font 1 and the other in font 2 (both 2 when
//!   equal); 663 + i the good's icon (0x49f720, pressed image 0) and tooltips (table 0xffc7ac).
//!
//! Checked against the exe run in unicorn (`tools/re/gui-2026-10-23/d77_emu.py`,
//! `tests/d77_vectors.rs`).

use crate::guistate::GuiState;

/// The dialog slot.
pub const DIALOG: usize = 77;
/// Its info code (`CreateUITab` case 0x24).
pub const CODE: usize = 0x24;
/// The extras submenu's button (0x3a1).
pub const BUTTON: (usize, u16) = (8, 929);

/// One entry of the fill 0x4f8da0.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Row {
    pub good: u8,
    /// Alliance 1's players.
    pub team: i32,
    /// Every other player 1..=`PlayerCount`.
    pub rest: i32,
}

/// The fill 0x4f8da0: `teams` = the alliance ids by player (0x12dcd0c), `start(p, good)` =
/// `CStatistic +0x1078 + 4 · good` of player `p`.
pub fn fill(goods: [u8; 7], player_count: u8, teams: &[u8; 9], start: impl Fn(u8, u8) -> i32) -> [Row; 7] {
    goods.map(|good| {
        let mut r = Row { good, team: 0, rest: 0 };
        for p in 1..=player_count.min(8) {
            let v = start(p, good);
            if teams[p as usize] == 1 {
                r.team = r.team.wrapping_add(v);
            } else {
                r.rest = r.rest.wrapping_add(v);
            }
        }
        r
    })
}

/// The init 0x474b20.
pub fn init(st: &mut GuiState, s: &dyn Fn(u32) -> String) {
    st.texts.insert((DIALOG, 646), s(0x7fa));
    st.texts.insert((DIALOG, 670), "I".into());
    st.texts.insert((DIALOG, 671), "II".into());
}

/// The update 0x474b70.
pub fn update(st: &mut GuiState, rows: &[Row; 7]) {
    for (i, r) in rows.iter().enumerate() {
        let (a, b, icon) = (649 + i as u16, 656 + i as u16, 663 + i as u16);
        let (fa, fb) = match r.team.cmp(&r.rest) {
            std::cmp::Ordering::Less => (2, 1),
            std::cmp::Ordering::Greater => (1, 2),
            std::cmp::Ordering::Equal => (2, 2),
        };
        st.fonts.insert((DIALOG, a), fa);
        st.fonts.insert((DIALOG, b), fb);
        st.texts.insert((DIALOG, a), (r.team as u32).to_string());
        st.texts.insert((DIALOG, b), (r.rest as u32).to_string());
        st.images.insert((DIALOG, icon), (crate::building_dialog::good_icon(r.good as i32), 0));
        if let Some(t) = tooltips(r.good) {
            st.tooltips.insert((DIALOG, icon), t);
        }
    }
}

/// Table 0xffc7a8 (stride 0x14) +4 / +8: a good's tooltips. [unk] Goods past 43 read beyond it;
/// the lobby never draws them.
fn tooltips(good: u8) -> Option<(u16, u16)> {
    match good {
        0..43 => {
            let t = crate::gui_pages::TRANSPORT_GOOD[good as usize];
            Some((t.2, t.3))
        }
        43 => Some((718, 731)),
        _ => None,
    }
}

/// 0x51f5c0's goods it never draws.
const EXCLUDED: [u8; 18] = [0, 1, 2, 6, 13, 23, 26, 30, 39, 40, 35, 19, 17, 5, 37, 38, 41, 42];

/// 0x51f5c0: from the goods in `goods` (the lobby's previous ones), every entry that is excluded
/// or repeats an earlier one drawn anew as `rand() % 43` until it is neither.
pub fn draw(mut goods: [u8; 7], rand: &mut impl FnMut() -> u32) -> [u8; 7] {
    for i in 0..7 {
        while EXCLUDED.contains(&goods[i]) || goods[..i].contains(&goods[i]) {
            goods[i] = (rand() % 43) as u8;
        }
    }
    goods
}

/// The CRT's `rand` (0xd96b7f: `holdrand · 0x343fd + 0x269ec3`, bits 16..30).
pub fn crt_rand(seed: u32) -> impl FnMut() -> u32 {
    let mut h = seed;
    move || {
        h = h.wrapping_mul(0x343fd).wrapping_add(0x269ec3);
        (h >> 16) & 0x7fff
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_seven_distinct_allowed_goods() {
        let g = draw([0; 7], &mut crt_rand(1));
        for (i, x) in g.iter().enumerate() {
            assert!(!EXCLUDED.contains(x) && !g[..i].contains(x) && *x < 43, "{g:?}");
        }
        // Allowed and distinct goods are kept.
        assert_eq!(draw([3, 4, 7, 8, 9, 10, 11], &mut || panic!()), [3, 4, 7, 8, 9, 10, 11]);
        // MSVC's rand from seed 1: 41, 18467, 6334, ...
        let mut r = crt_rand(1);
        assert_eq!([r(), r(), r()], [41, 18467, 6334]);
    }
}
