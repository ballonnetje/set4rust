//! The Mission CD, Mission CD 2 and Mission CD 3 campaigns: their own start-screen GUI sets,
//! the mission choice and a briefing. Decompiles and table dumps in
//! `tools/re/viewer-2026-10-22/addon/` (`tables.py`, `transitions.py`).
//!
//! **Mission CD** (main menu event 7001 → `CStateMDCampaigns` 0x525570, `GUISetMDStartscreens`):
//! a menu (dialog 0, proc 0x496770, init 0x4965b0: texts 888 / 2988, the difficulty, the bonus
//! button only while `GetCampaignStatus(10)` is set) leads to the Roman (dialog 1, 0x525250),
//! Maya (2, 0x524f30), Viking (3, 0x525d30) and settle / conflict (4, 0x5259e0) screens, whose
//! mission buttons (texts from their inits 0x4961f0 / 0x495e30 / 0x496e90 / 0x496a90) are enabled
//! up to `status + 1`, and to the bonus mission. Campaign types 5 Roman, 6 Viking, 7 Maya (five
//! missions), 8 settle, 9 conflict (three), 10 bonus (one; its status is Data05's top bit, set
//! by `SetCampaignStatus` 0x501d90 when the other five read 5 5 5 3 3). The briefing (dialog 5,
//! state 0x524350, texts 0x524b80, picture 0xffdb68) starts `Map\Campaign\md_<kind>%01i.map`
//! (0x423ab0; `md_bonus.map`); its back goes to the campaign's screen (0x524590).
//!
//! **Add-On** (event 8001 → `CStateAOCampaigns` 0x50f360, `GUISetAOStartscreens`, the
//! `CAddOn` object 0x12947ac): a menu (dialog 0, proc 0x46a910, init 0x46a6e0: texts 888 / 116,
//! the difficulty) leads to the Roman (dialog 1, 0x50f090), Viking (2, 0x50fef0), Maya (3,
//! 0x50ed70), settle (4, 0x50f890) and Trojan (5, 0x50fbb0) screens (events 0x1f43 / 0x1f45 /
//! 0x1f44 / 0x1f51 / 0x1f46, jump table 0x50f6a0); their inits (0x46a3d0 / 0x46b5b0 / 0x46a0c0 /
//! 0x46ac80 / 0x46af90) label the missions and enable them up to `status + 1`. Campaign types
//! 11 Roman, 12 Viking, 13 Maya, 14 settle (four missions), 15 Trojan (twelve), 16 bonus (one,
//! not reachable from the menu: its button 76 sends 0x1f48, which 0x50f4d0 ignores). The briefing
//! (dialog 7, state 0x50dda0, texts 0x50e7c0, pictures 0xffc190) plays
//! `Snd\Campaign<""|EN>\ao_<kind>%01i.mp3` (Trojan `%02i`, 0x423090; the HE ships only the
//! German ones) and starts `Map\Campaign\ao_<kind>%01i.map` (Trojan `%02i`). Not ported: the
//! Trojan campaign's videos (state 0x528360 between missions).
//!
//! * Main menu events 9001 / 9501 (`CStateMainMenu` 0x522b70) construct `CStateMD2Campaigns`
//!   (0x523f40) / `CStateXMD3Campaigns` (0x5295f0): the add-on object (`CMissionCD2` 0x12947b0,
//!   `CMissionCD3` 0x12947b8, vtable +8) switches to its GUI set (`GUISetMD2Startscreens.dat`,
//!   `GUISetXMD3Startscreens.dat`) and opens dialog 0 with the proc 0x495270 / 0x4b0450.
//! * The map screen: difficulty radios (0x12987ec 0 easy / 1 normal, saved to 0x11b5948 when a
//!   mission is chosen), back (to the main menu, vtable +0xc restores the start screens), the
//!   mission buttons (events 0x232a..0x232d / 0x251e..0x2521 with the mission → the briefing
//!   state 0x5234f0 / 0x528ae0 with `level << 16 | type`). Campaign types: MCD2 17 Roman,
//!   18 Viking, 19 Maya, 20 Trojan (five missions each); MCD3 21 Roman, 22 Viking, 23 Maya,
//!   24 Trojan (four). MCD2's init (0x494cb0) shows mission k + 1 of a campaign after k wins
//!   (`GetCampaignStatus` 0x501ba0, Data07 nibbles), the Viking and Maya first missions always,
//!   the Roman and Trojan first ones after a Viking or Maya win; MCD3's (0x4b02e0) shows all
//!   (`GetCampaignStatus` gives 15 for types 21..24).
//! * The briefing (inits 0x4947f0 / 0x4afd40, texts 0x523c80 / 0x529330): header string 880, the
//!   mission's title, goal, story (scrolled by its arrows and the wheel) and tips (shown while the
//!   tips toggle is down; MCD2 with its picture from 0xffdb18, MCD3 with the map preview); back
//!   (event 0x2330 / 0x2523: the map screen again), start (0x2331 / 0x2524): the map
//!   `Map\Campaign\mcd2_<race>%01i.map` / `xmd3_<race>%01i.map` with level + 1, read by
//!   0x5069b0 as a local game with the difficulty (computers 2 easy / 3 normal), campaign type and
//!   level into the GameState (+0x2d4 / +0x2d8, game type 3). No briefing speech.

use std::path::PathBuf;

/// The two add-on campaign packs this module drives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pack {
    Ao,
    Md,
    Mcd2,
    Xmd3,
}

/// A pack's links (dialog 0: map screen, dialog 1: briefing).
pub struct Links {
    pub set: &'static str,
    /// The briefing's dialog.
    pub brief: usize,
    pub easy: u16,
    pub normal: u16,
    pub back: u16,
    /// (first link, campaign type) of each campaign's mission buttons.
    pub missions: [(u16, u8); 4],
    pub missions_per: u16,
    pub header: u16,
    pub brief_back: u16,
    pub start: u16,
    pub story: u16,
    pub tips: u16,
    pub title: u16,
    pub goal: u16,
    pub up: u16,
    pub down: u16,
    pub tips_toggle: u16,
    /// MCD2: the picture (an image); MCD3: the map preview (an owner-drawn area).
    pub picture: u16,
}

pub const MCD2: Links = Links {
    set: "GUISetMD2Startscreens.DAT",
    brief: 1,
    easy: 1824,
    normal: 1825,
    back: 1826,
    missions: [(1827, 17), (1832, 18), (1842, 19), (1837, 20)],
    missions_per: 5,
    header: 1802,
    brief_back: 1803,
    start: 1804,
    story: 1805,
    tips: 1806,
    title: 1807,
    goal: 1808,
    up: 1809,
    down: 1810,
    tips_toggle: 1812,
    picture: 1813,
};

pub const XMD3: Links = Links {
    set: "GUISetXMD3Startscreens.DAT",
    brief: 1,
    easy: 2936,
    normal: 2937,
    back: 2938,
    missions: [(2939, 21), (2943, 22), (2951, 23), (2947, 24)],
    missions_per: 4,
    header: 2914,
    brief_back: 2915,
    start: 2916,
    story: 2917,
    tips: 2918,
    title: 2919,
    goal: 2920,
    up: 2921,
    down: 2922,
    tips_toggle: 2924,
    picture: 2926,
};

/// Mission CD: the menu's difficulty and back (dialog 0) and the briefing (dialog 5); its
/// missions are on [`MD_SCREENS`].
pub const MD: Links = Links {
    set: "GUISetMDStartscreens.DAT",
    brief: 5,
    easy: 1923,
    normal: 1924,
    back: 1925,
    missions: [(0, 0); 4],
    missions_per: 0,
    header: 1866,
    brief_back: 1867,
    start: 1868,
    story: 1869,
    tips: 1870,
    title: 1871,
    goal: 1872,
    up: 1873,
    down: 1874,
    tips_toggle: 1876,
    picture: 1877,
};

/// Add-On: the menu's difficulty and back (dialog 0) and the briefing (dialog 7).
pub const AO: Links = Links {
    set: "GUISetAOStartscreens.DAT",
    brief: 7,
    easy: 64,
    normal: 65,
    back: 59,
    missions: [(0, 0); 4],
    missions_per: 0,
    header: 13,
    brief_back: 1,
    start: 2,
    story: 14,
    tips: 19,
    title: 22,
    goal: 15,
    up: 21,
    down: 5,
    tips_toggle: 20,
    picture: 6,
};

/// A campaign screen of a menu pack: dialog, title (link, string), back, and the missions (link,
/// campaign type, level, label string).
pub struct MdScreen {
    pub dialog: usize,
    pub title: (u16, usize),
    pub back: u16,
    pub missions: &'static [(u16, u8, u32, usize)],
}

pub const MD_SCREENS: [MdScreen; 4] = [
    MdScreen { dialog: 1, title: (1902, 2982), back: 1915, missions: &[(1903, 5, 0, 2939), (1904, 5, 1, 2941), (1905, 5, 2, 2943), (1906, 5, 3, 2945), (1907, 5, 4, 2947)] },
    MdScreen { dialog: 2, title: (1888, 2986), back: 1901, missions: &[(1889, 7, 0, 2955), (1890, 7, 1, 2957), (1891, 7, 2, 2959), (1892, 7, 3, 2961), (1893, 7, 4, 2963)] },
    MdScreen { dialog: 3, title: (1948, 2980), back: 1949, missions: &[(1950, 6, 0, 2929), (1951, 6, 1, 2931), (1952, 6, 2, 2933), (1953, 6, 3, 2935), (1954, 6, 4, 2937)] },
    MdScreen {
        dialog: 4,
        title: (1947, 2984),
        back: 1940,
        missions: &[(1941, 8, 0, 2949), (1942, 8, 1, 2951), (1943, 8, 2, 2953), (1944, 9, 0, 2923), (1945, 9, 1, 2925), (1946, 9, 2, 2927)],
    },
];

pub const AO_SCREENS: [MdScreen; 5] = [
    MdScreen { dialog: 1, title: (58, 119), back: 46, missions: &[(47, 11, 0, 59), (48, 11, 1, 61), (49, 11, 2, 63), (50, 11, 3, 65)] },
    MdScreen { dialog: 2, title: (123, 125), back: 111, missions: &[(112, 12, 0, 101), (113, 12, 1, 103), (114, 12, 2, 105), (115, 12, 3, 107)] },
    MdScreen { dialog: 3, title: (45, 114), back: 33, missions: &[(34, 13, 0, 35), (35, 13, 1, 37), (36, 13, 2, 39), (37, 13, 3, 41)] },
    MdScreen { dialog: 4, title: (89, 121), back: 81, missions: &[(77, 14, 0, 69), (78, 14, 1, 71), (79, 14, 2, 73), (80, 14, 3, 75)] },
    MdScreen {
        dialog: 5,
        title: (110, 123),
        back: 90,
        missions: &[
            (91, 15, 0, 77),
            (92, 15, 1, 79),
            (93, 15, 2, 81),
            (94, 15, 3, 83),
            (95, 15, 4, 85),
            (96, 15, 5, 87),
            (97, 15, 6, 89),
            (98, 15, 7, 91),
            (99, 15, 8, 93),
            (100, 15, 9, 95),
            (101, 15, 10, 97),
            (102, 15, 11, 99),
        ],
    },
];

/// A menu button: the screen's dialog it opens, `None` for the bonus briefing.
pub type MenuButton = (u16, Option<usize>);

/// The Add-On menu (0x46a910): Roman, Viking, Maya, settle, Trojan.
pub const AO_MENU: [(u16, Option<usize>); 5] = [(60, Some(1)), (61, Some(2)), (62, Some(3)), (63, Some(4)), (66, Some(5))];
/// The menu's texts (0x46a6e0).
pub const AO_MENU_TEXTS: [(u16, usize); 2] = [(75, 888), (74, 116)];

/// The Mission CD menu (0x496770 → events 0x1b5b..0x1b5f → 0x525660): button → the screen's
/// dialog, or `None` for the bonus mission's briefing (type 10).
pub const MD_MENU: [(u16, Option<usize>); 5] = [(1916, Some(3)), (1917, None), (1918, Some(1)), (1919, Some(4)), (1920, Some(2))];
/// The menu's texts (0x4965b0) and the bonus button.
pub const MD_MENU_TEXTS: [(u16, usize); 2] = [(1922, 888), (1921, 2988)];
pub const MD_BONUS: u16 = 1917;

impl Pack {
    pub fn links(self) -> &'static Links {
        match self {
            Pack::Ao => &AO,
            Pack::Md => &MD,
            Pack::Mcd2 => &MCD2,
            Pack::Xmd3 => &XMD3,
        }
    }

    /// The pack of a campaign type.
    pub fn of(campaign: u8) -> Option<Pack> {
        match campaign {
            5..=10 => Some(Pack::Md),
            11..=16 => Some(Pack::Ao),
            17..=20 => Some(Pack::Mcd2),
            21..=24 => Some(Pack::Xmd3),
            _ => None,
        }
    }

    /// The mission button `link` of the map / campaign screen `dialog` stands for: (campaign
    /// type, level).
    pub fn mission(self, dialog: usize, link: u16) -> Option<(u8, u32)> {
        if !self.screens().is_empty() {
            let s = self.screens().iter().find(|s| s.dialog == dialog)?;
            return s.missions.iter().find(|m| m.0 == link).map(|m| (m.1, m.2));
        }
        let l = self.links();
        l.missions.iter().find(|&&(first, _)| (first..first + l.missions_per).contains(&link)).map(|&(first, t)| (t, (link - first) as u32))
    }

    /// The screen a mission's briefing goes back to (0x524590 for the Mission CD: the campaign's
    /// screen, the menu for the bonus).
    pub fn back_from_briefing(self, campaign: u8) -> usize {
        match (self, campaign) {
            (Pack::Md, 5) => 1,
            (Pack::Md, 6) => 3,
            (Pack::Md, 7) => 2,
            (Pack::Md, 8 | 9) => 4,
            (Pack::Ao, 11..=15) => campaign as usize - 10,
            _ => 0,
        }
    }

    /// The campaign screens of a pack with a menu (Mission CD, Add-On).
    pub fn screens(self) -> &'static [MdScreen] {
        match self {
            Pack::Md => &MD_SCREENS,
            Pack::Ao => &AO_SCREENS,
            _ => &[],
        }
    }

    /// The menu's buttons and texts of a pack with a menu.
    pub fn menu(self) -> (&'static [MenuButton], &'static [(u16, usize)]) {
        match self {
            Pack::Md => (&MD_MENU, &MD_MENU_TEXTS),
            Pack::Ao => (&AO_MENU, &AO_MENU_TEXTS),
            _ => (&[], &[]),
        }
    }
}

/// The briefing strings (0x523c80 / 0x529330): per campaign type the goal, story, tips and
/// title ids of each mission (`tables.py`).
fn strings(campaign: u8) -> Option<[[usize; 5]; 4]> {
    Some(match campaign {
        11 => [[410, 411, 412, 413, 0], [430, 431, 432, 433, 0], [435, 436, 437, 438, 0], [440, 441, 442, 443, 0]],
        12 => [[618, 619, 620, 621, 0], [648, 649, 650, 651, 0], [653, 654, 655, 656, 0], [658, 659, 660, 661, 0]],
        13 => [[317, 318, 319, 320, 0], [347, 348, 349, 350, 0], [352, 353, 354, 355, 0], [357, 358, 359, 360, 0]],
        14 => [[446, 447, 448, 449, 0], [465, 466, 467, 468, 0], [469, 470, 471, 472, 0], [473, 474, 475, 476, 0]],
        5 => [[3147, 3148, 3149, 3150, 3151], [3177, 3178, 3179, 3180, 3181], [3182, 3183, 3184, 3185, 3186], [3187, 3188, 3189, 3190, 3191]],
        6 => [[3025, 3026, 3027, 3028, 3029], [3055, 3056, 3057, 3058, 3059], [3060, 3061, 3062, 3063, 3064], [3065, 3066, 3067, 3068, 3069]],
        7 => [[3220, 3221, 3222, 3223, 3224], [3250, 3251, 3252, 3253, 3254], [3255, 3256, 3257, 3258, 3259], [3260, 3261, 3262, 3263, 3264]],
        8 => [[3192, 3193, 3194, 0, 0], [3210, 3211, 3212, 0, 0], [3213, 3214, 3215, 0, 0], [3216, 3217, 3218, 0, 0]],
        9 => [[2998, 2999, 3000, 0, 0], [3016, 3017, 3018, 0, 0], [3019, 3020, 3021, 0, 0], [3022, 3023, 3024, 0, 0]],
        10 => [[2909, 0, 0, 0, 0], [2920, 0, 0, 0, 0], [2921, 0, 0, 0, 0], [2922, 0, 0, 0, 0]],
        17 => [[2732, 2733, 2734, 2735, 2736], [2766, 2767, 2768, 2769, 2770], [2771, 2772, 2773, 2774, 2775], [2776, 2777, 2778, 2779, 2780]],
        18 => [[2857, 2858, 2859, 2860, 2861], [2894, 2895, 2896, 2897, 2898], [2899, 2900, 2901, 2902, 2903], [2904, 2905, 2906, 2907, 2908]],
        19 => [[2674, 2675, 2676, 2677, 2678], [2707, 2708, 2709, 2710, 2711], [2712, 2713, 2714, 2715, 2716], [2717, 2718, 2719, 2720, 2721]],
        20 => [[2812, 2813, 2814, 2815, 2816], [2842, 2843, 2844, 2845, 2846], [2847, 2848, 2849, 2850, 2851], [2852, 2853, 2854, 2855, 2856]],
        21 => [[3786, 3787, 3788, 3789, 0], [3790, 3791, 3792, 3793, 0], [3794, 3795, 3796, 3797, 0], [3798, 3799, 3800, 3801, 0]],
        22 => [[3819, 3820, 3821, 3822, 0], [3824, 3825, 3826, 3827, 0], [3828, 3829, 3830, 3831, 0], [3832, 3833, 3834, 3835, 0]],
        23 => [[3755, 3756, 3757, 3758, 0], [3773, 3774, 3775, 3776, 0], [3777, 3778, 3779, 3780, 0], [3781, 3782, 3783, 3784, 0]],
        24 => [[3802, 3803, 3804, 3805, 0], [3806, 3807, 3808, 3809, 0], [3810, 3811, 3812, 3813, 0], [3814, 3815, 3816, 3817, 0]],
        _ => return None,
    })
}

/// (goal, story, tips, title) string ids of mission `level`.
pub fn briefing(campaign: u8, level: u32) -> Option<[usize; 4]> {
    if campaign == 15 {
        // The Trojans' twelve missions (0x1019738.., 0x30 apart).
        let l = level as usize;
        return (l < 12).then(|| [510 + l, 582 + l, 594 + l, 606 + l]);
    }
    let s = strings(campaign)?;
    let l = level as usize;
    (l < 5 && s[0][l] != 0).then(|| [s[0][l], s[1][l], s[2][l], s[3][l]])
}

/// The briefing picture: MCD2 0xffdb18 `[(type − 17) · 5 + level]`, Mission CD 0xffdb68
/// `[(type − 5) · 5 + level]` (images of the pack's set; 0 = none).
pub fn picture(campaign: u8, level: u32) -> Option<u16> {
    const MCD2: [u16; 20] = [88, 89, 90, 91, 92, 93, 94, 95, 96, 97, 83, 84, 85, 86, 87, 77, 78, 79, 80, 81];
    const MD: [u16; 30] = [116, 117, 118, 119, 120, 124, 125, 126, 127, 128, 111, 112, 113, 114, 115, 121, 122, 123, 0, 0, 108, 109, 110, 0, 0, 107, 0, 0, 0, 0];
    const AO: [u16; 72] = [
        106, 107, 108, 109, 0, 0, 0, 0, 0, 0, 0, 0, 114, 115, 116, 117, 0, 0, 0, 0, 0, 0, 0, 0, 102, 103, 104, 105, 0, 0, 0, 0, 0, 0, 0, 0, 110, 111, 112, 113, 0, 0, 0, 0, 0, 0, 0, 0, 118,
        119, 120, 121, 122, 123, 124, 125, 126, 127, 128, 129, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    if (11..=16).contains(&campaign) {
        let img = *AO.get((campaign as usize - 11) * 12 + level as usize)?;
        return (img != 0).then_some(img);
    }
    if level >= 5 {
        return None;
    }
    let img = match campaign {
        5..=10 => MD[(campaign as usize - 5) * 5 + level as usize],
        17..=20 => MCD2[(campaign as usize - 17) * 5 + level as usize],
        _ => 0,
    };
    (img != 0).then_some(img)
}

/// The mission's map: `Map\Campaign\mcd2_<race>%01i.map` (0x423980) / `xmd3_<race>%01i.map`
/// (0x423ca0), level + 1; found without regard to case (the files are `MCD2_roman1.map` …).
pub fn map_path(campaign: u8, level: u32) -> Option<PathBuf> {
    let name = match campaign {
        11..=14 => format!("ao_{}{}.map", ["roman", "viking", "maya", "settle"][campaign as usize - 11], level + 1),
        15 => format!("ao_trojan{:02}.map", level + 1),
        16 => "ao_bonus.map".to_string(),
        5..=9 => format!("md_{}{}.map", ["roman", "viking", "maya", "settle", "conflict"][campaign as usize - 5], level + 1),
        10 => "md_bonus.map".to_string(),
        17..=20 => format!("mcd2_{}{}.map", ["roman", "viking", "maya", "trojan"][campaign as usize - 17], level + 1),
        21..=24 => format!("xmd3_{}{}.map", ["roman", "viking", "maya", "trojan"][campaign as usize - 21], level + 1),
        _ => return None,
    };
    let dir = s4_formats::path::resolve(s4_formats::game_dir().join("Map").join("Campaign"));
    let found = std::fs::read_dir(&dir).ok()?.flatten().map(|e| e.path()).find(|p| p.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(&name)));
    Some(found.unwrap_or_else(|| dir.join(name)))
}

/// The Add-On briefing's speech (0x50ddf0: the table at 0x149bc80 by type, 0x423090):
/// `Snd\Campaign<"" German | "EN">\ao_<kind>%01i.mp3`, the Trojans' `%02i`.
pub fn speech(campaign: u8, level: u32, language: u32) -> Option<PathBuf> {
    let name = match campaign {
        11..=14 => format!("ao_{}{}.mp3", ["roman", "viking", "maya", "settle"][campaign as usize - 11], level + 1),
        15 => format!("ao_trojan{:02}.mp3", level + 1),
        16 => "ao_bonus.mp3".to_string(),
        _ => return None,
    };
    let dir = s4_formats::path::resolve(s4_formats::game_dir().join("Snd").join(if language == 1 { "Campaign" } else { "CampaignEN" }));
    let found = std::fs::read_dir(&dir).ok()?.flatten().map(|e| e.path()).find(|p| p.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(&name)));
    Some(found.unwrap_or_else(|| dir.join(name)))
}

/// Which mission buttons the map screen shows (0x494cb0 for MCD2: by the statuses of types
/// 17..20; MCD3 shows every mission). `status(type)` = `GetCampaignStatus`.
pub fn visible(pack: Pack, campaign: u8, level: u32, status: &impl Fn(u8) -> u32) -> bool {
    match pack {
        Pack::Xmd3 => true,
        // Enabled rather than shown (EnableControl): mission k + 1 after k wins.
        Pack::Md | Pack::Ao => status(campaign) >= level,
        Pack::Mcd2 => match (campaign, level) {
            (18 | 19, 0) => true,
            (17 | 20, 0) => status(18) >= 1 || status(19) >= 1,
            (_, k) => status(campaign) >= k,
        },
    }
}

/// MCD2's route markers (0x494cb0): (icon, campaign type, mission) — each shown with its
/// mission button.
pub const MCD2_MARKERS: [(u16, u8, u32); 19] = [
    (1851, 17, 1),
    (1852, 17, 2),
    (1853, 17, 3),
    (1854, 17, 4),
    (1861, 18, 1),
    (1862, 18, 1),
    (1863, 18, 2),
    (1864, 18, 3),
    (1865, 18, 4),
    (1847, 19, 1),
    (1848, 19, 2),
    (1849, 19, 3),
    (1850, 19, 4),
    (1855, 20, 1),
    (1856, 20, 2),
    (1857, 20, 2),
    (1858, 20, 2),
    (1859, 20, 3),
    (1860, 20, 4),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missions_maps_and_unlocking() {
        assert_eq!(Pack::Mcd2.mission(0, 1829), Some((17, 2)));
        assert_eq!(Pack::Mcd2.mission(0, 1842), Some((19, 0)));
        assert_eq!(Pack::Xmd3.mission(0, 2954), Some((23, 3)));
        assert_eq!(Pack::Xmd3.mission(0, 2938), None);
        assert_eq!(Pack::Md.mission(4, 1945), Some((9, 1)));
        assert_eq!(Pack::Md.back_from_briefing(6), 3);
        assert_eq!(picture(8, 2), Some(123));
        assert_eq!(picture(8, 3), None);
        assert_eq!(Pack::Ao.mission(5, 102), Some((15, 11)));
        assert_eq!(Pack::Ao.back_from_briefing(14), 4);
        assert_eq!(briefing(15, 11), Some([521, 593, 605, 617]));
        assert_eq!(picture(15, 11), Some(129));
        assert_eq!(briefing(20, 0), Some([2812, 2842, 2847, 2852]));
        assert_eq!(briefing(21, 4), None, "MCD3 has four missions");
        assert_eq!(picture(19, 4), Some(87));
        let none = |_: u8| 0u32;
        assert!(visible(Pack::Mcd2, 18, 0, &none) && !visible(Pack::Mcd2, 17, 0, &none) && !visible(Pack::Mcd2, 18, 1, &none));
        let viking = |t: u8| if t == 18 { 1 } else { 0 };
        assert!(visible(Pack::Mcd2, 17, 0, &viking) && visible(Pack::Mcd2, 18, 1, &viking) && !visible(Pack::Mcd2, 18, 2, &viking));
        if s4_formats::game_dir().join("Map").exists() {
            for t in (5..=15u8).chain(17..=24) {
                for l in 0..crate::progress::mission_count(t) {
                    let p = map_path(t, l).unwrap();
                    assert!(p.exists(), "{}", p.display());
                }
            }
        }
    }
}
