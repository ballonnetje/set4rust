//! Render every dialog of a GUI set on its own and list its controls:
//! `cargo run -p s4-viewer --example guiset_dump -- GUISetStartscreens.dat OUTDIR`

fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (file, out) = (a[0].as_str(), std::path::PathBuf::from(&a[1]));
    std::fs::create_dir_all(&out)?;
    let probe = s4_viewer::gui::Gui::from_set(file, 2048, &[])?;
    let slots: Vec<usize> = probe.state.set.dialogs.iter().flatten().map(|d| d.slot).collect();
    for slot in slots {
        let mut g = s4_viewer::gui::Gui::from_set(file, 2048, &[])?;
        let mut ev = Vec::new();
        g.state.open(slot, &mut ev);
        let d = g.state.set.dialog(slot).unwrap().clone();
        println!("dialog {slot}: surface {} at {},{} {}x{} bg {} flags {:#x}, {} controls", d.surface, d.x, d.y, d.width, d.height, d.background, d.flags, d.controls.len());
        for c in &d.controls {
            println!(
                "  link {:5} kind {:2} at {:4},{:4} {:3}x{:3} img {:5} pressed {:5} tip {:5}/{:5} font {:2} style {} textslot {} fx {:#x} group {}",
                c.link, c.kind, c.x, c.y, c.width, c.height, c.image, c.pressed_image, c.tooltip, c.tooltip_ext, c.font, c.text_style, c.text_slot, c.effects, c.group
            );
        }
        if let Ok(img) = g.compose() {
            s4_viewer::client::write_png(&out.join(format!("dlg_{slot:02}.png")), img.width, img.height, &img.rgba)?;
        }
    }
    Ok(())
}
