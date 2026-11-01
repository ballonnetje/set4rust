//! Map geometry and the house worker's routine.

use crate::model::{Building, Seq, Step, WorkList, half_k};

/// World pixels of a cell offset: rows are 12 px apart, columns 24 px, shifted 12 px per row.
pub fn cell_px(c: [i32; 2]) -> [i32; 2] {
    [24 * c[0] - 12 * c[1], 12 * c[1]]
}

/// Cell offsets that exist (older specs carry unset work spots around 980).
pub fn is_cell(c: [i32; 2]) -> bool {
    c[0].abs() < 128 && c[1].abs() < 128
}

/// Nearest map node to world pixel (x, y).
pub fn snap_cell(x: f64, y: f64) -> [i32; 2] {
    let mut best = [0, 0];
    let mut bd = f64::INFINITY;
    for dy in [(y / 12.0).floor(), (y / 12.0).ceil()] {
        let dx = ((x + 12.0 * dy) / 24.0 + 0.5).floor();
        let c = [dx as i32, dy as i32];
        let p = cell_px(c);
        let d = (p[0] as f64 - x).powi(2) + (p[1] as f64 - y).powi(2);
        if d < bd {
            bd = d;
            best = c;
        }
    }
    best
}

/// Every world-pixel point a slot's data uses (for sizing views).
pub fn data_points(b: &Building) -> Vec<[i32; 2]> {
    let mut pts = Vec::new();
    let cells = b.footprint.iter().chain(&b.block).chain(&b.dig).copied().chain([b.door, b.flag]).chain(b.work).chain(b.builders.iter().map(|s| [s[0], s[1]])).chain(b.piles.iter().map(|p| p.cell));
    pts.extend(cells.filter(|c| is_cell(*c)).map(cell_px));
    pts.extend(b.soldiers_px.iter().map(|s| [s[0], s[1]]));
    pts.extend(b.mini_flag_px);
    for wl in b.work_lists.iter() {
        pts.extend(wl.steps.iter().map(|s| s.to));
    }
    pts
}

/// A view's world rectangle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub x0: f64,
    pub y0: f64,
    pub w: f64,
    pub h: f64,
}

/// Growing world extent.
#[derive(Clone, Copy, Debug)]
pub struct Extent {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl Extent {
    pub fn new(r: f64) -> Extent {
        Extent { x0: -r, y0: -r, x1: r, y1: r }
    }

    pub fn add(&mut self, x: f64, y: f64) {
        self.x0 = self.x0.min(x);
        self.y0 = self.y0.min(y);
        self.x1 = self.x1.max(x);
        self.y1 = self.y1.max(y);
    }

    pub fn pad(&self, pad: f64) -> Bounds {
        Bounds { x0: (self.x0 - pad).floor(), y0: (self.y0 - pad).floor(), w: (self.x1 - self.x0 + 2.0 * pad).ceil(), h: (self.y1 - self.y0 + 2.0 * pad).ceil() }
    }
}

/// The box a sequence card shows: every frame and the slot's data.
pub fn seq_box(seq: &Seq, building: Option<&Building>) -> Bounds {
    let mut e = Extent::new(12.0);
    for fr in &seq.frames {
        let k = half_k(fr);
        e.add(-fr[3] as f64 * k, -fr[4] as f64 * k);
        e.add((fr[1] - fr[3]) as f64 * k, (fr[2] - fr[4]) as f64 * k);
    }
    if let Some(b) = building {
        for [px, py] in data_points(b) {
            e.add(px as f64 - 8.0, py as f64 - 8.0);
            e.add(px as f64 + 8.0, py as f64 + 8.0);
        }
    }
    e.pad(6.0)
}

/// The zoom of a card: at most `zoom`, fitting 760 × 900 points, half steps above 1×.
pub fn card_zoom(zoom: f64, b: &Bounds) -> f64 {
    let z = (zoom.min(760.0 / b.w).min(900.0 / b.h)).max(0.25);
    if z >= 1.0 { (z * 2.0).floor() / 2.0 } else { z }
}

/// The routine's state at a tick.
#[derive(Clone, Debug)]
pub struct RoutineState<'a> {
    pub i: usize,
    pub step: &'a Step,
    pub dir: i32,
    pub frame: i32,
    pub pos: [f64; 2],
    pub t: i32,
}

/// Step, direction, position and animation frame at `tick` into the routine. Frame stepping of the
/// work tasks follows UpdateJob where it is ported (walk, put, get) and plays the cycle otherwise.
pub fn routine_at(wl: &WorkList, tick: i64) -> Option<RoutineState<'_>> {
    if wl.ticks == 0 {
        return None;
    }
    let t = tick.rem_euclid(wl.ticks as i64) as i32;
    let mut dir = 0;
    for (i, s) in wl.steps.iter().enumerate() {
        if s.dir >= 0 {
            dir = s.dir;
        }
        let len = s.ticks.unwrap_or(0);
        if t >= s.start + len && i + 1 < wl.steps.len() {
            continue;
        }
        let e = (t - s.start).max(0);
        let cyc = s.frames.max(1);
        let f0 = if s.forward { 0 } else { cyc - 1 };
        let task = s.task_name();
        let frame = match task {
            "GO_VIRTUAL" => {
                let f = (f0 + e) % cyc;
                if f == 0 && cyc > 1 { 1 } else { f }
            }
            "WORK_VIRTUAL" | "PRODUCE_VIRTUAL" => {
                if s.forward {
                    e % cyc
                } else {
                    cyc - 1 - (e % cyc)
                }
            }
            "PUT_GOOD_VIRTUAL" => (cyc - 1 - e).max(0),
            "GET_GOOD_VIRTUAL" => (cyc - 1).min(e),
            _ => f0,
        };
        let k = if task == "GO_VIRTUAL" && len > 0 { (e as f64 / len as f64).min(1.0) } else { 1.0 };
        let pos = [s.from[0] as f64 + (s.to[0] - s.from[0]) as f64 * k, s.from[1] as f64 + (s.to[1] - s.from[1]) as f64 * k];
        return Some(RoutineState { i, step: s, dir, frame, pos, t });
    }
    None
}

/// Timeline colour of a task.
pub fn task_color(task: &str) -> [u8; 3] {
    match task.strip_prefix("CEntityTask::").unwrap_or(task) {
        "GO_VIRTUAL" => [0xff, 0xd1, 0x66],
        "WORK_VIRTUAL" | "PRODUCE_VIRTUAL" => [0xef, 0x47, 0x6f],
        "WAIT_VIRTUAL" => [0x8d, 0x99, 0xae],
        "GET_GOOD_VIRTUAL" => [0x06, 0xd6, 0xa0],
        "PUT_GOOD_VIRTUAL" => [0x11, 0x8a, 0xb2],
        _ => [0x6c, 0x75, 0x7d],
    }
}

/// The tasks the legend lists.
pub const TASK_LEGEND: [(&str, &str); 5] =
    [("GO_VIRTUAL", "go"), ("WORK_VIRTUAL", "work"), ("WAIT_VIRTUAL", "wait"), ("GET_GOOD_VIRTUAL", "get good"), ("PUT_GOOD_VIRTUAL", "put good")];

/// The screen direction of a facing: the six hex directions as cell offsets.
pub fn facing_cell(dir: i32) -> [i32; 2] {
    [[1, 0], [1, 1], [0, 1], [-1, 0], [-1, -1], [0, -1]][dir.rem_euclid(6) as usize]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::test_spec;
    use std::path::Path;

    #[test]
    fn cells_and_pixels_agree() {
        assert_eq!(cell_px([3, 6]), [0, 72]);
        for c in [[0, 0], [1, 0], [0, 1], [-3, 4], [5, -2], [2, 2]] {
            let p = cell_px(c);
            assert_eq!(snap_cell(p[0] as f64, p[1] as f64), c);
            assert_eq!(snap_cell(p[0] as f64 + 3.0, p[1] as f64 - 2.0), c);
        }
        assert!(is_cell([127, -127]));
        assert!(!is_cell([980, 0]));
    }

    #[test]
    fn card_zoom_keeps_whole_steps_and_fits() {
        assert_eq!(card_zoom(2.0, &Bounds { x0: 0.0, y0: 0.0, w: 100.0, h: 100.0 }), 2.0);
        assert_eq!(card_zoom(6.0, &Bounds { x0: 0.0, y0: 0.0, w: 300.0, h: 100.0 }), 2.5);
        assert_eq!(card_zoom(2.0, &Bounds { x0: 0.0, y0: 0.0, w: 1520.0, h: 100.0 }), 0.5);
        assert_eq!(card_zoom(2.0, &Bounds { x0: 0.0, y0: 0.0, w: 8000.0, h: 100.0 }), 0.25);
    }

    #[test]
    fn data_extends_the_box() {
        let s = test_spec(Path::new("x"));
        let slot = &s.groups[0].slots[0];
        let b = slot.building.as_ref().unwrap();
        let bare = seq_box(&slot.seqs[1], None);
        let with = seq_box(&slot.seqs[1], Some(b));
        assert!(with.x0 <= bare.x0 && with.w >= bare.w);
        // The builders' spot (2, 2) lies at (24, 24) px; the mini flag at (10, -20).
        assert!(data_points(b).contains(&[24, 24]) && data_points(b).contains(&[10, -20]));
    }

    #[test]
    fn the_routine_walks_then_works() {
        let s = test_spec(Path::new("x"));
        let b = s.groups[0].slots[0].building.as_ref().unwrap();
        let wl = &b.work_lists[0];
        let st = routine_at(wl, 0).unwrap();
        assert_eq!((st.i, st.pos), (0, [0.0, 24.0]));
        assert_eq!(st.frame, 1, "walk frames skip frame 0");
        let st = routine_at(wl, 4).unwrap();
        assert_eq!(st.pos[0], 24.0 * 4.0 / 9.0);
        let st = routine_at(wl, 9).unwrap();
        assert_eq!((st.i, st.frame, st.pos), (1, 0, [24.0, 24.0]));
        // Wraps, also below zero.
        assert_eq!(routine_at(wl, 29).unwrap().t, 9);
        assert_eq!(routine_at(wl, -1).unwrap().t, 19);
        assert_eq!(routine_at(&WorkList { name: String::new(), ticks: 0, steps: vec![] }, 3).map(|s| s.t), None);
    }
}
