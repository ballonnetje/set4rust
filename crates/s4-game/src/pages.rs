//! The settler pages' recruiting buttons → typed commands, as their procedures and
//! `CInputProcessor` (handler table 0x4e96d0, transitions `tools/re/viewer-2026-10-18/bdlg/trans.txt`)
//! do it. Evidence: `tools/re/viewer-2026-10-19/recruit/`.
//! * Free carriers (dialog 32, `GuiDlgFreeCarrierMenuProc` 0x475dc0): the init sets the target 0
//!   and selects tab 765 (target 1, carriers); tabs 761 → 2 (diggers), 760 → 3 (builders). The
//!   buttons 755 / 759 (−1), 756 (+5), 757 (−5), 758 (+1) send UI event 0x1ba (target, value)
//!   and sound 11; its handler (0x4ec0a0) sends 0x13a3 = 5027 `SetUnitReservation` with the
//!   player's eco-sector on the spiral around the screen centre (0x4ebfe0), nothing without one.
//! * Specialists (dialog 30, `GuiDlgSettlerProductionMenuProc` 0x4a4ae0): the init sets the
//!   target 0 and clicks radio 1251 (target 50); radios 1251..=1255 → 50, 49, 46, 47, 48. The
//!   buttons 1268 (0), 1269 (+5), 1270 (−5), 1271 (+1), 1272 (−1) send UI event 0x1e1 only while
//!   the page's fill found an own eco-sector (`IsInEcoSector`, 0x11aacbc); its handler (0x4ea900)
//!   sends 0x13af = 5039 `SetSpecialistWish` the same way.
//!
//! * Transport priority (dialog 25, `TransportPrioTabCallback` 0x4acf20): a good slot selects
//!   its good (`guistate`); with a good selected (> 0) the buttons 0x6a3 (to the top, 0), 0x6a4
//!   (−1), 0x6a5 (+1), 0x6a6 (to the bottom, 100) send UI event 0x1be (good, value); its handler
//!   (0x4ebd00) sends 0x13a5 = 5029 `SetTransportPriority` with the eco-sector of 0x4ebfe0.
//!   The mouse wheel over the page (code 10): the selected good up (−1) / down (+1).
//!
//! * Goods distribution (dialog 26, `ProductionOrderMenuCallback` 0x476810): a released knob
//!   (795..=799) copies the knobs' values into the page info's shares and sends UI event 0x1bd;
//!   its handler (0x4ebda0) takes the eco-sector of 0x4ebfe0 and that eco-sector's current shares
//!   of the good (0x4c5290 on eco +0x218), then per building of the page info sends 0x13a4 = 5028
//!   `SetBuildingSupplyPriority` with the change (new − old). Before that the missing or surplus
//!   points to 100 go one by one, cyclically from the first, to the buildings whose new share is
//!   not 0.
//!
//! * Settler search (dialog 29, `FindUnitTabCallback` 0x4a3eb0): the init (0x4a3cf0) sets the
//!   type 0 and clicks tab 1240; the tabs set the type ([`find_type`]); 1245 / 1246 / 1247 send UI
//!   event 0x1b9 (type, 0 / 1 / 2) to `CInputProcessor` 0x4ec230 (`Pages::find`); mode 1 selects
//!   the type's units drawn on the screen (a box over the output size with the type's
//!   `GetEntitySelectionType` as a fixed filter). The init (guistate) hides tab 1242 in a campaign mission
//!   (`GlobalGameStateObject +0x3c` = 3). Evidence: `tools/re/viewer-2026-10-20/find/`.
//!
//! The handlers run only where `CInputProcessor` has a transition for their event (input.rs):
//! 0x1ba, 0x1bd and 0x1be in state 0, 0x1e1 in 0 and 1, 0x1b9 in 0 and 1 (→ 1). The tabs that
//! lead to these pages send 0x192 first (state 0), so the gate matters in the other states only.

use s4_net::commands::{Stamped, execution_tick};
use s4_sim::{Command, PlayerCommand};
use s4_viewer::gpu::{Gpu, View};
use s4_viewer::guistate::UiEvent;
use s4_viewer::client;
use s4_viewer::population::Population;
use s4_viewer::scene::Scene;

use crate::select::Pointer;
use crate::sim::SimHandle;

#[derive(Default)]
pub struct Pages {
    /// `JobTabIncrementTarget` (0x15aa5c8): the settler type the free carrier buttons change.
    pub job_target: u16,
    /// `SpecialUnitRecruitingTarget` (0x15aabcc): the specialist type of dialog 30.
    pub specialist_target: u16,
    /// `FindUnitTarget`: the settler type of dialog 29.
    pub find_target: u16,
    /// A node to centre the view on (event 0x13), taken by the main loop.
    pub centre: Option<(u16, u16)>,
}

/// `FindUnitTabCallback` (0x4a3eb0) code 3: the settler type of a tab of dialog 29.
fn find_type(link: u16, race: u8) -> Option<u16> {
    Some(match link {
        0x4d4 => 0x1d,
        0x4d5 => 0x20,
        0x4d6 => match race {
            1 => 0x26,
            2 => 0x29,
            4 => 0x3d,
            _ => 0x23,
        },
        0x4d7 => 0x2c,
        0x4d8 => 0x32,
        0x4d9 => 0x31,
        0x4da => 0x2e,
        0x4db => 0x2f,
        0x4dc => 0x30,
        0x4e0 => 0x2d,
        _ => return None,
    })
}

/// The soldier types whose search goes on with the next level (0x4ec230).
fn levelled(t: u16) -> bool {
    matches!(t, 0x1d | 0x1e | 0x20 | 0x21 | 0x23 | 0x24 | 0x26 | 0x27 | 0x29 | 0x2a)
}

impl Pages {
    /// The GUI's events of one input: targets and buttons of dialogs 25, 26, 29, 30 and 32.
    #[allow(clippy::too_many_arguments)]
    pub fn ui(&mut self, events: &[UiEvent], gpu: &mut Gpu, scene: &Scene, view: View, sim: &SimHandle, pointer: &mut Pointer, state: u8) {
        let player = pointer.player;
        let runs = |ev: u16| crate::input::transition(state, ev).is_some();
        for e in events {
            match *e {
                UiEvent::DialogOpened(29) => self.find_target = 0,
                UiEvent::Command { dialog: 29, link, .. } => {
                    let snap = sim.snapshot();
                    let race = snap.population.races.get(player as usize).copied().unwrap_or(0);
                    if let Some(t) = find_type(link, race) {
                        self.find_target = t;
                    } else if let 1245..=1247 = link
                        && runs(0x1b9)
                    {
                        self.find(gpu, scene, view, &snap.population, pointer, (link - 1245) as u8);
                    }
                }
                // `ExtrasMenuCallback` 0x47b4e0 in game type 3: 0x3a1 → UI 0x192, then 0x1b4
                // (handler 0x4e9c20; 0x192 has just left state 0, where 0x1b4 has its transition):
                // the goods race dialog (s4_viewer::goods_race).
                UiEvent::Command { dialog, link, .. } if (dialog, link) == s4_viewer::goods_race::BUTTON => {
                    if let Some(ui) = gpu.ui.as_mut()
                        && ui.state.goods_race
                    {
                        ui.open_goods_race(&sim.snapshot().population);
                        client::recompose(gpu);
                    }
                }
                UiEvent::DialogOpened(32) => self.job_target = 0,
                UiEvent::DialogOpened(30) => self.specialist_target = 0,
                UiEvent::Command { dialog: 32, link, .. } => match link {
                    765 => self.job_target = 1,
                    761 => self.job_target = 2,
                    760 => self.job_target = 3,
                    755..=759 if runs(0x1ba) => {
                        let value = match link {
                            756 => 5,
                            757 => -5,
                            758 => 1,
                            _ => -1,
                        };
                        let kind = self.job_target;
                        send(gpu, scene, view, sim, player, |eco| Command::SetUnitReservation { eco_sector: eco, kind, value: value as u32 });
                    }
                    _ => {}
                },
                // Code 10 (0x4acf20): the wheel moves the selected good up (−1) / down (+1).
                UiEvent::Wheel { dialog: 25, delta } if delta != 0 && runs(0x1be) => {
                    let good = gpu.ui.as_ref().map_or(-1, |u| u.state.page_info.transport.target);
                    if good > 0 {
                        let delta: i16 = if delta > 0 { -1 } else { 1 };
                        send(gpu, scene, view, sim, player, |eco| Command::SetTransportPriority { eco_sector: eco as u32, good: good as u16, delta });
                    }
                }
                UiEvent::Command { dialog: 25, link: link @ 0x6a3..=0x6a6, .. } if runs(0x1be) => {
                    let good = gpu.ui.as_ref().map_or(-1, |u| u.state.page_info.transport.target);
                    if good > 0 {
                        let delta: i16 = match link {
                            0x6a3 => 0,
                            0x6a4 => -1,
                            0x6a5 => 1,
                            _ => 100,
                        };
                        send(gpu, scene, view, sim, player, |eco| Command::SetTransportPriority { eco_sector: eco as u32, good: good as u16, delta });
                    }
                }
                UiEvent::Command { dialog: 26, link: 795..=799, .. } if runs(0x1bd) => {
                    let Some(d) = gpu.ui.as_ref().map(|u| u.state.page_info.distribution.clone()) else { continue };
                    let snap = sim.snapshot();
                    let eco = s4_viewer::client::own_eco_at_centre(gpu, scene, view, &snap.population);
                    let Some(e) = snap.population.economy.sector(eco) else { continue };
                    let old = e.supply_of(d.good);
                    for (building, delta) in supply_changes(&d, &old) {
                        let command = Command::SetBuildingSupplyPriority { eco_sector: eco, good: d.good as u16, building: building as u16, priority: delta as i16 };
                        tracing::info!("page: {command:?}");
                        sim.command(Stamped { tick: execution_tick(snap.tick), command: PlayerCommand { player, command } });
                    }
                }
                UiEvent::Command { dialog: 30, link, .. } => match link {
                    1251 => self.specialist_target = 50,
                    1252 => self.specialist_target = 49,
                    1253 => self.specialist_target = 46,
                    1254 => self.specialist_target = 47,
                    1255 => self.specialist_target = 48,
                    1268..=1272 if runs(0x1e1) => {
                        let value = match link {
                            1268 => 0,
                            1269 => 5,
                            1270 => -5,
                            1271 => 1,
                            _ => -1,
                        };
                        let specialist = self.specialist_target;
                        // `IsInEcoSector`: the page's fill found an own eco-sector (the same
                        // search as the handler's).
                        send(gpu, scene, view, sim, player, |eco| Command::SetSpecialistWish { eco_sector: eco, specialist, value: value as u32 });
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }
}

impl Pages {
    /// `CInputProcessor` 0x4ec230 (UI event 0x1b9 with the type and `mode`):
    /// * 0: the settler after the selected one in the player's list of the type (0x1544cec, `+0x2a`)
    ///   if exactly one settler of the type is selected, else the list's first; with an empty list
    ///   the soldier types go on with the next level (the swordsmen 0x1d → 0x1e → 0x1f, …). It
    ///   becomes the selection and the view is centred on it (event 0x13); none: the selection is
    ///   cleared (0x4edb10).
    /// * 2: at the camera node's eco-sector (`client::camera_node`, no search) the player's settlers of the
    ///   type (a soldier type: all three levels; the Trojan 0x3d: 0x3d..=0x3f) standing in it
    ///   (0x4ec160, the release's up to 100, ours up to `selection::MAX`), selected, the view centred on the first; none: cleared.
    ///
    /// [inf] The player check at 0x149b608 + player·0x3c is taken as passed.
    fn find(&mut self, gpu: &mut Gpu, scene: &Scene, view: View, pop: &Population, pointer: &mut Pointer, mode: u8) {
        let player = pointer.player;
        let list = |t: u16| pop.economy.lists.get(&(player, t)).cloned().unwrap_or_default();
        let found: Vec<(u16, u16, u16)> = match mode {
            0 => {
                let mut t = self.find_target;
                let mut cur = None;
                if let [one] = pointer.selection.ids[..]
                    && pop.settlers.iter().any(|u| u.id == one && u.kind == t)
                {
                    let l = list(t);
                    cur = l.iter().position(|e| e.0 == one).and_then(|i| l.get(i + 1)).copied();
                }
                while cur.is_none() {
                    cur = list(t).first().copied();
                    if cur.is_some() || !levelled(t) {
                        break;
                    }
                    t += 1;
                    cur = list(t).first().copied();
                    if !levelled(t) {
                        break;
                    }
                }
                cur.into_iter().collect()
            }
            2 => {
                let eco = client::camera_node(gpu, view, scene.source.w)
                    .and_then(|(x, y)| pop.economy.eco_of.get(y * pop.size + x).copied())
                    .unwrap_or(0);
                let t = self.find_target;
                let types: Vec<u16> = match t {
                    0x1d | 0x20 | 0x23 | 0x26 | 0x29 => vec![t, t + 1, t + 2],
                    0x3d => vec![0x3d, 0x3e, 0x3f, 0x3d],
                    _ => vec![t],
                };
                let mut sel: Vec<(u16, u16, u16)> = Vec::new();
                for e in types.into_iter().flat_map(list) {
                    let at = pop.economy.eco_of.get(e.2 as usize * pop.size + e.1 as usize).copied().unwrap_or(0);
                    // [inf] `SetId` keeps an entity once (the Trojans' type 0x3d is searched twice).
                    if eco != 0 && at == eco && sel.len() < s4_viewer::selection::MAX && !sel.iter().any(|s| s.0 == e.0) {
                        sel.push(e);
                    }
                }
                sel
            }
            1 => {
                // `sub_EE690`, the box state (+0x34 = 1, +0x66 = 1, +0x4c = the filter, +0x60 =
                // 2, the output's rectangle); the next draw collects, the box end requests the
                // dialog.
                let ty = s4_viewer::selection::entity_type(1, self.find_target, warrior_type(self.find_target));
                let ev = client::select_type_on_screen(gpu, scene, view, pop, &mut pointer.selection, player, ty);
                tracing::info!("page 29: search {:#x} mode 1: {:?} {ev:?}", self.find_target, pointer.selection.ids);
                return;
            }
            _ => return,
        };
        tracing::info!("page 29: search {:#x} mode {mode}: {:?}", self.find_target, found.iter().map(|e| e.0).collect::<Vec<_>>());
        if let Some(&(_, x, y)) = found.first() {
            // `sub_EE690` + `SetId`: the selection only. 0x4ec230 does not call `RequestDialog`
            // (0x4ee730, whose callers are the click / box selections), so the page stays open
            // while `CInputProcessor` goes to state 1.
            pointer.selection.ids = found.iter().map(|e| e.0).collect();
            pointer.selection.other.clear();
            self.centre = Some((x, y));
        } else {
            client::deselect(gpu, pop, &mut pointer.selection);
        }
    }
}

/// `m_uSettlerWarriorTypes` (release 0x12dbca0, filled at 0x581f20 from the (type, warrior,
/// level) table at file offset 0xc19a48): a settler type's warrior type, 0 for civilians.
fn warrior_type(t: u16) -> u8 {
    match t {
        29..=31 => 2,
        32..=34 => 3,
        35..=43 | 54 | 61..=63 => 4,
        44 => 5,
        45 => 6,
        46..=50 | 52 => 7,
        53 | 55 => 1,
        60 => 14,
        _ => 0,
    }
}

/// 0x4ebda0: per building of the page info (building, new − old share), the shares first made to
/// add up to 100 (one point at a time, cyclically, to the buildings whose new share is not 0).
/// [inf] Old shares past the eco-sector's own list are taken as 0 (the original reads its stack
/// there), and with every new share 0 nothing is adjusted (the original loops forever).
fn supply_changes(d: &s4_viewer::guistate::DistributionInfo, old: &[(i32, i32)]) -> Vec<(i32, i32)> {
    let n = d.count.clamp(0, 5) as usize;
    let new: Vec<i32> = (0..n).map(|i| d.list[i].1).collect();
    let mut delta: Vec<i32> = (0..n).map(|i| new[i] - old.get(i).map_or(0, |o| o.1)).collect();
    let mut rest = 100 - new.iter().sum::<i32>();
    if new.iter().any(|&v| v != 0) {
        let mut i = 0;
        while rest != 0 {
            if new[i] != 0 {
                let step = if rest < 0 { -1 } else { 1 };
                delta[i] += step;
                rest -= step;
            }
            i = (i + 1) % n;
        }
    }
    (0..n).map(|i| (d.list[i].0, delta[i])).collect()
}

/// The handler's eco-sector (0x4ebfe0: the player's first on the spiral around the screen
/// centre); nothing is sent without one.
fn send(gpu: &mut Gpu, scene: &Scene, view: View, sim: &SimHandle, player: u8, command: impl FnOnce(u16) -> Command) {
    let snap = sim.snapshot();
    let eco = s4_viewer::client::own_eco_at_centre(gpu, scene, view, &snap.population);
    if eco == 0 {
        return;
    }
    let command = command(eco);
    tracing::info!("page: {command:?}");
    sim.command(Stamped { tick: execution_tick(snap.tick), command: PlayerCommand { player, command } });
}
