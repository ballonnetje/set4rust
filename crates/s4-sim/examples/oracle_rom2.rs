//! Territory oracle anomaly probe: lists land nodes that exactly one player influences but that
//! the save gives another owner, plus the military buildings and settlers near them.
//! `cargo run -p s4-sim --example oracle_rom2 -- FILE [--role]`: with `--role`, "inhabited" is
//! `IBuildingRole::m_bWorkerInHouse` (+29) read from the building's role record instead of the
//! "garrisoned settler on the building node" approximation of `tests/sim_territory.rs`.
use s4_sim::entity::{BuildingData, Entity, EntityData, ObjType, flag};
use s4_sim::save::SaveGame;
use s4_sim::territory::{influence_info, influence_map};
use s4_sim::{Grid, Rules, World, Y16X16};

fn main() {
    let path = std::env::args().nth(1).expect("path to .sav");
    let s = SaveGame::open(&path).expect("open");
    let rules = Rules::load_default().expect("rules");
    let render = s4_formats::map::MapFile::open(&path).unwrap().container.load_id(200, 0).unwrap().unwrap().1;
    let mut w = World::new(Grid::new(s.size).unwrap(), rules);
    w.layers.flags.copy_from_slice(&s.flags);
    w.layers.map_object.copy_from_slice(&s.map_object);
    for (i, c) in render.as_chunks::<4>().0.iter().enumerate() {
        w.layers.ground[i] = c[1];
        w.layers.gfx_bits[i] = c[3];
    }
    w.init_tiling(&s.owner);
    let use_role = std::env::args().any(|a| a == "--role");
    let text = s4_formats::map::MapFile::open(&path).unwrap().container.load_id(162, 0).unwrap().unwrap().1;
    let toks: Vec<&[u8]> = text.split(|c| c.is_ascii_whitespace() || *c == 0).filter(|t| !t.is_empty()).collect();
    let on_node = |b: &s4_sim::save::SavedEntity| s.settlers.iter().filter(|t| t.pos == b.pos && t.flags & flag::ON_BOARD != 0).count();
    for b in &s.buildings {
        let approx = b.flags & flag::READY != 0 && b.race != 3 && on_node(b) > 0;
        let role = worker_in_house(&toks, b);
        if influence_info(b.kind).0 > 0 && role != Some(approx) {
            println!("tower {} kind {} owner {} ready {}: approx inhabited {approx}, role m_bWorkerInHouse {role:?}", b.id, b.kind, b.owner, b.flags & flag::READY != 0);
        }
        let inhabited = if use_role { role == Some(true) && b.race != 3 } else { approx };
        let data = BuildingData {
            type_ex: b.kind as u8,
            door: Y16X16(0),
            ensign: Y16X16(0),
            working_area: Y16X16(0),
            health: 0,
            events: Vec::new(),
        };
        w.entities.insert(b.id, Entity::new(ObjType::Building, b.kind, b.owner, b.race, b.pos, EntityData::Building(data)));
        s4_sim::buildings::restore_military_inhabited(&mut w, b.id, inhabited);
    }
    println!("size {} tick {} ({} min) buildings {} settlers {}", s.size, s.tick, s.tick / 846, s.buildings.len(), s.settlers.len());
    let (mask, best) = influence_map(&w);
    let sz = s.size as usize;
    let mut bad = Vec::new();
    for (i, &m) in mask.iter().enumerate() {
        if m.count_ones() == 1 && !w.layers.is_water(i) && s.owner[i] as u32 != m.trailing_zeros() + 1 {
            bad.push(i);
            println!(
                "node ({},{}) saved owner {} influence by {} flags {:#04x} mapobj {}",
                i % sz,
                i / sz,
                s.owner[i],
                best[i],
                s.flags[i],
                s.map_object[i]
            );
        }
    }
    if bad.is_empty() {
        return;
    }
    let (cx, cy) = (
        bad.iter().map(|i| (i % sz) as i64).sum::<i64>() / bad.len() as i64,
        bad.iter().map(|i| (i / sz) as i64).sum::<i64>() / bad.len() as i64,
    );
    println!("centroid ({cx},{cy}); military buildings within 120:");
    for b in &s.buildings {
        let (dx, dy) = (b.pos.x() as i64 - cx, b.pos.y() as i64 - cy);
        if dx.abs().max(dy.abs()) > 120 || influence_info(b.kind).0 == 0 {
            continue;
        }
        let on_board: Vec<_> = s.settlers.iter().filter(|t| t.pos == b.pos).map(|t| (t.kind, t.owner, t.flags)).collect();
        println!(
            "  id {} kind {} owner {} race {} at ({},{}) hp {} flags {:#x} uid {:#x} settlers-on-node {:?}",
            b.id,
            b.kind,
            b.owner,
            b.race,
            b.pos.x(),
            b.pos.y(),
            b.health,
            b.flags,
            b.uid,
            on_board
        );
    }
    for b in s.buildings.iter().filter(|b| influence_info(b.kind).0 > 0 && b.owner == 1 && b.kind == 46) {
        println!("settlers within 15 of tower {} at ({},{}):", b.id, b.pos.x(), b.pos.y());
        for t in &s.settlers {
            let (dx, dy) = (t.pos.x() - b.pos.x(), t.pos.y() - b.pos.y());
            if dx.abs().max(dy.abs()) <= 15 {
                println!("    id {} kind {} owner {} at ({},{}) hp {} flags {:#x}", t.id, t.kind, t.owner, t.pos.x(), t.pos.y(), t.health, t.flags);
            }
        }
    }
    // Saved owners in the area, as a small map around the centroid.
    println!("owner map (saved) rows y={}..; '*' = anomaly node", cy - 30);
    for y in (cy - 30).max(0)..(cy + 30).min(sz as i64) {
        let row: String = ((cx - 45).max(0)..(cx + 45).min(sz as i64))
            .map(|x| {
                let i = y as usize * sz + x as usize;
                if bad.contains(&i) {
                    '*'
                } else if s.map_object[i] != 0 && s.buildings.iter().any(|b| b.pos.x() as i64 == x && b.pos.y() as i64 == y) {
                    'B'
                } else if w.layers.is_water(i) {
                    '~'
                } else {
                    char::from(b'0' + s.owner[i])
                }
            })
            .collect();
        println!("{y:4} {row}");
    }
}

/// `IBuildingRole::Store`: `version 4, u8 +4, u8 +5, u16 +6 (building id), u16 +8, s16 +10, u16 +12,
/// int +16, u16 +20, +22, +24, +26, bool +28, bool +29 (m_bWorkerInHouse)`. Located as the first
/// `4 _ _ id` after the building's IEntity header.
fn worker_in_house(toks: &[&[u8]], b: &s4_sim::save::SavedEntity) -> Option<bool> {
    let id = b.id.to_string();
    let uid = b.uid.to_string();
    let h = (0..toks.len().saturating_sub(10)).find(|&i| toks[i + 2] == id.as_bytes() && toks[i + 9] == uid.as_bytes())?;
    let k = (h + 10..h + 60).find(|&k| toks.get(k) == Some(&id.as_bytes()) && toks.get(k - 3) == Some(&b"4".as_slice()))?;
    match *toks.get(k + 10)? {
        b"0" => Some(false),
        b"1" => Some(true),
        _ => None,
    }
}
