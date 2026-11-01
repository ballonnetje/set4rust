//! Multiplayer foundations: deterministic lockstep over UDP.
//!
//! * [`commands`]: commands stamped with their execution tick (the command log type, shared with
//!   s4-game) and the original's tick arithmetic (`COMMUNICATION_TICK_VALUE`, the network delay
//!   `dt`);
//! * [`lockstep`]: the valid-tick model of `CGameHost::StartNewCycle` / `CMsgStacks`, transport
//!   free (turns per communication tick, stalls, hash comparison);
//! * [`wire`]: our datagram format;
//! * [`lobby`]: the LAN lobby (discovery, slot table, chat, start handshake), non-blocking;
//! * [`session`]: the game's per-tick exchange (and blocking host / join for headless games);
//! * [`relay`]: [ours] the transport: a UDP socket, or a link through the internet relay
//!   (s4-relay);
//! * [`step`]: one networked tick of an `s4_sim::World`.

pub mod commands;
pub mod lobby;
pub mod lockstep;
pub mod relay;
pub mod session;
pub mod wire;

use std::time::Duration;

use s4_sim::World;

pub use commands::{COMMUNICATION_TICK_VALUE, CommandQueue, Stamped, execution_tick, execution_tick_dt};
pub use lockstep::{Desync, Lockstep};
pub use session::{NetEvent, Session};
pub use lobby::{ClientLobby, Discovery, HOST_LOST, HostLobby, LobbyEvent, RELAY_TIMEOUT};
pub use relay::{Link, RELAY_PORT, RelayError, RelayLink};
pub use wire::{GameInfo, GameSettings, SlotInfo};

/// The per-subsystem hashes a desync dump compares (`World::subsystem_hashes`, in state-hash
/// order).
pub fn world_parts(w: &World) -> Vec<(String, u64)> {
    w.subsystem_hashes().into_iter().map(|(n, h)| (n.to_string(), h)).collect()
}

/// The state hash and [`world_parts`] in one pass over the world (`World::hashes`): what a hash
/// exchange sends, for about the cost of the state hash alone.
pub fn world_hashes(w: &World) -> (u64, Vec<(String, u64)>) {
    let (hash, parts) = w.hashes();
    (hash, parts.into_iter().map(|(n, h)| (n.to_string(), h)).collect())
}

/// FNV-1a over a map file's bytes: peers must run the same file.
pub fn map_hash(path: &std::path::Path) -> anyhow::Result<u64> {
    let b = std::fs::read(path).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
    Ok(b.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &x| (h ^ x as u64).wrapping_mul(0x0100_0000_01b3)))
}

/// One networked tick: send our turns up to the virtual tick, wait for every player's turn of
/// the next tick (stalling like the original), run it, exchange the hash when due. Returns the
/// network events, or an error when the peers stop answering for `timeout`.
pub fn step(session: &mut Session, world: &mut World, timeout: Duration) -> anyhow::Result<Vec<NetEvent>> {
    session.produce(world.tick);
    let next = world.tick + 1;
    let (cmds, mut events) = session.wait_ready(next, timeout);
    let Some(cmds) = cmds else {
        anyhow::bail!("tick {next}: no turn from player(s) {:?} for {timeout:?}", session.lock.missing(next));
    };
    s4_sim::tick(world, &cmds);
    if session.lock.hash_due(world.tick) {
        let (hash, parts) = world_hashes(world);
        session.send_hash(world.tick, hash, parts);
        events.extend(session.pump());
    }
    Ok(events)
}
