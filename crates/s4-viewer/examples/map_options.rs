//! Per map of a folder: chunk 1's flags and option bits, the player count and the preplaced
//! settlers / buildings: `cargo run -p s4-viewer --example map_options -- Map/Multiplayer`
fn main() -> anyhow::Result<()> {
    let dir = s4_formats::game_dir().join(std::env::args().nth(1).unwrap_or_else(|| "Map/Multiplayer".into()));
    let mut names: Vec<_> = std::fs::read_dir(&dir)?.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("map"))).collect();
    names.sort();
    for p in names {
        let m = s4_formats::map::MapFile::open(&p)?;
        let g = m.general()?;
        let settlers = m.settlers().map_or(0, |s| s.len());
        let buildings = m.buildings().map_or(0, |b| b.len());
        println!(
            "{:28} flags {:#06x} options {:#06x} players {} settlers {:5} buildings {:4}",
            p.file_stem().unwrap().to_string_lossy(),
            g.flags,
            g.options,
            g.player_count,
            settlers,
            buildings
        );
    }
    Ok(())
}
