//! The planters on MCD2_maya4 (a vineyard, agave farmers, beekeepers and a sunflower farmer),
//! driven by `tick()` only: the farm role's two searches (release 0x54e390 / 0x54e410 vine,
//! 0x54e5b0 / 0x54e640 agave, 0x54e8e0 / 0x54e880 beehive, 0x54e6c0 / 0x54e750 sunflower) find
//! places to plant and ripe plants (or READY hives, `CHive` 0x5656b0) to harvest, and the goods
//! reach the piles. Skipped when the game data is not present.

use s4_sim::World;
use s4_sim::entity::EntityData;

const GOODS: [usize; 4] = [37, 1, 19, 42]; // wine, agave, honey, sunflower
const OBJECTS: [u16; 4] = [162, 211, 217, 255]; // vine, agave, hive, sunflower

fn map() -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("MCD2_maya4.map");
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

fn goods(w: &World) -> [u32; 4] {
    GOODS.map(|g| w.economy.sectors.iter().flatten().map(|e| e.goods_on_piles[g] as u32 + e.goods_on_carriers[g] as u32).sum())
}

fn objects(w: &World) -> [usize; 4] {
    OBJECTS.map(|k| w.entities.iter().filter(|(_, e)| e.kind == k && matches!(e.data, EntityData::Deco(_))).count())
}

fn run(ticks: u32) -> Option<([u32; 4], [usize; 4], u64)> {
    let mut w = map()?;
    assert_eq!((goods(&w), objects(&w)), ([0; 4], [0; 4]), "nothing planted or made at the start");
    for _ in 0..ticks {
        s4_sim::tick(&mut w, &[]);
    }
    Some((goods(&w), objects(&w), w.state_hash()))
}

#[test]
fn planters_plant_and_harvest() {
    let Some(a) = run(9000) else { return };
    eprintln!("wine/agave/honey/sunflower {:?}, vine/agave/hive/sunflower objects {:?}", a.0, a.1);
    assert!(a.1.iter().all(|&n| n > 0), "every planter planted");
    assert!(a.0.iter().all(|&n| n > 0), "every planter harvested");
    assert_eq!(Some(a), run(9000), "not deterministic");
}
