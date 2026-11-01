//! The goods counts of the extension bar (dialog 1, fill 0x4f8b30: 622 boards = `CStatistic`
//! +0xa04 + 4·7, 623 stone = good 32) and of the goods page (dialog 27, fill 0x4df820) are the
//! player's `CStatistic` goods, which `CPile::AdjustStatistic` (0x56fe80) keeps equal to the goods
//! on the piles in the player's eco-sectors: construction-site, production, storage and free
//! piles alike, not carriers. A construction site uses its goods in `AddWork` (0x542390) through
//! an inlined `DecreaseUnforeseen(1)` that calls 0x56fe80 (0x542412), so the used boards and
//! stones leave the counts. Playtest: the bar showed more boards and stone than lay on the land,
//! because the port's site took them off its piles without the statistic.
//! Runs on roman02 when the game data is present.

use s4_sim::entity::EntityData;
use s4_sim::{Command, PlayerCommand, World};

const WOODCUTTER: u8 = 1;
const BOARDS: usize = 7;
const STONE: usize = 32;

fn roman02() -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map");
    if !p.exists() {
        return None;
    }
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    let rules = s4_sim::Rules::load_default().ok()?;
    let (w, _) = s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).ok()?;
    Some(w)
}

/// By hand: the goods on the piles whose node lies in an eco-sector of `player`.
fn on_piles(w: &World, player: u8) -> [i32; 43] {
    let mut n = [0; 43];
    for (_, e) in w.entities.iter() {
        if let EntityData::Pile(p) = &e.data {
            let eco = s4_sim::eco::eco_of(w, w.grid.index(e.x() as u32, e.y() as u32));
            if eco != 0 && w.economy.owner(eco) == player {
                n[p.good as usize] += p.amount as i32;
            }
        }
    }
    n
}

#[test]
fn bar_counts_the_goods_on_the_players_piles() {
    let Some(mut w) = roman02() else { return };
    let castle = w.entities.iter().find(|(id, e)| e.owner == 1 && matches!(&e.data, EntityData::Building(_)) && s4_sim::buildings::have_inhabitant(&w, *id)).map(|(_, e)| (e.x(), e.y())).unwrap();
    let soldiers: Vec<(i32, i32)> = w.entities.iter().filter(|(_, e)| matches!(e.data, EntityData::Settler(_)) && e.kind >= 29).map(|(_, e)| (e.x(), e.y())).collect();
    let size = w.grid.size() as i32;
    let &(x, y, _) = s4_sim::build::possible_sites(&w, 1, WOODCUTTER, (0, 0), (size, size))
        .iter()
        .filter(|s| soldiers.iter().all(|&(a, b)| (a - s.0 as i32).abs() + (b - s.1 as i32).abs() > 8))
        .min_by_key(|s| (s.0 as i32 - castle.0).abs() + (s.1 as i32 - castle.1).abs())
        .unwrap();
    s4_sim::tick(&mut w, &[PlayerCommand { player: 1, command: Command::Build { building: WOODCUTTER as u32, x, y } }]);
    let site = *w.sites.roles.keys().max().unwrap();
    let eco = match &w.entities.by_index(site as u32).unwrap().data {
        EntityData::Building(d) => s4_sim::eco::eco_of(&w, w.grid.index(d.ensign.x() as u32, d.ensign.y() as u32)),
        _ => unreachable!(),
    };
    let recruit: Vec<PlayerCommand> =
        [2u16, 3].iter().map(|&kind| PlayerCommand { player: 1, command: Command::SetUnitReservation { eco_sector: eco, kind, value: 4 } }).collect();
    let (cost_boards, cost_stone) = { let s = &w.sites.roles[&site]; (s.boards as i32, s.stones as i32) };
    let start = w.statistic.players[1].goods;
    s4_sim::tick(&mut w, &recruit);
    let mut last_used = 0;
    while w.sites.roles.contains_key(&site) {
        assert!(w.tick < 20_000, "site not finished");
        s4_sim::tick(&mut w, &[]);
        let s = w.sites.roles.get(&site).map_or(cost_boards + cost_stone, |s| (s.boards_used + s.stones_used) as i32);
        if s != last_used || w.tick % 500 == 0 {
            last_used = s;
            for p in 1..=8u8 {
                assert_eq!(w.statistic.players[p as usize].goods, on_piles(&w, p), "player {p} at tick {}", w.tick);
            }
        }
    }
    // What the bar and the goods page read (`Population` → `Economy::players`).
    let pop = s4_viewer::population::from_world(&w);
    let shown = pop.economy.players[1].goods;
    let piles = on_piles(&w, 1);
    assert_eq!((shown[BOARDS], shown[STONE]), (piles[BOARDS], piles[STONE]));
    eprintln!("boards {} -> {}, stone {} -> {} (hut: {cost_boards} / {cost_stone})", start[BOARDS], shown[BOARDS], start[STONE], shown[STONE]);
}
