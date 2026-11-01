//! Prints rule tables that size the deco instance pools (`CDecoObjMgr::Create`).
fn main() {
    let r = s4_sim::Rules::load_default().expect("rules");
    for t in 0..s4_sim::rules::OBJECT_MAX as u16 {
        let f: Vec<u8> = (0..4).map(|k| r.object_frames(t, k)).collect();
        if f.iter().any(|&v| v > 24) {
            println!("object {t}: frames of first..first+3 = {f:?}");
        }
    }
}
