//! Sliders of the GUI engine: a slider area (kind 16) followed by its knob (kind 7, or kind 8
//! for linked knobs whose values add up to 100), in the release event procedure 0x672f90 and its
//! helpers. Evidence: `tools/re/viewer-2026-10-20/goods/guieng.c`, `slider.c`, `hit.c`.
//!
//! * Hit test (0x6747c0): an area whose next control is a knob gives the knob when the point is on
//!   the knob (the knob's effects are not checked), else the area.
//! * The knob's value is `SGuiControl +0x1e` (a signed byte), its x the record's own x: the engine
//!   moves the control. Linked knobs keep the value saved at the last knob press at +0x21.
//! * Range: from `dialog x + area x` to `dialog x + area x + area w − knob w`; value
//!   `(int)(100 / (max − min) · (pos − min) + 0.5)` (0x675b60, 0 for an empty range); position of
//!   a value `(int)((area w − knob w) · 0.01 · v + 0.5) + area x` (0x6772d0, 0x6752e0).
//! * Left down on an area (event 7): the knob's left edge jumps to the cursor (clamped), code 2
//!   and code 3 with the value. On a knob: linked knobs save their values (0x675280) and the knob
//!   becomes the drag control. Moving while dragging (event 5) moves it the same way (code 2);
//!   the release (event 8) sends code 3.
//! * A linked knob's move (0x6752e0, 0x6753b0, 0x675540): the other free knobs of its group (kind
//!   8, effects & 0xd clear) get their saved values back, then take `saved − new` in steps of one,
//!   in control order, pass after pass while one changes (each step code 2), and the knob itself
//!   takes what still keeps the free knobs (effects & 0xc clear) at 100 (code 2).
//!
//! Faithful quirks: a jump compares the knob's dialog-relative x with the absolute position, and
//! the release value uses the relative x against the absolute range (both differ only for a
//! dialog not at x 0). Not ported: the key flags (0x675ba0) the engine adds to the release value.
//! * Right button down (event 9; `CStateGame::EventManagement` 0x5172f0 case 9 is the right press)
//!   on a linked knob, or on the area of one, under the cursor and not dragged: the knob is locked
//!   (pressed: left alone by the group's balancing) when more than two of its group would stay free
//!   (0x675640: kind-8 knobs of the group minus those with effects & 0xd), else unlocked. Evidence:
//!   `tools/re/gui-2026-10-20/guiproc.c` (case 9), `slider_group.c`.

use super::{GuiState, UiEvent};

impl GuiState {
    /// The knob of a slider area (the next control, kind 7 or 8).
    pub(super) fn knob_of(&self, slot: usize, area: usize) -> Option<usize> {
        let d = self.dialog(slot)?;
        (d.controls.get(area)?.kind == 16 && matches!(d.controls.get(area + 1)?.kind, 7 | 8)).then_some(area + 1)
    }

    /// A knob's (min, max) absolute x; `None` without the area before it ("GUI ENGINE: Illegal data.
    /// Can't find slider area.").
    fn knob_range(&self, key: (usize, usize)) -> Option<(i32, i32)> {
        let d = self.dialog(key.0)?;
        let area = d.controls.get(key.1.checked_sub(1)?)?;
        let knob = d.controls.get(key.1)?;
        if area.kind != 16 {
            return None;
        }
        let min = d.x as i32 + area.x as i32;
        Some((min, min + area.width as i32 - knob.width as i32))
    }

    /// The value of knob `key` (+0x1e).
    pub fn slider_value(&self, key: (usize, usize)) -> i8 {
        self.sliders.get(&key).map_or(0, |s| s.0)
    }

    /// A knob's value by link (`None` unless it is a kind 7/8 control).
    pub fn knob_value(&self, slot: usize, link: u16) -> Option<i8> {
        let key = self.find_link(slot, link)?;
        matches!(self.control(key)?.kind, 7 | 8).then(|| self.slider_value(key))
    }

    fn set_value(&mut self, key: (usize, usize), v: i8) {
        self.sliders.entry(key).or_default().0 = v;
    }

    /// Value `v` and the knob's x for it (0x6772d0's and 0x6752e0's position formula).
    fn place(&mut self, key: (usize, usize), v: i8) {
        self.set_value(key, v);
        let Some(d) = self.set.dialogs.get_mut(key.0).and_then(|d| d.as_mut()) else { return };
        let (Some(area), Some(knob)) = (d.controls.get(key.1.wrapping_sub(1)).copied(), d.controls.get(key.1).copied()) else { return };
        if area.kind != 16 {
            return;
        }
        let span = area.width as i32 - knob.width as i32;
        let x = (span as f32 * 0.01f32 * v as f32 + 0.5) as i32 + area.x as i32;
        d.controls[key.1].x = x as u16;
    }

    /// Code 2 to the dialog's procedure; `ProductionOrderMenuCallback` (dialog 26) prints the
    /// values (code 2 → `PrintDistributionValues`).
    fn slider_event(&mut self, key: (usize, usize), value: i8, ev: &mut Vec<UiEvent>) {
        if let Some(c) = self.control(key) {
            ev.push(UiEvent::Slider { dialog: key.0, link: c.link, value: value as i32 });
        }
        match key.0 {
            26 => self.print_distribution(),
            // The weapon smith's sidebar (0x4af090 code 2, outside its init): 0x4aea40.
            46 => {
                self.weapon_percentages();
            }
            _ => {}
        }
    }

    /// The other kind-8 knobs of `key`'s group whose effects have none of `mask`.
    fn linked(&self, key: (usize, usize), mask: u8) -> Vec<usize> {
        let Some(d) = self.dialog(key.0) else { return Vec::new() };
        let group = d.controls[key.1].group;
        (0..d.controls.len())
            .filter(|&j| {
                let c = &d.controls[j];
                let fx = self.effects.get(&(key.0, j)).copied().unwrap_or(c.effects);
                c.kind == 8 && c.group == group && j != key.1 && fx & mask == 0
            })
            .collect()
    }

    /// 0x675280: every free knob of the group (`key` included) saves its value.
    fn save_group(&mut self, key: (usize, usize)) {
        let mut all = self.linked(key, 0xc);
        let fx = self.effects.get(&key).copied().unwrap_or(0);
        if fx & 0xc == 0 {
            all.push(key.1);
        }
        for j in all {
            let s = self.sliders.entry((key.0, j)).or_default();
            s.1 = s.0;
        }
    }

    /// 0x6752e0, 0x6753b0, 0x675540 after knob `key` got `value`.
    fn spread(&mut self, key: (usize, usize), value: i8, ev: &mut Vec<UiEvent>) {
        let others = self.linked(key, 0xd);
        for &j in &others {
            let saved = self.sliders.get(&(key.0, j)).map_or(0, |s| s.1);
            self.place((key.0, j), saved);
        }
        let saved = self.sliders.get(&key).map_or(0, |s| s.1);
        let mut rest = saved as i32 - value as i32;
        let step = if rest < 0 { -1 } else { 1 };
        'passes: while rest != 0 {
            let mut changed = false;
            for &j in &others {
                if rest == 0 {
                    break 'passes;
                }
                let v = self.slider_value((key.0, j)) as i32 + step;
                if (0..=100).contains(&v) {
                    rest -= step;
                    self.place((key.0, j), v as i8);
                    self.slider_event((key.0, j), v as i8, ev);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        let sum: i32 = self.linked(key, 0xc).iter().map(|&j| self.slider_value((key.0, j)) as i32).sum();
        let own = self.slider_value(key);
        let r = 100 - own as i32 - sum;
        if r != 0 {
            let v = own.wrapping_add(r as i8).clamp(0, 100);
            self.place(key, v);
            self.slider_event(key, v, ev);
        }
    }

    /// A knob moved to absolute x `pos` (clamped first); `Some(value)` if it moved.
    fn move_knob(&mut self, key: (usize, usize), pos: i32, ev: &mut Vec<UiEvent>) -> Option<i8> {
        let (min, max) = self.knob_range(key)?;
        let pos = pos.max(min).min(max);
        let dx = self.dialog(key.0)?.x as i32;
        if self.control(key)?.x as i32 == pos {
            return None;
        }
        let v = if max == min { 0 } else { (100.0f32 / (max - min) as f32 * (pos - min) as f32 + 0.5) as i32 } as i8;
        self.set_value(key, v);
        if let Some(d) = self.set.dialogs.get_mut(key.0).and_then(|d| d.as_mut()) {
            d.controls[key.1].x = (pos - dx) as u16;
        }
        if self.control(key)?.kind == 8 {
            self.spread(key, v, ev);
        }
        Some(self.slider_value(key))
    }

    /// Event 7 on a slider area `area` at absolute x `x`: the knob jumps there (code 2, code 3).
    pub(super) fn area_down(&mut self, area: (usize, usize), x: i32, ev: &mut Vec<UiEvent>) {
        let Some(k) = self.knob_of(area.0, area.1) else { return };
        let key = (area.0, k);
        if let Some(v) = self.move_knob(key, x, ev) {
            self.slider_event(key, v, ev);
            self.command(key, ev);
        }
    }

    /// Event 9 on control `hit` (a knob, or an area whose knob it is): a kind-8 knob is locked when
    /// 0x675640 allows it, or unlocked.
    pub(super) fn knob_lock(&mut self, hit: (usize, usize)) {
        let key = match self.knob_of(hit.0, hit.1) {
            Some(k) => (hit.0, k),
            None if self.control(hit).is_some_and(|c| c.kind == 16) => return,
            None => hit,
        };
        if self.control(key).is_none_or(|c| c.kind != 8) {
            return;
        }
        if self.effects.get(&key).is_some_and(|fx| fx & 1 != 0) {
            self.set_effect(key, 1, false);
            return;
        }
        // 0x675640: more than two of the group's kind-8 knobs (`key` included, free here) are not
        // pressed, hidden or greyed.
        if self.linked(key, 0xd).len() + 1 > 2 {
            self.set_effect(key, 1, true);
        }
    }

    /// Event 7 on knob `key`: linked knobs save their values, the knob is released and dragged.
    pub(super) fn knob_down(&mut self, key: (usize, usize)) {
        if self.drag == Some(key) {
            return;
        }
        if self.control(key).is_some_and(|c| c.kind == 8) {
            self.save_group(key);
            self.set_effect(key, 1, false);
        }
        self.drag = Some(key);
    }

    /// Event 5 while knob `key` is dragged.
    pub(super) fn knob_drag(&mut self, key: (usize, usize), x: i32, ev: &mut Vec<UiEvent>) {
        if let Some(v) = self.move_knob(key, x, ev) {
            self.slider_event(key, v, ev);
        }
    }

    /// Event 8 while knob `key` is dragged: code 3 with the value of its x.
    pub(super) fn knob_up(&mut self, key: (usize, usize), ev: &mut Vec<UiEvent>) {
        let Some((min, max)) = self.knob_range(key) else { return };
        let Some(c) = self.control(key) else { return };
        let pos = c.x as i32;
        let v = if max == min { 0 } else { (100.0f32 / (max - min) as f32 * (pos - min) as f32 + 0.5) as i32 };
        tracing::debug!("slider release {:?}: {v}", c.link);
        self.command(key, ev);
    }

    /// 0x4aea40: the weapon smith sidebar's knobs 0x6f7, 0x6f8, 0x6f9, 0x6fb (no knob: -1 → 0) as
    /// "n%" texts 0x6ff..=0x702; returns the four values (0x15aad28..0x15aad34).
    pub fn weapon_percentages(&mut self) -> [u8; 4] {
        let mut out = [0u8; 4];
        for (k, (knob, text)) in [(0x6f7, 0x6ff), (0x6f8, 0x700), (0x6f9, 0x701), (0x6fb, 0x702)].into_iter().enumerate() {
            let v = self.knob_value(46, knob).map_or(-1, |v| v as i32).max(0);
            self.texts.insert((46, text), format!("{v}%"));
            out[k] = v as u8;
        }
        out
    }

    /// `IGuiEngine::SetSliderPosition(dialog, link, v)` (0x6772d0) for knobs (kinds 7 / 8; bars
    /// are [`GuiState::bars`]): nothing above 100, nor for a kind-8 knob while a knob of its group
    /// is dragged, nor for the dragged kind-7 knob; a pressed knob is released; a new value moves it.
    pub fn set_slider(&mut self, slot: usize, link: u16, v: u32) {
        let Some(key) = self.find_link(slot, link) else { return };
        let Some(c) = self.control(key).copied() else { return };
        if v > 100 {
            return;
        }
        if c.kind == 8
            && let Some(d) = self.drag
            && self.control(d).is_some_and(|dc| dc.group == c.group)
        {
            return;
        }
        self.set_effect(key, 1, false);
        if c.kind == 7 && self.drag == Some(key) {
            return;
        }
        if self.slider_value(key) as i32 != v as i32 && matches!(c.kind, 7 | 8) {
            self.place(key, v as i8);
        } else if !matches!(c.kind, 7 | 8) {
            self.set_value(key, v as i8);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;

    /// Roman GUI set with the goods distribution page (26) open and its five knobs at 20.
    fn page26() -> Option<GuiState> {
        let set = GuiSet::open(s4_formats::game_dir().join("Menu/GUISetRomans.dat")).ok()?;
        let mut s = GuiState::new(set);
        let mut ev = Vec::new();
        s.open(26, &mut ev);
        for l in 795..=799 {
            s.set_slider(26, l, 20);
        }
        Some(s)
    }

    fn values(s: &GuiState) -> Vec<i8> {
        (795..=799).map(|l| s.knob_value(26, l).unwrap()).collect()
    }

    fn knob_point(s: &GuiState, link: u16) -> (i32, i32) {
        let d = s.set.dialog(26).unwrap();
        let c = d.controls.iter().find(|c| c.link == link).unwrap();
        ((d.x + c.x + 2) as i32, (d.y + c.y + c.height / 2) as i32)
    }

    /// SetSliderPosition places the knob at `(int)((area w − knob w) · 0.01 · v + 0.5) + area x`.
    #[test]
    fn set_slider_places_the_knob() {
        let Some(s) = page26() else { return };
        let d = s.set.dialog(26).unwrap();
        let x = d.controls.iter().find(|c| c.link == 795).unwrap().x;
        // Area 790 at x 80, 107 wide; knob 15 wide: 92 · 0.2 + 0.5 → 18.
        assert_eq!(x, 80 + 18);
    }

    /// Dragging a linked knob: the others give up the difference one point at a time, cyclically,
    /// and the release sends code 3 for the knob.
    #[test]
    fn linked_knob_drag_keeps_100() {
        let Some(mut s) = page26() else { return };
        let (x, y) = knob_point(&s, 795);
        s.left_down(x, y, true);
        // To 80 + 46: (int)(100 / 92 · 46 + 0.5) = 50.
        let ev = s.mouse_move(80 + 46, y);
        assert_eq!(values(&s), vec![50, 12, 12, 13, 13]);
        assert!(ev.contains(&UiEvent::Slider { dialog: 26, link: 795, value: 50 }));
        let ev = s.left_up(80 + 46, y);
        assert!(ev.iter().any(|e| matches!(e, UiEvent::Command { dialog: 26, link: 795, .. })));
        assert_eq!(values(&s).iter().map(|&v| v as i32).sum::<i32>(), 100);
    }

    /// A right press locks a linked knob while more than two of the group (itself included) are
    /// free (0x675640), a second one unlocks it; a locked knob is left alone by a drag of another.
    #[test]
    fn right_press_locks_a_linked_knob() {
        let Some(mut s) = page26() else { return };
        let fx = |s: &GuiState, l: u16| s.effects[&s.find_link(26, l).unwrap()] & 1;
        for l in [795, 796, 797, 798] {
            let (x, y) = knob_point(&s, l);
            s.right_down(x, y, true);
        }
        // Five free: 795, 796, 797 lock (3 > 2 free counting the knob itself); then only two
        // are, so 798 may not.
        assert_eq!((fx(&s, 795), fx(&s, 796), fx(&s, 797), fx(&s, 798)), (1, 1, 1, 0));
        let (x, y) = knob_point(&s, 799);
        s.left_down(x, y, true);
        s.mouse_move(80 + 46, y);
        let v = values(&s);
        assert_eq!((v[0], v[1], v[2]), (20, 20, 20));
        assert_eq!(v.iter().map(|&v| v as i32).sum::<i32>(), 100);
        s.left_up(80 + 46, y);
        let (x, y) = knob_point(&s, 795);
        s.right_down(x, y, true);
        assert_eq!(fx(&s, 795), 0);
    }

    /// A click on an area moves its knob there; the others get the values saved at the last knob
    /// press (none yet: 0), so this one takes 100.
    #[test]
    fn area_click_restores_the_saved_values() {
        let Some(mut s) = page26() else { return };
        let (_, y) = knob_point(&s, 796);
        let ev = s.left_down(80 + 60, y, true);
        assert!(ev.iter().any(|e| matches!(e, UiEvent::Command { dialog: 26, link: 796, .. })));
        assert_eq!(values(&s), vec![0, 100, 0, 0, 0]);
    }
}
