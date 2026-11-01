//! `CEcoSectorAI` (vtable 0x106cc70, ctor 0x645210) and its scheduler (0x6515e0): the planning
//! engine of an eco-sector AI. Every eco AI tick runs one scheduler step ([`Engine::step`],
//! 0x6516a0): the manager's events first (0x655510), then the entries of the task list from the
//! cursor (at most 0x15, while an entry asks to go on).
//!
//! The task list (`CDynList`: append at the tail, remove by position, positions renumbered) holds
//! every task and event; each has an id from the scheduler's counter (+0x1c), a kind (+0x10) and
//! a state (+0x14: 1 waiting, 2 running, 3 done → removed). The kinds and their classes:
//! 2 `CProductivityControll`, 3 `CBuildingManagerTask`, 4 `CMaterialManagerTask`,
//! 5 `CResourceManagerTask`, 6 `CExpandationManagerTask`, 7 `CExpandationEvent`,
//! 8 `CPeopleManagerTask`, 9 `CBuildingEvent`, 10 `CMainProblemSolveEvent`,
//! 11 `CSubProblemSolveEvent`, 12 `CInvokeEvent` (their code in [`super::tasks`],
//! [`super::problems`]).

use super::manager::Manager;
use super::tables::{RECORDS, RECORD_OF};
use super::{board, problems, siedler, tasks};

/// A record of the building table 0x1456220 (15 words; [`RECORDS`]) by race and building type
/// (index table 0x1457598, [`RECORD_OF`]); out-of-range types give the empty record 0.
pub fn record(race: i32, t: i32) -> &'static [i32; 15] {
    let k = usize::try_from(race).ok().and_then(|r| RECORD_OF.get(r)).and_then(|row| row.get(usize::try_from(t).ok()?)).copied().unwrap_or(0);
    &RECORDS[k as usize]
}

/// `record(race, t)` word `w` (byte offset `w·4` from 0x1456220).
pub fn rec(race: i32, t: i32, w: usize) -> i32 {
    record(race, t)[w]
}

/// One entry of the task list (`IScheduleEntry`: +0xc id, +0x10 kind, +0x14 state, +0x20 the
/// linked entry, +0x24 "go on" flag; the class's own fields in [`Data`]).
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Entry {
    pub id: i32,
    pub kind: i32,
    pub state: i32,
    pub link: i32,
    pub go_on: i32,
    pub d: Data,
}

/// The classes' own fields.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Data {
    Productivity(tasks::Productivity),
    BuildingMgr(Box<tasks::BuildingMgr>),
    Material(tasks::Material),
    Resource(tasks::Resource),
    ExpandMgr(tasks::ExpandMgr),
    ExpandEvent(tasks::ExpandEvent),
    People(tasks::People),
    Building(tasks::BuildingEvent),
    Main(Box<problems::Main>),
    Sub(Box<problems::Sub>),
    Invoke(Box<problems::Invoke>),
}

/// The resource-goal table (the scheduler's +8 object, 0x328 bytes: 20 records of 0x28 and a
/// count at +0x320; filled by 0x6510f0 through 0x6584f0).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Goals {
    pub rows: Vec<Goal>,
}

/// One goal: +0 race, +4 good, +8 a flag (never set), +0xc level, +0x10 the amount wanted,
/// +0x14 from which tick, +0x18 the condition (0x658600), +0x1c 10000, +0x20 0.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Goal {
    pub race: i32,
    pub good: i32,
    pub flag: bool,
    pub level: i32,
    pub amount: i32,
    pub from: i32,
    pub cond: i32,
}

/// The scheduler (0x7c bytes, ctor 0x6515e0).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Sched {
    /// +4 the task list.
    pub list: Vec<Entry>,
    /// +8 the goals.
    pub goals: Goals,
    /// +0xc the cursor (a list position), +0x10 the step count.
    pub cursor: i32,
    pub steps: i32,
    /// +0x14 player, +0x18 race.
    pub player: i32,
    pub race: i32,
    /// +0x1c the next entry id (from 1).
    pub next_id: i32,
    /// +0x20 the entry that runs alone (an invoke event while it scans), 0 none.
    pub focus: i32,
}

/// `CEcoSectorAI` (0x24 bytes).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Engine {
    /// +0xc the reservation board (0x64f4a0).
    pub board: board::Board,
    /// The main problems' solvers (`CSiedlerAI`, their +0x64; sub-problems share them).
    pub solvers: Vec<Option<siedler::Siedler>>,
    /// +4 the scheduler.
    pub s: Sched,
    /// +0x20 (vtbl+0xc / +0x10): set from 0x1266b18 by the eco AI's ctor.
    pub f20: i32,
}

impl Engine {
    /// The ctor (0x645210, called by 0x4346d0): the board, the tables (static), the scheduler
    /// (0x6515e0: the tasks 0x650470, the goals 0x6510f0), then the existing buildings' events
    /// (0x651350).
    pub fn new(m: &mut dyn Manager, player: i32, race: i32) -> Engine {
        let mut e = Engine { board: board::Board::new(), solvers: Vec::new(), s: Sched { player, race, next_id: 1, ..Sched::default() }, f20: 0 };
        e.make_tasks();
        e.make_goals();
        e.add_existing(m);
        e
    }

    /// vtbl+0x10 (0x645500).
    pub fn set_f20(&mut self, v: i32) {
        self.f20 = v;
    }

    /// vtbl+0xc (0x6454f0).
    pub fn f20(&self) -> i32 {
        self.f20
    }

    /// 0x651800: `e` gets the next id and goes to the end of the list; its id.
    pub fn push(&mut self, kind: i32, d: Data) -> i32 {
        let id = self.s.next_id;
        self.s.next_id += 1;
        self.s.list.push(Entry { id, kind, state: 2, link: 0, go_on: 0, d });
        id
    }

    /// 0x651830 `(e, link)`: [`Engine::push`] with the entry's +0x20 set first.
    pub fn push_linked(&mut self, kind: i32, link: i32, d: Data) -> i32 {
        let id = self.push(kind, d);
        let n = self.s.list.len();
        self.s.list[n - 1].link = link;
        id
    }

    /// A solver into a free slot; its index.
    pub fn add_solver(&mut self, s: siedler::Siedler) -> usize {
        match self.solvers.iter().position(|x| x.is_none()) {
            Some(k) => {
                self.solvers[k] = Some(s);
                k
            }
            None => {
                self.solvers.push(Some(s));
                self.solvers.len() - 1
            }
        }
    }

    /// 0x651860: the entry with this id.
    pub fn find_id(&self, id: i32) -> Option<usize> {
        self.s.list.iter().position(|e| e.id == id)
    }

    /// 0x651890: the first entry of a kind.
    pub fn find_kind(&self, kind: i32) -> Option<usize> {
        self.s.list.iter().position(|e| e.kind == kind)
    }

    /// 0x650470: the tasks, in this order: people, building manager, expandation manager, 12
    /// resource managers (resources 0x70, 0x60 (threshold 0x28), 0, 0x10, 0x20, −3, −4, 0x30,
    /// 0x40, −2, −6, 0x50), material managers for goods 0x20, 7, 0x22, the race's drink (Maya
    /// 0x23, Roman 0x25, Viking 0x17, Trojan 0x29), 0xe, 9, 0x18, 0xb.
    fn make_tasks(&mut self) {
        self.s.steps = 0;
        self.s.cursor = 0;
        self.push(8, Data::People(tasks::People::new()));
        self.push(3, Data::BuildingMgr(Box::default()));
        self.push(6, Data::ExpandMgr(tasks::ExpandMgr::new()));
        for (res, threshold) in [(0x70, 0), (0x60, 0x28), (0, 0), (0x10, 0), (0x20, 0), (-3, 0), (-4, 0), (0x30, 0), (0x40, 0), (-2, 0), (-6, 0), (0x50, 0)] {
            self.push(5, Data::Resource(tasks::Resource::new(res, threshold)));
        }
        let mut goods = vec![0x20, 7, 0x22];
        match self.s.race {
            2 => goods.push(0x23),
            0 => goods.push(0x25),
            1 => goods.push(0x17),
            4 => goods.push(0x29),
            _ => {}
        }
        goods.extend([0xe, 9, 0x18, 0xb]);
        for g in goods {
            self.push(4, Data::Material(tasks::Material::new(g)));
        }
    }

    /// 0x6584f0 `(race, good, _, level, amount, from, cond)`: a goal replaces the one with the
    /// same race, good and level (at its place), else goes to the end (20 at most).
    fn goal(&mut self, race: i32, good: i32, level: i32, amount: i32, from: i32, cond: i32) {
        let rows = &mut self.s.goals.rows;
        let mut n = rows.len();
        let mut at = n;
        let mut k = 0;
        while k < n {
            let g = rows[k];
            if g.race == race && g.good == good && !g.flag && g.level == level {
                n -= 1;
                at = k;
            }
            k += 1;
        }
        let g = Goal { race, good, flag: false, level, amount, from, cond };
        if at < rows.len() {
            rows[at] = g;
            rows.truncate(n + 1);
        } else {
            rows.push(g);
        }
    }

    /// 0x6510f0: the goals by race: boards (0x20 / 7) and the drink first, then gold, coal,
    /// … (amounts, from-ticks and conditions as in the release).
    fn make_goals(&mut self) {
        let r = self.s.race;
        let (stone, boards, drink) = match r {
            1 => (Some(0x28), Some(0xa0), Some(0x17)),
            2 => (Some(0x50), Some(0x66), Some(0x23)),
            4 => (Some(0x50), Some(0x66), Some(0x29)),
            0 => (Some(0x3c), Some(0x9b), Some(0x25)),
            _ => (None, None, None),
        };
        if let (Some(s), Some(b), Some(d)) = (stone, boards, drink) {
            self.goal(r, 0x20, 0, s, 0, 0);
            self.goal(r, 7, 0, b, 0, 0);
            self.goal(r, d, 0, 0x19, 20000, 4);
        }
        self.goal(r, 0x22, 0, 0x19, 4000, 2);
        self.goal(r, 0xe, 0, 0x19, 5000, 3);
        self.goal(r, 9, 0, 0x14, 5000, 5);
        self.goal(r, 0x18, 0, 0x14, 5000, 6);
        self.goal(r, 0xb, 0, 0x32, 5000, 2);
        self.goal(r, 0x22, 1, 0x32, 10000, 3);
        self.goal(r, 0x22, 2, 100, 12000, 9);
        self.goal(r, 9, 1, 0x28, 12000, 10);
        self.goal(r, 0x18, 1, 0x28, 12000, 10);
    }

    /// 0x658590 `(race, good, _, tick)`: the amount of the last goal of `(race, good)` that has
    /// begun (`tick > from`) and whose condition holds.
    pub fn goal_amount(&self, m: &mut dyn Manager, good: i32, tick: i32) -> i32 {
        let mut v = 0;
        for g in &self.s.goals.rows {
            if g.race == self.s.race && g.good == good && !g.flag && tick > g.from && self.cond(m, g.cond) == 0 {
                v = g.amount;
            }
        }
        v
    }

    /// 0x658600: a goal's condition; 0 when it holds.
    fn cond(&self, m: &mut dyn Manager, c: i32) -> i32 {
        let done = |t| self.done_count(t);
        let count = |t| self.prod_count(t);
        match c {
            0 => 0,
            2 => (!(done(3) > 1 && done(4) > 1)) as i32,
            3 => (done(0x18) <= 0) as i32,
            4 => (done(0x13) <= 0) as i32,
            5 => (count(0x11) <= 0) as i32,
            6 => (count(0xf) <= 0) as i32,
            7 => (count(0x10) <= 0) as i32,
            8 => {
                if m.settlers(0x2d) > 0 {
                    0
                } else {
                    (done(0x2c) <= 0) as i32
                }
            }
            9 => {
                let t = match self.s.race {
                    2 => 0x25,
                    0 => 0x23,
                    1 => 0x27,
                    4 => 0x51,
                    _ => return 1,
                };
                (done(t) <= 0) as i32
            }
            10 => (!(count(0x2c) > 0 && count(0x2b) > 0)) as i32,
            _ => 1,
        }
    }

    /// 0x653490: the productivity controls (kind 2, not done) of building type `t`.
    pub fn prod_count(&self, t: i32) -> i32 {
        self.s.list.iter().filter(|e| e.state != 3 && matches!(&e.d, Data::Productivity(p) if p.kind == t)).count() as i32
    }

    /// 0x653560: those of type `t` whose building is finished (+0x30 == 3).
    pub fn done_count(&self, t: i32) -> i32 {
        self.s.list.iter().filter(|e| e.state != 3 && matches!(&e.d, Data::Productivity(p) if p.kind == t && p.status == 3)).count() as i32
    }

    /// 0x651350: for every building record (type +4 of records 1.. until a type 0), the
    /// player's buildings of that type in the eco-sector (slot 0xa0): unfinished ones (progress
    /// < 100) get a building event (status 2, state 4), every one a productivity control when the
    /// record's +0x38 is set; residences that are unfinished or have free beds clear the people
    /// manager's +0x3c.
    fn add_existing(&mut self, m: &mut dyn Manager) {
        let mut k = 1;
        while RECORDS[k][1] != 0 {
            let t = RECORDS[k][1];
            let mut ids = Vec::new();
            m.buildings(t, &mut ids);
            for &id in &ids {
                let (x, y) = m.building_pos(id).unwrap_or((0, 0));
                let pos = (y << 16) + x;
                let status = if m.progress(id) < 100 {
                    let ev = tasks::BuildingEvent { kind: t, building: id, status: 2, state: 4, cx: x / 16, cy: y / 16, pos, f48: 0, counter: 0, placed: id != 0, asked: false };
                    self.push(9, Data::Building(ev));
                    2
                } else {
                    3
                };
                if rec(self.s.race, t, 14) != 0 {
                    self.push(2, Data::Productivity(tasks::Productivity::new(id, status, t, pos)));
                }
                if (0x28..=0x2a).contains(&t)
                    && (m.progress(id) < 100 || m.free_beds(id) > 0)
                    && let Some(i) = self.find_kind(8)
                    && let Data::People(p) = &mut self.s.list[i].d
                {
                    p.residence_ok = false;
                }
            }
            k += 1;
        }
    }

    /// The step (vtbl+4 0x645510 → 0x6516a0). The manager's events first; when one was handled
    /// the step ends. Then, from the cursor (wrapped to 0 past the end), one entry (or, with
    /// [`Sched::focus`], that entry): state 1 asks vtbl+0x10 and +0x14 to start (→ 2); state 2
    /// runs vtbl+8 and on 1 its vtbl+0xc; state 3 removes it (cursor back by one). An entry
    /// that sets +0x24 lets the next one run in the same step (0x15 at most). Returns whether
    /// something ran.
    pub fn step(&mut self, m: &mut dyn Manager) -> bool {
        self.s.steps += 1;
        if tasks::poll_events(self, m) {
            return true;
        }
        if self.s.cursor + 1 > self.s.list.len() as i32 {
            self.s.cursor = 0;
        }
        let mut n = 0;
        let mut ran = false;
        loop {
            let i = if self.s.focus == 0 {
                let c = self.s.cursor;
                match self.s.list.get(c as usize) {
                    Some(_) => c as usize,
                    None => return true,
                }
            } else {
                match self.find_id(self.s.focus) {
                    Some(i) => {
                        self.s.cursor = i as i32;
                        i
                    }
                    None => return true,
                }
            };
            self.s.list[i].go_on = 0;
            if self.s.list[i].state == 1 && tasks::can_start(self, m, i) && tasks::check(self, m, i) {
                self.s.list[i].state = 2;
            }
            if self.s.list[i].state == 2 && tasks::run(self, m, i) {
                tasks::finish(self, m, i);
            }
            let Some(i) = self.find_entry_at(i) else { return true };
            if self.s.list[i].state == 3 {
                let gone = self.s.list.remove(i);
                problems::dropped(self, m, &gone.d);
                self.s.cursor -= 1;
                return ran;
            }
            self.s.cursor += 1;
            ran = true;
            if self.s.list[i].go_on != 1 {
                return ran;
            }
            n += 1;
            ran = false;
            if n > 0x14 {
                return true;
            }
        }
    }

    /// The entry the step works on stays at its position (entries are only appended while it
    /// runs).
    fn find_entry_at(&self, i: usize) -> Option<usize> {
        (i < self.s.list.len()).then_some(i)
    }
}
