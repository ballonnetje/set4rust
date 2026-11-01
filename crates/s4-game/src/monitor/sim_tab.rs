//! The Simulation tab: the sim thread's ticks (time and its parts by phase and entity class,
//! ticks per second against the speed, the time waiting for the other players' turns), the
//! lockstep of a network game and the entities by player and type. Also the store of what the
//! sim thread sends ([`SimData`]) for the other tabs.

use std::collections::BTreeMap;

use egui::RichText;

use super::graph::{COLOURS, Line, Ring, fmt, graph};
use super::{Monitor, SECONDS};
use crate::sim::stats;

/// Ticks kept (about 85 s at normal speed).
pub const TICKS: usize = 1200;
/// Decision log entries kept.
pub const LOG: usize = 4096;
/// Tick parts graphed (the costliest of the last second).
const PART_LINES: usize = 6;

/// One second of the simulation.
#[derive(Clone, Debug, Default)]
pub struct SimSecond {
    /// Ticks run and the ticks the speed asks for (0: paused).
    pub tps: f32,
    pub target: f32,
    pub tick_avg: f32,
    pub tick_max: f32,
    pub wait_avg: f32,
    /// Player commands run.
    pub commands: f32,
    /// Each part's ms per tick.
    pub parts: BTreeMap<&'static str, f32>,
    /// Path searches in the second: coarse, coarse failed, coarse partial, fine, fine failed,
    /// then the coarse waypoints and fine steps they found.
    pub paths: [u32; 7],
}

/// Entities by player (index 0: no player) and kind, from the snapshot.
#[derive(Clone, Debug, Default)]
pub struct Counts {
    pub tick: u32,
    pub settlers: [u32; 9],
    pub buildings: [u32; 9],
    pub sites: [u32; 9],
    pub vehicles: [u32; 9],
    pub animals: u32,
    pub piles: u32,
    /// Settlers by type and player.
    pub by_type: BTreeMap<u16, [u32; 9]>,
}

impl Counts {
    pub fn of(s: &crate::sim::Snapshot) -> Counts {
        let p = &s.population;
        let mut c = Counts { tick: s.tick, animals: p.animals.len() as u32, piles: p.piles.len() as u32, ..Default::default() };
        let at = |o: u8| (o as usize).min(8);
        for u in &p.settlers {
            c.settlers[at(u.owner)] += 1;
            c.by_type.entry(u.kind).or_default()[at(u.owner)] += 1;
        }
        for b in &p.buildings {
            if b.site.is_some() {
                c.sites[at(b.owner)] += 1;
            } else {
                c.buildings[at(b.owner)] += 1;
            }
        }
        for v in &p.vehicles {
            c.vehicles[at(v.owner)] += 1;
        }
        c
    }
}

/// What the sim thread sent, kept for the tabs.
pub struct SimData {
    pub ticks: Ring<stats::Tick>,
    pub seconds: Ring<SimSecond>,
    /// This second's ticks so far.
    acc: Vec<stats::Tick>,
    pub net: Option<stats::Net>,
    pub desyncs: u32,
    pub counts: Option<Counts>,
    /// The newest views (economy, AI, settlers, paths).
    pub views: stats::Views,
    pub log: Ring<s4_sim::debug::LogEntry>,
    /// The snapshot's tick length at the last frame (0: paused).
    tick_ms: u64,
}

impl SimData {
    pub fn new() -> SimData {
        SimData {
            ticks: Ring::new(TICKS),
            seconds: Ring::new(SECONDS),
            acc: Vec::new(),
            net: None,
            desyncs: 0,
            counts: None,
            views: stats::Views::default(),
            log: Ring::new(LOG),
            tick_ms: 0,
        }
    }

    /// Each frame: what the sim thread sent since the last one.
    pub fn take(&mut self, sim: Option<&stats::Shared>, snapshot: Option<&crate::sim::Snapshot>) {
        let st = sim.map(stats::Shared::take).unwrap_or_default();
        for t in &st.ticks {
            self.ticks.push(t.clone());
        }
        self.acc.extend(st.ticks);
        if st.net.is_some() {
            self.net = st.net;
        }
        self.desyncs += st.desyncs;
        // A read replaces only what it read: no view goes empty between two reads.
        if let Some(v) = st.views {
            self.views.merge(v);
        }
        for e in st.log {
            self.log.push(e);
        }
        self.tick_ms = snapshot.map_or(0, |s| s.tick_ms);
    }

    /// Each second (`secs` long).
    pub fn second(&mut self, secs: f32, snapshot: Option<&crate::sim::Snapshot>) {
        let ticks = std::mem::take(&mut self.acc);
        let k = ticks.len().max(1) as f32;
        let mut parts: BTreeMap<&'static str, f32> = BTreeMap::new();
        let mut paths = [0u32; 7];
        for t in &ticks {
            for (n, ms) in &t.parts {
                *parts.entry(n).or_default() += ms / k;
            }
            let p = t.paths;
            for (a, b) in paths.iter_mut().zip([p.coarse, p.coarse_failed, p.coarse_partial, p.fine, p.fine_failed, p.coarse_waypoints, p.fine_steps]) {
                *a += b;
            }
        }
        self.seconds.push(SimSecond {
            tps: ticks.len() as f32 / secs,
            target: if self.tick_ms == 0 { 0.0 } else { 1000.0 / self.tick_ms as f32 },
            tick_avg: ticks.iter().map(|t| t.ms).sum::<f32>() / k,
            tick_max: ticks.iter().map(|t| t.ms).fold(0.0, f32::max),
            wait_avg: ticks.iter().map(|t| t.wait_ms).sum::<f32>() / k,
            commands: ticks.iter().map(|t| t.commands as f32).sum::<f32>() / secs,
            parts,
            paths,
        });
        self.counts = snapshot.map(Counts::of);
    }
}

impl Monitor {
    pub(super) fn simulation_tab(&self, ui: &mut egui::Ui) {
        let d = &self.data.sim;
        let last = d.seconds.last().cloned().unwrap_or_default();
        let tick = d.ticks.last().map_or(0, |t| t.tick);
        let target = if last.target == 0.0 { "paused".to_string() } else { fmt(last.target) };
        super::graph::stats(
            ui,
            "sim stats",
            &[
                ("tick", tick.to_string()),
                ("ticks/s", fmt(last.tps)),
                ("wanted", target),
                ("tick ms", fmt(last.tick_avg)),
                ("max ms", fmt(last.tick_max)),
                ("turn wait ms", fmt(last.wait_avg)),
                ("commands/s", fmt(last.commands)),
            ],
            3,
        );
        graph(
            ui,
            "Tick time",
            "ms",
            &[
                Line { name: "tick", colour: COLOURS[0], values: d.ticks.iter().map(|t| t.ms).collect() },
                Line { name: "waiting", colour: COLOURS[3], values: d.ticks.iter().map(|t| t.wait_ms).collect() },
            ],
            TICKS,
            5.0,
        );
        let per = |f: &dyn Fn(&SimSecond) -> f32| d.seconds.iter().map(f).collect::<Vec<f32>>();
        graph(ui, "Ticks per second", "", &[Line { name: "run", colour: COLOURS[2], values: per(&|s| s.tps) }, Line { name: "wanted", colour: COLOURS[1], values: per(&|s| s.target) }], SECONDS, 15.0);
        graph(ui, "Tick time per second", "ms", &[Line { name: "avg", colour: COLOURS[0], values: per(&|s| s.tick_avg) }, Line { name: "max", colour: COLOURS[3], values: per(&|s| s.tick_max) }], SECONDS, 5.0);
        // The tick's parts (s4_sim::debug): the costliest of the last second over time, then all.
        let mut costly: Vec<(&&'static str, &f32)> = last.parts.iter().collect();
        costly.sort_by(|a, b| b.1.total_cmp(a.1));
        let lines: Vec<Line> = costly
            .iter()
            .take(PART_LINES)
            .enumerate()
            .map(|(k, (n, _))| Line { name: n, colour: COLOURS[k % COLOURS.len()], values: d.seconds.iter().map(|s| s.parts.get(**n).copied().unwrap_or(0.0)).collect() })
            .collect();
        graph(ui, "Tick parts", "ms per tick", &lines, SECONDS, 1.0);
        egui::CollapsingHeader::new(format!("Tick parts, last second ({})", costly.len())).id_salt("tick_parts@sim_tab.rs").show(ui, |ui| {
            egui::Grid::new("parts").striped(true).min_col_width(48.0).show(ui, |ui| {
                ui.label(RichText::new("part").strong());
                ui.label(RichText::new("ms per tick").strong());
                ui.label(RichText::new("share").strong());
                ui.end_row();
                let total = last.tick_avg.max(1e-6);
                for (n, ms) in &costly {
                    ui.label(**n);
                    ui.label(fmt(**ms));
                    ui.label(format!("{:.0}%", **ms / total * 100.0));
                    ui.end_row();
                }
            });
        });
        egui::CollapsingHeader::new("Network").id_salt("network@sim_tab.rs").default_open(d.net.is_some()).show(ui, |ui| match &d.net {
            None => {
                ui.label("single player");
            }
            Some(n) => {
                let waiting = if n.missing.is_empty() { String::new() } else { format!(", waiting for {:?}", n.missing) };
                ui.label(format!("players {:?}, turn delay {} ticks, state hash every {} ticks, desyncs {}{waiting}", n.players, n.dt, n.hash_interval, d.desyncs));
            }
        });
        let Some(c) = &d.counts else { return };
        egui::CollapsingHeader::new(format!("Entities at tick {}", c.tick)).id_salt("entities_at_tick@sim_tab.rs").default_open(true).show(ui, |ui| {
            let players: Vec<usize> = (0..9).filter(|&p| c.settlers[p] + c.buildings[p] + c.sites[p] + c.vehicles[p] > 0).collect();
            egui::Grid::new("counts").striped(true).min_col_width(48.0).show(ui, |ui| {
                ui.label("");
                for p in &players {
                    ui.label(RichText::new(if *p == 0 { "none".to_string() } else { format!("player {p}") }).strong());
                }
                ui.label(RichText::new("all").strong());
                ui.end_row();
                for (name, row) in [("settlers", &c.settlers), ("buildings", &c.buildings), ("sites", &c.sites), ("vehicles", &c.vehicles)] {
                    ui.label(name);
                    for p in &players {
                        ui.label(row[*p].to_string());
                    }
                    ui.label(row.iter().sum::<u32>().to_string());
                    ui.end_row();
                }
            });
            ui.label(format!("{} animals, {} piles", c.animals, c.piles));
            egui::CollapsingHeader::new("Settlers by type").id_salt("settlers_by_type@sim_tab.rs").show(ui, |ui| {
                egui::Grid::new("types").striped(true).min_col_width(48.0).show(ui, |ui| {
                    ui.label("");
                    for p in &players {
                        ui.label(RichText::new(p.to_string()).strong());
                    }
                    ui.end_row();
                    for (k, row) in &c.by_type {
                        ui.label(format!("{k} {}", super::settler_name(*k)));
                        for p in &players {
                            ui.label(row[*p].to_string());
                        }
                        ui.end_row();
                    }
                });
            });
        });
    }
}
