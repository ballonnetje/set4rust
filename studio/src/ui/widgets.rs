//! Small building blocks shared by the panels.

use egui::{Color32, CornerRadius, Response, RichText, Sense, Stroke, Ui, vec2};

use crate::status::State;
use crate::theme::Palette;

/// A small rounded label.
pub fn chip(ui: &mut Ui, text: &str, color: Option<Color32>, p: &Palette) -> Response {
    let c = color.unwrap_or(p.dim);
    let stroke = if color.is_some() { c.gamma_multiply(0.55) } else { p.line };
    egui::Frame::new()
        .stroke(Stroke::new(1.0, stroke))
        .corner_radius(9)
        .inner_margin(egui::Margin::symmetric(8, 1))
        .show(ui, |ui| ui.label(RichText::new(text).size(11.5).color(c)))
        .response
}

/// A dim caption.
pub fn caption(ui: &mut Ui, text: &str, p: &Palette) -> Response {
    ui.label(RichText::new(text).color(p.dim).size(12.0))
}

pub fn small(ui: &mut Ui, text: impl Into<String>, p: &Palette) -> Response {
    ui.label(RichText::new(text.into()).color(p.dim).size(11.5))
}

/// A thin progress bar, at `width`.
pub fn bar(ui: &mut Ui, have: usize, need: usize, width: f32, p: &Palette) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 5.0), Sense::hover());
    paint_bar(ui, rect, have, need, p);
    resp
}

pub fn paint_bar(ui: &Ui, rect: egui::Rect, have: usize, need: usize, p: &Palette) {
    let r = CornerRadius::same(3);
    ui.painter().rect_filled(rect, r, p.line);
    if need > 0 && have > 0 {
        let k = (have as f32 / need as f32).clamp(0.0, 1.0);
        let mut full = rect;
        full.set_width((rect.width() * k).max(3.0));
        ui.painter().rect_filled(full, r, p.ok);
    }
}

pub fn state_color(s: State, p: &Palette) -> Color32 {
    match s {
        State::Done => p.ok,
        State::Partial => p.accent,
        State::Problem => p.err,
        State::Missing => p.raised,
    }
}

/// A button with a filled look. Disabled when `enabled` is false.
pub fn primary(ui: &mut Ui, text: &str, enabled: bool, p: &Palette) -> Response {
    let b = egui::Button::new(RichText::new(text).color(if enabled { p.on_accent } else { p.dim })).fill(if enabled { p.accent } else { p.panel2 }).stroke(Stroke::NONE);
    ui.add_enabled(enabled, b)
}

/// A square button with an icon.
pub fn icon_button(ui: &mut Ui, icon: &str, tip: &str, p: &Palette) -> Response {
    let b = egui::Button::new(RichText::new(icon).size(16.0).color(p.text)).min_size(vec2(30.0, 30.0)).frame_when_inactive(false);
    ui.add(b).on_hover_text(tip)
}

/// A toolbar button that stays lit while selected.
pub fn tool(ui: &mut Ui, label: &str, on: bool, tip: &str, p: &Palette) -> Response {
    let b = egui::Button::new(RichText::new(label).color(if on { p.accent } else { p.text })).fill(if on { p.accent_soft } else { p.panel2 }).stroke(Stroke::new(1.0, if on { p.accent } else { p.line }));
    ui.add(b).on_hover_text(tip)
}

/// Mutually exclusive choices in one strip.
pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, items: &[(T, &str)], p: &Palette) -> bool {
    let mut changed = false;
    egui::Frame::new().fill(p.panel2).corner_radius(7).inner_margin(2).show(ui, |ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.horizontal(|ui| {
            for (v, label) in items {
                let on = *value == *v;
                let b = egui::Button::new(RichText::new(*label).size(12.5).color(if on { p.text } else { p.dim })).fill(if on { p.raised } else { Color32::TRANSPARENT }).stroke(Stroke::NONE).corner_radius(5).min_size(vec2(0.0, 24.0));
                if ui.add(b).clicked() && !on {
                    *value = *v;
                    changed = true;
                }
            }
        });
    });
    changed
}

/// A row of buttons aligned to the right, built right to left.
pub fn button_row<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let w = ui.available_width();
    ui.allocate_ui_with_layout(vec2(w, 30.0), egui::Layout::right_to_left(egui::Align::Center), add).inner
}

/// The frame cards and panels sit in.
pub fn card_frame(p: &Palette) -> egui::Frame {
    egui::Frame::new().fill(p.panel).stroke(Stroke::new(1.0, p.line)).corner_radius(10).inner_margin(egui::Margin::same(12))
}

/// A thin slider with a small handle.
pub fn slider<N: egui::emath::Numeric>(ui: &mut Ui, value: &mut N, range: std::ops::RangeInclusive<N>) -> Response {
    let r = ui
        .scope(|ui| {
            ui.spacing_mut().interact_size.y = 14.0;
            ui.spacing_mut().slider_rail_height = 4.0;
            ui.add(egui::Slider::new(value, range).show_value(false))
        })
        .inner;
    // Space and the arrows stay with playback.
    if r.drag_stopped() || r.clicked() {
        r.surrender_focus();
    }
    r
}

/// A section title.
pub fn heading(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).font(crate::theme::semibold(14.0)));
}

/// A label that keeps to one line, ending in … when too wide.
pub fn elide(ui: &Ui, text: &str, font: egui::FontId, color: Color32, width: f32) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_string(), font, color);
    job.wrap = egui::text::TextWrapping { max_width: width, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    ui.fonts_mut(|f| f.layout_job(job))
}
