//! A read-only debug channel for the client's monitor overlay. Nothing here writes to the
//! [`World`], enters the state hash or a save: the tick timings are collected beside the tick
//! (through [`crate::tick::TickProbe`], the same work in the same order) only while
//! [`enable`]d — off, a tick costs one relaxed atomic load — and the views are plain functions
//! over `&World`, called on demand.
//!
//! The simulation itself never reads a clock; only [`TickTimer`] does, outside of any state.

use crate::World;
use crate::entity::{EntityData, ObjType, flag};
use crate::tick::{Probe, TickProbe};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};

static ENABLED: AtomicBool = AtomicBool::new(false);
static TICKS: Mutex<Vec<TickTimes>> = Mutex::new(Vec::new());
/// Tick records kept until [`take_ticks`] (the oldest go first).
pub const KEEP: usize = 1024;

/// Turns the collection on or off (off drops what was collected).
pub fn enable(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
    if !on && let Ok(mut t) = TICKS.lock() {
        t.clear();
    }
}

/// Whether the collection is on.
#[inline]
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// One tick's wall time by part, in the tick's order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TickTimes {
    pub tick: u32,
    pub total: Duration,
    /// The steps of the tick (`tick::tick_probed`'s phases: `input_process`, `ai_update`,
    /// `tiling_update`, `script`, `animal_update`, `eco_sector_update`, `statistic_update`, …)
    /// and the entity updates of the logic update by class (`settlers`, `buildings`, `sites`,
    /// `piles`, `decos`, `animals`, `vehicles`, `flying`), each once, in order of first use.
    pub parts: Vec<(&'static str, Duration)>,
    /// The path searches of the tick.
    pub paths: PathCounts,
}

/// Path searches: the coarse one over tiles (`CAStarTiling::FindPath`, result 1 found, −1
/// partial, 2 goal only, 0 none) and the fine one over nodes (`CAStar64`, up to 16 steps).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PathCounts {
    pub coarse: u32,
    pub coarse_failed: u32,
    pub coarse_partial: u32,
    /// Coarse waypoints (tile centres) the searches pushed, summed.
    pub coarse_waypoints: u32,
    pub fine: u32,
    pub fine_failed: u32,
    /// Fine steps (nodes) the searches found, summed (at most 16 a search).
    pub fine_steps: u32,
}

static COUNTS: [AtomicU32; 7] = [const { AtomicU32::new(0) }; 7];

/// A coarse search's result (`pathing::astar_tiling`); counted while on.
#[inline]
pub(crate) fn coarse_path(r: i32, waypoints: u32) {
    if enabled() {
        COUNTS[0].fetch_add(1, Ordering::Relaxed);
        COUNTS[5].fetch_add(waypoints, Ordering::Relaxed);
        match r {
            0 => COUNTS[1].fetch_add(1, Ordering::Relaxed),
            -1 => COUNTS[2].fetch_add(1, Ordering::Relaxed),
            _ => 0,
        };
    }
}

/// A fine search's result (`pathing::astar64`); counted while on.
#[inline]
pub(crate) fn fine_path(found: bool, steps: u8) {
    if enabled() {
        COUNTS[3].fetch_add(1, Ordering::Relaxed);
        COUNTS[6].fetch_add(steps as u32, Ordering::Relaxed);
        if !found {
            COUNTS[4].fetch_add(1, Ordering::Relaxed);
        }
    }
}

fn take_counts() -> PathCounts {
    let c = |k: usize| COUNTS[k].swap(0, Ordering::Relaxed);
    PathCounts { coarse: c(0), coarse_failed: c(1), coarse_partial: c(2), coarse_waypoints: c(5), fine: c(3), fine_failed: c(4), fine_steps: c(6) }
}

/// The tick records since the last call, oldest first (at most [`KEEP`]).
pub fn take_ticks() -> Vec<TickTimes> {
    TICKS.lock().map(|mut t| std::mem::take(&mut *t)).unwrap_or_default()
}

/// The probe [`crate::tick::tick_with_script`] runs with while the collection is on: the time
/// from one probe call to the next is charged to the earlier one's part.
pub struct TickTimer {
    times: TickTimes,
    start: Instant,
    last: Instant,
    part: &'static str,
}

impl TickTimer {
    pub fn new(tick: u32) -> TickTimer {
        take_counts();
        let now = Instant::now();
        TickTimer { times: TickTimes { tick, ..TickTimes::default() }, start: now, last: now, part: "input_process" }
    }

    fn charge(&mut self, next: &'static str) {
        let now = Instant::now();
        let d = now - self.last;
        match self.times.parts.iter_mut().find(|p| p.0 == self.part) {
            Some(p) => p.1 += d,
            None => self.times.parts.push((self.part, d)),
        }
        self.last = now;
        self.part = next;
    }

    /// The record goes to the queue [`take_ticks`] drains.
    pub fn finish(mut self) {
        self.charge("end");
        self.times.parts.retain(|p| p.0 != "end");
        self.times.total = self.last - self.start;
        self.times.paths = take_counts();
        if let Ok(mut t) = TICKS.lock() {
            if t.len() >= KEEP {
                t.remove(0);
            }
            t.push(self.times);
        }
    }
}

fn class(obj: ObjType) -> &'static str {
    match obj {
        ObjType::Settler => "settlers",
        ObjType::Building => "buildings",
        ObjType::Pile => "piles",
        ObjType::Deco => "decos",
        ObjType::Animal => "animals",
        ObjType::Ship | ObjType::LandVehicle => "vehicles",
        ObjType::Flying => "flying",
        _ => "other entities",
    }
}

impl TickProbe for TickTimer {
    fn at(&mut self, what: Probe) {
        let next = match what {
            Probe::Phase(p) => p,
            Probe::Entity(obj, _, _) => class(obj),
            Probe::Site(_, _) => "sites",
        };
        if next != self.part {
            self.charge(next);
        }
    }
}

/// One material of a construction site.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Material {
    /// Good type (boards 7, stone 32, gold 14).
    pub good: u8,
    /// Needed in all, built in so far, lying on the site's pile, on their way to it.
    pub needed: u8,
    pub used: u8,
    pub on_site: u8,
    pub coming: u8,
}

/// A construction site of a player.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SiteView {
    pub id: u16,
    /// Building type.
    pub kind: u16,
    pub x: i32,
    pub y: i32,
    /// `CBuildingSiteRole` state (2 levelling, 4 / 5 building, 6..8 Dark Tribe).
    pub state: u8,
    /// Building work done, 0..=100 (work done / work; the levelling not counted).
    pub progress: u8,
    pub materials: Vec<Material>,
    /// Diggers wanted / at work, builders at work.
    pub diggers: i32,
    pub diggers_working: u8,
    pub builders_working: u8,
    /// The site's priority (command 5004).
    pub priority: bool,
}

/// The player's construction sites.
pub fn sites(w: &World, player: u8) -> Vec<SiteView> {
    let mut out = Vec::new();
    for (&id, s) in &w.sites.roles {
        let Some(e) = w.entities.by_index(id as u32).filter(|e| e.owner == player && e.obj_type == ObjType::Building && !e.is_dead()) else { continue };
        let pile = |p: u16| crate::pile::get(w, p).map_or((0, 0), |d| (d.amount, d.coming));
        let materials = [(7u8, s.boards, s.boards_used, s.piles[0]), (32, s.stones, s.stones_used, s.piles[1]), (14, s.gold, s.gold_used, s.piles[2])]
            .into_iter()
            .filter(|m| m.1 > 0)
            .map(|(good, needed, used, p)| {
                let (on_site, coming) = pile(p);
                Material { good, needed, used, on_site, coming }
            })
            .collect();
        let progress = if s.work > 0 { (s.work_done.clamp(0, s.work) as i64 * 100 / s.work as i64) as u8 } else { 0 };
        out.push(SiteView {
            id,
            kind: e.kind,
            x: e.x(),
            y: e.y(),
            state: s.state,
            progress,
            materials,
            diggers: s.diggers,
            diggers_working: s.diggers_working,
            builders_working: s.builders_working,
            priority: s.prio.on,
        });
    }
    out
}

/// Why a production building is (not) working, from what the sim keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProdState {
    /// Its worker is not home yet (none came, or none was found).
    NoWorker,
    /// Stopped by the player (building flag 0x1000 clear).
    Stopped,
    /// An output pile is full (8 goods).
    OutputFull(u8),
    /// An input pile is empty, nothing on the way.
    WaitingFor(u8),
    Working,
}

/// A finished building of a player with piles (production, work-up, storage-less roles).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProdView {
    pub id: u16,
    pub kind: u16,
    pub x: i32,
    pub y: i32,
    pub state: ProdState,
    /// Its piles: (good, amount, coming), input (deliver) and output (production) ones.
    pub inputs: Vec<(u8, u8, u8)>,
    pub outputs: Vec<(u8, u8, u8)>,
}

/// The player's finished buildings that have input or output piles.
pub fn production(w: &World, player: u8) -> Vec<ProdView> {
    let mut out = Vec::new();
    for (&id, r) in &w.buildings.roles {
        let Some(e) = w.entities.by_index(id as u32).filter(|e| e.owner == player && e.obj_type == ObjType::Building && !e.is_dead()) else { continue };
        if w.sites.roles.contains_key(&id) || r.base.piles.is_empty() {
            continue;
        }
        let (mut inputs, mut outputs) = (Vec::new(), Vec::new());
        for &p in &r.base.piles {
            let Some(d) = crate::pile::get(w, p) else { continue };
            match d.role {
                crate::pile::role::DELIVER => inputs.push((d.good, d.amount, d.coming)),
                crate::pile::role::PRODUCTION => outputs.push((d.good, d.amount, d.coming)),
                _ => {}
            }
        }
        if inputs.is_empty() && outputs.is_empty() {
            continue;
        }
        let state = if !r.base.worker_in_house {
            ProdState::NoWorker
        } else if e.flags & 0x1000 == 0 {
            ProdState::Stopped
        } else if let Some(o) = outputs.iter().find(|o| o.1 >= crate::pile::MAX_AMOUNT) {
            ProdState::OutputFull(o.0)
        } else if let Some(i) = inputs.iter().find(|i| i.1 == 0 && i.2 == 0) {
            ProdState::WaitingFor(i.0)
        } else {
            ProdState::Working
        };
        out.push(ProdView { id, kind: e.kind, x: e.x(), y: e.y(), state, inputs, outputs });
    }
    out
}

/// The player's carriers and transport.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CarrierView {
    /// Carriers (type 1) alive.
    pub carriers: u32,
    /// Free ones: offered to their eco-sector (entity flag OFFERED).
    pub idle: u32,
    /// Busy ones: the rest.
    pub busy: u32,
    /// Goods requested by buildings and not served yet (the eco-sectors' `RequestGood` queues).
    pub open_requests: u32,
    /// Goods lying on piles / carried, over the player's eco-sectors.
    pub goods_on_piles: u32,
    pub goods_carried: u32,
}

pub fn carriers(w: &World, player: u8) -> CarrierView {
    let mut v = CarrierView::default();
    for (_, e) in w.entities.iter() {
        if e.obj_type == ObjType::Settler && e.owner == player && e.kind == 1 && !e.is_dead() && matches!(e.data, EntityData::Settler(_)) {
            v.carriers += 1;
            if e.flags & flag::OFFERED != 0 {
                v.idle += 1;
            }
        }
    }
    v.busy = v.carriers - v.idle;
    for &eco in w.economy.player_ecos.get(player as usize).map_or(&[][..], |v| &v[..]) {
        let Some(s) = w.economy.get(eco) else { continue };
        v.open_requests += s.requests.iter().map(|q| q.len() as u32).sum::<u32>();
        v.goods_on_piles += s.goods_on_piles.iter().map(|&n| n as u32).sum::<u32>();
        v.goods_carried += s.goods_on_carriers.iter().map(|&n| n as u32).sum::<u32>();
    }
    v
}

/// The path a walking settler or vehicle is on, for drawing: its node, the fine steps cached
/// (`CDirCache`, up to 16 moves), the coarse waypoint it heads for, the coarse waypoints cached
/// after it (`CWaypoints`, up to 16 tile centres) and the goal. Empty when it has no walking
/// object or is not walking. [inf] The fine cache's entries are direction indices 0..5 of
/// `jobs::walking::NEIGHBOR`, the next move first.
pub fn path_of(w: &World, entity: u16) -> Vec<(i32, i32)> {
    let Some(e) = w.entities.by_index(entity as u32) else { return Vec::new() };
    let walking = match &e.data {
        EntityData::Settler(s) => s.job.walking.as_deref(),
        _ => None,
    };
    let Some(s) = walking.filter(|s| s.walk_to >= 0) else { return Vec::new() };
    let mut out = vec![(e.x(), e.y())];
    let (mut x, mut y) = (e.x(), e.y());
    for i in 0..s.fine_dirs.count as usize {
        let d = s.fine_dirs.get(i) as usize;
        let Some(&(dx, dy)) = crate::jobs::walking::NEIGHBOR.get(d) else { break };
        (x, y) = (x + dx, y + dy);
        out.push((x, y));
    }
    let mut push = |p: i32| {
        if p > 0 {
            let q = crate::grid::Y16X16(p as u32);
            if out.last() != Some(&(q.x(), q.y())) {
                out.push((q.x(), q.y()));
            }
        }
    };
    push(s.coarse);
    let c = &s.coarse_waypoints;
    for i in 0..c.count as usize {
        push(c.ring[(c.head as usize + 16 - i) & 15] as i32);
    }
    push(s.walk_to);
    out
}

// ---- Decision log of watched entities -----------------------------------------------------

static WATCHED: Mutex<Vec<u16>> = Mutex::new(Vec::new());
static LOG: Mutex<Vec<LogEntry>> = Mutex::new(Vec::new());
/// Log entries kept until [`take_log`].
pub const LOG_KEEP: usize = 4096;

/// One decision of a watched entity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogEntry {
    pub tick: u32,
    pub entity: u16,
    /// What happened: `event` (a = event, b = its kind), `role` (a = the new role), `task` (a =
    /// the task type, b = the job part), `todo` (a = the list's length), `set free` (a = the
    /// eco-sector asked for), `attack` (a = the target).
    pub what: &'static str,
    pub a: i32,
    pub b: i32,
}

/// The entities whose decisions are logged (replaces the set; empty stops the log).
pub fn watch(ids: &[u16]) {
    if let Ok(mut w) = WATCHED.lock() {
        w.clear();
        w.extend_from_slice(ids);
        w.sort_unstable();
        w.dedup();
    }
}

/// The log since the last call, oldest first (at most [`LOG_KEEP`]).
pub fn take_log() -> Vec<LogEntry> {
    LOG.lock().map(|mut l| std::mem::take(&mut *l)).unwrap_or_default()
}

/// Called at the decisions of the job / role code: logged while on and `entity` is watched.
#[inline]
pub(crate) fn log(world: &World, entity: u16, what: &'static str, a: i32, b: i32) {
    if !enabled() {
        return;
    }
    let watched = WATCHED.lock().is_ok_and(|w| w.binary_search(&entity).is_ok());
    if watched && let Ok(mut l) = LOG.lock() {
        if l.len() >= LOG_KEEP {
            l.remove(0);
        }
        l.push(LogEntry { tick: world.tick, entity, what, a, b });
    }
}

// ---- One settler ---------------------------------------------------------------------------

/// A settler's job state, for the inspector.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SettlerView {
    pub kind: u16,
    pub owner: u8,
    pub x: i32,
    pub y: i32,
    pub flags: u32,
    pub health: u8,
    /// `jobs::role` number.
    pub role: u8,
    /// The current task type and job part.
    pub task: u8,
    pub job_part: u16,
    /// The task list: `(race, list id)` of a shared one, or the role's own tasks (type, x, y).
    pub list: Option<(u8, u16)>,
    pub own_tasks: Vec<(u8, i16, i16)>,
    /// The target entity, the home building, the queued events.
    pub target: u16,
    pub home: u16,
    pub events: Vec<(i32, i32)>,
    /// Where it walks to (`walk_to`), −1 none.
    pub walk_to: i32,
    /// A soldier's behaviour: state, target.
    pub soldier: Option<(u8, u16)>,
}

pub fn settler(w: &World, id: u16) -> Option<SettlerView> {
    let e = w.entities.by_index(id as u32)?;
    let EntityData::Settler(s) = &e.data else { return None };
    let j = &s.job;
    let (list, own_tasks) = match &j.todo {
        crate::jobs::ToDo::Shared { race, list } => (Some((*race, *list)), Vec::new()),
        crate::jobs::ToDo::Own(v) => (None, v.iter().map(|t| (t.task, t.x, t.y)).collect()),
        crate::jobs::ToDo::None => (None, Vec::new()),
    };
    let soldier = matches!(j.role, crate::jobs::role::SOLDIER | crate::jobs::role::SQUAD_LEADER | crate::jobs::role::SABOTEUR | crate::jobs::role::SHAMAN).then_some((j.soldier.state, j.soldier.target));
    Some(SettlerView {
        kind: e.kind,
        owner: e.owner,
        x: e.x(),
        y: e.y(),
        flags: e.flags,
        health: s.health,
        role: j.role,
        task: j.task,
        job_part: j.job_part,
        list,
        own_tasks,
        target: j.entity,
        home: j.home,
        events: j.events.iter().map(|v| (v.event, v.kind)).collect(),
        walk_to: j.walking.as_ref().map_or(-1, |s| s.walk_to),
        soldier,
    })
}

// ---- The computer players ------------------------------------------------------------------

/// An agent of a player AI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentView {
    /// Its sector AI (index), none for the global suicide mission.
    pub sector: Option<u8>,
    pub kind: String,
    /// The tick it runs next, its period.
    pub next: u32,
    pub period: u32,
}

/// A task force of a player AI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskForceView {
    /// Where it is kept: `reservoir k`, `squad`, `sector s slot k`, `sector s force k`, `sector s
    /// group (type t)`.
    pub place: String,
    pub members: Vec<u16>,
}

/// A player AI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AiView {
    pub player: u8,
    pub dark: bool,
    /// Events waiting in the player AI's queue.
    pub queued: usize,
    /// The agents in scheduler order.
    pub agents: Vec<AgentView>,
    pub task_forces: Vec<TaskForceView>,
    /// The sector AIs: (walking sector, class).
    pub sectors: Vec<(u16, &'static str)>,
    /// The player evaluation (`ai::evaluation`, 22 words).
    pub evaluation: Vec<i32>,
}

pub fn ai(w: &World, player: u8) -> Option<AiView> {
    use crate::ai::model::SectorKind;
    use crate::ai::sched::{AgentRef, Agents};
    let a = w.ai.players.get(player as usize)?.as_ref()?;
    let b = &a.body;
    let agents = a
        .sched
        .queue
        .iter()
        .filter_map(|&r| {
            let g = b.get(r)?;
            let sector = match r {
                AgentRef::Sector(s, _) => Some(s),
                _ => None,
            };
            Some(AgentView { sector, kind: format!("{:?}", g.kind), next: g.next, period: g.period })
        })
        .collect();
    let mut task_forces = Vec::new();
    for (k, t) in b.reservoirs.iter().enumerate() {
        task_forces.push(TaskForceView { place: format!("reservoir {k}"), members: t.members.clone() });
    }
    task_forces.push(TaskForceView { place: "squad".into(), members: b.squad.members.clone() });
    let mut sectors = Vec::new();
    for (s, sec) in b.sectors.iter().enumerate() {
        for (k, t) in sec.slots.iter().enumerate() {
            if let Some((_, t)) = t {
                task_forces.push(TaskForceView { place: format!("sector {s} slot {k}"), members: t.members.clone() });
            }
        }
        if let SectorKind::Normal { forces, .. } = &sec.kind {
            for (k, t) in forces.iter().enumerate() {
                task_forces.push(TaskForceView { place: format!("sector {s} force {k}"), members: t.members.clone() });
            }
        }
        for it in &sec.group.items {
            task_forces.push(TaskForceView { place: format!("sector {s} group (type {})", it.kind), members: it.tf.members.clone() });
        }
        sectors.push((sec.sector(), if matches!(sec.kind, SectorKind::Dark { .. }) { "dark" } else { "normal" }));
    }
    Some(AiView {
        player,
        dark: a.dark,
        queued: a.queue.events.len().saturating_sub(a.queue.read as usize),
        agents,
        task_forces,
        sectors,
        evaluation: crate::ai::evaluation::of(w, player).to_vec(),
    })
}

// ---- Queues (the monitor's Queues tab) -------------------------------------------------------

/// The tick each building's good request was queued (`RequestGood`), kept beside the world while
/// the channel is on: (eco-sector, good, building) → tick.
type RequestKey = (u16, u8, u16);
static REQUESTED: Mutex<Vec<(RequestKey, u32)>> = Mutex::new(Vec::new());
/// The tick the queue view first saw a site wait for one thing: (site, reason) → tick.
static SITE_WAIT: Mutex<Vec<((u16, SiteWait), u32)>> = Mutex::new(Vec::new());

/// `eco::request_good`: notes when a request was queued (only while on).
#[inline]
pub(crate) fn requested(world: &World, eco: u16, good: u8, building: u16) {
    if !enabled() {
        return;
    }
    if let Ok(mut r) = REQUESTED.lock()
        && !r.iter().any(|(k, _)| *k == (eco, good, building))
    {
        r.push(((eco, good, building), world.tick));
    }
}

/// A building's open request for a good.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    pub good: u8,
    /// The building (or site) that asked; its position.
    pub building: u16,
    pub x: i32,
    pub y: i32,
    /// The tick it was queued, when the channel was on then.
    pub since: Option<u32>,
}

/// A pile offering goods.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Offer {
    pub pile: u16,
    pub good: u8,
    /// Goods free to take (amount − leaving).
    pub amount: i32,
    pub x: i32,
    pub y: i32,
}

/// A carrier on a job.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CarrierJob {
    pub carrier: u16,
    /// The good it carries or fetches (0 = none yet).
    pub good: u8,
    /// The piles it takes it from / to (0 = none), and where it is.
    pub from: u16,
    pub to: u16,
    pub x: i32,
    pub y: i32,
}

/// One eco-sector's transport.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EcoTransport {
    pub eco: u16,
    /// The `RequestGood` queues, good by good, each in its order.
    pub requests: Vec<Request>,
    /// The offer lists of piles, good by good, each from its head.
    pub offers: Vec<Offer>,
    /// The free carriers (the type-1 settler offer list).
    pub idle_carriers: Vec<u16>,
    /// Carriers not offered that have a pile to fetch from or bring to (by the eco-sector of
    /// their node).
    pub jobs: Vec<CarrierJob>,
}

fn pos(w: &World, id: u16) -> (i32, i32) {
    w.entities.by_index(id as u32).map_or((0, 0), |e| (e.x(), e.y()))
}

/// An offer list from its head (entity `next` links).
fn offer_list(w: &World, head: u16) -> Vec<u16> {
    let mut out = Vec::new();
    let mut id = head;
    while id != 0 && out.len() < 0x10000 {
        out.push(id);
        id = w.entities.by_index(id as u32).map_or(0, |e| e.next);
    }
    out
}

/// The player's transport, per eco-sector.
pub fn transport(w: &World, player: u8) -> Vec<EcoTransport> {
    let ecos = w.economy.player_ecos.get(player as usize).cloned().unwrap_or_default();
    let since = REQUESTED.lock().map(|r| r.clone()).unwrap_or_default();
    let mut out = Vec::new();
    let mut live = Vec::new();
    for &eco in &ecos {
        let Some(s) = w.economy.get(eco) else { continue };
        let mut t = EcoTransport { eco, requests: Vec::new(), offers: Vec::new(), idle_carriers: Vec::new(), jobs: Vec::new() };
        for (good, q) in s.requests.iter().enumerate() {
            for &b in q {
                let key = (eco, good as u8, b);
                live.push(key);
                let (x, y) = pos(w, b);
                t.requests.push(Request { good: good as u8, building: b, x, y, since: since.iter().find(|(k, _)| *k == key).map(|e| e.1) });
            }
        }
        for (good, &head) in s.good_offer.iter().enumerate() {
            for p in offer_list(w, head) {
                let (x, y) = pos(w, p);
                t.offers.push(Offer { pile: p, good: good as u8, amount: crate::pile::available(w, p), x, y });
            }
        }
        t.idle_carriers = offer_list(w, s.settler_offer.get(1).copied().unwrap_or(0));
        out.push(t);
    }
    // The player's requests no longer queued are forgotten.
    if let Ok(mut r) = REQUESTED.lock() {
        r.retain(|(k, _)| live.contains(k) || !ecos.contains(&k.0));
    }
    for (i, e) in w.entities.iter() {
        if e.obj_type != ObjType::Settler || e.owner != player || e.kind != 1 || e.flags & flag::OFFERED != 0 || e.is_dead() {
            continue;
        }
        let EntityData::Settler(s) = &e.data else { continue };
        let j = &s.job;
        if j.source_pile == 0 && j.carrier.dest_pile == 0 {
            continue;
        }
        let eco = w.grid.checked_index(e.x(), e.y()).map_or(0, |n| crate::eco::eco_of(w, n));
        let job = CarrierJob { carrier: i, good: j.carrier.good, from: j.source_pile, to: j.carrier.dest_pile, x: e.x(), y: e.y() };
        if let Some(t) = out.iter_mut().find(|t| t.eco == eco) {
            t.jobs.push(job);
        } else if let Some(t) = out.first_mut() {
            t.jobs.push(job);
        }
    }
    out
}

/// What a construction site waits for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SiteWait {
    /// Levelling: fewer diggers at work than it wants.
    Diggers,
    /// A material: none on its pile, none on the way, some still needed.
    Material(u8),
    /// Building with no builder at work.
    Builders,
    /// Nothing: work goes on.
    Nothing,
}

/// A site in the build queue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueuedSite {
    /// 0 = served first.
    pub position: usize,
    pub site: SiteView,
    pub waits: SiteWait,
    /// The tick the queue view first saw it wait for that (while the channel was on).
    pub since: Option<u32>,
}

/// The player's construction sites in the order their eco-sector serves them: the sites with
/// priority (command 5004) first, then by entity id. [inf] The release serves each site in its
/// own logic update (`OrderDigger` / `OrderBuilder` / `OrderMaterial`); the priority decides who
/// gets diggers, builders and boards / stone first.
pub fn build_queue(w: &World, player: u8) -> Vec<QueuedSite> {
    let mut list = sites(w, player);
    list.sort_by_key(|s| (!s.priority, s.id));
    let mut memo = SITE_WAIT.lock().map(|m| m.clone()).unwrap_or_default();
    let mut live = Vec::new();
    let mut out = Vec::new();
    for (position, site) in list.into_iter().enumerate() {
        let missing = site.materials.iter().find(|m| m.on_site == 0 && m.coming == 0 && m.used < m.needed).map(|m| m.good);
        let waits = if site.state <= 2 && (site.diggers_working as i32) < site.diggers {
            SiteWait::Diggers
        } else if let Some(g) = missing {
            SiteWait::Material(g)
        } else if site.state >= 4 && site.builders_working == 0 {
            SiteWait::Builders
        } else {
            SiteWait::Nothing
        };
        let key = (site.id, waits);
        live.push(site.id);
        let since = if waits == SiteWait::Nothing {
            None
        } else if let Some(e) = memo.iter().find(|(k, _)| *k == key) {
            Some(e.1)
        } else if enabled() {
            memo.retain(|(k, _)| k.0 != site.id);
            memo.push((key, w.tick));
            Some(w.tick)
        } else {
            None
        };
        out.push(QueuedSite { position, site, waits, since });
    }
    memo.retain(|(k, _)| live.contains(&k.0) || w.entities.by_index(k.0 as u32).is_none_or(|e| e.owner != player));
    if let Ok(mut m) = SITE_WAIT.lock() {
        *m = memo;
    }
    out
}

/// One eco-sector's production orders.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EcoOrders {
    pub eco: u16,
    /// Toolsmith (5033): (tool good, orders left; 100 = endless), in building-info order.
    pub tools: Vec<(u8, u8)>,
    /// Weaponsmith (5034 / 5035): (weapon good, orders left); auto-production on (100) / off (0);
    /// the percentages.
    pub weapons: Vec<(u8, u8)>,
    pub weapon_auto: u8,
    pub weapon_percent: Vec<u8>,
    /// The barracks' soldier wishes (5047, ten soldier types, `ChangeSoldierWish`; the computer
    /// players' recruitment rewrites them every 250 ticks).
    pub soldier_orders: [i8; 10],
    /// Specialist wishes.
    pub specialist_wish: [i8; 5],
}

/// A workshop / shipyard's vehicle orders.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildingOrders {
    pub id: u16,
    pub kind: u16,
    /// (vehicle type, orders left; 100 = endless), the non-zero ones.
    pub orders: Vec<(u8, u8)>,
    /// The vehicle under construction (0 = none).
    pub building: u16,
}

pub fn orders(w: &World, player: u8) -> (Vec<EcoOrders>, Vec<BuildingOrders>) {
    let mut ecos = Vec::new();
    for &eco in w.economy.player_ecos.get(player as usize).map_or(&[][..], |v| &v[..]) {
        let Some(s) = w.economy.get(eco) else { continue };
        ecos.push(EcoOrders {
            eco,
            tools: s.tools.iter().copied().zip(s.tool_orders.iter().copied()).collect(),
            weapons: s.weapons.iter().copied().zip(s.weapon_orders.iter().copied()).collect(),
            weapon_auto: s.weapon_auto,
            weapon_percent: s.weapon_percent.clone(),
            soldier_orders: s.soldier_orders,
            specialist_wish: s.specialist_wish,
        });
    }
    let mut buildings = Vec::new();
    for (&id, r) in &w.buildings.roles {
        let crate::buildings::RoleKind::Workshop(s) = &r.kind else { continue };
        let Some(e) = w.entities.by_index(id as u32).filter(|e| e.owner == player && e.obj_type == ObjType::Building && !e.is_dead()) else { continue };
        let orders = s.orders.iter().enumerate().filter(|&(_, &n)| n != 0).map(|(k, &n)| (k as u8 + 1, n)).collect();
        buildings.push(BuildingOrders { id, kind: e.kind, orders, building: s.vehicle });
    }
    (ecos, buildings)
}

/// A settler type's job states (see [`job_states`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct JobStates {
    pub kind: u16,
    pub idle: u32,
    pub walking: u32,
    pub working: u32,
    pub waiting: u32,
}

/// Per settler type of the player (alive, door settlers left out): **idle** — offered to its
/// eco-sector (free for a job), or a soldier standing without a target; **walking** — its task is
/// a walk (GO 6, GO_TO_POS 7..9, GO_TO_TARGET 10, GO_VIRTUAL 0) or a soldier's behaviour walking
/// somewhere; **waiting** — task WAIT 17 / WAIT_VIRTUAL 2 and not offered; **working** — every
/// other task (WORK, CHECKIN, the virtual work / produce / put / get, a fight).
pub fn job_states(w: &World, player: u8) -> Vec<JobStates> {
    use crate::jobs::task::task_type as t;
    let mut by: std::collections::BTreeMap<u16, JobStates> = Default::default();
    for (_, e) in w.entities.iter() {
        if e.obj_type != ObjType::Settler || e.owner != player || e.is_dead() || e.kind == crate::create::DOOR_SETTLER {
            continue;
        }
        let EntityData::Settler(s) = &e.data else { continue };
        let j = &s.job;
        let c = by.entry(e.kind).or_insert(JobStates { kind: e.kind, ..Default::default() });
        let walking_to = j.walking.as_ref().is_some_and(|ws| ws.walk_to >= 0 && ws.state != 0);
        if e.flags & flag::OFFERED != 0 {
            c.idle += 1;
        } else if matches!(j.task, t::GO | t::GO_TO_POS | 8 | 9 | t::GO_TO_TARGET | t::GO_VIRTUAL) || (j.task == 27 && walking_to) {
            c.walking += 1;
        } else if j.task == 27 && j.soldier.target == 0 {
            c.idle += 1;
        } else if matches!(j.task, t::WAIT | t::WAIT_VIRTUAL) {
            c.waiting += 1;
        } else {
            c.working += 1;
        }
    }
    by.into_values().collect()
}

/// A normal sector AI's attack agent (`CAIAgentAttack`, slot 0; `ai::attack`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AiAttack {
    /// The sector AI.
    pub sector: u8,
    /// 0 waiting, 1 walking to the enemy's edge, 3 all attack, 2 fighting on (see `ai::attack`).
    pub step: i32,
    /// Its squads' soldiers and their strength.
    pub soldiers: i32,
    pub strength: i32,
    /// The earliest tick of the next attack; the chance of trying one (out of 65536).
    pub earliest: i32,
    pub chance: i32,
    /// The target: the building, its owner, its ensign.
    pub target: i32,
    pub owner: i32,
    pub x: i32,
    pub y: i32,
}

/// A computer player's attack agents, one per normal sector AI. Its recruitment has no queue:
/// it rewrites the barracks' soldier wishes every 250 ticks ([`orders`]' `soldier_orders`).
pub fn ai_queues(w: &World, player: u8) -> Vec<AiAttack> {
    use crate::ai::model::SectorKind;
    let Some(a) = w.ai.players.get(player as usize).and_then(|a| a.as_ref()) else { return Vec::new() };
    let mut out = Vec::new();
    for (s, sec) in a.body.sectors.iter().enumerate() {
        let SectorKind::Normal { agents, .. } = &sec.kind else { continue };
        let Some(g) = agents.first() else { continue };
        let x = |k: usize| g.extra.get(k).copied().unwrap_or(0);
        out.push(AiAttack { sector: s as u8, step: x(0), soldiers: x(1), strength: x(2), earliest: x(3), chance: x(4), target: x(5), owner: x(7), x: x(8), y: x(9) });
    }
    out
}

// ---- The world build (`load::new_game`) ----------------------------------------------------

static LOAD: Mutex<Vec<(&'static str, Instant)>> = Mutex::new(Vec::new());

/// `load::new_game` marks the start of each of its steps (only while on).
#[inline]
pub(crate) fn load_step(step: &'static str) {
    if enabled() && let Ok(mut l) = LOAD.lock() {
        l.push((step, Instant::now()));
    }
}

/// The last world build's steps and their wall time, in order (empty when the channel was off).
pub fn take_load_times() -> Vec<(&'static str, Duration)> {
    let marks = LOAD.lock().map(|mut l| std::mem::take(&mut *l)).unwrap_or_default();
    marks.windows(2).map(|w| (w[0].0, w[1].1 - w[0].1)).collect()
}

/// Time per [`Probe`] for a profile ([`bench`]): the time until the next call is charged to the
/// last one; entities and sites per object type and kind.
#[derive(Default)]
pub struct Profiler {
    last: Option<(Probe, Instant)>,
    spent: std::collections::HashMap<Probe, (Duration, u64)>,
}

impl TickProbe for Profiler {
    fn at(&mut self, what: Probe) {
        let now = Instant::now();
        let what = match what {
            Probe::Entity(t, k, _) => Probe::Entity(t, k, 0),
            Probe::Site(k, _) => Probe::Site(k, 0),
            p => p,
        };
        if let Some((p, t)) = self.last.take() {
            let e = self.spent.entry(p).or_default();
            e.0 += now - t;
            e.1 += 1;
        }
        self.last = (!matches!(what, Probe::Phase("end"))).then_some((what, now));
    }
}

impl Profiler {
    /// The steps and entity classes by time, then the `top` most costly entries: per tick of
    /// `ticks`, share of `wall`, calls.
    pub fn report(&self, ticks: u32, wall: Duration, top: usize) -> String {
        use std::fmt::Write;
        // Integers only in this crate (`tests/no_floats.rs`): microseconds a tick, per mille.
        let us = |d: Duration| d.as_micros() / ticks.max(1) as u128;
        let pm = |d: Duration| d.as_micros() * 1000 / wall.as_micros().max(1);
        let mut groups: std::collections::HashMap<String, (Duration, u64)> = std::collections::HashMap::new();
        let mut rows: Vec<(String, Duration, u64)> = Vec::new();
        for (p, &(d, n)) in &self.spent {
            let (group, name) = match p {
                Probe::Phase(s) => (format!("step {s}"), format!("step {s}")),
                Probe::Entity(t, k, _) => (class(*t).to_string(), format!("{t:?} kind {k}")),
                Probe::Site(k, _) => ("sites".to_string(), format!("Site kind {k}")),
            };
            let g = groups.entry(group).or_default();
            g.0 += d;
            g.1 += n;
            rows.push((name, d, n));
        }
        let mut groups: Vec<_> = groups.into_iter().collect();
        groups.sort_by_key(|g| std::cmp::Reverse(g.1.0));
        rows.sort_by_key(|r| std::cmp::Reverse(r.1));
        let mut s = String::new();
        for (k, (d, n)) in groups {
            let _ = writeln!(s, "  {k:<28} {:>7} us/tick {:>4} permille  {n:>9} calls", us(d), pm(d));
        }
        let _ = writeln!(s, "  -- top {top} --");
        for (k, d, n) in rows.iter().take(top) {
            let _ = writeln!(s, "  {k:<28} {:>7} us/tick {:>4} permille  {n:>9} calls  {:>6} ns each", us(*d), pm(*d), d.as_nanos() / (*n).max(1) as u128);
        }
        s
    }
}

/// A timing run of `ticks` ticks without commands or script ([`bench`]).
pub struct Bench {
    /// Wall time of the run (with the probes when profiled).
    pub wall: Duration,
    /// The slowest tick.
    pub worst: (u32, Duration),
    /// `(tick, state hash)` every `every` ticks and after the last.
    pub hashes: Vec<(u32, u64)>,
    /// The wall time of each of those blocks of ticks (without the hashing): on a busy machine
    /// the minimum per block over several runs is the steadier measure.
    pub blocks: Vec<Duration>,
    /// With `profile`: where the time went.
    pub profile: Option<String>,
}

/// Ticks `world` `ticks` times (no commands, no script; the events dropped each tick), timing
/// each tick, the state hash every `every` ticks. With `profile` through a [`Profiler`] (the
/// clock reads add to the time).
pub fn bench(world: &mut World, ticks: u32, every: u32, profile: bool) -> Bench {
    let mut prof = Profiler::default();
    let mut b = Bench { wall: Duration::ZERO, worst: (0, Duration::ZERO), hashes: Vec::new(), blocks: Vec::new(), profile: None };
    let mut block = Duration::ZERO;
    let mut script = crate::tick::NoScript;
    for k in 1..=ticks {
        let t = Instant::now();
        if profile {
            crate::tick::tick_probed(world, &[], &mut script, &mut prof);
        } else {
            crate::tick::tick_probed(world, &[], &mut script, &mut crate::tick::NoProbe);
        }
        let d = t.elapsed();
        b.wall += d;
        block += d;
        if d > b.worst.1 {
            b.worst = (world.tick, d);
        }
        world.events.clear();
        if k % every.max(1) == 0 || k == ticks {
            b.hashes.push((world.tick, world.state_hash()));
            b.blocks.push(std::mem::take(&mut block));
        }
    }
    b.profile = profile.then(|| prof.report(ticks, b.wall, 25));
    b
}

/// The time of each part of [`World::state_hash`] (the fastest of `runs`), in its order.
pub fn hash_times(w: &World, runs: u32) -> Vec<(&'static str, Duration)> {
    crate::world::HASH_PARTS
        .iter()
        .map(|(name, part)| {
            let best = (0..runs.max(1))
                .map(|_| {
                    let t = Instant::now();
                    let mut h = crate::hash::StateHasher::default();
                    part(w, &mut h);
                    std::hint::black_box(h.finish());
                    t.elapsed()
                })
                .min()
                .unwrap_or_default();
            (*name, best)
        })
        .collect()
}
