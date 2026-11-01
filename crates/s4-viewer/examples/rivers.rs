//! River nodes (ground types 96..=99) per map of a folder, and one river node to aim at:
//! `cargo run -p s4-viewer --example rivers -- Map/Campaign`
fn main() -> anyhow::Result<()> {
    let dir = s4_formats::game_dir().join(std::env::args().nth(1).unwrap_or_else(|| "Map/Campaign".into()));
    let mut names: Vec<_> = std::fs::read_dir(&dir)?.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("map"))).collect();
    names.sort();
    for p in names {
        let m = s4_formats::map::MapFile::open(&p)?;
        let l = m.landscape()?;
        let w = l.width as usize;
        let rivers: Vec<usize> = (0..l.nodes.len()).filter(|&i| (96..=99).contains(&l.nodes[i].ground)).collect();
        if let Some(&i) = rivers.get(rivers.len() / 2) {
            println!("{} {} river nodes, e.g. {},{}", p.file_name().unwrap().to_string_lossy(), rivers.len(), i % w, i / w);
        }
    }
    Ok(())
}
