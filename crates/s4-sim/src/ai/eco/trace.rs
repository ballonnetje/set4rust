//! A record of every manager call of the eco AIs (an observer for the replay harness
//! `tools/re/script/ecoai_replay.py`, which runs the release engine on the recorded answers and
//! compares the calls it makes). Off unless [`start`] was called on this thread.

use super::manager::{CellValue, Manager};
use super::places::Place;
use crate::ai::AiEvent;
use std::cell::RefCell;
use std::fmt::Write;

/// Changes a scalar answer `(slot, args, answer) → answer` before it is recorded and handed to
/// the engine (bools as 0 / 1).
type Perturb = Box<dyn FnMut(i32, &[i32], i32) -> i32>;

thread_local! {
    static TRACE: RefCell<Option<String>> = const { RefCell::new(None) };
    static PERTURB: RefCell<Option<Perturb>> = const { RefCell::new(None) };
}

/// While recording, let `f` change the manager's scalar answers: the engine then runs paths the
/// world does not lead to, and the replay checks the release engine answers them the same way
/// (the record holds the changed answers). `None` turns it off.
pub fn perturb(f: Option<Perturb>) {
    PERTURB.with(|p| *p.borrow_mut() = f);
}

fn tweak(slot: i32, args: &[i32], v: i32) -> i32 {
    PERTURB.with(|p| match p.borrow_mut().as_mut() {
        Some(f) => f(slot, args, v),
        None => v,
    })
}

/// Start recording (an empty record).
pub fn start() {
    TRACE.with(|t| *t.borrow_mut() = Some(String::new()));
}

/// Stop and hand over the record.
pub fn take() -> Option<String> {
    TRACE.with(|t| t.borrow_mut().take())
}

pub fn on() -> bool {
    TRACE.with(|t| t.borrow().is_some())
}

/// One line.
pub fn line(s: &str) {
    TRACE.with(|t| {
        if let Some(b) = t.borrow_mut().as_mut() {
            b.push_str(s);
            b.push('\n');
        }
    });
}

fn place(s: &mut String, p: &Place) {
    let _ = write!(s, " {} {} {} {}", p.x, p.y, p.t, p.level);
    for c in p.classes {
        let _ = write!(s, " {c}");
    }
    for r in p.res {
        let _ = write!(s, " {r}");
    }
    let _ = write!(s, " {}", p.nodes.len());
    for &(pos, class) in &p.nodes {
        let _ = write!(s, " {pos} {class}");
    }
}

fn places(s: &mut String, list: &[Place]) {
    let _ = write!(s, " {}", list.len());
    for p in list {
        place(s, p);
    }
}

fn cells(s: &mut String, list: &[CellValue]) {
    let _ = write!(s, " {}", list.len());
    for c in list {
        let _ = write!(s, " {} {} {} {} {}", c.what, c.value, c.cx, c.cy, c.own);
    }
}

/// [`Manager`] that records every call of the one it wraps.
pub struct Recorder<'a> {
    pub inner: &'a mut dyn Manager,
}

impl Recorder<'_> {
    fn c(&self, slot: i32, args: &[i32], tail: &str) {
        let mut s = format!("c {slot:x}");
        for a in args {
            let _ = write!(s, " {a}");
        }
        s.push_str(" =");
        s.push_str(tail);
        line(&s);
    }

    fn r(&self, slot: i32, args: &[i32], v: i32) -> i32 {
        let v = tweak(slot, args, v);
        self.c(slot, args, &format!(" {v}"));
        v
    }

    fn b(&self, slot: i32, args: &[i32], v: bool) -> bool {
        let v = tweak(slot, args, v as i32) != 0;
        self.c(slot, args, if v { " 1" } else { " 0" });
        v
    }
}

impl Manager for Recorder<'_> {
    fn scan(&mut self, t: i32) -> i32 {
        let v = self.inner.scan(t);
        self.r(0xc, &[t], v)
    }
    fn rescan(&mut self, t: i32, cx: i32, cy: i32) -> i32 {
        let v = self.inner.rescan(t, cx, cy);
        self.r(0x8, &[t, cx, cy], v)
    }
    fn clear(&mut self, t: i32) -> bool {
        let v = self.inner.clear(t);
        self.b(0x10, &[t], v)
    }
    fn place_near(&mut self, level: i32, t: i32, b: [i32; 3]) -> Place {
        let p = self.inner.place_near(level, t, b);
        let mut s = String::new();
        place(&mut s, &p);
        self.c(0x18, &[level, t, b[0], b[1], b[2]], &s);
        p
    }
    fn places_around(&mut self, t: i32, p: &Place, out: &mut Vec<Place>, r: i32) -> i32 {
        let before = out.len();
        let v = self.inner.places_around(t, p, out, r);
        let mut s = format!(" {v}");
        places(&mut s, &out[before..]);
        self.c(0x1c, &[t, p.x, p.y, p.t, p.level, r], &s);
        v
    }
    fn places_of(&mut self, level: i32, t: i32, out: &mut Vec<Place>) -> i32 {
        let before = out.len();
        let v = self.inner.places_of(level, t, out);
        let mut s = format!(" {v}");
        places(&mut s, &out[before..]);
        self.c(0x20, &[level, t], &s);
        v
    }
    fn place_family(&mut self, t: i32, level: i32, p: &Place, out: &mut Vec<Place>) -> i32 {
        let before = out.len();
        let v = self.inner.place_family(t, level, p, out);
        let mut s = format!(" {v}");
        places(&mut s, &out[before..]);
        self.c(0x108, &[t, level, p.x, p.y, p.t, p.level], &s);
        v
    }
    fn remove_place(&mut self, t: i32, p: &Place) -> bool {
        let v = self.inner.remove_place(t, p);
        self.b(0x14, &[t, p.x, p.y, p.t, p.level], v)
    }
    fn can_build(&mut self, t: i32, x: i32, y: i32) -> bool {
        let v = self.inner.can_build(t, x, y);
        self.b(0x24, &[t, x, y], v)
    }
    fn overlap(&mut self, ta: i32, pa: i32, tb: i32, pb: i32) -> bool {
        let v = self.inner.overlap(ta, pa, tb, pb);
        self.b(0x28, &[ta, pa, tb, pb], v)
    }
    fn overlapping_building(&mut self, t: i32, pos: i32) -> i32 {
        let v = self.inner.overlapping_building(t, pos);
        self.r(0x2c, &[t, pos], v)
    }
    fn building_at(&mut self, x: i32, y: i32) -> i32 {
        let v = self.inner.building_at(x, y);
        self.r(0x30, &[x, y], v)
    }
    fn building_pos(&mut self, id: i32) -> Option<(i32, i32)> {
        let v = self.inner.building_pos(id);
        let tail = match v {
            Some((x, y)) => format!(" 1 {x} {y}"),
            None => " 0 0 0".to_string(),
        };
        self.c(0x34, &[id], &tail);
        v
    }
    fn pile_amount(&mut self, id: i32, good: i32) -> i32 {
        let v = self.inner.pile_amount(id, good);
        self.r(0x38, &[id, good], v)
    }
    fn cost(&mut self, t: i32, which: i32) -> i32 {
        let v = self.inner.cost(t, which);
        self.r(0x3c, &[t, which], v)
    }
    fn need(&mut self, id: i32, which: i32) -> i32 {
        let v = self.inner.need(id, which);
        self.r(0x40, &[id, which], v)
    }
    fn progress(&mut self, id: i32) -> i32 {
        let v = self.inner.progress(id);
        self.r(0x44, &[id], v)
    }
    fn kind(&mut self, id: i32) -> i32 {
        let v = self.inner.kind(id);
        self.r(0x48, &[id], v)
    }
    fn finished_working(&mut self, id: i32) -> bool {
        let v = self.inner.finished_working(id);
        self.b(0x4c, &[id], v)
    }
    fn working(&mut self, id: i32) -> bool {
        let v = self.inner.working(id);
        self.b(0x50, &[id], v)
    }
    fn worker_home(&mut self, id: i32) -> bool {
        let v = self.inner.worker_home(id);
        self.b(0x54, &[id], v)
    }
    fn beds(&mut self, t: i32) -> i32 {
        let v = self.inner.beds(t);
        self.r(0x58, &[t], v)
    }
    fn free_beds(&mut self, id: i32) -> i32 {
        let v = self.inner.free_beds(id);
        self.r(0x5c, &[id], v)
    }
    fn eco_of_building(&mut self, id: i32) -> i32 {
        let v = self.inner.eco_of_building(id);
        self.r(0x60, &[id], v)
    }
    fn construct(&mut self, t: i32, x: i32, y: i32) {
        self.inner.construct(t, x, y);
        self.r(0x64, &[t, x, y], 0);
    }
    fn destroy(&mut self, id: i32) {
        self.inner.destroy(id);
        self.r(0x68, &[id], 0);
    }
    fn plant_nodes(&mut self, t: i32, x: i32, y: i32) -> i32 {
        let v = self.inner.plant_nodes(t, x, y);
        self.r(0x6c, &[t, x, y], v)
    }
    fn set_working_area(&mut self, id: i32, x: i32, y: i32) -> bool {
        let v = self.inner.set_working_area(id, x, y);
        self.b(0x74, &[id, x, y], v)
    }
    fn working_area(&mut self, id: i32) -> (i32, i32) {
        let (x, y) = self.inner.working_area(id);
        self.c(0x78, &[id], &format!(" 1 {x} {y}"));
        (x, y)
    }
    fn working_radius(&mut self, t: i32) -> i32 {
        let v = self.inner.working_radius(t);
        self.r(0x7c, &[t], v)
    }
    fn player(&mut self) -> i32 {
        let v = self.inner.player();
        self.r(0x80, &[], v)
    }
    fn eco(&mut self) -> i32 {
        let v = self.inner.eco();
        self.r(0x84, &[], v)
    }
    fn race(&mut self) -> i32 {
        let v = self.inner.race();
        self.r(0x88, &[], v)
    }
    fn influence_radius(&mut self, t: i32) -> i32 {
        let v = self.inner.influence_radius(t);
        self.r(0x8c, &[t], v)
    }
    fn offered(&mut self, good: i32) -> i32 {
        let v = self.inner.offered(good);
        self.r(0x90, &[good], v)
    }
    fn worker_delta(&mut self, kind: i32) -> i32 {
        let v = self.inner.worker_delta(kind);
        self.r(0x94, &[kind], v)
    }
    fn settlers(&mut self, kind: i32) -> i32 {
        let v = self.inner.settlers(kind);
        self.r(0x98, &[kind], v)
    }
    fn count_buildings(&mut self, t: i32) -> i32 {
        let v = self.inner.count_buildings(t);
        self.r(0x9c, &[t], v)
    }
    fn buildings(&mut self, t: i32, out: &mut Vec<i32>) -> i32 {
        let v = self.inner.buildings(t, out);
        let mut s = format!(" {v} {}", out.len());
        for id in out.iter() {
            let _ = write!(s, " {id}");
        }
        self.c(0xa0, &[t], &s);
        v
    }
    fn cell_resource(&mut self, res: i32, cx: i32, cy: i32) -> i32 {
        let v = self.inner.cell_resource(res, cx, cy);
        self.r(0xa4, &[res, cx, cy], v)
    }
    fn cell_resource_around(&mut self, res: i32, cx: i32, cy: i32) -> i32 {
        let v = self.inner.cell_resource_around(res, cx, cy);
        self.r(0xa8, &[res, cx, cy], v)
    }
    fn resource_centre(&mut self, res: i32, sx: i32, sy: i32) -> Option<(i32, i32)> {
        let v = self.inner.resource_centre(res, sx, sy);
        let tail = match v {
            Some((x, y)) => format!(" 1 {x} {y}"),
            None => " 0 0 0".to_string(),
        };
        self.c(0xac, &[res, sx, sy], &tail);
        v
    }
    fn resource_in_square(&mut self, res: i32, sx: i32, sy: i32) -> i32 {
        let v = self.inner.resource_in_square(res, sx, sy);
        self.r(0xb0, &[res, sx, sy], v)
    }
    fn own_resource_cells(&mut self, res: i32, out: &mut Vec<CellValue>) {
        self.inner.own_resource_cells(res, out);
        let mut s = format!(" {}", out.len());
        cells(&mut s, out);
        self.c(0xb4, &[res], &s);
    }
    fn own_resource_cells_around(&mut self, res: i32, out: &mut Vec<CellValue>) {
        self.inner.own_resource_cells_around(res, out);
        let mut s = format!(" {}", out.len());
        cells(&mut s, out);
        self.c(0xb8, &[res], &s);
    }
    fn foreign_resource_cells(&mut self, res: i32, out: &mut Vec<CellValue>) {
        self.inner.foreign_resource_cells(res, out);
        let mut s = format!(" {}", out.len());
        cells(&mut s, out);
        self.c(0xbc, &[res], &s);
    }
    fn foreign_resource_cells_around(&mut self, res: i32, out: &mut Vec<CellValue>) {
        self.inner.foreign_resource_cells_around(res, out);
        let mut s = format!(" {}", out.len());
        cells(&mut s, out);
        self.c(0xc0, &[res], &s);
    }
    fn own_node(&mut self, x: i32, y: i32) -> bool {
        let v = self.inner.own_node(x, y);
        self.b(0xc4, &[x, y], v)
    }
    fn border_cell(&mut self, cx: i32, cy: i32) -> bool {
        let v = self.inner.border_cell(cx, cy);
        self.b(0xc8, &[cx, cy], v)
    }
    fn resource_near(&mut self, res: i32, cx: i32, cy: i32, r: i32) -> bool {
        let v = self.inner.resource_near(res, cx, cy, r);
        self.b(0xcc, &[res, cx, cy, r], v)
    }
    fn class_near(&mut self, class: i32, cx: i32, cy: i32, r: i32) -> bool {
        let v = self.inner.class_near(class, cx, cy, r);
        self.b(0xd0, &[class, cx, cy, r], v)
    }
    fn cell_bit(&mut self, slot: i32, cx: i32, cy: i32) -> bool {
        let v = self.inner.cell_bit(slot, cx, cy);
        self.b(slot, &[cx, cy], v)
    }
    fn resource_at(&mut self, res: i32, x: i32, y: i32) -> i32 {
        let v = self.inner.resource_at(res, x, y);
        self.r(0xec, &[res, x, y], v)
    }
    fn ground_is(&mut self, n: i32, x: i32, y: i32) -> bool {
        let v = self.inner.ground_is(n, x, y);
        self.b(0xf0, &[n, x, y], v)
    }
    fn cell_class(&mut self, class: i32, cx: i32, cy: i32) -> i32 {
        let v = self.inner.cell_class(class, cx, cy);
        self.r(0xf4, &[class, cx, cy], v)
    }
    fn own_class_cells(&mut self, class: i32, out: &mut Vec<CellValue>) {
        self.inner.own_class_cells(class, out);
        let mut s = format!(" {}", out.len());
        cells(&mut s, out);
        self.c(0xf8, &[class], &s);
    }
    fn foreign_class_cells(&mut self, class: i32, out: &mut Vec<CellValue>) {
        self.inner.foreign_class_cells(class, out);
        let mut s = format!(" {}", out.len());
        cells(&mut s, out);
        self.c(0xfc, &[class], &s);
    }
    fn border_cells(&mut self, out: &mut Vec<i32>) {
        self.inner.border_cells(out);
        let mut s = format!(" {} {}", out.len(), out.len());
        for v in out.iter() {
            let _ = write!(s, " {v}");
        }
        self.c(0x100, &[], &s);
    }
    fn water_node(&mut self, cx: i32, cy: i32) -> i32 {
        let v = self.inner.water_node(cx, cy);
        self.r(0x104, &[cx, cy], v)
    }
    fn produce(&mut self, id: i32, good: i32, amount: i32) {
        self.inner.produce(id, good, amount);
        self.r(0x10c, &[id, good, amount], 0);
    }
    fn recruit(&mut self, id: i32, kind: i32, amount: i32) {
        self.inner.recruit(id, kind, amount);
        self.r(0x110, &[id, kind, amount], 0);
    }
    fn switch(&mut self, id: i32) {
        self.inner.switch(id);
        self.r(0x114, &[id], 0);
    }
    fn net_message(&mut self, id: i32, a: i32, b: i32, player: i32, c: i32) {
        self.inner.net_message(id, a, b, player, c);
        self.r(0x118, &[id, a, b, player, c], 0);
    }
    fn player_carriers(&mut self) -> i32 {
        let v = self.inner.player_carriers();
        self.r(0x11c, &[], v)
    }
    fn total_beds(&mut self) -> i32 {
        let v = self.inner.total_beds();
        self.r(0x120, &[], v)
    }
    fn tick(&mut self) -> i32 {
        let v = self.inner.tick();
        self.r(0x124, &[], v)
    }
    fn spiral_end(&mut self, r: i32) -> i32 {
        let v = self.inner.spiral_end(r);
        self.r(0x128, &[r], v)
    }
    fn spiral_offset(&mut self, i: i32) -> (i32, i32) {
        let (dx, dy) = self.inner.spiral_offset(i);
        self.c(0x12c, &[i], &format!(" 0 {dx} {dy}"));
        (dx, dy)
    }
    fn own_building(&mut self, id: i32) -> bool {
        let v = self.inner.own_building(id);
        self.b(0x130, &[id], v)
    }
    fn min_carriers(&mut self) -> i32 {
        let v = self.inner.min_carriers();
        self.r(0x134, &[], v)
    }
    fn set_min_carriers(&mut self, n: i32) {
        self.inner.set_min_carriers(n);
        self.r(0x138, &[n], 0);
    }
    fn extra_carriers(&mut self) -> i32 {
        let v = self.inner.extra_carriers();
        self.r(0x13c, &[], v)
    }
    fn set_extra_carriers(&mut self, n: i32) {
        self.inner.set_extra_carriers(n);
        self.r(0x140, &[n], 0);
    }
    fn storage_space(&mut self, id: i32, good: i32) -> i32 {
        let v = self.inner.storage_space(id, good);
        self.r(0x144, &[id, good], v)
    }
    fn next_event(&mut self) -> Option<AiEvent> {
        let v = self.inner.next_event();
        match v {
            Some(e) => line(&format!("ev {} {} {} {}", e.kind, e.player, e.a, e.b)),
            None => line("ev -"),
        }
        v
    }
}
