//! The Units tab: the selected settlers' job state (`s4_sim::debug::settler`: role, task and job
//! part, the task list, target, home, pending events, where it walks, a soldier's state) and the
//! decision log of each (`s4_sim::debug::watch` / `take_log`: events, role and task changes, the
//! to-do list, set free, attacks) since it was first selected with the monitor open.

use egui::RichText;

use super::{Monitor, settler_name};

/// Settlers inspected at most (the first of the selection).
pub const INSPECTED: usize = 8;

impl Monitor {
    pub(super) fn units_tab(&self, ui: &mut egui::Ui) {
        let v = &self.data.sim.views;
        if v.settlers.is_empty() {
            ui.label("Select settlers to inspect them (the first 8 of the selection).");
            return;
        }
        for (id, s) in &v.settlers {
            let title = format!("{id} {} of player {} at ({}, {})", settler_name(s.kind), s.owner, s.x, s.y);
            egui::CollapsingHeader::new(RichText::new(title).strong()).id_salt(("unit", *id)).default_open(true).show(ui, |ui| {
                egui::Grid::new(("unit grid", *id)).striped(true).min_col_width(48.0).show(ui, |ui| {
                    let mut row = |k: &str, v: String| {
                        ui.label(k);
                        ui.label(v);
                        ui.end_row();
                    };
                    row("role / task / job part", format!("{} / {} / {}", s.role, s.task, s.job_part));
                    row("health, flags", format!("{}, {:#010x}", s.health, s.flags));
                    match s.list {
                        Some((race, list)) => row("task list", format!("race {race}, list {list}")),
                        None if !s.own_tasks.is_empty() => row("own tasks", s.own_tasks.iter().map(|(t, x, y)| format!("{t}@({x}, {y})")).collect::<Vec<_>>().join(" ")),
                        None => row("task list", "none".into()),
                    }
                    row("target, home", format!("{}, {}", s.target, s.home));
                    if s.walk_to >= 0 {
                        row("walking to", format!("({}, {})", s.walk_to & 0xffff, s.walk_to >> 16));
                    }
                    if !s.events.is_empty() {
                        row("events", s.events.iter().map(|(e, k)| format!("{e}:{k}")).collect::<Vec<_>>().join(" "));
                    }
                    if let Some((state, target)) = s.soldier {
                        row("soldier", format!("state {state}, target {target}"));
                    }
                });
                let log: Vec<&s4_sim::debug::LogEntry> = self.data.sim.log.iter().filter(|e| e.entity == *id).collect();
                egui::CollapsingHeader::new(format!("Decisions ({})", log.len())).id_salt(("log", *id)).default_open(true).show(ui, |ui| {
                    for e in log.iter().rev().take(40) {
                        ui.label(RichText::new(format!("tick {:>6}  {:<9} {} {}", e.tick, e.what, e.a, e.b)).monospace());
                    }
                });
            });
        }
    }
}
