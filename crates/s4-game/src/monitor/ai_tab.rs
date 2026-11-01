//! The AI tab: one computer player's AI (`s4_sim::debug::ai`): its agents in the scheduler's
//! order with their next run and period, its task forces with their members, the sector AIs and
//! the player evaluation; its eco engine's task list, problems and plans, planned buildings,
//! reservations and goals (script's `eco_view`, [`super::eco_ui`]).

use egui::RichText;

use super::Monitor;

impl Monitor {
    pub(super) fn ai_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Player");
            for p in self.players.clone() {
                ui.selectable_value(&mut self.ai_player, p, p.to_string());
            }
        });
        let v = &self.data.sim.views;
        let Some(ai) = v.ai.as_ref().filter(|a| a.player == self.ai_player) else {
            ui.label("(no AI for this player, or reading…)");
            return;
        };
        let now = v.tick;
        ui.label(format!("tick {now}: {}{} events queued; {} agents, {} task forces", if ai.dark { "Dark Tribe AI, " } else { "" }, ai.queued, ai.agents.len(), ai.task_forces.len()));
        egui::CollapsingHeader::new(format!("Agents ({})", ai.agents.len())).id_salt("agents@ai_tab.rs").show(ui, |ui| {
            egui::Grid::new("agents").striped(true).min_col_width(48.0).show(ui, |ui| {
                for h in ["agent", "sector", "next run", "every"] {
                    ui.label(RichText::new(h).strong());
                }
                ui.end_row();
                for a in &ai.agents {
                    ui.label(&a.kind);
                    ui.label(a.sector.map_or("player".to_string(), |s| s.to_string()));
                    ui.label(format!("{} (in {})", a.next, a.next as i64 - now as i64));
                    ui.label(a.period.to_string());
                    ui.end_row();
                }
            });
        });
        egui::CollapsingHeader::new(format!("Task forces ({})", ai.task_forces.len())).id_salt("task_forces@ai_tab.rs").show(ui, |ui| {
            egui::Grid::new("forces").striped(true).min_col_width(48.0).show(ui, |ui| {
                for f in &ai.task_forces {
                    ui.label(&f.place);
                    let ids: Vec<String> = f.members.iter().take(12).map(|m| m.to_string()).collect();
                    ui.label(format!("{} members{}", f.members.len(), if ids.is_empty() { String::new() } else { format!(": {}{}", ids.join(" "), if f.members.len() > 12 { " …" } else { "" }) }));
                    ui.end_row();
                }
            });
        });
        egui::CollapsingHeader::new(format!("Sectors ({})", ai.sectors.len())).id_salt("sectors@ai_tab.rs").show(ui, |ui| {
            ui.label(ai.sectors.iter().map(|(s, k)| format!("{s} {k}")).collect::<Vec<_>>().join(", "));
        });
        egui::CollapsingHeader::new("Player evaluation").id_salt("player_evaluation@ai_tab.rs").show(ui, |ui| {
            egui::Grid::new("evaluation").striped(true).min_col_width(48.0).num_columns(8).show(ui, |ui| {
                for (k, w) in ai.evaluation.iter().enumerate() {
                    ui.label(RichText::new(format!("{k}: {w}")).monospace());
                    if k % 4 == 3 {
                        ui.end_row();
                    }
                }
            });
        });
        match v.eco.as_ref().filter(|e| e.player == self.ai_player) {
            Some(eco) => super::eco_ui::engine(ui, eco),
            None => {
                ui.label(RichText::new("(no eco engine view)").weak());
            }
        }
    }
}
