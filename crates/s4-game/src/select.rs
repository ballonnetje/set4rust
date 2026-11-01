//! Unit selection on the map (s4_viewer::selection): left click selects (`DotSelection`), a
//! left drag selects in a box (`PrepareBoxSelection` … `BoxSelection`), a right click sends the
//! selection (`SendTo`, emitted as a typed event only until the sim has military commands).

use s4_viewer::client;
use s4_viewer::gpu::{Gpu, View};
use s4_viewer::guistate::UiEvent;
use s4_viewer::population::Population;
use s4_viewer::scene::Scene;
use s4_viewer::selection::Selection;

/// `CStateGame::EventManagement` (release 0x5172f0), the left button on the map:
/// * down (case 7): the press point is kept and the box cleared;
/// * move with the button down: once the cursor is more than 4 px away from the press point on
///   either axis, the box (press point to cursor, the cursor's x at least 210, the GUI panel's
///   width, `this +0xe4`) is kept (0x160702c..) and drawn (`sub_25A6D0`) in white
///   (`GetColorFromRGB(0xff, 0xff, 0xff)`), the drawing skipped while the cursor shape (0x65e650
///   reads `SetCursorShape`'s 0x11691dc) is 3 (build), 4 (hourglass), 6 (trade target) or 7
///   (soldiers with Shift) — the box is still kept and selects —, and event 0x1df (the box
///   preview) is sent;
/// * up (case 8): within 4 px on both axes (`< 5`) it is a click (event 0x1dd at the press
///   point's map node); else the box is cleared and, if it was ever set, event 0x1de goes out with
///   the release point (x at least 210) and the press point, the sign bit set for `MK_CONTROL`
///   (strict box selection).
const DRAG_MIN: f64 = 5.0;
/// `this +0xe4` = 0xd2: the GUI panel's width, the left edge of the map view.
const MAP_LEFT: f64 = 210.0;

#[derive(Default)]
pub struct Pointer {
    /// The local player (whose units can be selected).
    pub player: u8,
    pub selection: Selection,
    /// Where the left button went down on the map.
    drag: Option<(f64, f64)>,
    /// The box shown while dragging (screen px): press point and cursor.
    pub rect: Option<(f64, f64, f64, f64)>,
    /// Modifier key Ctrl (`MK_CONTROL`, `wParam & 8`): appends to a click selection
    /// (`DotSelection`) and makes a box strict (`BoxSelection`, the sign bit of its wParam).
    pub ctrl: bool,
    /// Shift (`MK_SHIFT`, `& 4`): `SendTo`'s mode bit 1; a click selects the same type around
    /// (event 0x1aa).
    pub shift: bool,
    /// Alt (key bit 0x10): `SendTo`'s mode bit 2 (the forced order); a click selects the same
    /// type in the wide range (event 0x1aa with 0x10).
    pub alt: bool,
    /// GUI sounds of the last clicks, for `GameSound::ui` (taken by the caller).
    pub sounds: Vec<u16>,
    /// The cursor shape shown (cursor.rs; the caller keeps it current): 3, 4, 6 and 7 do not
    /// draw the box.
    pub cursor_shape: u8,
}

impl Pointer {
    pub fn new(player: u8) -> Pointer {
        Pointer { player, ..Default::default() }
    }

    /// The pending map press is dropped (both buttons went down: camera zoom).
    pub fn cancel(&mut self, gpu: &mut Gpu) {
        self.drag = None;
        self.set_rect(gpu, None);
    }

    /// Left button down on the map (not on the GUI, not in build mode).
    pub fn press(&mut self, gpu: &mut Gpu, p: (f64, f64)) {
        self.drag = Some(p);
        self.set_rect(gpu, None);
    }

    /// The cursor moved to `p` (screen px; `scale` = GUI px per panel px): with the left button
    /// down on the map and more than 4 px away on either axis, the box follows the cursor.
    pub fn moved(&mut self, gpu: &mut Gpu, p: (f64, f64), scale: f64) {
        let Some(a) = self.drag else { return };
        if (p.0 - a.0).abs() >= DRAG_MIN || (p.1 - a.1).abs() >= DRAG_MIN {
            let x = p.0.max(MAP_LEFT * scale);
            self.set_rect(gpu, Some((a.0, a.1, x, p.1)));
        }
    }

    fn set_rect(&mut self, gpu: &mut Gpu, rect: Option<(f64, f64, f64, f64)>) {
        if rect != self.rect {
            self.rect = rect;
            // `sub_25A6D0(0, 0)` always clears; a box is drawn only with another cursor shape (the
            // last one drawn stays up meanwhile).
            if rect.is_none() || !matches!(self.cursor_shape, 3 | 4 | 6 | 7) {
                client::selection_box(gpu, rect.map(|(a, b, c, d)| (a as i32, b as i32, c as i32, d as i32)));
            }
        }
    }

    /// Left button up: a click or a box, if the press was on the map (the box only if it was
    /// shown).
    pub fn release(&mut self, gpu: &mut Gpu, scene: &mut Scene, view: View, pop: &Population, p: (f64, f64), scale: f64) -> Vec<UiEvent> {
        let Some(a) = self.drag.take() else { return Vec::new() };
        let shown = self.rect.is_some();
        self.set_rect(gpu, None);
        let x = p.0.max(MAP_LEFT * scale);
        let click = (x - a.0).abs() < DRAG_MIN && (p.1 - a.1).abs() < DRAG_MIN;
        let ev = if click && (self.shift || self.alt) {
            // 0x4e9ac0: 0x1dd with key bits & 0x14 (Shift, Alt) is event 0x1aa.
            client::select_same_type(gpu, scene, view, pop, &mut self.selection, self.player, a, self.alt)
        } else if click {
            // Event 0x1dd at the press point's node.
            let ev = client::select_at(gpu, scene, view, pop, &mut self.selection, self.player, a, self.ctrl);
            if building_clicked(pop, &self.selection, &ev) {
                self.sounds.push(SOUND_BUILDING_SELECTED);
            }
            ev
        } else if shown {
            client::select_box(gpu, scene, view, pop, &mut self.selection, self.player, a, (x, p.1), self.ctrl)
        } else {
            Vec::new()
        };
        log(&ev);
        ev
    }

    /// `SendTo`'s mode (0x4ec960: `((k >> 1 & 4 | k & 0x10) >> 1 | k & 4) >> 1` of the key bits
    /// Shift 4, Ctrl 8, Alt 0x10): bit 0 Ctrl, bit 1 Shift, bit 2 Alt (the forced order).
    pub fn send_mode(&self) -> u8 {
        self.ctrl as u8 | (self.shift as u8) << 1 | (self.alt as u8) << 2
    }

    /// Right click on map node `node` with a selection: `CInputProcessor::SendTo` (0x4ec960).
    /// When the first selected entity is a building (obj type 8) or has flag 0x4000 now (the
    /// entity's `+0x14`), nothing is sent: the selection is cleared (0x4edc90). Otherwise, after
    /// the command, `PlaySound(10, 100, ..)` (`SOUND_GUI_SUBBUTTONDOWN`, 0x4ecacd).
    pub fn send(&mut self, gpu: &mut Gpu, pop: &Population, node: (usize, usize)) -> Vec<UiEvent> {
        let Some(&first) = self.selection.ids.first() else { return Vec::new() };
        let building = pop.buildings.iter().any(|b| b.id == first);
        let other = pop.settlers.iter().chain(&pop.vehicles).any(|u| u.id == first && u.flags & 0x4000 != 0);
        if building || other {
            let ev = client::clear_selection(gpu, pop, &mut self.selection);
            log(&ev);
            return ev;
        }
        let ev = vec![UiEvent::SendTo { ids: self.selection.ids.clone(), x: node.0 as u16, y: node.1 as u16, mode: self.send_mode() }];
        self.sounds.push(SOUND_SENT);
        log(&ev);
        ev
    }
}

/// `DotSelection` (0x4eb3b0) plays `PlaySound(7, 100, ..)` when the clicked entity is a building
/// (obj type 8) it selects (flag 0x200; also when it was selected already), before
/// `RequestDialog`; not for an entity with flag 0x4000 (it gets 0x400 instead), a Ctrl click that
/// deselects, box selections or the next-building button.
const SOUND_BUILDING_SELECTED: u16 = 7;
/// `SendTo` (0x4ec960) plays `PlaySound(10, 100, ..)` once the command is made (sent only with a
/// network engine, the sound always).
const SOUND_SENT: u16 = 10;

/// A click that selected a building: a building is never mixed with other entities, so the
/// selection is that one building ([`client::select_at`] reported it).
fn building_clicked(pop: &Population, sel: &Selection, ev: &[UiEvent]) -> bool {
    let selected = ev.iter().any(|e| matches!(e, UiEvent::Selection(ids) if !ids.is_empty()));
    selected && sel.ids.len() == 1 && sel.other.is_empty() && pop.buildings.iter().any(|b| b.id == sel.ids[0])
}

fn log(ev: &[UiEvent]) {
    for e in ev {
        tracing::info!("selection: {e:?}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 0x4ec960's mode from the key bits k (Shift 4, Ctrl 8, Alt 0x10) for every combination:
    /// Alt sets bit 2.
    #[test]
    fn send_mode_bits() {
        for k in 0u32..32 {
            let p = Pointer { shift: k & 4 != 0, ctrl: k & 8 != 0, alt: k & 0x10 != 0, ..Default::default() };
            let release = ((k >> 1 & 4 | k & 0x10) >> 1 | k & 4) >> 1;
            assert_eq!(p.send_mode() as u32, release, "keys {k:#x}");
        }
        assert_eq!(Pointer { alt: true, ..Default::default() }.send_mode(), 4);
    }
}
