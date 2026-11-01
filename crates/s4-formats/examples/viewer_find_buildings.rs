//! Maps whose start buildings include the given building types:
//! `cargo run --release -p s4-formats --example viewer_find_buildings -- 6 42`.
use s4_formats::map::MapFile;

fn main() -> anyhow::Result<()> {
    let want: Vec<u8> = std::env::args().skip(1).filter_map(|s| s.parse().ok()).collect();
    let dir = s4_formats::game_dir().join("Map");
    for sub in ["Campaign", "Multi", "User", "Single"] {
        let Ok(rd) = std::fs::read_dir(dir.join(sub)) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let Ok(m) = MapFile::open(&p) else { continue };
            let Ok(bs) = m.buildings() else { continue };
            for b in bs.iter().filter(|b| want.contains(&b.kind)) {
                println!("{sub}/{} kind {} owner {} at {},{}", p.file_stem().unwrap().to_string_lossy(), b.kind, b.owner, b.x, b.y);
            }
        }
    }
    Ok(())
}
