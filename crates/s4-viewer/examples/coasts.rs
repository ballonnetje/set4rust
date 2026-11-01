//! Nodes where water (ground type <= 8) meets land, sampled over a map, to aim captures at:
//! `cargo run -p s4-viewer --example coasts -- Campaign/roman02 [N]`
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let path = s4_viewer::scene::resolve_map(&a[0]);
    let n: usize = a.get(1).map_or(Ok(12), |s| s.parse())?;
    let m = s4_formats::map::MapFile::open(&path)?;
    let land = m.landscape()?;
    let size = land.width as usize;
    let t = |x: usize, y: usize| land.nodes[y * size + x].ground;
    let mut found = Vec::new();
    for y in (8..size - 8).step_by(4) {
        for x in (8..size - 8).step_by(4) {
            let w = t(x, y) <= 8;
            if w != (t(x + 1, y) <= 8) || w != (t(x, y + 1) <= 8) {
                found.push((x, y, t(x, y), t(x + 1, y)));
            }
        }
    }
    let step = (found.len() / n).max(1);
    for f in found.iter().step_by(step).take(n) {
        println!("{},{} types {} {}", f.0, f.1, f.2, f.3);
    }
    println!("{} coast samples", found.len());
    Ok(())
}
