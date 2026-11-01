//! Print the dialog rectangles of a GUISet: `cargo run --example viewer_guilayout -- GUISetRomans.dat`.
fn main() -> anyhow::Result<()> {
    let name = std::env::args().nth(1).unwrap_or("GUISetRomans.dat".into());
    let set = s4_formats::gui::GuiSet::open(s4_formats::game_dir().join("Menu").join(&name))?;
    let detail: Vec<usize> = std::env::args().skip(2).filter_map(|a| a.parse().ok()).collect();
    for d in set.dialogs.iter().flatten() {
        if detail.contains(&d.slot) {
            for c in &d.controls {
                println!(
                    "    ctl kind {:2} {:12} at {:3},{:3} {:3}x{:3} img {:4} pressed {:4} link {:4} tip {:5} font {:2} style {} fx {:#x} group {}",
                    c.kind, c.kind_name(), c.x, c.y, c.width, c.height, c.image, c.pressed_image, c.link, c.tooltip, c.font, c.text_style, c.effects, c.group
                );
            }
        }
        println!(
            "slot {:2} surface {:2} at {:3},{:3} size {:3}x{:3} bg {:4} flags {} controls {}",
            d.slot, d.surface, d.x, d.y, d.width, d.height, d.background, d.flags, d.controls.len()
        );
    }
    Ok(())
}
