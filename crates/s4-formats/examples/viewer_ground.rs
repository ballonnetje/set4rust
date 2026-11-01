//! Print a block of ground types (chunk 13 byte 1) and byte 2 of a map, one row per line:
//! `cargo run --example viewer_ground -- <map path> x0 y0 w h`.

fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let map = s4_formats::map::MapFile::open(&a[0])?;
    let l = map.landscape()?;
    let n = |i: usize| a[i].parse::<usize>().unwrap();
    let (x0, y0, w, h) = (n(1), n(2), n(3), n(4));
    let mut hist = std::collections::BTreeMap::new();
    for y in y0..y0 + h {
        let mut line = format!("{y:4}: ");
        for x in x0..x0 + w {
            let nd = &l.nodes[y * l.width as usize + x];
            *hist.entry(nd.ground).or_insert(0) += 1;
            line.push_str(&format!("{:3}{}", nd.ground, if nd.gradient & 0x20 != 0 { '*' } else { ' ' }));
        }
        println!("{line}");
    }
    println!("histogram (ground: count): {hist:?}   (* = byte 2 bit 0x20)");
    Ok(())
}
