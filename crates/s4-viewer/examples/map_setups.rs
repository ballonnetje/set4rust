//! The team setups (chunk 3) of every map in a folder: per setup its index, name, and per slot
//! (control, team). `cargo run -p s4-viewer --example map_setups -- Map/Multiplayer`
fn main() -> anyhow::Result<()> {
    let dir = s4_formats::game_dir().join(std::env::args().nth(1).unwrap_or_else(|| "Map/Multiplayer".into()));
    let mut names: Vec<_> = std::fs::read_dir(&dir)?.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("map"))).collect();
    names.sort();
    for p in names {
        let m = s4_formats::map::MapFile::open(&p)?;
        let players = m.general().map(|g| g.player_count).unwrap_or(0);
        let setups = m.team_setups().unwrap_or_default();
        let s: Vec<String> = setups.iter().map(|t| format!("#{} {:?} {:?}", t.index, t.name, t.slots.iter().map(|s| (s.control, s.team)).collect::<Vec<_>>())).collect();
        println!("{} players {players}: {}", p.file_name().unwrap().to_string_lossy(), s.join(" | "));
    }
    Ok(())
}
