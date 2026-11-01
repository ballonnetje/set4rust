//! The internet relay for s4rs network games [ours] (not a port: the original used Ubisoft's
//! session layer). A host registers its game in a room; clients list the rooms and join one;
//! every datagram between the peers goes through the relay, so any NAT works. Protocol in
//! [`proto`], server in [`server`]; the client side is s4-net's `relay` module. docs/NETWORK.md
//! has the protocol, the build for the server and how to run it.

pub mod proto;
pub mod server;

pub use proto::{MAX_PAYLOAD, RELAY_PORT, RoomInfo, ToClient, ToServer};
pub use server::{Chaos, Config, Server, spawn};
