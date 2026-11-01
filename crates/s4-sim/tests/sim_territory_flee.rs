//! Land that changes owner under an idle carrier. `SetOwner` (0x60cd30) only destroys buildings
//! (0x53ce10) and changes the tiling's owner; `NotifyChangeEcoSector` (0x4de900) takes a
//! non-warrior on a lost node out of the offer and the counts and sets entity flag 0x80. At its
//! next logic update `CSettler::LogicUpdate` (0x5814f0) clears the flag and calls the role's
//! `ESChanged` (vtbl+0x3c, base 0x587450): `SetFree(-1)` on foreign land gives the settler a
//! `CFleeRole` (0x58727f), which walks it home, where it becomes a carrier again.
//! MCD2_roman1, skipped without the data.

use s4_sim::World;
use s4_sim::entity::{ObjType, flag};
use s4_sim::jobs::role;

fn map() -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("MCD2_roman1.map");
    if !p.exists() {
        return None;
    }
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    let rules = s4_sim::Rules::load_default().ok()?;
    s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).ok().map(|(w, _)| w)
}

fn job(w: &World, i: u16) -> &s4_sim::jobs::JobState {
    match &w.entities.by_index(i as u32).unwrap().data {
        s4_sim::entity::EntityData::Settler(s) => &s.job,
        _ => panic!("not a settler"),
    }
}

#[test]
fn an_idle_carrier_on_lost_land_flees_home() {
    let Some(mut w) = map() else { return };
    for _ in 0..200 {
        s4_sim::tick(&mut w, &[]);
    }
    // An idle offered carrier of player 1.
    let c = w
        .entities
        .iter()
        .find(|(i, e)| e.obj_type == ObjType::Settler && e.kind == 1 && e.owner == 1 && e.flags & flag::OFFERED != 0 && job(&w, *i).role == role::CARRIER)
        .map(|(i, _)| i)
        .expect("an idle carrier");
    let p = w.entities.by_index(c as u32).unwrap().pos;
    // Its land within 3 becomes neutral.
    for k in 0..=s4_sim::spiral::last(3) {
        w.change_owner(p.x() + s4_sim::spiral::dx(k), p.y() + s4_sim::spiral::dy(k), 0);
    }
    let e = w.entities.by_index(c as u32).unwrap();
    assert_eq!(w.owner_of(w.grid.index(e.x() as u32, e.y() as u32)), 0);
    assert_eq!(e.flags & flag::OFFERED, 0, "NotifyChangeEcoSector takes it out of the offer");
    assert_eq!(job(&w, c).role, role::CARRIER, "the owner change itself does not set it free");
    assert_ne!(e.flags & s4_sim::eco::FLAG_LEFT_ECO, 0, "flag 0x80");
    let mut fled = None;
    let mut home = None;
    for t in 0..6000 {
        s4_sim::tick(&mut w, &[]);
        let r = job(&w, c).role;
        if fled.is_none() && r == role::FLEE {
            fled = Some(t);
        }
        if fled.is_some() && r == role::CARRIER {
            home = Some(t);
            break;
        }
    }
    eprintln!("carrier {c} at {p:?}: fled after {fled:?} ticks, a carrier again after {home:?}");
    assert!(fled.is_some(), "the carrier never fled");
    assert!(home.is_some(), "the carrier did not get home");
    let e = w.entities.by_index(c as u32).unwrap();
    let n = w.grid.index(e.x() as u32, e.y() as u32);
    assert_eq!(w.owner_of(n), 1);
    // Offered at most once: the carrier role's Init offers it, and `CFleeRole` tests OFFERED after the
    // new role (0x577c78), so it does not link it a second time (a self-link, which made
    // `InitTransport` loop forever).
    let eco = s4_sim::eco::eco_of(&w, n);
    let mut seen = Vec::new();
    let mut id = w.economy.get(eco).unwrap().settler_offer[1];
    while id != 0 {
        assert!(!seen.contains(&id), "the carrier offer list loops at {id}");
        seen.push(id);
        id = w.entities.by_index(id as u32).unwrap().next;
    }
    // (It may already have a job again, then it is not offered at all.)
    assert!(seen.iter().filter(|&&x| x == c).count() <= 1, "the carrier is offered twice");
    s4_sim::eco::check_offer_lists(&w).unwrap();
}
