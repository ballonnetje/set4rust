//! Validation and progress, from the pack listing only: sizes come from the PNG headers.

use std::cmp::Ordering;

use crate::model::{F_RAW, F_TEAM, Group, Kind, Seq, Slot, SlotRef, Spec};
use crate::pack::{Conf, Pack, expect_size, orig_of, seq_dir, seq_mult, sound_path, valid_mult};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Error,
    Warn,
    Info,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Problem {
    pub level: Level,
    pub msg: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SeqStatus {
    pub need: usize,
    pub have: usize,
    pub probs: Vec<Problem>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Missing,
    Partial,
    Done,
    Problem,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotStatus {
    pub need: usize,
    pub have: usize,
    pub state: State,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GroupStatus {
    pub need: usize,
    pub have: usize,
    pub slots: usize,
    pub done: usize,
    pub problem: usize,
}

pub fn plural(n: usize, w: &str) -> String {
    format!("{n} {w}{}", if n == 1 { "" } else { "s" })
}

fn list_frames(items: &[String], max: usize) -> String {
    if items.len() <= max { items.join(", ") } else { format!("{} and {} more", items[..max].join(", "), items.len() - max) }
}

/// Validation of one sequence, given its settings (the editor's unsaved ones win over the stored).
pub fn seq_status(pack: &Pack, g: &Group, slot: &Slot, seq: &Seq, conf: &Conf) -> SeqStatus {
    let mut probs = Vec::new();
    match g.kind {
        Kind::Sound => {
            if seq.missing {
                return SeqStatus::default();
            }
            return SeqStatus { need: 1, have: pack.has(&sound_path(slot, seq)) as usize, probs };
        }
        Kind::Music => return SeqStatus { need: 1, have: pack.music_file(slot).is_some() as usize, probs },
        Kind::Gfx => {}
    }
    let dir = seq_dir(g, slot, seq);
    let names = pack.names_in(&dir);
    let n = seq.frames.len();
    let k = seq_mult(pack, slot, seq, conf);
    let ku = k as usize;
    let need = n * ku;
    if names.is_empty() {
        return SeqStatus { need, have: 0, probs };
    }
    let scale = pack.scale();
    let mut push = |level, msg: String| probs.push(Problem { level, msg });
    if conf.bad {
        push(Level::Error, "seq.json is not valid JSON.".to_string());
    }
    if let Some(v) = conf.bad_multiplier() {
        push(Level::Error, format!("seq.json frame_multiplier {v}: must be a whole number 1..8."));
    }
    let mut have = 0;
    let (mut wrong_exact, mut other_size, mut no_mask, mut bad_mask) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for i in 0..need {
        let fr = orig_of(seq, k, i);
        let Some(rec) = pack.file(&format!("{dir}/{i:03}.png")) else { continue };
        have += 1;
        let [ew, eh] = expect_size(fr, scale);
        let exact = slot.exact() || fr[5] & F_RAW != 0;
        if rec.w as i64 != ew || rec.h as i64 != eh {
            let what = format!("{i} ({}×{}, {ew}×{eh} expected)", rec.w, rec.h);
            if exact { wrong_exact.push(what) } else { other_size.push(what) }
        }
        match pack.file(&format!("{dir}/{i:03}.team.png")) {
            Some(m) if m.w != rec.w || m.h != rec.h => bad_mask.push(i.to_string()),
            None if fr[5] & F_TEAM != 0 => no_mask.push(i.to_string()),
            _ => {}
        }
    }
    let frame_no = |f: &str| -> Option<usize> {
        let digits = f.strip_suffix(".png")?;
        (digits.len() == 3 && digits.bytes().all(|b| b.is_ascii_digit())).then(|| digits.parse().ok())?
    };
    let extra: Vec<String> = names.iter().filter(|f| frame_no(f).is_some_and(|x| x >= need)).cloned().collect();
    let counted = if k > 1 { format!("the original's {n} × {k} = {need}") } else { format!("the original's {need}") };
    if !extra.is_empty() {
        // The highest frame present tells whether another whole multiple was meant.
        let top = extra.iter().filter_map(|f| frame_no(f)).max().unwrap_or(0) + 1;
        let hint = if top % n == 0 && valid_mult(&((top / n) as u64).into()).is_some() {
            format!(" {top} frames would be {}× the original: set \"Frames per original frame\" to {}.", top / n, top / n)
        } else {
            format!(" Use a whole multiple of {n}.")
        };
        let keep = g.keep_frames.as_ref().map_or(String::new(), |why| {
            let shown = if k > 1 { format!("{k} of your frames per original frame") } else { "one of your frames per original frame".to_string() };
            format!(" Frame counts here are mechanics ({why}); the game keeps the original count and shows {shown}.")
        });
        push(if g.keep_frames.is_some() { Level::Error } else { Level::Warn }, format!("{} beyond {counted} ({}).{hint}{keep}", plural(extra.len(), "frame"), list_frames(&extra, 4)));
    }
    if !wrong_exact.is_empty() {
        push(Level::Error, format!("Must be exactly the original size × {scale}: frame {}.", list_frames(&wrong_exact, 3)));
    }
    if !other_size.is_empty() && !conf.has_anchor() {
        push(Level::Warn, format!("No anchor set and the canvas differs from the original box × {scale}: frame {}. The anchor is then assumed at the original (left, top) × scale; set it below if your canvas differs.", list_frames(&other_size, 3)));
    }
    if !bad_mask.is_empty() {
        push(Level::Error, format!("Team mask size differs from its frame: {}.", list_frames(&bad_mask, 4)));
    }
    if !no_mask.is_empty() {
        push(Level::Warn, format!("The original shows team colour but there is no .team.png mask: frame {}.", list_frames(&no_mask, 4)));
    }
    if have > 0 && have < need {
        let missing: Vec<String> = (0..need).filter(|i| !pack.has(&format!("{dir}/{i:03}.png"))).map(|i| format!("{i:03}")).collect();
        push(Level::Info, format!("Missing frame {}.", list_frames(&missing, 6)));
    }
    SeqStatus { need, have, probs }
}

pub fn slot_status(pack: &Pack, g: &Group, slot: &Slot) -> SlotStatus {
    let (mut need, mut have, mut err) = (0, 0, false);
    for seq in &slot.seqs {
        let s = seq_status(pack, g, slot, seq, pack.conf(g, slot, seq));
        need += s.need;
        have += s.have;
        err |= s.probs.iter().any(|p| p.level == Level::Error);
    }
    let state = if err {
        State::Problem
    } else if have == 0 {
        State::Missing
    } else if have >= need {
        State::Done
    } else {
        State::Partial
    };
    SlotStatus { need, have, state }
}

/// Progress of the whole spec against one pack.
#[derive(Clone, Debug, Default)]
pub struct Statuses {
    pub slots: Vec<Vec<SlotStatus>>,
    pub groups: Vec<GroupStatus>,
    pub need: usize,
    pub have: usize,
}

impl Statuses {
    pub fn compute(spec: &Spec, pack: &Pack) -> Statuses {
        let mut out = Statuses::default();
        for g in &spec.groups {
            let mut gs = GroupStatus { slots: g.slots.len(), ..GroupStatus::default() };
            let mut row = Vec::with_capacity(g.slots.len());
            for slot in &g.slots {
                let s = slot_status(pack, g, slot);
                gs.need += s.need;
                gs.have += s.have;
                gs.done += (s.state == State::Done) as usize;
                gs.problem += (s.state == State::Problem) as usize;
                row.push(s);
            }
            out.need += gs.need;
            out.have += gs.have;
            out.groups.push(gs);
            out.slots.push(row);
        }
        out
    }

    pub fn slot(&self, r: SlotRef) -> SlotStatus {
        self.slots[r.g][r.s]
    }

    /// One slot's files changed: much cheaper than [`Statuses::compute`] on a full pack.
    pub fn update_slot(&mut self, spec: &Spec, pack: &Pack, r: SlotRef) {
        let (g, slot) = spec.slot(r);
        let new = slot_status(pack, g, slot);
        let old = std::mem::replace(&mut self.slots[r.g][r.s], new);
        let gs = &mut self.groups[r.g];
        gs.need = gs.need - old.need + new.need;
        gs.have = gs.have - old.have + new.have;
        gs.done = gs.done - (old.state == State::Done) as usize + (new.state == State::Done) as usize;
        gs.problem = gs.problem - (old.state == State::Problem) as usize + (new.state == State::Problem) as usize;
        self.need = self.need - old.need + new.need;
        self.have = self.have - old.have + new.have;
    }

    /// `"1,234 / 5,678 replaced (21.7 %)"`.
    pub fn total_label(&self) -> String {
        let pct = if self.need > 0 { 100.0 * self.have as f64 / self.need as f64 } else { 0.0 };
        format!("{} / {} replaced ({pct:.1} %)", thousands(self.have), thousands(self.need))
    }
}

/// 1234567 as "1,234,567".
pub fn thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Sidebar filter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    All,
    Todo,
    Problem,
    Done,
}

/// Whether a slot passes the filter and the (lower case) search text.
pub fn matches(status: SlotStatus, slot: &Slot, filter: Filter, search: &str) -> bool {
    match filter {
        Filter::Todo if status.state == State::Done => return false,
        Filter::Problem if status.state != State::Problem => return false,
        Filter::Done if status.state != State::Done => return false,
        _ => {}
    }
    if search.is_empty() {
        return true;
    }
    [slot.id.as_str(), slot.name.as_str(), slot.owner.as_deref().unwrap_or("")].iter().any(|s| s.to_lowercase().contains(search))
}

/// Name order: case-insensitive, digit runs by value (`frame2` before `frame10`).
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(p), Some(q)) if p.is_ascii_digit() && q.is_ascii_digit() => {
                let run = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut s = String::new();
                    while let Some(c) = it.next_if(char::is_ascii_digit) {
                        s.push(c);
                    }
                    s
                };
                let (s, t) = (run(&mut x), run(&mut y));
                let (s, t) = (s.trim_start_matches('0'), t.trim_start_matches('0'));
                match s.len().cmp(&t.len()).then_with(|| s.cmp(t)) {
                    Ordering::Equal => {}
                    o => return o,
                }
            }
            (Some(p), Some(q)) => {
                match p.to_lowercase().cmp(q.to_lowercase()) {
                    Ordering::Equal => {}
                    o => return o,
                }
                x.next();
                y.next();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::test_spec;
    use crate::pack::tests::{png_bytes, temp};
    use crate::pack::{conf_path, frame_path, mask_path};

    fn setup(name: &str) -> (std::path::PathBuf, Pack, Spec) {
        let d = temp(name);
        let p = Pack::open(&d).unwrap();
        let s = test_spec(&d);
        (d, p, s)
    }

    fn msgs(s: &SeqStatus, level: Level) -> Vec<&str> {
        s.probs.iter().filter(|p| p.level == level).map(|p| p.msg.as_str()).collect()
    }

    #[test]
    fn no_files_means_no_problems() {
        let (d, p, s) = setup("v0");
        let st = Statuses::compute(&s, &p);
        assert_eq!((st.have, st.slots.len()), (0, 5));
        assert_eq!(st.slot(s.find("settlers-roman/JOB_X").unwrap()).state, State::Missing);
        // 1 + 1 + 3 + 4 + 4 + 1 frames, 1 sound (the empty variant is nothing to replace), 1 track.
        assert_eq!(st.need, 16);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_complete_sequence_with_masks_is_done() {
        let (d, mut p, s) = setup("v1");
        let r = s.find("settlers-roman/JOB_X").unwrap();
        let (g, slot) = s.slot(r);
        for q in &slot.seqs {
            for i in 0..4 {
                p.put(&frame_path(g, slot, q, i), &png_bytes(16, 24)).unwrap();
                if q.key == "0" {
                    p.put(&mask_path(g, slot, q, i), &png_bytes(16, 24)).unwrap();
                }
            }
        }
        let st = seq_status(&p, g, slot, &slot.seqs[0], p.conf(g, slot, &slot.seqs[0]));
        assert_eq!((st.need, st.have, st.probs.len()), (4, 4, 0), "{:?}", st.probs);
        assert_eq!(slot_status(&p, g, slot).state, State::Done);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn frame_counts_are_errors_where_they_are_mechanics() {
        let (d, mut p, s) = setup("v2");
        let (g, slot) = s.slot(s.find("settlers-roman/JOB_X").unwrap());
        let q = &slot.seqs[1];
        for i in 0..8 {
            p.put(&frame_path(g, slot, q, i), &png_bytes(16, 24)).unwrap();
        }
        let st = seq_status(&p, g, slot, q, p.conf(g, slot, q));
        let e = msgs(&st, Level::Error);
        assert_eq!(e.len(), 1, "{:?}", st.probs);
        assert!(e[0].starts_with("4 frames beyond the original's 4 (004.png, 005.png, 006.png, 007.png)."), "{}", e[0]);
        assert!(e[0].contains("8 frames would be 2× the original: set \"Frames per original frame\" to 2."), "{}", e[0]);
        assert!(e[0].contains("Frame counts here are mechanics"), "{}", e[0]);
        assert!(e[0].ends_with("shows one of your frames per original frame."), "{}", e[0]);
        assert_eq!(slot_status(&p, g, slot).state, State::Problem);
        // At 2× the same files are complete.
        p.put(&conf_path(g, slot, q), br#"{"frame_multiplier":2}"#).unwrap();
        let st = seq_status(&p, g, slot, q, p.conf(g, slot, q));
        assert_eq!((st.need, st.have, st.probs.len()), (8, 8, 0), "{:?}", st.probs);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_count_that_is_no_multiple_says_so() {
        let (d, mut p, s) = setup("v3");
        let (g, slot) = s.slot(s.find("settlers-roman/JOB_X").unwrap());
        let q = &slot.seqs[1];
        for i in [0, 6] {
            p.put(&frame_path(g, slot, q, i), &png_bytes(16, 24)).unwrap();
        }
        let st = seq_status(&p, g, slot, q, p.conf(g, slot, q));
        assert!(msgs(&st, Level::Error)[0].contains("Use a whole multiple of 4."));
        assert_eq!(msgs(&st, Level::Info), vec!["Missing frame 001, 002, 003."]);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn exact_sizes_and_masks_and_canvas() {
        let (d, mut p, s) = setup("v4");
        // GUI: exact size.
        let (g, slot) = s.slot(s.find("gui-roman/0001").unwrap());
        let q = &slot.seqs[0];
        p.put(&frame_path(g, slot, q, 0), &png_bytes(10, 10)).unwrap();
        let st = seq_status(&p, g, slot, q, p.conf(g, slot, q));
        assert_eq!(msgs(&st, Level::Error), vec!["Must be exactly the original size × 1: frame 0 (10×10, 64×32 expected)."]);
        // Settler: a canvas of its own warns only until an anchor is set; a team mask is expected.
        let (g, slot) = s.slot(s.find("settlers-roman/JOB_X").unwrap());
        let q = &slot.seqs[0];
        p.put(&frame_path(g, slot, q, 0), &png_bytes(20, 20)).unwrap();
        let st = seq_status(&p, g, slot, q, p.conf(g, slot, q));
        let w = msgs(&st, Level::Warn);
        assert!(w[0].starts_with("No anchor set and the canvas differs from the original box × 1: frame 0 (20×20, 16×24 expected)."), "{}", w[0]);
        assert_eq!(w[1], "The original shows team colour but there is no .team.png mask: frame 0.");
        p.put(&mask_path(g, slot, q, 0), &png_bytes(5, 5)).unwrap();
        p.put(&conf_path(g, slot, q), br#"{"anchor":[1,2]}"#).unwrap();
        let st = seq_status(&p, g, slot, q, p.conf(g, slot, q));
        assert_eq!(msgs(&st, Level::Error), vec!["Team mask size differs from its frame: 0."]);
        assert!(msgs(&st, Level::Warn).is_empty());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn broken_settings_are_errors() {
        let (d, mut p, s) = setup("v5");
        let (g, slot) = s.slot(s.find("settlers-roman/JOB_X").unwrap());
        let q = &slot.seqs[1];
        p.put(&conf_path(g, slot, q), b"{nope").unwrap();
        assert_eq!(msgs(&seq_status(&p, g, slot, q, p.conf(g, slot, q)), Level::Error), vec!["seq.json is not valid JSON."]);
        p.put(&conf_path(g, slot, q), br#"{"frame_multiplier":12}"#).unwrap();
        assert_eq!(msgs(&seq_status(&p, g, slot, q, p.conf(g, slot, q)), Level::Error), vec!["seq.json frame_multiplier 12: must be a whole number 1..8."]);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn scale_two_doubles_the_expected_size() {
        let (d, mut p, s) = setup("v6");
        p.set_meta("scale", 2.into()).unwrap();
        let (g, slot) = s.slot(s.find("gui-roman/0001").unwrap());
        let q = &slot.seqs[0];
        p.put(&frame_path(g, slot, q, 0), &png_bytes(128, 64)).unwrap();
        assert!(seq_status(&p, g, slot, q, p.conf(g, slot, q)).probs.is_empty());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn sound_and_music_count_files() {
        let (d, mut p, s) = setup("v7");
        let r = s.find("sounds/SND_STEP").unwrap();
        let (g, slot) = s.slot(r);
        assert_eq!(slot_status(&p, g, slot).need, 1, "the empty variant is not counted");
        p.put(&sound_path(slot, &slot.seqs[0]), b"RIFF").unwrap();
        assert_eq!(slot_status(&p, g, slot).state, State::Done);
        let (g, slot) = s.slot(s.find("music/track1").unwrap());
        p.put("music/track1.ogg", b"x").unwrap();
        assert_eq!(slot_status(&p, g, slot).state, State::Done);
        assert_eq!(p.music_file(slot).as_deref(), Some("music/track1.ogg"));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn filters_and_search() {
        let (d, p, s) = setup("v8");
        let r = s.find("settlers-roman/JOB_X").unwrap();
        let (_, slot) = s.slot(r);
        let st = SlotStatus { need: 1, have: 1, state: State::Done };
        assert!(!matches(st, slot, Filter::Todo, ""));
        assert!(matches(st, slot, Filter::Done, ""));
        assert!(!matches(st, slot, Filter::Problem, ""));
        assert!(matches(st, slot, Filter::All, "carrier"), "searches the owner");
        assert!(!matches(st, slot, Filter::All, "nope"));
        drop(p);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn one_slot_updates_like_the_whole() {
        let (d, mut p, s) = setup("v9");
        let r = s.find("settlers-roman/JOB_X").unwrap();
        let (g, slot) = s.slot(r);
        let mut st = Statuses::compute(&s, &p);
        for i in 0..4 {
            p.put(&frame_path(g, slot, &slot.seqs[1], i), &png_bytes(16, 24)).unwrap();
        }
        st.update_slot(&s, &p, r);
        let whole = Statuses::compute(&s, &p);
        assert_eq!((st.need, st.have, &st.groups, &st.slots), (whole.need, whole.have, &whole.groups, &whole.slots));
        p.put(&conf_path(g, slot, &slot.seqs[1]), b"{nope").unwrap();
        st.update_slot(&s, &p, r);
        let whole = Statuses::compute(&s, &p);
        assert_eq!((&st.groups, whole.groups[1].problem), (&whole.groups, 1));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn natural_order() {
        let mut v = vec!["Frame10.png", "frame2.png", "frame1.png", "a.png", "frame02b.png"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, vec!["a.png", "frame1.png", "frame2.png", "frame02b.png", "Frame10.png"]);
        assert_eq!(thousands(1234567), "1,234,567");
        assert_eq!(thousands(999), "999");
    }
}
