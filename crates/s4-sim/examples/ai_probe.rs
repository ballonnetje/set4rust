//! Diagnostic: runs a campaign map with every player but 1 a computer player and prints what the
//! normal sector AIs hold every N ticks.
use s4_sim::ai::model::SectorKind;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let map = args.get(1).cloned().unwrap_or("MCD2_viking1.map".into());
    let ticks: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(20000);
    let every: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(2000);
    let p = ["Campaign", "Multiplayer", "Singleplayer", "User"]
        .iter()
        .map(|d| s4_formats::game_dir().join("Map").join(d).join(&map))
        .find(|p| p.exists())
        .expect("map file");
    let m = s4_formats::map::MapFile::open(&p).expect("map");
    let mut setup = s4_sim::NewGame::default();
    let all = args.get(4).is_some_and(|a| a == "all");
    for k in if all { 0..8 } else { 1..8 } {
        setup.control[k] = 3;
    }
    let (mut w, _) = s4_sim::new_game(&m, s4_sim::Rules::load_default().unwrap(), &setup).unwrap();
    for t in 1..=ticks {
        s4_sim::tick(&mut w, &[]);
        if t % every == 0 {
            println!("tick {t}");
            for pl in 1..=8u8 {
                let Some(a) = s4_sim::ai::player::get(&w, pl) else { continue };
                if a.dark {
                    continue;
                }
                let soldiers = w.entities.iter().filter(|(_, e)| e.owner == pl && e.obj_type == s4_sim::entity::ObjType::Settler && (2..=5).contains(&(e.flags & 0xf))).count();
                let towers = (46..=48).map(|k| w.building_list(pl as i32, k).len()).sum::<usize>();
                println!(
                    "  p{pl}: soldiers {soldiers} towers {towers} strength own {} foreign {} ev {:?}",
                    w.statistic.players[pl as usize].own,
                    w.statistic.players[pl as usize].foreign,
                    &s4_sim::ai::evaluation::of(&w, pl)[..]
                );
                for (si, s) in a.body.sectors.iter().enumerate() {
                    let SectorKind::Normal { caches, agents, eco, forces, .. } = &s.kind else { continue };
                    let kinds: Vec<(i32, usize, u32)> = s.group.items.iter().map(|i| (i.kind, i.tf.members.len(), i.tf.flags() >> 4 & 0xff)).collect();
                    let slots: Vec<usize> = s.slots.iter().map(|x| x.as_ref().map_or(0, |t| t.1.members.len())).collect();
                    let fc: Vec<usize> = forces.iter().map(|f| f.members.len()).collect();
                    println!(
                        "    s{si} sector {} caches {:?} attack {:?} eco {} slots {:?} forces {:?} group {:?}",
                        s.sector(),
                        caches.iter().map(|c| c.count).collect::<Vec<_>>(),
                        agents[0].extra,
                        eco.len(),
                        slots,
                        fc,
                        kinds
                    );
                }
            }
        }
    }
}
