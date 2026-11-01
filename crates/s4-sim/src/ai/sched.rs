//! `CAIScheduler` (vtable 0xff0154, 0x2c bytes; a `CAIAgent` itself): the agents of an owner in
//! one list ordered by their next tick (+4), then priority (+0x10). Decompiles in
//! `tools/re/decompiled/script/ai/s1.c` (0x42e9b0) and `sched.c`.
//!
//! * `Register(agent, period)` (0x42e760, `ret 0x10`, ECX the scheduler): the agent leaves any
//!   scheduler it is in, then next tick 0, period (+8) as given, cost (+0xc) 0x100, priority
//!   (+0x10) 8, and it is inserted (0x42e7e0).
//! * Insert (0x42e7e0): before the first agent whose next tick is later, or equal with a priority
//!   not above the new one's. Agents with the same tick and priority therefore run newest first.
//! * `Reschedule(agent, tick)` (0x42e950): for an agent of this scheduler whose next tick is not
//!   `tick` already: out of the list, next tick `tick`, inserted again. Loads call it with the saved
//!   tick when that is not 0 (0x42e5c0); 0x42b820 / 0x441d40 / 0x443d90 only bring an agent
//!   forward.
//! * `Update(now, budget)` (vtbl+4, 0x42e9b0), see [`Scheduler::update`].
//!
//! The schedulers: `CAIMain` +0x30 (0x146b178) with the one `CAIAgentEvaluation` ("player
//! evaluation", 0x146b1a4, period 0xf; ctor 0x438230), and each player AI's at +0x1c with its
//! global suicide mission (period 0x96, ctor 0x439b20) and its sector AIs' agents (ctors 0x442180
//! / 0x440600 register them with `sector +4 → player AI +0x1c`).

use super::model::{Agent, PlayerBody, SectorKind};

/// An agent a scheduler holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum AgentRef {
    /// `CAIMain`'s player evaluation (0x146b1a4).
    Evaluation,
    /// The player AI's global suicide mission (+0x210).
    GlobalSuicide,
    /// Sector AI `s`'s agent `k` (in save order: [`super::model::NORMAL_AGENTS`] /
    /// [`super::model::DARK_AGENTS`]).
    Sector(u8, u8),
}

/// Where a scheduler finds its agents.
pub trait Agents {
    fn get(&self, r: AgentRef) -> Option<&Agent>;
    fn agent(&mut self, r: AgentRef) -> Option<&mut Agent>;
}

impl Agents for PlayerBody {
    fn get(&self, r: AgentRef) -> Option<&Agent> {
        match r {
            AgentRef::Evaluation => None,
            AgentRef::GlobalSuicide => Some(&self.suicide),
            AgentRef::Sector(s, k) => match &self.sectors.get(s as usize)?.kind {
                SectorKind::Normal { agents, .. } | SectorKind::Dark { agents } => agents.get(k as usize),
            },
        }
    }
    fn agent(&mut self, r: AgentRef) -> Option<&mut Agent> {
        match r {
            AgentRef::Evaluation => None,
            AgentRef::GlobalSuicide => Some(&mut self.suicide),
            AgentRef::Sector(s, k) => match &mut self.sectors.get_mut(s as usize)?.kind {
                SectorKind::Normal { agents, .. } | SectorKind::Dark { agents } => agents.get_mut(k as usize),
            },
        }
    }
}

impl Agents for Agent {
    fn get(&self, r: AgentRef) -> Option<&Agent> {
        (r == AgentRef::Evaluation).then_some(self)
    }
    fn agent(&mut self, r: AgentRef) -> Option<&mut Agent> {
        (r == AgentRef::Evaluation).then_some(self)
    }
}

/// Default cost (+0xc) and priority (+0x10) that `Register` (0x42e760) gives.
pub const COST: u32 = 0x100;
pub const PRIORITY: u32 = 8;
/// The budget both callers give `Update` (0x4384d0, 0x43aac0, 0x43b270).
pub const BUDGET: u32 = 0x1ff;

/// The normal sector AI's registration order (ctor 0x442180, by save index): gardeners (+0xbbc)
/// before geologists (+0xb94).
pub const NORMAL_ORDER: [u8; 13] = [0, 1, 2, 4, 3, 5, 6, 7, 8, 9, 10, 11, 12];
/// The Dark Tribe sector AI's (ctor 0x440600): recruitment +0x198, gardeners +0x148, shamans
/// +0x1c0, manakopters +0x1e8, squad manager +0x210, mushroom farms +0x170, attack +0x104,
/// halls +0x23c — all with period 100.
pub const DARK_ORDER: [u8; 8] = [3, 1, 4, 6, 5, 2, 0, 7];
pub const DARK_PERIOD: u32 = 100;
/// The global suicide mission's period (0x439d48).
pub const SUICIDE_PERIOD: u32 = 0x96;
/// The player evaluation's period (0x438336).
pub const EVALUATION_PERIOD: u32 = 0xf;

/// `CAIScheduler`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Scheduler {
    /// +4 (as an agent): the tick of its first agent, set by `Update`.
    pub next: u32,
    /// +0x28 / +0x24: the agents in order.
    pub queue: Vec<AgentRef>,
}

impl Scheduler {
    /// 0x42e7e0.
    fn insert(&mut self, agents: &impl Agents, r: AgentRef) {
        let Some(a) = agents.get(r) else { return };
        let at = self
            .queue
            .iter()
            .position(|&q| agents.get(q).is_none_or(|c| a.next < c.next || (a.next == c.next && a.prio >= c.prio)))
            .unwrap_or(self.queue.len());
        self.queue.insert(at, r);
    }

    fn unlink(&mut self, r: AgentRef) -> bool {
        match self.queue.iter().position(|&q| q == r) {
            Some(k) => {
                self.queue.remove(k);
                true
            }
            None => false,
        }
    }

    /// 0x42e760 `Register(agent, period)`.
    pub fn register(&mut self, agents: &mut impl Agents, r: AgentRef, period: u32) {
        self.unlink(r);
        let Some(a) = agents.agent(r) else { return };
        a.next = 0;
        a.period = period;
        a.cost = COST;
        a.prio = PRIORITY;
        self.insert(agents, r);
    }

    /// 0x42e950 `Reschedule(agent, tick)`.
    pub fn reschedule(&mut self, agents: &mut impl Agents, r: AgentRef, tick: u32) {
        if !self.queue.contains(&r) || agents.get(r).is_none_or(|a| a.next == tick) {
            return;
        }
        self.unlink(r);
        if let Some(a) = agents.agent(r) {
            a.next = tick;
        }
        self.insert(agents, r);
    }

    /// Whether agent `r` is registered (+0x14, its scheduler, is set).
    pub fn is_registered(&self, r: AgentRef) -> bool {
        self.queue.contains(&r)
    }

    /// The agent `Update` would run now with `budget` left, if any.
    pub fn due(&self, agents: &impl Agents, now: u32, budget: u32) -> Option<AgentRef> {
        let r = *self.queue.first()?;
        let a = agents.get(r)?;
        (now >= a.next && budget >= a.cost).then_some(r)
    }

    /// The bookkeeping of `Update` (0x42e9b0) after agent `r` ran and returned `ret` (bits 0..11
    /// its cost, the rest its delay): a delay of 0 means its period (or 1), a cost of 0 its cost
    /// (+0xc, or 1). The agent goes back into the list at `now + delay`. Returns the cost.
    pub fn after_run(&mut self, agents: &mut impl Agents, r: AgentRef, now: u32, ret: u32) -> u32 {
        let Some(a) = agents.agent(r) else {
            self.unlink(r);
            return 1;
        };
        let mut delay = ret >> 12;
        if delay == 0 {
            delay = if a.period == 0 { 1 } else { a.period };
        }
        let mut cost = ret & 0xfff;
        if cost == 0 {
            cost = if a.cost == 0 { 1 } else { a.cost };
        }
        a.next = now.wrapping_add(delay);
        self.unlink(r);
        self.insert(agents, r);
        cost
    }

    /// The end of `Update`: +4 the first agent's tick (at most `now + 0xfffff`), or `now + 1`
    /// when it is due already. (The return value, `(delay << 12) | min(cost sum, 0xfff)`, is
    /// not used by either caller.)
    pub fn finish(&mut self, agents: &impl Agents, now: u32) {
        let Some(&r) = self.queue.first() else { return };
        let n = agents.get(r).map_or(0, |a| a.next);
        self.next = if now < n { n.min(now.wrapping_add(0xfffff)) } else { now.wrapping_add(1) };
    }
}

/// The player AI's agents registered as a new player AI and its sector AIs register them: the
/// global suicide mission (ctor 0x439b20), then per sector AI in its ctor's order.
pub fn register_player(sched: &mut Scheduler, body: &mut PlayerBody) {
    sched.register(body, AgentRef::GlobalSuicide, SUICIDE_PERIOD);
    for s in 0..body.sectors.len() {
        register_sector(sched, body, s);
    }
}

/// A new sector AI's agents (ctor 0x442180 or 0x440600).
pub fn register_sector(sched: &mut Scheduler, body: &mut PlayerBody, s: usize) {
    let Some(sec) = body.sectors.get(s) else { return };
    match sec.kind {
        SectorKind::Normal { .. } => {
            for k in NORMAL_ORDER {
                sched.register(body, AgentRef::Sector(s as u8, k), super::model::NORMAL_AGENTS[k as usize].2);
            }
        }
        SectorKind::Dark { .. } => {
            for k in DARK_ORDER {
                sched.register(body, AgentRef::Sector(s as u8, k), DARK_PERIOD);
            }
        }
    }
}

/// The schedule a load rebuilds: the ctors register every agent at tick 0, then each agent's
/// load (0x42e5c0) reschedules it to its saved tick when that is not 0 — the global suicide
/// mission with the player AI (0x43ab40), then per sector AI its agents in save order right
/// after that sector AI's ctor (0x43b4a0 / 0x43b0c0).
pub fn rebuild(sched: &mut Scheduler, body: &mut PlayerBody) {
    *sched = Scheduler::default();
    let saved = body.suicide.next;
    sched.register(body, AgentRef::GlobalSuicide, SUICIDE_PERIOD);
    if saved != 0 {
        sched.reschedule(body, AgentRef::GlobalSuicide, saved);
    }
    for s in 0..body.sectors.len() {
        let saved: Vec<u32> = match &body.sectors[s].kind {
            SectorKind::Normal { agents, .. } | SectorKind::Dark { agents } => agents.iter().map(|a| a.next).collect(),
        };
        register_sector(sched, body, s);
        for (k, t) in saved.into_iter().enumerate() {
            if t != 0 {
                sched.reschedule(body, AgentRef::Sector(s as u8, k as u8), t);
            }
        }
    }
}
