//! `SetMinimapButtonState` (0x49b630, GUI event 0x26f): the four minimap toggles of dialog 0 take
//! the flag bits (0x266 bit 0, 0x268 bit 1, 0x267 bit 2, 0x269 bit 3) without a command.

#[test]
fn minimap_buttons_follow_the_flags() {
    let Ok(mut g) = s4_viewer::gui::Gui::from_set("GUISetRomans.dat", 2048, &[]) else { return };
    let mut ev = Vec::new();
    g.state.open(0, &mut ev);
    for flags in [0b1010u32, 0b0101, 0, 0b1111] {
        g.state.calls.clear();
        assert_eq!(g.set_minimap_buttons(flags), flags);
        for (link, bit) in [(0x266, 0), (0x268, 1), (0x267, 2), (0x269, 3)] {
            assert_eq!(g.state.is_pressed_link(0, link), flags >> bit & 1 != 0, "flags {flags:#b} link {link:#x}");
        }
        assert!(!g.state.calls.iter().any(|c| matches!(c, s4_viewer::guistate::UiEvent::Command { .. })));
    }
}
