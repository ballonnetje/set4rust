//! Sidebar: categories, groups and slots with progress; search and filter.

use egui::{Align2, Color32, CornerRadius, FontId, RichText, Sense, Stroke, Ui, pos2, vec2};

use super::{Row, Studio, widgets};
use crate::model::SlotRef;
use crate::status::{Filter, matches};
use crate::theme::{self, Palette, icon};

const ROW_H: f32 = 28.0;

impl Studio {
    /// Slots of a group that pass the filter and search.
    fn shown_slots(&self, gi: usize) -> Vec<usize> {
        let g = &self.spec.groups[gi];
        (0..g.slots.len()).filter(|&s| matches(self.status.slots[gi][s], &g.slots[s], self.filter, &self.search)).collect()
    }

    fn rebuild_rows(&mut self) {
        let mut rows = Vec::new();
        let searching = !self.search.is_empty();
        for (ci, cat) in self.spec.categories.iter().enumerate() {
            rows.push(Row::Category(ci));
            if !(self.open.contains(&cat.id) || searching) {
                continue;
            }
            for id in &cat.groups {
                let Some(gi) = self.spec.group_index(id) else { continue };
                let shown = self.shown_slots(gi);
                if searching && shown.is_empty() {
                    continue;
                }
                rows.push(Row::Group(gi));
                if self.open.contains(id) || (searching && shown.len() <= 200) {
                    rows.extend(shown.into_iter().map(|s| Row::Slot(SlotRef { g: gi, s })));
                }
            }
        }
        self.rows = rows;
        self.rows_dirty = false;
    }

    fn toggle(&mut self, id: String) {
        if !self.open.remove(&id) {
            self.open.insert(id);
        }
        self.rows_dirty = true;
    }

    pub(super) fn sidebar(&mut self, ui: &mut Ui, p: &Palette) {
        let panel = egui::Panel::left("sidebar").resizable(true).default_size(self.settings.sidebar).size_range(240.0..=560.0).frame(egui::Frame::new().fill(p.panel).stroke(Stroke::new(1.0, p.line))).show(ui, |ui| {
            egui::Frame::new().inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                let hint = RichText::new(format!("{}  Search slots ( / )", icon::MAGNIFYING_GLASS)).color(p.faint);
                let edit = egui::TextEdit::singleline(&mut self.search_text).hint_text(hint).desired_width(f32::INFINITY).margin(egui::Margin::symmetric(9, 6));
                let r = ui.add(edit);
                if std::mem::take(&mut self.focus_search) {
                    r.request_focus();
                }
                if r.changed() {
                    self.search_changed();
                }
                ui.add_space(2.0);
                let items = [(Filter::All, "All"), (Filter::Todo, "To do"), (Filter::Problem, "Problems"), (Filter::Done, "Done")];
                if widgets::segmented(ui, &mut self.filter, &items, p) {
                    self.rows_dirty = true;
                }
            });
            if self.rows_dirty {
                self.rebuild_rows();
            }
            let mut area = egui::ScrollArea::vertical().auto_shrink([false, false]);
            if self.scroll_to_sel > 0
                && let Some(i) = self.rows.iter().position(|r| matches!(r, Row::Slot(s) if Some(*s) == self.sel))
            {
                // Over several frames: the area clamps to last frame's content height.
                self.scroll_to_sel -= 1;
                let view = ui.available_height();
                let y = (i as f32 * ROW_H - view * 0.4).max(0.0);
                area = area.vertical_scroll_offset(y);
            }
            let rows = std::mem::take(&mut self.rows);
            ui.spacing_mut().item_spacing.y = 0.0;
            area.show_rows(ui, ROW_H, rows.len(), |ui, range| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for i in range {
                    self.row(ui, rows[i], p);
                }
            });
            self.rows = rows;
        });
        let w = panel.response.rect.width();
        if (w - self.settings.sidebar).abs() > 1.0 {
            self.settings.sidebar = w;
            self.save_settings_later();
        }
    }

    fn row(&mut self, ui: &mut Ui, row: Row, p: &Palette) {
        let spec = self.spec.clone();
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
        if !ui.is_rect_visible(rect) {
            return;
        }
        let painter = ui.painter();
        let selected = matches!(row, Row::Slot(s) if Some(s) == self.sel);
        let body = rect.shrink2(vec2(6.0, 1.0));
        if selected {
            painter.rect_filled(body, CornerRadius::same(6), p.accent_soft);
        } else if resp.hovered() {
            painter.rect_filled(body, CornerRadius::same(6), p.panel2);
        }
        let y = rect.center().y;
        let caret = |x: f32, open: bool, color: Color32| {
            painter.text(pos2(x, y), Align2::CENTER_CENTER, if open { icon::CARET_DOWN } else { icon::CARET_RIGHT }, FontId::proportional(11.0), color);
        };
        let right = rect.right() - 14.0;
        match row {
            Row::Category(ci) => {
                let cat = &spec.categories[ci];
                let open = self.open.contains(&cat.id) || !self.search.is_empty();
                caret(rect.left() + 18.0, open, p.dim);
                let (mut have, mut need) = (0, 0);
                for gi in cat.groups.iter().filter_map(|id| spec.group_index(id)) {
                    have += self.status.groups[gi].have;
                    need += self.status.groups[gi].need;
                }
                let bar = egui::Rect::from_min_size(pos2(right - 54.0, y - 2.5), vec2(54.0, 5.0));
                widgets::paint_bar(ui, bar, have, need, p);
                let g = widgets::elide(ui, &cat.label, theme::semibold(13.0), p.text, bar.left() - rect.left() - 38.0);
                painter.galley(pos2(rect.left() + 30.0, y - g.size().y / 2.0), g, p.text);
                if resp.clicked() {
                    let id = cat.id.clone();
                    self.defer(move |st| st.toggle(id));
                }
            }
            Row::Group(gi) => {
                let g = &spec.groups[gi];
                let gs = self.status.groups[gi];
                let open = self.open.contains(&g.id) || (!self.search.is_empty() && self.shown_slots(gi).len() <= 200);
                caret(rect.left() + 30.0, open, p.faint);
                let bar = egui::Rect::from_min_size(pos2(right - 40.0, y - 2.5), vec2(40.0, 5.0));
                widgets::paint_bar(ui, bar, gs.have, gs.need, p);
                let count = format!("{}/{}", gs.done, gs.slots);
                let num = painter.text(pos2(bar.left() - 8.0, y), Align2::RIGHT_CENTER, count, FontId::proportional(11.0), p.faint);
                let t = widgets::elide(ui, &g.label, FontId::proportional(13.0), p.text, num.left() - rect.left() - 50.0);
                painter.galley(pos2(rect.left() + 42.0, y - t.size().y / 2.0), t, p.text);
                if resp.clicked() {
                    let id = g.id.clone();
                    self.defer(move |st| st.toggle(id));
                }
            }
            Row::Slot(r) => {
                let (_, slot) = spec.slot(r);
                let st = self.status.slot(r);
                painter.circle_filled(pos2(rect.left() + 50.0, y), 3.5, widgets::state_color(st.state, p));
                let count = format!("{}/{}", st.have, st.need);
                let num = painter.text(pos2(right, y), Align2::RIGHT_CENTER, count, FontId::proportional(11.0), p.faint);
                let color = if selected { p.accent } else { p.text };
                let t = widgets::elide(ui, &slot.name, FontId::proportional(12.5), color, num.left() - rect.left() - 68.0);
                painter.galley(pos2(rect.left() + 60.0, y - t.size().y / 2.0), t, color);
                if resp.clicked() {
                    self.defer(move |st| st.go(Some(r)));
                }
                let tip = match &slot.owner {
                    Some(o) => format!("{} — {o}", slot.name),
                    None => slot.name.clone(),
                };
                resp.on_hover_text(tip);
            }
        }
    }
}
