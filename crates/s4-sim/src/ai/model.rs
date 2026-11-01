//! The player AI's objects and their persistence (savegame chunk 150, per AI player). Every
//! class writes its own tags through the persistence object (`+0x14` an int, `+0x18` a tag,
//! `+0x10` raw bytes); loads check them (`+0xc`) and range-check versions (`+4`). Decompiles in
//! `tools/re/decompiled/script/ai/persist.c` and `goalcache.c`; tag list `tags.txt`.
//!
//! * `CAIEventQueue` 0x436f50 / 0x437030: 0x221ae300, 0x10, read index, count, count × 16 bytes,
//!   0x221ae301.
//! * `CAITaskForce` / `CAITaskForceReservoir` 0x444dc0 / 0x444ec0: 0xa59e0100, 1, 0xa59e0102, the
//!   bytes +0x34..+0x37 and the ints +0x38..+0x4c, 0xa59e0103, the member count and entity ids,
//!   0xa59e0101.
//! * `CAITaskForceSquad` 0x445d30 / 0x445da0: 0xa59e0200, 1, the task force, 0xa59e0202, +0x50,
//!   0xc and 12 bytes (+0x54), 0xa59e0201.
//! * `CAITaskForceGroup` 0x445160 / 0x4452a0: 0xa59e0000, 1, 0xa59e0002, +4, 0xa59e0003, the
//!   count, per task force its type (0..0xd, the class from table 0xff07a8 via factory 0x445470),
//!   0, the task force and the index of its linked one (−1 none), 0xa59e0001.
//! * `CAIAgent` 0x42e5c0 / 0x42e620: 0xa59f0100, 1, 0xa59f0102, the agent's next scheduled tick
//!   (0 when not scheduled), 0xa59f0101. Subclasses wrap it: attack 0xa59f4000 (+10 ints),
//!   defence evaluation 0xa59f5000 (+1), global suicide mission 0xa59f8000 (+0), Dark Tribe
//!   attack 0xa59f1000 (version 1..2, +5 or +6), manakopter halls 0xa59f1003 (+1).
//! * `CAIGoalCache` 0x438db0 / 0x438e00: 1, 0x20, 0xc, the count (0..32), 0x180 bytes (32 ×
//!   12, written whole, used or not).
//! * `CAISectorAI` 0x43f020 / 0x43f0d0: 0xa5920000, 1, 0xa5920002, its event queue (+8), 15
//!   slots of (id ≥ 0 and a task force, or −1), the task force group (+0x58), 0x20 and 32 bytes
//!   (+0xe4), 0xa5920001.
//! * `CAINormalSectorAI` 0x443060 / 0x443240: 0xa5922000, 1, the sector AI, 0xa5920002, 5 goal
//!   caches, 7 task forces, 13 agents, 0xa5922001. `CAIDarkTribeSectorAI` 0x440fa0 / 0x441070:
//!   0xa5921000, 1, the sector AI, 0xa5920002, 8 agents, 0xa5921001.
//! * `CAIPlayerAI` 0x43ab40 / 0x43ac00: 0xa5910000, version 1..2, 0xa5910002, the event queue
//!   (+0x6c), 5 task forces (+0x80 …), with version 2 also the global suicide mission (+0x210)
//!   and the squad (+0x238), 0xa5910001.
//! * `CAINormalPlayerAI` 0x43b4a0 / 0x43b580: 0xa5912000, 0..1 (0: none), the player AI,
//!   0xa5912002, the sector AI count and the sector AIs, 0xa5912001. `CAIDarkTribePlayerAI`
//!   0x43b0c0 / 0x43b1d0: 0xa5911000, 0..2, the player AI, 0xa5911002, +0x2a4, +0x2a8, then
//!   (version 2) whether its one sector AI follows and that sector AI, 0xa5911001.

use super::EventQueue;
use super::AiEvent;

/// Reads the words of chunk 150.
pub struct Rd<'a> {
    pub w: &'a [u32],
    pub at: usize,
}

pub type R<T> = Result<T, String>;

impl Rd<'_> {
    pub fn u32(&mut self) -> R<u32> {
        let v = *self.w.get(self.at).ok_or_else(|| format!("chunk 150 ends at word {}", self.at))?;
        self.at += 1;
        Ok(v)
    }
    pub fn i32(&mut self) -> R<i32> {
        self.u32().map(|v| v as i32)
    }
    pub fn tag(&mut self, t: u32) -> R<()> {
        let v = self.u32()?;
        if v == t { Ok(()) } else { Err(format!("tag {t:#x} expected at word {}, found {v:#x}", self.at - 1)) }
    }
    pub fn ranged(&mut self, lo: i32, hi: i32) -> R<i32> {
        let v = self.i32()?;
        if (lo..=hi).contains(&v) { Ok(v) } else { Err(format!("{v} outside {lo}..={hi} at word {}", self.at - 1)) }
    }
    pub fn peek(&self) -> Option<u32> {
        self.w.get(self.at).copied()
    }
    fn words<const N: usize>(&mut self) -> R<[u32; N]> {
        let mut a = [0; N];
        for v in a.iter_mut() {
            *v = self.u32()?;
        }
        Ok(a)
    }
}

/// Words of chunk 150 being written.
pub type Wr = Vec<u32>;

fn put(w: &mut Wr, v: i32) {
    w.push(v as u32);
}

impl EventQueue {
    pub fn load(r: &mut Rd) -> R<EventQueue> {
        r.tag(0x221a_e300)?;
        r.ranged(0x10, 0x10)?;
        let read = r.u32()?;
        let n = r.i32()?;
        let mut events = Vec::new();
        for _ in 0..n.max(0) {
            events.push(AiEvent { kind: r.i32()?, player: r.i32()?, a: r.i32()?, b: r.i32()? });
        }
        r.tag(0x221a_e301)?;
        Ok(EventQueue { read, events })
    }
    pub fn save(&self, w: &mut Wr) {
        w.extend([0x221a_e300, 0x10, self.read, self.events.len() as u32]);
        for e in &self.events {
            w.extend([e.kind as u32, e.player as u32, e.a as u32, e.b as u32]);
        }
        w.push(0x221a_e301);
    }
}

/// `CAITaskForce` (base ctor 0x4448e0) and its saved subclasses.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TaskForce {
    /// +0x34..+0x37 (bytes, saved as ints).
    pub bytes: [i32; 4],
    /// +0x38..+0x4c; +0x38 is the player with flags (0x80000000 for a reservoir; adding a
    /// member sets 0x0c000000, 0x444ab0).
    pub ints: [i32; 6],
    /// The members (+0x10 list; entity ids).
    pub members: Vec<u16>,
    /// `CAITaskForceSquad`: +0x50 and the 12 bytes at +0x54.
    pub squad: Option<(i32, [u32; 3])>,
    /// +0x60 of the priest task forces: the tick of the next spell look (0x446f20; not saved).
    #[cfg_attr(feature = "serde", serde(default))]
    pub next_cast: u32,
}

impl TaskForce {
    /// The base ctor 0x4448e0 `(player, type, kind, flags)`: +0x38 = player | flags, rest 0.
    pub fn new(player: u8, flags: u32) -> TaskForce {
        TaskForce { ints: [(player as u32 | flags) as i32, 0, 0, 0, 0, 0], ..Default::default() }
    }

    /// A reservoir (`sub_448E0(player, 1, 1, 0x80000000)`).
    pub fn reservoir(player: u8) -> TaskForce {
        TaskForce::new(player, 0x8000_0000)
    }

    /// +0x34: the timer, counted down by 8 at each visit of the timer list (0x445610); at 0 the
    /// task force's update (vtbl+0x28) runs.
    pub fn timer(&self) -> u8 {
        self.bytes[0] as u8
    }
    pub fn set_timer(&mut self, v: u8) {
        self.bytes[0] = v as i32;
    }
    /// +0x35: the state of the update's state machine.
    pub fn state(&self) -> u8 {
        self.bytes[1] as u8
    }
    pub fn set_state(&mut self, v: u8) {
        self.bytes[1] = v as i32;
    }
    /// +0x36: the state after the current move (0x4464a0 cases 0x65 / 100).
    pub fn next_state(&self) -> u8 {
        self.bytes[2] as u8
    }
    /// +0x37: the phase others read (1 idle, 2 moving, 3/5 attacking, 6 working, 7 following,
    /// 8 done, 9 boarding, 10 cast / flying …).
    pub fn phase(&self) -> u8 {
        self.bytes[3] as u8
    }
    /// 0x4458b0: a new phase notes the tick (+0x48).
    pub fn set_phase(&mut self, phase: u8, now: u32) {
        if self.phase() != phase {
            self.bytes[3] = phase as i32;
            self.ints[4] = now as i32;
        }
    }
    /// 0x4458d0 `(phase, state, next)`.
    pub fn set_states(&mut self, phase: u8, state: u8, next: u8, now: u32) {
        self.bytes[1] = state as i32;
        self.bytes[2] = next as i32;
        self.set_phase(phase, now);
    }
    /// +0x38: player (bits 0..3), the order (bits 4..11), 0x1000 / 0x2000 / 0x10000 / 0x4000000 /
    /// 0x8000000 / 0x20000000 working bits, 0x0c000000 when a member joined, 0x80000000 reservoir.
    pub fn flags(&self) -> u32 {
        self.ints[0] as u32
    }
    pub fn set_flags(&mut self, v: u32) {
        self.ints[0] = v as i32;
    }
    /// The order (bits 4..11 of +0x38).
    pub fn command(&self) -> u32 {
        self.flags() >> 4 & 0xff
    }
    /// +0x3c: the order's target (a packed position or an entity).
    pub fn target(&self) -> i32 {
        self.ints[1]
    }
    /// +0x44: the tick of the last order.
    pub fn order_tick(&self) -> u32 {
        self.ints[3] as u32
    }
    /// +0x4c (the dark gardener agent's next look).
    pub fn b4c(&self) -> u32 {
        self.ints[5] as u32
    }
    /// +0x50: where the task force goes (−1 none; squad-like classes).
    pub fn goal(&self) -> i32 {
        self.squad.map_or(-1, |s| s.0)
    }
    /// +0x54..+0x5c: the next three waypoints (−1 none).
    pub fn waypoints(&self) -> [i32; 3] {
        self.squad.map_or([-1; 3], |s| s.1.map(|v| v as i32))
    }
    pub fn set_waypoints(&mut self, w: [i32; 3]) {
        if let Some(s) = self.squad.as_mut() {
            s.1 = w.map(|v| v as u32);
        }
    }
    /// vtbl+0x2c (0x446340): the goal set, the waypoints cleared.
    pub fn set_goal(&mut self, v: i32) {
        if let Some(s) = self.squad.as_mut() {
            *s = (v, [u32::MAX; 3]);
        }
    }

    /// `CAITaskForceReservoir` vtbl+0x14 (0x444ab0) for an entity not in it: to the front
    /// (`front`, mode 1) or the back; flags 0x0c000000.
    pub fn add_member(&mut self, id: u16, front: bool) {
        if front {
            self.members.insert(0, id);
        } else {
            self.members.push(id);
        }
        self.ints[0] |= 0x0c00_0000;
    }

    /// vtbl+0x18 (0x444ba0).
    pub fn remove_member(&mut self, id: u16) -> bool {
        match self.members.iter().position(|&m| m == id) {
            Some(k) => {
                self.members.remove(k);
                true
            }
            None => false,
        }
    }

    fn load_base(r: &mut Rd) -> R<([i32; 4], [i32; 6], Vec<u16>)> {
        r.tag(0xa59e_0100)?;
        r.ranged(1, 1)?;
        r.tag(0xa59e_0102)?;
        let bytes = [r.i32()?, r.i32()?, r.i32()?, r.i32()?];
        let ints = [r.i32()?, r.i32()?, r.i32()?, r.i32()?, r.i32()?, r.i32()?];
        r.tag(0xa59e_0103)?;
        let n = r.i32()?;
        let members = (0..n.max(0)).map(|_| r.u32().map(|v| v as u16)).collect::<R<_>>()?;
        r.tag(0xa59e_0101)?;
        Ok((bytes, ints, members))
    }

    /// Either layout, by its tag.
    pub fn load(r: &mut Rd) -> R<TaskForce> {
        if r.peek() == Some(0xa59e_0200) {
            r.tag(0xa59e_0200)?;
            r.ranged(1, 1)?;
            let (bytes, ints, members) = Self::load_base(r)?;
            r.tag(0xa59e_0202)?;
            let v50 = r.i32()?;
            r.ranged(0xc, 0xc)?;
            let raw = r.words::<3>()?;
            r.tag(0xa59e_0201)?;
            Ok(TaskForce { bytes, ints, members, squad: Some((v50, raw)), next_cast: 0 })
        } else {
            let (bytes, ints, members) = Self::load_base(r)?;
            Ok(TaskForce { bytes, ints, members, squad: None, next_cast: 0 })
        }
    }

    pub fn save(&self, w: &mut Wr) {
        if let Some((v50, raw)) = self.squad {
            w.extend([0xa59e_0200, 1]);
            self.save_base(w);
            w.extend([0xa59e_0202, v50 as u32, 0xc]);
            w.extend(raw);
            w.push(0xa59e_0201);
        } else {
            self.save_base(w);
        }
    }

    fn save_base(&self, w: &mut Wr) {
        w.extend([0xa59e_0100, 1, 0xa59e_0102]);
        self.bytes.iter().chain(&self.ints).for_each(|&v| put(w, v));
        w.extend([0xa59e_0103, self.members.len() as u32]);
        w.extend(self.members.iter().map(|&m| m as u32));
        w.push(0xa59e_0101);
    }
}

/// One task force of a group: its type (0..0xd, class by [`CLASS_OF_TYPE`]), the task force, its
/// id in the group and the task force it is linked with (+0x2c, both ways; 0 none).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GroupItem {
    pub kind: i32,
    pub tf: TaskForce,
    /// s4rs's stable id of the task force within its group (the release uses its address).
    pub uid: u32,
    /// The linked task force's `uid` (saved as its index in the group, −1 none).
    pub link: u32,
}

/// .rdata 0xff07a8: the class of each task-force type (the factory 0x445470): 1 reservoir,
/// 2 squad (`CAITaskForceSquad`, 0x4466e0), 3 priests (by tribe, 0x446d50), 4 war machines,
/// 5 war ships, 6 dark gardeners, 7 shamans, 8 manakopters. The group keeps one list per class
/// (+0x68 + 4·class).
pub const CLASS_OF_TYPE: [u8; 14] = [0, 1, 2, 2, 2, 2, 2, 2, 3, 4, 5, 6, 7, 8];

/// The class of task-force type `kind` (0: none).
pub fn class_of(kind: i32) -> u8 {
    usize::try_from(kind).ok().and_then(|k| CLASS_OF_TYPE.get(k)).copied().unwrap_or(0)
}

/// `CAITaskForceGroup` (vtable 0x102b3dc).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TaskForceGroup {
    /// +4 (the player).
    pub b4: i32,
    /// The task forces in the order saved: by class list (+0x6c.., classes 1..8), each list from
    /// its head (newest first).
    pub items: Vec<GroupItem>,
    /// The next `uid`.
    #[cfg_attr(feature = "serde", serde(default))]
    pub next_uid: u32,
}

impl TaskForceGroup {
    pub fn load(r: &mut Rd) -> R<TaskForceGroup> {
        r.tag(0xa59e_0000)?;
        r.ranged(1, 1)?;
        r.tag(0xa59e_0002)?;
        let b4 = r.i32()?;
        r.tag(0xa59e_0003)?;
        let n = r.i32()?;
        let mut items: Vec<GroupItem> = Vec::new();
        for k in 0..n.max(0) {
            let kind = r.ranged(0, 0xd)?;
            r.i32()?;
            let tf = TaskForce::load(r)?;
            let link = r.i32()?;
            let uid = k as u32 + 1;
            // 0x445160: a link to an earlier task force links both.
            let mut l = 0;
            if (0..k).contains(&link) {
                l = items[link as usize].uid;
                items[link as usize].link = uid;
            }
            items.push(GroupItem { kind, tf, uid, link: l });
        }
        r.tag(0xa59e_0001)?;
        let next_uid = n.max(0) as u32 + 1;
        Ok(TaskForceGroup { b4, items, next_uid })
    }
    pub fn save(&self, w: &mut Wr) {
        w.extend([0xa59e_0000, 1, 0xa59e_0002, self.b4 as u32, 0xa59e_0003, self.items.len() as u32]);
        for it in &self.items {
            w.extend([it.kind as u32, 0]);
            it.tf.save(w);
            let link = if it.link == 0 { -1 } else { self.items.iter().position(|o| o.uid == it.link).map_or(-1, |k| k as i32) };
            put(w, link);
        }
        w.push(0xa59e_0001);
    }

    /// The item with `uid`.
    pub fn get(&self, uid: u32) -> Option<&GroupItem> {
        self.items.iter().find(|i| i.uid == uid)
    }

    pub fn get_mut(&mut self, uid: u32) -> Option<&mut GroupItem> {
        self.items.iter_mut().find(|i| i.uid == uid)
    }

    /// The list of class `class` from its head (`+0x68 + 4·class`): the uids.
    pub fn list(&self, class: u8) -> Vec<u32> {
        self.items.iter().filter(|i| class_of(i.kind) == class).map(|i| i.uid).collect()
    }

    /// The factory 0x445470 `(player, type)` and the head insertion into the class list (0x445020,
    /// inlined in 0x430300): a new task force of `kind` (classes 2, 6, 7, 8 are squad-like and save
    /// their target and waypoints). `None` for a type without a class. The caller registers it with
    /// the task forces' timer list (the ctor 0x4448e0 does).
    pub fn create(&mut self, player: u8, kind: i32) -> Option<u32> {
        let class = class_of(kind);
        if class == 0 {
            return None;
        }
        let mut tf = TaskForce::new(player, if class == 1 { 0x8000_0000 } else { 0 });
        if class != 1 {
            tf.squad = Some((0, [0; 3]));
        }
        if self.next_uid == 0 {
            self.next_uid = self.items.iter().map(|i| i.uid).max().unwrap_or(0) + 1;
        }
        let uid = self.next_uid;
        self.next_uid += 1;
        let at = self.items.iter().position(|i| class_of(i.kind) >= class).unwrap_or(self.items.len());
        self.items.insert(at, GroupItem { kind, tf, uid, link: 0 });
        Some(uid)
    }

    /// The task force leaves the group (0x4449e0 / 0x445100): its link goes both ways.
    pub fn remove(&mut self, uid: u32) -> Option<GroupItem> {
        let k = self.items.iter().position(|i| i.uid == uid)?;
        let it = self.items.remove(k);
        if it.link != 0
            && let Some(o) = self.get_mut(it.link)
        {
            o.link = 0;
        }
        Some(it)
    }

    /// Links two task forces (+0x2c both ways), dropping their old links (0x430450).
    pub fn link(&mut self, a: u32, b: u32) {
        for u in [a, b] {
            let old = self.get(u).map_or(0, |i| i.link);
            if old != 0
                && let Some(o) = self.get_mut(old)
            {
                o.link = 0;
            }
            if let Some(i) = self.get_mut(u) {
                i.link = 0;
            }
        }
        if let Some(i) = self.get_mut(a) {
            i.link = b;
        }
        if let Some(i) = self.get_mut(b) {
            i.link = a;
        }
    }
}

/// The saved agent classes (their wrapper tag).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum AgentKind {
    /// `CAIAgent` only (geologists, gardeners, pioneers, recruitment, …).
    #[default]
    Base,
    /// `CAIAgentAttack` (0xa59f4000).
    Attack,
    /// `CAIAgentEvalDefence` (0xa59f5000).
    EvalDefence,
    /// `CAIAgentGlobalSuicideMission` (0xa59f8000).
    GlobalSuicide,
    /// `CAIAgentDarkTribeAttack` (0xa59f1000, version 1 or 2).
    DarkAttack(u8),
    /// `CAIAgentDarkTribeManakopterHalls` (0xa59f1003).
    ManakopterHalls,
}

impl AgentKind {
    fn wrap(self) -> Option<(u32, u32, u32, usize)> {
        Some(match self {
            AgentKind::Base => return None,
            AgentKind::Attack => (0xa59f_4000, 0xa59f_4002, 0xa59f_4001, 10),
            AgentKind::EvalDefence => (0xa59f_5000, 0xa59f_5002, 0xa59f_5001, 1),
            AgentKind::GlobalSuicide => (0xa59f_8000, 0xa59f_8002, 0xa59f_8001, 0),
            // +0x28, +0x2c, +0x30, +0x34, +0x38, +0x40 and (version 2) +0x3c (save 0x42bf40, load
            // 0x42be40).
            AgentKind::DarkAttack(v) => (0xa59f_1000, 0xa59f_1002, 0xa59f_1001, if v > 1 { 7 } else { 6 }),
            AgentKind::ManakopterHalls => (0xa59f_1003, 0xa59f_1005, 0xa59f_1004, 1),
        })
    }
}

/// `CAIAgent` (vtable 0xff016c): a named, scheduled job of a player or sector AI (+4 the next
/// tick, +0x14 the scheduler it is registered with; registered by 0x42e760 with a period).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Agent {
    pub kind: AgentKind,
    /// +4: the next tick (saved; 0 when not scheduled, which a registered agent never is after its
    /// first run).
    pub next: u32,
    /// +8: the period `Register` gives (0x42e760); not saved.
    pub period: u32,
    /// +0xc: the cost `Update` charges when the agent's `Execute` returns none (0x100).
    pub cost: u32,
    /// +0x10: the priority among agents due on the same tick (8).
    pub prio: u32,
    /// The subclass's saved ints (attack +0x28..+0x4c, …).
    pub extra: Vec<i32>,
    /// +0x28 of a subclass that does not save it (the Dark Tribe squad manager's step 0..7).
    #[cfg_attr(feature = "serde", serde(default))]
    pub state: i32,
}

impl Agent {
    pub fn new(kind: AgentKind) -> Agent {
        let n = kind.wrap().map_or(0, |w| w.3);
        let mut extra = vec![0; n];
        if let AgentKind::DarkAttack(_) = kind {
            // The ctor's 0x42c010 (no sector AI yet: no RNG draw): strength +0x30 = −1.
            extra[2] = -1;
        }
        Agent { kind, next: 0, period: 0, cost: 0, prio: 0, extra, state: 0 }
    }

    fn load_base(r: &mut Rd) -> R<u32> {
        r.tag(0xa59f_0100)?;
        r.ranged(1, 1)?;
        r.tag(0xa59f_0102)?;
        let next = r.u32()?;
        r.tag(0xa59f_0101)?;
        Ok(next)
    }

    /// The agent by its tag.
    pub fn load(r: &mut Rd) -> R<Agent> {
        let kind = match r.peek() {
            Some(0xa59f_4000) => AgentKind::Attack,
            Some(0xa59f_5000) => AgentKind::EvalDefence,
            Some(0xa59f_8000) => AgentKind::GlobalSuicide,
            Some(0xa59f_1000) => AgentKind::DarkAttack(0),
            Some(0xa59f_1003) => AgentKind::ManakopterHalls,
            _ => {
                return Ok(Agent { kind: AgentKind::Base, next: Self::load_base(r)?, period: 0, cost: 0, prio: 0, extra: Vec::new(), state: 0 });
            }
        };
        let (open, mid, close, _) = kind.wrap().expect("wrapped");
        r.tag(open)?;
        let kind = if let AgentKind::DarkAttack(_) = kind { AgentKind::DarkAttack(r.ranged(1, 2)? as u8) } else {
            r.ranged(1, 1)?;
            kind
        };
        let n = kind.wrap().expect("wrapped").3;
        let next = Self::load_base(r)?;
        r.tag(mid)?;
        let extra = (0..n).map(|_| r.i32()).collect::<R<_>>()?;
        r.tag(close)?;
        Ok(Agent { kind, next, period: 0, cost: 0, prio: 0, extra, state: 0 })
    }

    pub fn save(&self, w: &mut Wr) {
        let wrap = self.kind.wrap();
        if let Some((open, ..)) = wrap {
            let v = if let AgentKind::DarkAttack(v) = self.kind { v as u32 } else { 1 };
            w.extend([open, v]);
        }
        w.extend([0xa59f_0100, 1, 0xa59f_0102, self.next, 0xa59f_0101]);
        if let Some((_, mid, close, _)) = wrap {
            w.push(mid);
            self.extra.iter().for_each(|&v| put(w, v));
            w.push(close);
        }
    }
}

/// `CAIGoalCache` (vtable 0x102b248): up to 32 goals of 12 bytes (entity id, its unique id, a
/// value), kept sorted by value, highest first, from entry 1; entry 0 is the sentinel (0, 0,
/// 0x7fffffff).
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GoalCache {
    /// +4.
    pub count: i32,
    /// +8, 0x180 bytes as words (the ctor sets the first goal to (0, 0, 0x7fffffff)).
    pub raw: Vec<u32>,
    /// +0x188: the AI tick of the last clean-up (0x438d10; not saved).
    #[cfg_attr(feature = "serde", serde(default))]
    pub cleaned: u32,
}

impl Default for GoalCache {
    fn default() -> Self {
        let mut raw = vec![0; 0x60];
        raw[2] = 0x7fff_ffff;
        GoalCache { count: 0, raw, cleaned: 0 }
    }
}

impl GoalCache {
    pub fn load(r: &mut Rd) -> R<GoalCache> {
        r.ranged(1, 1)?;
        r.ranged(0x20, 0x20)?;
        r.ranged(0xc, 0xc)?;
        let count = r.ranged(0, 0x20)?;
        let raw = (0..0x60).map(|_| r.u32()).collect::<R<_>>()?;
        Ok(GoalCache { count, raw, cleaned: 0 })
    }

    /// The goals 1..=count: (entity, unique id, value).
    pub fn entries(&self) -> Vec<(i32, u32, i32)> {
        (1..=self.count.clamp(0, 31) as usize).map(|k| (self.raw[3 * k] as i32, self.raw[3 * k + 1], self.raw[3 * k + 2] as i32)).collect()
    }

    fn set(&mut self, k: usize, e: (i32, u32, i32)) {
        self.raw[3 * k] = e.0 as u32;
        self.raw[3 * k + 1] = e.1;
        self.raw[3 * k + 2] = e.2 as u32;
    }

    fn get(&self, k: usize) -> (i32, u32, i32) {
        (self.raw[3 * k] as i32, self.raw[3 * k + 1], self.raw[3 * k + 2] as i32)
    }

    /// Entry `k` out, the later ones move up.
    fn take_out(&mut self, k: usize) {
        for j in k..self.count as usize {
            let e = self.get(j + 1);
            self.set(j, e);
        }
        self.count -= 1;
    }

    /// 0x438bb0 `(entity)`: its first goal goes.
    pub fn remove(&mut self, id: i32) {
        if let Some(k) = (1..=self.count.clamp(0, 31) as usize).find(|&k| self.raw[3 * k] as i32 == id) {
            self.take_out(k);
        }
    }

    /// 0x438c30 `(entity, value)` with the entity's unique id `uid` (the caller checked that the
    /// entity is above 0, exists and has a unique id above 0): its old goal goes; the first goal
    /// is kept as is when the cache is empty, else the count is cut to 30 and the goal goes in
    /// after the goals with a value at least as high.
    pub fn add(&mut self, id: i32, uid: u32, value: i32) {
        self.remove(id);
        if self.count < 1 {
            self.count = 1;
            self.set(1, (id, uid, value));
            return;
        }
        if self.count > 0x1e {
            self.count = 0x1e;
        }
        let n = self.count as usize;
        let mut k = n + 1;
        while self.get(k - 1).2 < value {
            k -= 1;
        }
        for j in (k..=n).rev() {
            let e = self.get(j);
            self.set(j + 1, e);
        }
        self.set(k, (id, uid, value));
        self.count += 1;
    }

    /// 0x438d10: the goals whose entity no longer has the noted unique id go (an id ≤ 0 or a
    /// gone entity has 0); +0x188 = `now`.
    pub fn clean(&mut self, uid_of: impl Fn(i32) -> u32, now: u32) {
        let mut k = 1;
        while k as i32 <= self.count {
            let (id, uid, _) = self.get(k);
            let cur = if id > 0 { uid_of(id) } else { 0 };
            if uid == cur {
                k += 1;
            } else {
                self.take_out(k);
            }
        }
        self.cleaned = now;
    }
    pub fn save(&self, w: &mut Wr) {
        w.extend([1, 0x20, 0xc, self.count as u32]);
        w.extend(&self.raw);
    }
}

/// The `CAISectorAI` part of a load: queue, slots, group, +0xe4.
type SectorBase = (EventQueue, Vec<Option<(i32, TaskForce)>>, TaskForceGroup, [u32; 8]);

/// What the sector AI subclass adds.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SectorKind {
    /// `CAINormalSectorAI` (vtable 0x102b218, ctor 0x442180, 0x1598 bytes): 5 goal caches
    /// (+0x104 …, stride 0x18c), 7 task forces (+0x8c0 …) and 13 agents (+0xaf0 …: attack,
    /// defence and offence evaluation, geologists, gardeners, intruder combat, moving home,
    /// pioneers, recruitment, suicide missions, squad management, thieves, tower occupation).
    ///
    /// Not saved by the release (made by the ctor or a load): the eco-sector AIs (+0xd94 /
    /// +0xd98, made anew at the end of a load by 0x4439f0) and the two building cursors (+0xd4c
    /// the own military buildings, +0xd70 the enemies').
    Normal {
        caches: Vec<GoalCache>,
        forces: Vec<TaskForce>,
        agents: Vec<Agent>,
        #[cfg_attr(feature = "serde", serde(default))]
        eco: Vec<super::eco::EcoSectorAi>,
        #[cfg_attr(feature = "serde", serde(default))]
        cursors: [Cursor; 2],
    },
    /// `CAIDarkTribeSectorAI` (vtable 0x102afac, ctor 0x440600): 8 agents (+0x104 …).
    Dark { agents: Vec<Agent> },
}

/// A building cursor of the normal sector AI (0x20 bytes; set up by the ctor with 0x4392a0, first
/// 0x439470, next 0x4394c0): it walks `CBuildingMgr`'s lists of the players in a mask (lowest
/// first) and the types 46..=48 (small tower, large tower, castle). The mask and types follow from
/// which cursor it is; kept here: the current player (+0xc), type (+0x14) and entity (+0x18).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Cursor {
    pub player: u8,
    pub kind: u16,
    pub entity: u16,
}

/// `CAISectorAI` (vtable 0x102af04): one per eco-sector of the player.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SectorAi {
    /// +8.
    pub queue: EventQueue,
    /// +0x1c: 15 task forces, indexed by warrior type (0x43a170), with the member count read
    /// from the save (+0xc), or none.
    pub slots: Vec<Option<(i32, TaskForce)>>,
    /// +0x58.
    pub group: TaskForceGroup,
    /// +0xe4..+0x103: the anchor building (Dark Tribe), the base position, the anchor (+0xf4 /
    /// +0xf8), its node index and its sector (+0x100).
    pub raw: [u32; 8],
    pub kind: SectorKind,
}

/// The normal sector AI's agents in their slots (+0xaf0 …) with their scheduling periods (ctor
/// 0x442180, 0x42e760).
pub const NORMAL_AGENTS: [(AgentKind, &str, u32); 13] = [
    (AgentKind::Attack, "attack", 100),
    (AgentKind::EvalDefence, "defence evaluation", 5),
    (AgentKind::Base, "offence evaluation", 10),
    (AgentKind::Base, "geologists", 100),
    (AgentKind::Base, "gardeners", 100),
    (AgentKind::Base, "intruder combat", 0x25),
    (AgentKind::Base, "moving home", 100),
    (AgentKind::Base, "pioneers", 100),
    (AgentKind::Base, "recruitment", 0xfa),
    (AgentKind::Base, "suicide missions", 100),
    (AgentKind::Base, "squad management", 100),
    (AgentKind::Base, "thieves", 100),
    (AgentKind::Base, "tower occupation", 0x96),
];

/// The Dark Tribe sector AI's agents in their save order (+0x104, +0x148, +0x170, +0x198,
/// +0x1c0, +0x210, +0x1e8, +0x23c; ctor 0x440600, all with period 100).
pub const DARK_AGENTS: [(AgentKind, &str); 8] = [
    (AgentKind::DarkAttack(2), "dark attack"),
    (AgentKind::Base, "dark gardeners"),
    (AgentKind::Base, "mushroom farms"),
    (AgentKind::Base, "dark tribe recruitment"),
    (AgentKind::Base, "shamans"),
    (AgentKind::Base, "dark tribe squad manager"),
    (AgentKind::Base, "manakopters"),
    (AgentKind::ManakopterHalls, "manakopterhalls"),
];

impl SectorAi {
    /// `CAINormalSectorAI` (ctor 0x442180, base 0x43eed0) for `player` at the anchor the
    /// caller found (0x436020: the nearest node of `base` without flag 1): 15 reservoirs in the
    /// slots, the goal caches, 7 reservoirs, the agents.
    pub fn new_normal(player: u8, base: (i32, i32), at: (i32, i32), index: i32, sector: u16) -> SectorAi {
        SectorAi {
            queue: EventQueue::default(),
            slots: (0..15).map(|_| Some((0, TaskForce::reservoir(player)))).collect(),
            group: TaskForceGroup { b4: player as i32, items: Vec::new(), next_uid: 1 },
            raw: [0, base.0 as u32, base.1 as u32, 0, at.0 as u32, at.1 as u32, index as u32, sector as u32],
            kind: SectorKind::Normal {
                caches: (0..5).map(|_| GoalCache::default()).collect(),
                forces: (0..7).map(|_| TaskForce::reservoir(player)).collect(),
                agents: NORMAL_AGENTS.iter().map(|a| Agent::new(a.0)).collect(),
                eco: Vec::new(),
                cursors: Default::default(),
            },
        }
    }

    /// `CAIDarkTribeSectorAI` (ctor 0x440600) at the Dark Tribe's temple or fortress `building`
    /// (its position, its ensign, the ensign's node and sector): reservoirs in slots 2, 3, 4, 7
    /// and 9, the agents.
    pub fn new_dark(player: u8, building: u16, pos: (i32, i32), ensign: (i32, i32), index: i32, sector: u16) -> SectorAi {
        SectorAi {
            queue: EventQueue::default(),
            slots: (0..15).map(|k| [2, 3, 4, 7, 9].contains(&k).then(|| (0, TaskForce::reservoir(player)))).collect(),
            group: TaskForceGroup { b4: player as i32, items: Vec::new(), next_uid: 1 },
            raw: [building as u32, pos.0 as u32, pos.1 as u32, building as u32, ensign.0 as u32, ensign.1 as u32, index as u32, sector as u32],
            kind: SectorKind::Dark { agents: DARK_AGENTS.iter().map(|a| Agent::new(a.0)).collect() },
        }
    }

    /// +0xf4 / +0xf8: the anchor.
    pub fn anchor(&self) -> (i32, i32) {
        (self.raw[4] as i32, self.raw[5] as i32)
    }

    /// +0x100: the (walking) sector of the anchor.
    pub fn sector(&self) -> u16 {
        self.raw[7] as u16
    }

    fn load_base(r: &mut Rd) -> R<SectorBase> {
        r.tag(0xa592_0000)?;
        r.ranged(1, 1)?;
        r.tag(0xa592_0002)?;
        let queue = EventQueue::load(r)?;
        let n = r.ranged(0, 0xf)?;
        let mut slots = Vec::new();
        for _ in 0..n {
            let id = r.i32()?;
            slots.push(if id >= 0 { Some((id, TaskForce::load(r)?)) } else { None });
        }
        let group = TaskForceGroup::load(r)?;
        r.ranged(0x20, 0x20)?;
        let raw = r.words::<8>()?;
        r.tag(0xa592_0001)?;
        Ok((queue, slots, group, raw))
    }

    pub fn load(r: &mut Rd) -> R<SectorAi> {
        let dark = match r.u32()? {
            0xa592_2000 => false,
            0xa592_1000 => true,
            v => return Err(format!("sector AI tag {v:#x}")),
        };
        r.ranged(1, 1)?;
        let (queue, slots, group, raw) = Self::load_base(r)?;
        r.tag(0xa592_0002)?;
        let kind = if dark {
            let agents = (0..8).map(|_| Agent::load(r)).collect::<R<_>>()?;
            r.tag(0xa592_1001)?;
            SectorKind::Dark { agents }
        } else {
            let caches = (0..5).map(|_| GoalCache::load(r)).collect::<R<_>>()?;
            let forces = (0..7).map(|_| TaskForce::load(r)).collect::<R<_>>()?;
            let agents = (0..13).map(|_| Agent::load(r)).collect::<R<_>>()?;
            r.tag(0xa592_2001)?;
            SectorKind::Normal { caches, forces, agents, eco: Vec::new(), cursors: Default::default() }
        };
        Ok(SectorAi { queue, slots, group, raw, kind })
    }

    pub fn save(&self, w: &mut Wr) {
        let dark = matches!(self.kind, SectorKind::Dark { .. });
        w.extend([if dark { 0xa592_1000 } else { 0xa592_2000 }, 1]);
        w.extend([0xa592_0000, 1, 0xa592_0002]);
        self.queue.save(w);
        // (0x43f0d0 always writes 15.)
        w.push(0xf);
        for k in 0..15 {
            match self.slots.get(k).and_then(|s| s.as_ref()) {
                None => w.push(u32::MAX),
                Some((_, tf)) => {
                    // +0xc, the member count.
                    put(w, tf.members.len() as i32);
                    tf.save(w);
                }
            }
        }
        self.group.save(w);
        w.push(0x20);
        w.extend(self.raw);
        w.push(0xa592_0001);
        w.push(0xa592_0002);
        match &self.kind {
            SectorKind::Dark { agents } => {
                agents.iter().for_each(|a| a.save(w));
                w.push(0xa592_1001);
            }
            SectorKind::Normal { caches, forces, agents, .. } => {
                caches.iter().for_each(|c| c.save(w));
                forces.iter().for_each(|f| f.save(w));
                agents.iter().for_each(|a| a.save(w));
                w.push(0xa592_2001);
            }
        }
    }
}

/// The saved part of `CAIPlayerAI` and its subclass.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PlayerBody {
    /// +0x80 …: 5 task-force reservoirs (ctor 0x439b20).
    pub reservoirs: Vec<TaskForce>,
    /// +0x210: "global suicide".
    pub suicide: Agent,
    /// +0x238.
    pub squad: TaskForce,
    /// +0x48 / +0x4c: the sector AIs.
    pub sectors: Vec<SectorAi>,
    /// `CAIDarkTribePlayerAI` +0x2a4 / +0x2a8 (the dark temple / fortress the AI works from).
    pub dark: Option<(i32, i32)>,
}

impl PlayerBody {
    /// A new player AI's objects (ctor 0x439b20): 5 reservoirs, the global suicide mission,
    /// the squad task force, no sector AIs yet.
    pub fn new(player: u8, dark: bool) -> PlayerBody {
        let mut squad = TaskForce::new(player, 0);
        squad.squad = Some((0, [0; 3]));
        PlayerBody {
            reservoirs: (0..5).map(|_| TaskForce::reservoir(player)).collect(),
            suicide: Agent::new(AgentKind::GlobalSuicide),
            squad,
            sectors: Vec::new(),
            dark: dark.then_some((0, 0)),
        }
    }

    /// The body with its queue (`CAINormalPlayerAI` / `CAIDarkTribePlayerAI` load). `None` for a
    /// saved "no AI" (version 0).
    pub fn load(r: &mut Rd) -> R<Option<(EventQueue, PlayerBody)>> {
        let dark = match r.u32()? {
            0xa591_2000 => false,
            0xa591_1000 => true,
            v => return Err(format!("player AI tag {v:#x}")),
        };
        let version = r.ranged(0, if dark { 2 } else { 1 })?;
        if version == 0 {
            r.tag(if dark { 0xa591_1001 } else { 0xa591_2001 })?;
            return Ok(None);
        }
        r.tag(0xa591_0000)?;
        let base = r.ranged(1, 2)?;
        r.tag(0xa591_0002)?;
        let queue = EventQueue::load(r)?;
        let reservoirs = (0..5).map(|_| TaskForce::load(r)).collect::<R<Vec<_>>>()?;
        let (suicide, squad) = if base > 1 { (Agent::load(r)?, TaskForce::load(r)?) } else { (Agent::new(AgentKind::GlobalSuicide), TaskForce::default()) };
        r.tag(0xa591_0001)?;
        let mut body = PlayerBody { reservoirs, suicide, squad, sectors: Vec::new(), dark: None };
        if dark {
            r.tag(0xa591_1002)?;
            body.dark = Some((r.i32()?, r.i32()?));
            if version != 1 && r.ranged(0, 1)? != 0 {
                body.sectors.push(SectorAi::load(r)?);
            }
            r.tag(0xa591_1001)?;
        } else {
            r.tag(0xa591_2002)?;
            let n = r.i32()?;
            for _ in 0..n.max(0) {
                body.sectors.push(SectorAi::load(r)?);
            }
            r.tag(0xa591_2001)?;
        }
        Ok(Some((queue, body)))
    }

    /// `CAINormalPlayerAI` 0x43b580 / `CAIDarkTribePlayerAI` 0x43b1d0.
    pub fn save(&self, queue: &EventQueue, w: &mut Wr) {
        let dark = self.dark.is_some();
        w.extend([if dark { 0xa591_1000 } else { 0xa591_2000 }, if dark { 2 } else { 1 }]);
        w.extend([0xa591_0000, 2, 0xa591_0002]);
        queue.save(w);
        self.reservoirs.iter().for_each(|t| t.save(w));
        self.suicide.save(w);
        self.squad.save(w);
        w.push(0xa591_0001);
        if let Some((a, b)) = self.dark {
            w.extend([0xa591_1002, a as u32, b as u32]);
            match self.sectors.first() {
                Some(s) if a > 0 => {
                    w.push(1);
                    s.save(w);
                }
                _ => w.push(0),
            }
            w.push(0xa591_1001);
        } else {
            w.extend([0xa591_2002, self.sectors.len() as u32]);
            self.sectors.iter().for_each(|s| s.save(w));
            w.push(0xa591_2001);
        }
    }
}
