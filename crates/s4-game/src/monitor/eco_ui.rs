//! A computer player's eco engine (`s4_sim::ai::eco::view::eco_view`, script's), for the AI tab
//! (everything) and the Queues tab (the planned builds): per eco-sector AI its task list in
//! scheduler order with the main / sub problems and their chosen plans, the planned buildings,
//! the reservation board and the goals.

use egui::{Color32, RichText};
use s4_sim::ai::eco::view::{EcoView, PlanNode, PlannedBuilding, SectorView};

use super::{building_name, good_name};

/// Rows shown per list (the rest counted).
const ROWS: usize = 40;

fn planned_status(p: &PlannedBuilding) -> (&'static str, Color32) {
    match p.status {
        1 => ("planned", Color32::YELLOW),
        2 => ("site", Color32::LIGHT_BLUE),
        3 => ("finished", Color32::LIGHT_GREEN),
        _ => ("?", Color32::GRAY),
    }
}

fn entry_state(s: i32) -> &'static str {
    match s {
        1 => "waiting",
        2 => "running",
        3 => "finished",
        _ => "?",
    }
}

fn kind_name(k: i32) -> &'static str {
    u16::try_from(k).map_or("?", building_name)
}

/// The planned buildings of every sector, in order.
pub fn planned_builds(ui: &mut egui::Ui, eco: &EcoView) {
    let all: Vec<(&SectorView, &PlannedBuilding)> = eco.sectors.iter().flat_map(|s| s.planned.iter().map(move |p| (s, p))).collect();
    if all.is_empty() {
        ui.label("nothing planned");
        return;
    }
    egui::Grid::new("planned").striped(true).min_col_width(48.0).show(ui, |ui| {
        for h in ["sector", "building", "at", "status", "state", "site / building"] {
            ui.label(RichText::new(h).strong());
        }
        ui.end_row();
        for (s, p) in all.iter().take(ROWS) {
            ui.label(s.sector_ai.to_string());
            ui.label(kind_name(p.kind));
            ui.label(format!("({}, {})", p.x, p.y));
            let (t, c) = planned_status(p);
            ui.label(RichText::new(t).color(c));
            ui.label(p.state.to_string());
            ui.label(if p.building == 0 { "–".to_string() } else { p.building.to_string() });
            ui.end_row();
        }
    });
    if all.len() > ROWS {
        ui.label(RichText::new(format!("… and {} more", all.len() - ROWS)).weak());
    }
}

/// A chosen plan as an indented tree (op 1 builds: its params carry the board reservation).
fn plan(ui: &mut egui::Ui, n: &PlanNode, depth: usize) {
    let op = match n.op {
        1 => "build".to_string(),
        o => format!("op {o}"),
    };
    ui.label(RichText::new(format!("{}{op} {:?}", "  ".repeat(depth), n.params)).monospace());
    for s in &n.subs {
        plan(ui, s, depth + 1);
    }
}

/// Everything of the engine, sector by sector.
pub fn engine(ui: &mut egui::Ui, eco: &EcoView) {
    for s in &eco.sectors {
        let lock = if s.lock != 0 || s.waiting != 0 { format!(", planning lock {} / waiting {}", s.lock, s.waiting) } else { String::new() };
        let title = format!("eco engine, sector AI {} (eco-sector {}): {} entries, {} planned{lock}", s.sector_ai, s.eco, s.entries.len(), s.planned.len());
        egui::CollapsingHeader::new(title).id_salt(("eco engine", s.sector_ai)).default_open(true).show(ui, |ui| {
            ui.label(format!("made {}, steps {}, cursor {}, focus {}", s.made, s.steps, s.cursor, s.focus));
            egui::CollapsingHeader::new(format!("Planned buildings ({})", s.planned.len())).id_salt(("planned", s.sector_ai)).default_open(true).show(ui, |ui| {
                planned_builds(ui, &EcoView { player: eco.player, sectors: vec![s.clone()] });
            });
            egui::CollapsingHeader::new(format!("Task list ({})", s.entries.len())).id_salt(("entries", s.sector_ai)).show(ui, |ui| {
                for e in s.entries.iter().take(ROWS * 2) {
                    let head = format!("#{} {} [{}] → {}: {}", e.id, e.class, entry_state(e.state), e.link, e.text);
                    match &e.problem {
                        None => {
                            ui.label(RichText::new(head).monospace());
                        }
                        Some(p) => {
                            let result = match p.result {
                                1 => " failed",
                                2 => " done",
                                _ => "",
                            };
                            let name = if p.kind_name.is_empty() { format!("kind {}", p.kind) } else { p.kind_name.to_string() };
                            let what = format!("{head} — {} problem: {name} {:?}{result}", if p.main { "main" } else { "sub" }, p.params);
                            egui::CollapsingHeader::new(RichText::new(what).monospace()).id_salt(("problem", s.sector_ai, e.id)).show(ui, |ui| {
                                if let Some(st) = p.solver_status {
                                    ui.label(format!("solver status {st}"));
                                }
                                if !p.scans.is_empty() {
                                    ui.label(format!("waits for place scans of {}", p.scans.iter().map(|&k| kind_name(k)).collect::<Vec<_>>().join(", ")));
                                }
                                match &p.plan {
                                    Some(n) => plan(ui, n, 0),
                                    None => {
                                        ui.label("no plan chosen");
                                    }
                                }
                            });
                        }
                    }
                }
            });
            egui::CollapsingHeader::new(format!("Goals ({})", s.goals.len())).id_salt(("goals", s.sector_ai)).show(ui, |ui| {
                for g in &s.goals {
                    let good = u8::try_from(g.good).map_or("?", good_name);
                    ui.label(format!("{} {good} at level {} (from {})", g.amount, g.level, g.from));
                }
            });
            egui::CollapsingHeader::new(format!("Reservation board ({})", s.reserved.len())).id_salt(("reserved", s.sector_ai)).show(ui, |ui| {
                let mut r: Vec<_> = s.reserved.iter().collect();
                r.sort_by_key(|r| std::cmp::Reverse(r.score));
                for r in r.iter().take(ROWS) {
                    ui.label(format!("{} at ({}, {}) cell ({}, {}), score {}{}", kind_name(r.kind), r.x, r.y, r.cx, r.cy, r.score, if r.group != 0 { format!(", group {}", r.group) } else { String::new() }));
                }
            });
        });
    }
}
