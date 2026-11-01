//! The GUI sounds. The original has no central click sound: every dialog procedure plays its own
//! with `PlaySound(id, 100, pan 64)` (release 0x50c390 on `CSoundManager`, whose first field, the
//! pan, is 64 from the constructor 0x50c150), so they are non-positional
//! (`AudioEvent::Ui`). This module maps the procedures' calls as `GuiState::calls` records them
//! (code 1 = a control's state changed, code 3 = a control was activated) to those sounds.
//!
//! Which procedure a dialog gets is the `OpenDialog(slot, proc)` call or its inlined copy
//! (`tools/re/audio/dialog_procs.py`); the procedures are decompiled in `tools/re/audio/procs.c`
//! (in-game) and `menuprocs.c` (start screens).
//!
//! * **Code 1.** All but three in-game procedures and nearly all start-screen ones play 7
//!   (`SOUND_GUI_BUILDBUTTONUP`) when the new state has the pressed bit (`a2 & 0x10000`), else 6
//!   (`…DOWN`), for any control: e.g. 0x49b6b0 (main bar), 0x46cc10, 0x469540. A click on a
//!   button therefore plays 7 on the press and 6 on the release; selecting a tab plays 6 for the
//!   tab released and 7 for the one pressed; an edit field plays 6 when it gains or loses the
//!   focus (bit 0x40, the pressed bit stays clear). Silent: 0x4ad910 (in-game dialog 57) and
//!   `DrawClockCallback` 0x474a80 (74); the message window's `ChatEvents` 0x499d80 (60..=65)
//!   plays only the 7; so do the game settings' 0x48ba20 (start screens 4 / 19). The credits'
//!   0x47d070 / 0x47d080 / 0x47d090 (15..=17) are silent. Some procedures suppress code 1 while
//!   their own init or refresh selects controls (a flag set around it: 22 0x46c8b0, 25 0x4acf20,
//!   27 0x46c060, 31 0x4a4f30, 58 0x474560, start screens 3 / 18 0x48e5e0, 4 / 19 0x48ba20's
//!   refresh 0x48a450); those selects are not modelled here.
//! * **Code 2** (a character typed into or deleted from an edit field): [`code2`].
//! * **Code 3.** Per procedure and link, mostly 11 (`SOUND_GUI_SUBBUTTONUP`) after it handled
//!   the control ([`code3_game`], [`code3_start`]).
//! * **Code 7** (right click): only 0x479270 (in-game 55) plays, 11 for 873..=881.
//! * The start screens' list scrolls play 11 only when the list moves; the frontend plays those
//!   itself (`Frontend::play`), as it knows the list.

use s4_viewer::guistate::UiEvent;

/// The GUI set whose procedures play.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Procs {
    /// The in-game sets (`GUISetRomans.dat` …): one slot layout and one procedure per slot.
    Game,
    /// A start-screen set; each numbers its dialogs from 0 with its own procedures
    /// (`tools/re/audio/dialog_procs.py`: the `OpenDialog` sites of each state).
    Start(StartSet),
}

/// The start screens' GUI sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartSet {
    /// `GUISetStartscreens.dat`.
    Main,
    /// `GUISetAOStartscreens.DAT`: the Add-On's menu, campaign screens, briefing and splash.
    AddOn,
    /// `GUISetMDStartscreens.DAT`: the Mission CD's screens (0..=5) and the random map
    /// parameters (6 network, 7 local: `CStateMDRandomMapParameters`).
    MissionCd,
    /// `GUISetMD2Startscreens.DAT`.
    MissionCd2,
    /// `GUISetXMD3Startscreens.DAT`.
    MissionCd3,
}

const BUTTON_DOWN: u16 = 6;
const BUTTON_UP: u16 = 7;
/// `SOUND_GUI_SUBBUTTONUP`, also what a moved list scroll plays.
pub const SUB_BUTTON_UP: u16 = 11;

/// The sound page's volume sliders (dialog 37: music 0x4c0, effects 0x4c1, voice chat 0x4c8,
/// microphone 0x4cd).
const VOLUME_SLIDERS: [u16; 4] = [0x4c0, 0x4c1, 0x4c8, 0x4cd];

/// The sounds a procedure plays for one call (several voices at once if more than one).
///
/// Ours, not the release's (the user's request): the volume sliders are silent. Their procedure
/// 0x4a3840 plays 7 / 6 on code 1 (the knob pressed / let go) and 7 on code 3, so adjusting a
/// volume clicked over the change one is listening to.
pub fn sounds(procs: Procs, call: &UiEvent) -> &'static [u16] {
    if let UiEvent::State { dialog: 37, link, .. } | UiEvent::Command { dialog: 37, link, .. } = *call
        && procs == Procs::Game
        && VOLUME_SLIDERS.contains(&link)
    {
        return &[];
    }
    match *call {
        UiEvent::State { dialog, effects, .. } => {
            let pressed = effects & 1 != 0;
            let silent = match procs {
                Procs::Game => matches!(dialog, 57 | 74) || (60..=65).contains(&dialog) && !pressed,
                // 0x48ba20 plays only the press; 0x47d070 / 0x47d080 / 0x47d090 return at once.
                Procs::Start(StartSet::Main) => matches!(dialog, 4 | 19) && !pressed || matches!(dialog, 15..=17),
                Procs::Start(_) => false,
            };
            if silent {
                &[]
            } else if pressed {
                &[BUTTON_UP]
            } else {
                &[BUTTON_DOWN]
            }
        }
        UiEvent::Edit { dialog, link } => code2(procs, dialog, link),
        UiEvent::Command { dialog, link, .. } => match procs {
            Procs::Game => code3_game(dialog, link),
            Procs::Start(set) => code3_start(set, dialog, link),
        },
        // Code 7 (right click): only 0x479270 (55) plays, 11 for the links it handles.
        UiEvent::Context { dialog: 55, link: 873..=881 } if procs == Procs::Game => &[SUB_BUTTON_UP],
        _ => &[],
    }
}

/// Code 3 of the in-game procedures (`procs.c`; slot: procedure). Mostly 11 after the links
/// the procedure handles; 7 for some settings pages; 6 for the message lines.
///
/// Conditions the original tests that are not modelled (so the sound always plays here):
/// * the per-procedure guard flags that its own refresh sets while it selects controls
///   (e.g. `DAT_015aabb5` around `SetMinimapButtonState` 0x49b630; 14, 17..=20, 22..=25, 27,
///   31, 36..=39, 46, 50, 58);
/// * "not the open one" on the main tabs (0, `GUIEventHandler +0x18`) and the build tabs (3,
///   `+0xc`): a pressed tab does not fire again (`UIEventMethod` on a pressed type 2), so a
///   user click always passes;
/// * [inf] list scrolls that only sound when the list moves (25: 1699..=1702, 33: 925 / 926)
///   and 36's 1168 / 1169 (only when `GameManager +0x118` changes).
pub fn code3_game(dialog: usize, link: u16) -> &'static [u16] {
    const SUB: &[u16] = &[SUB_BUTTON_UP];
    const UP: &[u16] = &[BUTTON_UP];
    const DOWN: &[u16] = &[BUTTON_DOWN];
    match (dialog, link) {
        // UiMainSidebarSurfaceCallbacks 0x49b6b0: main tabs 606..=611 play 7, the minimap
        // view toggles 614..=617 11.
        (0, 606..=611) => UP,
        (0, 614..=617) => SUB,
        // BuildingsSurfaceCallback 0x472750: the build tabs; 539 plays 11 twice (two calls).
        (3, 539) => &[SUB_BUTTON_UP, SUB_BUTTON_UP],
        (3, 536..=540) => SUB,
        // The other submenus: 0x475150, 0x49ce30, 0x49c040, 0x47b4e0 (0x4aa5e0, 6, is silent).
        (4, 745..=748) | (5, 1067..=1069) | (7, 1021..=1024) | (8, 928..=931) => SUB,
        (14, 404..=407 | 417)
        | (15, 465 | 466)
        | (16, 970..=972 | 999..=1020)
        | (17, 1025..=1027 | 1057 | 1060 | 1061)
        | (18, 1083..=1086)
        | (19, 1097..=1099 | 1138)
        | (20, 1387..=1389)
        | (22, 328..=330 | 344 | 347)
        | (23, 1605..=1607 | 1623)
        | (24, 1363..=1365 | 1381 | 1383)
        | (25, 1671..=1702 | 1704..=1709)
        | (26, 776..=782 | 2880)
        | (27, 323)
        | (28, _)
        | (29, 1236..=1248)
        | (31, 1350)
        | (32, 755..=759)
        | (33, 912..=914 | 925 | 926)
        | (34, 1071 | 1072)
        | (40, 1761..=1765)
        | (41, 1713 | 1717 | 1750..=1752)
        | (42, 714 | 730 | 742)
        | (43, 1799 | 1800)
        | (44, 1465..=1467)
        | (45, 1437..=1440 | 1444)
        | (46, 1766..=1771 | 1775..=1778 | 1788 | 2888 | 2912)
        | (47, 221..=233)
        // 0x467970: 146 jumps past the PlaySound (0x467b0b.. → 0x467d28), so it is silent.
        | (48, 140..=145 | 147..=149 | 160..=165)
        | (50, 189..=214 | 218..=220)
        | (51, 244..=249 | 253..=255)
        | (53, 805..=810 | 824..=827 | 829..=837)
        | (54, 842..=845)
        | (55, 873..=881)
        | (58, 579..=589)
        | (59, 891)
        | (75, _)
        | (76, _) => SUB,
        (36, 1168 | 1169 | 1174) | (37, 1216..=1219 | 1224 | 1225 | 1229 | 1233) | (38, 1139 | 1146 | 1149 | 1150) | (39, 1194..=1206) => UP,
        (46, 1783..=1785 | 1787) => UP,
        (56, 932..=939) => DOWN,
        // The message window, `ChatEvents` 0x499d80 (60..=65, by screen size): a line 6, the
        // scroll and close 11, the chat input 7; `ChatEvents2` 0x4ad610 (66..=71): 6.
        (60..=65, 958..=961) => DOWN,
        (60..=65, 966..=968) => SUB,
        (60..=65, 969) => UP,
        (66..=71, 1711 | 1712) => DOWN,
        _ => &[],
    }
}

/// Code 2 (an edit field's text changed by a key): the procedures that play
/// (`tools/re/audio/startprocs.c`, `code3_table.txt`).
///
/// [simplification] Not modelled: the random map key 0x7bf plays a second 11 when the typed key
/// is complete and valid (0x4981b0: `RandomMap +0x10`), and the game list's name 0x908 plays 11
/// only with an Uplay name (`UplayAdapter +0x10`).
pub fn code2(procs: Procs, dialog: usize, link: u16) -> &'static [u16] {
    const SUB: &[u16] = &[SUB_BUTTON_UP];
    const TWICE: &[u16] = &[SUB_BUTTON_UP, SUB_BUTTON_UP];
    match (procs, dialog, link) {
        // 0x47af10, the save page's name; `ChatEvents` 0x499d80, the chat input 0x3c9 (7).
        (Procs::Game, 33, 0x392) => SUB,
        (Procs::Game, 60..=65, 0x3c9) => &[BUTTON_UP],
        // 0x47a430 (game list): the address 0x909.
        (Procs::Start(StartSet::Main), 2, 0x909) => SUB,
        // 0x48e5e0 (map settings): the chat 0x994, 11 and 11 again after copying its text.
        (Procs::Start(StartSet::Main), 3 | 18, 0x994) => TWICE,
        // 0x48ba20 (game settings): the chat 0x869 plays 7.
        (Procs::Start(StartSet::Main), 4 | 19, 0x869) => &[BUTTON_UP],
        // 0x4981b0 (random map): the seed 0x7b4 and the game name 0x7c1 play 11 twice (the
        // second at 0x498e18), the key 0x7bf once.
        (Procs::Start(StartSet::MissionCd), 6 | 7, 0x7b4 | 0x7c1) => TWICE,
        (Procs::Start(StartSet::MissionCd), 6 | 7, 0x7bf) => SUB,
        _ => &[],
    }
}

/// Code 3 of the start-screen procedures (`menuprocs.c`, `startprocs.c`, `startprocs2.c`): 11
/// after these links. The list scrolls (load 2340 / 2341, map settings 2431 / 2432, the game
/// list 2309 / 2310, the end statistics' detail 0x857 / 0x858) play only when the list moves:
/// the frontend plays them (`Frontend::play`).
pub fn code3_start(set: StartSet, dialog: usize, link: u16) -> &'static [u16] {
    let plays = match set {
        StartSet::Main => match dialog {
            // CStateMainMenu 0x490360 and the network type 0x49beb0: PlaySound(11) before their
            // switch, for every link.
            0 | 1 => true,
            // The game list 0x47a430: back 0x8e1, 0x8e4, create 0x907 (the rows connect silently).
            2 => matches!(link, 0x8e1 | 0x8e4 | 0x907),
            // Map settings 0x48e5e0 (network 3, local 18): start 2433, the lobby's settings
            // buttons (2441 / 2442 / 2445 / 2446, 2487..=2492, 2505); not back 2415, the rows,
            // the category radios.
            3 | 18 => matches!(link, 2433 | 2441 | 2442 | 2445 | 2446 | 2487..=2492 | 2505),
            // The game settings 0x48ba20 (network 4, local 19): back 0x85d, start 0x85e, 0x860,
            // the slot buttons 0x86b..=0x872, the edit arrows 0x884..=0x88b, the race / colour
            // lists 0x8ae..=0x8bd, the chat scroll 0x8be / 0x8bf (always), ready 0x8c4, the rows
            // 0x8d9..=0x8e0; not the chat line 0x869.
            4 | 19 => matches!(link, 0x85d | 0x85e | 0x860 | 0x86b..=0x872 | 0x884..=0x88b | 0x8ae..=0x8bf | 0x8c4 | 0x8d9..=0x8e0),
            // The load type 0x48cee0: back, the three types.
            5 => matches!(link, 0x955..=0x957 | 0x95f),
            // Load game 0x48ca60: back 2339, load 2342, the 18 rows (both columns).
            6 => matches!(link, 2339 | 2342..=2378),
            // The single player type 0x48d2b0.
            7 => matches!(link, 2400 | 2401 | 2403 | 2404 | 2413 | 2414),
            // The campaign screens 0x47c150 (back, nine missions) and 0x47da10 (back, twelve).
            8 => (2037..=2046).contains(&link),
            9 => (2080..=2092).contains(&link),
            // The briefing 0x47cda0: back and start; the story scroll and picture toggle are silent.
            10 => matches!(link, 2055 | 2056),
            // The tutorials 0x494330: back and the twelve missions.
            11 => (0xaed..=0xaf9).contains(&link),
            // The end statistics 0x491a30 (OK, the players' buttons, the random map's save; not
            // the mode radios), its detail 0x480250 (back), the goods race's 0x493c80 (OK, save).
            13 => matches!(link, 0xa07..=0xa0f | 0xa71),
            21 => link == 0x83a,
            22 => matches!(link, 0xa78 | 0xaeb),
            // 12 0x490720 (victory), 14..=17 (credits), 20 0x48f1d0 (message box): none.
            _ => false,
        },
        StartSet::AddOn => match dialog {
            // 0x46a910 (menu), 0x46a490 / 0x46b6b0 / 0x46a180 / 0x46ad40 / 0x46b1a0 (the
            // campaign screens), 0x469c80 (briefing: back, start; not the scroll or the tips
            // toggle), 0x46bc40 (splash: OK).
            0 => matches!(link, 0x3b..=0x42 | 0x4c),
            1 => (0x2e..=0x32).contains(&link),
            2 => (0x6f..=0x73).contains(&link),
            3 => (0x21..=0x25).contains(&link),
            4 => (0x4d..=0x51).contains(&link),
            5 => (0x5a..=0x66).contains(&link),
            7 => matches!(link, 1 | 2),
            8 => link == 0x80,
            _ => false,
        },
        StartSet::MissionCd => match dialog {
            // 0x496770 (menu), 0x496310 / 0x495f50 / 0x496fb0 / 0x496bd0 (campaign screens),
            // 0x495bd0 (briefing: back, start).
            0 => matches!(link, 0x77c..=0x780 | 0x783..=0x785),
            1 => matches!(link, 0x76f..=0x773 | 0x77b),
            2 => matches!(link, 0x761..=0x765 | 0x76d),
            3 => (0x79d..=0x7a2).contains(&link),
            4 => (0x794..=0x79a).contains(&link),
            5 => matches!(link, 0x74b | 0x74c),
            // The random map parameters 0x4981b0: every button but the two check boxes 0x7e7 /
            // 0x7e9 (0x7e5, paste, plays a second 11 for a valid key: not modelled).
            6 | 7 => matches!(
                link,
                0x7ad | 0x7ae | 0x7b1 | 0x7b2 | 0x7b5 | 0x7b8 | 0x7b9 | 0x7bc | 0x7bd | 0x7c4 | 0x7c5 | 0x7c8 | 0x7c9 | 0x7ce | 0x7cf | 0x7d1 | 0x7d2 | 0x7d4..=0x7d6 | 0x7e1..=0x7e5
            ),
            _ => false,
        },
        // 0x495270 (map screen), 0x494a50 (briefing: back, start).
        StartSet::MissionCd2 => match dialog {
            0 => (0x720..=0x736).contains(&link),
            1 => matches!(link, 0x70b | 0x70c),
            _ => false,
        },
        // 0x4b0450 (map screen), 0x4aff80 (briefing: back, start).
        StartSet::MissionCd3 => match dialog {
            0 => (0xb78..=0xb8a).contains(&link),
            1 => matches!(link, 0xb63 | 0xb64),
            _ => false,
        },
    };
    if plays { &[SUB_BUTTON_UP] } else { &[] }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(dialog: usize, link: u16, effects: u8) -> UiEvent {
        UiEvent::State { dialog, link, effects }
    }
    fn command(dialog: usize, link: u16) -> UiEvent {
        UiEvent::Command { dialog, link, effects: 1 }
    }

    #[test]
    fn code1_pressed_7_released_6() {
        assert_eq!(sounds(Procs::Game, &state(9, 373, 3)), &[7]);
        assert_eq!(sounds(Procs::Game, &state(9, 373, 2)), &[6]);
        assert_eq!(sounds(Procs::Game, &state(57, 1, 1)), &[] as &[u16]);
        assert_eq!(sounds(Procs::Game, &state(60, 958, 0)), &[] as &[u16]);
        assert_eq!(sounds(Procs::Start(StartSet::Main), &state(0, 2531, 0x40)), &[6]);
        // The game settings play only the press; the credits nothing.
        assert_eq!(sounds(Procs::Start(StartSet::Main), &state(19, 0x85d, 1)), &[7]);
        assert_eq!(sounds(Procs::Start(StartSet::Main), &state(19, 0x85d, 0)), &[] as &[u16]);
        assert_eq!(sounds(Procs::Start(StartSet::Main), &state(16, 1, 1)), &[] as &[u16]);
        assert_eq!(sounds(Procs::Start(StartSet::AddOn), &state(16, 1, 1)), &[7]);
    }

    #[test]
    fn code3_tables() {
        assert_eq!(sounds(Procs::Game, &command(0, 607)), &[7]);
        assert_eq!(sounds(Procs::Game, &command(3, 539)), &[11, 11]);
        assert_eq!(sounds(Procs::Game, &command(9, 373)), &[] as &[u16]);
        assert_eq!(sounds(Procs::Game, &command(48, 146)), &[] as &[u16]);
        assert_eq!(sounds(Procs::Game, &command(56, 932)), &[6]);
        assert_eq!(sounds(Procs::Start(StartSet::Main), &command(0, 2531)), &[11]);
        assert_eq!(sounds(Procs::Start(StartSet::Main), &command(3, 2415)), &[] as &[u16]);
        assert_eq!(sounds(Procs::Start(StartSet::Main), &command(18, 2433)), &[11]);
        assert_eq!(sounds(Procs::Start(StartSet::Main), &command(18, 2431)), &[] as &[u16], "a scroll: the frontend's");
        // Dialog 8 is a different procedure in each set.
        assert_eq!(sounds(Procs::Start(StartSet::Main), &command(8, 0x80)), &[] as &[u16]);
        assert_eq!(sounds(Procs::Start(StartSet::AddOn), &command(8, 0x80)), &[11]);
        assert_eq!(sounds(Procs::Start(StartSet::MissionCd), &command(7, 0x7e7)), &[] as &[u16]);
        assert_eq!(sounds(Procs::Start(StartSet::MissionCd), &command(7, 0x7d5)), &[11]);
        assert_eq!(sounds(Procs::Start(StartSet::MissionCd), &UiEvent::Edit { dialog: 7, link: 0x7c1 }), &[11, 11]);
        assert_eq!(sounds(Procs::Game, &UiEvent::Edit { dialog: 33, link: 0x392 }), &[11]);
        assert_eq!(sounds(Procs::Game, &UiEvent::Edit { dialog: 62, link: 0x3c9 }), &[7]);
        assert_eq!(sounds(Procs::Game, &UiEvent::Context { dialog: 55, link: 873 }), &[11]);
        assert_eq!(sounds(Procs::Game, &UiEvent::Context { dialog: 54, link: 842 }), &[] as &[u16]);
    }

    /// Dragging a volume knob of the sound page (37) through the GUI: the slider moves (code 2)
    /// and none of the procedure's calls plays a sound (ours, the user's request); the page's
    /// check boxes still click.
    #[test]
    fn volume_sliders_are_silent() {
        let Ok(set) = s4_formats::gui::GuiSet::open(s4_formats::game_dir().join("Menu/GUISetRomans.dat")) else { return };
        let mut s = s4_viewer::guistate::GuiState::new(set);
        s.open(37, &mut Vec::new());
        for link in VOLUME_SLIDERS {
            s.set_slider(37, link, 50);
            let d = s.set.dialog(37).unwrap();
            let c = d.controls.iter().find(|c| c.link == link).unwrap();
            let (x, y) = ((d.x + c.x + 2) as i32, (d.y + c.y + c.height / 2) as i32);
            s.calls.clear();
            let mut ev = s.left_down(x, y, true);
            ev.extend(s.mouse_move(x - 20, y));
            ev.extend(s.left_up(x - 20, y));
            assert!(ev.iter().any(|e| matches!(e, UiEvent::Slider { dialog: 37, link: l, .. } if *l == link)), "{link:#x} moved: {ev:?}");
            assert!(!s.calls.is_empty(), "{link:#x}: the procedure was called");
            for c in &s.calls {
                assert_eq!(sounds(Procs::Game, c), &[] as &[u16], "{link:#x}: {c:?}");
            }
        }
        assert_eq!(sounds(Procs::Game, &command(37, 0x4c2)), &[7], "the music check box");
        assert_eq!(sounds(Procs::Game, &state(37, 0x4c2, 1)), &[7]);
    }
}
