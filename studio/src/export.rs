//! Export of slots as a zip: your frames and masks, sprite sheets with atlases, per-slot data
//! (timing, anchors, events, building data, routines), CSV tables and your sounds. Only the pack's
//! own files and the spec's numbers go in; original images and sounds never do, except the original
//! frames as an opt-in reference. Saved data only: the editor's working copy is not looked at.

use std::collections::HashSet;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use anyhow::{Context, Result, bail};
use chrono::{Datelike, Timelike};
use serde_json::{Map, Value, json};

use crate::geometry::{cell_px, is_cell};
use crate::model::{BLD_KEYS, Building, F_HALF, F_SHADOW, F_TEAM, GAME_FPS, Group, Kind, Seq, Slot, SlotRef, Spec};
use crate::pack::{Pack, expect_size, frame_anchor, frame_path, mask_path, num, orig_of, seq_mult, sound_path};
use crate::pngio::{Rgba, decode_rgba, encode_rgba, parse_ramp, recolour};
use crate::status::plural;
use crate::zip::{ZipWriter, dos_time};

pub const EXPORT_FORMAT: &str = "s4studio-export/1";
pub const SHEET_W: u32 = 4096;
pub const SHEET_H: u32 = 8192;
/// One PNG per original frame: more than this does not fit one zip.
pub const MAX_REFERENCE_FRAMES: usize = 60000;

pub const REFERENCE_WARNING: &str = "reference/ folders hold the original game's frames, copyrighted, cut from your own install for your own reference. Do not publish them, put them in the pack, or give them to artists to trace or paint over: traced art is a derivative work and carries the copyright problem into the new game.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Slot,
    Group,
    Pack,
    All,
}

impl Scope {
    pub fn name(self) -> &'static str {
        match self {
            Scope::Slot => "slot",
            Scope::Group => "group",
            Scope::Pack => "pack",
            Scope::All => "all",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Options {
    pub images: bool,
    pub sheets: bool,
    pub data: bool,
    pub csv: bool,
    pub sounds: bool,
    pub empty: bool,
    pub reference: bool,
    pub team: usize,
    /// Sequence keys to export; `None`: all.
    pub seq_keys: Option<HashSet<String>>,
}

impl Options {
    fn takes(&self, seq: &Seq) -> bool {
        self.seq_keys.as_ref().is_none_or(|k| k.contains(&seq.key))
    }
}

/// The slots of a scope, as the dialog offers them. `group`: for [`Scope::Group`]; `current`: for
/// [`Scope::Slot`].
pub fn slot_list(spec: &Spec, pack: &Pack, scope: Scope, group: usize, current: Option<SlotRef>, opt: &Options) -> Vec<SlotRef> {
    let all = |g: usize| (0..spec.groups[g].slots.len()).map(move |s| SlotRef { g, s });
    let mut list: Vec<SlotRef> = match scope {
        Scope::Slot => current.into_iter().collect(),
        Scope::Group => all(group).collect(),
        Scope::Pack | Scope::All => (0..spec.groups.len()).flat_map(all).collect(),
    };
    if scope != Scope::Slot && !(scope == Scope::All || opt.empty || opt.reference) {
        let with = pack.slots_with_files();
        list.retain(|r| {
            let (g, s) = spec.slot(*r);
            with.contains(&format!("{}/{}", g.id, s.id))
        });
    }
    list
}

/// Original frames the list holds under `opt` (the zip's entry limit is checked against this).
pub fn reference_frames(spec: &Spec, list: &[SlotRef], opt: &Options) -> usize {
    let mut n = 0;
    for r in list {
        let (g, s) = spec.slot(*r);
        if g.kind == Kind::Gfx {
            n += s.seqs.iter().filter(|q| opt.takes(q)).map(|q| q.frames.len()).sum::<usize>();
        }
    }
    n
}

/// `s4pack-export-<what>-<date>.zip`.
pub fn file_name(what: &str) -> String {
    format!("s4pack-export-{what}-{}.zip", chrono::Utc::now().format("%Y-%m-%d"))
}

/// Live progress of a running export; shared with the UI.
#[derive(Default)]
pub struct Progress {
    pub done: AtomicUsize,
    pub total: AtomicUsize,
    pub files: AtomicUsize,
    pub bytes: AtomicU64,
    pub cancel: AtomicBool,
}

#[derive(Debug)]
pub struct Summary {
    pub slots: usize,
    pub entries: usize,
    pub bytes: u64,
    pub path: PathBuf,
}

impl Summary {
    pub fn label(&self) -> String {
        format!("Done: {}, {} entries, {:.1} MB.", plural(self.slots, "slot"), self.entries, self.bytes as f64 / 1048576.0)
    }
}

const CANCELLED: &str = "cancelled";

/// True for the error of a cancelled export.
pub fn is_cancelled(e: &anyhow::Error) -> bool {
    e.to_string() == CANCELLED
}

/// JSON as `JSON.stringify(x, null, 1)` prints it.
fn pretty(v: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    let fmt = serde_json::ser::PrettyFormatter::with_indent(b" ");
    let mut ser = serde_json::Serializer::with_formatter(&mut out, fmt);
    serde::Serialize::serialize(v, &mut ser).expect("JSON to memory");
    out
}

/// A number with `toFixed(digits)`'s rounding, printed like JavaScript's.
fn fixed(x: f64, digits: i32) -> Value {
    let m = 10f64.powi(digits);
    num((x * m).round() / m)
}

fn csv_cell(s: &str) -> String {
    if s.contains(['"', ',', '\n']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() }
}

fn csv(rows: &[Vec<String>]) -> String {
    let mut out = String::new();
    for r in rows {
        let cells: Vec<String> = r.iter().map(|c| csv_cell(c)).collect();
        out.push_str(&cells.join(","));
        out.push('\n');
    }
    out
}

fn strs(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

struct Rows {
    coords: Vec<Vec<String>>,
    routines: Vec<Vec<String>>,
    frames: Vec<Vec<String>>,
}

impl Rows {
    fn new() -> Rows {
        Rows {
            coords: vec![strs(&["slot", "layer", "index", "cell_dx", "cell_dy", "px_x", "px_y", "extra"])],
            routines: vec![strs(&["slot", "routine", "step", "start_tick", "ticks", "task", "cell_dx", "cell_dy", "px_x", "px_y", "animation", "frames", "dir", "good", "trigger", "sound"])],
            frames: vec![strs(&["slot", "sequence", "frame", "original_frame", "file", "w", "h", "anchor_x", "anchor_y", "expected_w", "expected_h", "reference_file"])],
        }
    }
}

/// Shelf packing of frame sizes into pages of at most [`SHEET_W`] × [`SHEET_H`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    /// Index into the sizes and position.
    pub items: Vec<(usize, u32, u32)>,
    pub w: u32,
    pub h: u32,
}

pub fn layout_sheets(sizes: &[(u32, u32)]) -> Vec<Page> {
    let mut pages: Vec<Page> = Vec::new();
    let (mut x, mut y, mut row_h) = (0u32, 0u32, 0u32);
    for (i, &(w, h)) in sizes.iter().enumerate() {
        if pages.is_empty() || (x + w > SHEET_W && x > 0 && y + row_h + h > SHEET_H) {
            pages.push(Page { items: Vec::new(), w: 1, h: 1 });
            (x, y, row_h) = (0, 0, 0);
        } else if x + w > SHEET_W && x > 0 {
            x = 0;
            y += row_h + 1;
            row_h = 0;
        }
        let page = pages.last_mut().expect("a page exists");
        page.items.push((i, x, y));
        page.w = page.w.max(x + w);
        page.h = page.h.max(y + h);
        x += w + 1;
        row_h = row_h.max(h);
    }
    pages
}

struct SheetItem {
    j: usize,
    frame: Rgba,
    mask: Option<Rgba>,
    anchor: [f64; 2],
}

fn blit(page: &mut [u8], pw: u32, x: u32, y: u32, img: &Rgba) {
    let row = img.w as usize * 4;
    for yy in 0..img.h as usize {
        let o = (y as usize + yy) * pw as usize * 4 + x as usize * 4;
        page[o..o + row].copy_from_slice(&img.px[yy * row..(yy + 1) * row]);
    }
}

fn write_sheets<W: Write>(zip: &mut ZipWriter<W>, dir: &str, seq: &Seq, items: &[SheetItem], scale: f64) -> Result<Vec<Value>> {
    let sizes: Vec<(u32, u32)> = items.iter().map(|it| (it.frame.w, it.frame.h)).collect();
    let pages = layout_sheets(&sizes);
    let mut out = Vec::new();
    for (pi, pg) in pages.iter().enumerate() {
        let suffix = if pages.len() > 1 { format!("-{pi}") } else { String::new() };
        let name = format!("{dir}/sheets/{}{suffix}", seq.key);
        let size = pg.w as usize * pg.h as usize * 4;
        let (mut cv, mut mv) = (vec![0u8; size], vec![0u8; size]);
        let mut masks = false;
        let mut frames = Map::new();
        for &(i, fx, fy) in &pg.items {
            let it = &items[i];
            let (w, h) = (it.frame.w, it.frame.h);
            blit(&mut cv, pg.w, fx, fy, &it.frame);
            if let Some(m) = it.mask.as_ref().filter(|m| m.w == w && m.h == h) {
                blit(&mut mv, pg.w, fx, fy, m);
                masks = true;
            }
            frames.insert(
                format!("{}/{:03}", seq.key, it.j),
                json!({
                    "frame": {"x": fx, "y": fy, "w": w, "h": h}, "rotated": false, "trimmed": false,
                    "spriteSourceSize": {"x": 0, "y": 0, "w": w, "h": h}, "sourceSize": {"w": w, "h": h},
                    "pivot": {"x": fixed(it.anchor[0] / w as f64, 6), "y": fixed(it.anchor[1] / h as f64, 6)},
                    "anchor": [num(it.anchor[0]), num(it.anchor[1])],
                }),
            );
        }
        zip.add(&format!("{name}.png"), &encode_rgba(pg.w, pg.h, &cv)?)?;
        if masks {
            zip.add(&format!("{name}.team.png"), &encode_rgba(pg.w, pg.h, &mv)?)?;
        }
        let team_mask = masks.then(|| format!("{}{suffix}.team.png", seq.key));
        zip.add(
            &format!("{name}.json"),
            &pretty(&json!({
                "frames": frames,
                "meta": {"app": "s4rs Asset Studio", "version": EXPORT_FORMAT, "image": format!("{}{suffix}.png", seq.key), "team_mask": team_mask,
                         "format": "RGBA8888", "size": {"w": pg.w, "h": pg.h}, "scale": num(scale).to_string()},
            })),
        )?;
        out.push(json!({
            "image": format!("sheets/{}{suffix}.png", seq.key), "atlas": format!("sheets/{}{suffix}.json", seq.key),
            "team_mask": masks.then(|| format!("sheets/{}{suffix}.team.png", seq.key)), "frames": pg.items.len(),
        }));
    }
    Ok(out)
}

/// Original frames, cut from the spec's reference strip: team pixels in the chosen player's
/// colours, one PNG per original frame at its own pixel size.
fn write_reference<W: Write>(spec: &Spec, zip: &mut ZipWriter<W>, dir: &str, g: &Group, seq: &Seq, team: usize) -> Result<Vec<String>> {
    let reference = seq.reference.as_deref().context("no reference strip")?;
    let read = |rel: &str| -> Result<Rgba> {
        let p = spec.ref_path(rel);
        decode_rgba(&std::fs::read(&p).with_context(|| p.display().to_string())?).with_context(|| p.display().to_string())
    };
    let mut strip = read(reference)?;
    if g.has_team_ramp(seq)
        && let (Some(mask), Some(pal)) = (&seq.mask, seq.pal)
        && let Some(hex) = g.team_ramps.get(&pal.to_string()).and_then(|r| r.get(team))
    {
        recolour(&mut strip, &read(mask)?, spec.player_colors[team], &parse_ramp(hex));
    }
    let mut out = Vec::new();
    for (i, fr) in seq.frames.iter().enumerate() {
        let (w, h) = (fr[1].max(0) as usize, fr[2].max(0) as usize);
        let mut px = vec![0u8; w * h * 4];
        for y in 0..h {
            let (sx, sy) = (fr[6] as usize, fr[7] as usize + y);
            if sy >= strip.h as usize {
                break;
            }
            let o = (sy * strip.w as usize + sx) * 4;
            let n = (w * 4).min(strip.px.len().saturating_sub(o));
            px[y * w * 4..y * w * 4 + n].copy_from_slice(&strip.px[o..o + n]);
        }
        let name = format!("reference/{}/{i:03}.png", seq.key);
        zip.add(&format!("{dir}/{name}"), &encode_rgba(w as u32, h as u32, &px)?)?;
        out.push(name);
    }
    Ok(out)
}

fn read_pack(pack: &Pack, rel: &str) -> Result<Vec<u8>> {
    std::fs::read(pack.abs(rel)).with_context(|| rel.to_string())
}

fn read_rgba(pack: &Pack, rel: &str) -> Result<Rgba> {
    decode_rgba(&read_pack(pack, rel)?).with_context(|| rel.to_string())
}

struct Run<'a, W: Write> {
    spec: &'a Spec,
    pack: &'a Pack,
    opt: &'a Options,
    zip: ZipWriter<W>,
    rows: Rows,
    progress: &'a Progress,
}

impl<W: Write> Run<'_, W> {
    fn stop_if_cancelled(&self) -> Result<()> {
        if self.progress.cancel.load(Ordering::Relaxed) {
            bail!(CANCELLED);
        }
        Ok(())
    }

    fn add(&mut self, name: &str, data: &[u8]) -> Result<()> {
        self.zip.add(name, data)?;
        self.progress.bytes.store(self.zip.offset, Ordering::Relaxed);
        Ok(())
    }

    fn slot(&mut self, g: &Group, slot: &Slot) -> Result<usize> {
        let dir = format!("slots/{}/{}", g.id, slot.id);
        let mut out = Map::new();
        out.insert("format".into(), EXPORT_FORMAT.into());
        out.insert("slot".into(), format!("{}/{}", g.id, slot.id).into());
        out.insert("group".into(), g.id.as_str().into());
        out.insert("group_label".into(), g.label.as_str().into());
        out.insert("kind".into(), match g.kind {
            Kind::Gfx => "gfx",
            Kind::Sound => "sound",
            Kind::Music => "music",
        }.into());
        out.insert("name".into(), slot.name.as_str().into());
        out.insert("owner".into(), slot.owner.as_deref().filter(|s| !s.is_empty()).into());
        let mut files = 0;
        match g.kind {
            Kind::Gfx => files = self.gfx(g, slot, &dir, &mut out)?,
            Kind::Sound => {
                out.insert("volume".into(), slot.volume.into());
                out.insert("uses".into(), json!(slot.uses));
                let mut variants = Vec::new();
                for seq in &slot.seqs {
                    let p = sound_path(slot, seq);
                    let has = self.pack.has(&p);
                    let original = (!seq.missing).then(|| json!({"seconds": seq.secs.map(num), "rate": seq.rate, "channels": seq.channels}));
                    variants.push(json!({
                        "key": seq.key, "label": seq.label, "empty_in_original": seq.missing, "original": original,
                        "file": has.then(|| format!("{}.wav", seq.key)),
                    }));
                    if has && self.opt.sounds {
                        let bytes = read_pack(self.pack, &p)?;
                        self.add(&format!("{dir}/{}.wav", seq.key), &bytes)?;
                        files += 1;
                    }
                }
                out.insert("variants".into(), Value::Array(variants));
            }
            Kind::Music => {
                let p = self.pack.music_file(slot);
                out.insert("uses".into(), json!(slot.uses));
                let file = p.as_deref().map(|p| p.rsplit('/').next().unwrap_or(p).to_string());
                out.insert("file".into(), file.clone().into());
                if let (Some(p), Some(file), true) = (&p, &file, self.opt.sounds) {
                    let bytes = read_pack(self.pack, p)?;
                    self.add(&format!("{dir}/{file}"), &bytes)?;
                    files += 1;
                }
            }
        }
        if self.opt.data {
            self.add(&format!("{dir}/data.json"), &pretty(&Value::Object(out)))?;
        }
        Ok(files)
    }

    fn gfx(&mut self, g: &Group, slot: &Slot, dir: &str, out: &mut Map<String, Value>) -> Result<usize> {
        let (pack, spec, opt) = (self.pack, self.spec, self.opt);
        let mut files = 0;
        if let Some(job) = slot.job {
            out.insert("gfx_job".into(), json!({"index": job, "file": g.file, "job_lists": slot.job_lists, "length_from_frames": slot.length_from_frames}));
        }
        if let Some(k) = &g.keep_frames {
            out.insert("keep_frame_counts".into(), k.as_str().into());
        }
        if let Some(e) = slot.exact_size {
            out.insert("exact_size".into(), e.into());
        }
        if !slot.uses.is_empty() {
            out.insert("uses".into(), json!(slot.uses));
        }
        let id = format!("{}/{}", g.id, slot.id);
        let scale = pack.scale();
        let mut sequences = Vec::new();
        for seq in slot.seqs.iter().filter(|q| opt.takes(q)) {
            let conf = pack.conf(g, slot, seq);
            let n = seq.frames.len();
            let k = seq_mult(pack, slot, seq, conf);
            let (ku, nk) = (k as usize, n * k as usize);
            let refs = if opt.reference { write_reference(spec, &mut self.zip, dir, g, seq, opt.team)? } else { Vec::new() };
            self.progress.bytes.store(self.zip.offset, Ordering::Relaxed);
            files += refs.len();
            let mut frames = Vec::new();
            let mut sheet_items = Vec::new();
            for j in 0..nk {
                self.stop_if_cancelled()?;
                let fr = orig_of(seq, k, j);
                let (p, mp) = (frame_path(g, slot, seq, j), mask_path(g, slot, seq, j));
                let rec = pack.file(&p);
                let has_mask = pack.has(&mp);
                let file = rec.map(|_| format!("{}/{j:03}.png", seq.key));
                let mask = (rec.is_some() && has_mask).then(|| format!("{}/{j:03}.team.png", seq.key));
                let anchor = frame_anchor(pack, slot, seq, conf, j);
                let [ew, eh] = expect_size(fr, scale);
                let reference = refs.get(j / ku).cloned();
                frames.push(json!({
                    "index": j, "original_frame": j / ku, "file": file, "team_mask": mask, "reference_file": reference,
                    "size": rec.filter(|r| r.w > 0).map(|r| [r.w, r.h]), "expected_size": [ew, eh], "anchor": [num(anchor[0]), num(anchor[1])],
                    "original": {"size": [fr[1], fr[2]], "anchor": [fr[3], fr[4]], "half_size": fr[5] & F_HALF != 0, "team_colour": fr[5] & F_TEAM != 0, "shadow": fr[5] & F_SHADOW != 0},
                }));
                let dim = |v: Option<u32>| v.filter(|&x| x > 0).map_or(String::new(), |x| x.to_string());
                self.rows.frames.push(vec![
                    id.clone(),
                    seq.key.clone(),
                    j.to_string(),
                    (j / ku).to_string(),
                    file.clone().unwrap_or_default(),
                    dim(rec.map(|r| r.w)),
                    dim(rec.map(|r| r.h)),
                    num(anchor[0]).to_string(),
                    num(anchor[1]).to_string(),
                    ew.to_string(),
                    eh.to_string(),
                    reference.unwrap_or_default(),
                ]);
                if rec.is_none() {
                    continue;
                }
                if opt.images {
                    let bytes = read_pack(pack, &p)?;
                    self.add(&format!("{dir}/{}", file.as_deref().unwrap_or_default()), &bytes)?;
                    files += 1;
                    if let Some(m) = &mask {
                        let bytes = read_pack(pack, &mp)?;
                        self.add(&format!("{dir}/{m}"), &bytes)?;
                        files += 1;
                    }
                }
                if opt.sheets {
                    let frame = read_rgba(pack, &p)?;
                    let mask = if mask.is_some() { Some(read_rgba(pack, &mp)?) } else { None };
                    sheet_items.push(SheetItem { j, frame, mask, anchor });
                }
            }
            // Events stay on their original frame f; at k frames per original frame that is f·k.
            let mut events = Vec::new();
            let mut ev = |src: &str, s: &crate::model::SoundRef| {
                events.push(json!({"source": src, "sound": s.name, "original_frame": s.frame, "frame": s.frame * k as i32, "chance": s.chance}));
            };
            if let Some(s) = &slot.sound {
                ev("jobSoundList.xml", s);
            }
            if let Some(s) = seq.patch_sound() {
                ev("buildingInfo.xml patch", &s);
            }
            let mut sheets = Vec::new();
            if opt.sheets && !sheet_items.is_empty() {
                sheets = write_sheets(&mut self.zip, dir, seq, &sheet_items, scale)?;
                self.progress.bytes.store(self.zip.offset, Ordering::Relaxed);
                files += sheets.len();
            }
            sequences.push(json!({
                "key": seq.key, "label": seq.label, "part_name": seq.part_name.as_deref().filter(|s| !s.is_empty()), "patch": seq.patch,
                "original_frames": n, "frame_multiplier": k, "frames": nk,
                "ticks": n, "tick_ms": 71, "fps": fixed(GAME_FPS * k as f64, 4), "frame_ms": fixed(71.0 / k as f64, 4),
                "events": events, "frames_detail": frames, "sheets": sheets,
            }));
        }
        out.insert("sequences".into(), Value::Array(sequences));
        if let Some((b, _)) = pack.saved_building(g, slot) {
            out.insert("building".into(), building_export(g, slot, &b, &mut self.rows));
        }
        Ok(files)
    }
}

fn building_export(g: &Group, slot: &Slot, b: &Building, rows: &mut Rows) -> Value {
    let px = |c: [i32; 2]| is_cell(c).then(|| cell_px(c));
    let original = slot.building.as_ref();
    let changed: Vec<&str> = BLD_KEYS.iter().copied().filter(|k| original.is_some_and(|o| !b.same_key(o, k))).collect();
    let id = format!("{}/{}", g.id, slot.id);
    let mut add = |layer: &str, i: usize, c: Option<[i32; 2]>, p: Option<[i32; 2]>, extra: String| {
        let (cs, ps) = (c.map(|c| c.map(|v| v.to_string())), p.map(|c| c.map(|v| v.to_string())));
        let [cx, cy] = cs.unwrap_or_default();
        let [px_x, px_y] = ps.unwrap_or_default();
        rows.coords.push(vec![id.clone(), layer.to_string(), i.to_string(), cx, cy, px_x, px_y, extra]);
    };
    for (name, cells) in [("footprint", &b.footprint), ("block", &b.block), ("dig", &b.dig)] {
        for (i, c) in cells.iter().enumerate() {
            add(name, i, Some(*c), px(*c), String::new());
        }
    }
    for (name, c) in [("door", Some(b.door)), ("flag", Some(b.flag)), ("work", b.work)] {
        if let Some(c) = c.filter(|c| is_cell(*c)) {
            add(name, 0, Some(c), px(c), String::new());
        }
    }
    for (i, s) in b.builders.iter().enumerate() {
        add("builder", i, Some([s[0], s[1]]), px([s[0], s[1]]), format!("dir {}", s[2]));
    }
    for (i, p) in b.piles.iter().enumerate() {
        add("pile", i, Some(p.cell), px(p.cell), p.good.clone().unwrap_or_default());
    }
    for (i, s) in b.soldiers_px.iter().enumerate() {
        add("soldier", i, None, Some([s[0], s[1]]), format!("dir {}{}", s[2], if s[3] != 0 { " top" } else { "" }));
    }
    if let Some(m) = b.mini_flag_px {
        add("mini_flag", 0, None, Some(m), String::new());
    }
    for wl in b.work_lists.iter() {
        for (i, s) in wl.steps.iter().enumerate() {
            rows.routines.push(vec![
                id.clone(),
                wl.name.clone(),
                (i + 1).to_string(),
                s.start.to_string(),
                s.ticks.map_or(String::new(), |t| t.to_string()),
                s.task_name().to_string(),
                s.cell[0].to_string(),
                s.cell[1].to_string(),
                s.to[0].to_string(),
                s.to[1].to_string(),
                s.job.clone(),
                s.frames.to_string(),
                s.dir.to_string(),
                s.good.clone().unwrap_or_default(),
                s.trigger.clone().unwrap_or_default(),
                s.sound.as_ref().map_or(String::new(), |x| format!("{}@{}", x.name, x.frame)),
            ]);
        }
    }
    json!({
        "kind": b.kind, "inhabitant": b.inhabitant,
        "changed_from_original": changed,
        "cells": {
            "footprint": b.footprint, "block": b.block, "dig": b.dig, "door": b.door, "flag": b.flag, "work": b.work.filter(|c| is_cell(*c)),
            "builders": b.builders.iter().map(|s| json!({"cell": [s[0], s[1]], "dir": s[2]})).collect::<Vec<_>>(),
            "piles": b.piles.iter().map(|p| json!({"cell": p.cell, "good": p.good, "pixel_offset": p.px})).collect::<Vec<_>>(),
        },
        "pixels": {
            "door": px(b.door), "flag": px(b.flag), "work": b.work.and_then(px),
            "builders": b.builders.iter().map(|s| px([s[0], s[1]])).collect::<Vec<_>>(),
            "piles": b.piles.iter().map(|p| px(p.cell)).collect::<Vec<_>>(),
            "soldiers": b.soldiers_px.iter().map(|s| json!({"px": [s[0], s[1]], "dir": s[2], "top": s[3] != 0})).collect::<Vec<_>>(),
            "mini_flag": b.mini_flag_px,
        },
        "work_routines": &*b.work_lists,
        "notes": [
            "Cells are offsets (dx, dy) from the building's anchor node; pixels are zoom-1 game pixels from the anchor: x = 24·dx − 12·dy, y = 12·dy.",
            "Pile pixel_offset is buildingInfo.xml's xPixelOffset / yPixelOffset; it does not match the cell and its meaning is unverified.",
            "Routine ticks follow CHouseWorkerRole (start on the door cell, 9 ticks per node walked); one tick is 71 ms.",
        ],
    })
}

fn readme() -> String {
    [
        "s4rs Asset Studio export",
        "",
        "index.json             what was exported, the pack settings and the conventions (geometry, time, anchors, team colour)",
        "slots/<group>/<slot>/  one folder per slot:",
        "  data.json            the slot's data: sequences (frame timing, multiplier, anchors, events, original sizes),",
        "                       building data (cells and pixels), worker routines, job lists",
        "  <seq>/NNN.png        your frames; NNN.team.png their team masks",
        "  sheets/<seq>.png     sprite sheet of a sequence; <seq>.json its atlas (TexturePacker JSON hash, pivot = anchor);",
        "                       <seq>.team.png the masks in the same layout",
        "  <variant>.wav        your sounds (sound slots); music slots hold the track",
        "  reference/<seq>/NNN.png  only when chosen: the original frames (see the warning below)",
        "coords.csv             building cells and pixel positions (footprint, blocked, dig, door, flag, work spot, builders, piles, soldiers, mini flag)",
        "routines.csv           house worker routines step by step (start tick, length, task, cell, pixel, animation, direction, good, trigger, sound)",
        "frames.csv             every frame: file, size, anchor, the original frame it stands for and the expected size",
        "",
        "Original game sounds are not included. Original frames only when chosen:",
        REFERENCE_WARNING,
        "",
    ]
    .join("\n")
}

/// Everything an export needs; moved to the worker thread.
pub struct Job {
    pub spec: Arc<Spec>,
    pub pack: Arc<Pack>,
    pub scope: Scope,
    pub list: Vec<SlotRef>,
    pub opt: Options,
    pub out: PathBuf,
}

/// Runs an export; a partial file is removed when it fails or is cancelled.
pub fn run(job: &Job, progress: &Progress) -> Result<Summary> {
    let tmp = partial_path(&job.out);
    let r = write_zip(job, &tmp, progress).and_then(|s| {
        if job.out.exists() {
            std::fs::remove_file(&job.out).with_context(|| job.out.display().to_string())?;
        }
        std::fs::rename(&tmp, &job.out).with_context(|| job.out.display().to_string())?;
        Ok(s)
    });
    if r.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    r
}

fn partial_path(out: &Path) -> PathBuf {
    let mut name = out.file_name().unwrap_or_default().to_os_string();
    name.push(".partial");
    out.with_file_name(name)
}

fn write_zip(job: &Job, tmp: &Path, progress: &Progress) -> Result<Summary> {
    let file = std::fs::File::create(tmp).with_context(|| tmp.display().to_string())?;
    let now = chrono::Local::now();
    let stamp = dos_time(now.year(), now.month(), now.day(), now.hour(), now.minute(), now.second());
    let (spec, pack, opt) = (&*job.spec, &*job.pack, &job.opt);
    let mut run = Run { spec, pack, opt, zip: ZipWriter::new(BufWriter::new(file), stamp), rows: Rows::new(), progress };
    progress.total.store(job.list.len(), Ordering::Relaxed);
    for (i, r) in job.list.iter().enumerate() {
        run.stop_if_cancelled()?;
        let (g, s) = spec.slot(*r);
        let n = run.slot(g, s)?;
        progress.files.fetch_add(n, Ordering::Relaxed);
        progress.done.store(i + 1, Ordering::Relaxed);
    }
    if opt.csv {
        let rows = std::mem::replace(&mut run.rows, Rows { coords: Vec::new(), routines: Vec::new(), frames: Vec::new() });
        run.add("coords.csv", csv(&rows.coords).as_bytes())?;
        run.add("routines.csv", csv(&rows.routines).as_bytes())?;
        run.add("frames.csv", csv(&rows.frames).as_bytes())?;
    }
    let slots: Vec<String> = job.list.iter().map(|r| {
        let (g, s) = spec.slot(*r);
        format!("{}/{}", g.id, s.id)
    }).collect();
    let index = json!({
        "format": EXPORT_FORMAT,
        "exported_at": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "pack": {"name": pack.name(), "scale": num(pack.scale()), "frame_multiplier": pack.mult()},
        "scope": job.scope.name(),
        "include": {"images": opt.images, "sheets": opt.sheets, "data": opt.data, "csv": opt.csv, "sounds": opt.sounds, "empty": opt.empty, "reference": opt.reference, "team": opt.team},
        "sequences": if opt.seq_keys.is_some() { "filtered" } else { "all" },
        "slots": slots,
        "conventions": {
            "geometry": "a cell (dx, dy) from an anchor node lies at (24·dx − 12·dy, 12·dy) game pixels at zoom 1",
            "time": "one simulation tick is 71 ms; an original frame lasts one tick; with frame multiplier k your frame j shows original frame floor(j / k) and lasts 71 / k ms",
            "anchor": "the pixel of your frame that stands on the map node, in your pixels (pack scale)",
            "team_colour": "colour = mix(rgb, player × luma(rgb), mask), luma = 0.299 r + 0.587 g + 0.114 b",
            "events": "an event on original frame f is on your frame f·k",
        },
        "not_included": if opt.reference { "original game sounds" } else { "original game images and sounds" },
        "reference": opt.reference.then(|| json!({"team_player": opt.team + 1, "warning": REFERENCE_WARNING})),
    });
    run.add("index.json", &pretty(&index))?;
    run.add("README.txt", readme().as_bytes())?;
    let entries = run.zip.count;
    let bytes = run.zip.finish()?;
    Ok(Summary { slots: job.list.len(), entries, bytes, path: job.out.clone() })
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::model::test_spec;
    use crate::pack::tests::{png_bytes, temp};
    use crate::pack::{bld_path, conf_path};
    use std::collections::HashMap;

    /// Entries of a stored zip.
    pub fn unzip(bytes: &[u8]) -> HashMap<String, Vec<u8>> {
        let u32at = |o: usize| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap()) as usize;
        let u16at = |o: usize| u16::from_le_bytes(bytes[o..o + 2].try_into().unwrap()) as usize;
        let e = bytes.len() - 22;
        let (count, mut c) = (u16at(e + 10), u32at(e + 16));
        let mut out = HashMap::new();
        for _ in 0..count {
            assert_eq!(u32at(c), 0x0201_4b50);
            let (size, nlen, xlen, clen, off) = (u32at(c + 24), u16at(c + 28), u16at(c + 30), u16at(c + 32), u32at(c + 42));
            let name = String::from_utf8(bytes[c + 46..c + 46 + nlen].to_vec()).unwrap();
            let start = off + 30 + u16at(off + 26) + u16at(off + 28);
            out.insert(name, bytes[start..start + size].to_vec());
            c += 46 + nlen + xlen + clen;
        }
        out
    }

    fn opts() -> Options {
        Options { images: true, sheets: true, data: true, csv: true, sounds: true, empty: false, reference: false, team: 1, seq_keys: None }
    }

    #[test]
    fn sheets_pack_on_shelves_and_pages() {
        let pages = layout_sheets(&[(3000, 100), (2000, 50), (10, 10)]);
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].items, vec![(0, 0, 0), (1, 0, 101), (2, 2001, 101)]);
        assert_eq!((pages[0].w, pages[0].h), (3000, 151));
        // A row that no longer fits starts a new page.
        let pages = layout_sheets(&[(3000, 6000), (3000, 3000)]);
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[1].items, vec![(1, 0, 0)]);
        assert!(layout_sheets(&[]).is_empty());
    }

    #[test]
    fn sheet_pixels_land_where_the_atlas_says() {
        let frame = |c: u8| Rgba { w: 2, h: 3, px: [c, 0, 0, 255].repeat(6) };
        let items: Vec<SheetItem> = (0..2).map(|j| SheetItem { j, frame: frame(10 + j as u8), mask: (j == 1).then(|| frame(200)), anchor: [1.0, 2.0] }).collect();
        let d = temp("sheet");
        let spec = test_spec(&d);
        let seq = &spec.groups[1].slots[0].seqs[0];
        let mut bytes = Vec::new();
        let mut z = ZipWriter::new(&mut bytes, (0, 0));
        let out = write_sheets(&mut z, "s", seq, &items, 1.0).unwrap();
        z.finish().unwrap();
        assert_eq!(out[0]["frames"], 2);
        let e = unzip(&bytes);
        let sheet = decode_rgba(&e["s/sheets/0.png"]).unwrap();
        assert_eq!((sheet.w, sheet.h), (5, 3));
        let at = |img: &Rgba, x: usize, y: usize| img.px[(y * img.w as usize + x) * 4];
        assert_eq!((at(&sheet, 0, 0), at(&sheet, 1, 2), at(&sheet, 2, 0), at(&sheet, 3, 0), at(&sheet, 4, 2)), (10, 10, 0, 11, 11), "1 px gap between frames");
        let masks = decode_rgba(&e["s/sheets/0.team.png"]).unwrap();
        assert_eq!((at(&masks, 0, 0), at(&masks, 3, 0)), (0, 200), "a frame without a mask stays empty");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn numbers_print_like_tofixed() {
        assert_eq!(fixed(1000.0 / 71.0 * 2.0, 4).to_string(), "28.169");
        assert_eq!(fixed(71.0 / 3.0, 4).to_string(), "23.6667");
        assert_eq!(fixed(71.0, 4).to_string(), "71");
        assert_eq!(fixed(1.0 / 128.0, 6).to_string(), "0.007813", "ties go up like toFixed");
    }

    #[test]
    fn csv_quotes_what_needs_it() {
        assert_eq!(csv(&[strs(&["a", "b,c", "d\"e", "f\ng"])]), "a,\"b,c\",\"d\"\"e\",\"f\ng\"\n");
    }

    #[test]
    fn pretty_json_matches_javascript() {
        let v = json!({"a": [1, {"b": []}], "c": {}});
        assert_eq!(String::from_utf8(pretty(&v)).unwrap(), "{\n \"a\": [\n  1,\n  {\n   \"b\": []\n  }\n ],\n \"c\": {}\n}");
    }

    #[test]
    fn data_json_has_the_documented_shape() {
        let d = temp("x0");
        let mut pack = Pack::open(&d.join("pack")).unwrap();
        let spec = Arc::new(test_spec(&d.join("spec")));
        let r = spec.find("settlers-roman/JOB_X").unwrap();
        let (g, slot) = spec.slot(r);
        let q = &slot.seqs[0];
        for i in 0..8 {
            pack.put(&frame_path(g, slot, q, i), &png_bytes(20, 20)).unwrap();
        }
        pack.put(&mask_path(g, slot, q, 3), &png_bytes(20, 20)).unwrap();
        pack.put(&conf_path(g, slot, q), br#"{"frame_multiplier":2,"anchor":[10,18]}"#).unwrap();
        let (bg, bslot) = spec.slot(spec.find("buildings-roman/BUILDING_MILL").unwrap());
        pack.put(&bld_path(bg, bslot), br#"{"flag":[7,7]}"#).unwrap();
        let mut sq = opts();
        sq.seq_keys = Some(["0".to_string()].into_iter().collect());
        let list = vec![r, spec.find("buildings-roman/BUILDING_MILL").unwrap()];
        let out = d.join("out.zip");
        let job = Job { spec: spec.clone(), pack: Arc::new(pack), scope: Scope::Group, list, opt: sq, out: out.clone() };
        let progress = Progress::default();
        let s = run(&job, &progress).unwrap();
        assert_eq!(progress.done.load(Ordering::Relaxed), 2);
        assert!(s.label().starts_with("Done: 2 slots, "), "{}", s.label());
        let z = unzip(&std::fs::read(&out).unwrap());
        assert!(z.contains_key("slots/settlers-roman/JOB_X/0/007.png"));
        assert!(z.contains_key("slots/settlers-roman/JOB_X/0/003.team.png"));
        assert!(z.contains_key("slots/settlers-roman/JOB_X/sheets/0.png"));
        assert!(z.contains_key("slots/settlers-roman/JOB_X/sheets/0.team.png"));
        assert!(!z.contains_key("slots/settlers-roman/JOB_X/1/000.png"));
        let data: Value = serde_json::from_slice(&z["slots/settlers-roman/JOB_X/data.json"]).unwrap();
        assert_eq!(data["format"], EXPORT_FORMAT);
        assert_eq!(data["owner"], "SETTLER_CARRIER");
        assert_eq!(data["gfx_job"]["index"], 3);
        let q0 = &data["sequences"][0];
        assert_eq!((q0["original_frames"].as_u64(), q0["frame_multiplier"].as_u64(), q0["frames"].as_u64()), (Some(4), Some(2), Some(8)));
        assert_eq!(q0["fps"].to_string(), "28.169");
        assert_eq!(q0["frame_ms"].to_string(), "35.5");
        let f = &q0["frames_detail"][3];
        assert_eq!(f["file"], "0/003.png");
        assert_eq!(f["team_mask"], "0/003.team.png");
        assert_eq!(f["original_frame"], 1);
        assert_eq!(f["anchor"].to_string(), "[10,18]");
        assert_eq!(f["expected_size"].to_string(), "[16,24]");
        assert_eq!(f["original"]["team_colour"], true);
        assert_eq!(q0["events"][0], json!({"source": "jobSoundList.xml", "sound": "SND_STEP", "original_frame": 2, "frame": 4, "chance": 50}));
        assert_eq!(q0["sheets"][0]["atlas"], "sheets/0.json");
        let atlas: Value = serde_json::from_slice(&z["slots/settlers-roman/JOB_X/sheets/0.json"]).unwrap();
        assert_eq!(atlas["frames"]["0/001"]["frame"], json!({"x": 21, "y": 0, "w": 20, "h": 20}));
        assert_eq!(atlas["frames"]["0/001"]["pivot"], json!({"x": 0.5, "y": 0.9}));
        assert_eq!(atlas["meta"]["version"], EXPORT_FORMAT);
        let b: Value = serde_json::from_slice(&z["slots/buildings-roman/BUILDING_MILL/data.json"]).unwrap();
        assert_eq!(b["building"]["changed_from_original"], json!(["flag"]));
        assert_eq!(b["building"]["cells"]["flag"], json!([7, 7]));
        assert_eq!(b["building"]["pixels"]["flag"], json!([84, 84]));
        assert_eq!(b["building"]["work_routines"][0]["steps"][0]["task"], "CEntityTask::GO_VIRTUAL");
        let index: Value = serde_json::from_slice(&z["index.json"]).unwrap();
        assert_eq!(index["sequences"], "filtered");
        assert_eq!(index["include"]["team"], 1);
        assert_eq!(index["not_included"], "original game images and sounds");
        assert!(std::str::from_utf8(&z["routines.csv"]).unwrap().contains("buildings-roman/BUILDING_MILL,WL,1,0,9,GO_VIRTUAL,1,2,24,24,JOB_X,8,1,GOOD_PLANK,,"));
        assert!(std::str::from_utf8(&z["coords.csv"]).unwrap().contains("flag,0,7,7,84,84,"));
        assert!(std::str::from_utf8(&z["frames.csv"]).unwrap().contains("settlers-roman/JOB_X,0,3,1,0/003.png,20,20,10,18,16,24,"));
        assert!(std::str::from_utf8(&z["README.txt"]).unwrap().contains(REFERENCE_WARNING));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn sounds_and_music_are_copied_and_data_says_so() {
        let d = temp("x1");
        let mut pack = Pack::open(&d.join("pack")).unwrap();
        let spec = Arc::new(test_spec(&d.join("spec")));
        let r = spec.find("sounds/SND_STEP").unwrap();
        let (_, slot) = spec.slot(r);
        pack.put(&sound_path(slot, &slot.seqs[0]), b"RIFFdata").unwrap();
        pack.put("music/track1.ogg", b"OggS").unwrap();
        let list = vec![r, spec.find("music/track1").unwrap()];
        let out = d.join("o.zip");
        let job = Job { spec: spec.clone(), pack: Arc::new(pack), scope: Scope::All, list, opt: opts(), out: out.clone() };
        run(&job, &Progress::default()).unwrap();
        let z = unzip(&std::fs::read(&out).unwrap());
        assert_eq!(z["slots/sounds/SND_STEP/00.wav"], b"RIFFdata");
        assert_eq!(z["slots/music/track1/track1.ogg"], b"OggS");
        let v: Value = serde_json::from_slice(&z["slots/sounds/SND_STEP/data.json"]).unwrap();
        assert_eq!(v["volume"], 80);
        assert_eq!(v["variants"][0]["original"], json!({"seconds": 0.5, "rate": 22050, "channels": 1}));
        assert_eq!(v["variants"][1], json!({"key": "01", "label": "variant 1 (bank slot 2)", "empty_in_original": true, "original": null, "file": null}));
        let v: Value = serde_json::from_slice(&z["slots/music/track1/data.json"]).unwrap();
        assert_eq!(v["file"], "track1.ogg");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn reference_frames_are_opt_in_cut_and_marked() {
        let d = temp("x2");
        let pack = Pack::open(&d.join("pack")).unwrap();
        let spec_dir = d.join("spec");
        let spec = Arc::new(test_spec(&spec_dir));
        let r = spec.find("settlers-roman/JOB_X").unwrap();
        // Two 16×24 frames' worth of strip for each of the 4 frames.
        let w = 4 * 17;
        let mut px = vec![0u8; (w * 24 * 4) as usize];
        px[17 * 4] = 200;
        px[17 * 4 + 3] = 255;
        for name in ["0", "1"] {
            let p = spec_dir.join(format!("ref/s/x/{name}.png"));
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, encode_rgba(w, 24, &px).unwrap()).unwrap();
        }
        std::fs::write(spec_dir.join("ref/s/x/0.team.png"), encode_rgba(w, 24, &vec![0u8; (w * 24 * 4) as usize]).unwrap()).unwrap();
        let mut o = opts();
        o.reference = true;
        o.images = false;
        o.sheets = false;
        let out = d.join("r.zip");
        assert_eq!(reference_frames(&spec, &[r], &o), 8);
        let job = Job { spec: spec.clone(), pack: Arc::new(pack), scope: Scope::Slot, list: vec![r], opt: o, out: out.clone() };
        run(&job, &Progress::default()).unwrap();
        let z = unzip(&std::fs::read(&out).unwrap());
        let f = decode_rgba(&z["slots/settlers-roman/JOB_X/reference/0/001.png"]).unwrap();
        assert_eq!((f.w, f.h, f.px[0], f.px[3]), (16, 24, 200, 255));
        let index: Value = serde_json::from_slice(&z["index.json"]).unwrap();
        assert_eq!(index["reference"]["team_player"], 2);
        assert_eq!(index["not_included"], "original game sounds");
        let data: Value = serde_json::from_slice(&z["slots/settlers-roman/JOB_X/data.json"]).unwrap();
        assert_eq!(data["sequences"][0]["frames_detail"][1]["reference_file"], "reference/0/001.png");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn scopes_and_cancel() {
        let d = temp("x3");
        let mut pack = Pack::open(&d.join("pack")).unwrap();
        let spec = Arc::new(test_spec(&d.join("spec")));
        let r = spec.find("settlers-roman/JOB_X").unwrap();
        let (g, slot) = spec.slot(r);
        pack.put(&frame_path(g, slot, &slot.seqs[0], 0), &png_bytes(16, 24)).unwrap();
        let mut o = opts();
        assert_eq!(slot_list(&spec, &pack, Scope::Pack, 0, None, &o), vec![r]);
        assert_eq!(slot_list(&spec, &pack, Scope::Group, 1, None, &o), vec![r]);
        assert!(slot_list(&spec, &pack, Scope::Group, 0, None, &o).is_empty());
        assert_eq!(slot_list(&spec, &pack, Scope::All, 0, None, &o).len(), 5);
        assert_eq!(slot_list(&spec, &pack, Scope::Slot, 0, Some(r), &o), vec![r]);
        o.empty = true;
        assert_eq!(slot_list(&spec, &pack, Scope::Group, 0, None, &o).len(), 1);
        let out = d.join("c.zip");
        let job = Job { spec: spec.clone(), pack: Arc::new(pack), scope: Scope::All, list: vec![r], opt: o, out: out.clone() };
        let progress = Progress::default();
        progress.cancel.store(true, Ordering::Relaxed);
        let e = run(&job, &progress).unwrap_err();
        assert!(is_cancelled(&e), "{e}");
        assert!(!out.exists() && !partial_path(&out).exists());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn an_existing_file_is_replaced() {
        let d = temp("x4");
        let pack = Pack::open(&d.join("pack")).unwrap();
        let spec = Arc::new(test_spec(&d.join("spec")));
        let out = d.join("e.zip");
        std::fs::write(&out, b"old").unwrap();
        let job = Job { spec, pack: Arc::new(pack), scope: Scope::Pack, list: vec![], opt: opts(), out: out.clone() };
        run(&job, &Progress::default()).unwrap();
        assert!(unzip(&std::fs::read(&out).unwrap()).contains_key("README.txt"));
        std::fs::remove_dir_all(&d).unwrap();
    }
}
