//! The Economy tab: one player's construction sites (progress, materials needed / used / at the
//! site / on their way, diggers and builders), the production buildings' state, the carriers and
//! the goods in store, from [`crate::econ_view`] (`s4_sim::debug`'s views, read on the sim thread
//! once a second while the tab is shown).

use egui::{Color32, RichText};
use s4_sim::debug::ProdState;

use super::{Monitor, building_name, good_name};

fn site_state(s: u8) -> &'static str {
    match s {
        0 | 1 => "placed",
        2 | 3 => "levelling",
        4 => "waiting for the ground",
        5 => "building",
        6..=8 => "building (Dark Tribe)",
        _ => "?",
    }
}

impl Monitor {
    pub(super) fn economy_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Player");
            for p in self.players.clone() {
                ui.selectable_value(&mut self.economy_player, p, p.to_string());
            }
        });
        let Some(e) = self.data.sim.views.economy.as_ref().filter(|e| e.player == self.economy_player) else {
            ui.label("(reading…)");
            return;
        };
        let c = e.carriers;
        ui.label(format!(
            "tick {}: {} construction sites, {} production buildings; carriers {} ({} idle, {} busy), {} goods asked for, {} on piles, {} carried",
            e.tick,
            e.sites.len(),
            e.producers.len(),
            c.carriers,
            c.idle,
            c.busy,
            c.open_requests,
            c.goods_on_piles,
            c.goods_carried
        ));
        egui::CollapsingHeader::new(format!("Construction sites ({})", e.sites.len())).id_salt("construction_sites@econ_tab.rs").default_open(true).show(ui, |ui| {
            egui::Grid::new("sites").striped(true).min_col_width(48.0).show(ui, |ui| {
                for h in ["site", "state", "built", "materials: used + here / needed (+ coming)", "diggers", "builders"] {
                    ui.label(RichText::new(h).strong());
                }
                ui.end_row();
                for s in &e.sites {
                    ui.label(format!("{} {} ({}, {}){}", s.id, building_name(s.kind), s.x, s.y, if s.priority { " !" } else { "" }));
                    ui.label(site_state(s.state));
                    ui.add(egui::ProgressBar::new(s.progress as f32 / 100.0).desired_width(60.0).text(format!("{}%", s.progress)));
                    ui.horizontal(|ui| {
                        for m in &s.materials {
                            let short = m.needed.saturating_sub(m.used + m.on_site + m.coming);
                            let coming = if m.coming > 0 { format!(" (+{})", m.coming) } else { String::new() };
                            let t = format!("{} {}+{}/{}{coming}", good_name(m.good), m.used, m.on_site, m.needed);
                            ui.label(RichText::new(t).color(if short > 0 { Color32::LIGHT_RED } else { Color32::GRAY }));
                        }
                    });
                    ui.label(format!("{}/{}", s.diggers_working, s.diggers));
                    ui.label(s.builders_working.to_string());
                    ui.end_row();
                }
            });
        });
        let state = |s: ProdState| match s {
            ProdState::NoWorker => ("no worker".to_string(), Color32::LIGHT_RED),
            ProdState::Stopped => ("stopped".to_string(), Color32::GRAY),
            ProdState::OutputFull(g) => (format!("{} full", good_name(g)), Color32::YELLOW),
            ProdState::WaitingFor(g) => (format!("waiting for {}", good_name(g)), Color32::YELLOW),
            ProdState::Working => ("working".to_string(), Color32::LIGHT_GREEN),
        };
        let mut states: std::collections::BTreeMap<String, u32> = Default::default();
        for p in &e.producers {
            let k = match p.state {
                ProdState::OutputFull(_) => "output full".to_string(),
                ProdState::WaitingFor(_) => "waiting".to_string(),
                s => state(s).0,
            };
            *states.entry(k).or_default() += 1;
        }
        let summary: Vec<String> = states.iter().map(|(k, v)| format!("{v} {k}")).collect();
        egui::CollapsingHeader::new(if summary.is_empty() { "Production".to_string() } else { format!("Production ({})", summary.join(", ")) }).id_salt("production@econ_tab.rs").default_open(true).show(ui, |ui| {
            egui::Grid::new("producers").striped(true).min_col_width(48.0).show(ui, |ui| {
                for h in ["building", "state", "in", "out"] {
                    ui.label(RichText::new(h).strong());
                }
                ui.end_row();
                let piles = |v: &[(u8, u8, u8)]| v.iter().map(|&(g, n, c)| if c > 0 { format!("{n} {} (+{c})", good_name(g)) } else { format!("{n} {}", good_name(g)) }).collect::<Vec<_>>().join(", ");
                for p in &e.producers {
                    ui.label(format!("{} {} ({}, {})", p.id, building_name(p.kind), p.x, p.y));
                    let (t, c) = state(p.state);
                    ui.label(RichText::new(t).color(c));
                    ui.label(piles(&p.inputs));
                    ui.label(piles(&p.outputs));
                    ui.end_row();
                }
            });
        });
        egui::CollapsingHeader::new("Goods in store").id_salt("goods_in_store@econ_tab.rs").default_open(true).show(ui, |ui| {
            egui::Grid::new("goods").striped(true).min_col_width(48.0).num_columns(6).show(ui, |ui| {
                let mut k = 0;
                for (g, &n) in e.goods.iter().enumerate() {
                    if n == 0 {
                        continue;
                    }
                    ui.label(good_name(g as u8));
                    ui.label(n.to_string());
                    k += 1;
                    if k % 3 == 0 {
                        ui.end_row();
                    }
                }
            });
        });
    }
}
