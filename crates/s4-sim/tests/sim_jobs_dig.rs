//! Digger end to end on a real map: command 5025 places a site that needs levelling, the site
//! calls free diggers (`OrderDigger` → `CEcoSector::CallWorker`), and
//! they flatten its digging window until the site reports `DiggingIsReady`. Skipped when the game
//! data is not present.

use s4_sim::build::possible_sites;
use s4_sim::entity::EntityData;
use s4_sim::{Command, PlayerCommand, World};

const DIGGER: u16 = 2;

fn roman02() -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map");
    if !p.exists() {
        return None;
    }
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    let rules = s4_sim::Rules::load_default().ok()?;
    let (mut w, _) = s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).ok()?;
    let towers: Vec<(i32, i32)> = w
        .entities
        .iter()
        .filter_map(|(id, e)| match &e.data {
            EntityData::Building(_) if s4_sim::buildings::have_inhabitant(&w, id) => Some((e.x(), e.y())),
            _ => None,
        })
        .collect();
    for (x, y) in towers {
        s4_sim::territory::set_owner(&mut w, x, y);
    }
    Some(w)
}

/// Place a woodcutter on the possible site with the most digging and return its id.
fn dig_site(w: &mut World) -> Option<u16> {
    let size = w.grid.size() as i32;
    let sites = possible_sites(w, 1, 1, (0, 0), (size, size));
    let &(x, y, _) = sites.iter().max_by_key(|s| s.2)?;
    let cmd = PlayerCommand { player: 1, command: Command::Build { building: 1, x, y } };
    s4_sim::tick(w, &[cmd]);
    w.sites.roles.iter().find(|(_, s)| s.dig_slot >= 0).map(|(id, _)| *id)
}

/// State hash, ticks until digging was ready, and the nodes left unlevelled.
type Outcome = (u64, u32, Vec<(i32, i32)>);

fn run(diggers: usize) -> Option<Outcome> {
    let mut w = roman02()?;
    let site = dig_site(&mut w)?;
    let (sx, sy) = { let e = w.entities.by_index(site as u32)?; (e.x(), e.y()) };
    let effort = w.sites.roles[&site].dig_effort;
    let slot = (1u8, w.sites.roles[&site].dig_slot as u8);
    let targets: Vec<(i32, i32, u8)> = w.sites.dig_slots[&slot]
        .heights
        .iter()
        .enumerate()
        .filter(|&(_, &t)| t != 0)
        .map(|(k, &t)| ((k / 32) as i32 + sx - 15, (k % 32) as i32 + sy - 15, t))
        .collect();
    eprintln!("site {site} at ({sx},{sy}): effort {effort}, {} nodes to level", targets.len());
    // Free diggers next to the site; the site calls them itself (CBuildingSiteRole::OrderDigger,
    // release 0x5425f0, through CEcoSector::CallWorker).
    let mut ids = Vec::new();
    for k in 0..diggers {
        let (x, y) = (sx + 6 + k as i32, sy + 6);
        ids.push(s4_sim::create::add_settler(&mut w, x, y, 1, DIGGER)?);
    }
    let mut ticks = 0;
    // State 3 is still digging (with materials ordered), once the effort per digger is small.
    while matches!(w.sites.roles[&site].state, 2 | 3) && ticks < 40_000 {
        s4_sim::tick(&mut w, &[]);
        ticks += 1;
    }
    let s = &w.sites.roles[&site];
    eprintln!("after {ticks} ticks: state {}, effort {}, slot {}", s.state, s.dig_effort, s.dig_slot);
    assert_eq!(s.state, 4, "digging did not finish");
    assert_eq!(s.dig_slot, -1);
    assert!(!w.sites.dig_slots.contains_key(&slot));
    let mut left = Vec::new();
    for &(x, y, t) in &targets {
        let n = w.grid.index(x as u32, y as u32);
        let h = w.layers.height[n] as i32;
        if h != t as i32 {
            // CDiggerRole's slope rule: raising is refused while (x−1, y−1) or (x, y−1) is at
            // h − 5 or (x, y+1) or (x+1, y+1) at h − 7; lowering while they are at h + 7 / h + 5.
            // Whatever is left when the last digger gives up must be refused by it.
            let nb = |dx: i32, dy: i32| w.layers.height[w.grid.index((x + dx) as u32, (y + dy) as u32)] as i32;
            let (up, down) = if (t as i32) < h { (h + 7, h + 5) } else { (h - 5, h - 7) };
            let refused = nb(-1, -1) == up || nb(0, -1) == up || nb(0, 1) == down || nb(1, 1) == down;
            assert!(refused, "({x},{y}) at {h}, target {t}, could still be dug");
            left.push((x, y));
        }
        assert_eq!(w.layers.flags[n] & 0x20, 0, "picked flag left at ({x},{y})");
    }
    assert!(left.len() < targets.len(), "nothing was levelled");
    for &d in &ids {
        let e = w.entities.by_index(d as u32)?;
        let EntityData::Settler(st) = &e.data else { return None };
        assert_eq!(st.job.home, 0);
        assert_eq!(st.job.list_id, DIGGER as i16, "digger {d} not back on its wait list");
    }
    Some((w.state_hash(), ticks, left))
}

#[test]
fn diggers_level_a_site() {
    let Some((h1, ticks, left)) = run(3) else { return };
    // Nodes the slope rule never let anyone change stay as they are (SearchDig skips them).
    eprintln!("{} nodes not at target (slope rule)", left.len());
    let (h2, ticks2, _) = run(3).unwrap();
    assert_eq!((h1, ticks), (h2, ticks2), "two runs differ");
}
