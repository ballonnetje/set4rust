//! Commands stamped with the tick they execute at (moved here from s4-game, which re-exports
//! them): the command log, the single-player queue and the original's tick arithmetic.
//!
//! The original sends player input through the network engine, which delivers it "for a later
//! valid tick"; every peer runs it at the start of that tick (`CInputProcessor::Process`, see
//! docs/SIM.md §2). Here every command carries its execution tick from the moment it is issued, so
//! the same log replays identically, in single player and over the network (lockstep.rs).

use std::collections::BTreeMap;

use s4_sim::PlayerCommand;

/// `COMMUNICATION_TICK_VALUE`, used as a mask: input is only sent on ticks `t` with
/// `t & COMMUNICATION_TICK_VALUE == 0`. The release exe folds it to 1: the `CMsgStacks` ctor
/// (0x5a3de1) computes `pow(1, 2)` for its stack count (`2·dt + 1 + COMMUNICATION_TICK_VALUE²`).
pub const COMMUNICATION_TICK_VALUE: u32 = 1;

/// Milliseconds per tick at game speed 1 (`CStateGame`), the unit of the network delay `dt`.
pub const TICK_MS: u32 = 71;

/// The tick a command issued by the local player executes at, when `last` is the last tick that
/// has run.
///
/// `CStateGame`'s loop, single player (`CMsgStacks(players, dt = 0, start)`: virtual tick =
/// valid tick + 1, then + 2 from the first cycle on): UI input is dispatched at the top of the
/// iteration (`DispatchSystemMessages`) and queued (`CGameHost::PushMsg`); the iteration runs
/// tick `last + 1` (`StartNewTick`, `CGameRun::Run`), then `INetworkEngine::EndTick` →
/// `CGameHost::AllSend` stamps the queue with the event engine's tick (`m_uVirtualTick`, read at
/// the top of the iteration = `last + 2`) and pushes it into the message stack of that tick; it
/// returns without sending when that tick `& COMMUNICATION_TICK_VALUE` is non-zero, leaving the
/// queue for the next iteration (tick `last + 3`). So: the first even tick from `last + 2`.
pub fn execution_tick(last: u32) -> u32 {
    execution_tick_dt(last, 0)
}

/// [`execution_tick`] in a network game: `CGameHost::StartNewCycle` keeps the virtual tick `dt`
/// ticks ahead of the valid tick (`valid + dt > virtual` → `AllSend` + `AdvanceVirtualTick`
/// without a logic tick), so input is stamped `dt` ticks later. `dt` = the game's network delay
/// in ms / 71 (`HostInitGame` / `ClientStartingGame`: `CMsgStacks(players, g_pGameType+660 / 0x47,
/// start)`).
pub fn execution_tick_dt(last: u32, dt: u32) -> u32 {
    let t = last + 2 + dt;
    if t & COMMUNICATION_TICK_VALUE != 0 { t + 1 } else { t }
}

/// The network delay in ticks for a delay in ms (`g_pGameType+660 / 0x47`).
pub fn dt_for_delay(ms: u32) -> u32 {
    ms / TICK_MS
}

/// A command and the tick it executes at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stamped {
    pub tick: u32,
    pub command: PlayerCommand,
}

/// Commands waiting for their tick, in issue order per tick.
#[derive(Default)]
pub struct CommandQueue {
    pending: BTreeMap<u32, Vec<PlayerCommand>>,
}

impl CommandQueue {
    pub fn push(&mut self, s: Stamped) {
        self.pending.entry(s.tick).or_default().push(s.command);
    }

    /// Every waiting command, in tick and issue order (for a savegame).
    pub fn pending(&self) -> Vec<Stamped> {
        self.pending.iter().flat_map(|(&tick, cs)| cs.iter().map(move |c| Stamped { tick, command: c.clone() })).collect()
    }

    /// The commands of `tick`. Commands stamped for a tick that already passed run now
    /// (they arrived late; the lockstep transport stalls instead).
    pub fn take(&mut self, tick: u32) -> Vec<PlayerCommand> {
        let late: Vec<u32> = self.pending.range(..=tick).map(|(&t, _)| t).collect();
        late.into_iter().flat_map(|t| self.pending.remove(&t).unwrap_or_default()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use s4_sim::Command;

    #[test]
    fn single_player_execution_tick() {
        assert_eq!(execution_tick(10), 12);
        assert_eq!(execution_tick(11), 14);
        assert_eq!(execution_tick(0), 2);
    }

    #[test]
    fn network_delay_moves_the_tick() {
        assert_eq!(dt_for_delay(300), 4);
        assert_eq!(execution_tick_dt(10, 4), 16);
        assert_eq!(execution_tick_dt(10, 3), 16);
        assert_eq!(execution_tick_dt(11, 3), 16);
    }

    #[test]
    fn queue_releases_by_tick_in_order() {
        let mut q = CommandQueue::default();
        let c = |b| PlayerCommand { player: 1, command: Command::SwitchBuilding { building: b } };
        q.push(Stamped { tick: 5, command: c(1) });
        q.push(Stamped { tick: 3, command: c(2) });
        q.push(Stamped { tick: 5, command: c(3) });
        assert!(q.take(2).is_empty());
        assert_eq!(q.take(3), vec![c(2)]);
        assert_eq!(q.take(5), vec![c(1), c(3)]);
    }
}
