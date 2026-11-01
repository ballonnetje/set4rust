//! The generated map of a key against savegames of a game on it: per save the share of nodes
//! whose landscape (chunk 200, the render map: height, ground) equals the generator's, split by
//! what the game may have changed there (owned land: chunk 210; objects: 201 / 202).
//!
//! cargo run --release -p s4-mapgen --example save_compare -- RD_H8GQE2M0 SAVE...
use s4_formats::map::MapFile;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let key = &args[0];
    // A key, or a map file as the control (its chunk 13 as the "generated" landscape).
    let m = if key.ends_with(".map") {
        let f = MapFile::open(key).unwrap();
        let size = f.general().unwrap().size as i32;
        let (_, d) = f.container.load_id(13, 0).unwrap().unwrap();
        let land: Vec<[u8; 4]> = d.chunks(4).map(|c| [c[0], c[1], c[2], c[3]]).collect();
        s4_mapgen::Map { size, players: 0, objs: vec![[0; 4]; land.len()], land, starts: [(0, 0); 9], preview: Vec::new() }
    } else {
        let data = s4_formats::gamedata::GameData::load_default().unwrap();
        s4_mapgen::generate(&s4_mapgen::key::parse(key).unwrap(), s4_mapgen::objects::records(&data))
    };
    let n = (m.size * m.size) as usize;
    println!("{key}: size {}, starts {:?}", m.size, &m.starts[1..=m.players as usize]);
    let mut type_pairs = std::collections::BTreeMap::<(u8, u8), usize>::new();
    for path in &args[1..] {
        let f = match MapFile::open(path) {
            Ok(f) => f,
            Err(e) => {
                println!("{path}: {e}");
                continue;
            }
        };
        let load = |id: u16| f.container.load_id(id, 0).ok().flatten().map(|c| c.1);
        let Some(render) = load(200) else { continue };
        let owner = load(210).unwrap_or_default();
        if render.len() < n * 4 {
            println!("{path}: chunk 200 has {} bytes", render.len());
            continue;
        }
        let off = render.len() - n * 4;
        let (mut h, mut g, mut both) = (0, 0, 0);
        let (mut free, mut free_g, mut owned, mut owned_g) = (0, 0, 0, 0);
        for i in 0..n {
            let s = &render[off + i * 4..off + i * 4 + 4];
            let (sh, sg) = (s[0], s[1]);
            let (gh, gg) = (m.land[i][0], m.land[i][1]);
            h += (sh == gh) as usize;
            g += (sg == gg) as usize;
            both += (sh == gh && sg == gg) as usize;
            let own = owner.get(i).copied().unwrap_or(0) != 0;
            if own {
                owned += 1;
                owned_g += (sg == gg) as usize;
            } else {
                free += 1;
                free_g += (sg == gg) as usize;
            }
            if sg != gg && !own {
                *type_pairs.entry((gg, sg)).or_default() += 1;
            }
        }
        // DUMP=dir: the generated and the saved landscape (4 bytes per node) for pictures.
        if let Ok(dir) = std::env::var("DUMP") {
            let name = std::path::Path::new(path).file_stem().unwrap().to_string_lossy().into_owned();
            std::fs::write(format!("{dir}/gen.bin"), m.land.concat()).unwrap();
            std::fs::write(format!("{dir}/{name}.bin"), &render[off..]).unwrap();
        }
        let pc = |a: usize, b: usize| 100.0 * a as f64 / b.max(1) as f64;
        let name = std::path::Path::new(path).file_name().unwrap().to_string_lossy().into_owned();
        println!(
            "{name}: header {} | height {:.2}% ground {:.2}% both {:.2}% | unowned {} nodes ground {:.2}% | owned {} nodes ground {:.2}%",
            off,
            pc(h, n),
            pc(g, n),
            pc(both, n),
            free,
            pc(free_g, free),
            owned,
            pc(owned_g, owned)
        );
    }
    let mut v: Vec<_> = type_pairs.into_iter().collect();
    v.sort_by_key(|e| std::cmp::Reverse(e.1));
    println!("unowned ground mismatches (generated → saved): {:?}", &v[..v.len().min(20)]);
}
