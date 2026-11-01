//! Parse every map under `Map\` (and `*.edm`) with every typed accessor; report counts.
//! Writes ground / height / preview PNGs for a few maps to `out/map/`.
//! `cargo run --release -p s4-formats --example map_info [-v]`

use s4_formats::map::{Landscape, MapFile, MapText, ground_rgb};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut v: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    v.sort();
    for p in v {
        if p.is_dir() {
            walk(&p, out);
        } else if let Some(ext) = p.extension().and_then(|e| e.to_str())
            && ["map", "edm"].contains(&ext.to_ascii_lowercase().as_str())
        {
            out.push(p);
        }
    }
}

#[derive(Default)]
struct Totals {
    nodes: u64,
    objects: u64,
    settlers: u64,
    vehicles: u64,
    buildings: u64,
    piles: u64,
    scripts: u64,
    campaign: u64,
    sizes: BTreeMap<u32, u32>,
    grounds: BTreeMap<u8, u64>,
    unknown_grounds: BTreeMap<u8, u64>,
    max_height: u8,
    versions: BTreeMap<u32, u32>,
}

fn check(path: &Path, t: &mut Totals, verbose: bool) -> anyhow::Result<()> {
    let m = MapFile::open(path)?;
    let c = &m.container;
    anyhow::ensure!(c.file_crc_ok(), "file CRC mismatch");
    for h in &c.chunks {
        anyhow::ensure!(c.chunk_crc_ok(h), "chunk {}/{} CRC mismatch", h.kind, h.subtype);
        c.load(h)?;
    }
    *t.versions.entry(m.version()).or_default() += 1;
    let g = m.general()?;
    let players = m.players()?;
    let teams = m.team_setups()?;
    anyhow::ensure!(!teams.is_empty(), "no team setup");
    let l = m.landscape()?;
    let objects = m.objects()?;
    let settlers = m.settlers()?;
    let buildings = m.buildings()?;
    let piles = m.piles()?;
    let (pside, _) = m.preview_rgba()?;
    for w in [MapText::Description, MapText::Tips, MapText::DescriptionEnglish, MapText::TipsEnglish] {
        m.text(w)?;
    }
    let script = m.script()?;
    m.editor_info()?;
    for p in &players {
        anyhow::ensure!(p.start_x < g.size && p.start_y < g.size, "player {} start outside map", p.id);
    }
    for s in &settlers {
        anyhow::ensure!((s.x as u32) < g.size && (s.y as u32) < g.size, "settler outside map");
    }
    for b in &buildings {
        anyhow::ensure!((b.x as u32) < g.size && (b.y as u32) < g.size, "building outside map");
    }
    t.nodes += l.nodes.len() as u64;
    t.objects += objects.len() as u64;
    t.vehicles += settlers.iter().filter(|s| s.vehicle_type().is_some()).count() as u64;
    t.settlers += settlers.len() as u64;
    t.buildings += buildings.len() as u64;
    t.piles += piles.len() as u64;
    t.scripts += script.is_some() as u64;
    t.campaign += m.is_campaign() as u64;
    *t.sizes.entry(g.size).or_default() += 1;
    for n in &l.nodes {
        *t.grounds.entry(n.ground).or_default() += 1;
        if ground_rgb(n.ground).is_none() {
            *t.unknown_grounds.entry(n.ground).or_default() += 1;
        }
        t.max_height = t.max_height.max(n.height);
    }
    if verbose {
        println!(
            "{}: v{} size {} players {} setups {} objects {} settlers {} buildings {} piles {} preview {} general {:?}",
            path.display(), m.version(), g.size, players.len(), teams.len(), objects.len(),
            settlers.len(), buildings.len(), piles.len(), pside, g
        );
    }
    Ok(())
}

fn save_png(path: &Path, w: u32, h: u32, rgba: &[u8]) -> anyhow::Result<()> {
    let f = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut e = png::Encoder::new(f, w, h);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header()?.write_image_data(rgba)?;
    Ok(())
}

/// Plain row-major rendering (x right, y down) — the same orientation as the preview chunk.
fn plain(l: &Landscape, f: impl Fn(&s4_formats::map::Node) -> [u8; 4]) -> (u32, u32, Vec<u8>) {
    let img = l.nodes.iter().flat_map(f).collect();
    (l.width, l.height, img)
}

fn render(path: &Path, out: &Path) -> anyhow::Result<()> {
    let m = MapFile::open(path)?;
    let stem = path.file_stem().unwrap().to_string_lossy().replace(' ', "_");
    let l = m.landscape()?;
    let ground = |n: &s4_formats::map::Node| {
        let [r, g, b] = ground_rgb(n.ground).unwrap_or([255, 0, 255]);
        // Darken by gradient so relief is visible on top of the ground colour.
        let k = 255 - (n.gradient & 0xF) as u32 * 8;
        [(r as u32 * k / 255) as u8, (g as u32 * k / 255) as u8, (b as u32 * k / 255) as u8, 255]
    };
    let (w, h, img) = plain(&l, ground);
    save_png(&out.join(format!("{stem}_ground.png")), w, h, &img)?;
    let maxh = l.nodes.iter().map(|n| n.height).max().unwrap_or(1).max(1) as u32;
    let (w, h, img) = plain(&l, |n| {
        let v = (n.height as u32 * 255 / maxh) as u8;
        if n.is_water() { [0, 0, v / 2 + 64, 255] } else { [v, v, v, 255] }
    });
    save_png(&out.join(format!("{stem}_height.png")), w, h, &img)?;
    let (side, rgba) = m.preview_rgba()?;
    save_png(&out.join(format!("{stem}_preview.png")), side, side, &rgba)?;
    // Same pixels read as RGB565, kept for comparison: wrong colours (see report).
    let (_, raw) = m.preview_raw()?;
    let alt: Vec<u8> = raw
        .iter()
        .flat_map(|&p| {
            let (r, g, b) = (p >> 11, (p >> 5) & 0x3F, p & 0x1F);
            [(r * 255 / 31) as u8, (g * 255 / 63) as u8, (b * 255 / 31) as u8, 255]
        })
        .collect();
    save_png(&out.join(format!("{stem}_preview565.png")), side, side, &alt)?;
    println!("rendered {stem}: {}x{}", l.width, l.height);
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let verbose = std::env::args().any(|a| a == "-v");
    let root = s4_formats::game_dir().join("Map");
    let mut files = Vec::new();
    walk(&root, &mut files);
    let mut t = Totals::default();
    let (mut ok, mut fail) = (0, 0);
    for f in &files {
        match check(f, &mut t, verbose) {
            Ok(()) => ok += 1,
            Err(e) => {
                fail += 1;
                println!("FAIL {}: {e}", f.display());
            }
        }
    }
    println!("{} files: {ok} ok, {fail} failed", files.len());
    println!("versions {:?}", t.versions);
    println!("map sizes {:?}", t.sizes);
    println!(
        "nodes {} objects {} settlers {} (vehicles {}) buildings {} piles {} scripts {} campaign {}",
        t.nodes, t.objects, t.settlers, t.vehicles, t.buildings, t.piles, t.scripts, t.campaign
    );
    println!("max height {}", t.max_height);
    println!("ground types {:?}", t.grounds);
    println!("ground types without a minimap colour {:?}", t.unknown_grounds);

    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/map");
    std::fs::create_dir_all(&out)?;
    for name in [r"Campaign\AO_maya1.map", r"Multiplayer\4 Isles.map", r"User\Auenland.map"] {
        if let Err(e) = render(&root.join(name), &out) {
            println!("render {name}: {e}");
        }
    }
    Ok(())
}
