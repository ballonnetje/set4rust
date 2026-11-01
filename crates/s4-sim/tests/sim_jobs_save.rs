//! Savegames as oracle for the job lists: every settler in chunk 161 that runs a shared list
//! (list id != -1) must point into a list `JobData` has, and the saved animation values must be
//! the ones `InitCommonTaskValues` copies from that task. Skipped when no saves are present.

use s4_sim::Rules;
use s4_sim::jobs::save_settler::{self, SavedSettler};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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

fn save_dir() -> PathBuf {
    std::env::var_os("S4_SAVE_DIR").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(std::env::var_os("USERPROFILE").unwrap_or_default()).join(r"Documents\TheSettlers4\Save")
    })
}

#[test]
fn token_numbers() {
    use save_settler::parse_number as num;
    assert_eq!(num(b"42"), Some(42));
    assert_eq!(num(b"-1"), Some(-1));
    assert_eq!(num(b"-14.1666622161865234"), Some(-14_166_662));
    assert_eq!(num(b"0.5"), Some(500_000));
    assert_eq!(num(b"x"), None);
}

#[derive(Default, Debug)]
struct Stats {
    settlers: usize,
    own_list: usize,
    shared: usize,
    missing_list: usize,
    cursor_past_end: usize,
    at_end: usize,
    compared: usize,
    job_part_ok: usize,
    task_ok: usize,
    frames_ok: usize,
    /// Tasks the role did not rewrite (saved task == list task, not WAIT): job part and frame
    /// count must be exactly the list's.
    unchanged: usize,
    unchanged_job_part_ok: usize,
    unchanged_frames_ok: usize,
}

#[test]
fn saved_settlers_point_into_our_job_lists() {
    let mut files = Vec::new();
    saves(&save_dir(), &mut files);
    if files.is_empty() || !s4_formats::game_dir().join("GameData").exists() {
        eprintln!("skipping: no saves / no install");
        return;
    }
    files.sort();
    let rules = Rules::load_default().unwrap();
    let j = &rules.jobs;
    let mut st = Stats::default();
    let mut failed_records = 0;
    let mut mismatch: BTreeMap<String, usize> = BTreeMap::new();
    let mut examples: BTreeMap<String, String> = BTreeMap::new();
    for f in &files {
        let Ok((settlers, failed)) = save_settler::load(f) else { continue };
        failed_records += failed;
        for s in &settlers {
            st.settlers += 1;
            if s.list_id == -1 {
                st.own_list += 1;
                continue;
            }
            st.shared += 1;
            let Some(list) = j.list(s.race, s.list_id as u16) else {
                st.missing_list += 1;
                *mismatch.entry(format!("missing list r{} {}", s.race, s.list_id)).or_default() += 1;
                continue;
            };
            let cur = s.cursor.unwrap_or(0) as usize;
            if cur > list.len() {
                st.cursor_past_end += 1;
                let key = format!("cursor past end: role {} race {} list {} len {} cursor {cur}", s.role, s.race, s.list_id, list.len());
                *mismatch.entry(key.clone()).or_default() += 1;
                examples.entry(key).or_insert_with(|| f.file_name().unwrap().to_string_lossy().into_owned());
                continue;
            }
            if cur == list.len() {
                st.at_end += 1;
                continue;
            }
            let t = list[cur];
            st.compared += 1;
            let mut note = |what: &str, s: &SavedSettler, ours: String| {
                let key = format!("{what} role {} task {}→{}", s.role, t.task, s.task());
                *mismatch.entry(key.clone()).or_default() += 1;
                examples.entry(key).or_insert_with(|| {
                    format!("{} id {} kind {} race {} list {} cur {}: saved {:?} ours {}", f.file_name().unwrap().to_string_lossy(), s.id, s.kind, s.race, s.list_id, cur, s.role_fields, ours)
                });
            };
            if s.task() == t.task && t.task != 17 {
                st.unchanged += 1;
                st.unchanged_job_part_ok += (s.job_part == t.job_part) as usize;
                st.unchanged_frames_ok += (s.cycle_frames() == t.frames) as usize;
                if s.job_part != t.job_part || s.cycle_frames() != t.frames {
                    note("UNCHANGED TASK DIFFERS", s, format!("jp {} frames {}", t.job_part, t.frames));
                }
            }
            if s.job_part == t.job_part {
                st.job_part_ok += 1;
            } else {
                note("job_part", s, format!("{}", t.job_part));
            }
            if s.task() == t.task {
                st.task_ok += 1;
            } else {
                note("task", s, format!("{}", t.task));
            }
            if s.cycle_frames() == t.frames {
                st.frames_ok += 1;
            } else {
                note("frames", s, format!("{} (saved {})", t.frames, s.cycle_frames()));
            }
        }
    }
    eprintln!("{} saves, {failed_records} undecoded records\n{st:#?}", files.len());
    for (k, v) in &mismatch {
        eprintln!("{v:6}  {k}   e.g. {}", examples.get(k).map_or("", |s| s));
    }
    assert_eq!(failed_records, 0, "every chunk-161 record decodes");
    assert_eq!(st.missing_list, 0, "every saved shared list id exists in JobData for the settler's race");
    assert!(st.unchanged > 10_000);
    assert_eq!(st.unchanged_job_part_ok, st.unchanged, "job part of untouched tasks");
    assert_eq!(st.unchanged_frames_ok, st.unchanged, "frame count of untouched tasks");
    // Known: in one multiplayer series (H8GQE2M0_*) viking charcoal makers sit at position 13/14
    // of list 157, which has 10 nodes in the installed jobInfo.xml. Those saves are mod-tainted:
    // 121 of the 123 saves were played with the Settlers United "usd" balance data (loaded in
    // memory from BalanceConfigs.zip, SIM.md §10), whose list is longer. Rule-sensitive oracles
    // should use the 2 vanilla saves (Archiv-2026-01-20/rom2.sav, AutoSave.sav).
    let past_end: Vec<&String> = mismatch.keys().filter(|k| k.starts_with("cursor past end")).collect();
    assert!(past_end.iter().all(|k| k.contains("race 1 list 157 len 10")), "{past_end:?}");
}

/// Carriers (`CCarrierRole`, role 1, store version 3: good, next type, source pile, destination
/// pile, source piles 2/3, idle count, idle threshold) in every save:
/// * the idle threshold is always `Rand() % 300 + 300` and the counter is below it;
/// * on JOB_CARRIER_TRANSPORT_GOOD the good is 0 before GET_GOOD and set after it, and while
///   walking to the destination pile the shown gfx job is the list's job plus the good
///   (`CCarrierRole::TakeJob` GO_TO_DESTINATION_PILE: `SetJobPart(JobPart() + m_uGood)`);
/// * on the idle lists 154..156 the carrier is on the WORK (cursor 0) or the WAIT (cursor 1).
#[test]
fn saved_carriers_follow_the_carrier_role() {
    let mut files = Vec::new();
    saves(&save_dir(), &mut files);
    if files.is_empty() || !s4_formats::game_dir().join("GameData").exists() {
        eprintln!("skipping: no saves / no install");
        return;
    }
    let rules = Rules::load_default().unwrap();
    let transport = s4_sim::jobs::carrier::LIST_TRANSPORT_GOOD;
    let mut n = BTreeMap::<&str, usize>::new();
    let mut bad = Vec::new();
    for f in &files {
        let Ok((settlers, _)) = save_settler::load(f) else { continue };
        for s in settlers.iter().filter(|s| s.role == 1) {
            let t = &s.role_tail;
            *n.entry("carriers").or_default() += 1;
            if t.len() < 9 || t[0] != 3 {
                bad.push(format!("{}: carrier {} tail {:?}", f.display(), s.id, &t[..t.len().min(9)]));
                continue;
            }
            let (good, idle_count, idle_after) = (t[1], t[7], t[8]);
            if !(300..600).contains(&idle_after) || idle_count > idle_after.max(1) {
                bad.push(format!("{}: carrier {} idle {idle_count}/{idle_after}", f.display(), s.id));
            }
            *n.entry("idle ok").or_default() += 1;
            let cur = s.cursor.unwrap_or(0) as usize;
            if s.list_id == transport as i16 {
                let list = rules.jobs.list(s.race, transport).unwrap();
                let task = list.get(cur).map_or(0, |t| t.task);
                match task {
                    18 if good != 0 => bad.push(format!("{}: carrier {} has good {good} before GET_GOOD", f.display(), s.id)),
                    // PUT_GOOD clears the good half way through the task.
                    19 if good == 0 => bad.push(format!("{}: carrier {} task {} has no good on the way to the destination", f.display(), s.id, s.task())),
                    19 if s.task() == 6 => {
                        *n.entry("carry gfx checked").or_default() += 1;
                        if s.job_part as i64 != list[cur].job_part as i64 + good {
                            bad.push(format!("{}: carrier {} jp {} != {} + {good}", f.display(), s.id, s.job_part, list[cur].job_part));
                        }
                    }
                    _ => {}
                }
                *n.entry("on transport list").or_default() += 1;
            }
            if (154..=156).contains(&(s.list_id as u16)) {
                *n.entry("on idle list").or_default() += 1;
                if !(cur == 0 || (cur == 1 && s.task() == 17)) {
                    bad.push(format!("{}: carrier {} idle list {} cursor {cur} task {}", f.display(), s.id, s.list_id, s.task()));
                }
            }
        }
    }
    eprintln!("{n:?}");
    for b in bad.iter().take(20) {
        eprintln!("{b}");
    }
    assert!(n.get("carry gfx checked").copied().unwrap_or(0) > 1000);
    assert!(bad.is_empty(), "{} carrier records disagree", bad.len());
}

/// Diggers (role 2) and builders (role 3) against `CDiggerRole` / `CBuilderRole`, with the
/// save's node flags (chunk 205) and heights (render map, chunk 200) for the digging state.
#[test]
fn saved_diggers_and_builders_follow_their_roles() {
    // CDiggerRole: event 1 → list 68 (GO_TO_TARGET, CHECKIN), check-in → list 161 (JOB_DIGGER);
    // CBuilderRole: event 1 → list 149 (JOB_BUILDER).
    const LIST_COME_TO_WORK: i16 = 68;
    const LIST_DIG: i16 = 161;
    const LIST_BUILD: i16 = 149;
    let mut files = Vec::new();
    saves(&save_dir(), &mut files);
    if files.is_empty() {
        eprintln!("skipping: no saves");
        return;
    }
    let mut n = BTreeMap::<&str, usize>::new();
    let mut bad = Vec::new();
    for f in &files {
        let Ok(sg) = s4_sim::save::SaveGame::open(f) else { continue };
        let Ok((settlers, _)) = save_settler::load(f) else { continue };
        let size = sg.size as i32;
        let node = |p: (i32, i32)| (p.1 * size + p.0) as usize;
        let unpack = |v: i64| ((v & 0xFFFF) as i32, ((v >> 16) & 0xFFFF) as i32);
        let building_pos = |id: u16| sg.buildings.iter().find(|b| b.id == id).map(|b| (b.pos.x(), b.pos.y()));
        for s in settlers.iter().filter(|s| s.role == 2 || s.role == 3) {
            let t = &s.role_tail;
            let mut fail = |what: String| bad.push(format!("{}: {} {} {what} events {:?}", f.display(), if s.role == 2 { "digger" } else { "builder" }, s.id, s.event_list));
            if t.len() < 3 || t[0] != 1 {
                fail(format!("tail {:?}", &t[..t.len().min(3)]));
                continue;
            }
            let cur = s.cursor.unwrap_or(0);
            let dest = unpack(s.role_fields[10]);
            let home = s.home();
            // A site that is finished or torn down detaches its workers (home 0) and queues event 9
            // (BuildingDone) or 7/9 (TidyUp); the worker acts on it at its next update.
            let cancelled = s.event_list.iter().any(|e| e[0] == 7 || e[0] == 9);
            if cancelled {
                *n.entry("cancel event queued").or_default() += 1;
            }
            if s.role == 2 {
                *n.entry("diggers").or_default() += 1;
                let (target, working) = (t[1], t[2]);
                match (s.list_id, cur) {
                    (2, _) => {}
                    (l, _) if l == LIST_COME_TO_WORK => {
                        *n.entry("digger coming").or_default() += 1;
                        if !cancelled && (home == 0 || building_pos(home) != Some(dest)) {
                            fail(format!("list 68 home {home} dest {dest:?} site {:?}", building_pos(home)));
                        }
                    }
                    (l, 0) if l == LIST_DIG => {
                        *n.entry("digger walking to a node").or_default() += 1;
                        if s.task() != 6 || sg.flags[node(dest)] & 0x20 == 0 {
                            fail(format!("GO_TO_POS task {} dest {dest:?} flags {:#x}", s.task(), sg.flags[node(dest)]));
                        }
                    }
                    (l, 1) if l == LIST_DIG => {
                        *n.entry("digger digging").or_default() += 1;
                        // The node was picked by SearchDig, or it is the site's own node (the
                        // first WORK after check-in, at the site position from event 1).
                        let picked = sg.flags[node(s.pos)] & 0x20 != 0;
                        if s.task() != 16 || working != 1 || target == 0 || !(picked || building_pos(home) == Some(s.pos)) {
                            fail(format!("WORK task {} working {working} target {target} picked {picked} pos {:?} site {:?}", s.task(), s.pos, building_pos(home)));
                        }
                        if sg.render[4 * node(s.pos)] as i64 == target {
                            *n.entry("digger at target (last step)").or_default() += 1;
                        }
                    }
                    other => fail(format!("list/cursor {other:?}")),
                }
            } else {
                *n.entry("builders").or_default() += 1;
                let (dir, at_work) = (t[1], t[2]);
                if !(0..6).contains(&dir) || !(0..2).contains(&at_work) {
                    fail(format!("dir {dir} at_work {at_work}"));
                }
                match (s.list_id, cur) {
                    (3, _) => {
                        if s.task() == 6 {
                            // SetFree after work: stepping aside (walk to the free node at +28).
                            *n.entry("builder stepping aside").or_default() += 1;
                            if at_work != 1 {
                                fail(format!("walks on the wait list with at_work {at_work}"));
                            }
                        }
                    }
                    (l, 0) if l == LIST_BUILD => {
                        *n.entry("builder walking to its position").or_default() += 1;
                        if (home == 0 && !cancelled) || at_work != 0 {
                            fail(format!("GO_TO_POS home {home} at_work {at_work}"));
                        }
                    }
                    (l, 1) if l == LIST_BUILD => {
                        *n.entry("builder building").or_default() += 1;
                        if s.task() != 16 || at_work != 1 || s.direction as i64 != dir || s.pos != dest || (home == 0 && !cancelled) {
                            fail(format!("WORK task {} at_work {at_work} facing {} want {dir} pos {:?} dest {dest:?}", s.task(), s.direction, s.pos));
                        }
                    }
                    other => fail(format!("list/cursor {other:?}")),
                }
            }
        }
    }
    eprintln!("{n:?}");
    for b in bad.iter().take(20) {
        eprintln!("{b}");
    }
    assert!(n.get("digger digging").copied().unwrap_or(0) > 100);
    assert!(n.get("builder building").copied().unwrap_or(0) > 100);
    assert!(bad.is_empty(), "{} digger/builder records disagree", bad.len());
}

/// Woodcutters (type 4) and stonecutters (type 5), `CFreeWorkerRole`, on their work list against
/// the role: the good carried per list position, the work flag on the target, and the in-node
/// pixel offset after each virtual walk, which checks the `GO_VIRTUAL` target formula.
#[test]
fn saved_cutters_follow_the_free_worker_role() {
    let mut files = Vec::new();
    saves(&save_dir(), &mut files);
    if files.is_empty() || !s4_formats::game_dir().join("GameData").exists() {
        eprintln!("skipping: no saves / no install");
        return;
    }
    let rules = Rules::load_default().unwrap();
    let mut n = BTreeMap::<String, usize>::new();
    let mut bad = Vec::new();
    for f in &files {
        let Ok(sg) = s4_sim::save::SaveGame::open(f) else { continue };
        let Ok((settlers, _)) = save_settler::load(f) else { continue };
        let size = sg.size as i32;
        for s in settlers.iter().filter(|s| s.role == 5 && matches!(s.kind, 4 | 5 | 7)) {
            let (name, product) = match s.kind {
                4 => ("woodcutter", 22),
                5 => ("stonecutter", 32),
                _ => ("grain farmer", 16),
            };
            *n.entry(format!("{name}s")).or_default() += 1;
            let t = &s.role_tail;
            if t.len() < 6 || t[0] != 1 {
                bad.push(format!("{}: {name} {} tail {:?}", f.display(), s.id, &t[..t.len().min(6)]));
                continue;
            }
            // Tail: +44 good, +45 on entity, +48/+52 step; the offset +36/+40 is ISettlerRole's.
            let (good, on_entity) = (t[1], t[2]);
            let (ox, oy) = (s.role_fields[14], s.role_fields[15]);
            let Some(work) = rules.jobs.anim_list(s.race, s.kind, 1) else { continue };
            if s.list_id != work as i16 {
                continue;
            }
            let list = rules.jobs.list(s.race, work).unwrap();
            let cur = s.cursor.unwrap_or(0) as usize;
            let task = list.get(cur).map_or(0, |t| t.task);
            *n.entry(format!("{name} cursor {cur:2} task {task:2} good {good}")).or_default() += 1;
            // The log is carried from RESOURCE_GATHERING's end until PUT_GOOD lets it go.
            let put = |t: u8| t == 21 || t == 4;
            let gather = |t: u8| t == 13 || t == 30;
            let carrying = list[..cur].iter().any(|t| gather(t.task)) && !list[..cur].iter().any(|t| put(t.task));
            let putting = put(task) || gather(task);
            if !putting && carrying != (good == product) {
                bad.push(format!("{}: {name} {} cursor {cur} task {task} good {good}", f.display(), s.id));
            }
            // The gather huts mark the target node (0x20); the farm role does not (release
            // 0x5464a0 sends event 5 without it), which the farmer records confirm.
            if cur == 0 && s.kind != 7 {
                let d = s.role_fields[10];
                let node = ((d >> 16) as i32 * size + (d & 0xFFFF) as i32) as usize;
                *n.entry(format!("{name} walking to a marked target")).or_default() += 1;
                if sg.flags.get(node).is_none_or(|f| f & 0x20 == 0) {
                    bad.push(format!("{}: {name} {} target not marked", f.display(), s.id));
                }
            }
            if task == 28 || task == 29 {
                *n.entry(format!("{name} working on the entity")).or_default() += 1;
                if on_entity != 1 {
                    bad.push(format!("{}: {name} {} works with on_entity {on_entity}", f.display(), s.id));
                }
            }
            // Right after a GO_VIRTUAL (the task before is one, the current one is not): the
            // offset is the walk's target, (255x − 127.5y − 127.5, 255y − 255) in pixels, up to
            // the original's float drift (a few pixels after several walks).
            if cur > 0 && task != 0 && list[cur - 1].task == 0 {
                let p = &list[cur - 1];
                let (tx, ty) = ((255 * p.x as i64 - 127 * p.y as i64) * 1_000_000 - 500_000 * p.y as i64 - 127_500_000, (255 * p.y as i64 - 255) * 1_000_000);
                *n.entry("offset after a virtual walk".into()).or_default() += 1;
                if (ox - tx).abs() > 4_000_000 || (oy - ty).abs() > 4_000_000 {
                    bad.push(format!("{}: woodcutter {} cursor {cur} offset ({ox}, {oy}) want ({tx}, {ty})", f.display(), s.id));
                }
            }
        }
    }
    for (k, v) in &n {
        eprintln!("{v:7} {k}");
    }
    for b in bad.iter().take(20) {
        eprintln!("{b}");
    }
    assert!(n.get("offset after a virtual walk").copied().unwrap_or(0) > 100);
    assert!(bad.is_empty(), "{} woodcutter/stonecutter/farmer records disagree", bad.len());
}

/// Foresters (type 6, `CFreeWorkerRole` on JOB_FORESTER_PLANT): the tree to plant (+46) is set
/// until PLANT is done and is one `CDecoObjMgr::GetObjectType` can draw for the race; while the
/// forester walks out, the planting node (destination − the seed search's offset) already has
/// the tree's rings reserved (`SetFlagsForObject`: flag 0x40).
#[test]
fn saved_planters_follow_the_free_worker_role() {
    let mut files = Vec::new();
    saves(&save_dir(), &mut files);
    if files.is_empty() || !s4_formats::game_dir().join("GameData").exists() {
        eprintln!("skipping: no saves / no install");
        return;
    }
    let rules = Rules::load_default().unwrap();
    let mut n = BTreeMap::<String, usize>::new();
    let mut bad = Vec::new();
    for f in &files {
        let Ok(sg) = s4_sim::save::SaveGame::open(f) else { continue };
        let Ok((settlers, _)) = save_settler::load(f) else { continue };
        let size = sg.size as i32;
        for s in settlers.iter().filter(|s| s.role == 5 && (s.kind == 6 || s.kind == 7)) {
            let name = if s.kind == 6 { "forester" } else { "grain farmer" };
            *n.entry(format!("{name}s")).or_default() += 1;
            let t = &s.role_tail;
            if t.len() < 6 || t[0] != 1 {
                bad.push(format!("{}: {name} {} tail {:?}", f.display(), s.id, &t[..t.len().min(6)]));
                continue;
            }
            let plant = t[5];
            let Some(list_id) = rules.jobs.anim_list(s.race, s.kind, 2) else { continue };
            if s.list_id != list_id as i16 {
                continue;
            }
            let list = rules.jobs.list(s.race, list_id).unwrap();
            let cur = s.cursor.unwrap_or(0) as usize;
            let planting_done = list[..cur].iter().any(|t| t.task == 25);
            let allowed: &[i64] = match (s.kind, s.race) {
                (7, _) => &[209, 210],
                (_, 2) => &[11, 12, 13, 14],
                (_, 4) => &[11, 12, 15, 16, 17, 18],
                _ => &[9, 10],
            };
            *n.entry(format!("{name} cursor {cur} plant {}", if plant == 0 { "none" } else { "set" })).or_default() += 1;
            if planting_done != (plant == 0) || (plant != 0 && !allowed.contains(&plant)) {
                bad.push(format!("{}: {name} {} race {} cursor {cur} plant {plant}", f.display(), s.id, s.race));
            }
            if cur == 0 {
                let d = s.role_fields[10];
                // The planting node: destination − the seed search's offset (forester (−1, −1),
                // grain (0, 0)).
                let (ox, oy) = if s.kind == 6 { (-1, -1) } else { (0, 0) };
                let (x, y) = ((d & 0xFFFF) as i32 - ox, (d >> 16) as i32 - oy);
                *n.entry(format!("{name} walking to a reserved spot")).or_default() += 1;
                if sg.flags.get((y * size + x) as usize).is_none_or(|f| f & 0x40 == 0) {
                    bad.push(format!("{}: {name} {} planting node ({x},{y}) not reserved", f.display(), s.id));
                }
            }
        }
    }
    for (k, v) in &n {
        eprintln!("{v:7} {k}");
    }
    for b in bad.iter().take(20) {
        eprintln!("{b}");
    }
    assert!(n.get("forester walking to a reserved spot").copied().unwrap_or(0) > 50);
    assert!(n.get("grain farmer walking to a reserved spot").copied().unwrap_or(0) > 10);
    assert!(bad.is_empty(), "{} forester / farmer records disagree", bad.len());
}

/// Hunters (type 11, `CHunterRole` 0x105bb18, role 8; tail: version, +0x2c the good) against the
/// role: the meat is carried from the harvest to the delivery (and only then), the target stays
/// set until the harvest, and a shot's target is a huntable animal of chunk 165 within reach.
#[test]
fn saved_hunters_follow_the_hunter_role() {
    let mut files = Vec::new();
    saves(&save_dir(), &mut files);
    if files.is_empty() || !s4_formats::game_dir().join("GameData").exists() {
        eprintln!("skipping: no saves / no install");
        return;
    }
    let rules = Rules::load_default().unwrap();
    let animal_rules = s4_sim::animals::AnimalRules::load_default();
    let mut n = BTreeMap::<String, usize>::new();
    let mut bad = Vec::new();
    for f in &files {
        let Ok((settlers, _)) = save_settler::load(f) else { continue };
        let animals = s4_formats::map::MapFile::open(f)
            .ok()
            .and_then(|m| m.container.load_id(165, 0).ok().flatten())
            .and_then(|(_, d)| s4_sim::animals::save::parse(&d))
            .unwrap_or_default();
        for s in settlers.iter().filter(|s| s.kind == 11) {
            *n.entry("hunters".into()).or_default() += 1;
            if s.role != 8 {
                bad.push(format!("{}: hunter {} role {}", f.display(), s.id, s.role));
                continue;
            }
            let t = &s.role_tail;
            let good = t.get(1).copied().unwrap_or(-1);
            let entity = s.role_fields[13] as u16;
            let (hunt, harvest) = (rules.jobs.anim_list(s.race, 11, 1), rules.jobs.anim_list(s.race, 11, 2));
            let cur = s.cursor.unwrap_or(0) as usize;
            let task = s.task();
            *n.entry(format!("list {} cursor {cur} task {task} good {good} target {}", s.list_id, entity != 0)).or_default() += 1;
            let mut fail = |what: &str| bad.push(format!("{}: hunter {} list {} cursor {cur} task {task} good {good} entity {entity}: {what}", f.display(), s.id, s.list_id));
            if good != 0 && good != 24 {
                fail("good");
            }
            if Some(s.list_id as u16) == harvest {
                // Harvest list: GO_TO_TARGET, RESOURCE_GATHERING, GO_TO_DESTINATION_PILE,
                // PUT_GOOD (either), GO_HOME, CHECKIN.
                let carrying = match cur {
                    2 => Some(true),
                    3 => None,
                    _ => Some(false),
                };
                if carrying.is_some_and(|c| c != (good == 24)) {
                    fail("meat carried outside harvest → delivery");
                }
                if cur >= 2 && entity != 0 {
                    fail("target kept after the harvest");
                }
                if cur < 2 && entity == 0 {
                    fail("no target before the harvest");
                }
            }
            if Some(s.list_id as u16) == hunt {
                if good != 0 {
                    fail("meat carried on a hunt");
                }
                if entity == 0 {
                    fail("hunting without a target");
                }
                if task == 28 {
                    *n.entry("shooting".into()).or_default() += 1;
                    match animals.iter().find(|a| a.id == entity) {
                        None => fail("shooting at no animal"),
                        Some(a) => {
                            *n.entry("shooting at a saved animal".into()).or_default() += 1;
                            if !animal_rules.types.get(a.kind as usize).copied().flatten().is_some_and(|c| c.huntable) {
                                fail("shooting at a type that is not huntable");
                            }
                            let d = s4_sim::spiral::grid_distance(a.pos.x() - s.pos.0, a.pos.y() - s.pos.1);
                            // Shot from within 12; the animal may have stepped on since.
                            if d >= 12 + 3 {
                                fail(&format!("shooting from distance {d}"));
                            }
                        }
                    }
                }
            }
        }
    }
    for (k, v) in &n {
        eprintln!("{v:7} {k}");
    }
    for b in bad.iter().take(20) {
        eprintln!("{b}");
    }
    assert!(n.get("hunters").copied().unwrap_or(0) > 0);
    assert!(bad.is_empty(), "{} hunter records disagree", bad.len());
}

/// Donkeys (type 60, `CDonkeyRole` 0x105ae90, role 20; tail: version 5, …, +0x30 the settler id
/// at index 5) without an order: on the wait list (60) in WAIT, with the flags of `Init`
/// (0x576300: 0x200, 0x2000, 0x10000, 0x20000 off, 0x4000 on).
#[test]
fn saved_idle_donkeys_follow_the_donkey_role() {
    let mut files = Vec::new();
    saves(&save_dir(), &mut files);
    if files.is_empty() {
        eprintln!("skipping: no saves");
        return;
    }
    let mut n = BTreeMap::<String, usize>::new();
    let mut bad = Vec::new();
    for f in &files {
        let Ok((settlers, _)) = save_settler::load(f) else { continue };
        for s in settlers.iter().filter(|s| s.kind == 60) {
            *n.entry(format!("donkeys on list {}", s.list_id)).or_default() += 1;
            let mut fail = |what: &str| bad.push(format!("{}: donkey {} list {} task {}: {what}", f.display(), s.id, s.list_id, s.task()));
            if s.role != 20 {
                fail("role");
                continue;
            }
            if s.role_tail.first() != Some(&5) || s.role_tail.get(5) != Some(&(s.id as i64)) {
                fail("tail: version / +0x30");
            }
            if s.list_id == 60 {
                if s.task() != 17 {
                    fail("not waiting on the wait list");
                }
                if s.flags & 0x3_2200 != 0 || s.flags & 0x4000 == 0 {
                    fail("Init flags");
                }
            }
        }
    }
    for (k, v) in &n {
        eprintln!("{v:7} {k}");
    }
    for b in bad.iter().take(20) {
        eprintln!("{b}");
    }
    assert!(n.get("donkeys on list 60").copied().unwrap_or(0) > 0);
    assert!(bad.is_empty(), "{} donkey records disagree", bad.len());
}
