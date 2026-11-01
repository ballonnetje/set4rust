//! Dark land (`CDarkTribe` 0x102dff0) through the shadow herb (`CShadowHerb`, deco 216): planted
//! near trees on MCD2_roman1, it lives 14 + 1 + 31 + 31 + 16 ticks and darkens the land around
//! it twice (0x4cb9a0); no light tree is left on dark land (0x4cbb60 → 0x4cbcc0 turns those on
//! a darkened node, or whose ring closes around it, into their dark variants). Skipped when the
//! game data is not present.

use s4_sim::World;
use s4_sim::entity::ObjType;

fn map() -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("MCD2_roman1.map");
    if !p.exists() {
        return None;
    }
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    let rules = s4_sim::Rules::load_default().ok()?;
    s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).ok().map(|(w, _)| w)
}

fn dark(w: &World, x: i32, y: i32) -> bool {
    w.grid.checked_index(x, y).is_some_and(|n| w.layers.flags[n] & 4 != 0)
}

fn deco_kind(w: &World, x: i32, y: i32) -> u16 {
    let n = w.grid.index(x as u32, y as u32);
    w.entities.by_index(w.layers.deco_object[n] as u32).map_or(0, |e| e.kind)
}

/// A node where a herb may go (no object ring on it), with light trees (types 1..=14) on two of
/// the second ring's nodes.
fn spot(w: &World) -> Option<(i32, i32)> {
    let size = w.grid.size() as i32;
    let ring2 = s4_sim::spiral::first(2)..=s4_sim::spiral::last(2);
    for y in 8..size - 8 {
        for x in 8..size - 8 {
            let trees = ring2.clone().filter(|&i| (1..=14).contains(&deco_kind(w, x + s4_sim::spiral::dx(i), y + s4_sim::spiral::dy(i)))).count();
            if trees >= 2 && !dark(w, x, y) && s4_sim::deco::is_add_deco_obj_ok(w, x, y, 216, 0) {
                return Some((x, y));
            }
        }
    }
    None
}

/// (state hash, dark trees around the herb, dark nodes on the map).
type Outcome = (u64, Vec<(i32, i32, u16)>, usize);

fn run() -> Option<Outcome> {
    let mut w = map()?;
    let (x, y) = spot(&w).expect("a spot next to trees");
    let herb = s4_sim::deco::add_deco_obj_without_flags(&mut w, x, y, 216, 0).ok()?;
    s4_sim::deco::set_flags_for_object(&mut w, x, y, 216);
    let mut died = None;
    for t in 1..=120 {
        s4_sim::tick(&mut w, &[]);
        let alive = w.entities.by_index(herb as u32).is_some_and(|e| e.obj_type == ObjType::Deco && e.kind == 216 && !e.is_dead());
        if !alive && died.is_none() {
            died = Some(t);
        }
    }
    assert_eq!(died, Some(14 + 1 + 31 + 31 + 16), "the herb's life");
    assert!(dark(&w, x, y), "the herb's node is dark");
    assert!(dark(&w, x + s4_sim::spiral::dx(6), y + s4_sim::spiral::dy(6)), "spiral entry 6 always darkens");
    let mut dark_trees = Vec::new();
    for i in 0..=s4_sim::spiral::last(3) {
        let (px, py) = (x + s4_sim::spiral::dx(i), y + s4_sim::spiral::dy(i));
        let k = deco_kind(&w, px, py);
        if dark(&w, px, py) {
            assert!(!(1..=14).contains(&k), "a light tree {k} on dark land at ({px},{py})");
        }
        if (23..=31).contains(&k) {
            dark_trees.push((px, py, k));
        }
    }
    let dark_nodes = w.layers.flags.iter().filter(|&&f| f & 4 != 0).count();
    Some((w.state_hash(), dark_trees, dark_nodes))
}

#[test]
fn shadow_herb_darkens_the_land_and_its_trees() {
    let Some(a) = run() else { return };
    eprintln!("hash {:x}, dark trees {:?}, dark nodes {}", a.0, a.1, a.2);
    assert!(a.2 >= 2);
    assert_eq!(Some(a), run(), "not deterministic");
}

/// 0x4cbb60 → 0x4cbcc0 on a tree's own node: darkening turns it into its dark variant (pair
/// table 0x101a430), lightening back into the first light type of that pair; its stand-in keeps
/// the node's state.
#[test]
fn trees_change_with_their_node() {
    let Some(mut w) = map() else { return };
    let size = w.grid.size() as i32;
    let (x, y, kind) = (8..size - 8)
        .flat_map(|y| (8..size - 8).map(move |x| (x, y)))
        .find_map(|(x, y)| {
            let k = deco_kind(&w, x, y);
            ((1..=14).contains(&k) && !dark(&w, x, y)).then_some((x, y, k))
        })
        .expect("a light tree");
    s4_sim::dark::darken_around(&mut w, x, y, false);
    let d = deco_kind(&w, x, y);
    eprintln!("tree {kind} at ({x},{y}) -> {d}");
    assert!(dark(&w, x, y));
    assert!((23..=31).contains(&d), "dark variant of {kind}: {d}");
    s4_sim::dark::lighten(&mut w, x, y, 0);
    let l = deco_kind(&w, x, y);
    assert!(!dark(&w, x, y));
    assert!((1..=14).contains(&l), "light variant of {d}: {l}");
}

/// `CMushroom` (0x565ea0 / 0x565f90): a planted mushroom grows a stage every 5 updates of 31
/// ticks and at stage 3 becomes the shared static instance, READY; a beaten copy (0x565bd0, then
/// vtbl+0x20) loses READY and is deleted after its last frame (a frame every 5 updates).
#[test]
fn mushrooms_grow_and_die_when_beaten() {
    let Some(mut w) = map() else { return };
    let size = w.grid.size() as i32;
    let (x, y) = (8..size - 8)
        .flat_map(|y| (8..size - 8).map(move |x| (x, y)))
        .find(|&(x, y)| s4_sim::deco::is_add_deco_obj_ok(&w, x, y, 213, 0))
        .expect("room for a mushroom");
    s4_sim::deco::plant(&mut w, x, y, 213);
    let n = w.grid.index(x as u32, y as u32);
    let planted = w.layers.deco_object[n];
    // At stage 3, ChangeToStaticInstance (0x564390) destroys the planted copy at once (0x56d200
    // lowers the min free id) and only then does AddDecoObjWithoutFlags (0x563940) take
    // GetFreeSlot (0x56cf10, the lowest free id from the min free id) for the new static
    // instance. So the instance gets the planted id unless a lower id was free.
    let mut expected = 0;
    for t in 1..=(3 * 5 * 31 + 1) {
        if t == 3 * 5 * 31 + 1 {
            expected = (1..planted).find(|&i| w.entities.by_index(i as u32).is_none()).unwrap_or(planted);
        }
        s4_sim::tick(&mut w, &[]);
        let e = w.entities.by_index(w.layers.deco_object[n] as u32).expect("a mushroom");
        let grown = matches!(&e.data, s4_sim::entity::EntityData::Deco(d) if d.static_instance);
        assert_eq!(grown, t > 3 * 5 * 31, "static from tick {}, now {t}", 3 * 5 * 31 + 1);
    }
    let grown = w.layers.deco_object[n];
    let e = w.entities.by_index(grown as u32).expect("a mushroom");
    assert_eq!(grown, expected, "the static instance takes the lowest free id");
    assert_eq!(e.kind, 213);
    assert!(e.flags & s4_sim::entity::flag::READY != 0, "grown mushrooms are READY");
    assert!(matches!(&e.data, s4_sim::entity::EntityData::Deco(d) if d.static_instance && d.stage == 3));
    // The gardener's beating.
    let copy = s4_sim::deco::mushroom::make_dynamic(&mut w, grown, x, y).expect("a copy");
    s4_sim::deco::mushroom::hit(&mut w, copy);
    let frames = w.rules.object_frames(213, 4) as u32;
    let mut gone = None;
    for t in 1..=(frames * 5 * 31 + 40) {
        s4_sim::tick(&mut w, &[]);
        if gone.is_none() && w.layers.deco_object[n] != copy {
            gone = Some(t);
        }
    }
    // (The hit is registered from outside a tick: the first update comes one tick later.)
    assert_eq!(gone, Some(frames * 5 * 31 + 1), "deleted after its last frame");
}
