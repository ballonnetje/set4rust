//! The goods race's end screen (`CStateEcoStatistic`, game type 3), as the shipped exe runs it
//! (own RE; `tools/re/gui-2026-10-24/race_end*.c`; `race_emu.py` runs the original fill in
//! unicorn, `tests::race_end_vectors` checks this port against it).
//!
//! * `EndGameGoToStatistics` 0x514450 pushes the state 0x5128e0 for game type 3 instead of the
//!   end statistics: start-screen dialog 22 with the procedure 0x493c80 (code 0: mode 0x15aabae
//!   = 0 and the fill 0x491f70; code 3: 0xa78 OK → event 0x77, 0xa81 / 0xa82 → mode 0 / 1 and the
//!   fill again, 0xaeb the random map's save).
//! * The fill reads the end record (frontend/endstat.rs) and the goods race's info object
//!   0x11ab0b0 (D77's fill 0x4f8da0: per good, team 1's sum and the others'). It greys and hides
//!   0xa82 (mode 1), so mode 1 (the players and their own goods, +0xa04) is never shown; ported
//!   anyway.
//! * Quirks kept: the goods loop runs eight times (0x492450 `jle`), the eighth "good" being the
//!   next object's vtable (its link 0x6925 is no control: nothing visible); mode 0 shows a team
//!   on the row of its first player and then hides rows 3..=8, so a team whose first player is
//!   3 or later is hidden again and row 2 stays as the dialog had it.

use super::endstat::{time, tooltip_in, EndRecord};
use s4_viewer::guistate::GuiState;

/// The dialog slot.
pub const DIALOG: usize = 22;
/// OK, the mode radios.
pub const OK: u16 = 0xa78;
pub const MODE0: u16 = 0xa81;
pub const MODE1: u16 = 0xa82;
pub const SAVE_MAP: u16 = 0xaeb;

/// .rdata 0x102d86c..: the link and image tables the fill indexes (row r = 1..=8): values
/// 7r + j, team images 62 + team, race images 71 + race, colour images 76 + colour, result links
/// 83 + r, team links 91 + r, race links 99 + r, colour links 107 + r, name links 115 + r, the
/// good icon links 124 + k (k < 8: the eighth is 0x6925), the mode-0 hide loop's extra link 8r.
const F: [u32; 136] = [
    2614, 2615, 2616, 2617, 1835099476, 6563104, 0, 2715, 2723, 2731, 2739, 2747, 2755, 2787, 2716, 2724, 2732, 2740, 2748, 2756, 2788, 2717, 2725,
    2733, 2741, 2749, 2757, 2789, 2718, 2726, 2734, 2742, 2750, 2758, 2790, 2719, 2727, 2735, 2743, 2751, 2759, 2791, 2720, 2728, 2736, 2744, 2752,
    2760, 2792, 2721, 2729, 2737, 2745, 2753, 2761, 2793, 2722, 2730, 2738, 2746, 2754, 2762, 2794, 115, 116, 117, 118, 119, 120, 121, 122, 33, 34,
    32, 35, 38, 0, 2, 4, 6, 8, 10, 12, 14, 2707, 2708, 2709, 2710, 2711, 2712, 2713, 2714, 2699, 2700, 2701, 2702, 2703, 2704, 2705, 2706, 2691,
    2692, 2693, 2694, 2695, 2696, 2697, 2698, 2681, 2682, 2683, 2684, 2685, 2686, 2687, 2688, 2779, 2780, 2781, 2782, 2783, 2784, 2785, 2786, 2768,
    2769, 2770, 2771, 2772, 2773, 2778, 26917, 796206376, 2716965, 623867173, 117,
];

/// `F[i]` as a link or an image (indices outside the table read 0).
fn f(i: i64) -> u16 {
    usize::try_from(i).ok().and_then(|i| F.get(i)).map_or(0, |&v| v as u16)
}

/// The good icons of `sub_93F90` 0x493f90 (goods 0..=43; others 314).
const ICONS: [u16; 44] = [
    314, 281, 280, 279, 305, 304, 302, 314, 301, 299, 290, 298, 297, 296, 313, 295, 294, 293, 312, 292, 303, 278, 311, 283, 291, 310, 289, 309, 308,
    307, 288, 306, 300, 287, 284, 286, 285, 282, 330, 328, 329, 327, 331, 314,
];

/// The goods' (tooltip, extended tooltip) (.rdata 0xffd7c4 / 0xffd7c8, stride 0x14).
const TIPS: [(i32, i32); 44] = [
    (0, 0), (1610, 1611), (1612, 1613), (1614, 1615), (1616, 1617), (1618, 1619), (1620, 1621), (1662, 1663), (1622, 1623), (1624, 1625),
    (1626, 1627), (1632, 1633), (1634, 1635), (1636, 1637), (1640, 1641), (1638, 1639), (1642, 1643), (1664, 1665), (1644, 1645), (1646, 1647),
    (1650, 1651), (1648, 1649), (1652, 1653), (1654, 1655), (1656, 1657), (1658, 1659), (1660, 1661), (1630, 1631), (1666, 1667), (1668, 1669),
    (1670, 1671), (1672, 1673), (1674, 1675), (1676, 1677), (1678, 1679), (1680, 1681), (1682, 1683), (1684, 1685), (271, 272), (274, 275),
    (269, 270), (276, 277), (278, 279), (90, 91),
];

/// The map's text here: from after the last backslash (`wcsrchr` 0xd767fb, unlike the end
/// statistics' first) to the next '.', or the whole path without one.
pub fn map_name(path: &str) -> String {
    match path.rsplit_once('\\') {
        None => path.to_string(),
        Some((_, rest)) => rest.split('.').next().unwrap_or("").to_string(),
    }
}

/// The goods race's rows as D77's fill left them: (good, team 1's sum, the others' sum).
pub type Goods = [(i32, i32, i32); 7];

fn hide(st: &mut GuiState, link: u16, hidden: bool) {
    st.set_hidden_notify(DIALOG, link, hidden);
}

/// The fill 0x491f70 in mode `mode1` (`random_map`: `RandomMap` vtbl+0x24 on the map's name).
pub fn fill(st: &mut GuiState, rec: &EndRecord, goods: &Goods, strings: &dyn Fn(u32) -> String, random_map: bool, mode1: bool) {
    let text = |st: &mut GuiState, link: u16, t: String| {
        st.texts.insert((DIALOG, link), t);
    };
    st.set_greyed(DIALOG, MODE1, true);
    hide(st, MODE1, true);
    tooltip_in(st, DIALOG, OK, 0x353);
    tooltip_in(st, DIALOG, 0xacf, 0x35d);
    tooltip_in(st, DIALOG, 0xacb, 0x36b);
    text(st, 0xacf, time(rec.tick));
    text(st, 0xacb, strings(0x36b));
    text(st, 0xacc, strings(0x360));
    text(st, 0xacd, strings(0x35d));
    tooltip_in(st, DIALOG, 0xace, 0x360);
    text(st, 0xace, map_name(&rec.path));
    if !random_map {
        hide(st, SAVE_MAP, true);
    }
    // Eight goods: the seventh is the last of the object, the eighth reads past it [unk].
    for k in 0..8usize {
        let link = f(124 + k as i64);
        let good = goods.get(k).map(|g| g.0);
        hide(st, link, false);
        let icon = good.and_then(|g| usize::try_from(g).ok()).and_then(|g| ICONS.get(g)).copied().unwrap_or(314);
        st.images.insert((DIALOG, link), (icon, 0));
        let (tip, ext) = good.and_then(|g| usize::try_from(g).ok()).and_then(|g| TIPS.get(g)).copied().unwrap_or((0, 0));
        let old = st.tooltips.get(&(DIALOG, link)).copied().unwrap_or((0, 0));
        st.tooltips.insert((DIALOG, link), (if tip >= 0 { tip as u16 } else { old.0 }, if ext >= 0 { ext as u16 } else { old.1 }));
    }
    let win = |st: &mut GuiState, i: usize| {
        let link = f(83 + i as i64);
        let (img, tip) = if !rec.decided {
            (0xfb, 0x35e)
        } else if rec.team[i] == rec.winner {
            (0xfd, 0x36c)
        } else {
            (0xfc, 0x35f)
        };
        st.images.insert((DIALOG, link), (img, 0));
        tooltip_in(st, DIALOG, link, tip);
    };
    if !mode1 {
        for t in 1..3 {
            let Some(i) = (1..9).find(|&i| rec.team[i] == t) else { continue };
            let r = i as i64;
            hide(st, f(115 + r), false);
            hide(st, f(107 + r), true);
            hide(st, f(99 + r), true);
            hide(st, f(91 + r), false);
            hide(st, f(83 + r), false);
            text(st, f(115 + r), format!("Team {t}"));
            st.images.insert((DIALOG, f(107 + r)), (f(76 + rec.colour[i] as i64), 0));
            st.images.insert((DIALOG, f(91 + r)), (f(62 + rec.team[i] as i64), 0));
            win(st, i);
            for (j, &(_, a, b)) in goods.iter().enumerate() {
                let (own, other) = if t == 1 { (a, b) } else { (b, a) };
                let link = f(7 * r + j as i64);
                hide(st, link, false);
                st.fonts.insert((DIALOG, link), if other <= own { 1 } else { 2 });
                text(st, link, (own as u32).to_string());
            }
        }
        for r in 3..9i64 {
            for l in [f(115 + r), f(107 + r), f(99 + r), f(91 + r), f(83 + r), f(8 * r)] {
                hide(st, l, true);
            }
            for j in 0..7 {
                hide(st, f(7 * r + j), true);
            }
        }
        return;
    }
    for i in 1..9usize {
        let r = i as i64;
        if i as i32 > rec.players {
            for l in [f(115 + r), f(107 + r), f(99 + r), f(91 + r), f(83 + r)] {
                hide(st, l, true);
            }
            for j in 0..7 {
                hide(st, f(7 * r + j), true);
            }
            continue;
        }
        for l in [f(115 + r), f(107 + r), f(99 + r), f(91 + r), f(83 + r)] {
            hide(st, l, false);
        }
        for (l, tip) in [(f(115 + r), 0x368), (f(107 + r), 0x355), (f(99 + r), 0x369), (f(91 + r), 0x36a)] {
            tooltip_in(st, DIALOG, l, tip);
        }
        text(st, f(115 + r), rec.names[i].clone());
        st.images.insert((DIALOG, f(107 + r)), (f(76 + rec.colour[i] as i64), 0));
        st.images.insert((DIALOG, f(99 + r)), (f(71 + rec.race[i] as i64), 0));
        st.images.insert((DIALOG, f(91 + r)), (f(62 + rec.team[i] as i64), 0));
        win(st, i);
        for (j, &(good, _, _)) in goods.iter().enumerate() {
            let at = |p: usize| rec.stats.get(p).map_or(0, |s| s.at(0xa04 + 4 * good as usize));
            let own = at(i);
            let best = (1..=rec.players.max(0) as usize).filter(|&p| p != i).map(at).fold(-1, i32::max);
            let link = f(7 * r + j as i64);
            hide(st, link, false);
            st.fonts.insert((DIALOG, link), if best == own { 1 } else { 2 });
            text(st, link, (own as u32).to_string());
        }
    }
}
