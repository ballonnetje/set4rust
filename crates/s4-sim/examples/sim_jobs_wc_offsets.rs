//! Saved woodcutter in-node offsets (CFreeWorkerRole +36/+40) by list position and counter, to
//! pin down how GO_VIRTUAL moves the offset.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use s4_sim::jobs::save_settler;
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
    let mut hist = BTreeMap::<(i16, u16, i64, i64, i64, i64, i64), usize>::new();
    for p in &files {
        let Ok((settlers, _)) = save_settler::load(p) else { continue };
        for s in settlers.iter().filter(|s| s.role == 5 && s.kind == 4 && s.list_id == 236) {
            let t = &s.role_tail;
            // role_fields: +4 task, +6 counter, +36/+40 offset (·10⁶); tail: good, on_entity, +48, +52 step.
            hist.entry((s.list_id, s.cursor.unwrap_or(0), s.counter() as i64, s.role_fields[14] / 1000, s.role_fields[15] / 1000, t[3] / 1000, t[4] / 1000)).and_modify(|v| *v += 1).or_insert(1);
        }
    }
    for (k, v) in hist.iter().filter(|(k, _)| k.1 <= 12) {
        println!("{v:5} cur {:2} counter {:3} offset ({:8}, {:8}) step ({:8}, {:8})", k.1, k.2, k.3, k.4, k.5, k.6);
    }
}
