//! The mouse cursor's shape, as the release picks it. The shapes are the 36 cursors of
//! `S4_Main.exe` (`s4_formats::cursor`, `CursorList`); `IGfxEngine::SetCursorShape` (0x65e5b0)
//! selects one by index.
//!
//! * The game's start screens: the hourglass (4) while a state loads, the arrow (0) once it
//!   runs (e.g. `CStateGame` 0x513190: 4 at 0x51330f / 0x51343e).
//! * Entering a mode sets its shape: `TryBuild` (0x4ea250, the build mode) 3, the trade-target
//!   mode (0x4eaf60) 6; leaving a mode (`sub_EDC10` 0x4edc10, the input processor's "leave"
//!   handler) the arrow.
//! * Pause (0x514be0): 0x1f while paused, the previous shape again after.
//! * Every logic tick (`GameLogicTick` 0x4f2720 → 0x4f2900, also after selection changes):
//!   [`choose`].
//!
//! [inf] The modifier keys: the release polls them (`GetAsyncKeyState`) at the tick; here they
//! are the window's modifier state at that moment.

/// `CursorList` indices.
pub mod shape {
    pub const ARROW: u8 = 0;
    /// A right-button drag (0x518186).
    pub const DRAG: u8 = 1;
    pub const BUILD: u8 = 3;
    pub const WAIT: u8 = 4;
    pub const TRADE_TARGET: u8 = 6;
    pub const ADD: u8 = 7;
    pub const WORK_AREA: u8 = 9;
    pub const SEND: u8 = 0xa;
    pub const SOLDIERS: u8 = 0x11;
    pub const SOLDIERS_CTRL: u8 = 0x13;
    pub const PAUSE: u8 = 0x1f;
    pub const VEHICLE: u8 = 0x20;
    pub const TARGET: u8 = 0x22;
}

/// The keys 0x4f2900 polls: Alt (VK_MENU) 0x10, Shift 4, Ctrl 8; Alt with Ctrl counts as Alt.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    pub alt: bool,
    pub shift: bool,
    pub ctrl: bool,
}

impl Mods {
    fn bits(self) -> u8 {
        let b = (self.alt as u8) << 4 | (self.shift as u8) << 2 | (self.ctrl as u8) << 3;
        if b == 0x18 { 0x10 } else { b }
    }
}

/// The first selected entity as 0x4f2900 reads it: its object type (+0xa), its type (+0xc,
/// the settler's job) and its flags (+0x14).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selected {
    pub obj: u8,
    pub kind: u16,
    pub flags: u32,
}

/// 0x4f2900: the shape for the input processor's `state` (input.rs), the first selected entity
/// and the keys, given the `current` shape; `None` = keep it. `held` = the right-button drag or
/// pause hides the decision (+0xd2 / +0x2e).
pub fn choose(current: u8, state: u8, held: bool, selected: Option<Selected>, mods: Mods) -> Option<u8> {
    if held || matches!(state, 4..=6) {
        return None;
    }
    let want = match state {
        3 => shape::WORK_AREA,
        9 => 0x13,
        10 => shape::TARGET,
        _ => match selected {
            None => {
                if matches!(current, 6 | 7) {
                    return None;
                }
                shape::ARROW
            }
            Some(s) => {
                let dl = mods.bits();
                let alt = dl & 0x10 != 0;
                if s.obj == 1 {
                    match s.kind {
                        0x2f => if alt { 0xa } else { 0x15 },
                        0x31 => if alt { 0xa } else { 0xd },
                        0x30 => if alt { 0xa } else { 0x1b },
                        0x2d => if alt { 0xa } else { 0xf },
                        0x2e => if alt { 0xa } else { 0x18 },
                        0x32 => 0xa | (!alt) as u8,
                        0x3c | 1 => shape::ARROW,
                        _ if alt => shape::SEND,
                        _ if dl & 4 != 0 => shape::ADD,
                        // `((dl & 8) | 0x44) >> 2`.
                        _ if dl & 8 != 0 => shape::SOLDIERS_CTRL,
                        _ => shape::SOLDIERS,
                    }
                } else {
                    // 0x436a20: a land vehicle (4) or a ship (2); anything else keeps 0.
                    let mut s2 = 0;
                    if matches!(s.obj, 2 | 4) {
                        if s.flags & 0x4000 != 0 {
                            return None;
                        }
                        s2 = if matches!(s.flags & 0xf, 8 | 0xa | 0xb) { shape::SOLDIERS } else { shape::VEHICLE };
                        if alt {
                            s2 = shape::SEND;
                        }
                    }
                    if matches!(current, 6 | 7) {
                        return None;
                    }
                    s2
                }
            }
        },
    };
    (want != current).then_some(want)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_by_selection_and_keys() {
        let none = Mods::default();
        assert_eq!(choose(5, 0, false, None, none), Some(shape::ARROW));
        assert_eq!(choose(6, 0, false, None, none), None, "6 and 7 stay without a selection");
        let soldier = Selected { obj: 1, kind: 0x1d, flags: 1 };
        assert_eq!(choose(0, 1, false, Some(soldier), none), Some(shape::SOLDIERS));
        assert_eq!(choose(0, 1, false, Some(soldier), Mods { ctrl: true, ..none }), Some(shape::SOLDIERS_CTRL));
        assert_eq!(choose(0, 1, false, Some(soldier), Mods { shift: true, ..none }), Some(shape::ADD));
        assert_eq!(choose(0, 1, false, Some(soldier), Mods { alt: true, ctrl: true, ..none }), Some(shape::SEND), "Alt+Ctrl is Alt");
        let pioneer = Selected { obj: 1, kind: 0x2f, flags: 0 };
        assert_eq!(choose(0, 1, false, Some(pioneer), none), Some(0x15));
        assert_eq!(choose(0, 1, false, Some(Selected { obj: 1, kind: 0x32, flags: 0 }), none), Some(0xb));
        assert_eq!(choose(0, 3, false, None, none), Some(shape::WORK_AREA));
        assert_eq!(choose(3, 4, false, None, none), None, "the build mode keeps TryBuild's shape");
        let cart = Selected { obj: 4, kind: 1, flags: 0 };
        assert_eq!(choose(0, 1, false, Some(cart), none), Some(shape::VEHICLE));
        let building = Selected { obj: 8, kind: 1, flags: 0 };
        assert_eq!(choose(0x11, 1, false, Some(building), none), Some(shape::ARROW));
        assert_eq!(choose(0, 1, true, Some(soldier), none), None);
    }
}
