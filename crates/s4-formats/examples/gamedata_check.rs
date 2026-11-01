//! Parse every GameData XML and print counts, warnings and a few resolved samples.
//! `cargo run -p s4-formats --example gamedata_check --release`

use s4_formats::gamedata::{GameData, Race, Building, Settler, AnimList};

fn main() -> anyhow::Result<()> {
    let gd = GameData::load_default()?;
    println!("buildingInfo.xml   v{} buildings {}", gd.building_xml_version, gd.buildings.len());
    println!("BuildingTrigger    triggers {}", gd.triggers.len());
    println!("jobInfo.xml        lists {} nodes {}", gd.jobs.len(), gd.jobs.iter().map(|j| j.nodes.len()).sum::<usize>());
    println!("jobSoundList.xml   entries {}", gd.job_sounds.len());
    println!("objectInfo.xml     v{} objects {}", gd.object_xml_version, gd.objects.len());
    println!("SettlerValues      {} / Mp {}", gd.settlers.len(), gd.settlers_mp.len());
    println!("VehicleInfo.xml    {}", gd.vehicles.len());
    println!("effectsInfo.xml    jobs {} (not used by the game)", gd.effects.len());
    println!("warnings: {}", gd.warnings.len());
    for w in &gd.warnings {
        println!("  {w}");
    }
    let b = gd.building(Race::ROMAN, Building::SAWMILL).unwrap();
    println!("\nRoman sawmill: cost stone {} boards {} gold {}, kind {:?}, inhabitant {:?}, footprint cells {}, hp {}",
        b.stone, b.boards, b.gold, b.kind, b.inhabitant, b.mask_cells(&b.building_pos_lines).len(), b.game_hitpoints());
    let s = gd.settler(Race::VIKING, Settler::WOODCUTTER).unwrap();
    println!("Viking woodcutter: {:?}, tool {:?}, lists {:?}", s.role, s.tool, s.anim_lists.iter().filter_map(|&v| AnimList::from_u16(v)).map(|a| a.name()).collect::<Vec<_>>());
    let j = gd.job_list(Race::ROMAN, AnimList::WOODCUTTER_WORK).unwrap();
    for n in &j.nodes[..4] {
        println!("  {:?} {} (gfx job {:?}) dir {} dur {}", n.task, n.job_part_name, n.job_part, n.dir, n.duration);
    }
    Ok(())
}
