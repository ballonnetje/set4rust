//! Deterministic lockstep, transport-agnostic: who sends what for which tick, when a tick may
//! run, and the state-hash comparison.
//!
//! The original's valid-tick model (`CGameHost::StartNewCycle`, `CMsgStacks`, docs/SIM.md §2.1):
//! * input is only exchanged for communication ticks (`t & COMMUNICATION_TICK_VALUE == 0`, the
//!   even ticks). Every peer sends exactly one message per communication tick: 1054 with its
//!   commands, or 1055 "nothing" (`CGameHost::AllSend`);
//! * a peer's input is stamped `dt` ticks ahead of its valid tick (the virtual tick, `dt` = delay
//!   / 71 ms), see [`crate::commands::execution_tick_dt`];
//! * a communication tick runs only when every player's message for it is there
//!   (`IsMsgStackValid(valid + 1)`; otherwise `NotifyClients` and wait); the other ticks run
//!   freely;
//! * at the communication ticks every peer also sends its state checksum
//!   (`CStateGame`: 0xFA6 with `CMapObjectMgr::CRCLogicUpdate`, and 0xFA3 with a game-RNG value).
//!   Here the full `World::state_hash` is exchanged every `hash_interval` ticks; on a mismatch the
//!   peers exchange per-subsystem hashes (`Packet::Dump`) so the report names what diverged.
//!
//! Commands of one tick run in player order, each player's in issue order. [inf] The original
//! pops its message stacks per player in slot order.
//!
//! **A player leaving** (`RemovePlayer` 0x5a2da0, from the disconnect handlers and the not-ready
//! kick 0x5a2b87): its message stacks are no longer awaited, its control becomes 2 / 3 by the
//! AI level (0x11b5948), +0x204 notes the turn it left at, event 0x39 follows. Ours, made
//! deterministic over our transport: a departure is `(player, T)` with `T` the first
//! communication tick it has no turn for — from the leaver itself ([`Lockstep::leave`]: its next
//! turn), or, for a peer silent past `NotReadyKickDelay`, from the deciding peer ([`Lockstep::kick_tick`]:
//! the most any peer is known to hold of its turns; the decider is the lowest player still in).
//! Every peer learns it (it travels with every turn packet), stops awaiting the player from `T`,
//! drops its turns from `T` on, and runs [`PLAYER_LEFT`] as that player's command of tick `T`;
//! the departed player's turns before `T` are relayed by whoever holds them
//! ([`Lockstep::relay`]), so a peer that missed one still gets it. [inf] A peer that is itself
//! silent while the decider kicks someone may hold more of that player's turns than the decider
//! knew of; that case is not handled.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use s4_sim::{Command, PlayerCommand};

use crate::commands::{COMMUNICATION_TICK_VALUE, execution_tick_dt};
use crate::wire::Turn;

pub fn is_comm_tick(t: u32) -> bool {
    t & COMMUNICATION_TICK_VALUE == 0
}

/// The communication tick after `t`.
fn next_comm(t: u32) -> u32 {
    let n = t + 1;
    if is_comm_tick(n) { n } else { n + 1 }
}

/// A difference between two peers' state hashes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Desync {
    pub tick: u32,
    pub player: u8,
    pub local: u64,
    pub remote: u64,
}

/// The command the lockstep runs for a departed player at its first missing turn: the release's
/// kick message number (0x5a2b87 sends 0xfa9); wparam = the player, lparam = its new control
/// (`2 + (AI level != 0)`, 0x5a2da0). No data.
pub const PLAYER_LEFT: u32 = 0xFA9;

/// Executed turns kept for relaying (communication ticks).
const RAN_KEPT: usize = 128;

/// A peer's own turns by communication tick.
pub type OwnTurns = Vec<(u32, Vec<PlayerCommand>)>;

/// `CConfigManager` "NETWORK" defaults (static initialisers 0x425660 / 0x425700 / 0x4257a0), in
/// milliseconds: a missing player is warned about after `NotReadyWarnDelay`, again at most every
/// `NotReadyWarnAgainDelay`, and kicked after `NotReadyKickDelay`.
pub const NOT_READY_WARN_DELAY: u64 = 6000;
pub const NOT_READY_WARN_AGAIN_DELAY: u64 = 2500;
pub const NOT_READY_KICK_DELAY: u64 = 20000;

/// What [`NotReady::check`] found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NotReadyOutcome {
    /// Players to warn about: 0x52a210(0x8d5, player, 0, 1, name).
    pub warn: Vec<u8>,
    /// Players past the kick delay: the deciding peer removes them ([`Lockstep::kick_tick`],
    /// 0x5a2da0; the release also sends 0xfa9).
    pub kick: Vec<u8>,
}

/// The waiting-for-players timer of 0x5a2a90 (called by 0x5a2970 when 0x5a8fa0 finds the next
/// turn incomplete, with the mask of the players whose messages are missing).
#[derive(Clone, Debug, Default)]
pub struct NotReady {
    /// Per player: when it was first found missing (the player record +0x14).
    since: BTreeMap<u8, std::time::Instant>,
    /// The last warning (+0xc0 of the session object; one for all players).
    last_warn: Option<std::time::Instant>,
    /// Tests: another kick delay than [`NOT_READY_KICK_DELAY`] (ms).
    pub kick_ms: Option<u64>,
}

impl NotReady {
    /// 0x5a2a90 for the players `missing` at `now` (the local one never counts): a player seen
    /// missing for the first time gets its time; past the kick delay it is to be kicked; else
    /// past the warn delay, and with no warning within the warn-again delay, it is warned about
    /// (and that warning holds back the others of this call). [inf] A player no longer missing
    /// starts again from zero (its record's time cleared when its turn is there).
    pub fn check(&mut self, now: std::time::Instant, missing: &[u8], local: u8) -> NotReadyOutcome {
        use std::time::Duration;
        let kick = Duration::from_millis(self.kick_ms.unwrap_or(NOT_READY_KICK_DELAY));
        self.since.retain(|p, _| missing.contains(p));
        let mut out = NotReadyOutcome::default();
        for &p in missing.iter().filter(|&&p| p != local) {
            let Some(&t) = self.since.get(&p) else {
                self.since.insert(p, now);
                continue;
            };
            if now > t + kick {
                out.kick.push(p);
            } else if now > t + Duration::from_millis(NOT_READY_WARN_DELAY)
                && self.last_warn.is_none_or(|w| now > w + Duration::from_millis(NOT_READY_WARN_AGAIN_DELAY))
            {
                self.last_warn = Some(now);
                out.warn.push(p);
            }
        }
        out
    }
}

pub struct Lockstep {
    pub local: u8,
    players: Vec<u8>,
    pub dt: u32,
    pub hash_interval: u32,
    /// The next communication tick the local player has not produced a turn for.
    next_turn: u32,
    /// Local commands waiting for the next turn.
    pending: Vec<Command>,
    /// Received and own turns not yet executed: tick → player → commands.
    turns: BTreeMap<u32, BTreeMap<u8, Vec<PlayerCommand>>>,
    /// Per player: the communication tick up to which every turn has arrived (or run).
    held: HashMap<u8, u32>,
    /// Own turns not yet acknowledged by every other player.
    outbox: BTreeMap<u32, Turn>,
    /// Per other player: up to which tick it holds our turns.
    acked: HashMap<u8, u32>,
    /// Own state hashes (and per-subsystem parts) of the recent exchange ticks.
    own_hashes: BTreeMap<u32, (u64, Vec<(String, u64)>)>,
    /// Hashes received before our own for that tick existed.
    remote_hashes: BTreeMap<u32, Vec<(u8, u64)>>,
    /// (tick, player) hashes compared already: hashes are resent, a desync is reported once.
    compared: BTreeSet<(u32, u8)>,
    pub desyncs: Vec<Desync>,
    /// The waiting-for-players timer (0x5a2a90).
    pub not_ready: NotReady,
    /// Departed players and their first missing communication tick.
    left: BTreeMap<u8, u32>,
    /// The last [`RAN_KEPT`] executed turns (for relaying a departed player's).
    ran: BTreeMap<u32, BTreeMap<u8, Vec<PlayerCommand>>>,
    /// Per other peer: its last acknowledgement of every player (what it holds).
    peer_held: HashMap<u8, HashMap<u8, u32>>,
    /// The host's AI level (`GameSettings::ai_level`): a departed player's control.
    pub ai_level: u8,
}

impl Lockstep {
    /// `players`: every player id in the game (including `local`); `start`: the tick the world is
    /// at (0 for a new game).
    pub fn new(local: u8, players: &[u8], dt: u32, hash_interval: u32, start: u32) -> Lockstep {
        let mut players = players.to_vec();
        players.sort_unstable();
        players.dedup();
        let first = next_comm(start);
        let held = players.iter().map(|&p| (p, first.saturating_sub(2))).collect();
        let acked = players.iter().filter(|&&p| p != local).map(|&p| (p, 0)).collect();
        Lockstep {
            local,
            players,
            dt,
            hash_interval: hash_interval.max(1),
            next_turn: first,
            pending: Vec::new(),
            turns: BTreeMap::new(),
            held,
            outbox: BTreeMap::new(),
            acked,
            own_hashes: BTreeMap::new(),
            remote_hashes: BTreeMap::new(),
            compared: BTreeSet::new(),
            desyncs: Vec::new(),
            not_ready: NotReady::default(),
            left: BTreeMap::new(),
            ran: BTreeMap::new(),
            peer_held: HashMap::new(),
            ai_level: 0,
        }
    }

    pub fn players(&self) -> &[u8] {
        &self.players
    }

    /// Queue a local command; it goes out with the next turn.
    pub fn issue(&mut self, c: Command) {
        self.pending.push(c);
    }

    /// `CGameHost::AllSend` for every communication tick up to the virtual tick, when `last` is
    /// the last tick that ran: the pending commands go into the turn of the virtual tick, the
    /// earlier ones (only at the start) are empty. Returns the new turns.
    pub fn produce(&mut self, last: u32) -> Vec<Turn> {
        let virt = execution_tick_dt(last, self.dt);
        // Gone (left or kicked): no more turns.
        if self.left.contains_key(&self.local) {
            return Vec::new();
        }
        let mut out = Vec::new();
        while self.next_turn <= virt {
            let t = self.next_turn;
            let cmds = if t == virt { std::mem::take(&mut self.pending) } else { Vec::new() };
            let pcs: Vec<PlayerCommand> = cmds.into_iter().map(|c| PlayerCommand::new(self.local, c)).collect();
            let turn = Turn { tick: t, player: self.local, messages: pcs.iter().map(|c| c.to_message()).collect() };
            self.insert(t, self.local, pcs);
            // Kept until acknowledged, while anybody else is still in.
            if self.players.iter().any(|p| *p != self.local && !self.left.contains_key(p)) {
                self.outbox.insert(t, turn.clone());
            }
            out.push(turn);
            self.next_turn = next_comm(t);
        }
        out
    }

    fn insert(&mut self, tick: u32, player: u8, cmds: Vec<PlayerCommand>) {
        let held = self.held.get(&player).copied().unwrap_or(0);
        if tick <= held {
            return; // a resend of a turn we have (or have run)
        }
        self.turns.entry(tick).or_default().entry(player).or_insert(cmds);
        // Advance the contiguous mark.
        let mut h = held;
        while self.turns.get(&next_comm(h)).is_some_and(|m| m.contains_key(&player)) {
            h = next_comm(h);
        }
        self.held.insert(player, h);
    }

    /// A turn from another peer (resends are ignored).
    pub fn receive(&mut self, turn: &Turn) {
        self.receive_from(turn, turn.player);
    }

    /// A turn that peer `from` sent: its own, or a departed player's turn before it left
    /// (relayed, [`Lockstep::relay`]). A departed player's turns from its first missing tick on
    /// are dropped.
    pub fn receive_from(&mut self, turn: &Turn, from: u8) {
        if turn.player == self.local || !self.players.contains(&turn.player) || !is_comm_tick(turn.tick) {
            return;
        }
        let gone = self.left.get(&turn.player).copied();
        if gone.is_some_and(|t| turn.tick >= t) || (from != turn.player && gone.is_none()) {
            return;
        }
        // Each message carries its owner; it must be the turn's player.
        let cmds = turn
            .messages
            .iter()
            .map(PlayerCommand::from_message)
            .filter(|c| c.player == turn.player)
            .collect();
        self.insert(turn.tick, turn.player, cmds);
    }

    /// Our acknowledgement for the others: per player, up to which tick we hold its turns.
    pub fn acks(&self) -> Vec<(u8, u32)> {
        self.players.iter().map(|p| (*p, self.held.get(p).copied().unwrap_or(0))).collect()
    }

    /// Another peer's acknowledgement: drop our turns it (and everyone) holds.
    pub fn on_acks(&mut self, from: u8, acks: &[(u8, u32)]) {
        self.peer_held.insert(from, acks.iter().copied().collect());
        if let Some(&(_, t)) = acks.iter().find(|a| a.0 == self.local)
            && let Some(a) = self.acked.get_mut(&from)
        {
            *a = (*a).max(t);
        }
        let all = self.acked.values().copied().min().unwrap_or(u32::MAX);
        self.outbox = self.outbox.split_off(&(all + 1));
    }

    /// Own turns some peer does not hold yet (to (re)send).
    pub fn unacked(&self) -> impl Iterator<Item = &Turn> {
        self.outbox.values()
    }

    /// The commands for tick `next`, if it may run: a communication tick needs the turn of every
    /// player still in; other ticks carry no input. A departed player's first missing tick runs
    /// its [`PLAYER_LEFT`] in its place.
    pub fn ready(&mut self, next: u32) -> Option<Vec<PlayerCommand>> {
        if !is_comm_tick(next) {
            return Some(Vec::new());
        }
        let control = 2 + (self.ai_level != 0) as u32;
        for (&p, &t) in &self.left {
            if t == next {
                let m = s4_sim::NetMessage { kind: PLAYER_LEFT, player: p, wparam: p as u32, lparam: control, data: Vec::new() };
                self.turns.entry(next).or_default().insert(p, vec![PlayerCommand::from_message(&m)]);
            }
        }
        let m = self.turns.get(&next)?;
        if !self.players.iter().filter(|p| self.awaited(**p, next)).all(|p| m.contains_key(p)) {
            return None;
        }
        let mut m = self.turns.remove(&next)?;
        m.retain(|p, _| self.left.get(p).is_none_or(|&t| next <= t));
        self.ran.insert(next, m.clone());
        while self.ran.len() > RAN_KEPT {
            self.ran.pop_first();
        }
        Some(m.into_values().flatten().collect())
    }

    /// Whether `player`'s turn for `next` is needed (it is in, or it left after `next`).
    fn awaited(&self, player: u8, next: u32) -> bool {
        self.left.get(&player).is_none_or(|&t| next <= t)
    }

    /// Which players' turns for `next` are missing (for a stall report).
    pub fn missing(&self, next: u32) -> Vec<u8> {
        let have = self.turns.get(&next);
        self.players.iter().copied().filter(|p| self.awaited(*p, next) && !have.is_some_and(|m| m.contains_key(p))).collect()
    }

    /// The departed players: (player, first missing communication tick).
    pub fn departed(&self) -> Vec<(u8, u32)> {
        self.left.iter().map(|(&p, &t)| (p, t)).collect()
    }

    /// `player` left with its first missing turn at `tick` (a communication tick). Returns
    /// whether this is news. Its turns are no longer acknowledged-for (our outbox does not wait
    /// for it).
    pub fn mark_left(&mut self, player: u8, tick: u32) -> bool {
        if !self.players.contains(&player) || !is_comm_tick(tick) || self.left.contains_key(&player) {
            return false;
        }
        self.left.insert(player, tick);
        self.acked.remove(&player);
        self.peer_held.remove(&player);
        let all = self.acked.values().copied().min().unwrap_or(u32::MAX);
        self.outbox = self.outbox.split_off(&(all.saturating_add(1)));
        // Its turns from `tick` on never run.
        for m in self.turns.range_mut(tick + 1..).map(|(_, m)| m) {
            m.remove(&player);
        }
        true
    }

    /// We leave: our first turn not produced is the departure tick. Returns it.
    pub fn leave(&mut self) -> u32 {
        let t = self.next_turn;
        let me = self.local;
        self.left.insert(me, t);
        t
    }

    /// The deciding peer for a kick: the lowest player still in.
    pub fn decider(&self) -> Option<u8> {
        self.players.iter().copied().find(|p| !self.left.contains_key(p))
    }

    /// The departure tick for dropping `player` when nobody can be reached any more: after the
    /// last of its turns we hold ourselves (nothing more will be relayed).
    pub fn own_kick_tick(&self, player: u8) -> u32 {
        next_comm(self.held.get(&player).copied().unwrap_or(0))
    }

    /// The departure tick for kicking the silent `player`: after the most of its turns any
    /// peer (ourselves included) is known to hold.
    pub fn kick_tick(&self, player: u8) -> u32 {
        let own = self.held.get(&player).copied().unwrap_or(0);
        let most = self.peer_held.values().filter_map(|h| h.get(&player).copied()).fold(own, u32::max);
        next_comm(most)
    }

    /// A departed player's turns some peer still lacks (to send along with ours): those before
    /// its departure tick past the lowest any other peer still in has acknowledged.
    pub fn relay(&self) -> Vec<Turn> {
        let mut out = Vec::new();
        for (&p, &t) in &self.left {
            if p == self.local {
                continue;
            }
            let lowest = self
                .players
                .iter()
                .filter(|&&q| q != self.local && q != p && !self.left.contains_key(&q))
                .map(|q| self.peer_held.get(q).and_then(|h| h.get(&p).copied()).unwrap_or(0))
                .min();
            let Some(lowest) = lowest else { continue };
            for (&tick, m) in self.ran.range(lowest + 1..t).chain(self.turns.range(lowest + 1..t)) {
                if let Some(c) = m.get(&p) {
                    out.push(Turn { tick, player: p, messages: c.iter().map(|c| c.to_message()).collect() });
                }
            }
        }
        out
    }

    /// Our own turns are all acknowledged (a leaver may go).
    pub fn all_acked(&self) -> bool {
        self.outbox.is_empty()
    }

    /// Whether the state after `tick` is exchanged.
    pub fn hash_due(&self, tick: u32) -> bool {
        tick > 0 && tick.is_multiple_of(self.hash_interval)
    }

    /// Record our hash after `tick`; compares with what already arrived.
    pub fn record_hash(&mut self, tick: u32, hash: u64, parts: Vec<(String, u64)>) {
        self.own_hashes.insert(tick, (hash, parts));
        // Keep the last 32 exchanges for late hashes and dumps.
        while self.own_hashes.len() > 32 {
            self.own_hashes.pop_first();
        }
        if let Some((&oldest, _)) = self.own_hashes.first_key_value() {
            self.compared.retain(|c| c.0 >= oldest);
        }
        if let Some(rs) = self.remote_hashes.remove(&tick) {
            for (p, h) in rs {
                self.compare(tick, p, h);
            }
        }
    }

    /// A peer's hash after `tick`.
    pub fn remote_hash(&mut self, from: u8, tick: u32, hash: u64) {
        if self.own_hashes.contains_key(&tick) {
            self.compare(tick, from, hash);
        } else {
            self.remote_hashes.entry(tick).or_default().push((from, hash));
        }
    }

    fn compare(&mut self, tick: u32, player: u8, remote: u64) {
        let Some((local, _)) = self.own_hashes.get(&tick) else { return };
        if !self.compared.insert((tick, player)) {
            return;
        }
        if *local != remote {
            self.desyncs.push(Desync { tick, player, local: *local, remote });
        }
    }

    /// Our last `n` hashes (tick, hash), the newest last (they are resent: a lost one would
    /// delay a desync's detection by an interval).
    pub fn recent_hashes(&self, n: usize) -> Vec<(u32, u64)> {
        let mut v: Vec<(u32, u64)> = self.own_hashes.iter().rev().take(n).map(|(t, (h, _))| (*t, *h)).collect();
        v.reverse();
        v
    }

    /// Our per-subsystem hashes after `tick`, if still kept.
    pub fn parts(&self, tick: u32) -> Option<&[(String, u64)]> {
        self.own_hashes.get(&tick).map(|(_, p)| p.as_slice())
    }

    /// What a network save keeps of the lockstep: our own turns not yet run (tick → commands,
    /// empty turns included), the last communication tick we produced, and the commands not yet
    /// in a turn. Every peer saves after the same tick and has produced its turns up to the
    /// same virtual tick, so the turns of every player are in some peer's save.
    pub fn own_pending(&self) -> (OwnTurns, u32, Vec<Command>) {
        let turns = self.turns.iter().filter_map(|(&t, m)| m.get(&self.local).map(|c| (t, c.clone()))).collect();
        let upto = self.next_turn.saturating_sub(2);
        (turns, upto, self.pending.clone())
    }

    /// A loaded network save: our own turns from [`Lockstep::own_pending`] go back in (and out
    /// again to the others), the next one produced follows `upto`.
    pub fn resume(&mut self, turns: OwnTurns, upto: u32, pending: Vec<Command>) {
        let mut t = self.next_turn;
        while t <= upto {
            let cmds = turns.iter().find(|(tt, _)| *tt == t).map(|(_, c)| c.clone()).unwrap_or_default();
            let turn = Turn { tick: t, player: self.local, messages: cmds.iter().map(|c| c.to_message()).collect() };
            self.insert(t, self.local, cmds);
            if self.players.len() > 1 {
                self.outbox.insert(t, turn);
            }
            t = next_comm(t);
        }
        self.next_turn = self.next_turn.max(t);
        self.pending = pending;
    }
}

/// A desync report from both peers' per-subsystem hashes: every part, marking the ones that
/// differ.
pub fn dump_report(tick: u32, local: &[(String, u64)], remote_player: u8, remote: &[(String, u64)]) -> String {
    let mut s = format!("desync after tick {tick}: subsystem hashes (local vs player {remote_player})\n");
    for (name, h) in local {
        let r = remote.iter().find(|(n, _)| n == name).map(|(_, h)| *h);
        let mark = if r == Some(*h) { "  " } else { "!=" };
        s.push_str(&format!("  {mark} {name:<12} {h:016x} {}\n", r.map_or("-".to_string(), |r| format!("{r:016x}"))));
    }
    s
}

#[cfg(test)]
mod tests {

    /// 0x5a2a90's timer: first seen, warned after 6 s, held back 2.5 s (one warning for all
    /// players), kicked after 20 s; the local player never; a player back clears its time.
    #[test]
    fn not_ready_timer() {
        use std::time::{Duration, Instant};
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        let mut n = NotReady::default();
        assert_eq!(n.check(at(0), &[1, 2, 3], 1), NotReadyOutcome::default());
        assert_eq!(n.check(at(6000), &[2, 3], 1).warn, Vec::<u8>::new(), "not past 6 s");
        assert_eq!(n.check(at(6001), &[2, 3], 1).warn, [2], "the first warning holds back 3");
        assert_eq!(n.check(at(8501), &[2, 3], 1).warn, Vec::<u8>::new());
        assert_eq!(n.check(at(8502), &[2, 3], 1).warn, [2]);
        assert_eq!(n.check(at(9000), &[3], 1), NotReadyOutcome::default(), "2 is back");
        assert_eq!(n.check(at(20001), &[2, 3], 1), NotReadyOutcome { warn: vec![], kick: vec![3] }, "2 starts again");
    }

    use super::*;

    fn cmd(b: u32) -> Command {
        Command::SwitchBuilding { building: b }
    }

    /// Two peers run to tick 9 with commands in flight, "save" (own pending turns), and two new
    /// lockstep objects started at tick 9 resume from them: every later tick gets the same
    /// commands as the uninterrupted pair.
    #[test]
    fn resume_from_saved_own_turns() {
        let run = |a: &mut Lockstep, b: &mut Lockstep, from: u32, to: u32, issue: &dyn Fn(u32, &mut Lockstep, &mut Lockstep)| -> Vec<(u32, Vec<PlayerCommand>)> {
            let mut out = Vec::new();
            for last in from..to {
                issue(last, a, b);
                let (ta, tb) = (a.produce(last), b.produce(last));
                for t in &ta {
                    b.receive(t);
                }
                for t in &tb {
                    a.receive(t);
                }
                let (ca, cb) = (a.ready(last + 1).unwrap(), b.ready(last + 1).unwrap());
                assert_eq!(ca, cb);
                out.push((last + 1, ca));
            }
            out
        };
        let issue = |last: u32, a: &mut Lockstep, b: &mut Lockstep| match last {
            7 => a.issue(cmd(1)),
            8 => {
                b.issue(cmd(2));
                a.issue(cmd(3));
            }
            12 => b.issue(cmd(4)),
            _ => {}
        };
        let (mut a, mut b) = (Lockstep::new(1, &[1, 2], 4, 64, 0), Lockstep::new(2, &[1, 2], 4, 64, 0));
        let whole = run(&mut a, &mut b, 0, 24, &issue);
        let (mut a, mut b) = (Lockstep::new(1, &[1, 2], 4, 64, 0), Lockstep::new(2, &[1, 2], 4, 64, 0));
        let mut first = run(&mut a, &mut b, 0, 9, &issue);
        let (sa, sb) = (a.own_pending(), b.own_pending());
        assert!(sa.0.iter().any(|(_, c)| !c.is_empty()), "a command is in flight at the save: {sa:?}");
        let (mut a2, mut b2) = (Lockstep::new(1, &[1, 2], 4, 64, 9), Lockstep::new(2, &[1, 2], 4, 64, 9));
        a2.resume(sa.0, sa.1, sa.2);
        b2.resume(sb.0, sb.1, sb.2);
        // The resumed own turns go out to the other peer again.
        for t in a2.unacked().cloned().collect::<Vec<_>>() {
            b2.receive(&t);
        }
        for t in b2.unacked().cloned().collect::<Vec<_>>() {
            a2.receive(&t);
        }
        first.extend(run(&mut a2, &mut b2, 9, 24, &issue));
        assert_eq!(first, whole);
    }

    #[test]
    fn ticks_wait_for_every_players_turn() {
        let mut a = Lockstep::new(1, &[1, 2], 2, 64, 0);
        let mut b = Lockstep::new(2, &[1, 2], 2, 64, 0);
        // Start: turns for 2 and 4 (virtual tick = 0 + 2 + 2).
        let ta = a.produce(0);
        let tb = b.produce(0);
        assert_eq!(ta.iter().map(|t| t.tick).collect::<Vec<_>>(), vec![2, 4]);
        assert_eq!(a.ready(1), Some(vec![]));
        assert_eq!(a.ready(2), None);
        assert_eq!(a.missing(2), vec![2]);
        for t in &tb {
            a.receive(t);
        }
        for t in &ta {
            b.receive(t);
        }
        assert_eq!(a.ready(2), Some(vec![]));
        // A command issued after tick 2 lands in the turn of tick 6 on both sides.
        a.issue(cmd(9));
        let ta = a.produce(2);
        let tb = b.produce(2);
        assert_eq!(ta.len(), 1);
        assert_eq!(ta[0].tick, 6);
        for t in &ta {
            b.receive(t);
            b.receive(t); // a resend changes nothing
        }
        for t in &tb {
            a.receive(t);
        }
        assert_eq!(b.ready(4), Some(vec![]));
        assert_eq!(b.ready(6), Some(vec![PlayerCommand::new(1, cmd(9))]));
        assert_eq!(a.ready(4), Some(vec![]));
        assert_eq!(a.ready(6), Some(vec![PlayerCommand::new(1, cmd(9))]));
    }

    #[test]
    fn acks_prune_the_outbox() {
        let mut a = Lockstep::new(1, &[1, 2], 0, 64, 0);
        let mut b = Lockstep::new(2, &[1, 2], 0, 64, 0);
        for t in a.produce(0).iter().chain(&a.produce(2)) {
            b.receive(t);
        }
        assert_eq!(a.unacked().count(), 2);
        a.on_acks(2, &b.acks());
        assert_eq!(a.unacked().count(), 0);
    }

    /// Two cores exchanging through a channel that drops every third delivery: resending the
    /// unacknowledged turns gets every tick the same commands on both sides.
    #[test]
    fn lossy_exchange_converges() {
        let mut peers = [Lockstep::new(1, &[1, 2], 3, 64, 0), Lockstep::new(2, &[1, 2], 3, 64, 0)];
        let mut ran: [Vec<(u32, Vec<PlayerCommand>)>; 2] = [Vec::new(), Vec::new()];
        let mut tick = [0u32; 2];
        let mut n = 0u32;
        for round in 0..2000 {
            for i in 0..2 {
                if round % 37 == 5 + i as u32 {
                    peers[i].issue(cmd(round));
                }
                peers[i].produce(tick[i]);
                let (turns, acks): (Vec<Turn>, _) = (peers[i].unacked().cloned().collect(), peers[i].acks());
                let other = 1 - i;
                for t in turns {
                    n += 1;
                    if !n.is_multiple_of(3) {
                        peers[other].receive(&t);
                    }
                }
                n += 1;
                if !n.is_multiple_of(3) {
                    let from = peers[i].local;
                    peers[other].on_acks(from, &acks);
                }
                while let Some(c) = peers[i].ready(tick[i] + 1) {
                    tick[i] += 1;
                    if !c.is_empty() {
                        ran[i].push((tick[i], c));
                    }
                    if tick[i].is_multiple_of(4) {
                        break;
                    }
                }
            }
        }
        assert!(tick[0] > 500 && tick[1] > 500, "{tick:?}");
        let common = tick[0].min(tick[1]);
        let upto = |r: &Vec<(u32, Vec<PlayerCommand>)>| r.iter().filter(|e| e.0 <= common).cloned().collect::<Vec<_>>();
        assert_eq!(upto(&ran[0]), upto(&ran[1]));
        assert!(upto(&ran[0]).len() > 20);
    }

    /// Exchanges every peer's turns, acks and departures with every other (a perfect network),
    /// optionally leaving one out (silent); runs each peer's ready ticks. Returns what ran.
    fn exchange(peers: &mut [Lockstep], ticks: &mut [u32], silent: Option<usize>, ran: &mut [Vec<(u32, Vec<PlayerCommand>)>]) {
        let n = peers.len();
        for i in 0..n {
            if Some(i) == silent {
                continue;
            }
            peers[i].produce(ticks[i]);
            let turns: Vec<Turn> = peers[i].unacked().cloned().chain(peers[i].relay()).collect();
            let (acks, left, from) = (peers[i].acks(), peers[i].departed(), peers[i].local);
            for j in (0..n).filter(|&j| j != i && Some(j) != silent) {
                for (p, t) in &left {
                    peers[j].mark_left(*p, *t);
                }
                for t in &turns {
                    peers[j].receive_from(t, from);
                }
                peers[j].on_acks(from, &acks);
            }
        }
        for i in (0..n).filter(|&i| Some(i) != silent) {
            while let Some(c) = peers[i].ready(ticks[i] + 1) {
                ticks[i] += 1;
                ran[i].push((ticks[i], c));
            }
        }
    }

    /// Three peers; player 3 leaves at its next turn (it lingers until acked): the other two run
    /// on, run its [`PLAYER_LEFT`] at the same tick (control 3 at AI level 1), and stop waiting
    /// for it. Then player 2 goes silent; the decider (player 1) kicks it at the tick after the
    /// most of its turns anyone holds; player 1 runs on alone with the same commands as player 2
    /// had up to its silence.
    #[test]
    fn players_leave_and_are_kicked() {
        let mut peers: Vec<Lockstep> = (1..=3).map(|p| Lockstep::new(p, &[1, 2, 3], 3, 64, 0)).collect();
        for p in &mut peers {
            p.ai_level = 1;
        }
        let mut ticks = [0u32; 3];
        let mut ran: [Vec<(u32, Vec<PlayerCommand>)>; 3] = Default::default();
        for round in 0..20 {
            if round == 4 {
                peers[2].issue(cmd(33));
            }
            exchange(&mut peers, &mut ticks, None, &mut ran);
        }
        let t3 = peers[2].leave();
        // The leaver lingers (exchanges) until its turns are acknowledged, then goes.
        for _ in 0..5 {
            exchange(&mut peers, &mut ticks, None, &mut ran);
        }
        assert!(peers[2].all_acked());
        for _ in 0..30 {
            exchange(&mut peers, &mut ticks, Some(2), &mut ran);
        }
        assert!(ticks[0] > t3 + 10 && ticks[1] > t3 + 10, "{ticks:?}, left at {t3}");
        let left_cmd = |r: &[(u32, Vec<PlayerCommand>)], p: u8| r.iter().filter(|e| e.1.iter().any(|c| c.player == p && c.command.kind() == PLAYER_LEFT)).map(|e| (e.0, e.1.clone())).collect::<Vec<_>>();
        let l1 = left_cmd(&ran[0], 3);
        assert_eq!(l1.len(), 1, "one PLAYER_LEFT for 3");
        assert_eq!(l1[0].0, t3);
        assert_eq!(l1, left_cmd(&ran[1], 3));
        let c = l1[0].1.iter().find(|c| c.player == 3).unwrap().to_message();
        assert_eq!((c.wparam, c.lparam), (3, 3));
        // Player 2 goes silent (no turns, no acks); player 1 decides after the kick delay.
        let at = ticks[1];
        for _ in 0..10 {
            exchange(&mut peers, &mut ticks, Some(1), &mut ran);
        }
        assert_eq!(peers[0].decider(), Some(1));
        let k = peers[0].kick_tick(2);
        assert!(peers[0].mark_left(2, k));
        for _ in 0..30 {
            let mut solo = [peers.remove(0)];
            let mut t = [ticks[0]];
            exchange(&mut solo, &mut t, None, std::slice::from_mut(&mut ran[0]));
            ticks[0] = t[0];
            peers.insert(0, solo.into_iter().next().unwrap());
        }
        assert!(ticks[0] > k + 10, "player 1 runs on alone: {} (kick at {k})", ticks[0]);
        // Everything both ran up to player 2's last tick is the same.
        let upto = |r: &[(u32, Vec<PlayerCommand>)]| r.iter().filter(|e| e.0 <= at).cloned().collect::<Vec<_>>();
        assert_eq!(upto(&ran[0]), upto(&ran[1]));
        assert_eq!(left_cmd(&ran[0], 2).iter().map(|e| e.0).collect::<Vec<_>>(), [k]);
        assert!(ran[0].iter().any(|e| e.1.contains(&PlayerCommand::new(3, cmd(33)))), "3's command before it left ran");
    }

    /// A departed player's turn that one peer missed is relayed by another.
    #[test]
    fn a_missed_turn_of_a_leaver_is_relayed() {
        let mut a = Lockstep::new(1, &[1, 2, 3], 2, 64, 0);
        let mut b = Lockstep::new(2, &[1, 2, 3], 2, 64, 0);
        let mut c = Lockstep::new(3, &[1, 2, 3], 2, 64, 0);
        let (ta, tb, tc) = (a.produce(0), b.produce(0), c.produce(0));
        // c's turns reach a only; c then leaves at its next turn.
        for t in &tc {
            a.receive(t);
        }
        for t in &ta {
            b.receive(t);
            c.receive(t);
        }
        for t in &tb {
            a.receive(t);
            c.receive(t);
        }
        let tl = c.leave();
        a.mark_left(3, tl);
        b.mark_left(3, tl);
        assert_eq!(b.ready(2), None, "b lacks 3's turn of tick 2");
        b.on_acks(1, &a.acks());
        a.on_acks(2, &b.acks());
        for t in a.relay() {
            b.receive_from(&t, 1);
        }
        assert!(b.ready(2).is_some() && b.ready(4).is_some());
        assert!(a.ready(2).is_some() && a.ready(4).is_some());
        assert_eq!(a.ready(tl), b.ready(tl));
    }

    #[test]
    fn hash_mismatch_is_reported() {
        let mut a = Lockstep::new(1, &[1, 2], 0, 64, 0);
        a.remote_hash(2, 64, 5);
        a.record_hash(64, 5, vec![]);
        assert!(a.desyncs.is_empty());
        a.record_hash(128, 6, vec![]);
        a.remote_hash(2, 128, 7);
        assert_eq!(a.desyncs, vec![Desync { tick: 128, player: 2, local: 6, remote: 7 }]);
        // Resent hashes (and one that came before ours) are compared once.
        a.remote_hash(2, 128, 7);
        a.remote_hash(2, 192, 8);
        a.remote_hash(2, 192, 8);
        a.record_hash(192, 9, vec![]);
        assert_eq!(a.desyncs.len(), 2);
        assert_eq!(a.recent_hashes(2), vec![(128, 6), (192, 9)]);
        let r = dump_report(128, &[("rng".into(), 1), ("entities".into(), 2)], 2, &[("rng".into(), 1), ("entities".into(), 3)]);
        assert!(r.contains("!= entities") && r.contains("   rng"));
    }
}
