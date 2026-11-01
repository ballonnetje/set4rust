//! `CSiedlerAI` (vtable 0x106ccd4, 0x48 bytes, ctor 0x645520): the operators of the planner
//! ([`super::plan`]) for one main problem and its sub-problems. Fields: +0x1c the event it plans
//! for, +0x20 the production chain, +0x24 the status the last search left (0 found, 1 a needed
//! place scan is missing, 2 a sub-problem was posted, 3 nothing possible), +0x40 race, +0x44 a
//! position the chain should keep near (the main problem's third word).
//!
//! Operators (edges in [`super::plan::EDGES`]): 1 build at a reservation or node (a leaf; its
//! execution posts a building event), 3 a production chain for a good (problem 10), 4 a
//! material (problem 3: the producers' clusters), 5 the next building of a chain (problem 4), 6
//! a building beside a reservation (problem 5), 8 a building near another (problem 8), 9 a
//! building near a node (problem 9), 10 a castle towards an expansion target (problem 0xb).
//! Condition kind 0x14 has no edge: it fails.

use super::board::Record;
use super::engine::{Data, Engine, rec};
use super::filter::{Filter, Scored};
use super::manager::Manager;
use super::places::Place;
use super::plan::{self, Cond, Module, Opts, Param, Planner, Solution, add_cond, f32_round};
use super::problems;
use super::scores::{self, best_node, place_score};
use super::tables::{CHAIN_OF, CHAINS};

/// The solver's own fields.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Siedler {
    pub id: i32,
    pub chain: i32,
    pub status: i32,
    pub race: i32,
    pub pos: i32,
    pub module: Module,
}

impl Siedler {
    /// 0x645520 `(engine, id)`: the race from the manager (slot 0x88).
    pub fn new(m: &mut dyn Manager, id: i32) -> Siedler {
        let race = m.race();
        Siedler { id, chain: 0, status: 0, race, pos: 0, module: Module::default() }
    }
}

/// The `k`-th building type of a chain (0x1455eec + chain · 0x20 + 4k; chain 1 is the first
/// table row).
pub fn chain_type(chain: i32, k: i32) -> i32 {
    if chain < 1 || !(0..5).contains(&k) {
        return 0;
    }
    CHAINS.get((chain - 1) as usize).map_or(0, |c| c.1[(2 + k) as usize])
}

/// 0x1457c18: the chain of a good for a race.
pub fn chain_of(race: i32, good: i32) -> i32 {
    usize::try_from(race).ok().and_then(|r| CHAIN_OF.get(r)).and_then(|row| row.get(usize::try_from(good).ok()?)).copied().unwrap_or(0)
}

/// 0x647e70 `(event)`: the building types whose place scans the problem needs (only the first
/// slot is ever set): kind 10 the chain's first type (and the solver's chain is set), kind 4
/// the chain's type `a`, kinds 5 / 8 / 9 the type `a`, kind 0xb the castle.
pub fn invoke_types(s: &mut Siedler, kind: i32, a: i32) -> [i32; 6] {
    let mut out = [0; 6];
    match kind {
        10 => {
            s.chain = chain_of(s.race, a);
            out[0] = chain_type(s.chain, 0);
        }
        4 => out[0] = chain_type(s.chain, a),
        5 | 8 | 9 => out[0] = a,
        0xb => out[0] = 0x2e,
        _ => {}
    }
    out
}

/// 0x647f50 `(kind, _, b, _, d)`: the cell of a sub-problem's reference: kinds 4 / 5 the place
/// of reservation `d` (`c << 16 | b`), kind 8 that of `b` divided by 16, kind 9 `b`.
pub fn sub_cell(board: &super::board::Board, kind: i32, b: i32, d: i32) -> i32 {
    match kind {
        4 | 5 => {
            let (x, y) = board.place(d);
            (y << 16) + x
        }
        8 => {
            let (x, y) = board.place(b);
            ((y / 16) << 16) + x / 16
        }
        9 => b,
        _ => 0,
    }
}

/// A scored place (`CFilterPlaceScoring` 0x106cc88, 0x38 bytes): +0x10 type, +0x14 / +0x18 the
/// cell, +0x20 the score (a double of an int), +0x28 the node, +0x2c / +0x34 a reservation
/// (castles), +0x30 the node's class.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct Scoring {
    t: i32,
    x: i32,
    y: i32,
    score: i32,
    pos: i32,
    w2c: i32,
    class: u8,
}

impl Scored for Scoring {
    /// 0x6484c0: the double through a float, truncated.
    fn score(&self) -> i32 {
        f32_round(self.score) as i32
    }
}

/// The planner on the world: the engine (scheduler, board), the manager and the solver.
pub struct Ctx<'a> {
    pub e: &'a mut Engine,
    pub m: &'a mut dyn Manager,
    pub ai: usize,
}

impl Ctx<'_> {
    fn s(&mut self) -> &mut Siedler {
        self.e.solvers[self.ai].as_mut().expect("live solver")
    }

    fn status(&mut self, v: i32) {
        self.s().status = v;
    }

    /// The invoke event of a type exists with its places ready (+0x30 == 0).
    fn places_ready(&self, t: i32) -> bool {
        super::tasks::find_invoke(self.e, t).is_some_and(|k| matches!(&self.e.s.list[k].d, Data::Invoke(v) if v.state == 0))
    }

    /// 0x6519d0: every building event as a record (type, cell, node).
    fn event_records(&self) -> Vec<Record> {
        self.e
            .s
            .list
            .iter()
            .filter_map(|x| match &x.d {
                Data::Building(b) if x.state != 3 => Some(Record { a: b.kind, b: b.cx, c: b.cy, id: 0, d: b.pos, block: 0 }),
                _ => None,
            })
            .collect()
    }

    /// The candidates of one place into the filter.
    #[allow(clippy::too_many_arguments)]
    fn score_place(&mut self, f: &mut Filter<Scoring>, t: i32, q: &Place, recs: &[Record], near: i32, flag: bool, w2c: i32) {
        let far = self.s().pos;
        if let Some((score, idx)) = best_node(self.e, self.m, t, q, recs, near, far, flag) {
            let (pos, class) = q.nodes[idx];
            f.insert(Scoring { t: q.t, x: q.x, y: q.y, score, pos: pos as i32, w2c: if q.t == 0x2e { w2c } else { 0 }, class });
        }
    }

    /// Reserve the filter's places under groups `g0 + i`; the parameters per id.
    fn reserve(&mut self, f: &Filter<Scoring>, g0: i32, mk: impl Fn(&Scoring, i32) -> Param) -> Vec<Param> {
        let mut out = Vec::new();
        for (i, it) in f.entries().iter().enumerate() {
            let id = self.e.board.reserve(it.t, it.x, it.y, it.pos, it.class as i8, it.score, g0 + i as i32);
            out.push(mk(it, id));
        }
        out
    }

    /// The existing reservations of groups `first`, `first + 1`, … (type `t`, 0 any) as long as
    /// they exist, from id `id` of the first.
    fn existing(&self, mut id: i32, first: i32, t: i32, mk: impl Fn(i32) -> Param) -> Vec<Param> {
        let mut out = Vec::new();
        let mut g = first + 1;
        while id != 0 {
            out.push(mk(id));
            id = self.e.board.id_of(g, t);
            g += 1;
        }
        out
    }

    /// The radius of a type's place search: record word 2 / 16 + 1.
    fn radius(&self, t: i32) -> i32 {
        rec(self.e.s.race, t, 2) / 16 + 1
    }

    fn places_around(&mut self, t: i32, q: &Place, out: &mut Vec<Place>, r: i32) -> i32 {
        self.m.places_around(t, q, out, r)
    }

    /// 0x2484e0 `(t, id, near_id, flag)`: the score of reservation `id` for type `t`, near
    /// reservation `near_id`'s node.
    fn reserved_score(&mut self, t: i32, id: i32, near_id: i32, flag: bool) -> i32 {
        let (pos, class) = self.e.board.get(id).map_or((0, 0), |e| (e.d, e.e as i32));
        let near = self.e.board.get(near_id).map_or(0, |e| e.d);
        let far = self.s().pos;
        place_score(self.e, self.m, t, pos, near, far, flag, class)
    }

    fn op3(&mut self, p: &Param) -> Option<Vec<Param>> {
        let race = self.s().race;
        let chain = chain_of(race, p.a);
        let mut flag = false;
        {
            let s = self.s();
            s.chain = chain;
            if p.c != 0 {
                s.pos = p.c;
                flag = chain_type(chain, 1) == 0;
            } else {
                s.pos = 0;
            }
        }
        let t0 = chain_type(chain, 0);
        if self.e.board.id_of(1, t0) != 0 {
            let id = self.e.board.id_of(1, t0);
            return Some(self.existing(id, 1, t0, |id| Param::new(0, id, 0, 0)));
        }
        if !self.places_ready(t0) {
            self.status(1);
            return Some(Vec::new());
        }
        let mut tops = Vec::new();
        self.m.places_of(0x40, t0, &mut tops);
        let mut recs = self.event_records();
        self.e.board.path_records(0, &mut recs);
        let mut f = Filter::new(5);
        for top in &tops {
            let mut cells = Vec::new();
            self.m.place_family(t0, 0x10, top, &mut cells);
            for q in &cells {
                self.score_place(&mut f, t0, q, &recs, 0, flag, 0);
            }
        }
        let out = self.reserve(&f, 1, |_, id| Param::new(0, id, 0, 0));
        if out.is_empty() {
            self.status(3);
        }
        Some(out)
    }

    fn op4(&mut self, p: &Param) -> Option<Vec<Param>> {
        let t = scores::producer(p.a);
        let mut members = Vec::new();
        let mut total = 0;
        scores::production_tree(self.e, t, &mut members, 0, &mut total);
        let mut out = Vec::new();
        let mut none = true;
        let mut fresh = false;
        for (k, mem) in members.iter().enumerate() {
            let g = k as i32 + 1;
            if self.e.board.id_of(1, 0) != 0 && !fresh {
                let id = self.e.board.id_of(g, 0);
                if id != 0 {
                    out.push(Param::new(t, id, 0, mem.building));
                    none = false;
                }
                continue;
            }
            fresh = true;
            let id = self.e.board.reserve(mem.kind, mem.pos & 0xffff, mem.pos >> 16, mem.pos, 0, 0, g);
            out.push(Param::new(mem.kind, id, 0, mem.building));
            none = false;
        }
        if none {
            self.status(3);
        }
        Some(out)
    }

    fn op5(&mut self, p: &Param) -> Option<Vec<Param>> {
        let k = p.a;
        let Some(e) = self.e.board.get(p.d).cloned() else {
            self.status(3);
            return None;
        };
        let chain = self.s().chain;
        let t = chain_type(chain, k);
        let flag = self.s().pos != 0 && chain_type(chain, k + 1) == 0;
        let q0 = Place { x: e.b, y: e.c, t: e.a, level: 0x10, ..Place::default() };
        let g1 = e.group * 10 + 1;
        let id = self.e.board.id_of(g1, t);
        if id != 0 {
            return Some(self.existing(id, g1, t, |id| Param::new(k, id, 0, p.d)));
        }
        if !self.places_ready(t) {
            self.status(3);
            return Some(Vec::new());
        }
        let r = self.radius(t);
        let mut cells = Vec::new();
        let mut n = self.places_around(t, &q0, &mut cells, r);
        if n < 2 {
            n = self.places_around(t, &q0, &mut cells, 3);
        }
        let mut f = Filter::new(5);
        let mut recs = self.event_records();
        self.e.board.path_records(e.group, &mut recs);
        for q in cells.iter().take(n.max(0) as usize) {
            self.score_place(&mut f, t, q, &recs, e.d, flag, p.d);
        }
        let out = self.reserve(&f, g1, |_, id| Param::new(k, id, 0, p.d));
        if out.is_empty() {
            self.status(3);
        }
        Some(out)
    }

    fn op6(&mut self, p: &Param) -> Option<Vec<Param>> {
        let t = p.a;
        let pid = p.d;
        let e = self.e.board.get(pid).cloned();
        let (b, c, a, group, near) = e.as_ref().map_or((0, 0, 0, 0, 0), |e| (e.b, e.c, e.a, e.group, e.d));
        let q0 = Place { x: b, y: c, t: a, level: 0x10, ..Place::default() };
        let g1 = group * 10 + 1;
        let id = self.e.board.id_of(g1, t);
        if id != 0 {
            return Some(self.existing(id, g1, t, |id| Param::new(t, id, 0, pid)));
        }
        let mut recs = self.event_records();
        self.e.board.path_records(pid, &mut recs);
        if !self.places_ready(t) {
            self.status(1);
            return Some(Vec::new());
        }
        let r = self.radius(t);
        let mut cells = Vec::new();
        let mut n = self.places_around(t, &q0, &mut cells, r);
        if n == 0 {
            n = self.places_around(t, &q0, &mut cells, 3);
        }
        let mut f = Filter::new(5);
        for (i, q) in cells.iter().take(n.max(0) as usize).enumerate() {
            // The release frees the places' node lists after the first one and keeps reading
            // them: only the first place has nodes.
            if i == 0 {
                self.score_place(&mut f, t, q, &recs, near, true, pid);
            } else {
                let mut q = q.clone();
                q.nodes.clear();
                self.score_place(&mut f, t, &q, &recs, near, true, pid);
            }
        }
        let out = self.reserve(&f, g1, |_, id| Param::new(t, id, 0, pid));
        if out.is_empty() {
            self.status(3);
        }
        Some(out)
    }

    fn op8(&mut self, p: &Param) -> Option<Vec<Param>> {
        let t = p.a;
        let (bx, by) = self.e.board.place(p.b);
        let group = self.e.board.get(p.b).map_or(0, |e| e.group);
        let g1 = group * 10 + 1;
        if self.e.board.id_of(g1, 0) != 0 {
            let id = self.e.board.id_of(g1, 0);
            return Some(self.existing(id, g1, 0, |id| Param::new(t, id, 0, 0)));
        }
        let q0 = Place { x: bx / 16, y: by / 16, t: 0, level: 0x10, ..Place::default() };
        if !self.places_ready(t) {
            self.status(1);
            return Some(Vec::new());
        }
        let r = self.radius(t);
        let mut cells = Vec::new();
        let mut n = self.places_around(t, &q0, &mut cells, r);
        if n == 0 {
            n = self.places_around(t, &q0, &mut cells, 2);
        }
        let recs = self.event_records();
        let near = (by << 16) + bx;
        let mut f = Filter::new(5);
        for q in cells.iter().take(n.max(0) as usize) {
            self.score_place(&mut f, t, q, &recs, near, true, 0);
        }
        let out = self.reserve(&f, g1, |it, id| Param::new(it.t, id, 0, 0));
        if out.is_empty() {
            self.status(3);
        }
        Some(out)
    }

    fn op9(&mut self, p: &Param) -> Option<Vec<Param>> {
        let t = p.a;
        let pos = p.b;
        let q0 = Place { x: (pos & 0xffff) >> 4, y: (pos >> 16) / 16, t: 0, level: 0x10, ..Place::default() };
        if !self.places_ready(t) {
            self.status(1);
            return Some(Vec::new());
        }
        let mut cells = Vec::new();
        let mut n = self.places_around(t, &q0, &mut cells, 4);
        if n < 5 {
            n = self.places_around(t, &q0, &mut cells, 8);
        }
        let recs = self.event_records();
        let center = ((pos >> 16) << 16) + (pos & 0xffff);
        let mut f = Filter::new(5);
        for q in cells.iter().take(n.max(0) as usize) {
            self.score_place(&mut f, t, q, &recs, center, true, 0);
        }
        let out: Vec<Param> = f.entries().iter().map(|it| Param::new(t, it.pos, 0, p.b)).collect();
        if out.is_empty() {
            self.status(3);
        }
        Some(out)
    }

    fn op10(&mut self, p: &Param) -> Option<Vec<Param>> {
        let res = p.a;
        let Some(ev) = self.e.s.list.iter().find_map(|x| match &x.d {
            Data::ExpandEvent(v) if v.res == res => Some(v.clone()),
            _ => None,
        }) else {
            self.status(3);
            return None;
        };
        // (target node, Q cell)
        let targets: Vec<(Option<i32>, i32, i32)> = if res != -5 {
            let Some(f) = &ev.filter else {
                self.status(3);
                return None;
            };
            f.entries()
                .iter()
                .map(|c| {
                    if c.w18 == 0 {
                        (None, 0, 0)
                    } else {
                        let cpos = ((c.cy << 16) + c.cx) * 16 + 0x80008;
                        (Some(cpos), c.w18 & 0xffff, c.w18 >> 16)
                    }
                })
                .collect()
        } else {
            scores::combined_cells(self.e, self.m).into_iter().map(|w| (Some(w * 16 + 0x80008), w & 0xffff, w >> 16)).collect()
        };
        let recs = self.event_records();
        let mut f = Filter::new(5);
        let mut out = None;
        let mut target = 0;
        for (far, x, y) in targets {
            let Some(far) = far else { continue };
            if res != -5 {
                target = far;
            }
            let q0 = Place { x, y, t: 0, level: 0x10, ..Place::default() };
            out = Some(Vec::new());
            if !self.places_ready(0x2e) {
                self.status(1);
                break;
            }
            let mut cells = Vec::new();
            let n = self.places_around(0x2e, &q0, &mut cells, 1);
            for q in cells.iter().take(n.max(0) as usize) {
                if let Some((score, idx)) = best_node(self.e, self.m, 0x2e, q, &recs, 0, far, true) {
                    let (pos, class) = q.nodes[idx];
                    f.insert(Scoring { t: q.t, x: q.x, y: q.y, score, pos: pos as i32, w2c: if q.t == 0x2e { target } else { 0 }, class });
                }
            }
        }
        if let Some(out) = out.as_mut() {
            for it in f.entries() {
                out.push(Param::new(0x2e, it.pos, it.class as i8 as i32, it.w2c));
            }
        }
        if f.count == 0 {
            self.status(3);
        }
        out
    }

    /// Post a sub-problem (0x64ebe0) for the conditions of chain steps and materials.
    fn post_sub(&mut self, kind: i32, a: i32, b: i32, d: i32, opts: Opts) {
        let id = self.s().id;
        let ai = self.ai;
        let mut sub = problems::Sub::new(ai, kind, a, b, d, opts);
        sub.types = invoke_types(self.s(), kind, a);
        self.e.push_linked(11, id, Data::Sub(Box::new(sub)));
    }
}

fn fail(list: &mut Vec<Cond>) {
    add_cond(list, 0x14, Param::default(), Opts::default());
}

impl Planner for Ctx<'_> {
    fn module(&mut self) -> &mut Module {
        &mut self.s().module
    }

    /// vtbl+0xc (0x645810).
    fn generate(&mut self, op: i32, p: &Param) -> Option<Vec<Param>> {
        match op {
            3 => self.op3(p),
            4 => self.op4(p),
            5 => self.op5(p),
            6 => self.op6(p),
            8 => self.op8(p),
            9 => self.op9(p),
            10 => self.op10(p),
            _ => None,
        }
    }

    /// vtbl+0 (0x647740).
    fn expand(&mut self, op: i32, p: &Param) -> Vec<Cond> {
        let mut l = Vec::new();
        let chain = self.s().chain;
        let sub_opts = Opts { count: 5, ..Opts::default() };
        match op {
            3 | 5 => {
                let (k, t) = if op == 3 { (p.a, chain_type(chain, 0)) } else { (p.a, chain_type(chain, p.a)) };
                if op == 3 {
                    if p.b == 0 && p.c == 0 {
                        self.status(3);
                        fail(&mut l);
                        return l;
                    }
                } else if !self.places_ready(t) {
                    self.status(1);
                    fail(&mut l);
                    return l;
                }
                add_cond(&mut l, 1, Param::new(t, p.b, 0, 0), Opts::default());
                let next = if op == 3 { chain_type(chain, 1) } else { chain_type(chain, k + 1) };
                if next == 0 {
                    return l;
                }
                let k1 = k + 1;
                let id = self.s().id;
                if problems::sub_problem(self.e, id, 4, k1, 0, 0, p.b) != 0 {
                    add_cond(&mut l, 4, Param::new(k1, 0, 0, p.b), sub_opts);
                    return l;
                }
                self.post_sub(4, k1, 0, p.b, sub_opts);
                self.status(2);
                fail(&mut l);
            }
            4 => {
                if p.b == 0 {
                    fail(&mut l);
                    self.status(3);
                    return l;
                }
                let a = self.e.board.get(p.b).map_or(0, |e| e.a);
                let id = self.s().id;
                if problems::sub_problem(self.e, id, 8, a, p.b, 0, p.d) != 0 {
                    add_cond(&mut l, 8, Param::new(a, p.b, 0, p.d), Opts::default());
                    return l;
                }
                self.post_sub(8, a, p.b, p.d, sub_opts);
                self.status(2);
                fail(&mut l);
            }
            6 => {
                if !self.places_ready(p.a) {
                    self.status(1);
                    fail(&mut l);
                } else if p.b == 0 && p.d == 0 {
                    self.status(3);
                    fail(&mut l);
                } else {
                    add_cond(&mut l, 1, Param::new(p.a, p.b, 0, p.d), Opts::default());
                }
            }
            8 => {
                if !self.places_ready(p.a) {
                    self.status(1);
                    fail(&mut l);
                } else {
                    add_cond(&mut l, 1, Param::new(p.a, p.b, 0, 0), Opts::default());
                }
            }
            9 => {
                if !self.places_ready(p.a) {
                    self.status(1);
                    fail(&mut l);
                } else {
                    add_cond(&mut l, 1, Param::new(p.a, 0, p.b, p.d), Opts::default());
                }
            }
            10 => {
                if p.b == 0 {
                    fail(&mut l);
                    self.status(3);
                } else {
                    add_cond(&mut l, 1, Param::new(0x2e, 0, p.b, p.d), Opts::default());
                }
            }
            _ => {}
        }
        l
    }

    /// vtbl+4 (0x647c60).
    fn evaluate(&mut self, op: i32, p: &Param, _best_max: i32, _mode: i32) -> i64 {
        let chain = self.s().chain;
        let v = match op {
            1 => return 0,
            3 => self.reserved_score(chain_type(chain, 0), p.b, 0, chain_type(chain, 1) == 0),
            5 => self.reserved_score(chain_type(chain, p.a), p.b, p.d, chain_type(chain, p.a + 1) == 0),
            6 => self.reserved_score(p.a, p.b, p.d, true),
            8 => self.reserved_score(p.a, p.b, p.b, true),
            9 => place_score(self.e, self.m, p.a, p.b, p.d, p.d, true, p.c),
            10 => place_score(self.e, self.m, p.a, p.b, 0, p.d, true, p.c),
            _ => return 1,
        };
        f32_round(v)
    }

    /// vtbl+0x24 (0x647de0).
    fn cond_kind(&mut self, kind: i32, _p: &Param) -> i32 {
        if (4..=5).contains(&kind) || kind == 8 { 2 } else { 0 }
    }

    /// vtbl+0x28 (0x647e10): the solution of the matching sub-problem, which is then marked
    /// done (state 1).
    fn cond_solution(&mut self, kind: i32, p: &Param) -> Option<Solution> {
        if !((4..=5).contains(&kind) || kind == 8) {
            return None;
        }
        let id = self.s().id;
        let sub = problems::sub_problem(self.e, id, kind, p.a, p.b, p.c, p.d);
        if sub == 0 {
            return None;
        }
        problems::take_solution(self.e, sub)
    }

    /// vtbl+0x1c (0x648050): operator 1 posts the building event (status 1, state 4) of type
    /// `a` at reservation `b` (its place and node), or with `b` 0 at node `c`. A castle tells
    /// its expansion event where (0x6550b0); the sector AI's slot 8 (0x43f380) never turns it
    /// into a tower.
    fn apply(&mut self, op: i32, p: &Param) {
        if op != 1 {
            return;
        }
        let (x, y, pos) = if p.b != 0 {
            let (x, y) = self.e.board.place(p.b);
            (x, y, self.e.board.get(p.b).map_or(0, |e| e.d))
        } else {
            (p.c & 0xffff, p.c >> 16, p.c)
        };
        if p.a == 0x2e {
            let id = self.s().id;
            problems::castle_target(self.e, id, x, y);
        }
        let ev = super::tasks::BuildingEvent { kind: p.a, building: 0, status: 1, state: 4, cx: x, cy: y, pos, f48: 0, counter: 0, placed: false, asked: false };
        self.e.push(9, Data::Building(ev));
    }
}

/// 0x64cd70 on a solver: plan problem `kind` with `param`.
pub fn plan_problem(e: &mut Engine, m: &mut dyn Manager, ai: usize, kind: i32, param: Param) -> Option<Solution> {
    let mut ctx = Ctx { e, m, ai };
    plan::plan(&mut ctx, kind, param)
}

/// 0x64d7b0 on a solver.
pub fn execute(e: &mut Engine, m: &mut dyn Manager, ai: usize, sol: &Solution) {
    let mut ctx = Ctx { e, m, ai };
    plan::execute(&mut ctx, sol);
}
