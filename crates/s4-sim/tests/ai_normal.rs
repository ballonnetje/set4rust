//! The normal (non Dark Tribe) computer players' agents (stage S2, `ai::normal_agents`) on
//! campaign maps with every player but 1 a computer player (control 3). No AI save exists to
//! compare with; these check that the agents act as their port says and stay deterministic.
//! Skipped without the game data.

use s4_sim::World;
use s4_sim::entity::ObjType;

fn game(map: &str) -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join(map);
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    let mut setup = s4_sim::NewGame::default();
    for k in 1..8 {
        setup.control[k] = 3;
    }
    s4_sim::new_game(&map, s4_sim::Rules::load_default().ok()?, &setup).ok().map(|(w, _)| w)
}

fn soldiers(w: &World, p: u8) -> usize {
    w.entities.iter().filter(|(_, e)| e.obj_type == ObjType::Settler && e.owner == p && e.flags & 0xf != 0 && !e.is_dead()).count()
}

/// `CAIAgentRecruitment` (0x432cd0): every 250 ticks the soldier wishes of each barracks'
/// eco-sector are set (5047), and the barracks turn carriers into soldiers.
#[test]
fn ai_players_recruit_at_their_barracks() {
    let Some(mut w) = game("MCD2_viking1.map") else { return };
    let (mut b, _) = (game("MCD2_viking1.map").unwrap(), ());
    let ai: Vec<u8> = (2..=8).filter(|&p| w.ai.is_ai(p) && !w.building_list(p as i32, 24).is_empty()).collect();
    assert!(!ai.is_empty(), "an AI player with barracks");
    let before: Vec<usize> = ai.iter().map(|&p| soldiers(&w, p)).collect();
    let mut wishes = false;
    for t in 1..=6000u32 {
        s4_sim::tick(&mut w, &[]);
        s4_sim::tick(&mut b, &[]);
        for &p in &ai {
            for bk in w.building_list(p as i32, 24) {
                let eco = s4_sim::buildings::storage::eco_of_building(&w, bk);
                if w.economy.get(eco).is_some_and(|e| e.soldier_orders[0] == 12 && e.soldier_orders[3] == 12) {
                    wishes = true;
                }
            }
        }
        if t % 1000 == 0 {
            assert_eq!(w.state_hash(), b.state_hash(), "tick {t}");
        }
    }
    assert!(wishes, "swordsmen 1 and bowmen 1 wished 12 each");
    let after: Vec<usize> = ai.iter().map(|&p| soldiers(&w, p)).collect();
    assert!(ai.iter().zip(before.iter().zip(&after)).any(|(_, (a, b))| b > a), "soldiers were recruited: {before:?} → {after:?}");
}
