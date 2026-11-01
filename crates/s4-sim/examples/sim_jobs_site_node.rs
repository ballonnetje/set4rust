//! What occupies a construction site's own node in the saves (chunk 202 map objects): the site
//! itself, a worker standing on it, or nothing; and the site node's flags. Explains how diggers
//! reach the site position (event 1 sends them there).
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use s4_sim::jobs::save_settler;
use s4_sim::save::SaveGame;
fn saves(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() { saves(&p, out) } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("sav")) { out.push(p) }
    }
}
fn main() {
    let mut files = Vec::new();
    saves(&PathBuf::from(std::env::var_os("USERPROFILE").unwrap()).join(r"Documents\TheSettlers4\Save"), &mut files);
    let mut hist = BTreeMap::<String, usize>::new();
    for p in files.iter().take(40) {
        let Ok(sg) = SaveGame::open(p) else { continue };
        let Ok((settlers, _)) = save_settler::load(p) else { continue };
        let size = sg.size as i32;
        let mut sites = BTreeMap::<u16, Vec<&save_settler::SavedSettler>>::new();
        for s in settlers.iter().filter(|s| s.role == 2 && s.home() != 0) {
            sites.entry(s.home()).or_default().push(s);
        }
        for (site, ws) in &sites {
            let Some(b) = sg.buildings.iter().find(|b| b.id == *site) else { continue };
            let n = (b.pos.y() * size + b.pos.x()) as usize;
            let mo = sg.map_object[n];
            let who = if mo == *site { "site".to_string() } else if mo == 0 { "empty".into() } else if ws.iter().any(|s| s.id == mo) { format!("own digger list {}", ws.iter().find(|s| s.id == mo).unwrap().list_id) } else { "other".into() };
            *hist.entry(format!("site node holds {who}; node flags {:#x}; deco==site {}", sg.flags[n], sg.deco_object[n] == *site)).or_default() += 1;
        }
    }
    for (k, v) in hist { println!("{v:6} {k}"); }
}
