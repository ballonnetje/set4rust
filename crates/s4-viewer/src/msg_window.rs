//! The message window and its button: the GUI side of the text messages (the list itself is
//! s4-game's `messages.rs`, `CTextMsgHandler`). Evidence: `tools/re/audio/{gui,chat,chatgui,
//! chatgui2}.c` (decompiles of `CGuiEventHandler` 0x4805a0 and the chat procedures) and
//! `tools/re/gui-2026-10-21/` (msgwin.c, render*.c, chat_vectors.py).
//!
//! # Button (dialog 59, `HiddenChatMenuCallback` 0x4797f0)
//! Opened with the first frame's event 0x27 (`+0x34 = 0x3b`), and again when the window closes
//! (0x26a, images 0x148 / 0x149). Link 891 (0x37b), code 3: events 0x269 (open the window) and
//! 0x1773 (refresh). While the closed window has unread messages its images blink between
//! 0x14a / 0x14b and 0x148 / 0x149 (timer event 0x15, every 5th). `ToggleUISurfaceVisibility`
//! (0x488650) puts it at x = 209 (0xd1), y = H − 1 − its height (H the output height).
//!
//! # Window (dialogs 60..=65, `ChatEvents` 0x499d80)
//! Event 0x269 closes the button and opens the dialog `SelectChatSize` (0x489900) picks by the
//! output width and `GameManager +0x144` (the extended tooltips, set from `ExtendedTooltip` by
//! `SetSettings` 0x514d30, 1 in game state 4): width ≥ 1280 → 64 / 65, < 1024 → 60 / 61, else
//! 62 / 63, the first of each pair with extended tooltips (narrower: dialog 73, the extended
//! tooltip window, takes the right of the bottom row). Placed at (209, H − 1 − height)
//! (`sub_2778C0`). Its flags 2: no background bitmap; `RenderGui` (0x6721e0) fills the dialog
//! with black on every other pixel (`sub_2788D0`, colour 0, rows alternating), the rest stays the
//! surface's colour key, so the map shows through a 50 % screen door.
//! * Init (code 0, `sub_99820`): the chat input 969 hidden; four slots (text 958 + k, icon
//!   962 + k), all hidden (`sub_99220`), then `HandleChatMessageLines(0)`.
//! * Refresh (`ChatCallback` 0x499760 on 0x267 / 0x268 / 0x279 / 0x27a): the list is rebuilt from
//!   the event's (text, icon) pairs and shown from the newest (`HandleChatMessageLines(count −
//!   1)`).
//! * `HandleChatMessageLines(last)` (0x499010): every slot hidden; `last` clamped to the list;
//!   from slot 3 upwards, message `last`, `last − 1`, … each take the lines 0x499380 wraps them
//!   into (see [`wrap`]), the newest at the bottom.
//! * Lines 958..=961 (kind 19), code 3: event 0x1771 with the slot's message index (the
//!   handler's click 0x52a800). Codes 5 / 6 (highlight on / off): every slot showing the same
//!   message gets font 10 / 9 (`sub_99CC0`). Code 10 (wheel): all four back to font 9, then up
//!   (positive delta) or down one message.
//! * 966 / 967: `HandleChatMessageLines(last ∓ 1)`. 968: event 0x26a (window closed, button back).

/// The button dialog and its control.
pub const BUTTON: usize = 59;
pub const BUTTON_LINK: u16 = 891;
/// The four line slots (text) and their icons, top to bottom.
pub const LINES: [u16; 4] = [958, 959, 960, 961];
pub const ICONS: [u16; 4] = [962, 963, 964, 965];
pub const UP: u16 = 966;
pub const DOWN: u16 = 967;
pub const CLOSE: u16 = 968;
pub const CHAT_INPUT: u16 = 969;
/// The button's images: plain and lit (the blink).
pub const IMAGES: (u16, u16) = (0x148, 0x149);
pub const IMAGES_LIT: (u16, u16) = (0x14a, 0x14b);
/// Fonts of a line: normal and highlighted (`SetTextStyle` 9 / 10).
pub const FONT: u8 = 9;
pub const FONT_HIGHLIGHT: u8 = 10;

/// `SelectChatSize` (0x489900): the window's dialog for an output `width` (panel px).
pub fn dialog_for(width: u32, extended_tooltips: bool) -> usize {
    let narrow = extended_tooltips as usize;
    if width > 0x4ff {
        65 - narrow
    } else if width < 0x400 {
        61 - narrow
    } else {
        63 - narrow
    }
}

/// `SelectChatSize` in a tutorial (`CGuiEventHandler +0x40`): the tutorial window
/// (`ChatEvents2` 0x4ad610) for an output `width`. The full-width variants 67 / 69 / 71 (extended
/// tooltips off) have no controls in the shipped GUI sets, so no text shows there.
pub fn tutorial_dialog_for(width: u32, extended_tooltips: bool) -> usize {
    let narrow = extended_tooltips as usize;
    if width > 0x4ff {
        71 - narrow
    } else if width < 0x400 {
        67 - narrow
    } else {
        69 - narrow
    }
}

/// The tutorial window's two lines (`ChatEvents2`): clicking 1711 sends UI event 0x276, 1712
/// 0x275 (`CTutorial::OnEvent`: the script's COMMAND 0x6af / 0x6b0).
pub const TUTORIAL_LINES: [u16; 2] = [0x6af, 0x6b0];

/// One slot as `HandleChatMessageLines` leaves it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Slot {
    /// The line shown, `None` = hidden.
    pub text: Option<String>,
    /// The icon (the message's `ChatMessageType`), shown on a message's first line only.
    pub icon: Option<u16>,
    /// The message index (`ChatMessageIndexLine`, +0xc of the slot record); kept from an earlier
    /// fill while the slot is hidden.
    pub message: i32,
}

/// The window's lines: `MessageCount` / `MessageStrings` / `ChatMessageType`, the slots,
/// `LastDisplayedMessageIndex` and `MaxVisibleChatMessages` (`sub_99220`: 4, or 3 while the chat
/// input takes the bottom row).
#[derive(Clone, Debug)]
pub struct Lines {
    pub messages: Vec<(String, u16)>,
    pub slots: [Slot; 4],
    pub last: i32,
    pub visible: usize,
}

impl Default for Lines {
    fn default() -> Self {
        Lines { messages: Vec::new(), slots: Default::default(), last: 0, visible: 4 }
    }
}

/// 0x499380: wrap message `i` into at most four lines and show them in `slot`, `slot − 1`, …
/// (its last line in `slot`). `fit(text)` is `sub_277970` on the slot's control: the index of
/// the last character that fits in its width − 4, or −1 when all of it fits. A line ends after
/// the last space before character `fit − 1`, or there without a space. Returns the slot above.
fn wrap(lines: &mut Lines, i: usize, slot: i32, fit: &dyn Fn(&str) -> i32) -> i32 {
    // `MessageStrings` entries are 500 bytes.
    let mut buf: Vec<char> = lines.messages[i].0.chars().take(499).collect();
    let mut out: Vec<String> = Vec::new();
    loop {
        let r = fit(&buf.iter().collect::<String>());
        let k = if r <= 0 {
            out.push(buf.iter().collect());
            r
        } else {
            let k = r - 1;
            // _mbsninc, then back to a space (_mbsnbcmp / _mbsdec).
            let start = (k as usize).min(buf.len());
            let mut p = start;
            while p > 0 && buf.get(p) != Some(&' ') {
                p -= 1;
            }
            let p = if buf.get(p) == Some(&' ') { p + 1 } else { start };
            out.push(buf[..p].iter().collect());
            buf.drain(..p);
            k
        };
        if k < 0 || out.len() >= 4 {
            break;
        }
    }
    let n = out.len();
    let mut slot = slot;
    for j in 0..n {
        let s = &mut lines.slots[slot as usize];
        let line = n - 1 - j;
        s.text = Some(out[line].clone());
        s.icon = (line == 0).then_some(lines.messages[i].1);
        s.message = i as i32;
        slot -= 1;
        if slot < 0 {
            break;
        }
    }
    slot
}

/// `HandleChatMessageLines(last)` (0x499010).
pub fn show(lines: &mut Lines, last: i32, fit: &dyn Fn(&str) -> i32) {
    for s in &mut lines.slots {
        s.text = None;
        s.icon = None;
    }
    let count = lines.messages.len() as i32;
    if count == 0 {
        return;
    }
    let last = if last < count { last.max(0) } else { count - 1 };
    lines.last = last;
    let mut slot = lines.visible.clamp(1, 4) as i32 - 1;
    let mut i = last;
    while slot >= 0 && i >= 0 {
        slot = wrap(lines, i as usize, slot, fit);
        i -= 1;
    }
}

/// `sub_277970` over a text measure: `width(text)` in px, `w` the control's width.
pub fn fit_in(text: &str, w: i32, width: &dyn Fn(&str) -> i32) -> i32 {
    if width(text) <= w - 4 {
        return -1;
    }
    let chars: Vec<char> = text.chars().collect();
    let upto = |n: usize| width(&chars[..(n + 1).min(chars.len())].iter().collect::<String>());
    let mut n = 0usize;
    let mut budget = 0xfb_i32;
    if upto(0) <= w - 4 {
        loop {
            budget -= 1;
            if budget < 0 {
                break;
            }
            n += 1;
            if upto(n) > w - 4 {
                break;
            }
        }
        if budget < 1 {
            return -1;
        }
    }
    n as i32 - 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialog_by_width_and_tooltips() {
        assert_eq!((dialog_for(800, false), dialog_for(800, true)), (61, 60));
        assert_eq!((dialog_for(1024, false), dialog_for(1279, true)), (63, 62));
        assert_eq!((dialog_for(1280, false), dialog_for(1600, true)), (65, 64));
    }
}
