//! Exploration: decompress every chunk of every map/edm/sav and print per-type statistics.
//! `cargo run --example map_probe [dump <file> <type> <sub>]`

use s4_formats::map::{MapFile, chunk_name};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else if let Some(ext) = p.extension().and_then(|e| e.to_str())
            && ["map", "edm", "sav"].contains(&ext.to_ascii_lowercase().as_str())
        {
            out.push(p);
        }
    }
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 5 && args[1] == "dump" {
        let m = MapFile::open(&args[2])?;
        let (h, d) = m.container.load_id(args[3].parse()?, args[4].parse()?)?.expect("no chunk");
        println!("{h:?}");
        for (i, row) in d.chunks(32).enumerate().take(64) {
            print!("{:6x}: ", i * 32);
            for b in row {
                print!("{b:02x} ");
            }
            println!();
        }
        return Ok(());
    }
    let mut files = Vec::new();
    walk(&s4_formats::game_dir().join("Map"), &mut files);
    if args.get(1).map(String::as_str) == Some("fields") {
        return fields(&files);
    }
    if let Some(home) = std::env::var_os("USERPROFILE") {
        walk(&Path::new(&home).join(r"Documents\TheSettlers4\Save"), &mut files);
    }
    // type -> (count, crc fails, decomp fails, min size, max size, params seen)
    type Stat = (usize, usize, usize, usize, usize, BTreeMap<u32, usize>);
    let mut stats: BTreeMap<u16, Stat> = BTreeMap::new();
    let mut bad_file_crc = 0;
    for f in &files {
        let m = match MapFile::open(f) {
            Ok(m) => m,
            Err(e) => {
                println!("OPEN FAIL {}: {e}", f.display());
                continue;
            }
        };
        let c = &m.container;
        if !c.file_crc_ok() {
            bad_file_crc += 1;
            println!("file crc mismatch {} stored {:x} calc {:x}", f.display(), c.file_crc, c.computed_file_crc());
        }
        for h in &c.chunks {
            let e = stats.entry(h.kind).or_insert((0, 0, 0, usize::MAX, 0, BTreeMap::new()));
            e.0 += 1;
            if !c.chunk_crc_ok(h) {
                e.1 += 1;
                continue;
            }
            match c.load(h) {
                Ok(d) => {
                    e.3 = e.3.min(d.len());
                    e.4 = e.4.max(d.len());
                    *e.5.entry(h.param).or_default() += 1;
                }
                Err(err) => {
                    e.2 += 1;
                    if e.2 < 4 {
                        println!("  decomp fail {} {}/{}: {err}", f.display(), h.kind, h.subtype);
                    }
                }
            }
        }
    }
    println!("{} files, {bad_file_crc} bad file crc", files.len());
    for (k, (n, crcf, df, mn, mx, params)) in &stats {
        let ps: Vec<String> = params.iter().take(8).map(|(p, c)| format!("{p}x{c}")).collect();
        println!(
            "type {k:5} {:22} n={n:5} crcfail={crcf} decompfail={df} size {mn}..{mx} params[{}]: {}",
            chunk_name(*k).unwrap_or("?"),
            params.len(),
            ps.join(" ")
        );
    }
    Ok(())
}

/// Value histograms for fields whose meaning is not fixed by a reader.
fn fields(files: &[PathBuf]) -> anyhow::Result<()> {
    use s4_formats::map::id;
    let mut h: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    let mut add = |k: &str, v: String| *h.entry(k.into()).or_default().entry(v).or_default() += 1;
    for f in files {
        let m = MapFile::open(f)?;
        let g = m.general()?;
        let np = m.players()?.len();
        add("general.flags", format!("{:#x}", g.flags));
        add("general.player_count - players", format!("{}", g.player_count as i64 - np as i64));
        add("general.start_resources", format!("{}", g.start_resources));
        add("general.unknown_10", format!("{:#x}", g.unknown_10));
        add("general.options", format!("{:#x}", g.options));
        for p in m.players()? {
            add("player.race", format!("{}", p.race));
        }
        for t in m.team_setups()? {
            for s in t.slots {
                add("team.team", format!("{}", s.team));
                add("team.control", format!("{}", s.control));
            }
        }
        for s in m.settlers()? {
            add("settler.kind", format!("{}", s.kind));
            add("settler.owner", format!("{}", s.owner));
        }
        for b in m.buildings()? {
            add("building.kind", format!("{}", b.kind));
            add("building.owner", format!("{}", b.owner));
            add("building.has_garrison", format!("{}", b.has_garrison));
            add("building.unknown_10", format!("{:?}", b.unknown_10));
            add("building.soldiers", format!("{:?}", b.soldiers));
        }
        for p in m.piles()? {
            add("pile.good", format!("{}", p.good));
            add("pile.amount", format!("{}", p.amount));
            add("pile.kind_flag", format!("{}", p.kind_flag));
            add("pile.unknown_07", format!("{}", p.unknown_07));
        }
        for (k, name) in [(0u16, "end"), (64, "editor_flag"), (65, "campaign_flag"), (256, "unknown_256"), (10, "unknown_10")] {
            if let Some((_, d)) = m.container.load_id(k, 0)? {
                add(&format!("chunk.{name}"), d.iter().map(|b| format!("{b:02x}")).collect());
            }
        }
        if let Some(e) = m.editor_info()? {
            add("editor.unknown_00", format!("{}", e.unknown_00));
            add("editor.xml", format!("{} {}", e.building_xml_version, e.object_xml_version));
            add("editor.unknown_06", format!("{:02x?}", e.unknown_06));
        }
        let l = m.landscape()?;
        for n in &l.nodes {
            add("node.gradient&0xF0", format!("{:#x}", n.gradient & 0xF0));
            add("node.gfx_bits", format!("{:#x}", n.gfx_bits));
            add("node.flags", format!("{:#x}", n.flags));
            add("node.owner", format!("{}", n.owner));
            add("node.resource_type", format!("{:#x}", n.resource_type()));
        }
        add("ext+chunk64", format!("{} {}", f.extension().unwrap().to_string_lossy(), m.is_editor_map()));
        let _ = id::END;
    }
    for (k, vals) in &h {
        let mut v: Vec<_> = vals.iter().collect();
        v.sort_by(|a, b| b.1.cmp(a.1));
        let shown: Vec<String> = v.iter().take(14).map(|(a, n)| format!("{a}x{n}")).collect();
        println!("{k} [{} distinct]: {}", vals.len(), shown.join("  "));
    }
    Ok(())
}
