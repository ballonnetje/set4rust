//! D17's update 0x49c330 by the kind of game (`GlobalGameState +0x2a8`, 0x49c6ff): in a local
//! free game (1) or a campaign (3) the foreign-trade button 1057 is hidden and the enable step
//! is skipped; otherwise 1060 / 1057 are enabled, for a port (info +8 = 0x20) only with +0xa.
//! Skipped without the game data.

use s4_viewer::building_dialog::{DIALOGS, Infos, update};
use s4_viewer::guistate::GuiState;

#[test]
fn foreign_trade_button_by_game_kind() {
    let Ok(set) = s4_formats::gui::GuiSet::open(s4_formats::game_dir().join("Menu/GUISetRomans.dat")) else { return };
    let d = DIALOGS[5];
    // (visible, enabled) of `link` after one update.
    let run = |kind: u32, port: bool, a: u8| {
        let mut infos = Infos::default();
        infos.obj[5][8] = if port { 0x20 } else { 0x21 };
        infos.obj[5][0xa] = a;
        let mut st = GuiState::new(set.clone());
        st.game_kind = kind;
        update(5, &infos, &mut st, &|id| format!("#{id}"));
        let fx = |link: u16| st.index_of(d, link).and_then(|i| st.effects.get(&(d, i)).copied()).unwrap_or(0);
        [1057u16, 1060].map(|l| (fx(l) & 8 == 0, fx(l) & 4 == 0))
    };
    for kind in [0, 2, 4] {
        assert_eq!(run(kind, false, 0), [(true, true), (true, true)], "kind {kind}: marketplace");
        assert_eq!(run(kind, true, 0), [(true, false), (true, false)], "kind {kind}: port without +0xa");
        assert_eq!(run(kind, true, 1), [(true, true), (true, true)], "kind {kind}: port with +0xa");
    }
    for kind in [1, 3] {
        // 1057 hidden; nothing enabled or disabled (the set's own state stays).
        let fresh = run(0, false, 0)[1];
        assert_eq!(run(kind, true, 0), [(false, true), fresh], "kind {kind}");
    }
}
