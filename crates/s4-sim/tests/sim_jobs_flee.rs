//! `CFleeRole` (role 18): a carrier set free on land that is not its owner's flees back home,
//! where it becomes a carrier again; one with no way home is killed after its fifth target.
//! MCD2_roman1, driven by `tick()`. Skipped when the game data is not present.

use s4_sim::World;
use s4_sim::entity::ObjType;
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

/// Moves settler `i` to the first free node from the spiral around it that `pick` accepts.
fn teleport(w: &mut World, i: u16, pick: impl Fn(&World, usize) -> bool) -> (i32, i32) {
    let e = w.entities.by_index(i as u32).unwrap();
    let (x, y) = (e.x(), e.y());
    let (tx, ty) = s4_sim::spiral::walk(w.grid, x, y, 60)
        .find(|&(px, py)| {
            let n = w.grid.index(px as u32, py as u32);
            w.layers.map_object[n] == 0 && w.layers.flags[n] & 1 == 0 && pick(w, n)
        })
        .expect("a node");
    let from = w.grid.index(x as u32, y as u32);
    w.layers.map_object[from] = 0;
    let to = w.grid.index(tx as u32, ty as u32);
    w.layers.map_object[to] = i;
    w.entities.by_index_mut(i as u32).unwrap().pos = s4_sim::grid::Y16X16::pack(tx, ty);
    (tx, ty)
}

#[test]
fn carrier_flees_home() {
    let Some(mut w) = map() else { return };
    for _ in 0..20 {
        s4_sim::tick(&mut w, &[]);
    }
    let (c, owner) = w
        .entities
        .iter()
        .find(|(_, e)| e.obj_type == ObjType::Settler && e.kind == 1 && e.owner == 1)
        .map(|(i, e)| (i, e.owner))
        .expect("a carrier");
    let sector = {
        let e = w.entities.by_index(c as u32).unwrap();
        w.tiling.sector_of(w.grid.index(e.x() as u32, e.y() as u32))
    };
    // Unowned land on its sector, at least 6 nodes from any owned node.
    let size = w.grid.size() as usize;
    teleport(&mut w, c, |w, n| {
        let (x, y) = ((n % size) as i32, (n / size) as i32);
        w.owner_of(n) == 0 && w.tiling.sector_of(n) == sector && s4_sim::spiral::walk(w.grid, x, y, 6).all(|(px, py)| w.owner_of(w.grid.index(px as u32, py as u32)) == 0)
    });
    assert!(!s4_sim::jobs::roles::set_free(&mut w, c, -1) || job(&w, c).role == role::FLEE);
    assert_eq!(job(&w, c).role, role::FLEE, "set free on foreign land: CFleeRole");
    let mut home = None;
    for t in 0..3000 {
        s4_sim::tick(&mut w, &[]);
        if job(&w, c).role != role::FLEE {
            home = Some(t);
            break;
        }
    }
    let t = home.expect("back home");
    eprintln!("home after {t} ticks");
    let e = w.entities.by_index(c as u32).unwrap();
    assert!(!e.is_dead());
    assert_eq!(job(&w, c).role, role::CARRIER, "a carrier again after {t} ticks");
    assert_eq!(w.owner_of(w.grid.index(e.x() as u32, e.y() as u32)), owner, "on its owner's land");
}

#[test]
fn stranded_settler_is_killed() {
    let Some(mut w) = map() else { return };
    let (c, owner) = w
        .entities
        .iter()
        .find(|(_, e)| e.obj_type == ObjType::Settler && e.kind == 1 && e.owner == 1)
        .map(|(i, e)| (i, e.owner))
        .expect("a carrier");
    let sector = {
        let e = w.entities.by_index(c as u32).unwrap();
        w.tiling.sector_of(w.grid.index(e.x() as u32, e.y() as u32))
    };
    // Unowned land on another sector without any of its owner's land.
    let owned_sectors: std::collections::BTreeSet<u16> = (0..w.layers.flags.len()).filter(|&n| w.owner_of(n) == owner).map(|n| w.tiling.sector_of(n)).collect();
    let Some(n) = (0..w.layers.flags.len()).find(|&n| {
        let s = w.tiling.sector_of(n);
        s != 0 && s != sector && !owned_sectors.contains(&s) && w.owner_of(n) == 0 && w.layers.map_object[n] == 0 && w.layers.flags[n] & 1 == 0
    }) else {
        panic!("MCD2_roman1 has unowned land on another sector");
    };
    let size = w.grid.size() as usize;
    let (tx, ty) = ((n % size) as i32, (n / size) as i32);
    let e = w.entities.by_index(c as u32).unwrap();
    let from = w.grid.index(e.x() as u32, e.y() as u32);
    w.layers.map_object[from] = 0;
    w.layers.map_object[n] = c;
    w.entities.by_index_mut(c as u32).unwrap().pos = s4_sim::grid::Y16X16::pack(tx, ty);
    s4_sim::jobs::roles::set_free(&mut w, c, -1);
    assert_eq!(job(&w, c).role, role::FLEE);
    let uid = w.entities.by_index(c as u32).unwrap().uid;
    for t in 0..5000 {
        s4_sim::tick(&mut w, &[]);
        if w.entities.by_index(c as u32).is_none_or(|e| e.uid != uid || e.is_dead()) {
            eprintln!("killed after {t} ticks");
            return;
        }
    }
    panic!("a settler with no way home dies");
}
