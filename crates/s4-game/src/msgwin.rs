//! The message window between the message list (messages.rs, `CTextMsgHandler`) and the GUI
//! (s4-viewer `msg_window`): what `CGuiEventHandler` (0x4805a0) and the procedures
//! `HiddenChatMenuCallback` (0x4797f0, dialog 59) and `ChatEvents` (0x499d80, 60..=65) do with
//! it.
//! * [`sync`], every frame: the window opens (0x269) / closes (0x26a) as the list's state says,
//!   a refresh (`dirty`, `ChatCallback`) rebuilds its lines from the unread messages (string
//!   text and class icon), and the closed window's button shows the blink images.
//! * [`ui`]: the button (891) opens the window, close (968) closes it, a line (958..=961) is the
//!   click 0x52a800 on its message (the camera goes to the message's node, event 0x13), the
//!   scroll buttons (966 / 967) and the wheel move the lines by one message, codes 5 / 6
//!   highlight a message's lines.

use s4_viewer::client;
use s4_viewer::gpu::Gpu;
use s4_viewer::guistate::UiEvent;
use s4_viewer::msg_window::{BUTTON, BUTTON_LINK, CLOSE, DOWN, LINES, UP};

use crate::messages::Messages;

/// `GetTickCount` for the GUI engine's timers (ms since the first call).
fn now_ms() -> u64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed().as_millis() as u64
}

/// The window's dialogs (`SelectChatSize`).
fn window(dialog: usize) -> bool {
    (60..=65).contains(&dialog)
}

/// Follow the list's window state, refresh and blink; recompose when the GUI changed.
/// `extended_tooltips`: `GameManager +0x144` (settings.rs).
pub fn sync(gpu: &mut Gpu, msgs: &mut Messages, extended_tooltips: bool) {
    let screen_w = (gpu.size().0 as f32 / gpu.gui_scale.max(0.1)) as u32;
    let Some(ui) = gpu.ui.as_mut() else { return };
    let mut changed = false;
    if msgs.window && ui.msg_dialog.is_none() {
        ui.msg_open(screen_w, extended_tooltips);
        changed = true;
    } else if !msgs.window && ui.msg_dialog.is_some() {
        ui.msg_close();
        changed = true;
    }
    if msgs.window && msgs.dirty {
        // `sub_12AAD0`: a message's formatted text (+0x20) if it has one, else its string.
        let lines: Vec<(String, u16)> = msgs.lines().map(|(_, m)| (m.formatted.clone().unwrap_or_else(|| ui.string(m.text as usize)), m.icon)).collect();
        ui.msg_set(lines);
        msgs.dirty = false;
        changed = true;
    }
    if !msgs.window {
        changed |= ui.msg_button_lit(msgs.blink_on);
    }
    changed |= ui.ext_tip_frame(screen_w, extended_tooltips, now_ms());
    if changed {
        client::recompose(gpu);
    }
}

/// What the tutorial asks of the rest of the client this frame ([`tutorial`]).
#[derive(Debug, Default)]
pub struct TutorialFrame {
    /// The procedures' events of `Tutorial.PressButton` (`SelectControl(dialog, control, 1)`),
    /// for the client's usual UI-event handling.
    pub events: Vec<UiEvent>,
    /// `Tutorial.SelectNextBuilding(type)`: UI event 0x198 (0, type) each.
    pub next_building: Vec<u8>,
}

/// The tutorial's GUI side, every frame in a tutorial game (`TutorialUi::active`):
/// * the GUI engine's control hook (0x146b054 = 0x4fa520, `SetupTutorialEvent`; dialog 0x24
///   exempt, 0x11695d4) installed once, and `IGuiEngine::Resize` (every open dialog asked again)
///   whenever the script changed the control bitsets;
/// * the window's text: GUI event 0x263 for each line `Tutorial.ShowText` changed (the window
///   opens on the first);
/// * the window's UI events 0x275 / 0x276 (its own init's too) to `CTutorial::OnEvent`;
/// * the requests `PressButton` / `SelectNextBuilding` taken from the shared list.
pub fn tutorial(gpu: &mut Gpu, shared: &crate::tutorial::Shared, extended_tooltips: bool) -> TutorialFrame {
    use std::hash::{Hash, Hasher};
    let mut out = TutorialFrame::default();
    let screen_w = (gpu.size().0 as f32 / gpu.gui_scale.max(0.1)) as u32;
    // Taken under the lock, used after it: the hook locks it again.
    let (text, seq, hash, presses, controls) = {
        let mut t = shared.lock().unwrap_or_else(|e| e.into_inner());
        if !t.active {
            return out;
        }
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (t.controls.disabled, t.controls.always_a, t.controls.always_b).hash(&mut h);
        let mut presses = Vec::new();
        t.requests.retain(|r| match *r {
            crate::tutorial::Request::PressButton { dialog, control } => {
                presses.push((dialog as usize, control));
                false
            }
            crate::tutorial::Request::SelectNextBuilding(b) => {
                out.next_building.push(b);
                false
            }
            _ => true,
        });
        (t.text, t.text_seq, h.finish(), presses, t.controls.clone())
    };
    let Some(ui) = gpu.ui.as_mut() else { return out };
    let mut changed = false;
    let (seen_seq, seen_hash) = ui.tut_seen.unwrap_or((0, 0));
    ui.tutorial = true;
    if ui.tut_seen.is_none() || seen_hash != hash {
        // The hook owns a copy of the bitsets (the script's are behind the shared lock, which the
        // client holds while it opens dialogs). [inf] The GUI's enable wishes the hook records
        // (0x15aad38) start afresh with each copy: only the setup hook 0x4fa4d0 reads them.
        let mut controls = controls;
        ui.state.hook = Some(Box::new(move |a, c, b, enabled, flags| controls.hook(a, c, b, enabled, flags)));
        ui.state.hook_exempt = Some(0x24);
        ui.state.rehook();
        changed = true;
    }
    if ui.tut_seen.is_none() || seen_seq != seq {
        // [inf] Each line the sequence brought (the script sets one line per event).
        for (line, id) in text.iter().enumerate() {
            if let Some(id) = *id {
                let s = ui.string(id as usize);
                ui.tutorial_text(line, s, screen_w, extended_tooltips);
            }
        }
        changed = true;
    }
    ui.tut_seen = Some((seq, hash));
    for (dialog, control) in presses {
        out.events.extend(ui.state.select_control(dialog, control));
        changed = true;
    }
    if changed {
        client::recompose(gpu);
    }
    tutorial_events(gpu, shared);
    out
}

/// The tutorial window's UI events (0x275 / 0x276) to `CTutorial::OnEvent`, as they happen.
pub fn tutorial_events(gpu: &mut Gpu, shared: &crate::tutorial::Shared) {
    let Some(ev) = gpu.ui.as_mut().map(|ui| std::mem::take(&mut ui.tut_events)) else { return };
    if ev.is_empty() {
        return;
    }
    let mut t = shared.lock().unwrap_or_else(|e| e.into_inner());
    for id in ev {
        t.on_event(crate::tutorial::UiEv { id, w: 0, l: 0, building: 0, settlers: false });
    }
}

/// Enter in the game (`IngameKeybindings` 0x4890b0: key 0xd without Ctrl / Shift, not in the
/// tutorial): event 0x269 (the window opens if closed) and `UIChatInteraction` (the chat input
/// shown and focused). Returns the GUI sounds.
pub fn chat_key(gpu: &mut Gpu, msgs: &mut Messages, extended_tooltips: bool) -> Vec<u16> {
    let snd = msgs.open();
    sync(gpu, msgs, extended_tooltips);
    if let Some(ui) = gpu.ui.as_mut() {
        ui.chat_open();
    }
    client::recompose(gpu);
    snd
}

/// The chat input's return (`ChatEvents` 0x499d80 code 3 on 0x3c9): its text (`None` when
/// empty), then `sub_99B90` (the input hidden, four slots again).
pub fn chat_done(gpu: &mut Gpu) -> Option<String> {
    let ui = gpu.ui.as_mut()?;
    let text = ui.chat_text();
    ui.chat_close();
    client::recompose(gpu);
    (!text.is_empty()).then_some(text)
}

/// The chat input's return among `ev`: the line sent to the recipients (`ChatMessageFilter`, our
/// bit included) as [`crate::sim::NET_CHAT`], or the cheat console's ([`Console::line`]).
/// Returns the GUI sounds of the console's messages.
pub fn chat_events(ev: &[UiEvent], gpu: &mut Gpu, sim: &crate::sim::SimHandle, console: &mut Console, msgs: Option<&mut Messages>) -> Vec<u16> {
    let mut snd = Vec::new();
    let mut msgs = msgs;
    for e in ev {
        let UiEvent::EditDone { dialog, link } = *e else { continue };
        if link != s4_viewer::msg_window::CHAT_INPUT || gpu.ui.as_ref().is_none_or(|u| u.msg_dialog != Some(dialog)) {
            continue;
        }
        let Some(text) = chat_done(gpu) else { continue };
        let line = match console.line(&text, !sim.names.is_empty()) {
            Line::Chat => text.as_str(),
            Line::Drop => continue,
            Line::Message(m) => {
                if let Some(msgs) = msgs.as_deref_mut() {
                    snd.extend(msgs.add_console(m.to_string(), 9));
                }
                continue;
            }
            Line::Win => {
                // Event 0x37 with the local player's alliance: the local team wins.
                sim.outcome.store(crate::script::WON, std::sync::atomic::Ordering::Relaxed);
                continue;
            }
            Line::Lua(code) => {
                sim.console(code);
                continue;
            }
        };
        let local = sim.local;
        let mask = gpu.ui.as_ref().map_or(u32::MAX, |u| u.state.page_info.chat.mask) | 1 << (local - 1);
        let data: Vec<u16> = line.encode_utf16().collect();
        let command = s4_sim::Command::Other { kind: crate::sim::NET_CHAT, wparam: mask, lparam: 0, data };
        let tick = s4_net::commands::execution_tick(sim.snapshot().tick);
        sim.command(s4_net::commands::Stamped { tick, command: s4_sim::PlayerCommand { player: local, command } });
    }
    snd
}

/// The chat lines that arrived for us (the mask has our bit): into the message list as
/// "[team] name: text" (0xfa5 → 0x52a060). `name(p)`: the sender's name. Returns the sounds.
pub fn chat_arrivals(sim: &crate::sim::SimHandle, msgs: &mut Messages, teams: &[u8; 9], name: &dyn Fn(u8) -> String) -> Vec<u16> {
    let mut snd = Vec::new();
    for (from, mask, text) in sim.drain_chat() {
        if from == 0 || mask & 1 << (sim.local - 1) == 0 {
            continue;
        }
        let team = teams.get(from as usize).copied().unwrap_or(0);
        snd.extend(msgs.add_chat(chat_line(team, &name(from), &text), from as u32 - 1));
    }
    snd
}

/// The session's notices: a player waited for too long, 0x8d5 with the name (0x5a2a90,
/// [`Messages::player_waited_for`]); the network players that left: 0x8d2 with the name (`EventManagement` event 0x39, 0x517763;
/// [`Messages::player_left`]). [not ported] What the release does next: `SetSettings`, and a
/// player marked for it (0x149b608 + 0x3c p) handed to the AI (0x146b148 vtbl+0x1c).
/// Returns the sounds.
pub fn net_notices(gpu: &Gpu, sim: &crate::sim::SimHandle, msgs: &mut Messages, name: &dyn Fn(u8) -> String) -> Vec<u16> {
    use crate::sim::NetNotice;
    let mut snd = Vec::new();
    for n in sim.drain_net() {
        let (text, p) = match n {
            NetNotice::Left(p) => (0x8d2, p),
            NetNotice::Waiting(p) => (0x8d5, p),
            // [ours] 2509 "Connection to lobby server lost!": the internet game goes on alone.
            NetNotice::RelayLost => {
                snd.extend(msgs.system(2509, 0));
                continue;
            }
        };
        let line = gpu.ui.as_ref().map(|u| printf_s(&u.string(text), &name(p)));
        snd.extend(match n {
            NetNotice::Left(_) => msgs.player_left(p, line),
            NetNotice::Waiting(_) => msgs.player_waited_for(p, line),
            NetNotice::RelayLost => Vec::new(),
        });
    }
    snd
}

/// The script's messages taken from the shared list (the other requests stay for
/// `tutorial::client_frame`): `Game.ShowText` (0x52a320, [`Messages::show_text`]) and event 0x38
/// (player lost, [`Messages::player_lost`]: another player's line is the string 0x8d1 `vsprintf`-ed
/// with the name), `Sounds.PlaySound` and `Debug.ShowTextMessage`. Returns the GUI sounds.
pub fn script_messages(gpu: &Gpu, shared: &crate::tutorial::Shared, msgs: &mut Messages, local: u8, name: &dyn Fn(u8) -> String) -> Vec<u16> {
    use crate::tutorial::Request;
    let mut taken = Vec::new();
    shared.lock().unwrap_or_else(|e| e.into_inner()).requests.retain(|r| match r {
        Request::ShowText { .. } | Request::PlayerLost(_) | Request::PlaySound(_) | Request::DebugText(_) => {
            taken.push(r.clone());
            false
        }
        _ => true,
    });
    let mut snd = Vec::new();
    for r in taken {
        match r {
            Request::ShowText { text, x, y } => snd.extend(msgs.show_text(text, x, y)),
            Request::PlayerLost(p) => {
                // The record's name (0x149b348 + 0x18 k): a network player's, else the chat
                // page's (the map's name, a computer's from AINames.cfg, `chat_names`).
                let who = |u: &s4_viewer::gui::Gui| {
                    let n = name(p);
                    if n.is_empty() { u.state.page_info.chat.names.get(p as usize).cloned().unwrap_or_default() } else { n }
                };
                let line = (p != local).then(|| gpu.ui.as_ref().map(|u| printf_s(&u.string(0x8d1), &who(u)))).flatten();
                snd.extend(msgs.player_lost(p, local, line));
            }
            Request::PlaySound(id) => snd.push(id),
            Request::DebugText(line) => snd.extend(msgs.add_console(line, 0)),
            _ => {}
        }
    }
    snd
}

/// `vsprintf` of a string with one `%s` argument (`%%` → `%`).
fn printf_s(fmt: &str, arg: &str) -> String {
    let mut out = String::new();
    let mut used = false;
    let mut it = fmt.chars().peekable();
    while let Some(c) = it.next() {
        if c == '%' {
            match it.peek() {
                Some('%') => {
                    it.next();
                    out.push('%');
                    continue;
                }
                Some('s') if !used => {
                    it.next();
                    out.push_str(arg);
                    used = true;
                    continue;
                }
                _ => {}
            }
        }
        out.push(c);
    }
    out
}

/// A player's name: the lobby's in a network game, `own` for the local player otherwise.
pub fn player_name(sim: &crate::sim::SimHandle, own: &str, p: u8) -> String {
    if let Some((_, n)) = sim.names.iter().find(|(id, _)| *id == p) {
        return n.clone();
    }
    if p == sim.local { own.to_string() } else { String::new() }
}

/// The cheat console of `EventManagement` 0x26 (the line to send, 0x518770..0x518903):
/// `CStateGame +0x102`, the cheats switch, kept for the game.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Console {
    pub cheats: bool,
}

/// What a typed line does ([`Console::line`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Line<'a> {
    /// Sent as a chat line (0xfa5).
    Chat,
    /// Nothing.
    Drop,
    /// 0x52a100 (`messages::Messages::add_console`): "Cheats activated!" / "Cheats deactivated!"
    /// (.rdata 0x1056a2c / 0x1056a40).
    Message(&'static str),
    /// "!win" (0x515420): event 0x37 with the local player's alliance.
    Win,
    /// Any other line with the cheats on (0x515420 → 0x67f2a0): run without its '!' as a Lua
    /// chunk in the game script's state.
    Lua(&'a str),
}

impl Console {
    /// `EventManagement` 0x26 for `text` (`network`: `CStateGame +0xa4` = `GlobalGameStateObject
    /// +0x74`, `GlobalGameState +0x2cc` clear, which only the network lobby clears, 0x51bd14 /
    /// 0x5a335d):
    /// * not starting with '!': chat;
    /// * "!hjb" (`CHJBMgr` vtbl+4 0x57b660, in every game): the "HJB" easter egg's vote (0x1e8)
    ///   when vtbl+0x18 (the GFX file 25 loads) and vtbl+8 (the player is named "Jodowein") pass
    ///   and +0xa7 is clear [not ported]; otherwise the cheats go off with "Cheats deactivated!";
    /// * a network game: chat;
    /// * "!incubation2": the cheats toggle, with their message;
    /// * the cheats on: "!win" wins, anything else runs as Lua;
    /// * else nothing.
    pub fn line<'a>(&mut self, text: &'a str, network: bool) -> Line<'a> {
        if !text.starts_with('!') {
            return Line::Chat;
        }
        if text == "!hjb" {
            self.cheats = false;
            return Line::Message("Cheats deactivated!");
        }
        if network {
            return Line::Chat;
        }
        if text == "!incubation2" {
            self.cheats = !self.cheats;
            return Line::Message(if self.cheats { "Cheats activated!" } else { "Cheats deactivated!" });
        }
        if !self.cheats {
            return Line::Drop;
        }
        if text == "!win" { Line::Win } else { Line::Lua(&text[1..]) }
    }
}

/// `EventManagement` 0xfa5 (a chat line arrived): "[<team digit>] <name>: <text>", the text cut so
/// the line stays under 0x200 bytes.
pub fn chat_line(team: u8, name: &str, text: &str) -> String {
    let head = format!("[{}] {name}: ", (b'0' + team) as char);
    let room = 0x200usize.saturating_sub(head.len() + 1);
    let mut t = text.to_string();
    while t.len() > room {
        t.pop();
    }
    head + &t
}

/// The GUI timer for the short tooltip (s4-viewer `gui::ShortTooltip`), every frame, with the
/// cursor in window px.
pub fn tooltip(gpu: &mut Gpu, cursor: Option<(f64, f64)>) {
    let scale = gpu.gui_scale.max(0.1) as f64;
    let screen_w = (gpu.size().0 as f64 / scale) as u32;
    let Some(ui) = gpu.ui.as_mut() else { return };
    let at = cursor.map(|(x, y)| ((x / scale) as i32, (y / scale) as i32));
    if ui.short_tip_frame(at, screen_w, now_ms()) {
        client::recompose(gpu);
    }
}

/// A left press: the short tooltip hides and is held back (0x673893).
pub fn tooltip_press(gpu: &mut Gpu) {
    if gpu.ui.as_mut().is_some_and(|ui| ui.short_tip_press(now_ms())) {
        client::recompose(gpu);
    }
}

/// The GUI's events for the message window. Returns the node to centre the camera on (a line's
/// message position) and the sounds the list's handlers play.
pub fn ui(events: &[UiEvent], gpu: &mut Gpu, msgs: &mut Messages, extended_tooltips: bool) -> (Option<(i32, i32)>, Vec<u16>) {
    let mut centre = None;
    let mut snd = Vec::new();
    let mut touched = false;
    for e in events {
        let Some(gui) = gpu.ui.as_mut() else { break };
        match *e {
            UiEvent::Command { dialog: BUTTON, link: BUTTON_LINK, .. } => snd.extend(msgs.open()),
            UiEvent::Command { dialog, link: CLOSE, .. } if window(dialog) => msgs.close(),
            UiEvent::Command { dialog, link, .. } if window(dialog) && LINES.contains(&link) => {
                if let Some(i) = gui.msg_line(link).filter(|&i| i >= 0) {
                    let (pos, s) = msgs.click(i as usize);
                    centre = pos.or(centre);
                    snd.extend(s);
                }
            }
            UiEvent::Command { dialog, link: l @ (UP | DOWN), .. } if window(dialog) => {
                let last = gui.msg.last;
                gui.msg_show(if l == UP { last - 1 } else { last + 1 });
                touched = true;
            }
            UiEvent::Highlight { dialog, link, on } if window(dialog) => {
                gui.msg_highlight(link, on);
                touched = true;
            }
            UiEvent::Wheel { dialog, delta } if window(dialog) => {
                gui.msg_wheel(delta);
                touched = true;
            }
            // Code 9: the extended tooltip window (s4-viewer `gui::ExtTooltip`).
            UiEvent::Hover { dialog, tooltip, .. } => touched |= gui.ext_tip_enter(dialog, tooltip, now_ms()),
            _ => {}
        }
    }
    sync(gpu, msgs, extended_tooltips);
    if touched {
        client::recompose(gpu);
    }
    (centre, snd)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::{Classes, Request};
    use s4_formats::gamedata::Race;

    #[test]
    fn printf_one_string() {
        assert_eq!(printf_s("Player %s was destroyed (100%%).", "Ann"), "Player Ann was destroyed (100%).");
        assert_eq!(printf_s("%s %s", "a"), "a %s");
    }

    /// Event 0x38 for a computer (no network name): GUI_SYS_PLAYER_DESTROYED formatted with the
    /// chat page's name for it (the record name 0x149b348 + 0x18 k, from AINames.cfg).
    #[test]
    fn computer_destroyed_by_name() {
        if !s4_formats::game_dir().join("Menu").exists() {
            return;
        }
        let mut gpu = Gpu::headless(1280, 1024).unwrap();
        gpu.ui = Some(s4_viewer::gui::Gui::new(Race::ROMAN, None, 1, 1024).unwrap());
        gpu.ui.as_mut().unwrap().state.page_info.chat.names[2] = "Hiawatha".into();
        let shared = crate::tutorial::shared();
        shared.lock().unwrap().requests.push(crate::tutorial::Request::PlayerLost(2));
        let mut msgs = Messages::new(Classes::default(), u32::MAX, false);
        script_messages(&gpu, &shared, &mut msgs, 1, &|_| String::new());
        let want = printf_s(&gpu.ui.as_ref().unwrap().string(0x8d1), "Hiawatha");
        assert!(want.contains("Hiawatha"), "{want}");
        assert_eq!(msgs.lines().next().and_then(|(_, m)| m.formatted.clone()), Some(want));
    }

    /// Button → window (dialog 64 at 1280 px with extended tooltips) with the message's line and
    /// icon; a click on it goes to its node and leaves the window empty; close → the button.
    #[test]
    fn button_window_click_close() {
        if !s4_formats::game_dir().join("Menu").exists() {
            return;
        }
        let mut gpu = Gpu::headless(1280, 1024).unwrap();
        gpu.ui = Some(s4_viewer::gui::Gui::new(Race::ROMAN, None, 1, 1024).unwrap());
        let mut msgs = Messages::new(Classes::default(), u32::MAX, false);
        let open = |gpu: &Gpu| gpu.ui.as_ref().unwrap().state.open.clone();
        assert!(open(&gpu).contains(&BUTTON));
        msgs.add(Request { text: 0x9fa, player: 1, x: 40, y: 50, filtered: false });
        sync(&mut gpu, &mut msgs, true);
        assert!(msgs.blink && !msgs.window);
        let click = |link: u16, dialog: usize| UiEvent::Command { dialog, link, effects: 0 };
        let (_, snd) = ui(&[click(BUTTON_LINK, BUTTON)], &mut gpu, &mut msgs, true);
        assert_eq!(snd, vec![7], "0x1773 → 0x267 without new: an open window plays 7");
        let g = gpu.ui.as_ref().unwrap();
        assert_eq!(g.msg_dialog, Some(64));
        assert!(open(&gpu).contains(&64) && !open(&gpu).contains(&BUTTON));
        let d = g.state.set.dialog(64).unwrap();
        assert_eq!((d.x, d.y), (209, 1024 - 1 - 98));
        let text = g.string(0x9fa);
        assert_eq!(g.msg.slots[3].text.as_deref(), Some(text.as_str()));
        assert_eq!((g.msg.slots[3].icon, g.msg.slots[3].message), (Some(0xa5), 0));
        assert_eq!(g.state.texts.get(&(64, LINES[3])), Some(&text));
        assert!(g.msg.slots[..3].iter().all(|s| s.text.is_none()));
        let (centre, _) = ui(&[click(LINES[3], 64)], &mut gpu, &mut msgs, true);
        assert_eq!(centre, Some((40, 50)));
        assert!(gpu.ui.as_ref().unwrap().msg.slots.iter().all(|s| s.text.is_none()), "read: gone from the window");
        ui(&[click(CLOSE, 64)], &mut gpu, &mut msgs, true);
        assert!(!msgs.window && gpu.ui.as_ref().unwrap().msg_dialog.is_none());
        assert!(open(&gpu).contains(&BUTTON) && !open(&gpu).contains(&64));
    }

    /// The extended tooltip window (dialog 73, `ExtTooltip`): the cursor on a main tab shows its
    /// extended tooltip at the bottom right; a control without one blanks it, 700 ms later it
    /// hides; off the GUI it blanks and hides the same way; switched off it never shows.
    #[test]
    fn extended_tooltip_window() {
        if !s4_formats::game_dir().join("Menu").exists() {
            return;
        }
        let mut gpu = Gpu::headless(1280, 1024).unwrap();
        gpu.ui = Some(s4_viewer::gui::Gui::new(Race::ROMAN, None, 1, 1024).unwrap());
        let g = gpu.ui.as_mut().unwrap();
        g.ext_tip_frame(1280, true, 0);
        let d = g.state.set.dialog(73).unwrap();
        assert_eq!((d.x, d.y), (1280 - 1 - 296, 1024 - 1 - 98));
        // Tab 607 (dialog 0 at (55, 175), 48 x 35): extended tooltip 1870.
        let ev = g.state.mouse_move(79, 192);
        let hover = ev.iter().find_map(|e| match *e {
            UiEvent::Hover { dialog, tooltip, .. } => Some((dialog, tooltip)),
            _ => None,
        });
        assert_eq!(hover, Some((0, Some(1870))));
        assert!(g.ext_tip_enter(0, Some(1870), 10));
        assert!(g.ext_tip.shown && g.ext_tip.text == g.string(1870) && !g.ext_tip.text.is_empty());
        assert!(!g.ext_tip_frame(1280, true, 20), "still on the tab");
        // A control without an extended tooltip: blank, hidden after 700 ms.
        assert!(g.ext_tip_enter(0, None, 100));
        assert!(g.ext_tip.shown && g.ext_tip.text.is_empty());
        assert!(!g.ext_tip_frame(1280, true, 800));
        assert!(g.ext_tip_frame(1280, true, 801) && !g.ext_tip.shown);
        // Back on the tab, then off the GUI.
        g.ext_tip_enter(0, Some(1870), 900);
        assert!(g.ext_tip.shown);
        g.state.mouse_move(700, 500);
        assert!(g.ext_tip_frame(1280, true, 950) && g.ext_tip.shown && g.ext_tip.text.is_empty());
        g.ext_tip_frame(1280, true, 1651);
        assert!(!g.ext_tip.shown);
        // Switched off.
        g.ext_tip_frame(1280, false, 2000);
        g.ext_tip_enter(0, Some(1870), 2001);
        assert!(!g.ext_tip.shown);
    }

    /// The short tooltip (`ShortTooltip`): on tab 607 its tooltip (1869) at the cursor + 18 px;
    /// hidden and held back after 8000 ms; leaving the control frees it; a left press hides it, and
    /// within 700 ms of the press a new control shows its tooltip only within 400 ms of leaving a
    /// shown one.
    #[test]
    fn short_tooltip() {
        if !s4_formats::game_dir().join("Menu").exists() {
            return;
        }
        let mut gpu = Gpu::headless(1280, 1024).unwrap();
        gpu.ui = Some(s4_viewer::gui::Gui::new(Race::ROMAN, None, 1, 1024).unwrap());
        let g = gpu.ui.as_mut().unwrap();
        let tab = (79, 192);
        g.state.mouse_move(tab.0, tab.1);
        assert!(g.short_tip_frame(Some(tab), 1280, 1000));
        assert!(g.tip.shown && g.tip.text == g.string(1869) && !g.tip.text.is_empty());
        assert_eq!((g.tip.rect.0, g.tip.rect.1), (79, 192 + 18));
        assert!(!g.short_tip_frame(Some(tab), 1280, 9000), "8000 ms not passed yet");
        assert!(g.short_tip_frame(Some(tab), 1280, 9001) && !g.tip.shown);
        assert!(!g.short_tip_frame(Some(tab), 1280, 9100), "held back on the same control");
        // Off the GUI and back: shown again.
        g.state.mouse_move(700, 500);
        g.short_tip_frame(Some((700, 500)), 1280, 9200);
        g.state.mouse_move(tab.0, tab.1);
        assert!(g.short_tip_frame(Some(tab), 1280, 9300) && g.tip.shown);
        // A press hides it (so leaving it notes no leave time); on tab 606 (at (9, 176)) the
        // tooltip waits until 700 ms after the press.
        assert!(g.short_tip_press(9400) && !g.tip.shown);
        g.state.mouse_move(30, 190);
        g.short_tip_frame(Some((30, 190)), 1280, 9450);
        assert!(!g.tip.shown, "left a hidden tooltip: no leave time, and 50 ms after the press");
        assert!(g.short_tip_frame(Some((30, 190)), 1280, 10101) && g.tip.shown, "700 ms after the press");
    }

    /// The tutorial's GUI side: the first text opens the tutorial window (dialog 70 at 1280 px
    /// with extended tooltips) with line 0 in font 0xb and line 1 in 0xe and sends 0x276 (script
    /// COMMAND 0x6af); a click on line 1712 sends 0x275 (COMMAND 0x6b0); `DisableExcept` greys
    /// dialog 0's controls but the kept ones; `PressButton` clicks the production tab.
    #[test]
    fn tutorial_window_hook_and_press() {
        use crate::tutorial::{Request, shared};
        use s4_script::events::COMMAND;
        if !s4_formats::game_dir().join("Menu").exists() {
            return;
        }
        let mut gpu = Gpu::headless(1280, 1024).unwrap();
        gpu.ui = Some(s4_viewer::gui::Gui::new(Race::ROMAN, None, 1, 1024).unwrap());
        let t = shared();
        {
            let mut u = t.lock().unwrap();
            u.active = true;
            u.text = [Some(100), Some(101)];
            u.text_seq = 1;
        }
        tutorial(&mut gpu, &t, true);
        let g = gpu.ui.as_ref().unwrap();
        assert_eq!(g.tut_dialog, Some(70));
        assert_eq!(g.state.texts.get(&(70, 0x6af)), Some(&g.string(100)));
        assert_eq!(g.state.texts.get(&(70, 0x6b0)), Some(&g.string(101)));
        assert_eq!((g.state.fonts.get(&(70, 0x6af)), g.state.fonts.get(&(70, 0x6b0))), (Some(&0xb), Some(&0xe)));
        assert!(!g.state.open.contains(&BUTTON));
        assert_eq!(std::mem::take(&mut t.lock().unwrap().pending), vec![(COMMAND, vec![0x6af])]);
        // A click on line 1712.
        let click = UiEvent::Command { dialog: 70, link: 0x6b0, effects: 0 };
        gpu.ui.as_mut().unwrap().graph_events(&[click]);
        tutorial_events(&mut gpu, &t);
        assert_eq!(std::mem::take(&mut t.lock().unwrap().pending), vec![(COMMAND, vec![0x6b0])]);
        // The hook: everything greyed but the kept controls (dialog 0's 610).
        let greyed = |gpu: &Gpu, d: usize, l: u16| {
            let s = &gpu.ui.as_ref().unwrap().state;
            s.index_of(d, l).map(|i| s.effects.get(&(d, i)).copied().unwrap_or(0) & 4 != 0)
        };
        assert_eq!(greyed(&gpu, 0, 606), Some(false));
        t.lock().unwrap().controls.disable_except(&[]);
        t.lock().unwrap().requests.push(Request::PressButton { dialog: 0, control: 608 });
        t.lock().unwrap().requests.push(Request::SetZoom(5));
        let tf = tutorial(&mut gpu, &t, true);
        assert_eq!((greyed(&gpu, 0, 606), greyed(&gpu, 0, 610)), (Some(true), Some(false)));
        assert!(tf.events.iter().any(|e| matches!(e, UiEvent::Command { dialog: 0, link: 608, .. })), "{:?}", tf.events);
        assert!(gpu.ui.as_ref().unwrap().state.open.contains(&5), "the production submenu opened");
        assert_eq!(t.lock().unwrap().requests, vec![Request::SetZoom(5)], "the other requests stay for client_frame");
        // Enabled again: the hook answers yes.
        t.lock().unwrap().controls.enable_all();
        tutorial(&mut gpu, &t, true);
        assert_eq!(greyed(&gpu, 0, 606), Some(false));
    }

    /// `EventManagement` 0x26's '!' lines (0x518770..0x518903): "!hjb" switches the cheats off
    /// in every game, a network game chats the rest, "!incubation2" toggles, the cheats on run
    /// "!win" / Lua (without the '!', 0x515550), off drop them.
    #[test]
    fn cheat_console_lines() {
        let mut c = Console::default();
        assert_eq!(c.line("hello", false), Line::Chat);
        assert_eq!(c.line("!win", false), Line::Drop);
        assert_eq!(c.line("!incubation2", true), Line::Chat);
        assert!(!c.cheats);
        assert_eq!(c.line("!incubation2", false), Line::Message("Cheats activated!"));
        assert!(c.cheats);
        assert_eq!(c.line("!win", false), Line::Win);
        assert_eq!(c.line("!win ", false), Line::Lua("win "));
        assert_eq!(c.line("!x = 1", false), Line::Lua("x = 1"));
        assert_eq!(c.line("!x = 1", true), Line::Chat);
        assert_eq!(c.line("!incubation2", false), Line::Message("Cheats deactivated!"));
        assert_eq!(c.line("!x = 1", false), Line::Drop);
        c.cheats = true;
        assert_eq!(c.line("!hjb", true), Line::Message("Cheats deactivated!"));
        assert!(!c.cheats);
    }
}
