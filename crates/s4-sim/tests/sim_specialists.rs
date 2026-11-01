//! Specialists back to carriers (`CSettler::ChangeType(1, …, 0)`, release 0x580ce0), on roman02:
//! command 5021 `MorphToCarrier` (0x4f662c) and a negative specialist wish in
//! `CEcoSector::RecruiteSpecialist` (0x4d51d0). Skipped without the game data.

use s4_sim::entity::{EntityData, ObjType};
use s4_sim::{Command, PlayerCommand, World};

const CARRIER: u16 = 1;
/// Two specialist types of `eco::SPECIALISTS`.
const PIONEER: u16 = 47;
const GEOLOGIST: u16 = 46;

fn roman02() -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map");
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    let rules = s4_sim::Rules::load_default().ok()?;
    s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).ok().map(|(w, _)| w)
}

/// `n` new settlers of `kind` for player 1 on free nodes of its land near its castle, and the
/// eco-sector they stand in.
fn specialists(w: &mut World, kind: u16, n: usize) -> (Vec<u16>, u16) {
    let castle = w
        .entities
        .iter()
        .find(|(id, e)| e.owner == 1 && e.obj_type == ObjType::Building && s4_sim::buildings::have_inhabitant(w, *id))
        .map(|(_, e)| (e.x(), e.y()))
        .expect("castle");
    let mut out = Vec::new();
    let mut eco = 0;
    for k in s4_sim::spiral::first(8)..s4_sim::spiral::last(20) {
        if out.len() == n {
            break;
        }
        let (x, y) = (castle.0 + s4_sim::spiral::dx(k), castle.1 + s4_sim::spiral::dy(k));
        let Some(node) = w.grid.checked_index(x, y) else { continue };
        let e = s4_sim::eco::eco_of(w, node);
        if w.layers.flags[node] & 0x5f != 0 || w.layers.map_object[node] != 0 || e == 0 || w.economy.owner(e) != 1 || (eco != 0 && e != eco) {
            continue;
        }
        if let Some(s) = s4_sim::create::add_settler(w, x, y, 1, kind) {
            eco = e;
            out.push(s);
        }
    }
    assert_eq!(out.len(), n, "room for the specialists");
    (out, eco)
}

fn kind(w: &World, s: u16) -> u16 {
    w.entities.by_index(s as u32).map_or(0, |e| e.kind)
}

/// 5021: the player's specialists become carriers with the carrier role; other players' and
/// non-specialists are left alone.
#[test]
fn morph_to_carrier_turns_specialists_into_carriers() {
    let Some(mut w) = roman02() else { return };
    let (s, _) = specialists(&mut w, GEOLOGIST, 2);
    for _ in 0..20 {
        s4_sim::tick(&mut w, &[]);
    }
    let before = w.settler_lists.settlers(1, GEOLOGIST);
    // A foreign player's command changes nothing.
    s4_sim::tick(&mut w, &[PlayerCommand { player: 2, command: Command::MorphToCarrier { entities: vec![s[0]] } }]);
    assert_eq!(kind(&w, s[0]), GEOLOGIST);
    s4_sim::tick(&mut w, &[PlayerCommand { player: 1, command: Command::MorphToCarrier { entities: vec![s[0]] } }]);
    assert_eq!(kind(&w, s[0]), CARRIER);
    assert_eq!(kind(&w, s[1]), GEOLOGIST);
    assert_eq!(w.settler_lists.settlers(1, GEOLOGIST), before - 1);
    let role = match &w.entities.by_index(s[0] as u32).unwrap().data {
        EntityData::Settler(d) => d.job.role,
        _ => 0,
    };
    assert_eq!(role, s4_sim::jobs::role::CARRIER);
    for _ in 0..500 {
        s4_sim::tick(&mut w, &[]);
    }
    s4_sim::eco::check_offer_lists(&w).unwrap();
}

/// 5039 with a negative value: at the next `RecruiteSpecialist` (tick & 15 == 15) that many of
/// the player's specialists of the type in the eco-sector become carriers and the wish is 0.
#[test]
fn a_negative_specialist_wish_turns_specialists_back() {
    let Some(mut w) = roman02() else { return };
    let (s, eco) = specialists(&mut w, PIONEER, 3);
    s4_sim::tick(&mut w, &[]);
    let wish = |w: &World| w.economy.get(eco).unwrap().specialist_wish[2];
    s4_sim::tick(&mut w, &[PlayerCommand { player: 1, command: Command::SetSpecialistWish { eco_sector: eco, specialist: PIONEER, value: (-2i32) as u32 } }]);
    assert_eq!(wish(&w), -2);
    for _ in 0..16 {
        s4_sim::tick(&mut w, &[]);
    }
    assert_eq!(wish(&w), 0);
    let carriers = s.iter().filter(|&&id| kind(&w, id) == CARRIER).count();
    assert_eq!(carriers, 2, "two of the three pioneers are carriers");
    for _ in 0..500 {
        s4_sim::tick(&mut w, &[]);
    }
    s4_sim::eco::check_offer_lists(&w).unwrap();
}
