//! What the monitor (monitor/) reads of a sim thread, collected only while it is open
//! ([`Shared::set_monitored`], which also switches `s4_sim::debug` on): each tick's time with its
//! parts and path searches, the lockstep, the decision log of the watched settlers, and the views
//! the monitor asks for ([`Shared::want`]: a player's economy and AI, settlers, paths) read from
//! the world a few times a second. Nothing here writes to the world.
//!
//! Each sim has its own [`Shared`] (`SimHandle::monitor`, bound to its thread by [`bind`]), so
//! the sims of other games in the same process (tests) do not mix in.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use s4_sim::World;
use s4_sim::debug;

/// One sim's exchange with the monitor.
#[derive(Default)]
pub struct Shared {
    on: AtomicBool,
    stats: Mutex<Stats>,
    want: Mutex<Want>,
    /// When the views were last read: the paths (10 a second) and the rest (once a second).
    views_at: Mutex<(Option<Instant>, Option<Instant>)>,
}

thread_local! {
    /// The sim thread's [`Shared`].
    static BOUND: RefCell<Option<Arc<Shared>>> = const { RefCell::new(None) };
}

/// The calling thread is the sim thread of `s`.
pub fn bind(s: Arc<Shared>) {
    BOUND.with(|b| *b.borrow_mut() = Some(s));
}

fn bound() -> Option<Arc<Shared>> {
    BOUND.with(|b| b.borrow().clone())
}

/// One tick: its time, its parts and path searches (`s4_sim::debug`), the commands it ran, how
/// long it waited for the other players' turns.
#[derive(Clone, Debug, Default)]
pub struct Tick {
    pub tick: u32,
    pub ms: f32,
    pub commands: u16,
    pub wait_ms: f32,
    pub parts: Vec<(&'static str, f32)>,
    pub paths: debug::PathCounts,
}

/// The lockstep of a network game.
#[derive(Clone, Debug, Default)]
pub struct Net {
    /// Turn delay in ticks, hash interval, the players, those whose turn was missing last.
    pub dt: u32,
    pub hash_interval: u32,
    pub players: Vec<u8>,
    pub missing: Vec<u8>,
}

/// What the monitor shows and so wants read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Want {
    /// The players whose economy / AI are shown (0: none).
    pub economy: u8,
    pub ai: u8,
    /// Settlers inspected (their state and decision log).
    pub settlers: Vec<u16>,
    /// Entities whose paths are drawn, and the player all of whose walkers' paths are (0: none).
    pub paths: Vec<u16>,
    pub paths_of: u8,
    /// The player whose queues are shown (0: none).
    pub queues: u8,
}

/// The views read for [`Want`].
/// A read of the views: the fast part (settlers, paths: 10 a second) and / or the slow one (the
/// rest: once a second). [`Views::merge`] keeps what a read did not cover, so the monitor never
/// shows a view empty between two reads.
#[derive(Clone, Debug, Default)]
pub struct Views {
    pub tick: u32,
    pub fast: bool,
    pub slow: bool,
    pub economy: Option<crate::econ_view::Economy>,
    pub ai: Option<debug::AiView>,
    pub settlers: Vec<(u16, debug::SettlerView)>,
    pub paths: Vec<(u16, Vec<(i32, i32)>)>,
    pub queues: Option<crate::queues_view::Queues>,
    /// The eco engine of the AI tab's player (else the Queues tab's): its plans.
    pub eco: Option<s4_sim::ai::eco::view::EcoView>,
}

#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub ticks: Vec<Tick>,
    pub net: Option<Net>,
    /// Desyncs reported since the last take.
    pub desyncs: u32,
    /// The newest views (None: none read since the last take).
    pub views: Option<Views>,
    pub log: Vec<debug::LogEntry>,
}

impl Shared {
    /// The monitor opened or closed.
    pub fn set_monitored(&self, on: bool) {
        self.on.store(on, Ordering::Relaxed);
        debug::enable(on);
        if !on {
            self.want(Want::default());
            self.take();
        }
    }

    /// What the monitor shows now; the inspected settlers are watched by the decision log.
    pub fn want(&self, w: Want) {
        let mut cur = self.want.lock().unwrap_or_else(|e| e.into_inner());
        if *cur != w {
            if cur.settlers != w.settlers {
                debug::watch(&w.settlers);
            }
            *cur = w;
            // Read at once for the new wish.
            *self.views_at.lock().unwrap_or_else(|e| e.into_inner()) = (None, None);
        }
    }

    /// What was collected since the last take (the lockstep stays as it was last seen).
    pub fn take(&self) -> Stats {
        let mut s = self.stats.lock().unwrap_or_else(|e| e.into_inner());
        let net = s.net.clone();
        let out = std::mem::take(&mut *s);
        s.net = net;
        out
    }

    fn with(&self, f: impl FnOnce(&mut Stats)) {
        f(&mut self.stats.lock().unwrap_or_else(|e| e.into_inner()));
    }
}

/// On the sim thread: whether its monitor is open.
pub fn on() -> bool {
    BOUND.with(|b| b.borrow().as_ref().is_some_and(|s| s.on.load(Ordering::Relaxed)))
}

/// On the sim thread: `f` on its stats (nothing without a monitor).
pub(crate) fn with(f: impl FnOnce(&mut Stats)) {
    if let Some(s) = bound() {
        s.with(f);
    }
}

/// After a tick (`ms` long, `commands` run, `wait_ms` waited): its record with the debug
/// channel's parts, the decision log, and the views when due.
pub(crate) fn after_tick(world: &World, ms: f32, commands: u16, wait_ms: f32) {
    let Some(shared) = bound() else { return };
    let probed = debug::take_ticks();
    let mut t = Tick { tick: world.tick, ms, commands, wait_ms, ..Default::default() };
    if let Some(p) = probed.iter().rev().find(|p| p.tick == world.tick) {
        t.parts = p.parts.iter().map(|(n, d)| (*n, d.as_secs_f32() * 1000.0)).collect();
        t.paths = p.paths;
    }
    let log = debug::take_log();
    let views = views(&shared, world);
    shared.with(|st| {
        st.ticks.push(t);
        st.log.extend(log);
        if let Some(v) = views {
            match &mut st.views {
                Some(old) => old.merge(v),
                None => st.views = Some(v),
            }
        }
    });
}

impl Views {
    /// `v`, a later read, over this one: the parts it read replace these, the others stay.
    pub fn merge(&mut self, v: Views) {
        self.tick = v.tick;
        if v.fast {
            self.fast = true;
            self.settlers = v.settlers;
            self.paths = v.paths;
        }
        if v.slow {
            self.slow = true;
            self.economy = v.economy;
            self.ai = v.ai;
            self.queues = v.queues;
            self.eco = v.eco;
        }
    }
}

/// The views due: the paths 10 times a second, the rest once a second.
fn views(shared: &Shared, world: &World) -> Option<Views> {
    let want = shared.want.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let (fast, slow) = {
        let mut at = shared.views_at.lock().unwrap_or_else(|e| e.into_inner());
        let due = |t: &mut Option<Instant>, secs: f32| {
            let d = t.is_none_or(|t| t.elapsed().as_secs_f32() >= secs);
            if d {
                *t = Some(Instant::now());
            }
            d
        };
        (due(&mut at.0, 0.1), due(&mut at.1, 1.0))
    };
    if !fast && !slow {
        return None;
    }
    let mut v = Views { tick: world.tick, fast, slow, ..Default::default() };
    if fast {
        let mut ids = want.paths.clone();
        if want.paths_of != 0 {
            // Every walker of the player (at most 400).
            ids.extend(world.entities.iter().filter(|(_, e)| e.owner == want.paths_of && !e.is_dead() && matches!(e.data, s4_sim::entity::EntityData::Settler(_))).map(|(id, _)| id).take(400));
        }
        v.paths = ids.into_iter().map(|id| (id, debug::path_of(world, id))).filter(|(_, p)| p.len() >= 2).collect();
        v.settlers = want.settlers.iter().filter_map(|&id| debug::settler(world, id).map(|s| (id, s))).collect();
    }
    if slow {
        if want.economy != 0 {
            v.economy = Some(crate::econ_view::of(world, want.economy));
        }
        if want.ai != 0 {
            v.ai = debug::ai(world, want.ai);
        }
        if want.queues != 0 {
            v.queues = Some(crate::queues_view::of(world, want.queues));
        }
        let eco = if want.ai != 0 { want.ai } else { want.queues };
        if eco != 0 {
            v.eco = s4_sim::ai::eco::view::eco_view(world, eco);
        }
    }
    Some(v)
}
