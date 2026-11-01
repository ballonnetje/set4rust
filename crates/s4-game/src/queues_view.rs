//! The monitor's view of one player's queues, read from the world on the sim thread (only while
//! the Queues tab is shown, once a second; read only): `s4_sim::debug`'s transport (the
//! eco-sectors' requests, offers, idle carriers, carrier jobs), build queue, production orders,
//! settler job states and the computer's attack agents.

use s4_sim::World;
use s4_sim::debug;

#[derive(Clone, Debug, Default)]
pub struct Queues {
    pub tick: u32,
    pub player: u8,
    pub transport: Vec<debug::EcoTransport>,
    pub build: Vec<debug::QueuedSite>,
    pub eco_orders: Vec<debug::EcoOrders>,
    pub building_orders: Vec<debug::BuildingOrders>,
    pub jobs: Vec<debug::JobStates>,
    pub attacks: Vec<debug::AiAttack>,
}

pub fn of(w: &World, player: u8) -> Queues {
    let (eco_orders, building_orders) = debug::orders(w, player);
    Queues {
        tick: w.tick,
        player,
        transport: debug::transport(w, player),
        build: debug::build_queue(w, player),
        eco_orders,
        building_orders,
        jobs: debug::job_states(w, player),
        attacks: debug::ai_queues(w, player),
    }
}
