//! D41 (DLG_VEHICLELOADCONTEXT), the dialog of a selected cart (info code 0x15) or ship (0x16,
//! 0x18), as the shipped exe fills and runs it (own RE; `tools/re/gui-2026-10-22/d41_emu.py`
//! runs the original update in unicorn, `tests/d41_vectors.rs` checks this port against it).
//!
//! * Opening: `CInputProcessor::RequestDialog` (0x4ee730) calls vtbl+0x94 of a single selected
//!   cart (land vehicle type 5) or ship of type 2 / 3; the cart's is 0x55bdb0: info object
//!   0x11aa918 (+4 code 0x15), event 0x25a (0x25c refreshes). `CreateUITab` (0x4841e0) opens
//!   dialog 41 with the procedure 0x4ae300 for the codes 0x15 / 0x16 / 0x18 (0x17 is dialog 42).
//! * The cart's fill 0x55bdb0: +0xf8 "may found a settlement" (a foundation cart with room,
//!   0x55da00), +0xf9 foundation cart (+0xbc), +0xfa "may become a foundation cart" (own land,
//!   no foundation cart, the eco-sector's stock 0x55c2d0); a trade cart: settlers 0 / 0 and its six
//!   slots {good, amount, _}; a foundation cart: its pioneers + carriers / the configured 10 and
//!   its goods {needed, good, arrived} [the field order as audio's `cart::dialog` gives it].
//! * The update 0x4ad9a0 (also the procedure's code 0): see [`update`].
//! * The procedure 0x4ae300 code 3: 0x6b1 → UI 0x1ab (centre on it, 0x4e9cc0), 0x6b5 → 0x1a1
//!   (net 5009 `VehicleUnload` for each selected, 0x4ed0f0), 0x6d6 → 0x1a3 (5012
//!   `CreateFoundationCart`, first selected, 0x4ece80), 0x6d7 → 0x1a5 (5011 `CreateSettlement`,
//!   0x4ed030), 0x6d8 → 0x1c0 (5049 `DestroyVehicle` for each selected, then deselect and 0x25b,
//!   0x4ee940); see [`proc_event`].

use crate::guistate::GuiState;

/// The dialog slot.
pub const DIALOG: usize = 41;

/// The rows' (text, icon) links (.rdata 0xffff58).
const ROWS: [(u16, u16); 15] = [
    (1718, 1734), (1719, 1735), (1720, 1736), (1721, 1737), (1722, 1738), (1723, 1739), (1724, 1740), (1725, 1741), (1726, 1742),
    (1727, 1743), (1728, 1744), (1729, 1745), (1730, 1746), (1731, 1747), (1732, 1748),
];

/// The goods' (tooltip, extended tooltip) (.rdata 0x10002ac, stride 0x14; good 43 reads past the
/// table: its words as the release stores them).
const GOOD_TIPS: [(u16, u16); 44] = [
    (0, 0), (1610, 1611), (1612, 1613), (1614, 1615), (1616, 1617), (1618, 1619), (1620, 1621), (1662, 1663), (1622, 1623), (1624, 1625),
    (1626, 1627), (1632, 1633), (1634, 1635), (1636, 1637), (1640, 1641), (1638, 1639), (1642, 1643), (1664, 1665), (1644, 1645),
    (1646, 1647), (1650, 1651), (1648, 1649), (1652, 1653), (1654, 1655), (1656, 1657), (1658, 1659), (1660, 1661), (1630, 1631),
    (1666, 1667), (1668, 1669), (1670, 1671), (1672, 1673), (1674, 1675), (1676, 1677), (1678, 1679), (1680, 1681), (1682, 1683),
    (1684, 1685), (271, 272), (274, 275), (269, 270), (276, 277), (278, 279), (0x2d00, 0x2dc0),
];

/// The picture of the dialog by race (.rdata 0x1000000 + race · 0x8c: +0xc ship 0x16, +0x34 the
/// cart, +0x48 ship 0x18).
const PICTURES: [(u16, u16, u16); 5] = [(602, 616, 617), (959, 972, 974), (973, 985, 988), (0, 0, 0), (1004, 1008, 999)];

fn dword(info: &[u8], off: usize) -> u32 {
    info.get(off..off + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn byte(info: &[u8], off: usize) -> u8 {
    info.get(off).copied().unwrap_or(0)
}

/// The update 0x4ad9a0 on info object `info` (from +0, code at +4) for the local `race`.
pub fn update(info: &[u8], race: u8, st: &mut GuiState, strings: &dyn Fn(u32) -> String) {
    let d = DIALOG;
    let code = dword(info, 4);
    let pic = PICTURES[(race as usize).min(4)];
    let foundation = byte(info, 0xf9) != 0;
    let text = |st: &mut GuiState, link: u16, t: String| {
        st.texts.insert((d, link), t);
    };
    match code {
        0x16 => {
            for link in [0x6d7, 0x6d6, 0x6b5] {
                st.set_hidden(d, link, true);
            }
            st.images.insert((d, 0x6b4), (pic.0, 0));
            text(st, 0x6b2, strings(0x8c6));
            st.set_hidden(d, 0x6d8, true);
        }
        0x18 => {
            for link in [0x6d7, 0x6d6, 0x6b1, 0x6b5] {
                st.set_hidden(d, link, true);
            }
            st.images.insert((d, 0x6b4), (pic.2, 0));
            text(st, 0x6b2, strings(0x372));
            st.set_hidden(d, 0x6d8, true);
        }
        _ => {
            st.set_hidden(d, 0x6b5, byte(info, 0xfb) == 0);
            if code == 0x15 {
                st.set_hidden(d, 0x6d6, false);
                st.images.insert((d, 0x6d6), if byte(info, 0xfa) == 0 { (0x112, 0x113) } else { (0x110, 0x111) });
            } else {
                st.set_hidden(d, 0x6d6, byte(info, 0xfa) == 0);
            }
            st.set_hidden(d, 0x6d7, byte(info, 0xf8) == 0);
            st.images.insert((d, 0x6b4), (pic.1, 0));
            text(st, 0x6b2, strings(if foundation { 0x373 } else { 0x8b6 }));
            st.set_hidden(d, 0x6d8, false);
        }
    }
    let pair = |a: u32, b: u32| if foundation { format!("{a}/{b}") } else { a.to_string() };
    for (r, &(t, icon)) in ROWS.iter().enumerate() {
        let off = 8 + 12 * r;
        let good = dword(info, off) as i32;
        if good < 1 {
            st.set_hidden(d, t, true);
            st.set_hidden(d, icon, true);
            continue;
        }
        text(st, t, pair(dword(info, off + 4), dword(info, off + 8)));
        st.images.insert((d, icon), (crate::building_dialog::good_icon(good), 0));
        let tips = GOOD_TIPS.get(good as usize).copied().unwrap_or_default();
        st.tooltips.insert((d, icon), tips);
        st.set_hidden(d, icon, false);
        st.set_hidden(d, t, false);
    }
    text(st, 0x6c5, pair(dword(info, 0xfc), dword(info, 0x100)));
    let ship = code == 0x16 || code == 0x18;
    st.set_hidden(d, 0x6c5, ship);
    st.set_hidden(d, 0x6d5, ship);
}

/// Code 3 on `link`: the UI event the procedure sends.
pub fn proc_event(link: u16) -> Option<u16> {
    Some(match link {
        0x6b1 => 0x1ab,
        0x6b5 => 0x1a1,
        0x6d6 => 0x1a3,
        0x6d7 => 0x1a5,
        0x6d8 => 0x1c0,
        _ => return None,
    })
}
