//! The Pathfinding tab: the path searches per tick (`s4_sim::debug::PathCounts`: coarse over the
//! tiling and fine over the nodes, with failures) and the paths themselves drawn over the map —
//! the selected units', and optionally every walker's of one player (`s4_sim::debug::path_of`:
//! the unit's node, its fine steps, then the coarse waypoints at tile centres, then the goal).

use egui::{Color32, Pos2, Stroke};
use s4_viewer::gpu::View;

use super::graph::{COLOURS, Line, fmt, graph};
use super::{Monitor, SECONDS};

/// Where a node is in the world (px at zoom 1, its height included: the scene's `node_world`).
pub type NodeWorld<'a> = &'a dyn Fn(i32, i32) -> [f32; 2];

/// The path drawing's options.
#[derive(Clone, Copy, Debug, Default)]
pub struct PathOptions {
    pub selected: bool,
    /// Every walker of this player (0: off).
    pub all_of: u8,
}

impl Monitor {
    pub(super) fn paths_tab(&mut self, ui: &mut egui::Ui) {
        let d = &self.data.sim;
        let last = d.seconds.last().cloned().unwrap_or_default();
        let [coarse, coarse_failed, partial, fine, fine_failed, waypoints, steps] = last.paths;
        let tps = last.tps.max(1e-3);
        ui.label(format!(
            "last second: {} coarse searches ({} failed, {} partial, {} waypoints each), {} fine ({} failed, {} steps each); {} a tick",
            coarse,
            coarse_failed,
            partial,
            fmt(waypoints as f32 / coarse.max(1) as f32),
            fine,
            fine_failed,
            fmt(steps as f32 / fine.max(1) as f32),
            fmt((coarse + fine) as f32 / tps)
        ));
        let per = |k: usize| d.seconds.iter().map(|s| s.paths[k] as f32).collect::<Vec<f32>>();
        graph(
            ui,
            "Path searches per second",
            "",
            &[
                Line { name: "coarse", colour: COLOURS[0], values: per(0) },
                Line { name: "fine", colour: COLOURS[2], values: per(3) },
                Line { name: "coarse failed", colour: COLOURS[3], values: per(1) },
                Line { name: "fine failed", colour: COLOURS[1], values: per(4) },
            ],
            SECONDS,
            10.0,
        );
        ui.separator();
        ui.checkbox(&mut self.paths.selected, "Draw the selected units' paths");
        ui.horizontal(|ui| {
            let mut all = self.paths.all_of != 0;
            ui.checkbox(&mut all, "Draw every walker's path of player");
            if !all {
                self.paths.all_of = 0;
            } else if self.paths.all_of == 0 {
                self.paths.all_of = self.economy_player.max(1);
            }
            if all {
                for p in self.players.clone() {
                    ui.selectable_value(&mut self.paths.all_of, p, p.to_string());
                }
            }
        });
        let shown = &self.data.sim.views.paths;
        if !shown.is_empty() {
            let n = shown.len() as f32;
            let nodes: usize = shown.iter().map(|(_, p)| p.len()).sum();
            let length: f32 = shown.iter().map(|(_, p)| p.windows(2).map(|w| (((w[1].0 - w[0].0).pow(2) + (w[1].1 - w[0].1).pow(2)) as f32).sqrt()).sum::<f32>()).sum();
            ui.label(format!("{} paths drawn: {} points, {} nodes long on average", shown.len(), fmt(nodes as f32 / n), fmt(length / n)));
        }
    }

    /// The paths to draw, in world px (projected when the frame is drawn, with its camera).
    pub(super) fn place_paths(&mut self, world: Option<NodeWorld>) {
        self.path_lines.clear();
        let Some(world) = world else { return };
        for (k, (_, path)) in self.data.sim.views.paths.iter().enumerate() {
            let pts: Vec<[f32; 2]> = path.iter().map(|&(x, y)| world(x, y)).collect();
            if pts.len() >= 2 {
                self.path_lines.push((pts, COLOURS[k % COLOURS.len()]));
            }
        }
    }

    /// The paths' shapes for the frame drawn with `view` (`ppp` px per point, `size` px): under the
    /// monitor's windows, over the game.
    pub(super) fn path_shapes(&self, view: View, ppp: f32, size: (u32, u32)) -> Vec<egui::epaint::ClippedShape> {
        let clip = egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(size.0 as f32, size.1 as f32) / ppp);
        let at = |p: [f32; 2]| Pos2::new((p[0] - view.x) * view.zoom / ppp, (p[1] - view.y) * view.zoom / ppp);
        let mut out = Vec::new();
        for (pts, c) in &self.path_lines {
            let pts: Vec<Pos2> = pts.iter().map(|&p| at(p)).collect();
            let (first, last) = (pts[0], *pts.last().unwrap());
            for shape in [
                egui::Shape::line(pts, Stroke::new(2.0_f32, *c)),
                // The unit's end and the goal.
                egui::Shape::circle_filled(first, 3.0, *c),
                egui::Shape::circle_stroke(last, 5.0, Stroke::new(2.0_f32, Color32::WHITE)),
            ] {
                out.push(egui::epaint::ClippedShape { clip_rect: clip, shape });
            }
        }
        out
    }
}
