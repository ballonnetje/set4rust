//! The in-game GUI pages filled from the game state: what `CLogic::SetFillDialogInfos` refreshes
//! (the `Fill*` functions into a `CInfoExchange`, then the pages' `UpdateGuiDlg*`), and the build
//! pages' cost display on hover (`Set*BuildCosts`, procedure code 5 / 6).
//!
//! Only values the simulation has are shown; the others stay empty. The statistic pages share one
//! mode (`GuiState::fill_global`): local = the eco-sector at the screen centre, global = the
//! player's totals.
//! * build pages (dialogs 9..=13): per type of the tab's list (`GuiDlgBuildSubmenuProc` 0x4728a0,
//!   by race) the finished count and "+n" sites (0x53d5a0; local mode: those whose door is in
//!   the centre eco-sector, 0x53f170, nothing outside eco-sectors), costs on hover (code 5: list entry bytes +12 / +13 / +14 = boards / stone / gold
//!   from buildingInfo +3 / +2 / +4 into the cost links, 0x46d200 / 0x4711a0 / 0x46fba0 / 0x473380
//!   / 0x471ec0; code 6 and other links: zeros);
//! * extension bar (slot 1, fill 0x4f8b30 → update 0x47c560, see [`ext_bar`]): the player's
//!   `CStatistic` goods (boards, stone, weapons, gold, food), free carriers, beds, soldiers, the
//!   fighting strength and the mana (`CStatistic +0x1060`);
//! * settler statistic (dialog 31, update 0x4a4db0, fill 0x584c40): per type of the tab's list
//!   (0x475150: tab 0x2EB, entries at 0x11aaf58) the player's settlers (soldier ranks summed, the
//!   warrior types always global), else in local mode the centre eco-sector's; the beds and the
//!   working / striking carriers;
//! * available goods (dialog 27, init 0x46bea0, update 0x46be10, fill 0x4df820): local mode, the
//!   centre eco-sector's piles + carriers; global, the player's `CStatistic` goods.
//!
//! "The centre eco-sector" is `GetCurrentEcoSectorIndex` (0x4dfde0, see
//! [`crate::population::Population::current_eco_at_centre`]): the local player's, 0 = none.
//!
//! Verified in the shipped exe: everything above, the build lists, every count / "+n" link of
//! pages 9..=13 (the `UpdateGuiDlgBuild*` switch tables; page 9's at 0x46d1b0 with the byte index
//! 0x46d1d4; `tools/re/build_page_links.py` dumps them), the hover indices (the procs' code-5
//! switches 0x46d2b0 / 0x471250 / 0x46fc50 / 0x473430 / 0x471f70), the context dialogs (0x4ee730)
//! and their soldier / specialist tables (0x4a6810 / 0x585320, 0x4a72d0 / 0x585530).

use std::collections::HashMap;

use crate::guistate::GuiState;
use crate::population::{Economy, Population};

/// (page dialog, building type, count link, plus link): `UpdateGuiDlgBuild*` / `Set*Properties`.
const COUNT_LINKS: &[(usize, u8, u16, u16)] = &[
    (9, 1, 380, 389),
    (9, 2, 376, 388),
    (9, 3, 374, 387),
    (9, 4, 378, 390),
    (9, 14, 402, 403),
    (9, 40, 386, 393),
    (9, 41, 384, 391),
    (9, 42, 385, 392),
    (10, 15, 477, 487),
    (10, 16, 475, 486),
    (10, 17, 479, 488),
    (10, 18, 496, 497),
    (10, 19, 483, 489),
    (10, 20, 485, 491),
    (10, 21, 484, 490),
    (10, 22, 493, 494),
    (10, 25, 2902, 2903),
    (11, 5, 447, 448),
    (11, 6, 437, 443),
    (11, 7, 438, 444),
    (11, 8, 439, 445),
    (11, 9, 429, 440),
    (11, 10, 449, 450),
    (11, 11, 433, 442),
    (11, 12, 431, 441),
    (11, 35, 2033, 2034),
    (11, 36, 452, 453),
    (11, 37, 454, 455),
    (11, 38, 2896, 2897),
    (11, 39, 2898, 2899),
    (11, 81, 2878, 2879),
    (11, 82, 2874, 2875),
    (12, 13, 542, 550),
    (12, 31, 549, 553),
    (12, 32, 554, 555),
    (12, 33, 544, 551),
    (12, 34, 546, 552),
    (12, 43, 559, 560),
    (12, 44, 561, 562),
    (13, 23, 527, 528),
    (13, 24, 510, 519),
    (13, 27, 515, 521),
    (13, 28, 514, 520),
    (13, 45, 523, 524),
    (13, 46, 508, 518),
    (13, 47, 506, 517),
    (13, 48, 516, 522),
];

/// The ten-entry building list of a page for a race (`GuiDlgBuildSubmenuProc` tabs 536..=540;
/// race 1 Viking, 2 Maya, 4 Trojan, else Roman).
pub fn build_list(page: usize, race: u8) -> [u8; 10] {
    match page {
        9 => [3, 2, 4, 1, 40, 41, 42, 14, 0, 0],
        10 => [15, 20, 17, 16, 18, 19, 21, 22, if race == 1 { 25 } else { 0 }, 0],
        11 => {
            let (a, b) = match race {
                1 => (38, 39),
                2 => (36, 37),
                4 => (82, 81),
                _ => (35, 0),
            };
            [11, 12, 9, 10, 8, 6, 7, 5, a, b]
        }
        12 => [34, 33, 13, 32, 31, 30, 43, 44, 0, 0],
        _ => [24, 46, 47, 45, 48, 27, 23, 0, 0, if race == 1 { 0 } else { 28 }],
    }
}

/// (page, hovered link → list index) and the page's three cost links (boards, stone, gold):
/// the pages' code 5 and `Set*BuildCosts`.
fn hover_index(page: usize, link: u16) -> Option<usize> {
    let i = match (page, link) {
        (9, 373) => 0,
        (9, 375) => 1,
        (9, 377) => 2,
        (9, 379) => 3,
        (9, 381) => 4,
        (9, 382) => 5,
        (9, 383) => 6,
        (9, 401) => 7,
        (10, 474) => 0,
        (10, 476) => 1,
        (10, 478) => 2,
        (10, 480) => 3,
        (10, 498) => 4,
        (10, 481) => 5,
        (10, 482) => 6,
        (10, 492) => 7,
        (10, 2904) => 8,
        (11, 432) => 0,
        (11, 428) => 1,
        (11, 434) => 2,
        (11, 451) => 3,
        (11, 430) => 4,
        (11, 435) => 5,
        (11, 436) => 6,
        (11, 446) => 7,
        (11, 2035 | 456 | 2877 | 2900) => 8,
        (11, 457 | 2876 | 2901) => 9,
        (12, 545) => 0,
        (12, 541) => 1,
        (12, 547) => 2,
        (12, 556) => 3,
        (12, 543) => 4,
        (12, 557) => 6,
        (12, 558) => 7,
        (13, 509) => 0,
        (13, 505) => 1,
        (13, 511) => 2,
        (13, 525) => 3,
        (13, 507) => 4,
        (13, 513) => 5,
        (13, 526) => 6,
        (13, 512) => 9,
        _ => return None,
    };
    Some(i)
}

fn cost_links(page: usize) -> Option<[u16; 3]> {
    Some(match page {
        9 => [398, 399, 400],
        10 => [502, 503, 504],
        11 => [459, 460, 461],
        12 => [567, 568, 569],
        13 => [530, 531, 532],
        _ => return None,
    })
}

/// The settler statistic list (`GuiDlgFiguresSubmenuProc` 0x2EB: 36 types, by race) and its text
/// links (release .rdata 0xffeb98).
fn settler_list(race: u8) -> [u16; 36] {
    let (a, b, c, d) = match race {
        1 => (25, 26, 21, 38),
        2 => (27, 28, 22, 41),
        4 => (64, 65, 22, 61),
        _ => (24, 22, 0, 35),
    };
    [1, 2, 3, 4, 6, 12, 14, 5, 13, 15, 8, 7, 11, 18, 9, 16, 17, 10, 19, 23, 56, 20, a, b, c, 0, 50, 49, 48, 47, 46, 29, 32, d, 44, 45]
}

const SETTLER_LINKS: [u16; 36] = [
    1283, 1316, 1317, 1318, 1319, 1320, 1321, 1322, 1323, 1324, 1325, 1326, 1327, 1328, 1329, 1330, 1331, 1332, 1333, 1334, 1335, 1336,
    1337, 1338, 1339, 1339, 1340, 1341, 1342, 1343, 1344, 1345, 1346, 1347, 1348, 1349,
];

/// Good → text link of dialog 27 (the goods page init, release 0x46bea0, fills the table at
/// 0x15aa510).
fn goods_link(good: usize) -> Option<u16> {
    const T: [(usize, u16); 42] = [
        (22, 293), (7, 294), (32, 295), (10, 296), (21, 297), (20, 298), (15, 299), (14, 300), (4, 301), (18, 302), (25, 303), (27, 304),
        (29, 305), (31, 306), (28, 307), (2, 308), (3, 309), (5, 2893), (34, 310), (8, 311), (6, 312), (17, 313), (33, 314), (1, 315),
        (9, 316), (11, 317), (12, 318), (16, 319), (19, 2894), (23, 2895), (24, 320), (35, 321), (36, 322), (37, 2028), (13, 325),
        (26, 327), (30, 2030), (39, 2032), (40, 2867), (38, 2871), (41, 2872), (42, 2873),
    ];
    T.iter().find(|t| t.0 == good).map(|t| t.1)
}

/// `m_uSettlerWarriorTypes[t] != 0` (release 0x12dbca0, filled at 0x581f20 from the table at file
/// offset 0xc19a48): the settler statistic counts these globally in both modes.
fn is_warrior(t: u16) -> bool {
    matches!(t, 29..=50 | 52..=55 | 60..=63)
}

fn set(st: &mut GuiState, d: usize, link: u16, t: String) {
    st.texts.insert((d, link), t);
}

fn set_visible(st: &mut GuiState, d: usize, link: u16, visible: bool) {
    let Some(i) = st.set.dialog(d).and_then(|x| x.controls.iter().position(|c| c.link == link)) else { return };
    if let Some(e) = st.effects.get_mut(&(d, i)) {
        *e = if visible { *e & !8 } else { *e | 8 };
    }
}

/// `IGuiEngine::EnableControl`: effects bit 4 (greyed) when disabled.
fn set_enabled(st: &mut GuiState, d: usize, link: u16, enabled: bool) {
    let Some(i) = st.set.dialog(d).and_then(|x| x.controls.iter().position(|c| c.link == link)) else { return };
    if let Some(e) = st.effects.get_mut(&(d, i)) {
        *e = if enabled { *e & !4 } else { *e | 4 };
    }
}

/// The mode buttons show the shared mode: pressed = the player's totals.
fn show_mode(st: &mut GuiState, d: usize, link: u16) {
    let Some(i) = st.set.dialog(d).and_then(|x| x.controls.iter().position(|c| c.link == link)) else { return };
    let global = st.fill_global;
    if let Some(e) = st.effects.get_mut(&(d, i)) {
        *e = if global { *e | 1 } else { *e & !1 };
    }
}

/// The eco-sectors around the camera node (`client::camera_node`: `CStateGame +0x44 / +0x48`,
/// which stands for the 0x519a70 anchor, not the screen centre) that the pages' fills read
/// (0 = none).
#[derive(Clone, Copy, Debug, Default)]
pub struct Centre {
    /// At the centre node (`GetLocalPlayerEcoSectorIdAtCenter`).
    pub eco: u16,
    /// The player's first on the 100-entry spiral around it (0x585870, `Economy::own_at_centre`).
    pub own: u16,
    /// `GetCurrentEcoSectorIndex` (0x4dfde0, `Population::current_eco_at_centre`).
    pub current: u16,
}

/// Refresh the open pages from `pop` for `local` (the player id) with the eco-sectors `at` the
/// screen centre.
pub fn update(st: &mut GuiState, pop: &Population, local: u8, at: Centre, strings: &dyn Fn(usize) -> String) {
    let Centre { eco: _, own: own_eco, current: current_eco } = at;
    let race = pop.races.get(local as usize).copied().unwrap_or(0);
    let eco = &pop.economy;
    let open = st.open.clone();
    for page in open {
        match page {
            1 => ext_bar(st, eco, local),
            9..=13 => {
                if page == 12 {
                    show_mode(st, 12, 563);
                }
                // Local mode (0x53d5a0 → 0x53f170): the player's buildings whose door is in the
                // eco-sector `GetCurrentEcoSectorIndex` finds; none without one.
                let global = st.fill_global;
                let sector_of = |b: &crate::population::Building| eco.eco_of.get(b.door.1 as usize * pop.size + b.door.0 as usize).copied().unwrap_or(0);
                for t in build_list(page, race) {
                    let Some(&(_, _, count, plus)) = COUNT_LINKS.iter().find(|c| c.0 == page && c.1 == t) else { continue };
                    let mine = pop.buildings.iter().filter(|b| b.owner == local && b.kind == t && (global || (current_eco != 0 && sector_of(b) == current_eco)));
                    let (done, sites) = mine.fold((0, 0), |(d, s), b| if b.site.is_some() { (d, s + 1) } else { (d + 1, s) });
                    set(st, page, count, done.to_string());
                    set(st, page, plus, format!("+{sites}"));
                    set_visible(st, page, plus, sites > 0);
                }
            }
            31 => settlers_page(st, eco, local, race, current_eco),
            27 => goods_page(st, eco, local, current_eco),
            32 => free_carrier_page(st, eco, own_eco, local),
            30 => specialist_page(st, eco, own_eco, local, race),
            25 => transport_page(st, eco, current_eco),
            58 => {
                st.page_info.chat.set_players(pop, local);
                chat_page(st);
            }
            26 => distribution_page(st, eco, current_eco, local, race, strings),
            _ => {}
        }
    }
}

/// The player's statistics (index 0 for none).
fn player(eco: &Economy, local: u8) -> crate::population::PlayerInfo {
    eco.players.get(local as usize).cloned().unwrap_or_default()
}

/// `wsprintfA("%u", v)`.
fn u(v: i32) -> String {
    (v as u32).to_string()
}

/// The extension bar (dialog 1): the fill 0x4f8b30 (info 0x11aad48, code 0x25) and the update
/// 0x47c560. `CStatistic` goods (+0xa04 + 4 · good): 622 boards (7), 623 stone (32), 625 gold
/// (14), 624 the weapons 38 + 5 + 6 + 8 + 34, 630 the foods 11 + 24 + 9; 631 `Σ max(0, +0x26 −
/// +0x20)` over the player's eco-sectors (list 0x149b870); 632 the beds (0x4df730); 633 the
/// player's settlers of types 29..=44 and 61..=63 (`CSettlerMgr` 0x1545658); 637 / 638 the
/// strength on foreign / own land (+0xbc0 / +0xbbc) · 100 >> 8, "%u%%"; 639 the mana, "%u" of
/// `CStatistic +0x1060` (read at 0x4f8d47: the copy of the pool `Statistic::Update` makes,
/// 0x4f87f0).
fn ext_bar(st: &mut GuiState, eco: &Economy, local: u8) {
    let p = player(eco, local);
    let g = |list: &[usize]| list.iter().map(|&k| p.goods[k]).sum::<i32>();
    let free: i64 = eco.sectors.iter().filter(|e| e.owner == local).map(|e| (e.settlers[1] as i64 - e.min_carrier as i64).max(0)).sum();
    let all = eco.settlers.get(local as usize).copied().unwrap_or([0; 67]);
    let soldiers: u32 = (29..=44).chain(61..=63).map(|t| all[t]).sum();
    for (link, v) in [(622, g(&[7])), (623, g(&[32])), (624, g(&[38, 5, 6, 8, 34])), (625, g(&[14])), (630, g(&[11, 24, 9]))] {
        set(st, 1, link, u(v));
    }
    set(st, 1, 631, (free as u32).to_string());
    set(st, 1, 632, u(p.beds));
    set(st, 1, 633, soldiers.to_string());
    set(st, 1, 637, format!("{}%", u((p.strength.1 * 100) >> 8)));
    set(st, 1, 638, format!("{}%", u((p.strength.0 * 100) >> 8)));
    set(st, 1, 639, u(p.stat_mana));
}

fn settlers_page(st: &mut GuiState, eco: &Economy, local: u8, race: u8, centre_eco: u16) {
    show_mode(st, 31, 1350);
    let global = st.fill_global;
    let all = eco.settlers.get(local as usize).copied().unwrap_or([0; 67]);
    let here = eco.sector(centre_eco).filter(|e| e.owner == local);
    for (i, &t) in settler_list(race).iter().enumerate() {
        if t == 0 {
            continue;
        }
        let n: u32 = match t {
            29..=31 => all[29] + all[30] + all[31],
            32..=34 => all[32] + all[33] + all[34],
            35..=37 => all[35] + all[36] + all[37],
            38..=40 => all[38] + all[39] + all[40],
            41..=43 => all[41] + all[42] + all[43],
            61..=63 => all[61] + all[62] + all[63],
            _ if global || is_warrior(t) => all[t as usize],
            _ => here.map_or(0, |e| e.settlers[t as usize] as u32),
        };
        set(st, 31, SETTLER_LINKS[i], n.to_string());
    }
    // 0x584c40's totals: +0xc the beds (0x4df730) → 1355; +0x10 / +0x14 the initial free beds
    // and the residences' ("(%u/%u)") → 1356; +0x18 / +0x1c the player's carriers less the
    // strikers and the strikers → 1361 / 1362; +0x20 / +0x24 the same globally, in local mode the
    // eco-sector's carriers less its strikers (0x4ddf40) and those → 1283 / 1357.
    let p = player(eco, local);
    let carriers = all[1] as i32;
    set(st, 31, 1355, u(p.beds));
    set(st, 31, 1356, format!("({}/{})", u(p.initial_beds), u(p.beds - p.initial_beds)));
    set(st, 31, 1361, u(carriers - p.strikers));
    set(st, 31, 1362, u(p.strikers));
    let (working, striking) = if !global {
        here.map_or((0, 0), |e| (e.settlers[1] as i32 - e.strikers as i32, e.strikers as i32))
    } else {
        (carriers - p.strikers, p.strikers)
    };
    set(st, 31, 1283, u(working));
    set(st, 31, 1357, u(striking));
}

/// Goods 0x1f (shovel) and 0x12 (hammer): the diggers' and builders' tools.
const SHOVEL: usize = 0x1f;
const HAMMER: usize = 0x12;

/// The free carrier page (dialog 32, `GuiDlgFreeCarrierMenuProc` 0x475dc0): the logic fill
/// 0x585190 into the info at 0x11aaee4, shown by 0x475690. Checked by running 0x475690 in the
/// emulator (`tools/re/viewer-2026-10-19/recruit/emu_fills.py`):
/// * the - / + buttons 755..=759 are enabled while there is an own eco-sector (`+0x34`);
/// * 752 the carriers over the minimum (`+0x26 - +0x20`), hidden when < 1; 766 the carriers and
///   767 the minimum, each hidden when < 1;
/// * 762 / 763 the diggers / builders (`+0x28` / `+0x2a`), 772 / 773 their worker deltas
///   (`%d`, `+%d` from 0), 775 / 774 the offered shovels / hammers (0x4d4280);
/// * 754 / 753 image 437 / 443 with a shovel / hammer offered, else 500 / 502.
///
/// Without an own eco-sector the fill zeroes everything but the two tool counts, which keep the
/// last values (`+0x20`, `+0x30` are not written).
fn free_carrier_page(st: &mut GuiState, eco: &Economy, own_eco: u16, local: u8) {
    let here = eco.sector(own_eco).filter(|e| e.owner == local);
    let (carriers, min, diggers, ddelta, builders, bdelta) = match here {
        Some(e) => {
            let delta = |t: u16| s4_sim::eco::WORKERS.iter().position(|&w| w == t).map_or(0, |i| e.worker_delta[i] as i32);
            (e.settlers[1] as i32, e.min_carrier as i32, e.settlers[2] as i32, delta(2), e.settlers[3] as i32, delta(3))
        }
        None => (0, 0, 0, 0, 0, 0),
    };
    if let Some(e) = here {
        st.page_info.tools = (e.offered[SHOVEL], e.offered[HAMMER]);
    }
    let (shovels, hammers) = st.page_info.tools;
    let has = |n: i32| here.is_some() && n > 0;
    for l in 755..=759 {
        set_enabled(st, 32, l, here.is_some());
    }
    let free = carriers - min;
    set_visible(st, 32, 752, free >= 1);
    if free >= 1 {
        set(st, 32, 752, free.to_string());
    }
    for (link, v) in [(762, diggers), (763, builders)] {
        set_visible(st, 32, link, true);
        set(st, 32, link, v.to_string());
    }
    for (link, v) in [(772, ddelta), (773, bdelta)] {
        set_visible(st, 32, link, true);
        set(st, 32, link, signed(v));
    }
    set(st, 32, 775, shovels.to_string());
    set(st, 32, 774, hammers.to_string());
    for (link, v) in [(766, carriers), (767, min)] {
        set_visible(st, 32, link, v >= 1);
        if v >= 1 {
            set(st, 32, link, v.to_string());
        }
    }
    st.images.insert((32, 753), (if has(hammers) { 0x1bb } else { 0x1f6 }, 0));
    st.images.insert((32, 754), (if has(shovels) { 0x1b5 } else { 500 }, 0));
}

/// The specialist page (dialog 30, `GuiDlgSettlerProductionMenuProc` 0x4a4ae0): the logic fill
/// 0x584fb0 into the info at 0x11aac20 (types 48, 50, 49, 46, 47 from .rdata 0x1071400, each with
/// the player's count, the wish of the eco-sector, whether its tool is offered and how many),
/// shown by 0x4a4250 (checked in the emulator, `recruit/emu_fills.py`): the buttons 1268..=1272
/// enabled with an own eco-sector, 1262 its free carriers (`max(0, +0x26 - +0x20)`), per type
/// (count, wish, tools, tool image with / without a tool):
/// 50 (1256, 1275, 1278, 1264: 437 / 500), 49 (1257, 1274, 1279, 1263: 443 / 502),
/// 48 (1258, 1276, no tool), 47 (1259, 1273, 1280, 1265: 437 / 500),
/// 46 (1260, 1277, 1282, 1266: 441 / 501). Without an own eco-sector only the free carriers and the
/// "has a tool" flags are cleared; the rest keeps the last fill (nothing before the first one).
fn specialist_page(st: &mut GuiState, eco: &Economy, own_eco: u16, local: u8, race: u8) {
    const TYPES: [u16; 5] = [48, 50, 49, 46, 47];
    let here = eco.sector(own_eco).filter(|e| e.owner == local);
    let mut free = 0;
    match here {
        Some(e) => {
            free = (e.settlers[1] as i32 - e.min_carrier as i32).max(0);
            let all = eco.settlers.get(local as usize).copied().unwrap_or([0; 67]);
            let tools = eco.settler_tool.get(race as usize).copied().unwrap_or([0; 67]);
            st.page_info.specialists = Some(TYPES.map(|t| {
                let wish = s4_sim::eco::SPECIALISTS.iter().position(|&s| s == t).map_or(0, |i| e.specialist_wish[i] as i32);
                let n = e.offered.get(tools[t as usize] as usize).copied().unwrap_or(0);
                (t, all[t as usize] as i32, wish, n > 0, n)
            }));
        }
        None => {
            if let Some(r) = &mut st.page_info.specialists {
                for x in r.iter_mut() {
                    x.3 = false;
                }
            }
        }
    }
    for l in 1268..=1272 {
        set_enabled(st, 30, l, here.is_some());
    }
    set(st, 30, 1262, free.to_string());
    let Some(recs) = st.page_info.specialists else { return };
    for (t, count, wish, has, tools) in recs {
        let (c, w, n, img, on, off) = match t {
            50 => (1256, 1275, 1278, 1264, 437, 500),
            49 => (1257, 1274, 1279, 1263, 443, 502),
            48 => (1258, 1276, 0, 0, 0, 0),
            47 => (1259, 1273, 1280, 1265, 437, 500),
            46 => (1260, 1277, 1282, 1266, 441, 501),
            _ => continue,
        };
        set(st, 30, c, count.to_string());
        set(st, 30, w, signed(wish));
        if n != 0 {
            set(st, 30, n, tools.to_string());
            st.images.insert((30, img), (if has { on } else { off }, 0));
        }
    }
}

/// .rdata 0xfffbe8 + good·0x14, fields +4..+0x10: the transport priority page's (image, pressed
/// image, tooltip, extended tooltip) of each good.
pub(crate) const TRANSPORT_GOOD: [(u16, u16, u16, u16); 43] = [
    (0, 0, 0, 0), (496, 497, 1610, 1611), (494, 495, 1612, 1613), (492, 493, 1614, 1615), (407, 394, 1616, 1617),
    (490, 491, 1618, 1619), (488, 489, 1620, 1621), (402, 388, 1662, 1663), (486, 487, 1622, 1623), (484, 485, 1624, 1625),
    (480, 481, 1626, 1627), (478, 479, 1632, 1633), (476, 477, 1634, 1635), (474, 475, 1636, 1637), (386, 387, 1640, 1641),
    (472, 473, 1638, 1639), (470, 471, 1642, 1643), (468, 469, 1664, 1665), (400, 397, 1644, 1645), (466, 467, 1646, 1647),
    (498, 499, 1650, 1651), (464, 465, 1648, 1649), (403, 393, 1652, 1653), (462, 463, 1654, 1655), (460, 461, 1656, 1657),
    (401, 389, 1658, 1659), (458, 459, 1660, 1661), (399, 396, 1630, 1631), (405, 391, 1666, 1667), (398, 390, 1668, 1669),
    (456, 457, 1670, 1671), (406, 392, 1672, 1673), (404, 395, 1674, 1675), (454, 455, 1676, 1677), (452, 453, 1678, 1679),
    (450, 451, 1680, 1681), (448, 449, 1682, 1683), (446, 447, 1684, 1685), (535, 534, 271, 272), (541, 540, 274, 275),
    (538, 537, 269, 270), (544, 543, 276, 277), (547, 546, 278, 279),
];

/// The transport priority page (dialog 25, `TransportPrioTabCallback` 0x4acf20).
/// * Fill 0x4dfae0 into the info at 0x11aaa38 (registered by 0x4ea4a0): with an eco-sector at
///   the centre (`GetCurrentEcoSectorIndex`) +0xc = its rank count (eco +0x280) and +0x10 the
///   goods in rank order (0x4e57e0 on eco +0x224: per slot the good with the smallest rank above
///   the last one, the later good on a tie; a slot with none keeps its old good). Without one only
///   +8 is cleared, so the page shows the last eco-sector's order.
/// * Display 0x4accc0 (info type 0x21): the slots' radio group released (0x6775a0 on 0x687),
///   then per slot `i < count` with a good: `SetImages`, the control's tooltips (+0x14 / +0x16),
///   the slot's good for the procedure, and the slot of the selected good pressed. Slots past the
///   count keep what they showed.
///
/// [inf] Slots 34.. would overwrite the selected good (0x15aad20 follows the 34 slot goods); the
/// rank counts are at most 30 (`s4_sim::eco::default_transport_priority`), so they never do.
fn transport_page(st: &mut GuiState, eco: &Economy, current: u16) {
    let t = &mut st.page_info.transport;
    if let Some(e) = eco.sector(current) {
        t.count = e.transport_count;
        let mut last = 0u16;
        for i in 0..(t.count.max(0) as usize).min(42) {
            let mut best: Option<(usize, u16)> = None;
            for (g, &r) in e.transport_prio.iter().enumerate() {
                if last < r && best.is_none_or(|(_, b)| r <= b) {
                    best = Some((g, r));
                }
            }
            if let Some((g, r)) = best {
                t.goods[i] = g as i32;
                last = r;
            }
        }
    }
    let t = st.page_info.transport.clone();
    st.release_group(25, 0x687);
    for (i, &g) in t.goods.iter().enumerate() {
        if g <= 0 || i as i32 >= t.count {
            continue;
        }
        let link = crate::guistate::TRANSPORT_LINKS[i];
        let (img, pressed, tip, ext) = TRANSPORT_GOOD[g as usize];
        st.images.insert((25, link), (img, pressed));
        st.tooltips.insert((25, link), (tip, ext));
        if let Some(x) = st.page_info.transport.link_goods.get_mut(i) {
            *x = g;
        }
        if g == t.target {
            st.press(25, link);
        }
    }
}

/// 0x49eb80 / 0x49eef0: the goods distribution page's (image, name string) of a building type per
/// race (Romans, Vikings, Mayas, Trojans; other races as the Romans), for the types with supply
/// priorities (`s4_sim::eco::default_supply`). Read by running the display in the emulator
/// (`tools/re/viewer-2026-10-20/goods/emu_supply.py --tables`, `supply_tables.txt`).
const SUPPLY_BUILDING: [(i32, [(u16, usize); 4]); 20] = [
    (3, [(0, 1064), (789, 1064), (0, 1064), (789, 1064)]),
    (9, [(838, 1098), (854, 1098), (859, 1098), (832, 1098)]),
    (10, [(836, 934), (863, 934), (861, 934), (776, 934)]),
    (12, [(835, 1066), (864, 1056), (862, 1028), (773, 273)]),
    (13, [(837, 948), (862, 948), (860, 948), (790, 948)]),
    (14, [(849, 1072), (869, 1072), (881, 1072), (861, 1072)]),
    (15, [(807, 1044), (829, 1044), (875, 1044), (817, 1044)]),
    (16, [(812, 1030), (836, 1030), (872, 1030), (802, 1030)]),
    (17, [(815, 946), (839, 946), (868, 946), (787, 946)]),
    (18, [(847, 1076), (865, 1076), (863, 1076), (866, 1076)]),
    (19, [(841, 1032), (857, 1032), (856, 1032), (853, 1032)]),
    (20, [(842, 1046), (858, 1046), (855, 1046), (856, 1046)]),
    (21, [(843, 1084), (859, 1084), (853, 1084), (872, 1084)]),
    (22, [(845, 1096), (860, 1096), (807, 1096), (882, 1096)]),
    (23, [(850, 1092), (870, 1092), (882, 1092), (877, 1092)]),
    (25, [(0, 944), (868, 944), (0, 944), (0, 944)]),
    (28, [(822, 932), (850, 932), (865, 1036), (770, 211)]),
    (31, [(840, 1068), (856, 1068), (857, 1068), (846, 1068)]),
    (37, [(785, 1082), (785, 1082), (785, 1082), (785, 1082)]),
    (39, [(861, 1054), (861, 1054), (861, 1054), (861, 1054)]),
];

/// The goods distribution page (dialog 26, `ProductionOrderMenuCallback` 0x476810).
/// * Fill `OnWhenGoodDistributionSelectionChanged` (0x4df9d0), the periodic provider once a tab
///   was clicked: the race; at `GetCurrentEcoSectorIndex` an eco-sector equal to the last one
///   (+0x40) changes nothing; otherwise the count is its buildings with a share of the good
///   (0x4c5290 on eco +0x218; 0 when the eco-sector is not the player's, or there is none), and
///   when the eco-sector differs from the last one (none = -1) it becomes the last one and the
///   display runs (event 0x25c).
/// * Display `UpdateUIGoodDistributionSliders` (0x476130, checked in the emulator,
///   `goods/emu_supply.py`): the tabs enabled with buildings; per slot `i < 5`, with a building:
///   its share as the slider value, image (783 + i) and name (800 + i) of the building, the image,
///   name, area (790 + i) and knob (795 + i) shown; without one those four hidden and value 0;
///   then `SetSliderPosition(795 + i, value)`.
fn distribution_page(st: &mut GuiState, eco: &Economy, current: u16, local: u8, race: u8, strings: &dyn Fn(usize) -> String) {
    let d = &mut st.page_info.distribution;
    d.race = race as i32;
    let id = match eco.sector(current) {
        Some(e) => {
            if d.last_eco == current as i32 {
                -2
            } else {
                d.count = if e.owner == local {
                    let l = e.supply_of(d.good);
                    for (i, &p) in l.iter().take(5).enumerate() {
                        d.list[i] = p;
                    }
                    l.len().min(5) as i32
                } else {
                    0
                };
                current as i32
            }
        }
        None => {
            d.count = 0;
            -1
        }
    };
    if id != -2 && d.last_eco != id {
        d.last_eco = id;
        d.show = true;
    }
    if !std::mem::take(&mut st.page_info.distribution.show) {
        return;
    }
    let release = std::mem::take(&mut st.page_info.distribution.release);
    let d = st.page_info.distribution.clone();
    for (l, _) in crate::guistate::DISTRIBUTION_TABS {
        set_enabled(st, 26, l, d.count > 0);
    }
    let r = match d.race {
        1 => 1,
        2 => 2,
        4 => 3,
        _ => 0,
    };
    for i in 0..5u16 {
        let v = if (i as i32) < d.count {
            let (b, share) = d.list[i as usize];
            st.page_info.distribution.values[i as usize] = share;
            let (img, name) = SUPPLY_BUILDING.iter().find(|x| x.0 == b).map_or((0, 0), |x| x.1[r]);
            st.images.insert((26, 783 + i), (img, 0));
            st.texts.insert((26, 800 + i), if name == 0 { String::new() } else { strings(name) });
            for l in [783, 800, 790, 795] {
                set_visible(st, 26, l + i, true);
            }
            share
        } else {
            for l in [783, 800, 795, 790] {
                set_visible(st, 26, l + i, false);
            }
            0
        };
        if !release {
            st.set_slider(26, 795 + i, v as u32);
        }
    }
}

/// The message recipients page's fill 0x473e90 (dialog 58; links by slot from .rdata 0xffc6a8:
/// name, radio, colour icon, kind icon): per player the name (font 11, 13 for another team), the
/// colour icon (.rdata 0xffc664 by colour), the kind icon (0xff0754 by kind) and the radio pressed
/// as it receives the messages; the local player's and the computers' radios greyed and hidden;
/// the rows past the players hidden; the three buttons enabled when a radio is.
fn chat_page(st: &mut GuiState) {
    const COLOUR: [u16; 9] = [505, 506, 507, 508, 509, 510, 511, 512, 829];
    const KIND: [u16; 8] = [319, 320, 318, 321, 322, 41, 42, 4];
    let c = st.page_info.chat.clone();
    st.page_info.chat.filling = true;
    let mut any = false;
    for i in 0..8u16 {
        let (name, radio, colour, kind) = (570 + i, 579 + i, 590 + i, 598 + i);
        let Some(s) = c.slots.get(i as usize) else {
            for l in [name, radio, colour, kind] {
                set_visible(st, 58, l, false);
            }
            continue;
        };
        for l in [name, radio, colour, kind] {
            set_visible(st, 58, l, true);
        }
        set(st, 58, name, s.name.clone());
        st.images.insert((58, colour), (COLOUR.get(s.colour as usize).copied().unwrap_or(0), 0));
        st.fonts.insert((58, name), if s.team != c.slots.get(c.local).map_or(0, |l| l.team) { 13 } else { 11 });
        st.images.insert((58, kind), (KIND.get(s.kind as usize).copied().unwrap_or(0), 0));
        st.release_group(58, radio);
        if c.receives(i as usize) {
            st.press(58, radio);
        }
        let off = i as usize == c.local || s.kind > 2;
        st.set_greyed(58, radio, off);
        if off {
            set_visible(st, 58, radio, false);
        } else {
            any = true;
        }
    }
    for l in [0x24b, 0x24c, 0x24d] {
        st.set_greyed(58, l, !any);
    }
    st.page_info.chat.filling = false;
}

/// `wsprintfA(v < 0 ? "%d" : "+%d", v)`.
fn signed(v: i32) -> String {
    if v < 0 { v.to_string() } else { format!("+{v}") }
}

/// The available goods (dialog 27): the fill 0x4df820 (info 0x11ab2d8, code 0x20), goods 1..=42:
/// global the player's `CStatistic` goods, local the centre eco-sector's piles (+0x290) plus
/// carriers (+0x2e6), 0 without one; the update 0x46be10 writes each "%u" on its link.
fn goods_page(st: &mut GuiState, eco: &Economy, local: u8, centre_eco: u16) {
    show_mode(st, 27, 323);
    let global = st.fill_global;
    let here = eco.sector(centre_eco);
    let p = player(eco, local);
    for good in 1..43 {
        let Some(link) = goods_link(good) else { continue };
        let n = if global { p.goods[good] } else { here.map_or(0, |e| e.goods_piles[good] as i32 + e.goods_carriers[good] as i32) };
        set(st, 27, link, u(n));
    }
}

/// The build pages' code 5 (cursor on a building button: its costs) and 6 (off: zeros).
/// `hovered` = the (dialog, link) under the cursor.
pub fn hover(st: &mut GuiState, costs: &HashMap<(u8, u8), (u8, u8, u8)>, race: u8, hovered: Option<(usize, u16)>) {
    for page in 9..=13 {
        if !st.open.contains(&page) {
            continue;
        }
        let Some(links) = cost_links(page) else { continue };
        let c = if let Some((_, link)) = hovered.filter(|h| h.0 == page) {
            // The cost lookup (0x53d5a0) reads types 31 / 32 as 58 / 52.
            let remap = |t: u8| match t {
                31 => 58,
                32 => 52,
                _ => t,
            };
            hover_index(page, link).map(|i| remap(build_list(page, race)[i])).and_then(|t| costs.get(&(race, t)).copied())
        } else {
            None
        };
        let (b, s, g) = c.unwrap_or((0, 0, 0));
        for (l, v) in links.iter().zip([b, s, g]) {
            set(st, page, *l, v.to_string());
        }
    }
}

/// buildingInfo costs per (race, type): (boards, stone, gold) = info bytes +3, +2, +4.
pub fn costs() -> HashMap<(u8, u8), (u8, u8, u8)> {
    let Ok(data) = s4_formats::gamedata::GameData::load_default() else { return HashMap::new() };
    data.buildings
        .iter()
        .map(|b| ((b.race as u8, b.building as u8), (b.boards.clamp(0, 255) as u8, b.stone.clamp(0, 255) as u8, b.gold.clamp(0, 255) as u8)))
        .collect()
}

// ------------------------------------------------------------------------------------------------
// Selection dialogs

/// The context dialog `CInputProcessor::RequestDialog` asks for, by the first selected entity:
/// a settler with warrior type 7 → specialists (44, `FillSpecialistMenu`, info type 19); type 60
/// (the donkey) → its role's vtbl+0x84 (0x577240: info code 0x18, D41, the building-dialog path,
/// `building_dialog::fill_donkey`): `None`; any other settler → soldiers (45, `FillSoldierMenu`,
/// info type 18). Vehicles: the group dialog 40 or a single ferry's 42
/// ([`crate::vehicle_group::request_dialog`]); carts and transport ships (D41) and buildings
/// have their own path: `None`.
pub fn context_dialog(pop: &Population, ids: &[u16]) -> Option<usize> {
    let first = *ids.first()?;
    if let Some(v) = pop.vehicles.iter().find(|u| u.id == first) {
        return crate::vehicle_group::request_dialog(v.obj, v.kind, ids.len());
    }
    let u = pop.settlers.iter().find(|u| u.id == first)?;
    match (u.flags & 0xF, u.kind) {
        (7, _) => Some(44),
        (_, 60) => None,
        _ => Some(45),
    }
}

/// `SHealthState` sums of the selected settlers of one type group: live points, missing live
/// points, count (`FillSoldierMenu` / `FillSpecialistMenu`).
#[derive(Clone, Copy, Default)]
struct Health {
    hp: u32,
    missing: u32,
    count: u32,
}

/// (text link, bar link) per slot of `g_cSoldierInfo` (`UpdateGuiDlgSoldierContext`, release
/// 0x4a6810: the loop passes group 1 with the rank table at 0xfff604, group 2 with 0xfff61c,
/// group 3 with 0xfff5ec): swordsmen ranks 1..3 (types 29..31), bowmen (32..34), the race's third
/// soldier (35..37, 38..40, 41..43, 61..63), then type 44 (1421 / 1436) and type 45 (1442 / 1443).
const SOLDIER_LINKS: [(u16, u16); 11] = [
    (1412, 1427),
    (1413, 1428),
    (1414, 1429),
    (1415, 1430),
    (1416, 1431),
    (1417, 1432),
    (1418, 1433),
    (1419, 1434),
    (1420, 1435),
    (1421, 1436),
    (1442, 1443),
];

fn soldier_slot(kind: u16) -> Option<usize> {
    Some(match kind {
        29..=31 => (kind - 29) as usize,
        32..=34 => 3 + (kind - 32) as usize,
        35..=37 => 6 + (kind - 35) as usize,
        38..=40 => 6 + (kind - 38) as usize,
        41..=43 => 6 + (kind - 41) as usize,
        61..=63 => 6 + (kind - 61) as usize,
        44 => 9,
        45 => 10,
        _ => return None,
    })
}

/// `g_cSpecialistsInfo` (`UpdateGuiDlgSpecialistsContext`): types 50, 49, 46, 47, 48.
const SPECIALIST_LINKS: [(u16, u16); 5] = [(1455, 1461), (1456, 1462), (1457, 1463), (1458, 1464), (1470, 1469)];

fn specialist_slot(kind: u16) -> Option<usize> {
    Some(match kind {
        50 => 0,
        49 => 1,
        46 => 2,
        47 => 3,
        48 => 4,
        _ => return None,
    })
}

/// Fill the open selection dialog from the selected settlers `ids` (`Fill*Menu`, then
/// `UpdateGuiDlg*Context` with `SetSoldierProperties` / `SetSpecialistProperties`: a group with
/// nobody is hidden, else its count and its health bar `100·hp / (hp + missing)`). The soldier
/// dialog (`FillSoldierMenu` 0x585320 → 0x4a6810) also shows the local player's strength on
/// foreign / own land · 100 >> 8 ("%u%%", 1445 / 1422) and `CStatistic +0x1060` (1448, always
/// 0). `titles` = the strings 885 / 886 the procedures' init sets.
pub fn fill_selection(st: &mut GuiState, pop: &Population, local: u8, ids: &[u16], titles: (&str, &str)) {
    type Slot = fn(u16) -> Option<usize>;
    let (dialog, links, slot): (usize, &[(u16, u16)], Slot) = if st.open.contains(&45) {
        (45, &SOLDIER_LINKS, soldier_slot)
    } else if st.open.contains(&44) {
        (44, &SPECIALIST_LINKS, specialist_slot)
    } else {
        return;
    };
    let mut groups = vec![Health::default(); links.len()];
    for u in pop.settlers.iter().filter(|u| ids.contains(&u.id)) {
        let Some(i) = slot(u.kind) else { continue };
        let hp = u.health.min(u.max_health) as u32;
        groups[i].hp += hp;
        groups[i].missing += u.max_health as u32 - hp;
        groups[i].count += 1;
    }
    for (g, &(text, bar)) in groups.iter().zip(links) {
        let shown = g.count > 0;
        set_visible(st, dialog, text, shown);
        set_visible(st, dialog, bar, shown);
        if shown {
            set(st, dialog, text, g.count.to_string());
            let total = g.hp + g.missing;
            st.bars.insert((dialog, bar), (100 * g.hp).checked_div(total).unwrap_or(0) as u8);
        }
    }
    if dialog == 45 {
        // EnableControl(45, 1444): only with type-45 settlers selected.
        let any = groups[10].hp != 0 || groups[10].missing != 0;
        if let Some(i) = st.set.dialog(45).and_then(|d| d.controls.iter().position(|c| c.link == 1444))
            && let Some(e) = st.effects.get_mut(&(45, i))
        {
            *e = if any { *e & !4 } else { *e | 4 };
        }
        let p = player(&pop.economy, local);
        set(st, 45, 1445, format!("{}%", u((p.strength.1 * 100) >> 8)));
        set(st, 45, 1422, format!("{}%", u((p.strength.0 * 100) >> 8)));
        set(st, 45, 1448, "0".into());
        set(st, 45, 1401, titles.0.to_string());
    } else {
        set(st, 44, 1450, titles.1.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every listed type has its count / plus links on its page, for every race (but the
    /// landscape maker on the town page: `UpdateGuiDlgBuildTown` has no case for it).
    #[test]
    fn lists_have_links() {
        for race in [0, 1, 2, 4] {
            for page in 9..=13 {
                for t in build_list(page, race).into_iter().filter(|&t| t != 0 && (page, t) != (12, 30)) {
                    assert!(COUNT_LINKS.iter().any(|c| c.0 == page && c.1 == t), "race {race} page {page} type {t}");
                }
            }
        }
    }
}
