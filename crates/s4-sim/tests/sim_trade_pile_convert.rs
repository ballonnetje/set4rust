//! `CTradePileRole::ChangeGoodType` (0x5721a0, the good-converting spell's vtbl+0x48 on a trade
//! pile) on roman02's storage area of player 1: the pile leaves the eco-sector's list for its old
//! good (0x551260 → 0x4d44f0) and joins the new good's (0x4d4480) only if the area stores that
//! good (+0x180). Skipped without the game data.

use s4_sim::buildings::RoleKind;
use s4_sim::{Command, PlayerCommand, World};

fn roman02() -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map");
    if !p.exists() {
        return None;
    }
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    let rules = s4_sim::Rules::load_default().ok()?;
    let (w, _) = s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).ok()?;
    Some(w)
}

#[test]
fn storage_pile_changes_its_good_list() {
    let Some(mut w) = roman02() else { return };
    let Some((store, piles)) = w.entities.iter().find_map(|(id, e)| match w.buildings.roles.get(&id).map(|r| &r.kind) {
        Some(RoleKind::Storage(s)) if e.owner == 1 => Some((id, s.piles.clone())),
        _ => None,
    }) else {
        return;
    };
    let (src, dst) = (w.rules.magic.convert_good_src[0] as u8, w.rules.magic.convert_good_dst[0] as u8);
    let (a, b) = (piles[0], piles[1]);
    let eco = s4_sim::buildings::storage::eco_of_building(&w, store);
    // Both piles hold the source good, registered for it.
    for p in [a, b] {
        s4_sim::pile::set_trade_type(&mut w, p, 2);
        s4_sim::pile::set_good_type(&mut w, p, src);
        s4_sim::eco::register_good_storage_pile(&mut w, eco, store, p, src);
    }
    let listed = |w: &World, p: u16, g: u8| w.economy.get(eco).is_some_and(|e| e.good_storage_piles.contains(&(store, p, g)));
    // The area does not store the new good: `a` leaves the source list and joins none.
    assert!(s4_sim::pile::change_good_type(&mut w, a, dst));
    assert_eq!(s4_sim::pile::get(&w, a).unwrap().good, dst);
    assert_eq!((listed(&w, a, src), listed(&w, a, dst)), (false, false));
    // It stores it now: `b` moves from the source list to the new good's.
    s4_sim::tick(&mut w, &[PlayerCommand { player: 1, command: Command::StorageGoodSwitch { building: store, good: dst as u16, value: 1 } }]);
    assert!(s4_sim::pile::change_good_type(&mut w, b, dst));
    assert_eq!((listed(&w, b, src), listed(&w, b, dst)), (false, true));
}
