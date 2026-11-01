//! Print the drawing-related parts of buildingInfo.xml for the military buildings:
//! `cargo run --example viewer_buildinginfo`.
use s4_formats::gamedata::{Building, GameData, Race};

fn main() -> anyhow::Result<()> {
    let gd = GameData::load_default()?;
    for race in [Race::ROMAN, Race::VIKING, Race::MAYA, Race::TROJAN] {
        for b in [Building::GUARDTOWERSMALL, Building::GUARDTOWERBIG, Building::CASTLE] {
            let Some(i) = gd.building(race, b) else { continue };
            println!(
                "{race:?} {}: hotspot {:?} flag {:?} mini_flag {:?} door {:?} patch_settler_slot {}",
                b.name(),
                i.hotspot,
                i.flag,
                i.mini_flag,
                i.door,
                i.patch_settler_slot
            );
            for s in &i.soldiers {
                println!("    soldier slot {s:?}");
            }
            for p in &i.patches {
                println!("    patch {p:?}");
            }
            println!("    anim_lists {:?}", i.anim_lists);
        }
    }
    Ok(())
}
