//! Print the ground types of a window of a map: `ground_window MAP x0 y0 w h`.
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let m = s4_formats::map::MapFile::open(s4_viewer::scene::resolve_map(&a[0]))?;
    let l = m.landscape()?;
    let w = l.width as usize;
    let n = |i: usize| -> usize { a[i].parse().unwrap() };
    let (x0, y0, ww, hh) = (n(1), n(2), n(3), n(4));
    for y in y0..y0 + hh {
        let row: Vec<String> = (x0..x0 + ww).map(|x| format!("{:3}", l.nodes[y * w + x].ground)).collect();
        println!("{y:4}: {}{}", " ".repeat(((y - y0) * 2) % 4), row.join(" "));
    }
    Ok(())
}
