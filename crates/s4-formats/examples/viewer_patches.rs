//! Print every building's patches (buildingInfo.xml): `cargo run --example viewer_patches [race]`.
use s4_formats::gamedata::GameData;

fn main() -> anyhow::Result<()> {
    let gd = GameData::load_default()?;
    let race: Option<u8> = std::env::args().nth(1).and_then(|s| s.parse().ok());
    for b in &gd.buildings {
        if race.is_some_and(|r| r != b.race as u8) || b.patches.is_empty() {
            continue;
        }
        println!("race {} building {} ({}):", b.race as u8, b.building as u8, b.building.name());
        for p in &b.patches {
            println!("    slot {} job {:?} {} {:?} ticks {} sound {:?}", p.slot, p.job, p.job_name, p.kind, p.ticks, p.sound);
        }
    }
    Ok(())
}
