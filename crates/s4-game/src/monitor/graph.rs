//! Ring buffers and the line graphs drawn from them.

use std::collections::VecDeque;

use egui::{Color32, Pos2, Stroke, Ui, Vec2};

/// The newest `cap` values.
#[derive(Clone, Debug)]
pub struct Ring<T> {
    buf: VecDeque<T>,
    cap: usize,
}

impl<T: Clone> Ring<T> {
    pub fn new(cap: usize) -> Ring<T> {
        Ring { buf: VecDeque::with_capacity(cap), cap }
    }

    pub fn push(&mut self, v: T) {
        if self.buf.len() == self.cap {
            self.buf.pop_front();
        }
        self.buf.push_back(v);
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &T> + ExactSizeIterator {
        self.buf.iter()
    }

    pub fn last(&self) -> Option<&T> {
        self.buf.back()
    }

}

/// A line of a graph: its name, colour and values (oldest first).
pub struct Line<'a> {
    pub name: &'a str,
    pub colour: Color32,
    pub values: Vec<f32>,
}

/// Distinct colours for the lines of one graph.
pub const COLOURS: [Color32; 6] = [
    Color32::from_rgb(90, 170, 255),
    Color32::from_rgb(255, 170, 60),
    Color32::from_rgb(120, 220, 120),
    Color32::from_rgb(230, 100, 230),
    Color32::from_rgb(240, 230, 90),
    Color32::from_rgb(120, 230, 230),
];

/// Lines over `slots` x positions (the ring's capacity: a young ring fills from the right), the
/// y axis from 0 to the largest value (at least `min_top`); the title with the unit and the
/// largest value, the legend with each line's newest value.
pub fn graph(ui: &mut Ui, title: &str, unit: &str, lines: &[Line], slots: usize, min_top: f32) {
    // The y range up to the next 1 / 2 / 5 · 10ⁿ above the largest value: it steps, it does not
    // follow every new sample.
    let top = nice(lines.iter().flat_map(|l| l.values.iter().copied()).filter(|v| v.is_finite()).fold(min_top, f32::max));
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new(title).strong());
        ui.label(egui::RichText::new(format!("(0..{} {unit})", fmt(top))).weak());
        for l in lines {
            let now = l.values.last().copied().unwrap_or(0.0);
            // Monospace and padded: a value that changes does not move or rewrap the legend.
            ui.label(egui::RichText::new(format!("■ {} {:>6}", l.name, fmt(now))).monospace().color(l.colour));
        }
    });
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 70.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 2.0, Color32::from_black_alpha(120));
    // The y axis: its top and 0, with the unit.
    let axis = egui::FontId::monospace(10.0);
    let weak = Color32::from_gray(150);
    p.text(rect.left_top() + Vec2::new(3.0, 1.0), egui::Align2::LEFT_TOP, format!("{} {unit}", fmt(top)), axis.clone(), weak);
    p.text(rect.left_bottom() + Vec2::new(3.0, -1.0), egui::Align2::LEFT_BOTTOM, format!("0 {unit}"), axis, weak);
    for f in [0.25, 0.5, 0.75] {
        let y = rect.bottom() - rect.height() * f;
        p.line_segment([Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)], Stroke::new(1.0_f32, Color32::from_white_alpha(18)));
    }
    let slots = slots.max(2);
    for l in lines {
        let n = l.values.len();
        let pts: Vec<Pos2> = l
            .values
            .iter()
            .enumerate()
            .map(|(i, &v)| {
                let x = rect.left() + rect.width() * (slots - n + i) as f32 / (slots - 1) as f32;
                let y = rect.bottom() - rect.height() * (v.max(0.0) / top).min(1.0);
                Pos2::new(x, y)
            })
            .collect();
        if pts.len() >= 2 {
            p.add(egui::Shape::line(pts, Stroke::new(1.5_f32, l.colour)));
        }
    }
}

/// Bytes as MB or GB.
pub fn bytes(b: u64) -> String {
    let mb = b as f64 / 1048576.0;
    if mb >= 1024.0 { format!("{:.1} GB", mb / 1024.0) } else { format!("{mb:.0} MB") }
}

/// The summary: a line per subject (its name, then its values, each with a tooltip saying what
/// it is), wrapped to the window's width; the values monospace.
pub fn summary(ui: &mut Ui, id: &str, rows: &[(&str, Vec<(String, &str)>)]) {
    ui.push_id(id, |ui| {
        for (name, values) in rows {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                let h = ui.text_style_height(&egui::TextStyle::Body);
                ui.add_sized([60.0, h], egui::Label::new(egui::RichText::new(*name).strong()));
                for (v, tip) in values {
                    ui.label(egui::RichText::new(v).monospace()).on_hover_text(*tip);
                }
            });
        }
    });
    ui.separator();
}

/// Name / value pairs in a fixed grid (`columns` pairs a row), the values monospace and padded:
/// numbers that change do not move the layout.
pub fn stats(ui: &mut Ui, id: &str, pairs: &[(&str, String)], columns: usize) {
    egui::Grid::new(id).num_columns(columns * 2).min_col_width(40.0).show(ui, |ui| {
        for (k, (name, v)) in pairs.iter().enumerate() {
            ui.label(egui::RichText::new(*name).weak());
            ui.label(egui::RichText::new(format!("{v:>9}")).monospace());
            if k % columns == columns - 1 {
                ui.end_row();
            }
        }
    });
}

/// The smallest 1 / 2 / 5 · 10ⁿ at or above `v`.
pub fn nice(v: f32) -> f32 {
    if v <= 0.0 {
        return 1.0;
    }
    let p = 10f32.powf(v.log10().floor());
    [1.0, 2.0, 5.0, 10.0].into_iter().map(|k| k * p).find(|&t| t >= v * 0.9999).unwrap_or(10.0 * p)
}

/// A number short: 3 significant digits.
pub fn fmt(v: f32) -> String {
    let a = v.abs();
    if a >= 100.0 {
        format!("{v:.0}")
    } else if a >= 10.0 {
        format!("{v:.1}")
    } else {
        format!("{v:.2}")
    }
}
