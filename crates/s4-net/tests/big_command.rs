//! A command with a large selection (the user's 1000-id cap, 2026-10-23) through two sessions on
//! localhost: the client's `SendTo` with 500 ids and a `DefineGroup` with 1000 reach both peers as
//! one command each with every id in order. The 1000-id one is past `MAX_DATAGRAM` (its turn is
//! sent alone; IP fragments it). No game install needed.

use std::sync::mpsc;
use std::time::Duration;

use s4_net::{GameSettings, Session};
use s4_sim::{Command, PlayerCommand};

/// Run `ticks` lockstep ticks without a world; the commands each tick brought.
fn run(s: &mut Session, ticks: u32, issue: Vec<(u32, Command)>) -> Vec<PlayerCommand> {
    let mut got = Vec::new();
    for tick in 0..ticks {
        for (_, c) in issue.iter().filter(|i| i.0 == tick) {
            s.lock.issue(c.clone());
        }
        s.produce(tick);
        let (cmds, _) = s.wait_ready(tick + 1, Duration::from_secs(20));
        got.extend(cmds.unwrap_or_else(|| panic!("tick {}: no turn", tick + 1)));
    }
    s.linger(Duration::from_millis(200));
    got
}

#[test]
fn large_selections_go_as_one_command() {
    let ids: Vec<u16> = (1..=500).map(|i| i * 3).collect();
    let cmd = Command::SendTo { x: 120, y: 340, mode: 4, entities: ids.clone() };
    let group = Command::DefineGroup { group: 2, entities: (1..=1000).collect() };
    const { assert!(7 + 15 + 2 * 1000 > s4_net::wire::MAX_DATAGRAM, "the group's turn needs more than a datagram") };
    let settings = GameSettings { map: "x".into(), delay_ms: 142, hash_interval: 64, map_hash: 77, ..Default::default() };
    let (tx, rx) = mpsc::channel();
    let host = std::thread::spawn(move || {
        let mut s = Session::host("127.0.0.1:0", "host", 2, settings, Duration::from_secs(20), |a| tx.send(a).unwrap()).unwrap();
        run(&mut s, 40, Vec::new())
    });
    let addr = rx.recv_timeout(Duration::from_secs(20)).unwrap();
    let mut c = Session::join("127.0.0.1:0", addr, "client", 77, Duration::from_secs(20)).unwrap();
    let mine = run(&mut c, 40, vec![(3, cmd.clone()), (9, group.clone())]);
    let theirs = host.join().unwrap();
    for (who, got) in [("client", &mine), ("host", &theirs)] {
        let sends: Vec<&PlayerCommand> = got.iter().filter(|c| matches!(c.command, Command::SendTo { .. })).collect();
        assert_eq!(sends.len(), 1, "{who}: one SendTo");
        assert_eq!(sends[0].command, cmd, "{who}: all 500 ids, in order");
        assert_eq!(sends[0].player, c.local);
        let groups: Vec<&PlayerCommand> = got.iter().filter(|c| matches!(c.command, Command::DefineGroup { .. })).collect();
        assert_eq!(groups.len(), 1, "{who}: one DefineGroup");
        assert_eq!(groups[0].command, group, "{who}: all 1000 ids");
    }
}
