//! The selected building's dialog (s4_viewer::building_dialog): the sim describes the watched
//! building in every snapshot, the GUI shows it, and the dialog's buttons become typed commands
//! the way `CInputProcessor` (handler table 0x4e96d0) turns the procedures' GUI events into net
//! commands:
//! * 0x198 next building (0x4e9e70): the player's next building of the type is selected and the
//!   view centred on it (event 0x13), walking `CBuildingMgr`'s list for (player, type) from a
//!   remembered cursor (0x14f0d78, 0x53d0b0; s4_viewer::build_goto). The build pages' right click
//!   (0x1b6, centre only) and Ctrl + right click (0x1e6, select the cursor's building) use the
//!   same cursor ([`BuildingDialog::build_page_goto`]).
//! * 0x193 work-area mode (0x4e9c50, state 3); the next map click (0x4ead10) sends 0x1389
//!   `SetWorkingArea` when 0x53d450 accepts the node, then the dialog is requested again
//!   (0x4edb10); a right click leaves the mode (state 3, event 0x191).
//! * 0x194 crush (0x4eaab0 → 0x536850): the confirmation D43 when the role allows it.
//! * 0x195 the confirmation's answer (0x4eaad0): yes → 0x138a `CrushBuilding` and the selection
//!   cleared (0x4edc90); no → the building's dialog again (0x4ee730).
//! * 0x196 stop / continue (0x4eabb0) → 0x138b `SwitchBuilding`; 0x197 site priority (0x4eac60)
//!   → 0x138c `PrioritizeBuildingSite`.

use s4_net::commands::{Stamped, execution_tick};
use s4_sim::{Command, PlayerCommand};
use s4_viewer::building_dialog::{proc_event, work_area_target};
use s4_viewer::client;
use s4_viewer::gpu::Gpu;
use s4_viewer::guistate::UiEvent;
use s4_viewer::build_goto;
use s4_viewer::spell_sidebar;
use s4_viewer::vehicle_group as vg;

use crate::select::Pointer;
use crate::sim::SimHandle;

#[derive(Default)]
pub struct BuildingDialog {
    /// The building the sim is asked to describe.
    watched: Option<u16>,
    /// Work-area mode (`CInputProcessor` state 3) for this building.
    pub work_area: Option<u16>,
    /// A map node to centre the view on (the "next" button's event 0x13).
    pub centre: Option<(u16, u16)>,
    /// Trade-target mode (`CInputProcessor` state 7, UI event 0x199) for this building.
    pub trade_target: Option<u16>,
    /// `CInputProcessor` stays in state 1 with an empty selection: a right click with a building
    /// selected (0x1e0 in state 1 → `SendTo` 0x4ec960 → `sub_EDC90`) empties the selection
    /// without a transition. The next map click (0x1dd / 0x1de / 0x191) or 0x192 ends it.
    pub held_one: bool,
    /// The spell mode (`CInputProcessor` state 8, UI events 0x1c1 / 0x1da) and the chosen spell
    /// (+0x14, −1 = none; s4_viewer::spell_sidebar).
    pub spell_mode: bool,
    pub spell: i32,
    /// `CBuildingMgr`'s cursors (0x14f0d78) of the local player by building type.
    pub cursors: std::collections::HashMap<u16, u16>,
    /// The soldier dialog's target mode (`CInputProcessor` state 9, UI event 0x19e from D45's
    /// 0x59e, handler 0x4ecb00 with cursor 1): the next map click sends the selection there as a
    /// Ctrl move (0x1dd → 0x4ecb20: key flags 8 → `SendTo` mode 1), then state 1 (0x191).
    pub target: bool,
    /// The camera window's pick mode (`CInputProcessor` state 10, UI event 0x1e7 from dialog 2's
    /// 0x282, handler 0x4eeb90): the next map click picks what it follows (0x1dd → 0x4eeba0,
    /// s4_viewer::mini_view), a right click leaves it (0x1e0 → 0x4edb10); both to state 1.
    pub mini_pick: bool,
}

#[cfg(test)]
thread_local! {
    /// The commands the dialogs sent (tests).
    pub static SENT: std::cell::RefCell<Vec<Command>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn send(sim: &SimHandle, player: u8, command: Command) {
    tracing::info!("building dialog: {command:?}");
    #[cfg(test)]
    SENT.with(|s| s.borrow_mut().push(command.clone()));
    sim.command(Stamped { tick: execution_tick(sim.snapshot().tick), command: PlayerCommand { player, command } });
}

impl BuildingDialog {
    /// Each frame: watch the selection's building (its first entity, if a building) and show or
    /// refresh its dialog.
    pub fn frame(&mut self, gpu: &mut Gpu, sim: &SimHandle, pointer: &Pointer) {
        let snap = sim.snapshot();
        // `RequestDialog` (0x4ee730) on the first selected: a building's dialog, or D41 of a cart
        // (land vehicle type 5, vtbl+0x94 0x55bdb0), a transport ship (ship type 3, 0x591dc0) or
        // a donkey (settler type 0x3c not in mode 7, role vtbl+0x84 0x577240).
        let first = pointer.selection.ids.first().copied().filter(|id| {
            let pop = &snap.population;
            pop.buildings.iter().any(|b| b.id == *id)
                || pop.vehicles.iter().any(|v| v.id == *id && ((v.obj == 4 && v.kind == s4_sim::vehicles::kind::CART) || (v.obj == 2 && v.kind == s4_sim::vehicles::kind::TRANSPORT_SHIP)))
                || pop.settlers.iter().any(|u| u.id == *id && u.obj == 1 && u.kind == s4_viewer::building_dialog::DONKEY && u.flags & 0xf != 7)
        });
        if first != self.watched {
            self.watched = first;
            self.work_area = None;
            self.trade_target = None;
            sim.watch_building(first);
        }
        let snap = sim.snapshot();
        if let Some(f) = snap.building.as_ref().filter(|f| Some(f.id) == first) {
            client::building_frame(gpu, f);
        }
    }

    /// The GUI's events of a click: the building dialogs' buttons. Each UI event runs only where
    /// `CInputProcessor` has a transition for it (input.rs; `build_mode`: state 4); the leave
    /// handler (state 3) ends the work-area mode instead.
    pub fn ui(&mut self, events: &[UiEvent], gpu: &mut Gpu, sim: &SimHandle, pointer: &mut Pointer, build_mode: bool) {
        for e in events {
            if self.selection_dialogs(e, gpu, sim, pointer, build_mode) || self.build_page_goto(e, gpu, sim, pointer, build_mode) {
                continue;
            }
            let UiEvent::Command { dialog, link, effects } = *e else { continue };
            if crate::input::sends_192(dialog, link) {
                // The main and submenu tabs send 0x192 (the main tabs also 0x1cc and 0x260):
                // 0x192's handler 0x4edac0 (every state) deselects (0x4ee690: the selection
                // emptied, without the 0x25b `ResetMainMenu` of 0x4edc90), clears the sidebar
                // provider and runs 0x4edb10 (the modes left); the tab's own reaction has closed
                // the building dialog and its sidebar.
                if let Some(ui) = gpu.ui.as_mut() {
                    ui.close_sidebar();
                }
                self.work_area = None;
                self.trade_target = None;
                self.held_one = false;
                self.spell_mode = false;
                let snap = sim.snapshot();
                client::deselect(gpu, &snap.population, &mut pointer.selection);
                continue;
            }
            let side = gpu.ui.as_mut().and_then(|ui| ui.sidebar_click(dialog, link, effects));
            let Some((ev, w, l)) = side.or_else(|| proc_event(dialog, link, effects)) else { continue };
            // The sidebar toggles of D19 / D22 send by the building type (0x4a06cd: 0x1c5 for the
            // weapon smith; 0x46ca83: 0x1c3 for the barracks, else 0x1c6): the transition is the
            // sent event's.
            let ev = match ev {
                0x1c4 | 0x1c6 => {
                    let snap = sim.snapshot();
                    let kind = snap.building.as_ref().filter(|f| pointer.selection.ids.first() == Some(&f.id)).map(|f| f.kind);
                    match (ev, kind) {
                        (0x1c4, Some(22)) => 0x1c5,
                        (0x1c6, Some(24)) => 0x1c3,
                        _ => ev,
                    }
                }
                _ => ev,
            };
            let state = self.state_of(build_mode, pointer);
            match crate::input::transition(state, ev) {
                None => {
                    tracing::info!("building dialog: UI event {ev:#x} has no transition in state {state}");
                    continue;
                }
                Some((_, crate::input::LEAVE)) => {
                    // 0x4edb10: the mode left (+0x50 = 0), the dialog refilled, 0x260 closes the
                    // sidebar.
                    self.work_area = None;
                    self.trade_target = None;
                    if let Some(ui) = gpu.ui.as_mut() {
                        ui.close_sidebar();
                    }
                    client::recompose(gpu);
                    continue;
                }
                Some(_) => {}
            }
            let snap = sim.snapshot();
            let Some(f) = snap.building.clone().filter(|f| pointer.selection.ids.first() == Some(&f.id)) else { continue };
            let player = pointer.player;
            tracing::info!("building dialog: UI event {ev:#x} ({w}) for building {}", f.id);
            // `CTutorial::OnEvent` hears the building dialog's events (tutorial.rs), the sidebar
            // buttons with the id the original sends (see the 0x1c3..=0x1cc case below).
            let tut_ev = match (ev, f.kind) {
                (0x1c4, 22) => 0x1c5,
                (0x1c6, 24) => 0x1c3,
                _ => ev,
            };
            crate::tutorial::building_event(&sim.tutorial, tut_ev as u32, w as i32, l as i32, f.kind as u16);
            match ev {
                0x198 => self.go_to(gpu, &snap, pointer, f.kind as u16, true),
                0x193 => self.work_area = Some(f.id),
                0x194 => {
                    let opened = gpu.ui.as_mut().is_some_and(|ui| ui.crush_request(&f));
                    if opened {
                        client::recompose(gpu);
                    } else {
                        // 0x549300 → `AddMessage` 0x52a460 (2551 GUI_WARN_TOWER_NO_CRUSH). Its
                        // class (0x52a660, Config\WarningTypes.cfg [WARNINGMSG_CLASSIFICATION]) is a
                        // key without a value: no class bit, so the message is filtered out and
                        // nothing is shown. [inf] the parser reads a valueless key as 0 or not at all.
                        tracing::info!("building dialog: crush refused (string 2551, not shown)");
                    }
                }
                0x195 if w == 1 => {
                    send(sim, player, Command::CrushBuilding { building: f.id as u32 });
                    client::clear_selection(gpu, &snap.population, &mut pointer.selection);
                }
                0x195 => {
                    if let Some(ui) = gpu.ui.as_mut() {
                        ui.rerequest_building();
                    }
                    client::building_frame(gpu, &f);
                }
                0x196 => {
                    send(sim, player, Command::SwitchBuilding { building: f.id as u32 });
                    crate::tutorial::building_event(&sim.tutorial, 0x138b, 0, 0, f.kind as u16);
                }
                0x197 => send(sim, player, Command::PrioritizeBuildingSite { building: f.id as u32 }),
                // 0x4eae00 / 0x4eaeb0: 0x13a7 FillTower (l), 0x13a8 EmptyTower (building << 16 | w, l).
                0x1bb => send(sim, player, Command::FillTower { building: f.id as u32, value: l }),
                0x1bc => send(sim, player, Command::EmptyTower { building: f.id, kind: w as u16, amount: l }),
                // 0x4eeaa0: 0x13a6 CancelProduction, then the selection cleared (0x4edc90).
                0x1bf => {
                    send(sim, player, Command::CancelProduction { building: f.id as u32 });
                    client::clear_selection(gpu, &snap.population, &mut pointer.selection);
                }
                0x199 => self.trade_target = Some(f.id),
                // The sidebars: D19's order-page button sends 0x1c4 for the tool smith (21),
                // 0x1c5 for the weapon smith (22) (0x4a06cd); D22's 0x1c3 for the barracks (24,
                // 0x46ca83), else 0x1c6, which has no handler; 0x1c9 storage; 0x1c7 trading
                // (D53), 0x1c8 (D54); 0x1cb the workshop (D51, 0x4ebca0); 0x1cc closes.
                0x1c3..=0x1cc => {
                    let ev = match (ev, f.kind) {
                        (0x1c4, 22) => 0x1c5,
                        (0x1c4, 21) => 0x1c4,
                        (0x1c4, _) => continue,
                        (0x1c6, 24) => 0x1c3,
                        (0x1c6, _) => continue,
                        _ => ev,
                    };
                    if gpu.ui.as_mut().is_some_and(|ui| ui.open_sidebar(&f, ev)) {
                        client::recompose(gpu);
                    } else {
                        tracing::info!("building dialog: sidebar {ev:#x} not ported");
                    }
                }
                // 0x4ea4e0: 0x13b0 WorkshopOrder (selected entity << 16 | type, amount) from D51.
                0x1d5 => send(sim, player, Command::WorkshopOrder { building: f.id, item: w as u16, amount: l }),
                // D41 (0x4ae300): 0x1ab the next vehicle of the type (0x4e9cc0, event 0x13);
                // 0x1a1 (0x4ed0f0) 0x1391 VehicleUnload and 0x1c0 (0x4ee940) 0x13b9 DestroyVehicle
                // for every selected entity, the latter then clears the selection (0x4edc10,
                // 0x4ee690, 0x25b); 0x1a3 (0x4ece80) 0x1394 CreateFoundationCart and 0x1a5
                // (0x4ed030) 0x1393 CreateSettlement for the first.
                0x1ab => self.next_vehicle(gpu, &snap, pointer),
                0x1a1 => {
                    for &id in &pointer.selection.ids {
                        send(sim, player, Command::VehicleUnload { vehicle: id as u32 });
                    }
                }
                0x1a3 => send(sim, player, Command::CreateFoundationCart { cart: f.id as u32 }),
                0x1a5 => send(sim, player, Command::CreateSettlement { cart: f.id as u32 }),
                0x1c0 => {
                    for &id in &pointer.selection.ids {
                        send(sim, player, Command::DestroyVehicle { vehicle: id as u32 });
                    }
                    client::clear_selection(gpu, &snap.population, &mut pointer.selection);
                }
                // 0x4ea850: 0x13b7 SoldierOrder (selected entity << 16 | type, amount); state 1
                // only (a selection).
                0x1cd => send(sim, player, Command::SoldierOrder { building: f.id, kind: w as u16, amount: l }),
                // 0x4ea7a0: 0x13a9 ProductionOrder (building << 16 | good, amount).
                0x1ce => send(sim, player, Command::ProductionOrder { building: f.id, good: w as u16, amount: l }),
                // 0x4ea640: 0x13aa SetWeaponPercentage (building, the four packed percentages).
                0x1cf => send(sim, player, Command::SetWeaponPercentage { building: f.id as u32, percent: w }),
                // 0x4ea6f0: 0x13ab SetWeaponAutoProduction (l).
                0x1d0 => send(sim, player, Command::SetWeaponAutoProduction { building: f.id as u32, value: l }),
                // 0x4eb0f0: 0x138e TransportGood (selected entity << 16 | good, amount); state 1.
                0x1d1 => send(sim, player, Command::TransportGood { building: f.id, good: w as u16, amount: l }),
                // 0x4eb250: 0x1390 TradeGood (selected entity, the event's good).
                0x1d3 => send(sim, player, Command::TradeGood { building: f.id as u32, value: w }),
                // 0x4eb300: 0x13ba SetTradeStatus (selected entity << 16 | player, pressed).
                0x1e3 => send(sim, player, Command::SetTradeStatus { building: f.id, player: w as u16, value: l }),
                // 0x4ea590: 0x13b4 StorageGoodSwitch (building << 16 | good, l).
                0x1d4 => send(sim, player, Command::StorageGoodSwitch { building: f.id, good: w as u16, value: l }),
                _ => {}
            }
        }
    }

    /// `CInputProcessor`'s state for the client's modes (input.rs).
    pub fn state_of(&self, build_mode: bool, pointer: &Pointer) -> u8 {
        let s = crate::input::state(build_mode, self.work_area.is_some(), self.trade_target.is_some(), self.target, !pointer.selection.ids.is_empty() || self.held_one);
        if self.mini_pick {
            10
        } else if self.spell_mode && s <= 1 {
            8
        } else {
            s
        }
    }

    /// The selection dialogs' buttons: the soldiers' D45 (procedure 0x4a6b80), the specialists'
    /// D44 (0x4a7350) and the groups sidebar D55 (0x479270). Returns whether the event was theirs.
    /// * D45 0x59d / D44 0x5b9 → UI 0x19f (w = Shift; 0x4ecda0): keep the wounded selected,
    ///   with Shift the healthy ([`by_health`]).
    /// * D45 0x59e → 0x19e: the target mode ([`BuildingDialog::target`]).
    /// * D45 0x59f → 0x19c (0x4ecb60: `SendTo` with node −1): the selection holds its ground
    ///   (mode 8).
    /// * D45 0x5a0 / D44 0x5ba (toggles) → 0x1c2: the groups sidebar; released → 0x1cc and 0x260
    ///   (closed). D45 0x5a4 → 0x1c1, the spells sidebar D56 (0x56a670, `s4_viewer::spell_sidebar`).
    /// * D44 0x5bb → 0x1a7 (0x4ecb90): net 5021 `MorphToCarrier` with the selection.
    /// * D55's buttons → 0x1d6 / 0x1d8 / 0x1d9 ([`BuildingDialog::group_event`]).
    /// * The vehicle group dialog D40 (0x4ae790) and the ferry's D42 (0x474f80): see
    ///   [`vg::group_event`] / [`vg::ferry_event`]; 0x1a0 (warships) has no transition.
    fn selection_dialogs(&mut self, e: &UiEvent, gpu: &mut Gpu, sim: &SimHandle, pointer: &mut Pointer, build_mode: bool) -> bool {
        let (dialog, link, effects, right) = match *e {
            UiEvent::Command { dialog, link, effects } => (dialog, link, effects, false),
            UiEvent::Context { dialog, link } => (dialog, link, 0, true),
            _ => return false,
        };
        let pressed = effects & 1 != 0;
        let (ev, w) = match (dialog, link, right) {
            (45, 1437, false) | (44, 1465, false) => (0x19f, pointer.shift as u8),
            (45, 1438, false) => (0x19e, 0),
            (45, 1439, false) => (0x19c, 0),
            (45, 1440, false) | (44, 1466, false) => (if pressed { 0x1c2 } else { 0x1cc }, 0),
            (45, 1444, false) => (if pressed { 0x1c1 } else { 0x1cc }, 0),
            (44, 1467, false) => (0x1a7, 0),
            (s4_viewer::group_sidebar::DIALOG, l, r) => match s4_viewer::group_sidebar::click(l, pointer.ctrl || pointer.shift, r) {
                Some((ev, g)) => (ev, g),
                None => return true,
            },
            (vg::GROUP, l, false) => match vg::group_event(l, pressed) {
                Some(ev) => (ev, 0),
                None => return true,
            },
            (vg::FERRY, l, false) => match vg::ferry_event(l) {
                Some(ev) => (ev, 0),
                None => return true,
            },
            // D56 (0x47bbc0): 0x1da with the spell (w = 0xff: −1, none).
            (spell_sidebar::DIALOG, l, false) => match spell_sidebar::click(l, pressed) {
                Some(w) => (0x1da, w as u8),
                None => return true,
            },
            (44 | 45 | vg::GROUP | vg::FERRY | spell_sidebar::DIALOG, _, _) => return true,
            _ => return false,
        };
        let state = self.state_of(build_mode, pointer);
        if crate::input::transition(state, ev).is_none() {
            tracing::info!("selection dialog: UI event {ev:#x} has no transition in state {state}");
            return true;
        }
        // State 8 left (to 1: 0x19b / 0x19c / 0x19f / 0x1cc; to 9: 0x19e).
        if state == 8 && matches!(ev, 0x19b | 0x19c | 0x19f | 0x1cc | 0x19e) {
            self.spell_mode = false;
        }
        let snap = sim.snapshot();
        let player = pointer.player;
        tracing::info!("selection dialog: UI event {ev:#x} ({w})");
        match ev {
            0x19f => {
                let ids = by_health(&snap.population, &pointer.selection.ids, w != 0);
                client::narrow_selection(gpu, &snap.population, &mut pointer.selection, ids);
            }
            0x19e => self.target = true,
            0x19c => {
                let ids: Vec<u16> = pointer.selection.ids.iter().copied().take(s4_viewer::selection::MAX).collect();
                if !ids.is_empty() {
                    send(sim, player, Command::SendTo { x: 0xffff, y: 0xffff, mode: 8, entities: ids });
                }
            }
            0x1c2 => {
                if gpu.ui.as_mut().is_some_and(|ui| ui.open_groups_sidebar(&snap.population)) {
                    client::recompose(gpu);
                }
            }
            // 0x4eba40: no spell chosen, the fill 0x56a670 → D56 (state 8).
            0x1c1 => {
                self.spell_mode = true;
                self.spell = -1;
                if gpu.ui.as_mut().is_some_and(|ui| ui.open_spells_sidebar(&snap.population)) {
                    client::recompose(gpu);
                }
            }
            // 0x4eb860: +0x14 = w (state 8).
            0x1da => {
                self.spell_mode = true;
                self.spell = if w == 0xff { -1 } else { w as i32 };
            }
            // 0x4ebce0: the provider cleared; the procedure's 0x260 closes the sidebar.
            0x1cc => {
                if let Some(ui) = gpu.ui.as_mut() {
                    ui.close_sidebar();
                }
                client::recompose(gpu);
            }
            0x1a7 => {
                let entities: Vec<u16> = pointer.selection.ids.clone();
                if !entities.is_empty() {
                    send(sim, player, Command::MorphToCarrier { entities });
                }
            }
            0x1d6 | 0x1d8 | 0x1d9 => self.group_event(ev, w, gpu, sim, pointer),
            // D40 / D42: 0x4ed0f0 5009 and 0x4ed1d0 5010 for each selected entity; 0x4ee940 5049
            // for each, then the selection cleared (0x4edc10, 0x4ee690, 0x25b).
            0x1a1 => {
                for &id in &pointer.selection.ids {
                    send(sim, player, Command::VehicleUnload { vehicle: id as u32 });
                }
            }
            0x1a2 => {
                for &id in &pointer.selection.ids {
                    send(sim, player, Command::CatapultLoad { vehicle: id as u32 });
                }
            }
            0x1c0 => {
                for &id in &pointer.selection.ids {
                    send(sim, player, Command::DestroyVehicle { vehicle: id as u32 });
                }
                client::clear_selection(gpu, &snap.population, &mut pointer.selection);
            }
            0x1ab => self.next_vehicle(gpu, &snap, pointer),
            // 0x4eccc0: GUI event 0x260 (the sidebar closed), then the group fill 0x5972b0 for the
            // selection: D40 in place of the ferry's D42.
            0x1a6 => {
                if let Some(ui) = gpu.ui.as_mut() {
                    ui.close_sidebar();
                    ui.show_group_dialog(&snap.population, &pointer.selection.ids);
                }
                client::recompose(gpu);
            }
            _ => {}
        }
        true
    }

    /// UI event 0x1ab (0x4e9cc0; D41's 0x6b1, D42's 0x2ca): the local player's next vehicle of the
    /// first selected one's type ([`vg::next_vehicle`]) selected alone (`SetId`, `RequestDialog`)
    /// and the view centred on it (event 0x13). [inf] A first selected vehicle missing from the
    /// local player's list does nothing (the original would follow its own list and wrap to the
    /// local head).
    fn next_vehicle(&mut self, gpu: &mut Gpu, snap: &crate::sim::Snapshot, pointer: &mut Pointer) {
        let pop = &snap.population;
        let Some(v) = pointer.selection.ids.first().and_then(|&id| pop.vehicles.iter().find(|u| u.id == id)) else { return };
        let list = pop.economy.vehicle_lists.get(&(pointer.player, v.kind)).cloned().unwrap_or_default();
        let flags = |id: u16| pop.vehicles.iter().find(|u| u.id == id).map_or(0, |u| u.flags);
        let Some(next) = vg::next_vehicle(&list, v.id, &flags) else { return };
        let Some(n) = pop.vehicles.iter().find(|u| u.id == next) else { return };
        client::select_ids(gpu, pop, &mut pointer.selection, vec![next]);
        self.centre = Some((n.x, n.y));
    }

    /// The group UI events (D55's buttons, the digit keys): 0x1d6 (0x4ec630) net 5041
    /// `DefineGroup` with the selected ids (the release's first 100, ours up to
    /// `selection::MAX`); 0x1d9 (0x4ec760) the view to the mean
    /// position of the selected own non-buildings (event 0x13, [`BuildingDialog::centre`]); 0x1d8
    /// (0x4ec880, Shift adds): the group's selectable members (flag 0x2000), up to the cap, replace
    /// the selection (Shift: are added, while the group's first member and the first selected
    /// entity are of one selection category, 0x4e6a40 vs 0x4eeb50), then `RequestDialog`.
    pub fn group_event(&mut self, ev: u16, g: u8, gpu: &mut Gpu, sim: &SimHandle, pointer: &mut Pointer) {
        let snap = sim.snapshot();
        let pop = &snap.population;
        let local = pointer.player;
        match ev {
            0x1d6 => {
                if !pointer.selection.ids.is_empty() {
                    let entities: Vec<u16> = pointer.selection.ids.iter().copied().take(s4_viewer::selection::MAX).collect();
                    send(sim, local, Command::DefineGroup { group: g as u32, entities });
                }
            }
            0x1d9 => {
                let own: Vec<(u16, u16)> = pointer
                    .selection
                    .ids
                    .iter()
                    .filter_map(|id| pop.settlers.iter().chain(&pop.vehicles).find(|u| u.id == *id && u.owner == local))
                    .map(|u| (u.x, u.y))
                    .collect();
                if !own.is_empty() {
                    let n = own.len() as u32;
                    let x = own.iter().map(|p| p.0 as u32).sum::<u32>() / n;
                    let y = own.iter().map(|p| p.1 as u32).sum::<u32>() / n;
                    self.centre = Some((x as u16, y as u16));
                }
            }
            0x1d8 => {
                let members = pop.groups.get(local as usize * s4_sim::group::GROUPS + g as usize).cloned().unwrap_or_default();
                if members.is_empty() {
                    return;
                }
                // 0x4ec880: with Shift the selection stays only when the group's first member and
                // the first selected entity are of one selection category (0x4e6a40 / 0x4eeb50).
                let keep = pointer.shift && selection_category(pop, members[0]) == pointer.selection.ids.first().map_or(0, |&id| selection_category(pop, id));
                let mut ids = if keep { pointer.selection.ids.clone() } else { Vec::new() };
                for id in members {
                    if ids.len() >= s4_viewer::selection::MAX {
                        break;
                    }
                    let selectable = pop.settlers.iter().chain(&pop.vehicles).any(|u| u.id == id && u.flags & 0x2000 != 0);
                    if selectable && !ids.contains(&id) {
                        ids.push(id);
                    }
                }
                // The dialogs follow inside (`RequestDialog`), as for a box selection.
                client::select_ids(gpu, pop, &mut pointer.selection, ids);
            }
            _ => {}
        }
    }

    /// A left click on map node `node` in the target mode (state 9, 0x1dd → 0x4ecb20): the
    /// selection sent there as a Ctrl move (mode 1), the mode left (0x4edb10, 0x191). Returns
    /// whether the mode took the click.
    pub fn target_click(&mut self, sim: &SimHandle, pointer: &Pointer, node: (usize, usize)) -> bool {
        if !std::mem::take(&mut self.target) {
            return false;
        }
        let ids: Vec<u16> = pointer.selection.ids.iter().copied().take(s4_viewer::selection::MAX).collect();
        if !ids.is_empty() {
            send(sim, pointer.player, Command::SendTo { x: node.0 as u16, y: node.1 as u16, mode: 1, entities: ids });
        }
        true
    }

    /// A build page's right click on a building button (code 7, s4_viewer::build_goto): Ctrl
    /// alone → UI event 0x1e6 ([`BuildingDialog::go_to`] without advancing; for types 21..=24 the
    /// next sidebar dialog presses its toggle, set even without a transition), else 0x1b6 (the
    /// cursor advanced, the view centred on its building). Returns whether the event was theirs.
    fn build_page_goto(&mut self, e: &UiEvent, gpu: &mut Gpu, sim: &SimHandle, pointer: &mut Pointer, build_mode: bool) -> bool {
        let UiEvent::Context { dialog: dialog @ (9..=13 | s4_viewer::eyecatcher_sidebar::DIALOG), link } = *e else { return false };
        let Some(kind) = s4_viewer::guistate::build_button(dialog, link) else { return false };
        // The eye catchers' sidebar (0x46ecf0 code 7) sends 0x1b6 whatever the keys.
        let (ev, defer) = if dialog == s4_viewer::eyecatcher_sidebar::DIALOG { (0x1b6, false) } else { build_goto::right_click(kind, pointer.ctrl, pointer.shift) };
        if defer && let Some(ui) = gpu.ui.as_mut() {
            ui.deferred_press = true;
        }
        let state = self.state_of(build_mode, pointer);
        if crate::input::transition(state, ev).is_none() {
            tracing::info!("build page: UI event {ev:#x} has no transition in state {state}");
            return true;
        }
        let snap = sim.snapshot();
        if ev == 0x1e6 {
            self.go_to(gpu, &snap, pointer, kind as u16, false);
        } else {
            // 0x4ea150: 0x53d0b0, then event 0x13 to the building.
            let list = build_goto::building_list(&snap.population, pointer.player, kind as u16);
            let id = build_goto::advance(&list, self.cursors.entry(kind as u16).or_insert(0));
            if let Some(b) = snap.population.buildings.iter().find(|b| id != 0 && b.id == id) {
                self.centre = Some((b.x, b.y));
            }
        }
        true
    }

    /// UI events 0x198 (0x4e9e70, `advance`) and 0x1e6 (0x4e9fd0): the selection dropped
    /// (0x4ee690) and the modes left (0x4edb10: the sidebar closed), then the local player's
    /// cursor of `kind` (0x53d0b0 first for 0x198; for 0x1e6 only when 0): its building selected
    /// alone (`SetId`, `RequestDialog`) and the view centred on it (event 0x13).
    fn go_to(&mut self, gpu: &mut Gpu, snap: &crate::sim::Snapshot, pointer: &mut Pointer, kind: u16, advance: bool) {
        let pop = &snap.population;
        self.work_area = None;
        self.trade_target = None;
        self.target = false;
        self.held_one = false;
        if let Some(ui) = gpu.ui.as_mut() {
            ui.close_sidebar();
        }
        client::deselect(gpu, pop, &mut pointer.selection);
        let list = build_goto::building_list(pop, pointer.player, kind);
        let cursor = self.cursors.entry(kind).or_insert(0);
        let id = if advance { build_goto::advance(&list, cursor) } else { build_goto::current(&list, cursor) };
        // [inf] An entity that is not a building any more (the cursor outlived it) is not selected.
        if let Some(b) = pop.buildings.iter().find(|b| id != 0 && b.id == id) {
            client::select_ids(gpu, pop, &mut pointer.selection, vec![id]);
            self.centre = Some((b.x, b.y));
        }
    }

    /// `Tutorial.SelectNextBuilding(type)` (0x5cb060): UI event 0x198 with lParam = `kind`
    /// (0x4e9e70 takes it instead of the selection's type).
    pub fn select_next_building(&mut self, gpu: &mut Gpu, sim: &SimHandle, pointer: &mut Pointer, kind: u8) {
        let snap = sim.snapshot();
        self.go_to(gpu, &snap, pointer, kind as u16, true);
    }

    /// A left click on map node `node` in work-area mode (0x4ead10). Returns whether the mode
    /// took the click.
    pub fn map_click(&mut self, gpu: &mut Gpu, sim: &SimHandle, player: u8, node: (usize, usize)) -> bool {
        let Some(id) = self.work_area.take() else { return false };
        let snap = sim.snapshot();
        let Some(f) = snap.building.as_ref().filter(|f| f.id == id) else { return true };
        match work_area_target(&snap.population, f, (node.0 as i32, node.1 as i32)) {
            Some((x, y)) => {
                send(sim, player, Command::SetWorkingArea { building: id as u32, x, y });
                crate::tutorial::building_event(&sim.tutorial, 0x1389, 0, 0, f.kind as u16);
                // 0x4edb10: the dialog is requested again. When the command runs, 5001 sends the
                // local player UI event 0x193 (0x4f6312), which enters the work-area mode again
                // and shows the new area. [inf] Here the mode stays on from the click (the
                // circle follows once the sim has moved the area).
                if let Some(ui) = gpu.ui.as_mut() {
                    ui.rerequest_building();
                }
                client::building_frame(gpu, f);
                self.work_area = Some(id);
            }
            // Refused: the mode stays (0x4ead10 returns without leaving state 3).
            None => {
                tracing::info!("building dialog: work area at {node:?} refused");
                self.work_area = Some(id);
            }
        }
        true
    }

    /// A left click on map node `node` in trade-target mode (0x4eaf90): 0x138d for every
    /// accepted building around it, then the dialog again (0x4edb10) and the mode left. Returns
    /// whether the mode took the click.
    pub fn trade_click(&mut self, gpu: &mut Gpu, sim: &SimHandle, player: u8, node: (usize, usize)) -> bool {
        let Some(id) = self.trade_target else { return false };
        let snap = sim.snapshot();
        let Some(f) = snap.building.as_ref().filter(|f| f.id == id) else { return true };
        let size = snap.population.size;
        let visible = |x: i32, y: i32| snap.fog.as_ref().is_none_or(|fog| fog.is_position_visible(size, x, y));
        let targets = s4_viewer::building_dialog::trade_targets(&snap.population, f, (node.0 as i32, node.1 as i32), &visible);
        for &t in &targets {
            send(sim, player, Command::SetTradeTarget { building: id as u32, target: t as u32 });
        }
        if !targets.is_empty() {
            self.trade_target = None;
            if let Some(ui) = gpu.ui.as_mut() {
                ui.rerequest_building();
            }
            client::building_frame(gpu, f);
        }
        true
    }

    /// The working-area rings to draw while the work-area mode is on (0x53d2e0).
    pub fn markers(&self, snap: &crate::sim::Snapshot) -> Vec<(u16, u16, u8)> {
        let Some(id) = self.work_area else { return Vec::new() };
        snap.building.as_ref().filter(|f| f.id == id).map_or(Vec::new(), |f| s4_viewer::building_dialog::work_area_markers(&snap.population, f))
    }

    /// Whether a map-click mode (work area, trade target) is on.
    pub fn in_mode(&self) -> bool {
        self.work_area.is_some() || self.trade_target.is_some() || self.target
    }

    /// A right click on map node `node` in the spell mode (state 8, 0x1e0 → 0x4eb880): with a
    /// chosen spell, net 5048 `CastSpell` for the selected priest nearest to it; returns whether
    /// it was taken (without a spell the click is `SendTo`, as in state 1).
    pub fn cast(&mut self, sim: &SimHandle, pointer: &Pointer, node: (usize, usize)) -> bool {
        if !self.spell_mode || self.spell < 0 {
            return false;
        }
        let snap = sim.snapshot();
        let priests: Vec<(u16, u16, u16)> =
            pointer.selection.ids.iter().filter_map(|id| snap.population.settlers.iter().find(|u| u.id == *id && u.kind == 45)).map(|u| (u.id, u.x, u.y)).collect();
        if let Some(priest) = spell_sidebar::caster(&priests, node.0 as u16, node.1 as u16) {
            send(sim, pointer.player, Command::CastSpell { priest, spell: self.spell as u16, x: node.0 as u16, y: node.1 as u16 });
        }
        true
    }

    /// A right click on the map: leaves the work-area or trade-target mode (states 3 / 7, event
    /// 0x191). Returns whether one was on.
    pub fn cancel(&mut self) -> bool {
        if std::mem::take(&mut self.mini_pick) {
            // State 10 → 1 also without a selection.
            self.held_one = true;
            return true;
        }
        self.work_area.take().is_some() | self.trade_target.take().is_some() | std::mem::take(&mut self.target)
    }
}

/// UI event 0x19f (0x4ecda0, D45 0x59d / D44 0x5b9 / the Backspace key): of `ids`, the settlers
/// whose hitpoints are below half the type's maximum (`SettlerInfo +2`, table 0x1546930) stay,
/// with `healthy` those at half or above; other entities stay.
pub fn by_health(pop: &s4_viewer::population::Population, ids: &[u16], healthy: bool) -> Vec<u16> {
    ids.iter()
        .copied()
        .filter(|id| match pop.settlers.iter().find(|u| u.id == *id) {
            Some(u) => (u.health < u.max_health >> 1) != healthy,
            None => true,
        })
        .collect()
}

/// 0x4e69a0 (through 0x4e6a20 / 0x4e6a40): the selection category of entity `id` — a building
/// (obj type 8) 0x10000, else by its warrior type (flags & 0xf, table 0x4e69ec): 1..=5 the
/// type plus 0x30000, 6 0x30000, 7 (kind + 0x200) << 8, 8 / 9 0x40000, 10 / 11 0x50000, others
/// 0; 0 without an entity.
fn selection_category(pop: &s4_viewer::population::Population, id: u16) -> u32 {
    if pop.buildings.iter().any(|b| b.id == id) {
        return 0x10000;
    }
    let Some(u) = pop.settlers.iter().chain(&pop.vehicles).find(|u| u.id == id) else { return 0 };
    match u.flags & 0xf {
        w @ 1..=5 => w + 0x30000,
        6 => 0x30000,
        7 => (u.kind as u32 + 0x200) << 8,
        8 | 9 => 0x40000,
        10 | 11 => 0x50000,
        _ => 0,
    }
}

#[cfg(test)]
mod category_tests {
    use super::*;
    use s4_viewer::population::{Population, Unit};

    /// 0x4e69a0's categories: soldiers by type, the other warrior types grouped, type 7 by kind.
    #[test]
    fn selection_categories() {
        let u = |id: u16, w: u32, kind: u16| Unit { id, flags: 0x2000 | w, kind, ..Default::default() };
        let pop = Population { settlers: vec![u(1, 1, 29), u(2, 1, 30), u(3, 5, 31), u(4, 6, 40), u(5, 7, 41), u(6, 7, 42), u(7, 9, 50), u(8, 11, 51), u(9, 0, 1)], ..Default::default() };
        let c = |id| selection_category(&pop, id);
        assert_eq!((c(1), c(2), c(3), c(4)), (0x30001, 0x30001, 0x30005, 0x30000));
        assert_eq!((c(5), c(6)), ((41 + 0x200) << 8, (42 + 0x200) << 8));
        assert_eq!((c(7), c(8), c(9), c(99)), (0x40000, 0x50000, 0, 0));
    }
}
