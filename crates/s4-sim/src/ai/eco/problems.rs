//! The problem events: `CMainProblemSolveEvent` (kind 10), `CSubProblemSolveEvent` (kind 11)
//! and `CInvokeEvent` (kind 12, a building type's place scan).
//!
//! A main problem waits for the planning lock (the board's +0x14), makes its solver (once), asks
//! for the place scans of its type (invoke events) and, once those are ready and no sub-problem
//! of it is pending, plans. A found plan is executed (building events) and its waiting entry is
//! told 2; a failed one 1 (1 or 3: at once; 0: on its next start, again with 2 later). Status 2
//! (a sub-problem posted) keeps it waiting. Sub-problems plan with their main problem's solver
//! while it holds the lock and keep their solution for its next search.

use super::engine::{Data, Engine};
use super::manager::Manager;
use super::plan::{Opts, Param, Solution};
use super::siedler::{self, Siedler};
use super::tasks::{self, bm_mut, find_invoke, tell};

/// `CMainProblemSolveEvent` (0x70 bytes, ctor 0x64e270 `(link, kind, a, b, c)`).
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Main {
    /// +0x2c / +0x30 / +0x34 / +0x38 the problem's parameters (+0x38 always 0; +0x6c a copy of
    /// +0x34), +0x3c the problem kind.
    pub a: i32,
    pub b: i32,
    pub c: i32,
    pub kind: i32,
    /// +0x40 the outcome for the next start: 1 failed, 2 failed (told twice), 0 none.
    pub result: i32,
    /// +0x44 the plan found.
    pub solution: Option<Solution>,
    /// +0x48 / +0x64 the solver (an index into [`Engine::solvers`]).
    pub solver: Option<usize>,
    /// +0x4c.. the place scans it needs.
    pub types: [i32; 6],
    /// +0x68 a rescan was asked for.
    pub rescan: bool,
}

/// `CSubProblemSolveEvent` (0x6c bytes, ctor 0x64ebe0 `(solver, link, kind, a, b, _, d, opts)`).
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Sub {
    /// +0x48 the main problem's solver.
    pub solver: usize,
    /// +0x2c / +0x30 / +0x38 the parameters (+0x34 is 0), +0x3c the problem kind.
    pub a: i32,
    pub b: i32,
    pub d: i32,
    pub kind: i32,
    /// +0x40 1 failed, 2 solved / failed for good; +0x44 the solution.
    pub result: i32,
    pub solution: Option<Solution>,
    /// +0x4c.. the place scans it needs (0x647e70 at creation).
    pub types: [i32; 6],
    /// +0x64 the condition's options.
    pub opts: Opts,
    /// +0x68 a rescan was asked for.
    pub rescan: bool,
}

impl Sub {
    pub fn new(solver: usize, kind: i32, a: i32, b: i32, d: i32, opts: Opts) -> Sub {
        Sub { solver, a, b, d, kind, result: 0, solution: None, types: [0; 6], opts, rescan: false }
    }
}

/// `CInvokeEvent` (0x4c bytes, ctor 0x64efc0 `(t)`).
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Invoke {
    /// +0x2c the building type.
    pub kind: i32,
    /// +0x30 the scan: 2 working, 0 done with places, 1 done without.
    pub state: i32,
    /// +0x34 scan steps, +0x38 no cells to rescan were given, +0x3c the cells (`cy << 16 | cx`).
    pub scans: i32,
    pub fresh: bool,
    pub cells: Vec<i32>,
    /// +0x48 a problem asked for it (0x64f110).
    pub started: bool,
}

impl Invoke {
    pub fn new(kind: i32) -> Invoke {
        Invoke { kind, state: 2, scans: 0, fresh: true, cells: Vec::new(), started: false }
    }
}

/// 0x64e270 + 0x651830: a main problem linked to `link`.
pub fn push_main(e: &mut Engine, link: i32, kind: i32, a: i32, b: i32, c: i32) -> i32 {
    let main = Main { a, b, c, kind, result: 0, solution: None, solver: None, types: [0; 6], rescan: false };
    e.push_linked(10, link, Data::Main(Box::new(main)))
}

/// 0x6553d0 `(id)`: no sub-problem of `id` is still open (+0x44 and +0x40 both 0).
pub fn no_pending_sub(e: &Engine, id: i32) -> bool {
    !e.s.list.iter().any(|x| x.link == id && matches!(&x.d, Data::Sub(s) if s.solution.is_none() && s.result == 0))
}

/// 0x655440 `(id)`: the sub-problems of `id` end.
fn end_subs(e: &mut Engine, id: i32) {
    for x in &mut e.s.list {
        if x.kind == 11 && x.link == id {
            x.state = 3;
        }
    }
}

/// 0x654ef0 `(id)`: the main problem above a sub-problem (through its links; where a link no
/// longer exists, the last id reached).
pub fn top_main(e: &Engine, id: i32) -> i32 {
    let Some(k) = e.find_id(id) else { return 0 };
    let mut cur = k;
    let mut found = id;
    while e.s.list[cur].kind == 11 {
        let link = e.s.list[cur].link;
        let Some(n) = e.find_id(link) else { return found };
        cur = n;
        found = e.s.list[n].id;
    }
    found
}

/// 0x654d50 `(main, kind, a, b, c, d)`: the first sub-problem of `main` with these parameters;
/// its id when it has an outcome, else 0.
pub fn sub_problem(e: &Engine, main: i32, kind: i32, a: i32, b: i32, c: i32, d: i32) -> i32 {
    for x in &e.s.list {
        if let Data::Sub(s) = &x.d
            && x.link == main
            && s.kind == kind
            && s.a == a
            && s.b == b
            && c == 0
            && s.d == d
        {
            return if s.result != 0 { x.id } else { 0 };
        }
    }
    0
}

/// 0x654df0 `(id)`: a problem's solution; the problem is set waiting (state 1).
pub fn take_solution(e: &mut Engine, id: i32) -> Option<Solution> {
    let k = e.find_id(id)?;
    let x = &mut e.s.list[k];
    let sol = match &x.d {
        Data::Sub(s) => s.solution.clone(),
        Data::Main(m) => m.solution.clone(),
        _ => return None,
    };
    x.state = 1;
    sol
}

/// 0x654e40 `(t)`: a problem still needs type `t`'s places.
pub fn type_needed(e: &Engine, t: i32) -> bool {
    e.s.list.iter().any(|x| match &x.d {
        Data::Sub(s) => s.types.contains(&t) && t != 0,
        Data::Main(m) => m.types.contains(&t) && t != 0,
        _ => false,
    })
}

/// 0x6550b0 `(id, _, x, y)`: the expansion event main problem `id` plans for learns the castle's
/// place (+0x54 / +0x58).
pub fn castle_target(e: &mut Engine, id: i32, x: i32, y: i32) -> bool {
    let Some(k) = e.find_id(id) else { return false };
    if e.s.list[k].kind != 10 {
        return false;
    }
    let link = e.s.list[k].link;
    let Some(j) = e.find_id(link) else { return false };
    if let Data::ExpandEvent(v) = &mut e.s.list[j].d {
        v.f54 = x;
        v.f58 = y;
        return true;
    }
    false
}

/// The board lines repeated in the main problem: its lock is let go.
fn release_lock(e: &mut Engine, id: i32) {
    if e.board.lock == id {
        e.board.unlock(id);
    }
}

// ---- dispatch ---------------------------------------------------------------------------------

pub fn can_start(e: &mut Engine, m: &mut dyn Manager, i: usize) -> bool {
    match &e.s.list[i].d {
        Data::Main(_) => main_can_start(e, i),
        Data::Sub(_) => {
            if top_main(e, e.s.list[i].link) == 0 {
                e.s.list[i].state = 3;
                return false;
            }
            true
        }
        Data::Invoke(v) => {
            if v.state == 1 {
                let t = v.kind;
                m.clear(t);
                e.s.list[i].state = 3;
                return false;
            }
            true
        }
        _ => true,
    }
}

pub fn check(e: &mut Engine, m: &mut dyn Manager, i: usize) -> bool {
    let id = e.s.list[i].id;
    match &e.s.list[i].d {
        Data::Main(_) => no_pending_sub(e, id),
        Data::Sub(s) => {
            let open = s.solution.is_none();
            no_pending_sub(e, id) && open
        }
        Data::Invoke(v) => {
            let t = v.kind;
            if !type_needed(e, t) {
                m.clear(t);
                e.s.list[i].state = 3;
            }
            false
        }
        _ => false,
    }
}

pub fn run(e: &mut Engine, _m: &mut dyn Manager, i: usize) -> bool {
    let id = e.s.list[i].id;
    match &e.s.list[i].d {
        Data::Main(mn) => {
            let has = mn.solution.is_some();
            main_run(e, i, id, has)
        }
        Data::Sub(s) => {
            let has = s.solution.is_some();
            let main = top_main(e, id);
            if e.board.lock != main || has || !no_pending_sub(e, id) {
                e.s.list[i].go_on = 1;
                return false;
            }
            true
        }
        Data::Invoke(v) => {
            if !v.started {
                return false;
            }
            match v.state {
                1 => {
                    e.s.list[i].state = 3;
                    false
                }
                2 => true,
                _ => {
                    e.s.list[i].state = 1;
                    false
                }
            }
        }
        _ => false,
    }
}

pub fn finish(e: &mut Engine, m: &mut dyn Manager, i: usize) {
    match &e.s.list[i].d {
        Data::Main(_) => main_finish(e, m, i),
        Data::Sub(_) => sub_finish(e, m, i),
        Data::Invoke(_) => invoke_finish(e, m, i),
        _ => {}
    }
}

/// An entry leaves the list (its dtor): an invoke event clears its scan (0x64ef20), a main
/// problem frees its solver (0x64e340).
pub fn dropped(e: &mut Engine, m: &mut dyn Manager, d: &Data) {
    match d {
        Data::Invoke(v) => {
            m.clear(v.kind);
        }
        Data::Main(mn) => {
            if let Some(k) = mn.solver {
                e.solvers[k] = None;
            }
        }
        _ => {}
    }
}

// ---- the main problem ---------------------------------------------------------------------------

/// vtbl+8 (0x64e3f0): the lock taken when free (and nobody else waits), else another entry runs;
/// a found plan or an open sub-problem wait too.
fn main_run(e: &mut Engine, i: usize, id: i32, has: bool) -> bool {
    let b = &mut e.board;
    if b.lock != 0 && b.lock != id {
        e.s.list[i].go_on = 1;
        return false;
    }
    if b.lock == 0 && (b.waiting == 0 || b.waiting == id) {
        b.lock = id;
        b.waiting = 0;
    }
    if has || !no_pending_sub(e, id) {
        e.s.list[i].go_on = 1;
        return false;
    }
    true
}

fn main_of(e: &mut Engine, i: usize) -> &mut Main {
    match &mut e.s.list[i].d {
        Data::Main(x) => x,
        _ => unreachable!("main problem"),
    }
}

/// Ask an invoke event for a rescan near a node (0x64f110 `(t, pos)`): for the resource users
/// (1, 4, 6, 0xe, 0x10..0x12) the 12×12 cells around every scored cell of their resource (within
/// 80 of `pos` when `pos` > 0), for 5, 0xf, 0x2b, 0x2c nothing, for the others the 12×12 cells
/// around `pos` (when not 0). The scan is marked started either way.
fn invoke_rescan(e: &mut Engine, k: usize, t: i32, pos: i32) {
    let race = e.s.race;
    let mut cells = Vec::new();
    match t {
        1 | 4 | 6 | 0xe | 0x10..=0x12 => {
            let res = super::engine::rec(race, t, 10);
            let f = e.s.list.iter().find_map(|x| match &x.d {
                Data::Resource(r) if r.res == res => Some(r.filter.clone()),
                _ => None,
            });
            if let Some(Some(f)) = f {
                for c in f.entries() {
                    let cp = ((c.cy << 16) + c.cx) * 16 + 0x80008;
                    if pos > 0 && tasks::dist(pos, cp) > 0x50 {
                        continue;
                    }
                    let cx = (cp >> 4) & 0xfff;
                    let cy = tasks::cell(cp >> 16);
                    for x in cx - 6..cx + 6 {
                        for y in cy - 6..cy + 6 {
                            if x >= 0 && y >= 0 {
                                cells.push((y << 16) + x);
                            }
                        }
                    }
                }
            }
        }
        5 | 0xf | 0x2b | 0x2c => {}
        _ => {
            if pos != 0 {
                let cx = (pos >> 4) & 0xfff;
                let cy = tasks::cell(pos >> 16);
                for x in cx - 6..cx + 6 {
                    for y in cy - 6..cy + 6 {
                        cells.push((y << 16) + x);
                    }
                }
            }
        }
    }
    if let Data::Invoke(v) = &mut e.s.list[k].d {
        v.started = true;
        if !cells.is_empty() {
            v.fresh = false;
            v.cells.extend(cells);
        }
    }
}

/// vtbl+0xc (0x64e450): the solver (first time), the place scans (a missing one is made, a
/// working one asked for a rescan once; the first type missing or working ends the turn), then
/// with no sub-problem open the search. Found: executed, the lock let go, 2 told, done (and the
/// last invoke event too). Not found: status 3 / 1 tells 1 at once and ends (the last invoke
/// event too, its type off the building manager's list); status 0 tells 1 and leaves 2 for the
/// next start; status 2 waits.
fn main_finish(e: &mut Engine, m: &mut dyn Manager, i: usize) {
    let id = e.s.list[i].id;
    let ai = match main_of(e, i).solver {
        Some(k) => {
            if let Some(s) = e.solvers[k].as_mut() {
                s.id = id;
            }
            k
        }
        None => {
            let s = Siedler::new(m, id);
            let k = e.add_solver(s);
            main_of(e, i).solver = Some(k);
            k
        }
    };
    let (kind, a) = {
        let mn = main_of(e, i);
        (mn.kind, mn.a)
    };
    let types = match e.solvers[ai].as_mut() {
        Some(s) => siedler::invoke_types(s, kind, a),
        None => [0; 6],
    };
    main_of(e, i).types = types;
    let mut wait = false;
    let mut last = None;
    // The release walks the list up to the first 0 (0x64e5c9).
    for (k, &t) in types.iter().enumerate() {
        if t == 0 {
            break;
        }
        match find_invoke(e, t) {
            None => {
                e.push(12, Data::Invoke(Box::new(Invoke::new(t))));
                if k == 0 {
                    wait = true;
                }
                last = None;
            }
            Some(inv) => {
                last = Some(e.s.list[inv].id);
                let working = matches!(&e.s.list[inv].d, Data::Invoke(v) if v.state == 2);
                if working {
                    let (rescan, first, p6) = {
                        let mn = main_of(e, i);
                        (mn.rescan, mn.types[0], mn.c)
                    };
                    if !rescan {
                        invoke_rescan(e, inv, first, p6);
                        main_of(e, i).rescan = true;
                    }
                    if k == 0 {
                        wait = true;
                    }
                }
            }
        }
    }
    if wait || !no_pending_sub(e, id) {
        e.s.list[i].go_on = 1;
        return;
    }
    if let Some(s) = e.solvers[ai].as_mut() {
        s.status = 0;
    }
    let (a, b, c) = {
        let mn = main_of(e, i);
        (mn.a, mn.b, mn.c)
    };
    let sol = siedler::plan_problem(e, m, ai, kind, Param::new(a, b, c, 0));
    let link = e.s.list[i].link;
    let end_invoke = |e: &mut Engine, last: Option<i32>| {
        if let Some(l) = last
            && let Some(k) = e.find_id(l)
        {
            e.s.list[k].state = 3;
        }
    };
    if let Some(sol) = sol {
        main_of(e, i).solution = Some(sol.clone());
        siedler::execute(e, m, ai, &sol);
        release_lock(e, id);
        e.s.list[i].state = 1;
        tell(e, link, 2);
        e.s.list[i].state = 3;
        end_invoke(e, last);
        return;
    }
    e.s.list[i].state = 1;
    let status = e.solvers[ai].as_ref().map_or(0, |s| s.status);
    if status == 3 || status == 1 {
        main_of(e, i).result = 1;
        tell(e, link, 1);
        release_lock(e, id);
        e.s.list[i].state = 3;
        if let Some(l) = last
            && let Some(k) = e.find_id(l)
        {
            e.s.list[k].state = 3;
            let t = match &e.s.list[k].d {
                Data::Invoke(v) => v.kind,
                _ => 0,
            };
            if let Some(b) = bm_mut(e) {
                b.unplace(t);
            }
        }
    }
    if status == 0 {
        main_of(e, i).result = 2;
        release_lock(e, id);
        tell(e, link, 1);
    }
}

/// vtbl+0x10 (0x64e9d0): a failure kept for the start is told (1, or 2) and ends it.
fn main_can_start(e: &mut Engine, i: usize) -> bool {
    let id = e.s.list[i].id;
    let link = e.s.list[i].link;
    match main_of(e, i).result {
        1 => {
            e.s.list[i].state = 3;
            release_lock(e, id);
            tell(e, link, 1);
            false
        }
        2 => {
            tell(e, link, 2);
            release_lock(e, id);
            e.s.list[i].state = 3;
            false
        }
        _ => true,
    }
}

// ---- the sub-problem ----------------------------------------------------------------------------

fn sub_of(e: &mut Engine, i: usize) -> &mut Sub {
    match &mut e.s.list[i].d {
        Data::Sub(x) => x,
        _ => unreachable!("sub-problem"),
    }
}

/// vtbl+0xc (0x64ed40): the solver works for it; its place scan (made when missing; asked once
/// for a rescan around its reference cell when working; failed: the main problem fails); then,
/// with nothing of its own open, the search: found → solved (2), waiting (state 1), its own
/// sub-problems end; not found → status 3 / 1 failed (1, its sub-problems end), status 0 failed
/// for good (2, waiting).
fn sub_finish(e: &mut Engine, m: &mut dyn Manager, i: usize) {
    let id = e.s.list[i].id;
    let (ai, t, kind, a, b, d, rescan) = {
        let s = sub_of(e, i);
        (s.solver, s.types[0], s.kind, s.a, s.b, s.d, s.rescan)
    };
    if let Some(s) = e.solvers.get_mut(ai).and_then(|s| s.as_mut()) {
        s.id = id;
    }
    if t != 0 {
        match find_invoke(e, t) {
            None => {
                e.push(12, Data::Invoke(Box::new(Invoke::new(t))));
                return;
            }
            Some(inv) => {
                let state = match &e.s.list[inv].d {
                    Data::Invoke(v) => v.state,
                    _ => 0,
                };
                if state == 2 && !rescan {
                    let c = siedler::sub_cell(&e.board, kind, b, d);
                    let pos = ((c as u32 & 0xffff_0000) as i32 + (c & 0xffff)) * 16 + 0x80008;
                    invoke_rescan(e, inv, t, pos);
                    sub_of(e, i).rescan = true;
                    e.s.list[i].go_on = 1;
                    return;
                }
                if state == 1 {
                    let main = top_main(e, id);
                    if let Some(k) = e.find_id(main)
                        && let Data::Main(mn) = &mut e.s.list[k].d
                    {
                        mn.result = 1;
                    }
                    return;
                }
            }
        }
    }
    if !no_pending_sub(e, id) {
        e.s.list[i].go_on = 1;
        return;
    }
    if e.solvers.get(ai).is_none_or(|s| s.is_none()) {
        return; // [inf] the main problem is gone: the release would use a freed solver
    }
    if let Some(s) = e.solvers[ai].as_mut() {
        s.status = 0;
    }
    let sol = siedler::plan_problem(e, m, ai, kind, Param::new(a, b, 0, d));
    if let Some(sol) = sol {
        let s = sub_of(e, i);
        s.solution = Some(sol);
        s.result = 2;
        e.s.list[i].state = 1;
        end_subs(e, id);
        return;
    }
    let status = e.solvers[ai].as_ref().map_or(0, |s| s.status);
    if status == 3 || status == 1 {
        sub_of(e, i).result = 1;
        end_subs(e, id);
    }
    if status == 0 {
        sub_of(e, i).result = 2;
        e.s.list[i].state = 1;
    }
}

// ---- the invoke event ---------------------------------------------------------------------------

/// vtbl+0xc (0x64f3a0): it scans alone (the scheduler's focus) while working: the cells given
/// rescanned first (slot 8, only before the first step), then a scan step (slot 0xc); while it
/// works the turn goes on, else the focus ends and it waits.
fn invoke_finish(e: &mut Engine, m: &mut dyn Manager, i: usize) {
    let id = e.s.list[i].id;
    if e.s.focus == 0 {
        e.s.focus = id;
    }
    let v = match &e.s.list[i].d {
        Data::Invoke(v) => v.clone(),
        _ => return,
    };
    if v.state == 2 {
        if v.scans == 0 && !v.fresh {
            for &c in &v.cells {
                m.rescan(v.kind, c & 0xffff, c >> 16);
            }
        }
        let r = m.scan(v.kind);
        if let Data::Invoke(x) = &mut e.s.list[i].d {
            x.scans += 1;
            x.state = r;
        }
        if r == 2 {
            return;
        }
    }
    if e.s.focus == id {
        e.s.focus = 0;
    }
    e.s.list[i].state = 1;
}

/// 0x653250 `(good)`.
pub fn stock(e: &mut Engine, _m: &mut dyn Manager, good: i32) -> i32 {
    super::scores::stock(e, good)
}

/// 0x654a90 `(good)`.
pub fn production_pos(e: &Engine, m: &mut dyn Manager, good: i32) -> i32 {
    super::scores::production_pos(e, m, good)
}

/// 0x654210 `(t, pos, _, scale)`.
pub fn resource_score(e: &Engine, m: &mut dyn Manager, t: i32, pos: i32, scale: i32) -> i32 {
    super::scores::resource_score(e, m, t, pos, scale)
}
