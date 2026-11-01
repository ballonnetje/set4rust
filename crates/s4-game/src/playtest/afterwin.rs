//! Playtest save `afterwin.s4rs`: P4, P5.

use super::load;
use s4_sim::entity::{EntityData, ObjType};
use s4_sim::World;

/// Exploration: buildings and sites standing on another player's land.
#[test]
#[ignore = "exploration"]
fn afterwin_explore() {
    for name in ["afterwin.s4rs", "towerbug.s4rs"] {
        let Some(w) = load(name) else { continue };
        eprintln!("{name}: tick {}", w.tick);
        for (id, e) in w.entities.iter().filter(|(_, e)| e.obj_type == ObjType::Building) {
            let n = w.grid.index(e.x() as u32, e.y() as u32);
            let land = w.owner_of(n);
            let site = w.sites.roles.contains_key(&id);
            let around: std::collections::BTreeMap<u8, u32> = s4_sim::spiral::walk(w.grid, e.x(), e.y(), 4).fold(Default::default(), |mut m, (x, y)| {
                *m.entry(w.owner_of(w.grid.index(x as u32, y as u32))).or_default() += 1;
                m
            });
            if site || land != e.owner {
                eprintln!("  around {id}: {around:?}");
                let site = w.sites.roles.get(&id).map(|s| s.state);
                let EntityData::Building(b) = &e.data else { continue };
                eprintln!("  {id} p{} race {} k{} type_ex {} at ({},{}) on land of p{land}, site state {site:?}, flags {:#x}, node flags {:#x}", e.owner, e.race, e.kind, b.type_ex, e.x(), e.y(), e.flags, w.layers.flags[n]);
            }
        }
    }
}

/// Exploration P4: how many of player 1's settlers move in 2000 ticks, by role.
#[test]
#[ignore = "exploration"]
fn afterwin_who_moves() {
    let Some(mut w) = load("afterwin.s4rs") else { return };
    let snap = |w: &World| -> std::collections::BTreeMap<u16, (u32, u8, u16, u32)> {
        w.entities
            .iter()
            .filter(|(_, e)| e.obj_type == ObjType::Settler && e.owner == 1)
            .map(|(i, e)| {
                let role = match &e.data {
                    EntityData::Settler(d) => d.job.role,
                    _ => 0,
                };
                (i, (e.pos.0, role, e.kind, e.flags))
            })
            .collect()
    };
    let a = snap(&w);
    for (i, e) in w.entities.iter().filter(|(_, e)| e.obj_type == ObjType::Settler && e.owner == 1 && matches!(e.kind, 2 | 3 | 4 | 6)) {
        if let EntityData::Settler(d) = &e.data {
            let home = w.entities.by_index(d.job.home as u32).map(|h| (h.obj_type, h.kind, h.flags & s4_sim::entity::flag::DIED != 0));
            eprintln!("s{i} k{} ({},{}) flags {:#x} role {} task {} home {} {home:?} events {} todo {:?}", e.kind, e.x(), e.y(), e.flags, d.job.role, d.job.task, d.job.home, d.job.events.len(), d.job.todo);
        }
    }
    let eco = |w: &World, x: i32, y: i32| w.grid.checked_index(x, y).map_or(0, |n| w.tiling.eco_sector_of(n));
    for (id, e) in w.entities.iter().filter(|(id, e)| e.obj_type == ObjType::Building && e.owner == 1 && w.sites.roles.contains_key(id)) {
        let s = &w.sites.roles[&id];
        let EntityData::Building(b) = &e.data else { continue };
        eprintln!("site {id} k{} at ({},{}) state {} eco {} (ensign eco {}) builders {:?}", e.kind, e.x(), e.y(), s.state, eco(&w, e.x(), e.y()), eco(&w, b.ensign.x(), b.ensign.y()), s);
    }
    if let Some(s) = w.sites.roles.get(&1183) {
        for &p in &s.piles {
            eprintln!("pile {p}: {:?}", s4_sim::pile::get(&w, p).map(|d| (d.good, d.amount, d.leaving, d.coming)));
        }
        let pr = s4_sim::build::order::site_priority(&w, 1183);
        if let Some(e) = w.economy.get(3) {
            eprintln!("prio {pr:?}; eco 3: needed_prio_builders {} settlers[3] {} settlers[2] {}", e.needed_prio_builders, e.settlers[3], e.settlers[2]);
        }
    }
    for (i, e) in w.entities.iter().filter(|(_, e)| e.obj_type == ObjType::Settler && e.owner == 1 && matches!(e.kind, 2 | 3)).take(6) {
        eprintln!("worker {i} k{} eco {}", e.kind, eco(&w, e.x(), e.y()));
    }
    for _ in 0..2000 {
        s4_sim::tick(&mut w, &[]);
    }
    let b = snap(&w);
    let mut by: std::collections::BTreeMap<(u8, u16), (u32, u32, u32)> = Default::default();
    for (i, &(p, r, k, f)) in &a {
        let c = by.entry((r, k)).or_default();
        c.0 += 1;
        if b.get(i).is_some_and(|q| q.0 != p) {
            c.1 += 1;
        }
        if f & s4_sim::entity::flag::REGISTERED != 0 {
            c.2 += 1;
        }
    }
    for ((r, k), (n, moved, reg)) in by {
        eprintln!("role {r} kind {k}: {n} settlers, {moved} moved, {reg} registered at start");
    }
}
