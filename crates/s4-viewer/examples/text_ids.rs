//! Print strings of the game's text table: `cargo run -p s4-viewer --example text_ids -- LANG ID...`
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let t = s4_formats::text::TextTable::open(s4_formats::game_dir(), a[0].parse()?)?;
    for id in &a[1..] {
        let id: usize = id.parse()?;
        println!("{id}: {:?}", t.get(id));
    }
    Ok(())
}
