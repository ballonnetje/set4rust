//! The build mode of `CInputProcessor` (Logic/CInputProcessor.cpp): a build-page button enters it
//! (`TryBuild`, event 0x1B5 with the building type: view state 4, icon layer on), the possible
//! sites show as markers (`GetPossibleBuildingSites` / `UpdateScreenPossibleBuildingSites`, see
//! `sim::Markers`), the node under the cursor shows the highlighted marker
//! (`ShowPossibleBuildingType`: helper − base + 18, mines 75), a left click on a possible site sends
//! command 5025 and leaves the mode (`CInputProcessor::Build`: `CheckForBuild(…) > 0`, then
//! `SendNetMessage(0x13A1, type, packedXY, player)`, `Default`, event 401), a right click leaves it
//! (FSM state 4, event 0x192).

use s4_sim::{Command, PlayerCommand};
use s4_viewer::client;
use s4_viewer::gpu::Gpu;
use s4_viewer::guistate::{UiEvent, build_button};

use crate::commands::{Stamped, execution_tick};
use crate::sim::{SimHandle, Snapshot};

pub struct BuildMode {
    /// The local player (whose sites are offered and who builds).
    pub player: u8,
    /// The building being placed.
    pub building: Option<u8>,
    /// The page button that entered the mode (released when the mode ends).
    button: Option<(usize, u16)>,
}

impl BuildMode {
    pub fn new(player: u8) -> BuildMode {
        BuildMode { player, building: None, button: None }
    }

    /// GUI events: a pressed build button enters the mode for its building, releasing it leaves.
    pub fn ui(&mut self, events: &[UiEvent], sim: &SimHandle) {
        for e in events {
            let &UiEvent::Command { dialog, link, effects } = e else { continue };
            let Some(t) = build_button(dialog, link) else { continue };
            if effects & 1 != 0 {
                self.building = Some(t);
                self.button = Some((dialog, link));
                sim.set_markers(Some((self.player, t)));
                tracing::info!("build mode: building type {t}");
            } else {
                self.building = None;
                self.button = None;
                sim.set_markers(None);
            }
        }
    }

    /// Leave the mode (right click, or after placing): markers off, the page button released.
    pub fn cancel(&mut self, gpu: &mut Gpu, sim: &SimHandle) {
        if self.building.take().is_some() {
            sim.set_markers(None);
        }
        if let Some((d, l)) = self.button.take() {
            client::release_control(gpu, d, l);
        }
    }

    /// Left click on map node `node`: place the building if the node holds a marker. Returns the
    /// command sent.
    pub fn click(&mut self, node: (usize, usize), snap: &Snapshot, gpu: &mut Gpu, sim: &SimHandle) -> Option<Stamped> {
        let t = self.building?;
        let m = snap.markers.as_ref().filter(|m| m.building == t)?;
        let (x, y) = (node.0 as u16, node.1 as u16);
        if !m.sites.iter().any(|s| (s.0, s.1) == (x, y)) {
            return None;
        }
        let s = Stamped { tick: execution_tick(snap.tick), command: PlayerCommand { player: self.player, command: Command::Build { building: t as u32, x, y } } };
        sim.command(s.clone());
        tracing::info!("build type {t} at {x},{y}, executes at tick {}", s.tick);
        self.cancel(gpu, sim);
        Some(s)
    }

    /// The building drawn under the cursor (`ShowPossibleBuildingType`): when the hovered node
    /// holds a marker, the building as finished at that node, `(x, y, race, type, player)`.
    pub fn ghost(&self, snap: &Snapshot, hover: Option<(usize, usize)>) -> Option<(u16, u16, u8, u8, u8)> {
        let t = self.building?;
        let (x, y) = hover.map(|(x, y)| (x as u16, y as u16))?;
        let m = snap.markers.as_ref().filter(|m| m.building == t)?;
        m.sites.iter().any(|s| (s.0, s.1) == (x, y)).then(|| (x, y, snap.population.races[self.player as usize], t, self.player))
    }

    /// The markers to draw: the snapshot's, the one under the cursor highlighted.
    pub fn display(&self, snap: &Snapshot, hover: Option<(usize, usize)>) -> Vec<(u16, u16, u8)> {
        let Some(t) = self.building else { return Vec::new() };
        let Some(m) = snap.markers.as_ref().filter(|m| m.building == t) else { return Vec::new() };
        let h = hover.map(|(x, y)| (x as u16, y as u16));
        m.sites
            .iter()
            .map(|&(x, y, icon)| {
                let icon = if Some((x, y)) != h {
                    icon
                } else if icon == 74 {
                    75
                } else {
                    icon - 10 + 18
                };
                (x, y, icon)
            })
            .collect()
    }
}
