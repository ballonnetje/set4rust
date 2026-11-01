//! The tutorial (game type 4; decompiles in `tools/re/decompiled/script/tutorial/`).
//!
//! * The tutorial menu (`CStateTutorial`, vtable 0x10575bc, 0x527c10): button `i` loads
//!   `Map\Tutorial\Tutorial%02i.map` (`i + 1`) as a single-player game with
//!   `GameState +0x2a8 = 4`; the script is the `Script\` file.
//! * At the game start (0x513190) `SetupTutorialEvent` (0x4fa5b0) registers the `CTutorial`
//!   singleton (0x11abe24) as an event handler and the GUI hook 0x146b054 = 0x4fa520 (which
//!   controls may be enabled, [`Controls::hook`]); `Exit` (0x513b10 → 0x4fa6a0) removes both.
//! * `CTutorial::OnEvent` (0x4fa780) turns UI events into script events ([`script_event`]).
//! * The `Tutorial.*` Lua functions (0x5ca720 …) show text in the tutorial window (UI event
//!   0x263), set the marker (0x26e), the world cursor (effect 0x39), the zoom (event 0x12),
//!   press buttons (`IGuiEngine::SelectControl` 0x677430), select the next building (UI event
//!   0x198), disable controls and end the tutorial (`Won`: event 0x37, `Exit`: event 0x17).
//!
//! [`TutorialUi`] is what the script (on the sim thread) and the client share: the client draws
//! from it and drains its requests; the script reads the client's camera from it and gets the
//! script events the client queues ([`TutorialUi::pending`]). The original calls the script
//! straight from the UI handler between two logic ticks; here the events run at the start of the
//! next script update, before its `TICK` (the same order).

use std::sync::{Arc, Mutex};

use s4_script::events;

/// The shared tutorial state.
pub type Shared = Arc<Mutex<TutorialUi>>;

pub fn shared() -> Shared {
    Arc::new(Mutex::new(TutorialUi::default()))
}

/// Whether a map is one of the tutorial missions (`Map\Tutorial\…`). The original knows the game
/// type from the start screen; s4rs's `--map Tutorial/TutorialNN` starts a tutorial the same way.
pub fn is_tutorial_map(path: &std::path::Path) -> bool {
    path.parent().and_then(|p| p.file_name()).is_some_and(|n| n.eq_ignore_ascii_case("Tutorial"))
}

/// The tutorial map `n` (1..=12) as the menu loads it (0x527c10: `Tutorial%02i.map`).
#[allow(dead_code)] // for the tutorial menu (gui, T2)
pub fn map_name(n: u32) -> String {
    format!("Tutorial/Tutorial{n:02}")
}

/// Something the script asks the client to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    /// `Tutorial.PressButton(dialog, control)`: `IGuiEngine::SelectControl(dialog, control, 1)`
    /// (0x5cacc0; dialog ≤ 0x4d, control 1..=0xb8b).
    PressButton { dialog: u8, control: u16 },
    /// `Tutorial.SelectNextBuilding(type)`: UI event 0x198 (0, type, local player) (0x5cb060).
    SelectNextBuilding(u8),
    /// `Tutorial.SetZoom(level)`: event 0x12 with the level (0x5cb140; 1..=99).
    SetZoom(i32),
    /// `Map.SetScreenPos(x, y)`: event 0x13 (the camera centred on the node, 0x5c9770).
    SetScreenPos(i32, i32),
    /// `Tutorial.Exit`: event 0x17 (`CStateGame` +0x103: the game ends, 0x514450).
    Exit,
    /// `Game.ShowText(key, x, y)` (0x5c7a30): the key's string id and the node, for the message
    /// list (0x52a320; `msgwin::script_messages`).
    ShowText { text: u16, x: i32, y: i32 },
    /// Event 0x38 (player lost): sent once per player by `Game.PlayerLost` 0x5c7c60,
    /// `DefaultPlayerLostCheck` 0x5c7ff0, `DefaultPlayersLostCheck` 0x5c8100 and
    /// `EnemyPlayersLost` 0x5c7d90 (all set the flag 0x149b7f4 + 0x8c · p first); its messages
    /// are the client's (`msgwin::script_messages`).
    PlayerLost(u8),
    /// `Sounds.PlaySound(id)` (0x5c9fb0): `PlaySound(id, 100, centre)` for an id in 1..0x6d
    /// (`msgwin::script_messages` hands it to the GUI sounds).
    PlaySound(u16),
    /// `Debug.ShowTextMessage(text)` (0x5c4940): a string argument → 0x52a100(line, player 0),
    /// the console line (`msgwin::script_messages`).
    DebugText(String),
    /// `Game.ShowClock(seconds)` (0x5c7920): GUI event 0x273 and the countdown
    /// (s4_viewer::clock).
    ShowClock(i32),
    /// `Game.DisableLastNPlayersInStatistic(n)` (0x5c81f0): 0x11b589c = n (`Gui::hidden_last`).
    DisableLastN(i32),
}

/// The client's camera, for `Map.PointIsOnScreen`: the top-left world point, the zoom and the
/// window size in pixels (screen = (world − top-left) · zoom).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScreenView {
    pub x: f32,
    pub y: f32,
    pub zoom: f32,
    pub w: f32,
    pub h: f32,
}

/// The control ids the bitsets cover (0xb8c).
pub const CONTROLS: usize = 0xb8c;
const WORDS: usize = 0x174 / 4;

/// The GUI control bitsets of the tutorial (0x15aad38 … 0x15aaec0).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Controls {
    /// 0x15aaec0: a set bit = the script disabled the control.
    pub disabled: [u32; WORDS],
    /// 0x15aad38: the enabled state the GUI asked for (the hook records it).
    pub state: [u32; WORDS],
    /// 0x15aaeac (u64): bit `a` = always enabled (the hook's first argument).
    pub always_a: u64,
    /// 0x15aaeb8: bit `b` = always enabled (the hook's third argument).
    pub always_b: u32,
}

impl Default for Controls {
    /// `SetupTutorialEvent` (0x4fa5b0): states all set, nothing disabled.
    fn default() -> Self {
        Controls { disabled: [0; WORDS], state: [u32::MAX; WORDS], always_a: 0, always_b: 0 }
    }
}

impl Controls {
    fn clear(&mut self, c: u32) {
        if (c as usize) < CONTROLS {
            self.disabled[c as usize >> 5] &= !(1 << (c & 31));
        }
    }

    /// Whether the script disabled control `c`.
    #[allow(dead_code)] // for the GUI (gui, T2)
    pub fn is_disabled(&self, c: u32) -> bool {
        (c as usize) < CONTROLS && self.disabled[c as usize >> 5] & (1 << (c & 31)) != 0
    }

    /// 0x4faca0, the start of `Tutorial.DisableExcept`: every control disabled except 610..612,
    /// 930, 1021..1023, 1070..1072 and 1711 / 1712 (the window's buttons); `always_a` bits 36,
    /// 37 and 57, `always_b` 0xc250.
    pub fn disable_all(&mut self) {
        self.disabled = [u32::MAX; WORDS];
        self.always_a = 0x0200_0030_0000_0000;
        self.disabled[31] &= 0x1fff_ffff;
        self.disabled[29] &= 0xffff_fffb;
        self.disabled[33] &= 0xfffe_3fff;
        self.disabled[53] &= 0xfffe_7fff;
        self.always_b = 0xc250;
        self.disabled[19] &= 0xffff_ffe3;
    }

    /// `Tutorial.DisableExcept(controls…)` (0x5cade0): [`Controls::disable_all`], then the listed
    /// controls (1..=0xb8b) enabled.
    pub fn disable_except(&mut self, controls: &[i32]) {
        self.disable_all();
        for &c in controls {
            if (1..CONTROLS as i32).contains(&c) {
                self.clear(c as u32);
            }
        }
    }

    /// `Tutorial.EnableControls` (0x5caec0) and also `Tutorial.DisableControls` (0x5caf90): both
    /// clear the listed controls' bits (`btr`) — the release's `DisableControls` enables.
    pub fn enable(&mut self, controls: &[i32]) {
        for &c in controls {
            if (0..CONTROLS as i32).contains(&c) {
                self.clear(c as u32);
            }
        }
    }

    /// `Tutorial.EnableAllControls` (0x5cada0): every bit and mask cleared.
    pub fn enable_all(&mut self) {
        self.disabled = [0; WORDS];
        self.always_a = 0;
        self.always_b = 0;
    }

    /// The GUI hook 0x4fa520 `(a, control, b, enabled, flags)`: unless `flags & 6`, the GUI's
    /// wish is recorded; the control may be enabled when bit `a` of `always_a` or bit `b` of
    /// `always_b` is set or the script has not disabled it. [inf] `a` is the dialog and `b` the
    /// control's kind (gui agent to confirm at a GUI-engine call site).
    #[allow(dead_code)] // for the GUI (gui, T2)
    pub fn hook(&mut self, a: u32, control: u32, b: u32, enabled: bool, flags: u8) -> bool {
        if flags & 6 == 0 && (control as usize) < CONTROLS {
            let (w, bit) = (control as usize >> 5, 1u32 << (control & 31));
            if enabled {
                self.state[w] |= bit;
            } else {
                self.state[w] &= !bit;
            }
        }
        if (a < 64 && self.always_a & (1 << a) != 0) || (b < 32 && self.always_b & (1 << b) != 0) {
            return true;
        }
        !self.is_disabled(control)
    }
}

/// Everything the tutorial's script and client share.
#[derive(Debug, Default)]
pub struct TutorialUi {
    /// A tutorial game (type 4): the tutorial window, the control hook and `CTutorial::OnEvent`.
    pub active: bool,
    /// The tutorial window's two lines (`Tutorial.ShowText(key, line)` → UI event 0x263 with the
    /// key's string id): string ids.
    pub text: [Option<u16>; 2],
    /// Bumped whenever `text` changes.
    pub text_seq: u32,
    /// The blinking marker (`Tutorial.SetMarker` / `ClearMarker`, UI event 0x26e: wParam
    /// `x << 16 | y`, lParam 1; cleared with wParam 0x10001, lParam 0).
    pub marker: Option<(u16, u16)>,
    /// The world cursor: effect 0x39 `EFFECT_TUTORIAL_ARROW` at a node (`Tutorial.SetWorldCursor`
    /// 0x5cb230 / `DeleteWorldCursor` 0x5cb340, handle 0x11abe2c).
    pub arrow: Option<(u16, u16)>,
    /// `CTutorial +4` (0x11abe28): the zoom of the last event 0x12 / 0x25d
    /// (`Tutorial.GetCurrentZoomFactor`).
    pub zoom: i32,
    pub controls: Controls,
    /// The client's requests, oldest first.
    pub requests: Vec<Request>,
    /// The client's camera (for `Map.PointIsOnScreen`); `None` without a screen.
    pub view: Option<ScreenView>,
    /// The tutorial was won (`Tutorial.Won`: event 0x37 with team 1; in a tutorial
    /// `CStateGame` only records it, no win screen).
    pub won: bool,
    /// The team event 0x37 carried (`CStateGame +0x68`, the end record's winner; 0 none).
    pub winner: u8,
    /// Event 0x37 decided the game (`DefaultGameEndCheck`), not the local player's own 0x38:
    /// a loss then shows the lose text (`Messages::game_decided`).
    pub end_event: bool,
    /// The game was decided when its save was loaded (`Messages::decided_on_load`).
    pub decided_on_load: bool,
    /// Script events from the UI, for the next script update: (event, arguments).
    pub pending: Vec<(u32, Vec<i32>)>,
    /// `Game.DefaultGameEndCheck(win, lose)` (0x5c8260): the two keys' string ids (0 = none), which
    /// event 0x37 carries in +0xc as `lose << 16 | win` for the end messages.
    pub end_texts: (u16, u16),
    /// The client's selection (`SelectedEntity`, entity ids in order), for `*.IsSelected` and
    /// `WARRIOR_SENT`.
    pub selection: Vec<u16>,
    /// The space key is down (0x16069b0: one `SPACE` per press).
    space_down: bool,
    /// The client's side of `arrow`: the node of the effect it shows and the effect's handle
    /// (0x11abe2c).
    shown_arrow: Option<((u16, u16), u32)>,
}

/// The UI events `CTutorial::OnEvent` (0x4fa780) passes to the script. `w` / `l` are the event's
/// wParam / lParam; `building` the type of the first selected building (0x4fabf0) and
/// `settlers` whether a settler is selected (0x4fab80).
#[derive(Clone, Copy, Debug)]
pub struct UiEv {
    pub id: u32,
    pub w: i32,
    pub l: i32,
    pub building: u16,
    pub settlers: bool,
}

impl TutorialUi {
    /// `CTutorial::OnEvent` (0x4fa780): queue the script event a UI event stands for. Returns it.
    pub fn on_event(&mut self, e: UiEv) -> Option<(u32, Vec<i32>)> {
        let b = e.building as i32;
        let ev: (u32, Vec<i32>) = match e.id {
            // The selection was sent somewhere with a settler in it.
            9 if e.settlers => (events::WARRIOR_SENT, vec![]),
            // Key down / up: the space bar, once per press.
            0xb if e.w == 0x20 && !self.space_down => {
                self.space_down = true;
                (events::SPACE, vec![])
            }
            0x14 if e.w == 0x20 => {
                self.space_down = false;
                return None;
            }
            // The zoom (CTutorial +4).
            0x12 | 0x25d => {
                self.zoom = e.l;
                (events::ZOOM_FACTOR_CHANGED, vec![e.l])
            }
            0x6a => {
                self.pending.push((events::COMMAND, vec![e.w]));
                (events::COMMAND, vec![0x425])
            }
            0x199 => (events::COMMAND, vec![0x425]),
            0x192 | 0x25a => (events::COMMAND, vec![0]),
            0x193 => (events::SHOW_WORK_AREA, vec![b]),
            0x195 if e.w != 0 => (events::CRUSH_BUILDING, vec![b]),
            0x197 => (events::BUILD_PRIO, vec![b]),
            0x1389 => (events::WORK_AREA, vec![b]),
            0x138b => (events::WORK_STATUS, vec![b]),
            0x1a3 => (events::CREATE_FOUNDATION_CART, vec![]),
            // The main menu tab (`gActiveMenu` = 8 · id, no event).
            0x1ac => (0, vec![0x1ac * 8]),
            0x1ad..=0x1b2 | 0x1c1..=0x1cb => (events::MENUCLICK, vec![e.id as i32 * 8, e.w, e.l]),
            0x1b5 => (events::DRAG_BUILDING, vec![e.l]),
            0x1ce => (events::PRODUCTION, vec![e.w, e.l]),
            // The tutorial window's buttons: `Control.DLG_TUTORIAL_CMD_NEXT` / `_RESET`.
            0x275 => (events::COMMAND, vec![0x6b0]),
            0x276 => (events::COMMAND, vec![0x6af]),
            _ => return None,
        };
        self.pending.push(ev.clone());
        Some(ev)
    }

    /// Take the requests for the client.
    pub fn take_requests(&mut self) -> Vec<Request> {
        std::mem::take(&mut self.requests)
    }
}

/// The GUI's events that `CTutorial::OnEvent` hears (in a tutorial): a pressed build-page button
/// (UI event 0x1b5 with the building type, `TryBuild`) and the main / submenu tabs (0x192).
pub fn ui_events(shared: &Shared, events: &[s4_viewer::guistate::UiEvent]) {
    let mut t = shared.lock().unwrap_or_else(|e| e.into_inner());
    if !t.active {
        return;
    }
    for e in events {
        let &s4_viewer::guistate::UiEvent::Command { dialog, link, effects } = e else { continue };
        let ev = |id, l| UiEv { id, w: 0, l, building: 0, settlers: false };
        if let Some(b) = s4_viewer::guistate::build_button(dialog, link)
            && effects & 1 != 0
        {
            t.on_event(ev(0x1b5, b as i32));
        }
        if crate::input::sends_192(dialog, link) {
            t.on_event(ev(0x192, 0));
        }
    }
}

/// A UI event of the selected building's dialog (`ev`, wParam `w`, lParam `l`; the building's
/// type is what 0x4fabf0 reads), in a tutorial: 0x193 / 0x195 / 0x197 / 0x199 / 0x1c1..0x1cb /
/// 0x1ce, and the commands 0x1389 (`SetWorkingArea`) / 0x138b (`SwitchBuilding`) as sent.
pub fn building_event(shared: &Shared, ev: u32, w: i32, l: i32, building: u16) {
    let mut t = shared.lock().unwrap_or_else(|e| e.into_inner());
    if t.active {
        t.on_event(UiEv { id: ev, w, l, building, settlers: false });
    }
}

/// The space bar (key events 0xb / 0x14 with wParam 0x20), in a tutorial.
pub fn space(shared: &Shared, down: bool) {
    let mut t = shared.lock().unwrap_or_else(|e| e.into_inner());
    if t.active {
        t.on_event(UiEv { id: if down { 0xb } else { 0x14 }, w: 0x20, l: 0, building: 0, settlers: false });
    }
}

/// The client's side each frame: the script's requests applied, the zoom's events passed to
/// `CTutorial::OnEvent`, the marker and the world cursor shown, the camera published for
/// `Map.PointIsOnScreen`. Returns the view and whether the script ended the game
/// (`Tutorial.Exit`). `tick` is the game tick (the world cursor effect's start). TODO(gui):
/// `PressButton`, `SelectNextBuilding` (logged until then).
pub fn client_frame(shared: &Shared, gpu: &mut s4_viewer::gpu::Gpu, scene: &mut s4_viewer::scene::Scene, mut view: s4_viewer::gpu::View, tick: u64) -> (s4_viewer::gpu::View, bool) {
    let size = scene.source.w;
    let mut t = shared.lock().unwrap_or_else(|e| e.into_inner());
    let mut exit = false;
    let mut keep = Vec::new();
    for r in t.take_requests() {
        match r {
            Request::SetScreenPos(x, y) => view = s4_viewer::client::view_on_node(gpu, view, (x.max(0) as usize, y.max(0) as usize), size),
            // `Tutorial.SetZoom`: event 0x12 with the level as lParam. `EventManagement` (0x5172f0)
            // zooms as for the slider's value (the knob stays), and `CTutorial` hears it too.
            Request::SetZoom(level) => {
                view = s4_viewer::client::zoom_from_slider(gpu, view, level, size);
                gpu.zoom_events.push((0x12, level));
            }
            Request::Exit => exit = true,
            // The message list's (`msgwin::script_messages`).
            r @ (Request::ShowText { .. } | Request::PlayerLost(_) | Request::PlaySound(_) | Request::DebugText(_)) => keep.push(r),
            Request::DisableLastN(n) => {
                if let Some(ui) = gpu.ui.as_mut() {
                    ui.hidden_last = n;
                }
            }
            Request::ShowClock(seconds) => {
                let screen_w = (gpu.size().0 as f32 / gpu.gui_scale.max(0.1)) as u32;
                if let Some(ui) = gpu.ui.as_mut() {
                    ui.clock.show(&mut ui.state, screen_w, seconds);
                }
                s4_viewer::client::recompose(gpu);
            }
            other => tracing::info!("script request not done by the client yet: {other:?}"),
        }
    }
    t.requests.extend(keep);
    // Events 0x12 / 0x25d: `CTutorial +4` = lParam and `ZOOM_FACTOR_CHANGED` (0x4fa780).
    for (id, l) in std::mem::take(&mut gpu.zoom_events) {
        if t.active {
            t.on_event(UiEv { id, w: 0, l, building: 0, settlers: false });
        }
    }
    if let Some(ui) = gpu.ui.as_mut()
        && (t.active || t.marker.is_some())
        && ui.tutorial_marker(t.marker)
    {
        s4_viewer::client::recompose(gpu);
    }
    // `SetWorldCursor` removes the old effect (vtbl+0x24) and starts effect 0x39 at the node
    // (vtbl+0x28); `DeleteWorldCursor` only removes it. [inf] The effect starts at the client's
    // tick (the script ran in the sim's tick before).
    if t.shown_arrow.map(|a| a.0) != t.arrow {
        if let Some((_, handle)) = t.shown_arrow.take() {
            scene.remove_effect(gpu, handle);
        }
        if let Some((x, y)) = t.arrow {
            let handle = scene.add_lasting_effect(gpu, EFFECT_TUTORIAL_ARROW, x, y, tick).unwrap_or(0);
            t.shown_arrow = Some(((x, y), handle));
        }
    }
    let (w, h) = gpu.size();
    t.view = Some(ScreenView { x: view.x, y: view.y, zoom: view.zoom, w: w as f32, h: h as f32 });
    (view, exit)
}

/// `EFFECT_TUTORIAL_ARROW` (gfx file 6, job 0x39).
pub const EFFECT_TUTORIAL_ARROW: u8 = 0x39;

/// The global the menu events set before their script event (`gActiveMenu`); event 0 in
/// [`TutorialUi::pending`] carries only it.
pub const ACTIVE_MENU: &str = "gActiveMenu";

/// [`TutorialUi::pending`] marker for UI event 9 (the right button down, `EventManagement` case
/// 9): the script host turns it into `WARRIOR_SENT` when a settler is selected (0x4fab80: an
/// entity of type 1 in the selection), which it can check against the world.
pub const RIGHT_DOWN: u32 = 0x1_0009;

/// The right button went down (UI event 9), in a tutorial.
pub fn right_down(shared: &Shared) {
    let mut t = shared.lock().unwrap_or_else(|e| e.into_inner());
    if t.active {
        t.pending.push((RIGHT_DOWN, Vec::new()));
    }
}

/// The client's selection, published every frame.
pub fn publish_selection(shared: &Shared, ids: &[u16]) {
    let mut t = shared.lock().unwrap_or_else(|e| e.into_inner());
    if t.selection != ids {
        t.selection = ids.to_vec();
    }
}

#[cfg(test)]
mod campaign;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disable_except_keeps_the_fixed_controls() {
        let mut c = Controls::default();
        c.disable_except(&[606, 3000]);
        for k in [606, 610, 611, 612, 930, 1021, 1022, 1023, 1070, 1071, 1072, 1711, 1712] {
            assert!(!c.is_disabled(k), "{k}");
        }
        for k in [1, 607, 609, 613, 929, 931, 1020, 1069, 1073, 1710, 1713, 0xb8b] {
            assert!(c.is_disabled(k), "{k}");
        }
        // Always-enabled masks.
        assert!(c.hook(36, 1, 0, true, 0) && c.hook(57, 1, 0, true, 0) && c.hook(0, 1, 4, true, 0));
        assert!(!c.hook(0, 1, 0, true, 0));
        c.enable(&[1]);
        assert!(c.hook(0, 1, 0, true, 0));
        c.enable_all();
        assert!(!c.is_disabled(5) && c.always_a == 0);
    }

    /// The player's side of a tutorial test: the game's sim thread with the mission's real
    /// script, and what a player does (keys, camera, selection, build and move commands).
    struct Driver {
        sim: crate::sim::SimHandle,
        tutorial: Shared,
    }

    /// Ticks per look at the window (a player reacts within a few ticks).
    const STEP: u32 = 5;

    impl Driver {
        /// Tutorial `n`; `None` without the game data.
        fn start(n: u32) -> Option<Driver> {
            use crate::sim::{Clock, SimHandle, SimSetup};
            let map_path = s4_viewer::scene::resolve_map(&map_name(n));
            if !map_path.exists() {
                return None;
            }
            let tutorial = shared();
            tutorial.lock().unwrap().active = true;
            let setup = SimSetup {
                map_path,
                prefer_file_script: true,
                run_script: true,
                fog: false,
                outcome: Default::default(),
                tutorial: tutorial.clone(),
                net: None,
                players: None,
                load: None,
            };
            Some(Driver { sim: SimHandle::spawn(setup, Clock::Manual).unwrap(), tutorial })
        }

        fn line0(&self) -> Option<&'static str> {
            self.tutorial.lock().unwrap().text[0].map(|id| s4_script::TEXT_KEYS[id as usize].as_str())
        }

        fn space(&self) {
            space(&self.tutorial, true);
            space(&self.tutorial, false);
        }

        /// A pressed build-page button (UI event 0x1b5).
        fn drag(&self, building: i32) {
            self.tutorial.lock().unwrap().on_event(UiEv { id: 0x1b5, w: 0, l: building, building: 0, settlers: false });
        }

        fn command(&self, command: s4_sim::Command) {
            use crate::commands::{Stamped, execution_tick};
            let tick = execution_tick(self.sim.snapshot().tick);
            self.sim.command(Stamped { tick, command: s4_sim::PlayerCommand { player: 1, command } });
        }

        /// Run `f` on the world between ticks and return its result.
        fn ask<T: Send + 'static>(&self, f: impl FnOnce(&mut s4_sim::World) -> T + Send + 'static) -> T {
            let (tx, rx) = std::sync::mpsc::channel();
            self.sim.with_world(move |w| {
                let _ = tx.send(f(w));
            });
            rx.recv().unwrap()
        }

        /// Build `building` at the building site nearest to `(x, y)` (the world cursor's node is
        /// not always a site), within the scripts' radius 5.
        fn build_near(&self, building: u8, x: i32, y: i32) {
            assert!(self.try_build_near(building, x, y), "no site for building {building} near ({x}, {y})");
        }

        /// [`Driver::build_near`] that reports whether there was a site.
        fn try_build_near(&self, building: u8, x: i32, y: i32) -> bool {
            let site = self.ask(move |w| {
                let size = w.grid.size() as i32;
                s4_sim::build::possible_sites(w, 1, building, (0, 0), (size, size))
                    .into_iter()
                    .map(|(sx, sy, _)| (s4_sim::spiral::grid_distance(sx as i32 - x, sy as i32 - y), sx, sy))
                    .filter(|s| s.0 <= 5)
                    .min()
            });
            let Some((_, sx, sy)) = site else { return false };
            self.command(s4_sim::Command::Build { building: building as u32, x: sx, y: sy });
            true
        }

        /// The camera on a screen around node `(x, y)`.
        fn look_at(&self, x: usize, y: usize) {
            let [wx, wy] = s4_viewer::terrain::node_pos(x, y, 0);
            self.tutorial.lock().unwrap().view = Some(ScreenView { x: wx - 400.0, y: wy - 300.0, zoom: 1.0, w: 800.0, h: 600.0 });
        }

        /// The local player's settlers of `kind`.
        fn settlers(&self, kind: u16) -> Vec<u16> {
            self.ask(move |w| w.entities.iter().filter(|(_, e)| e.obj_type == s4_sim::entity::ObjType::Settler && e.owner == 1 && e.kind == kind).map(|(i, _)| i).collect())
        }

        /// Run until `Tutorial.Exit` (at most `ticks`, three minutes of wall time): `on_text`
        /// gets every new line-0 text, `waiting` the current one every 50 ticks. Returns the
        /// texts in order.
        fn play(&self, ticks: u32, mut on_text: impl FnMut(&Driver, &str), mut waiting: impl FnMut(&Driver, &str)) -> Vec<&'static str> {
            let (mut done, mut last) = (Vec::new(), None);
            let started = std::time::Instant::now();
            loop {
                let before = self.sim.snapshot().tick;
                assert!(before < ticks, "not done after {ticks} ticks: {done:?}");
                assert!(self.sim.step(STEP) > before, "the sim thread stopped at tick {before}");
                assert!(started.elapsed() < std::time::Duration::from_secs(180), "not done after 3 minutes (tick {before}): {done:?}");
                if self.tutorial.lock().unwrap().take_requests().contains(&Request::Exit) {
                    break;
                }
                let Some(key) = self.line0() else { continue };
                if last != Some(key) {
                    last = Some(key);
                    eprintln!("tick {}: {key}", self.sim.snapshot().tick);
                    done.push(key);
                    on_text(self, key);
                } else if self.sim.snapshot().tick % 50 < STEP {
                    waiting(self, key);
                }
            }
            eprintln!("tick {}: {done:?}", self.sim.snapshot().tick);
            assert!(self.tutorial.lock().unwrap().won, "Tutorial.Won");
            assert_eq!(self.sim.outcome.load(std::sync::atomic::Ordering::Relaxed), crate::script::WON);
            done
        }
    }

    /// Tutorial 1 played through the game's sim thread and its real script
    /// (`Script\Tutorial01.txt`) to `Tutorial.Won` and `Tutorial.Exit`: the player's part is
    /// driven by the text on the tutorial window — the space bar, the camera on (66, 87), the
    /// build-page buttons (UI event 0x1b5) and the build commands at the world cursor's places.
    /// Skipped without the game data.
    #[test]
    fn tutorial01_played_to_won() {
        let Some(d) = Driver::start(1) else { return };
        let done = d.play(
            20_000,
            |d, key| match key {
                "TUT_01_001_R" | "TUT_01_002_R" | "TUT_01_003_R" | "TUT_01_004_R" | "TUT_01_006_N" | "TUT_01_006_R" | "TUT_01_008_R" | "TUT_01_017_R" => d.space(),
                "TUT_01_005_R" => d.look_at(66, 87),
                "TUT_01_009_R" => d.drag(1),
                "TUT_01_010_R" => d.build_near(1, 124, 95),
                "TUT_01_013_R" => d.drag(3),
                "TUT_01_014_R" => d.build_near(3, 122, 107),
                "TUT_01_015_R" => d.build_near(4, 102, 81),
                _ => {}
            },
            // Steps 13..15: the woodcutter is finished and inhabited, then the space bar.
            |d, key| {
                if key == "TUT_01_012_R" {
                    d.space();
                }
            },
        );
        assert_eq!(done.len(), 17);
        assert_eq!(done.last(), Some(&"TUT_01_017_R"));
    }

    /// Tutorial 2: forester, stone mine and small residence at the world cursors, then the
    /// script's soldiers and priest (`Settlers.AddSettlers`): the camera on them and on the
    /// target, the five selected (`Settlers.IsSelected`), the right button (`WARRIOR_SENT`) and
    /// the move order to (73, 133).
    #[test]
    fn tutorial02_played_to_won() {
        let Some(d) = Driver::start(2) else { return };
        let done = d.play(
            40_000,
            |d, key| match key {
                "TUT_02_001_R" | "TUT_02_004_R" | "TUT_02_014_R" => d.space(),
                "TUT_02_002_R" => d.build_near(2, 139, 105),
                "TUT_02_005_R" => d.build_near(14, 91, 89),
                "TUT_02_007_R" => d.build_near(40, 113, 104),
                "TUT_02_009_R" => d.look_at(142, 159),
                "TUT_02_010_R" => d.look_at(65, 136),
                "TUT_02_011_R" => {
                    let mut ids = d.settlers(31);
                    ids.extend(d.settlers(45));
                    publish_selection(&d.tutorial, &ids);
                }
                "TUT_02_012_R" => right_down(&d.tutorial),
                "TUT_02_013_R" => {
                    let entities = d.tutorial.lock().unwrap().selection.clone();
                    d.command(s4_sim::Command::SendTo { x: 73, y: 133, mode: 0, entities });
                }
                _ => {}
            },
            // Waiting for a building to be finished or inhabited, then the space bar.
            |d, key| {
                if matches!(key, "TUT_02_003_R" | "TUT_02_006_R" | "TUT_02_008_R") {
                    d.space();
                }
            },
        );
        assert_eq!(done.last(), Some(&"TUT_02_014_R"));
    }

    /// Tutorial 3: hunter, fisher, grain farm, waterworks, ranch, slaughterhouse, a second farm,
    /// mill and bakery at the world cursors; the steps between wait for the script's
    /// `GOODARRIVE` (food at a mine, sheep at the slaughterhouse, bread at a mine).
    #[test]
    fn tutorial03_played_to_won() {
        let Some(d) = Driver::start(3) else { return };
        let done = d.play(
            120_000,
            |d, key| match key {
                "TUT_03_001_R" | "TUT_03_005_R" | "TUT_03_013_R" => d.space(),
                "TUT_03_002_R" => d.build_near(7, 154, 160),
                "TUT_03_003_R" => d.build_near(6, 185, 169),
                "TUT_03_006_R" => d.build_near(11, 182, 198),
                "TUT_03_007_R" => d.build_near(5, 202, 253),
                "TUT_03_008_R" => d.build_near(12, 197, 223),
                "TUT_03_009_R" => d.build_near(8, 207, 244),
                // The second farm, and the mill the next step asks for.
                "TUT_03_011_R" => {
                    d.build_near(11, 224, 261);
                    d.build_near(9, 237, 255);
                }
                "TUT_03_012_R" => d.build_near(10, 187, 249),
                _ => {}
            },
            |_, _| {},
        );
        assert_eq!(done.last(), Some(&"TUT_03_013_R"));
    }

    /// Tutorial 4: ten geologists (`SetSpecialistWish`, counted by the script's
    /// `SETTLER_CHANGE_TYPE`) sent to the mountains, four mines, two smelters, the tool and
    /// weapon smiths, their sidebars (`MENUCLICK`: `gActiveMenu` = TOOL / WEAPON_SIDEBAR) and
    /// production orders until `Goods.Amount` has 3 pickaxes, 2 saws, 3 swords and 2 bows.
    #[test]
    fn tutorial04_played_to_won() {
        let Some(d) = Driver::start(4) else { return };
        let first = |d: &Driver, kind: u16| d.ask(move |w| w.first_building(1, kind as i32));
        let order = |d: &Driver, smith: u16, goods: &[(u16, u32)]| {
            let b = first(d, smith);
            for &(good, amount) in goods {
                d.command(s4_sim::Command::ProductionOrder { building: b, good, amount });
            }
        };
        let done = d.play(
            200_000,
            |d, key| match key {
                "TUT_04_001_R" | "TUT_04_002_R" | "TUT_04_003_R" | "TUT_04_006_R" | "TUT_04_011_R" | "TUT_04_026_R" => d.space(),
                "TUT_04_004_R" => {
                    let eco = d.ask(|w| w.economy.player_ecos[1].first().copied().unwrap_or(0));
                    d.command(s4_sim::Command::SetSpecialistWish { eco_sector: eco, specialist: 49, value: 10 });
                }
                "TUT_04_005_R" => d.command(s4_sim::Command::SendTo { x: 161, y: 204, mode: 0, entities: d.settlers(49) }),
                "TUT_04_007_R" => d.command(s4_sim::Command::SendTo { x: 144, y: 188, mode: 0, entities: d.settlers(49) }),
                "TUT_04_008_R" => d.build_near(15, 160, 203),
                "TUT_04_009_R" => d.build_near(17, 150, 192),
                "TUT_04_010_R" => d.build_near(16, 169, 217),
                "TUT_04_012_R" => d.build_near(14, 168, 227),
                "TUT_04_015_R" => d.build_near(20, 188, 236),
                "TUT_04_016_R" => d.build_near(19, 185, 249),
                // The tool smith, then (line 1 changes only) the weapon smith.
                "TUT_04_018_R" => {
                    d.build_near(21, 194, 227);
                    d.build_near(22, 190, 210);
                }
                // The tool smith's sidebar (UI event 0x1c4), then its orders.
                "TUT_04_021_R" => building_event(&d.tutorial, 0x1c4, 0, 0, 21),
                "TUT_04_022_R" => order(d, 21, &[(25, 3), (28, 2)]),
                "TUT_04_024_R" => building_event(&d.tutorial, 0x1c5, 0, 0, 22),
                "TUT_04_025_R" => order(d, 22, &[(34, 3), (8, 2)]),
                _ => {}
            },
            // Selecting the finished smiths.
            |d, key| match key {
                "TUT_04_020_R" => publish_selection(&d.tutorial, &[first(d, 21)]),
                "TUT_04_023_R" => publish_selection(&d.tutorial, &[first(d, 22)]),
                _ => {}
            },
        );
        assert_eq!(done.last(), Some(&"TUT_04_026_R"));
    }

    /// Tutorial 5 (`new_game` places a marketplace with `Buildings.AddBuildingEx`): a small
    /// tower, the big tower selected and crushed, 15 pioneers sent to claim land, a second small
    /// tower, the lookout tower, its eye-catcher sidebar (`gActiveMenu` = EYECATCHER_SIDBAR) and
    /// five eye-catchers.
    #[test]
    fn tutorial05_played_to_won() {
        let Some(d) = Driver::start(5) else { return };
        let first = |d: &Driver, kind: u16| d.ask(move |w| w.first_building(1, kind as i32));
        assert_ne!(first(&d, 33), 0, "AddBuildingEx's marketplace");
        let tower2 = std::cell::Cell::new(false);
        // Five different eye-catchers (the script counts types 01..12), one at a time (sites next
        // to each other collide), each at the first free site of a few places.
        let eye_catchers = |d: &Driver| {
            let placed = d.ask(|w| (64..76).filter(|&k| w.buildings_amount(1, k, 2) > 0).count());
            if placed >= 5 {
                return;
            }
            for kind in 64u8..76 {
                if d.ask(move |w| w.buildings_amount(1, kind as i32, 2)) == 0 {
                    for (cx, cy) in [(132, 200), (140, 200), (148, 200), (156, 200), (124, 205), (132, 210), (140, 210)] {
                        if d.try_build_near(kind, cx, cy) {
                            return;
                        }
                    }
                }
            }
        };
        let done = d.play(
            200_000,
            |d, key| match key {
                "TUT_05_001_R" | "TUT_05_002_R" | "TUT_05_003_R" | "TUT_05_006_R" | "TUT_05_009_R" | "TUT_05_020_R" => d.space(),
                "TUT_05_004_R" => d.build_near(46, 128, 191),
                "TUT_05_007_R" => publish_selection(&d.tutorial, &[first(d, 47)]),
                "TUT_05_008_R" => d.command(s4_sim::Command::CrushBuilding { building: first(d, 47) as u32 }),
                "TUT_05_010_R" => {
                    let eco = d.ask(|w| w.economy.player_ecos[1].first().copied().unwrap_or(0));
                    d.command(s4_sim::Command::SetSpecialistWish { eco_sector: eco, specialist: 47, value: 15 });
                }
                "TUT_05_011_R" => publish_selection(&d.tutorial, &d.settlers(47)),
                // Pioneers to the cursor, the next point and the land to claim.
                "TUT_05_012_R" => {
                    let p = d.settlers(47);
                    let n = p.len();
                    d.command(s4_sim::Command::SendTo { x: 137, y: 167, mode: 0, entities: p[..4.min(n)].to_vec() });
                    d.command(s4_sim::Command::SendTo { x: 141, y: 158, mode: 0, entities: p[4.min(n)..8.min(n)].to_vec() });
                    d.command(s4_sim::Command::SendTo { x: 152, y: 149, mode: 0, entities: p[8.min(n)..].to_vec() });
                }
                "TUT_05_014_R" => tower2.set(d.try_build_near(46, 152, 151)),
                "TUT_05_015_R" => d.build_near(45, 185, 168),
                "TUT_05_018_R" => building_event(&d.tutorial, 0x1ca, 0, 0, 45),
                "TUT_05_019_R" => eye_catchers(d),
                _ => {}
            },
            // The second tower's site needs the claimed land around it first.
            |d, key| {
                if key == "TUT_05_014_R" && !tower2.get() {
                    tower2.set(d.try_build_near(46, 152, 151));
                }
                if key == "TUT_05_019_R" {
                    eye_catchers(d);
                }
            },
        );
        assert_eq!(done.last(), Some(&"TUT_05_020_R"));
    }

    /// Tutorial 6: a storage area, the vehicle hall and its cart, the cart made a foundation
    /// cart (`CREATE_FOUNDATION_CART`) and driven to (153, 173) to found a settlement, its
    /// pioneers turned back into carriers, diggers and builders there, a marketplace, the trade
    /// target (`Buildings.GetTarget`), stone sent over, and a small tower.
    #[test]
    fn tutorial06_played_to_won() {
        use s4_sim::Command;
        let Some(d) = Driver::start(6) else { return };
        let first = |d: &Driver, kind: u16| d.ask(move |w| w.first_building(1, kind as i32));
        let markets = |d: &Driver| d.ask(|w| w.building_list(1, 33));
        let cart = |d: &Driver| d.ask(|w| s4_sim::vehicles::iter(w, 1, 5).next().unwrap_or(0));
        let eco_at = |d: &Driver, x: u32, y: u32| d.ask(move |w| s4_sim::eco::eco_of(w, w.grid.index(x, y)));
        let tower = std::cell::Cell::new(false);
        let (ordered, fcart) = (std::cell::Cell::new(false), std::cell::Cell::new(0u16));
        let done = d.play(
            200_000,
            |d, key| match key {
                "TUT_06_001_R" | "TUT_06_002_R" | "TUT_06_005_R" | "TUT_06_006_R" | "TUT_06_007_R" => d.space(),
                "TUT_06_020_R" => d.space(),
                "TUT_06_003_R" => d.build_near(34, 127, 256),
                // Store logs, boards and stone (5044).
                "TUT_06_004_R" => {
                    let store = first(d, 34);
                    for good in [32u16, 7, 22] {
                        d.command(Command::StorageGoodSwitch { building: store, good, value: 1 });
                    }
                }
                "TUT_06_008_R" => d.build_near(23, 157, 299),
                // Pioneers out: back to carriers.
                "TUT_06_013_R" => d.command(Command::MorphToCarrier { entities: d.settlers(47) }),
                "TUT_06_021_R" => {
                    let eco = eco_at(d, 153, 173);
                    // 5027 `ChangeMinMaxValues` with types 2 / 3: `ChangeWorkerDeltaAmount` (0x4d5750).
                    d.command(Command::SetUnitReservation { eco_sector: eco, kind: 2, value: 2 });
                    d.command(Command::SetUnitReservation { eco_sector: eco, kind: 3, value: 2 });
                }
                "TUT_06_014_R" => d.build_near(33, 153, 173),
                // The old marketplace selected, its trade-target mode (UI event 0x199 → COMMAND
                // 1061), the target set to the new one.
                "TUT_06_015_R" => {
                    let m = markets(d);
                    let old = *m.last().unwrap();
                    publish_selection(&d.tutorial, &[old]);
                }
                "TUT_06_016_R" => {
                    let m = markets(d);
                    let (old, new) = (*m.last().unwrap(), m[0]);
                    d.command(Command::SetTradeTarget { building: old as u32, target: new as u32 });
                }
                // The goods-out sidebar (0x1c7) and stone sent.
                "TUT_06_017_R" => {
                    let old = *markets(d).last().unwrap();
                    publish_selection(&d.tutorial, &[old]);
                    building_event(&d.tutorial, 0x1c7, 0, 0, 33);
                    d.command(Command::TransportGood { building: old, good: 32, amount: 8 });
                }
                "TUT_06_018_R" => tower.set(d.try_build_near(46, 153, 173)),
                _ => {}
            },
            |d, key| match key {
                "TUT_06_003_R" | "TUT_06_004_R" => publish_selection(&d.tutorial, &[first(d, 34)]),
                // The hall selected and a cart ordered (5040).
                "TUT_06_009_R" => {
                    let hall = first(d, 23);
                    publish_selection(&d.tutorial, &[hall]);
                    if !ordered.get() && d.ask(|w| w.buildings_amount(1, 23, 1)) > 0 {
                        ordered.set(true);
                        d.command(Command::WorkshopOrder { building: hall, item: 5, amount: 1 });
                    }
                }
                // The finished cart made a foundation cart (its button: UI event 0x1a3 and
                // command 5012).
                "TUT_06_010_R" if fcart.get() == 0 => {
                    let c = cart(d);
                    if d.ask(move |w| s4_sim::vehicles::cart::command_target(w, c as u32)).is_some() {
                        fcart.set(c);
                        d.tutorial.lock().unwrap().on_event(UiEv { id: 0x1a3, w: 0, l: 0, building: 0, settlers: false });
                        d.command(Command::CreateFoundationCart { cart: c as u32 });
                    }
                }
                // The full foundation cart driven to the place, then the settlement founded.
                // To a place near the cursor with room for a settlement (0x55da00: more than 27
                // free nodes of the radius-3 spiral on its sector, owned by nobody or the player).
                "TUT_06_011_R" => {
                    let (x, y) = d.ask(|w| {
                        let room = |x: i32, y: i32| {
                            let Some(c) = w.grid.checked_index(x, y) else { return 0 };
                            let sector = w.tiling.sector_of(c);
                            s4_sim::spiral::walk(w.grid, x, y, 3)
                                .map(|(nx, ny)| w.grid.index(nx as u32, ny as u32))
                                .filter(|&n| w.tiling.sector_of(n) == sector && matches!(w.owner_of(n), 0 | 1) && w.layers.flags[n] & s4_sim::grid::flag::BLOCKED_LAND == 0 && w.layers.map_object[n] == 0)
                                .count()
                        };
                        s4_sim::spiral::walk(w.grid, 153, 173, 4).find(|&(x, y)| room(x, y) > 0x1b).unwrap_or((153, 173))
                    });
                    d.command(Command::SendTo { x: x as u16, y: y as u16, mode: 0, entities: vec![fcart.get()] });
                }
                "TUT_06_012_R" => d.command(Command::CreateSettlement { cart: fcart.get() as u32 }),
                "TUT_06_015_R" => {
                    let old = *markets(d).last().unwrap();
                    publish_selection(&d.tutorial, &[old]);
                    building_event(&d.tutorial, 0x199, 0, 0, 33);
                }
                "TUT_06_018_R" if !tower.get() => tower.set(d.try_build_near(46, 153, 173)),
                // Pioneers standing on own land back to carriers, until none is left near.
                "TUT_06_013_R" => d.command(Command::MorphToCarrier { entities: d.settlers(47) }),
                // A swordsman sent onto the new tower (a move order onto a military building).
                "TUT_06_019_R" => {
                    let t = first(d, 46);
                    let (tx, ty) = d.ask(move |w| w.entities.by_index(t as u32).map_or((0, 0), |e| (e.x(), e.y())));
                    // A free one (selectable: not in a building).
                    let soldier = d.ask(|w| {
                        w.entities.iter().find(|(_, e)| e.obj_type == s4_sim::entity::ObjType::Settler && e.owner == 1 && matches!(e.kind, 29..=31) && e.flags & s4_sim::entity::flag::SELECTABLE != 0).map(|(i, _)| i)
                    });
                    if let Some(s) = soldier {
                        d.command(Command::SendTo { x: tx as u16, y: ty as u16, mode: 0, entities: vec![s] });
                    }
                }
                _ => {}
            },
        );
        assert_eq!(done.last(), Some(&"TUT_06_020_R"));
    }

    /// Tutorial 7: a small temple and a vineyard at the world cursors, the vineyard's working
    /// area moved onto the cursor (`Buildings.IsWorkingArea`), a big temple (its priests and
    /// `ManaPerBigTemple`), a priest selected, the magic sidebar (`gActiveMenu` = MAGIC_SIDEBAR,
    /// UI event 0x1c1) and the divine present cast (`Magic.CurrentManaAmount` /
    /// `IncreaseMana`, `Statistic.SpellsCast`).
    #[test]
    fn tutorial07_played_to_won() {
        use s4_sim::Command;
        let Some(d) = Driver::start(7) else { return };
        let first = |d: &Driver, kind: u16| d.ask(move |w| w.first_building(1, kind as i32));
        let cast = std::cell::Cell::new(false);
        let done = d.play(
            100_000,
            |d, key| match key {
                "TUT_07_001_R" | "TUT_07_016_R" => d.space(),
                "TUT_07_002_R" => d.build_near(43, 265, 384),
                "TUT_07_004_R" => d.build_near(35, 256, 348),
                "TUT_07_008_R" => d.build_near(44, 280, 382),
                "TUT_07_011_R" => building_event(&d.tutorial, 0x1c1, 0, 0, 0),
                _ => {}
            },
            |d, key| match key {
                "TUT_07_001_R" | "TUT_07_016_R" => d.space(),
                // The vineyard's working area onto the cursor (command 5001).
                "TUT_07_005_R" => {
                    let b = first(d, 35);
                    d.command(Command::SetWorkingArea { building: b as u32, x: 260, y: 341 });
                }
                "TUT_07_009_R" => publish_selection(&d.tutorial, &d.settlers(45)),
                // The divine present (spell 0) next to the priest.
                "TUT_07_014_R" if !cast.get() => {
                    let p = d.settlers(45)[0];
                    let (x, y) = d.ask(move |w| w.entities.by_index(p as u32).map_or((0, 0), |e| (e.x(), e.y())));
                    if d.ask(|w| s4_sim::magic::mana(w, 1)) >= d.ask(|w| w.magic.pool(1).map_or(i32::MAX, |p| p.costs[0])) {
                        cast.set(true);
                        d.command(Command::CastSpell { priest: p, spell: 0, x: x as u16 + 2, y: y as u16 });
                    }
                }
                _ => {}
            },
        );
        assert_eq!(done.last(), Some(&"TUT_07_016_R"));
    }

    /// Tutorial 8: a big tower filled (`Buildings.GetInhabitantAmount`), barracks with their
    /// sidebar (`gActiveMenu` = ADDSOLDIER_SIDEBAR), bowmen and swordsmen of level 2 and 3
    /// recruited (5047, counted by `SETTLER_CHANGE_TYPE`), soldiers sent to a point, a thief made
    /// and sent near the enemy, the enemy castle taken, a healer's hut, and soldiers next to it
    /// (`Settlers.SetHealthInArea`).
    #[test]
    fn tutorial08_played_to_won() {
        use s4_sim::Command;
        let Some(d) = Driver::start(8) else { return };
        let first = |d: &Driver, player: i32, kind: u16| d.ask(move |w| w.first_building(player, kind as i32));
        let order = |d: &Driver, kind: u16, amount: u32| {
            let b = first(d, 1, 24);
            d.command(Command::SoldierOrder { building: b, kind, amount });
        };
        // The player's free soldiers (not in a building).
        let soldiers = |d: &Driver| {
            d.ask(|w| {
                w.entities
                    .iter()
                    .filter(|(_, e)| e.obj_type == s4_sim::entity::ObjType::Settler && e.owner == 1 && matches!(e.kind, 29..=34) && e.flags & s4_sim::entity::flag::SELECTABLE != 0)
                    .map(|(i, _)| i)
                    .collect::<Vec<u16>>()
            })
        };
        let fill = |d: &Driver| d.command(Command::FillTower { building: first(d, 1, 47) as u32, value: u32::MAX });
        assert_eq!(d.ask(|w| w.buildings_amount(2, 48, 2)), 1, "the enemy castle");
        let done = d.play(
            200_000,
            |d, key| match key {
                "TUT_08_001_R" | "TUT_08_007_R" | "TUT_08_014_R" | "TUT_08_023_R" => d.space(),
                "TUT_08_002_R" => d.build_near(47, 170, 95),
                "TUT_08_003_R" => publish_selection(&d.tutorial, &[first(d, 1, 47)]),
                "TUT_08_005_R" | "TUT_08_011_R" => fill(d),
                "TUT_08_006_R" => d.build_near(24, 170, 65),
                "TUT_08_008_R" => building_event(&d.tutorial, 0x1c3, 0, 0, 24),
                "TUT_08_010_R" => order(d, 33, 3),
                "TUT_08_012_R" => order(d, 30, 4),
                "TUT_08_013_R" => d.command(Command::SendTo { x: 175, y: 115, mode: 0, entities: soldiers(d) }),
                "TUT_08_015_R" => {
                    let eco = d.ask(|w| w.economy.player_ecos[1].first().copied().unwrap_or(0));
                    d.command(Command::SetSpecialistWish { eco_sector: eco, specialist: 48, value: 1 });
                }
                "TUT_08_017_R" => {
                    order(d, 31, 5);
                    order(d, 34, 5);
                }
                "TUT_08_021_R" => d.build_near(27, 212, 186),
                _ => {}
            },
            |d, key| match key {
                "TUT_08_001_R" | "TUT_08_007_R" | "TUT_08_014_R" | "TUT_08_023_R" => d.space(),
                "TUT_08_005_R" | "TUT_08_011_R" => fill(d),
                // The thief selected and sent near the enemy.
                "TUT_08_025_R" => {
                    publish_selection(&d.tutorial, &d.settlers(48));
                    d.command(Command::SendTo { x: 218, y: 166, mode: 0, entities: d.settlers(48) });
                }
                // The enemy castle attacked by every free soldier; again with the eight swordsmen
                // the script adds (with text 024) when fewer than three are left.
                "TUT_08_020_R" | "TUT_08_024_R" => {
                    let c = first(d, 2, 48);
                    let (x, y) = d.ask(move |w| w.entities.by_index(c as u32).map_or((0, 0), |e| (e.x(), e.y())));
                    d.command(Command::SendTo { x: x as u16, y: y as u16, mode: 0, entities: soldiers(d) });
                }
                "TUT_08_022_R" => d.command(Command::SendTo { x: 212, y: 188, mode: 0, entities: soldiers(d) }),
                _ => {}
            },
        );
        assert_eq!(done.last(), Some(&"TUT_08_023_R"));
    }

    /// Tutorial 9 (Maya): nine blowgun warriors (5047, counted by `SETTLER_CHANGE_TYPE`), a war
    /// machine from the vehicle hall (5040, `Vehicles.Amount`), a geologist at the cursor, a
    /// sulfur mine, the war machine loaded with gunpowder (5010; the script waits for the
    /// gunpowder count to drop), the enemy small tower at (263, 261) destroyed by it, then the
    /// priest's attack spell (`Magic.IncreaseMana`, `Statistic.SpellsCast`).
    #[test]
    fn tutorial09_played_to_won() {
        use s4_sim::Command;
        let Some(d) = Driver::start(9) else { return };
        assert!(d.ask(|w| w.exists_building_in_area(2, 46, 263, 261, 3, s4_sim::query::ALL)), "the enemy tower");
        let first = |d: &Driver, player: i32, kind: u16| d.ask(move |w| w.first_building(player, kind as i32));
        let machine = |d: &Driver| d.ask(|w| s4_sim::vehicles::iter(w, 1, 4).next().unwrap_or(0));
        let cast = std::cell::Cell::new(false);
        let done = d.play(
            200_000,
            |d, key| match key {
                "TUT_09_001_R" | "TUT_09_004_R" | "TUT_09_006_R" | "TUT_09_007_R" | "TUT_09_011_R" => d.space(),
                "TUT_09_002_R" => d.command(Command::SoldierOrder { building: first(d, 1, 24), kind: 41, amount: 9 }),
                "TUT_09_003_R" => d.command(Command::WorkshopOrder { building: first(d, 1, 23), item: 4, amount: 1 }),
                "TUT_09_005_R" => d.build_near(18, 184, 184),
                _ => {}
            },
            |d, key| match key {
                "TUT_09_001_R" | "TUT_09_004_R" | "TUT_09_006_R" | "TUT_09_007_R" | "TUT_09_011_R" => d.space(),
                "TUT_09_012_R" => d.command(Command::SendTo { x: 186, y: 180, mode: 0, entities: d.settlers(49) }),
                // The war machine selected and loaded.
                "TUT_09_008_R" => {
                    let m = machine(d);
                    publish_selection(&d.tutorial, &[m]);
                    d.command(Command::CatapultLoad { vehicle: m as u32 });
                }
                // The war machine sent onto the enemy tower (an attack).
                "TUT_09_009_R" => {
                    let t = d.ask(|w| w.building_list(2, 46).into_iter().find(|&b| w.entities.by_index(b as u32).is_some_and(|e| s4_sim::spiral::grid_distance(e.x() - 263, e.y() - 261) <= 3)));
                    if let Some(t) = t {
                        let (x, y) = d.ask(move |w| w.entities.by_index(t as u32).map_or((0, 0), |e| (e.x(), e.y())));
                        d.command(Command::SendTo { x: x as u16, y: y as u16, mode: 0, entities: vec![machine(d)] });
                    }
                }
                // The attack spell (5) on the enemy near the priest.
                "TUT_09_010_R" if !cast.get() => {
                    if let Some(&p) = d.settlers(45).first() {
                        let (x, y) = d.ask(move |w| w.entities.by_index(p as u32).map_or((0, 0), |e| (e.x(), e.y())));
                        cast.set(true);
                        d.command(Command::CastSpell { priest: p, spell: 5, x: x as u16 + 3, y: y as u16 + 3 });
                    }
                }
                _ => {}
            },
        );
        assert_eq!(done.last(), Some(&"TUT_09_011_R"));
    }

    /// Tutorial 10 (Trojans): a shipyard and a war ship (`Vehicles.Amount`), the enemy big tower
    /// at (154, 275) destroyed from the water, the priest's special spell, fifteen thieves who
    /// steal the enemy's iron bars (`Goods.Amount` of player 2 below 18), nine battle axes from
    /// the weapon smith (`gActiveMenu` = WEAPON_SIDEBAR), ten axe warriors (`SETTLER_CHANGE_TYPE`)
    /// and every enemy small tower taken.
    #[test]
    fn tutorial10_played_to_won() {
        use s4_sim::Command;
        let Some(d) = Driver::start(10) else { return };
        let first = |d: &Driver, player: i32, kind: u16| d.ask(move |w| w.first_building(player, kind as i32));
        let pos = |d: &Driver, id: u16| d.ask(move |w| w.entities.by_index(id as u32).map_or((0, 0), |e| (e.x(), e.y())));
        let ordered = std::cell::Cell::new(false);
        let cast = std::cell::Cell::new(false);
        let done = d.play(
            200_000,
            |d, key| match key {
                "TUT_10_001_R" | "TUT_07_015_R" | "TUT_10_012_R" => d.space(),
                "TUT_10_002_R" => d.build_near(31, 355, 422),
                "TUT_10_006_R" => publish_selection(&d.tutorial, &d.settlers(45)),
                "TUT_10_007_R" => {
                    let eco = d.ask(|w| w.economy.player_ecos[1].first().copied().unwrap_or(0));
                    d.command(Command::SetSpecialistWish { eco_sector: eco, specialist: 48, value: 15 });
                }
                // By now the weapon smith has filled the barracks' sword and bow piles (8 each) and
                // its own product piles (4 + 4: 0x54cdc0 works only below 8 together), so it
                // stands still; the release does the same. As a player would, the storage area is
                // told to take swords and bows (5044), then ten axes are ordered (one per axe
                // warrior of TUT_10_010; the script itself waits for nine).
                "TUT_10_009_R" => {
                    let store = first(d, 1, 34);
                    for good in [34, 8] {
                        d.command(Command::StorageGoodSwitch { building: store, good, value: 1 });
                    }
                    let smith = first(d, 1, 22);
                    publish_selection(&d.tutorial, &[smith]);
                    building_event(&d.tutorial, 0x1c5, 0, 0, 22);
                    d.command(Command::ProductionOrder { building: smith, good: 5, amount: 10 });
                }
                "TUT_10_010_R" => d.command(Command::SoldierOrder { building: first(d, 1, 24), kind: 38, amount: 10 }),
                // The thieves once (a thief sent again on enemy land drops what he carries there)
                // to the enemy's iron bars: piles of 8 at (125, 211), (125, 213) and (160, 276);
                // the script wants fewer than 18 left.
                "TUT_10_008_R" => d.command(Command::SendTo { x: 125, y: 212, mode: 0, entities: d.settlers(48) }),
                _ => {}
            },
            |d, key| match key {
                "TUT_10_001_R" | "TUT_07_015_R" | "TUT_10_012_R" => d.space(),
                "TUT_10_004_R" if !ordered.get() => {
                    ordered.set(true);
                    d.command(Command::WorkshopOrder { building: first(d, 1, 31), item: 1, amount: 1 });
                }
                // The war ship sent next to the enemy big tower.
                "TUT_10_005_R" => {
                    let ships: Vec<u16> = d.ask(|w| s4_sim::vehicles::iter(w, 1, 1).collect());
                    let t = first(d, 2, 47);
                    if t != 0 && !ships.is_empty() {
                        let (x, y) = pos(d, t);
                        d.command(Command::SendTo { x: x as u16, y: y as u16, mode: 0, entities: ships });
                    }
                }
                // The special spell (7) next to the priest.
                "TUT_10_006_R" if !cast.get() => {
                    if let Some(&p) = d.settlers(45).first() {
                        let (x, y) = pos(d, p);
                        cast.set(true);
                        d.command(Command::CastSpell { priest: p, spell: 7, x: x as u16 + 2, y: y as u16 });
                    }
                }
                // Every axe warrior to the first enemy small tower.
                "TUT_10_011_R" => {
                    let t = first(d, 2, 46);
                    if t != 0 {
                        let (x, y) = pos(d, t);
                        d.command(Command::SendTo { x: x as u16, y: y as u16, mode: 0, entities: d.settlers(38) });
                    }
                }
                _ => {}
            },
        );
        assert_eq!(done.last(), Some(&"TUT_10_012_R"));
    }

    /// Tutorial 11 (free play, `Tutorial.EnableAllControls`): the wood, food, metal and military
    /// buildings the script counts (`Buildings.Amount`), 20 swordsmen and 20 bowmen, then every
    /// small tower of the computer player (2) taken. The driver plays a simple economy: a stone
    /// cutter next to stone and a woodcutter next to trees (again when theirs run out), the
    /// forester next to the woodcutter, mines on their resource, everything else near the
    /// player's first building; ten bows ordered from the weapon smith (5033).
    #[test]
    fn tutorial11_played_to_won() {
        use s4_sim::Command;
        use s4_sim::entity::ObjType;
        let Some(d) = Driver::start(11) else { return };
        assert!(d.ask(|w| w.buildings_amount(2, 46, 2)) > 0, "the computer player's small towers");
        let pos = |w: &s4_sim::World, b: u16| w.entities.by_index(b as u32).map_or((0, 0), |e| (e.x(), e.y()));
        let centre = d.ask(move |w| {
            let b = w.entities.iter().find(|(_, e)| e.obj_type == ObjType::Building && e.owner == 1).map_or(0, |(i, _)| i);
            pos(w, b)
        });
        // The deco objects giving `good` (stone 32, logs 22) within 10 of the building's
        // working area.
        let resources = move |w: &s4_sim::World, b: u16, good: u8| {
            s4_sim::jobs::core::building_working_area(w, b).map_or(0, |a| {
                s4_sim::spiral::walk(w.grid, a.x(), a.y(), 10)
                    .filter(|&(x, y)| {
                        let id = w.layers.deco_object[w.grid.index(x as u32, y as u32)];
                        id != 0 && s4_sim::deco::good_type(w, id) == good
                    })
                    .count()
            })
        };
        // A site for `k`: next to `good`'s objects, else nearest to `at`.
        let site = move |w: &s4_sim::World, k: u8, good: u8, at: (i32, i32)| {
            let size = w.grid.size() as i32;
            let res: Vec<(i32, i32)> = if good == 0 {
                Vec::new()
            } else {
                // Static deco instances share one entity over many nodes: scan the layer.
                (0..size * size)
                    .filter(|&n| {
                        let id = w.layers.deco_object[n as usize];
                        id != 0 && s4_sim::deco::good_type(w, id) == good && w.owner_of(n as usize) == 1
                    })
                    .map(|n| (n % size, n / size))
                    .collect()
            };
            let near = |x: i32, y: i32| res.iter().map(|&(rx, ry)| s4_sim::spiral::grid_distance(rx - x, ry - y)).min().unwrap_or(0);
            // A mine on the most of its resource (resource layer: type << 4 | amount) among the
            // first 64 spiral entries, as the mine digs (0x54bef0).
            let ore = match k {
                15 => 0x20,
                16 => 0x30,
                17 => 0x10,
                _ => 0,
            };
            let ore_at = |x: i32, y: i32| -> i32 {
                if ore == 0 {
                    return 0;
                }
                (0..64)
                    .filter_map(|i| w.grid.checked_index(x + s4_sim::spiral::dx(i), y + s4_sim::spiral::dy(i)))
                    .map(|n| w.layers.resource[n])
                    .filter(|&r| r & 0xf0 == ore)
                    .map(|r| (r & 0xf) as i32)
                    .sum()
            };
            s4_sim::build::possible_sites(w, 1, k, (0, 0), (size, size))
                .into_iter()
                .map(|(sx, sy, _)| ((-ore_at(sx as i32, sy as i32), near(sx as i32, sy as i32) / 4, s4_sim::spiral::grid_distance(sx as i32 - at.0, sy as i32 - at.1)), sx, sy))
                .min()
                .map(|(_, x, y)| (x, y))
        };
        let build_at = |d: &Driver, k: u8, good: u8, at: (i32, i32)| match d.ask(move |w| site(w, k, good, at)) {
            Some((x, y)) => d.command(Command::Build { building: k as u32, x, y }),
            None => eprintln!("no site for {k}"),
        };
        let build = |d: &Driver, kinds: &[u8]| {
            for &k in kinds {
                if d.ask(move |w| w.buildings_amount(1, k as i32, 2)) == 0 {
                    build_at(d, k, 0, centre);
                }
            }
        };
        // The raw materials: a cutter (4 stone, 1 wood) whose objects ran out gets a new one.
        let keep = |d: &Driver| {
            for (k, good) in [(4u8, 32u8), (1, 22)] {
                let (building, working) = d.ask(move |w| {
                    let list = w.building_list(1, k as i32);
                    let building = list.iter().any(|&b| w.entities.by_index(b as u32).is_some_and(|e| e.flags & s4_sim::entity::flag::READY == 0));
                    (building, list.iter().any(|&b| resources(w, b, good) > 0))
                });
                if !building && !working {
                    build_at(d, k, good, centre);
                }
            }
            let (wood, foresters) = d.ask(|w| (w.first_building(1, 1), w.buildings_amount(1, 2, 2)));
            if wood != 0 && foresters == 0 {
                let at = d.ask(move |w| pos(w, wood));
                build_at(d, 2, 0, at);
            }
        };
        let soldiers = |d: &Driver| {
            d.ask(|w| {
                w.entities
                    .iter()
                    .filter(|(_, e)| e.obj_type == ObjType::Settler && e.owner == 1 && matches!(e.kind, 29..=34) && e.flags & s4_sim::entity::flag::SELECTABLE != 0)
                    .map(|(i, _)| i)
                    .collect::<Vec<u16>>()
            })
        };
        let ordered = std::cell::Cell::new(false);
        let done = d.play(
            300_000,
            |d, key| {
                if matches!(key, "TUT_12_001_R" | "TUT_12_008_R") {
                    d.space();
                }
            },
            |d, key| {
                keep(d);
                match key {
                    "TUT_12_001_R" | "TUT_12_008_R" => d.space(),
                    "TUT_12_002_R" => build(d, &[3]),
                    "TUT_12_003_R" => build(d, &[11, 12, 9, 10, 6, 8, 5]),
                    "TUT_12_004_R" => build(d, &[15, 17, 20, 19, 16]),
                    "TUT_12_005_R" => build(d, &[22, 21, 24, 43, 35]),
                    "TUT_12_006_R" if !ordered.get() => {
                        let b = d.ask(|w| w.first_building(1, 24));
                        ordered.set(true);
                        d.command(Command::SoldierOrder { building: b, kind: 29, amount: 20 });
                        d.command(Command::SoldierOrder { building: b, kind: 32, amount: 20 });
                        // Bows from the weapon smith (5033).
                        let smith = d.ask(|w| w.first_building(1, 22));
                        d.command(Command::ProductionOrder { building: smith, good: 8, amount: 10 });
                    }
                    // Every free soldier to the first enemy small tower.
                    "TUT_12_007_R" => {
                        let towers = d.ask(|w| w.building_list(2, 46));
                        if let Some(&t) = towers.first() {
                            let (x, y) = d.ask(move |w| pos(w, t));
                            d.command(Command::SendTo { x: x as u16, y: y as u16, mode: 0, entities: soldiers(d) });
                        }
                    }
                    _ => {}
                }
            },
        );
        assert_eq!(done.last(), Some(&"TUT_12_008_R"));
    }

    /// Tutorial 12: ten gardeners (`SetSpecialistWish`, counted by the script's
    /// `SETTLER_CHANGE_TYPE`), one sent onto the cursor, the dark land around two points made
    /// green (`Game.IsAreaGreen`), then a woodcutter and a sawmill until a log arrives
    /// (`GOODARRIVE`).
    #[test]
    fn tutorial12_played_to_won() {
        use s4_sim::Command;
        let Some(d) = Driver::start(12) else { return };
        assert!(!d.ask(|w| w.is_area_green(205, 120, 2) || w.is_area_green(209, 130, 1)), "dark land at the start");
        let send = |d: &Driver, n: usize, x: u16, y: u16| {
            let g = d.settlers(50);
            d.command(Command::SendTo { x, y, mode: 0, entities: g.into_iter().take(n).collect() });
        };
        let built = std::cell::Cell::new(false);
        let done = d.play(
            100_000,
            |d, key| match key {
                "TUT_11_001_R" | "TUT_11_002_R" | "TUT_11_009_R" => d.space(),
                "TUT_11_003_R" => {
                    let eco = d.ask(|w| w.economy.player_ecos[1].first().copied().unwrap_or(0));
                    d.command(Command::SetSpecialistWish { eco_sector: eco, specialist: 50, value: 10 });
                }
                "TUT_11_004_R" => send(d, 1, 201, 120),
                "TUT_11_005_R" => send(d, 10, 205, 120),
                "TUT_11_006_R" => send(d, 10, 209, 130),
                _ => {}
            },
            |d, key| match key {
                "TUT_11_001_R" | "TUT_11_002_R" | "TUT_11_009_R" => d.space(),
                "TUT_11_007_R" if !built.get() => {
                    built.set(d.try_build_near(1, 209, 130) && d.try_build_near(3, 200, 125));
                }
                _ => {}
            },
        );
        assert_eq!(done.last(), Some(&"TUT_11_009_R"));
    }

    #[test]
    fn space_once_per_press_and_the_window_buttons() {
        let mut t = TutorialUi::default();
        let ev = |id, w| UiEv { id, w, l: 0, building: 0, settlers: false };
        assert_eq!(t.on_event(ev(0xb, 0x20)), Some((events::SPACE, vec![])));
        assert_eq!(t.on_event(ev(0xb, 0x20)), None);
        t.on_event(ev(0x14, 0x20));
        assert_eq!(t.on_event(ev(0xb, 0x20)), Some((events::SPACE, vec![])));
        assert_eq!(t.on_event(ev(0x275, 0)), Some((events::COMMAND, vec![1712])));
        assert_eq!(t.on_event(ev(0x276, 0)), Some((events::COMMAND, vec![1711])));
        assert_eq!(t.pending.len(), 4);
    }
}
