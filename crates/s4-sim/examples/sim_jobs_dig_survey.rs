//! Survey of saved diggers (type 2) and builders (type 3) in chunk 161: list, cursor, task, the
//! role tail, and the flags / height of the node they stand on or walk to. Used to design the
//! digger / builder oracle in `tests/sim_jobs_save.rs`.
//!
//! `cargo run --release -p s4-sim --example sim_jobs_dig_survey [save dir]`

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use s4_sim::jobs::save_settler;
use s4_sim::save::SaveGame;

fn saves(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            saves(&p, out);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("sav")) {
            out.push(p);
        }
    }
}

fn main() {
    let dir = std::env::args().nth(1).map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(std::env::var_os("USERPROFILE").unwrap_or_default()).join(r"Documents\TheSettlers4\Save")
    });
    let mut files = Vec::new();
    saves(&dir, &mut files);
    let mut hist = BTreeMap::<String, usize>::new();
    let mut shown = 0;
    for f in &files {
        let Ok(sg) = SaveGame::open(f) else { continue };
        let Ok((settlers, _)) = save_settler::load(f) else { continue };
        let size = sg.size as i32;
        let node = |x: i32, y: i32| (y * size + x) as usize;
        for s in settlers.iter().filter(|s| s.role == 2 || s.role == 3) {
            let t = &s.role_tail;
            let (x, y) = s.pos;
            let n = node(x, y);
            let flags = sg.flags.get(n).copied().unwrap_or(0);
            let h = sg.render.get(4 * n).copied().unwrap_or(0) as i64;
            let dest = s.role_fields[10];
            let (dx, dy) = ((dest & 0xFFFF) as i32, (dest >> 16) as i32);
            let dn = node(dx, dy);
            let dflags = sg.flags.get(dn).copied().unwrap_or(0);
            let dh = sg.render.get(4 * dn).copied().unwrap_or(0) as i64;
            let tail: Vec<i64> = t.iter().take(4).copied().collect();
            let key = format!(
                "role {} list {} cur {:?} task {} tail0 {}",
                s.role,
                s.list_id,
                s.cursor,
                s.task(),
                tail.first().copied().unwrap_or(-99)
            );
            *hist.entry(key).or_default() += 1;
            if s.role == 2 && s.list_id == 161 {
                let target = t.get(1).copied().unwrap_or(-1);
                let k = format!(
                    "digger161 cur {:?} task {} working {} picked(pos) {} lev(pos) {} picked(dest) {} h==t {} dest==pos {}",
                    s.cursor,
                    s.task(),
                    t.get(2).copied().unwrap_or(-1),
                    flags & 0x20 != 0,
                    flags & 0x10 != 0,
                    dflags & 0x20 != 0,
                    if s.task() == 6 { dh == target } else { h == target },
                    (dx, dy) == (x, y)
                );
                *hist.entry(k).or_default() += 1;
            }
            if shown < 12 && s.role == 3 {
                shown += 1;
                println!("{}: builder {} pos ({x},{y}) dir {} task {} list {} cur {:?} home {} tail {:?} dest ({dx},{dy}) start {}", f.display(), s.id, s.direction, s.task(), s.list_id, s.cursor, s.home(), tail, s.role_fields[11]);
            }
        }
    }
    for (k, v) in &hist {
        println!("{v:8}  {k}");
    }
}
