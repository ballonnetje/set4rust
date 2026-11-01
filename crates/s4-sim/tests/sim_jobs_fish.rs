//! Fishers and waterworkers (`CFreeWorkerRole` with `SearchFish` 0x54e920 / `SearchWater`
//! 0x54ea70) on MCD2_roman1, driven by `tick()` only: the map's fisher huts and waterworks order
//! their workers, who fish the water around them (a catch lowers the node's fish by one) and draw
//! water from rivers. Skipped when the game data is not present.

use s4_sim::World;
use s4_sim::entity::EntityData;

const FISH: u8 = 11;
const WATER: u8 = 36;

fn map() -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("MCD2_roman1.map");
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

/// Fish in the water (resource type 0, amounts 1..=15) and goods of `good` on piles or carried.
fn fish_left(w: &World) -> u32 {
    w.layers.resource.iter().filter(|&&r| (1..=15).contains(&r)).map(|&r| r as u32).sum()
}

fn goods(w: &World, good: u8) -> u32 {
    w.economy.sectors.iter().flatten().map(|e| e.goods_on_piles[good as usize] as u32 + e.goods_on_carriers[good as usize] as u32).sum()
}

fn run(ticks: u32) -> Option<(u64, u32, u32, u32, u32)> {
    let mut w = map()?;
    let (fish0, water0, food0) = (fish_left(&w), goods(&w, WATER), goods(&w, FISH));
    for _ in 0..ticks {
        s4_sim::tick(&mut w, &[]);
    }
    let caught = fish0 - fish_left(&w);
    Some((w.state_hash(), caught, water0, goods(&w, WATER), food0))
}

#[test]
fn fishers_fish_and_waterworkers_draw_water() {
    let Some(a) = run(3000) else { return };
    eprintln!("hash {:x}, fish caught {}, water {} -> {}, fish goods at start {}", a.0, a.1, a.2, a.3, a.4);
    assert!(a.1 > 0, "no fish caught");
    assert!(a.3 > a.2, "no water drawn");
    assert_eq!(Some(a), run(3000), "not deterministic");
}

/// While fishing (job part 0x52) a fisher faces water (0x578680: the direction of the first fish
/// node among spiral entries 0..=18, else of the last water node there).
#[test]
fn fishers_face_the_water() {
    let Some(mut w) = map() else { return };
    let (mut seen, mut wrong) = (0, 0);
    for _ in 0..3000 {
        s4_sim::tick(&mut w, &[]);
        for (_, e) in w.entities.iter() {
            let EntityData::Settler(s) = &e.data else { continue };
            if e.kind != 9 || s.job.job_part != 0x52 {
                continue;
            }
            seen += 1;
            let water = (1..=18).any(|k| {
                let (dx, dy, _, d) = s4_sim::spiral::entry(k);
                let n = w.grid.checked_index(e.x() + dx as i32, e.y() + dy as i32);
                d == s.direction && n.is_some_and(|n| w.layers.ground[n] < 9 || w.layers.ground[n] & 0xf0 == 0x60)
            });
            if !water {
                wrong += 1;
            }
        }
    }
    eprintln!("fishing observed {seen} times, not facing water {wrong}");
    assert!(seen > 0, "nobody fished");
    assert_eq!(wrong, 0);
}
