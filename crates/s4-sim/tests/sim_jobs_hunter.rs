//! Hunters (`CHunterRole`, release vtable 0x105bb18) on MCD2_roman1, driven by `tick()` only:
//! the map's hunter huts order their hunters, find animals (`CScanner::FindNearestAnimal`
//! 0x5b08b0), and the hunters shoot them, harvest the meat and carry it home. Skipped when the
//! game data is not present.

use s4_sim::World;
use s4_sim::entity::{EntityData, ObjType};

const HUNTER: u16 = 11;
/// Task types: WORK_ON_ENTITY (the shot), RESOURCE_GATHERING (the harvest), PUT_GOOD.
const SHOOT: u8 = 28;
const HARVEST: u8 = 13;
const PUT_GOOD: u8 = 21;

fn map() -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("MCD2_roman1.map");
    if !p.exists() {
        return None;
    }
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    let rules = s4_sim::Rules::load_default().ok()?;
    s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).ok().map(|(w, _)| w)
}

fn hunters(w: &World) -> Vec<(u16, u8, u16)> {
    w.entities
        .iter()
        .filter_map(|(id, e)| match &e.data {
            EntityData::Settler(s) if e.kind == HUNTER => Some((id, s.job.task, s.job.entity)),
            _ => None,
        })
        .collect()
}

#[derive(Debug, Default, PartialEq, Eq)]
struct Count {
    shots: u32,
    harvests: u32,
    deliveries: u32,
}

fn run(ticks: u32) -> Option<(u64, Count)> {
    let mut w = map()?;
    let mut c = Count::default();
    let mut last = hunters(&w);
    for _ in 0..ticks {
        s4_sim::tick(&mut w, &[]);
        let now = hunters(&w);
        for &(id, task, entity) in &now {
            let before = last.iter().find(|h| h.0 == id).map_or(0, |h| h.1);
            if task == before {
                continue;
            }
            match task {
                SHOOT => {
                    // The target is a huntable animal within shooting distance.
                    // (A target another hunter took away already is no animal any more.)
                    let Some(a) = w.entities.by_index(entity as u32) else { continue };
                    assert_eq!(a.obj_type, ObjType::Animal);
                    assert!(w.animals.types[a.kind as usize].huntable, "a huntable type");
                    let h = w.entities.by_index(id as u32).unwrap();
                    // Within 12 when the hunter decided; the animal may step once later that tick.
                    assert!(s4_sim::spiral::grid_distance(a.x() - h.x(), a.y() - h.y()) <= 12, "shot from within 12");
                    c.shots += 1;
                }
                HARVEST => c.harvests += 1,
                PUT_GOOD => c.deliveries += 1,
                _ => {}
            }
        }
        last = now;
    }
    Some((w.state_hash(), c))
}

#[test]
fn hunters_shoot_harvest_and_deliver_meat() {
    let Some(a) = run(3000) else { return };
    eprintln!("hash {:x}, {:?}", a.0, a.1);
    // The count depends on the RNG stream (one extra draw at the start gives 3..8 shots in 3000
    // ticks; the workshop role's worker orders since vehicles V2 give 3), so only "some".
    assert!(a.1.shots >= 3, "too few shots");
    assert!(a.1.harvests >= 3 && a.1.deliveries >= 3, "the kills are not brought home");
    assert_eq!(Some(a), run(3000), "not deterministic");
}
