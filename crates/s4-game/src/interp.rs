//! Smooth movement at any frame rate: entity positions interpolated between the last two
//! snapshots. Rendering runs one tick behind the simulation: at `alpha` = 0 an entity is drawn
//! where the previous snapshot had it, at 1 where the current one has it.

use std::collections::HashMap;
use std::time::Instant;

use s4_viewer::population::Population;
use s4_viewer::terrain::node_pos;

use crate::sim::Snapshot;

/// Fraction of the way from `cur`'s publication to the next tick (0..=1; 1 when paused).
pub fn alpha(cur: &Snapshot, now: Instant) -> f32 {
    if cur.tick_ms == 0 {
        return 1.0;
    }
    (now.saturating_duration_since(cur.at).as_secs_f32() * 1000.0 / cur.tick_ms as f32).clamp(0.0, 1.0)
}

/// World-px anchor of every unit (settlers outside buildings, vehicles) by entity id.
fn anchors(s: &Snapshot) -> HashMap<u16, [f32; 2]> {
    let p = &s.population;
    let h = |x: u16, y: u16| p.height.get(y as usize * p.size + x as usize).copied().unwrap_or(0);
    p.settlers.iter().chain(&p.vehicles).filter(|u| u.id != 0).map(|u| (u.id, node_pos(u.x as usize, u.y as usize, h(u.x, u.y)))).collect()
}

/// Offsets (world px) to add to the positions the sprite scene was built with (`base`) so that
/// each unit is drawn at `prev + (cur − prev)·alpha`. Only units present in all three get one:
/// the sprite scene is built once at load, so units created later are not drawn yet.
#[cfg(test)]
pub fn unit_offsets(base: &Snapshot, prev: &Snapshot, cur: &Snapshot, alpha: f32) -> HashMap<u16, (f32, f32)> {
    Offsets::default().get(base, prev, cur, alpha).collect()
}

/// A unit's id and its anchors in `base`, `prev`, `cur`.
type Anchored = (u16, [f32; 2], [f32; 2], [f32; 2]);

/// [`unit_offsets`] frame after frame: the anchors are kept while the three snapshots stay the
/// same (they change once a tick; frames come more often).
#[derive(Default)]
pub struct Offsets {
    /// The snapshots the anchors are from (tick, publication time).
    key: Option<[(u32, Instant); 3]>,
    /// The units in all three: id and anchor in `base`, `prev`, `cur`.
    units: Vec<Anchored>,
}

impl Offsets {
    pub fn get(&mut self, base: &Snapshot, prev: &Snapshot, cur: &Snapshot, alpha: f32) -> impl Iterator<Item = (u16, (f32, f32))> + '_ {
        let key = [base, prev, cur].map(|s| (s.tick, s.at));
        if self.key != Some(key) {
            let (o, a, b) = (anchors(base), anchors(prev), anchors(cur));
            self.units = b.iter().filter_map(|(id, q)| Some((*id, *o.get(id)?, *a.get(id)?, *q))).collect();
            self.key = Some(key);
        }
        self.units.iter().filter_map(move |&(id, z, p, q)| {
            let d = (p[0] + (q[0] - p[0]) * alpha - z[0], p[1] + (q[1] - p[1]) * alpha - z[1]);
            (d != (0.0, 0.0)).then_some((id, d))
        })
    }
}

/// Whether two populations place the same things (everything but the tick): if not, the
/// sprite scene is rebuilt from the newer one.
pub fn same_content(a: &Population, b: &Population) -> bool {
    a.decos == b.decos
        && a.buildings == b.buildings
        && a.settlers == b.settlers
        && a.vehicles == b.vehicles
        && a.animals == b.animals
        && a.piles == b.piles
        && a.owner == b.owner
        && a.height == b.height
}

#[cfg(test)]
mod tests {
    use super::*;
    use s4_viewer::population::Unit;

    fn snap(tick: u32, x: u16) -> Snapshot {
        let u = Unit { id: 7, x, y: 10, kind: 1, owner: 1, race: 0, ..Default::default() };
        let population = Population {
            tick,
            size: 64,
            height: vec![0; 64 * 64],
            ground: vec![16; 64 * 64],
            gradient: vec![8; 64 * 64],
            gfx_bits: vec![0; 64 * 64],
            owner: vec![0; 64 * 64],
            settlers: vec![u],
            ..Default::default()
        };
        Snapshot { tick, at: Instant::now(), tick_ms: 71, population: std::sync::Arc::new(population), hash: 0, fog: None, markers: None, building: None, ambient: None }
    }

    #[test]
    fn moving_unit_is_drawn_between_ticks() {
        let (p, c) = (snap(1, 10), snap(2, 11));
        // Scene built from `p`: at alpha 0 the unit is still where it was, halfway at 0.5, one node
        // (24 px) right at 1.
        assert!(unit_offsets(&p, &p, &c, 0.0).is_empty());
        assert_eq!(unit_offsets(&p, &p, &c, 0.5).get(&7), Some(&(12.0, 0.0)));
        assert_eq!(unit_offsets(&p, &p, &c, 1.0).get(&7), Some(&(24.0, 0.0)));
        // Scene built from `c`: no offset once the unit has arrived.
        assert!(unit_offsets(&c, &p, &c, 1.0).is_empty());
    }
}
