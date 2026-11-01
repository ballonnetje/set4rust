//! The monitor's view of one player's economy, read from the world on the sim thread (only while
//! the monitor's Economy tab is shown, once a second; read only): `s4_sim::debug`'s construction
//! sites, production buildings and carriers, and the goods in store (`CStatistic`).

use s4_sim::World;
use s4_sim::debug;

#[derive(Clone, Debug, Default)]
pub struct Economy {
    pub tick: u32,
    pub player: u8,
    pub sites: Vec<debug::SiteView>,
    pub producers: Vec<debug::ProdView>,
    pub carriers: debug::CarrierView,
    /// `CStatistic` goods of the player (on piles), by good.
    pub goods: Vec<i32>,
}

pub fn of(w: &World, player: u8) -> Economy {
    Economy {
        tick: w.tick,
        player,
        sites: debug::sites(w, player),
        producers: debug::production(w, player),
        carriers: debug::carriers(w, player),
        goods: w.statistic.players.get(player as usize).map(|p| p.goods.to_vec()).unwrap_or_default(),
    }
}
