//! Diagnostic: a campaign map with every player a computer player; prints each Dark Tribe
//! player's manakopter hall (state, stocks, READY), its manakopters and halls / sites every N
//! ticks. `--script MAP..` lists the map scripts' lines naming manakopters or `AddBuilding`.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).is_some_and(|a| a == "--script") {
        for m in &args[2..] {
            let s = dump_script(m).unwrap_or_default();
            for (i, l) in s.lines().enumerate() {
                let u = l.to_uppercase();
                if u.contains("MANAKOPTER") || u.contains("ADDBUILDING") || u.contains("FLYTO") {
                    println!("{m}:{i}: {}", l.trim());
                }
            }
        }
        return;
    }
    let map = args.get(1).cloned().unwrap_or("MCD2_dark4.map".into());
    let ticks: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(20000);
    let every: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(2000);
    let p = ["Campaign", "Multiplayer", "Singleplayer", "User"]
        .iter()
        .map(|d| s4_formats::game_dir().join("Map").join(d).join(&map))
        .find(|p| p.exists())
        .expect("map file");
    let m = s4_formats::map::MapFile::open(&p).expect("map");
    let mut setup = s4_sim::NewGame::default();
    setup.control.iter_mut().take(8).for_each(|c| *c = 3);
    let (mut w, _) = s4_sim::new_game(&m, s4_sim::Rules::load_default().unwrap(), &setup).unwrap();
    for t in 1..=ticks {
        s4_sim::tick(&mut w, &[]);
        if t % every == 0 {
            println!("tick {t}");
            for pl in 1..=8u8 {
                let Some(a) = s4_sim::ai::player::get(&w, pl) else { continue };
                if !a.dark {
                    continue;
                }
                let halls: Vec<(u16, u32)> = w
                    .entities
                    .iter()
                    .filter(|(_, e)| e.owner == pl && e.obj_type == s4_sim::entity::ObjType::Building && e.kind == 80)
                    .map(|(i, e)| (i, e.flags))
                    .collect();
                let hall = s4_sim::buildings::manakopter::hall_of(&w, pl);
                let st = hall.and_then(|h| s4_sim::buildings::manakopter::get(&w, h)).map(|s| (s.state, s.stock));
                println!(
                    "  p{pl}: halls {halls:x?} hall_of {hall:?} state {st:?} flying {} mana {} req {:?}",
                    s4_sim::flying::count(&w, pl),
                    s4_sim::magic::mana(&w, pl),
                    w.rules.magic.dark.manakopter_required
                );
            }
        }
    }
}

fn dump_script(map: &str) -> Option<String> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join(map);
    s4_formats::map::MapFile::open(&p).ok()?.script().ok()?
}
