//! `s4-relay [--bind ADDR] [--max-rooms N] [--max-clients N] [--rate-packets N] [--rate-bytes N]`
//! Serves until killed; one log line per room created / closed, and a status line every
//! 10 minutes.

use std::net::UdpSocket;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use anyhow::{Context, Result, bail};
use s4_relay::{Config, RELAY_PORT, Server};

fn main() -> Result<()> {
    let mut bind = format!("0.0.0.0:{RELAY_PORT}");
    let mut cfg = Config::default();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut val = || args.next().with_context(|| format!("{a}: value missing"));
        match a.as_str() {
            "--bind" => bind = val()?,
            "--max-rooms" => cfg.max_rooms = val()?.parse()?,
            "--max-clients" => cfg.max_clients = val()?.parse()?,
            "--rate-packets" => cfg.rate_packets = val()?.parse()?,
            "--rate-bytes" => cfg.rate_bytes = val()?.parse()?,
            "-h" | "--help" => {
                println!("s4-relay [--bind ADDR (default 0.0.0.0:{RELAY_PORT})] [--max-rooms N] [--max-clients N] [--rate-packets N] [--rate-bytes N]");
                return Ok(());
            }
            _ => bail!("unknown argument {a} (--help)"),
        }
    }
    let sock = UdpSocket::bind(&bind).with_context(|| format!("bind {bind}"))?;
    println!("s4-relay {} on {}, {cfg:?}", env!("CARGO_PKG_VERSION"), sock.local_addr()?);
    Server::new(cfg).run(sock, Arc::new(AtomicBool::new(false)))
}
