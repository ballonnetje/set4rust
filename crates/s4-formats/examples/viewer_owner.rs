//! Owner-byte statistics of a map (chunk 6 byte 1): `cargo run --example viewer_owner -- <map>`.
fn main() -> anyhow::Result<()> {
    let p = std::env::args().nth(1).expect("map path");
    let map = s4_formats::map::MapFile::open(&p)?;
    let l = map.landscape()?;
    let mut hist = std::collections::BTreeMap::new();
    for n in &l.nodes {
        *hist.entry((n.owner, n.is_water())).or_insert(0usize) += 1;
    }
    println!("(owner, water): count = {hist:?}");
    Ok(())
}
