//! Prints the rule data the magic system reads: temple infos, priest / servant roles and lists.
fn main() {
    let r = s4_sim::Rules::load_default().expect("rules");
    for race in 0..5u8 {
        for t in [43u8, 44] {
            let b = r.building(race, t);
            let w = r.work.info(race, t);
            println!("race {race} building {t}: house_kind {} info {:?}", b.house_kind, w);
        }
        for s in [45u16, 56, 54] {
            println!("race {race} settler {s}: role {} lists {:?}", r.jobs.settler_role(race, s), (0..4).map(|i| r.jobs.anim_list(race, s, i)).collect::<Vec<_>>());
        }
        for l in [0xc3u16, 0xfc, 45, 111, 56, 122, 223] {
            println!("race {race} list {l:#x}: {:?}", r.jobs.list(race, l).map(|v| v.iter().map(|t| (t.task, t.duration, t.frames, t.x, t.y, t.job_part, t.good)).collect::<Vec<_>>()));
        }
    }
}
