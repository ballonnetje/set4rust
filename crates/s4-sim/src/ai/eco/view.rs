//! A read-only view of a computer player's eco-sector AIs for the monitor overlay (gui's AI
//! tab): the engine's task list in scheduler order (tasks, building events, main / sub problems
//! and their plans), the planned buildings, the reservation board and the goals. Built only when
//! called; it reads `&World` and touches neither the state nor its hash.

use super::engine::{Data, Engine, Entry};
use super::plan::Solution;
use crate::World;

/// One player's eco-sector AIs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EcoView {
    pub player: u8,
    pub sectors: Vec<SectorView>,
}

/// One eco-sector AI (`CAIEcoSectorAIEx` and its `CEcoSectorAI`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SectorView {
    /// Its sector AI (index into the player AI's sectors) and eco-sector.
    pub sector_ai: u8,
    pub eco: u16,
    /// The AI tick it was made at.
    pub made: u32,
    /// Scheduler steps so far, the cursor (a list position) and the entry that runs alone (an
    /// invoke event scanning places; 0 none).
    pub steps: i32,
    pub cursor: i32,
    pub focus: i32,
    /// The board's planning lock: the main problem planning now and the one waiting for it (ids,
    /// 0 none).
    pub lock: i32,
    pub waiting: i32,
    /// The task list in scheduler order.
    pub entries: Vec<EntryView>,
    /// The building events not done: what the AI has decided to build, in list order.
    pub planned: Vec<PlannedBuilding>,
    /// The reservation board: places the planner holds for alternatives under consideration.
    pub reserved: Vec<Reservation>,
    /// The resource goals (good, amount wanted, level, from which tick).
    pub goals: Vec<GoalView>,
}

/// One entry of the task list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryView {
    pub id: i32,
    /// The class (`material`, `building`, `main problem`, …) and its kind number (2..12).
    pub class: &'static str,
    pub kind: i32,
    /// 1 waiting to start, 2 running, 3 finished (removed on its next turn).
    pub state: i32,
    /// The entry it reports to (0 none).
    pub link: i32,
    /// A one-line summary of the class's own fields (ids as numbers).
    pub text: String,
    /// Main and sub problems: the problem and its plan.
    pub problem: Option<ProblemView>,
}

/// A main or sub problem.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProblemView {
    pub main: bool,
    /// Problem kind (3 material, 9 a building near a node, 10 a production chain, 0xb a castle
    /// towards a target; the planner's other kinds by number) and its name.
    pub kind: i32,
    pub kind_name: &'static str,
    /// The parameters as the engine keeps them.
    pub params: [i32; 3],
    /// 0 none yet, 1 failed, 2 solved / failed for good.
    pub result: i32,
    /// Its solver's status (`CSiedlerAI` +status), main problems only.
    pub solver_status: Option<i32>,
    /// The building types whose place scans it waits for (0 = unused slot left out).
    pub scans: Vec<i32>,
    /// The plan chosen (the solution tree), if any.
    pub plan: Option<PlanNode>,
}

/// A node of a chosen plan: operator (1 build, …) and its parameters, then the conditions'
/// solutions. Building operators carry reservation ids from the board in their parameters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanNode {
    pub op: i32,
    pub params: [i32; 4],
    pub subs: Vec<PlanNode>,
}

/// A building the AI decided on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlannedBuilding {
    /// The entry id.
    pub id: i32,
    /// Building type, the building's entity (0 while only planned).
    pub kind: i32,
    pub building: u16,
    /// 1 planned, 2 site placed, 3 finished; the event's state (1 to order … 7 done).
    pub status: i32,
    pub state: i32,
    /// The node.
    pub x: i32,
    pub y: i32,
}

/// A reservation on the board.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reservation {
    pub id: i32,
    /// Building type, the 16×16 cell, the node.
    pub kind: i32,
    pub cx: i32,
    pub cy: i32,
    pub x: i32,
    pub y: i32,
    /// The place score, the group (decimal path of the planner's alternatives; 0 plain).
    pub score: i32,
    pub group: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GoalView {
    pub good: i32,
    pub amount: i32,
    pub level: i32,
    pub from: i32,
}

/// The view of `player`'s eco-sector AIs; `None` when the player has no AI.
pub fn eco_view(w: &World, player: u8) -> Option<EcoView> {
    let a = w.ai.players.get(player as usize)?.as_ref()?;
    let mut sectors = Vec::new();
    for (s, sec) in a.body.sectors.iter().enumerate() {
        if let crate::ai::model::SectorKind::Normal { eco, .. } = &sec.kind {
            for x in eco {
                sectors.push(sector(s as u8, x));
            }
        }
    }
    Some(EcoView { player, sectors })
}

fn sector(sector_ai: u8, x: &super::EcoSectorAi) -> SectorView {
    let e = &x.engine;
    let node = |pos: i32| (pos & 0xffff, pos >> 16);
    let entries = e.s.list.iter().map(|en| entry(e, en)).collect();
    let planned = e
        .s
        .list
        .iter()
        .filter(|en| en.state != 3)
        .filter_map(|en| match &en.d {
            Data::Building(b) => {
                let (x, y) = node(b.pos);
                Some(PlannedBuilding { id: en.id, kind: b.kind, building: b.building as u16, status: b.status, state: b.state, x, y })
            }
            _ => None,
        })
        .collect();
    let mut reserved: Vec<Reservation> = e
        .board
        .refs
        .iter()
        .filter_map(|&(id, k)| e.board.entries.get(k).map(|r| (id, r)))
        .chain(e.board.plain.iter().filter_map(|&k| e.board.entries.get(k).map(|r| (r.id, r))))
        .map(|(id, r)| {
            let (x, y) = node(r.d);
            Reservation { id, kind: r.a, cx: r.b, cy: r.c, x, y, score: r.score, group: r.group }
        })
        .collect();
    reserved.sort_by_key(|r| r.id);
    reserved.dedup_by_key(|r| r.id);
    SectorView {
        sector_ai,
        eco: x.eco,
        made: x.made,
        steps: e.s.steps,
        cursor: e.s.cursor,
        focus: e.s.focus,
        lock: e.board.lock,
        waiting: e.board.waiting,
        entries,
        planned,
        reserved,
        goals: e.s.goals.rows.iter().map(|g| GoalView { good: g.good, amount: g.amount, level: g.level, from: g.from }).collect(),
    }
}

fn plan(s: &Solution) -> PlanNode {
    PlanNode { op: s.op, params: [s.param.a, s.param.b, s.param.c, s.param.d], subs: s.subs.iter().map(plan).collect() }
}

fn scans(t: &[i32; 6]) -> Vec<i32> {
    t.iter().copied().filter(|&v| v != 0).collect()
}

/// The problem kinds the engine documents by name (docs/ECOAI.md).
fn kind_name(k: i32) -> &'static str {
    match k {
        3 => "material",
        9 => "building near a node",
        10 => "production chain",
        0xb => "castle towards a target",
        _ => "",
    }
}

fn entry(e: &Engine, en: &Entry) -> EntryView {
    let (class, text, problem) = match &en.d {
        Data::Productivity(p) => ("productivity", format!("type {} building {} status {} value {} food {}", p.kind, p.building, p.status, p.value, p.food), None),
        Data::BuildingMgr(_) => ("building manager", String::new(), None),
        Data::Material(m) => ("material", format!("good {} stock {} state {} result {}", m.good, m.stock, m.state, m.result), None),
        Data::Resource(r) => ("resource", format!("resource {} cells {} threshold {} type {} wants {} result {}", r.res, r.cells, r.threshold, r.kind, r.wants, r.result), None),
        Data::ExpandMgr(_) => ("expansion manager", String::new(), None),
        Data::ExpandEvent(x) => ("expansion", format!("problem {} resource {} radius {} foreign cells {} / {}", x.sub, x.res, x.radius, x.cells.len(), x.class_cells.len()), None),
        Data::People(p) => ("people", format!("wishes {:#x} residence {} result {} countdown {}", p.flags, p.kind, p.result, p.countdown), None),
        Data::Building(b) => ("building", format!("type {} building {} status {} state {} at {},{}", b.kind, b.building, b.status, b.state, b.pos & 0xffff, b.pos >> 16), None),
        Data::Invoke(v) => ("place scan", format!("type {} state {} scans {} cells {}", v.kind, v.state, v.scans, v.cells.len()), None),
        Data::Main(m) => {
            let status = m.solver.and_then(|k| e.solvers.get(k)?.as_ref()).map(|s| s.status);
            let p = ProblemView { main: true, kind: m.kind, kind_name: kind_name(m.kind), params: [m.a, m.b, m.c], result: m.result, solver_status: status, scans: scans(&m.types), plan: m.solution.as_ref().map(plan) };
            ("main problem", format!("kind {} ({}, {}, {}) result {}", m.kind, m.a, m.b, m.c, m.result), Some(p))
        }
        Data::Sub(s) => {
            let p = ProblemView { main: false, kind: s.kind, kind_name: kind_name(s.kind), params: [s.a, s.b, s.d], result: s.result, solver_status: None, scans: scans(&s.types), plan: s.solution.as_ref().map(plan) };
            ("sub problem", format!("kind {} ({}, {}, {}) prio {} result {}", s.kind, s.a, s.b, s.d, s.opts.prio, s.result), Some(p))
        }
    };
    EntryView { id: en.id, class, kind: en.kind, state: en.state, link: en.link, text, problem }
}
