//! Map reader against real install data (skipped when the game directory is absent).

use s4_formats::map::{MapFile, MapText};

fn open(rel: &str) -> Option<MapFile> {
    let p = s4_formats::game_dir().join("Map").join(rel);
    if !p.exists() {
        eprintln!("skipping: {} not found", p.display());
        return None;
    }
    Some(MapFile::open(p).expect("open"))
}

#[test]
fn campaign_map_decodes() {
    let Some(m) = open(r"Campaign\AO_maya1.map") else { return };
    let c = &m.container;
    assert!(c.file_crc_ok());
    for h in &c.chunks {
        assert!(c.chunk_crc_ok(h), "chunk {}/{}", h.kind, h.subtype);
        c.load(h).unwrap();
    }
    let g = m.general().unwrap();
    assert_eq!(g.size, 640);
    let l = m.landscape().unwrap();
    assert_eq!((l.width, l.height, l.nodes.len()), (640, 640, 640 * 640));
    let players = m.players().unwrap();
    assert_eq!(players.len(), 4);
    assert_eq!(players[1].name, "Patritius");
    assert!(!m.team_setups().unwrap().is_empty());
    assert!(m.is_campaign());
    let (side, rgba) = m.preview_rgba().unwrap();
    assert_eq!((side, rgba.len()), (256, 256 * 256 * 4));
    // Settler/building/pile records are validated against the header record counts.
    m.settlers().unwrap();
    m.buildings().unwrap();
    m.piles().unwrap();
    m.text(MapText::Description).unwrap();
}

#[test]
fn edm_decodes() {
    let Some(m) = open(r"Editor\576_demo.edm") else { return };
    assert!(m.container.file_crc_ok());
    let l = m.landscape().unwrap();
    assert_eq!(l.width, m.general().unwrap().size);
}
