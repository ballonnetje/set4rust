//! The Queues tab: one player's queues ([`crate::queues_view`], `s4_sim::debug`'s views read on
//! the sim thread once a second while the tab shows): transport (each eco-sector's requests in
//! queue order with how long they have waited, the carriers' jobs, the offers, the idle
//! carriers), the build queue (the order sites are served in, what each waits for and since when),
//! the production orders (tools, weapons, soldiers, specialists per eco-sector; workshop /
//! shipyard vehicles), the settlers' job states per type and the computer's attack agents.
//! The computer's planned builds come from its eco engine (script's `eco_view`).

use egui::{Color32, RichText};
use s4_sim::debug::SiteWait;

use super::{Monitor, building_name, good_name, settler_name};

/// Rows shown per list (the rest counted).
const ROWS: usize = 40;

/// The vehicle type names (types 1..=5).
const VEHICLES: [&str; 5] = ["war ship", "ferry", "transport ship", "war machine", "cart"];

/// The attack agent's steps (`ai::attack`).
fn attack_step(s: i32) -> &'static str {
    match s {
        0 => "waiting",
        1 => "walking to the enemy's edge",
        2 => "fighting on",
        3 => "all attack",
        _ => "?",
    }
}

/// `n` more rows than shown.
fn more(ui: &mut egui::Ui, n: usize) {
    if n > ROWS {
        ui.label(RichText::new(format!("… and {} more", n - ROWS)).weak());
    }
}

impl Monitor {
    pub(super) fn queues_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Player");
            for p in self.players.clone() {
                ui.selectable_value(&mut self.economy_player, p, p.to_string());
            }
        });
        let Some(q) = self.data.sim.views.queues.as_ref().filter(|q| q.player == self.economy_player) else {
            ui.label("(reading…)");
            return;
        };
        let now = q.tick;
        let waited = |since: Option<u32>| since.map_or("–".to_string(), |t| format!("{} ticks", now.saturating_sub(t)));
        ui.label(format!("tick {now}"));

        // Transport, per eco-sector.
        let requests: usize = q.transport.iter().map(|t| t.requests.len()).sum();
        let jobs: usize = q.transport.iter().map(|t| t.jobs.len()).sum();
        let idle: usize = q.transport.iter().map(|t| t.idle_carriers.len()).sum();
        egui::CollapsingHeader::new(format!("Transport: {requests} requests open, {jobs} carriers on a job, {idle} idle")).id_salt("transport@queues_tab.rs").default_open(true).show(ui, |ui| {
            for t in q.transport.iter().filter(|t| !t.requests.is_empty() || !t.jobs.is_empty() || !t.offers.is_empty() || !t.idle_carriers.is_empty()) {
                let offered: i32 = t.offers.iter().map(|o| o.amount).sum();
                let title = format!("eco-sector {}: {} requests, {} jobs, {} goods on offer in {} piles, {} idle carriers", t.eco, t.requests.len(), t.jobs.len(), offered, t.offers.len(), t.idle_carriers.len());
                egui::CollapsingHeader::new(title).id_salt(("eco", t.eco)).default_open(true).show(ui, |ui| {
                    egui::CollapsingHeader::new(format!("Requests ({})", t.requests.len())).id_salt(("req", t.eco)).default_open(true).show(ui, |ui| {
                        egui::Grid::new(("requests", t.eco)).striped(true).min_col_width(48.0).show(ui, |ui| {
                            for h in ["good", "for", "waiting"] {
                                ui.label(RichText::new(h).strong());
                            }
                            ui.end_row();
                            for r in t.requests.iter().take(ROWS) {
                                ui.label(good_name(r.good));
                                ui.label(format!("{} ({}, {})", r.building, r.x, r.y));
                                ui.label(waited(r.since));
                                ui.end_row();
                            }
                        });
                        more(ui, t.requests.len());
                    });
                    egui::CollapsingHeader::new(format!("Carrier jobs ({})", t.jobs.len())).id_salt(("jobs", t.eco)).show(ui, |ui| {
                        egui::Grid::new(("jobs grid", t.eco)).striped(true).min_col_width(48.0).show(ui, |ui| {
                            for h in ["carrier", "good", "from pile", "to pile", "at"] {
                                ui.label(RichText::new(h).strong());
                            }
                            ui.end_row();
                            for j in t.jobs.iter().take(ROWS) {
                                ui.label(j.carrier.to_string());
                                ui.label(if j.good == 0 { "–" } else { good_name(j.good) });
                                ui.label(if j.from == 0 { "–".to_string() } else { j.from.to_string() });
                                ui.label(if j.to == 0 { "–".to_string() } else { j.to.to_string() });
                                ui.label(format!("({}, {})", j.x, j.y));
                                ui.end_row();
                            }
                        });
                        more(ui, t.jobs.len());
                    });
                    egui::CollapsingHeader::new(format!("Offers ({})", t.offers.len())).id_salt(("offers", t.eco)).show(ui, |ui| {
                        for o in t.offers.iter().take(ROWS) {
                            ui.label(format!("{} {} on pile {} ({}, {})", o.amount, good_name(o.good), o.pile, o.x, o.y));
                        }
                        more(ui, t.offers.len());
                    });
                    if !t.idle_carriers.is_empty() {
                        let ids: Vec<String> = t.idle_carriers.iter().take(ROWS).map(|c| c.to_string()).collect();
                        ui.label(format!("idle carriers: {}{}", ids.join(" "), if t.idle_carriers.len() > ROWS { " …" } else { "" }));
                    }
                });
            }
        });

        // The build queue.
        egui::CollapsingHeader::new(format!("Building ({} sites, in the order served)", q.build.len())).id_salt("building@queues_tab.rs").default_open(true).show(ui, |ui| {
            egui::Grid::new("build queue").striped(true).min_col_width(48.0).show(ui, |ui| {
                for h in ["#", "site", "built", "waiting for", "since"] {
                    ui.label(RichText::new(h).strong());
                }
                ui.end_row();
                for s in q.build.iter().take(ROWS) {
                    let site = &s.site;
                    ui.label((s.position + 1).to_string());
                    ui.label(format!("{} {} ({}, {}){}", site.id, building_name(site.kind), site.x, site.y, if site.priority { " ! priority" } else { "" }));
                    ui.label(format!("{}%", site.progress));
                    let (t, c) = match s.waits {
                        SiteWait::Diggers => ("diggers".to_string(), Color32::YELLOW),
                        SiteWait::Material(g) => (format!("material: {}", good_name(g)), Color32::LIGHT_RED),
                        SiteWait::Builders => ("builders".to_string(), Color32::YELLOW),
                        SiteWait::Nothing => ("nothing (working)".to_string(), Color32::LIGHT_GREEN),
                    };
                    ui.label(RichText::new(t).color(c));
                    ui.label(if s.waits == SiteWait::Nothing { String::new() } else { waited(s.since) });
                    ui.end_row();
                }
            });
            more(ui, q.build.len());
        });

        // Production orders.
        egui::CollapsingHeader::new("Production orders").id_salt("production_orders@queues_tab.rs").default_open(true).show(ui, |ui| {
            let endless = |n: u8| if n >= 100 { "∞".to_string() } else { n.to_string() };
            for e in &q.eco_orders {
                let mut parts: Vec<String> = Vec::new();
                let tools: Vec<String> = e.tools.iter().filter(|t| t.1 != 0).map(|&(g, n)| format!("{} {}", endless(n), good_name(g))).collect();
                if !tools.is_empty() {
                    parts.push(format!("tools {}", tools.join(", ")));
                }
                let weapons: Vec<String> = e.weapons.iter().filter(|t| t.1 != 0).map(|&(g, n)| format!("{} {}", endless(n), good_name(g))).collect();
                if !weapons.is_empty() {
                    parts.push(format!("weapons {}", weapons.join(", ")));
                }
                if e.weapon_auto != 0 {
                    let pct: Vec<String> = e.weapons.iter().zip(&e.weapon_percent).map(|(&(g, _), p)| format!("{} {p}%", good_name(g))).collect();
                    parts.push(format!("weapons auto ({})", pct.join(", ")));
                }
                let soldiers: Vec<String> = e.soldier_orders.iter().enumerate().filter(|(_, n)| **n != 0).map(|(k, n)| format!("type {k}: {n}")).collect();
                if !soldiers.is_empty() {
                    parts.push(format!("soldiers {}", soldiers.join(", ")));
                }
                let wishes: Vec<String> = e.specialist_wish.iter().enumerate().filter(|(_, n)| **n != 0).map(|(k, n)| format!("{k}: {n}")).collect();
                if !wishes.is_empty() {
                    parts.push(format!("specialist wishes {}", wishes.join(", ")));
                }
                ui.label(format!("eco-sector {}: {}", e.eco, if parts.is_empty() { "nothing ordered".into() } else { parts.join("; ") }));
            }
            for b in &q.building_orders {
                let ordered: Vec<String> = b.orders.iter().map(|&(v, n)| format!("{} {}", endless(n), VEHICLES.get((v as usize).wrapping_sub(1)).copied().unwrap_or("?"))).collect();
                let making = if b.building != 0 { format!(", making vehicle {}", b.building) } else { String::new() };
                ui.label(format!("{} {}: {}{making}", b.id, building_name(b.kind), if ordered.is_empty() { "no orders".into() } else { ordered.join(", ") }));
            }
        });

        // Settlers by job state.
        let total = |f: &dyn Fn(&s4_sim::debug::JobStates) -> u32| q.jobs.iter().map(f).sum::<u32>();
        let title = format!("Settlers: {} idle, {} walking, {} working, {} waiting", total(&|j| j.idle), total(&|j| j.walking), total(&|j| j.working), total(&|j| j.waiting));
        egui::CollapsingHeader::new(title).id_salt("h@queues_tab.rs").default_open(true).show(ui, |ui| {
            egui::Grid::new("job states").striped(true).min_col_width(48.0).show(ui, |ui| {
                for h in ["type", "idle", "walking", "working", "waiting"] {
                    ui.label(RichText::new(h).strong());
                }
                ui.end_row();
                for j in &q.jobs {
                    ui.label(settler_name(j.kind));
                    for n in [j.idle, j.walking, j.working, j.waiting] {
                        ui.label(if n == 0 { String::new() } else { n.to_string() });
                    }
                    ui.end_row();
                }
            });
        });

        // The computer's attack agents.
        egui::CollapsingHeader::new(format!("AI: attacks ({})", q.attacks.len())).id_salt("ai:_attacks@queues_tab.rs").default_open(!q.attacks.is_empty()).show(ui, |ui| {
            if q.attacks.is_empty() {
                ui.label("no computer player, or no sector AI");
            }
            egui::Grid::new("attacks").striped(true).min_col_width(48.0).show(ui, |ui| {
                for h in ["sector", "step", "soldiers", "strength", "next try", "chance", "target"] {
                    ui.label(RichText::new(h).strong());
                }
                ui.end_row();
                for a in &q.attacks {
                    ui.label(a.sector.to_string());
                    ui.label(attack_step(a.step));
                    ui.label(a.soldiers.to_string());
                    ui.label(a.strength.to_string());
                    ui.label(if a.earliest > now as i32 { format!("in {} ticks", a.earliest - now as i32) } else { "now".into() });
                    ui.label(format!("{:.1}%", a.chance as f32 / 655.36));
                    ui.label(if a.target == 0 { "–".into() } else { format!("{} of player {} at ({}, {})", a.target, a.owner, a.x, a.y) });
                    ui.end_row();
                }
            });
            ui.label(RichText::new("Recruitment has no queue: the AI rewrites the barracks' soldier orders every 250 ticks.").weak());
        });
        if let Some(eco) = self.data.sim.views.eco.as_ref().filter(|e| e.player == q.player) {
            let n: usize = eco.sectors.iter().map(|s| s.planned.len()).sum();
            egui::CollapsingHeader::new(format!("AI: planned builds ({n})")).id_salt("ai:_planned_builds@queues_tab.rs").default_open(true).show(ui, |ui| super::eco_ui::planned_builds(ui, eco));
        }
    }
}
