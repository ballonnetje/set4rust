//! `IAI_Module` (vtable 0x106cf1c, ctor 0x64cc70): the planner under `CSiedlerAI`, an AND/OR
//! search. A problem (kind, parameters) is solved by the operators of its edges (+4,
//! `CSolutionEdge` 0x106cf58 {+0xc operator, +0x10 problem kind}); each operator gives one or
//! more alternatives (vtbl+0xc), an alternative needs conditions (vtbl+0, sub-problems, sorted by
//! priority) that are solved in turn; several alternatives are scored (vtbl+4, summed over the
//! solution tree) and the best kept. A found solution tree is executed bottom-up (vtbl+0x1c).
//!
//! The operators' own code (`CSiedlerAI`) is [`super::siedler`]; it implements [`Planner`].
//! +8 (operators checked by vtbl+8 before use) is never filled in the release, so that check
//! never runs.
//!
//! Scores: every evaluation is an int rounded to a float (`cvtdq2ps` / `cvtsd2ss`), the tree
//! sums them as doubles. The rounded ints are integers below 2^31, their sums exact in a double,
//! so an `i64` holds the same values ([`f32_round`]). Products (mode 1) are never asked for
//! (every option set has mode 0); they are kept in `i64` [inf: no rounding past 2^53].

use crate::sfloat::Sf;

/// `CParam` (0x106cef8, 0x1c bytes): +0xc..+0x18.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Param {
    pub a: i32,
    pub b: i32,
    pub c: i32,
    pub d: i32,
}

impl Param {
    pub fn new(a: i32, b: i32, c: i32, d: i32) -> Param {
        Param { a, b, c, d }
    }
}

/// The options of a condition (0x18 bytes, 0x64d970): +0 priority, +4 keep the highest score
/// (1) or the lowest (0), +8 sum (0) or product (1), +0xc a depth limit (0 none), +0x10
/// required, +0x14 how many alternatives to score at most (0 all).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Opts {
    pub prio: i32,
    pub best_max: i32,
    pub mode: i32,
    pub depth: i32,
    pub required: i32,
    pub count: i32,
}

impl Default for Opts {
    fn default() -> Self {
        Opts { prio: 0, best_max: 1, mode: 0, depth: 0, required: 1, count: 0 }
    }
}

/// `CCondition` (0x106ceec, 0x18 bytes): +0xc problem kind, +0x10 parameters, +0x14 options.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Cond {
    pub kind: i32,
    pub param: Param,
    pub opts: Opts,
}

/// 0x64df40 `(kind, param, opts)`: a condition goes before the first one of lower priority.
pub fn add_cond(list: &mut Vec<Cond>, kind: i32, param: Param, opts: Opts) {
    let at = list.iter().position(|c| opts.prio > c.opts.prio).unwrap_or(list.len());
    list.insert(at, Cond { kind, param, opts });
}

/// `CSolutionTree` (0x106ced4, 0x18 bytes, 0x64dd50): +0xc parameters, +0x10 operator, +0x14 the
/// sub-solutions.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Solution {
    pub param: Param,
    pub op: i32,
    pub subs: Vec<Solution>,
}

/// The edges (0x6455c0): (operator, problem kind).
pub const EDGES: [(i32, i32); 9] = [(1, 1), (2, 2), (4, 3), (8, 8), (3, 10), (5, 4), (6, 5), (9, 9), (10, 0xb)];

/// The module's counters: +0xc the depth, +0x10 the depth allowed (200), +0x14 the depth limit
/// in force (200; conditions may lower it, 0 none).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Module {
    pub depth: i32,
    pub max_depth: i32,
    pub limit: i32,
}

impl Default for Module {
    fn default() -> Self {
        Module { depth: 0, max_depth: 200, limit: 200 }
    }
}

/// `v` rounded to the nearest float, as an exact integer.
pub fn f32_round(v: i32) -> i64 {
    let f = Sf::from_i32(v).0;
    let exp = ((f >> 23) & 0xff) as i32;
    if exp == 0 {
        return 0;
    }
    let mant = ((f & 0x7f_ffff) | 0x80_0000) as i64;
    let shift = exp - 150;
    let mag = if shift >= 0 { mant << shift } else { mant >> -shift };
    if f >> 31 != 0 { -mag } else { mag }
}

/// The operators (the `CSiedlerAI` virtuals the module calls).
pub trait Planner {
    fn module(&mut self) -> &mut Module;
    /// vtbl+0xc: the alternatives of an operator (None: the parameters as they are).
    fn generate(&mut self, op: i32, p: &Param) -> Option<Vec<Param>>;
    /// vtbl+0: the conditions of an alternative.
    fn expand(&mut self, op: i32, p: &Param) -> Vec<Cond>;
    /// vtbl+4 `(param, op, best_max, mode)`: the score (already rounded through a float).
    fn evaluate(&mut self, op: i32, p: &Param, best_max: i32, mode: i32) -> i64;
    /// vtbl+0x24: 1 or 2 when vtbl+0x28 may already hold a solution of the condition.
    fn cond_kind(&mut self, kind: i32, p: &Param) -> i32;
    /// vtbl+0x28: that solution.
    fn cond_solution(&mut self, kind: i32, p: &Param) -> Option<Solution>;
    /// vtbl+0x1c: execute one node.
    fn apply(&mut self, op: i32, p: &Param);
}

/// A condition's solution: one already found (vtbl+0x24 1 or 2 and vtbl+0x28), else solved here,
/// under the condition's depth limit.
fn solve_cond<P: Planner + ?Sized>(pl: &mut P, c: &Cond) -> Option<Solution> {
    let saved = pl.module().limit;
    let m = pl.module();
    if c.opts.depth != 0 && (c.opts.depth < m.limit || m.limit == 0) {
        m.limit = c.opts.depth;
    }
    let k = pl.cond_kind(c.kind, &c.param);
    let mut sub = None;
    if k == 1 || k == 2 {
        sub = pl.cond_solution(c.kind, &c.param);
    }
    if sub.is_none() {
        sub = solve(pl, c.kind, &c.param, &c.opts);
    }
    pl.module().limit = saved;
    sub
}

/// 0x64ce40 `(kind, param, opts)`.
pub fn solve<P: Planner + ?Sized>(pl: &mut P, kind: i32, param: &Param, opts: &Opts) -> Option<Solution> {
    let m = pl.module();
    m.depth += 1;
    if m.depth > m.max_depth || (m.limit < m.depth && m.limit != 0) {
        m.depth -= 1;
        return None;
    }
    let mut alts = Vec::new();
    for &(op, k) in &EDGES {
        if k != kind {
            continue;
        }
        match pl.generate(op, param) {
            None => alts.push((op, *param)),
            Some(list) => alts.extend(list.into_iter().map(|p| (op, p))),
        }
    }
    let r = match alts.len() {
        0 => None,
        1 => solve_single(pl, alts[0].0, &alts[0].1),
        _ => choose(pl, &alts, opts),
    };
    pl.module().depth -= 1;
    r
}

/// The single alternative of 0x64ce40: the depth limit is lowered over all conditions, but only
/// the last one is solved (its limit before restored); an optional condition may fail.
fn solve_single<P: Planner + ?Sized>(pl: &mut P, op: i32, param: &Param) -> Option<Solution> {
    let mut sol = Solution { param: *param, op, subs: Vec::new() };
    let conds = pl.expand(op, param);
    let Some(last) = conds.last() else { return Some(sol) };
    let mut saved = pl.module().limit;
    for c in &conds {
        let m = pl.module();
        saved = m.limit;
        if c.opts.depth != 0 && (c.opts.depth < m.limit || m.limit == 0) {
            m.limit = c.opts.depth;
        }
    }
    let k = pl.cond_kind(last.kind, &last.param);
    let mut sub = None;
    if k == 1 || k == 2 {
        sub = pl.cond_solution(last.kind, &last.param);
    }
    if sub.is_none() {
        sub = solve(pl, last.kind, &last.param, &last.opts);
    }
    pl.module().limit = saved;
    match sub {
        Some(s) => {
            sol.subs.push(s);
            Some(sol)
        }
        None if last.opts.required == 1 => None,
        None => Some(sol),
    }
}

/// 0x64d2a0 `(set, opts)`: every alternative whose conditions all solve is scored; the first of
/// the highest (`best_max` 1, ties to the later) or lowest (0) score beyond ±100000 is kept; at
/// most `count` are scored.
fn choose<P: Planner + ?Sized>(pl: &mut P, alts: &[(i32, Param)], opts: &Opts) -> Option<Solution> {
    let mut best: i64 = if opts.best_max == 0 { 100_000 } else { -100_000 };
    let mut kept = None;
    let mut scored = 0;
    for &(op, param) in alts {
        let conds = pl.expand(op, &param);
        let mut sol = Some(Solution { param, op, subs: Vec::new() });
        for c in &conds {
            match solve_cond(pl, c) {
                Some(s) => {
                    if let Some(x) = sol.as_mut() {
                        x.subs.push(s);
                    }
                }
                None => {
                    sol = None;
                    break;
                }
            }
        }
        let Some(s) = sol else { continue };
        scored += 1;
        let v = score(pl, &s, opts.best_max, opts.mode);
        if (v >= best && opts.best_max == 1) || (best > v && opts.best_max == 0) {
            best = v;
            kept = Some(s);
        }
        if scored == opts.count && opts.count != 0 {
            return kept;
        }
    }
    kept
}

/// 0x64d600 `(tree, best_max, mode)`: the node's evaluation plus (mode 1: times) its subtrees'.
pub fn score<P: Planner + ?Sized>(pl: &mut P, s: &Solution, best_max: i32, mode: i32) -> i64 {
    if s.subs.is_empty() {
        return pl.evaluate(s.op, &s.param, best_max, mode);
    }
    let mut acc: i64 = if mode == 1 { 1 } else { 0 };
    for sub in &s.subs {
        let v = score(pl, sub, best_max, mode);
        acc = if mode == 1 { v.wrapping_mul(acc) } else { v + acc };
    }
    let own = pl.evaluate(s.op, &s.param, best_max, mode);
    if mode == 1 { own.wrapping_mul(acc) } else { own + acc }
}

/// 0x64d7b0: the sub-solutions first (one deeper each), then the node.
pub fn execute<P: Planner + ?Sized>(pl: &mut P, s: &Solution) {
    for sub in &s.subs {
        pl.module().depth += 1;
        execute(pl, sub);
        pl.module().depth -= 1;
    }
    pl.apply(s.op, &s.param);
}

/// 0x64cd70 `(kind, a, b, c, d)`: a search from depth 0 with the default options.
pub fn plan<P: Planner + ?Sized>(pl: &mut P, kind: i32, param: Param) -> Option<Solution> {
    pl.module().depth = 0;
    solve(pl, kind, &param, &Opts::default())
}
