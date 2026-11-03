//! The replacement pack on disk: the listing (sizes from the PNG headers, the settings files'
//! text), paths, and atomic writes. Layout: `README.md` and `index.json`'s `pack_layout`.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{Map, Value};

use crate::model::{BLD_KEYS, Building, Frame, Group, Seq, Slot, half_k};

pub const MUSIC_EXT: [&str; 3] = ["mp3", "ogg", "wav"];
/// The studio offers these; `seq.json` and `pack.json` may hold up to 8.
pub const MULTS: [u32; 4] = [1, 2, 3, 4];
/// Pack scales offered in the top bar: replacement pixels per original pixel. Powers of two, so a
/// loader can shrink a 4x pack to 2x or 1x without resampling artefacts.
pub const SCALES: [f64; 3] = [1.0, 2.0, 4.0];

pub fn seq_dir(g: &Group, slot: &Slot, seq: &Seq) -> String {
    format!("gfx/{}/{}/{}", g.id, slot.id, seq.key)
}

pub fn frame_path(g: &Group, slot: &Slot, seq: &Seq, i: usize) -> String {
    format!("{}/{i:03}.png", seq_dir(g, slot, seq))
}

pub fn mask_path(g: &Group, slot: &Slot, seq: &Seq, i: usize) -> String {
    format!("{}/{i:03}.team.png", seq_dir(g, slot, seq))
}

pub fn conf_path(g: &Group, slot: &Slot, seq: &Seq) -> String {
    format!("{}/seq.json", seq_dir(g, slot, seq))
}

pub fn sound_path(slot: &Slot, seq: &Seq) -> String {
    format!("snd/{}/{}.wav", slot.id, seq.key)
}

pub fn bld_path(g: &Group, slot: &Slot) -> String {
    format!("gfx/{}/{}/building.json", g.id, slot.id)
}

/// A JSON number that prints like JavaScript's: `7`, not `7.0`.
pub fn num(x: f64) -> Value {
    if x.fract() == 0.0 && x.abs() < 9e15 {
        Value::from(x as i64)
    } else {
        serde_json::Number::from_f64(x).map_or(Value::Null, Value::Number)
    }
}

/// A whole number 1..=8.
pub fn valid_mult(v: &Value) -> Option<u32> {
    let f = v.as_f64()?;
    (f.fract() == 0.0 && (1.0..=8.0).contains(&f)).then_some(f as u32)
}

/// A sequence's `seq.json`: `{anchor | anchors, frame_multiplier}`; unknown keys are kept.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Conf {
    pub map: Map<String, Value>,
    /// The file is not valid JSON.
    pub bad: bool,
}

impl Conf {
    pub fn parse(text: &str) -> Conf {
        match serde_json::from_str::<Value>(text) {
            Ok(Value::Object(map)) => Conf { map, bad: false },
            Ok(_) => Conf::default(),
            Err(_) => Conf { map: Map::new(), bad: true },
        }
    }

    pub fn multiplier(&self) -> Option<u32> {
        self.map.get("frame_multiplier").and_then(valid_mult)
    }

    /// `frame_multiplier` when it is set but not valid.
    pub fn bad_multiplier(&self) -> Option<&Value> {
        self.map.get("frame_multiplier").filter(|v| valid_mult(v).is_none())
    }

    pub fn has_anchor(&self) -> bool {
        self.map.get("anchor").is_some_and(Value::is_array) || self.map.get("anchors").is_some_and(Value::is_array)
    }

    /// The anchor set for frame `j`: `anchors[j]`, else `anchor`.
    pub fn anchor_at(&self, j: usize) -> Option<[f64; 2]> {
        let pair = |v: &Value| Some([v.get(0)?.as_f64()?, v.get(1)?.as_f64()?]);
        if let Some(Value::Array(a)) = self.map.get("anchors")
            && let Some(p) = a.get(j).and_then(pair)
        {
            return Some(p);
        }
        self.map.get("anchor").and_then(pair)
    }

    /// `other` with the same keys and values in any order: both are the same settings.
    pub fn same(&self, other: &Conf) -> bool {
        self.map.len() == other.map.len() && self.map.iter().all(|(k, v)| other.map.get(k) == Some(v))
    }
}

#[derive(Clone, Debug, Default)]
pub struct PackFile {
    /// Modification time; part of a texture's cache key.
    pub stamp: u64,
    /// PNG size from the header, 0 for other files.
    pub w: u32,
    pub h: u32,
    /// JSON files only.
    pub text: Option<String>,
}

/// The pack's files as listed from disk.
#[derive(Clone, Debug, Default)]
pub struct Pack {
    pub root: PathBuf,
    pub files: HashMap<String, PackFile>,
    /// Folder -> file names.
    pub dirs: HashMap<String, Vec<String>>,
    /// `pack.json`; unknown keys are kept.
    pub meta: Map<String, Value>,
    /// Parsed `seq.json` files by path.
    confs: HashMap<String, Conf>,
    /// Bumped whenever the listing changes.
    pub ver: u64,
}

static NO_CONF: std::sync::LazyLock<Conf> = std::sync::LazyLock::new(Conf::default);

impl Pack {
    /// Reads the folder, creating it and `pack.json` when they do not exist yet.
    pub fn open(root: &Path) -> Result<Pack> {
        std::fs::create_dir_all(root).with_context(|| root.display().to_string())?;
        let manifest = root.join("pack.json");
        if !manifest.exists() {
            let meta = serde_json::json!({"format": 1, "name": "my pack", "scale": 1});
            std::fs::write(&manifest, meta.to_string()).with_context(|| manifest.display().to_string())?;
        }
        let mut p = Pack { root: root.to_path_buf(), ..Pack::default() };
        p.reload()?;
        Ok(p)
    }

    /// Reads the whole listing again (a large pack takes a moment: use a worker thread).
    pub fn reload(&mut self) -> Result<()> {
        self.files.clear();
        self.dirs.clear();
        self.confs.clear();
        let root = self.root.clone();
        self.scan(&root)?;
        self.read_meta();
        self.ver += 1;
        Ok(())
    }

    fn read_meta(&mut self) {
        let mut meta = Map::new();
        meta.insert("scale".into(), 1.into());
        meta.insert("name".into(), "".into());
        if let Some(Ok(Value::Object(m))) = self.files.get("pack.json").and_then(|f| f.text.as_deref()).map(serde_json::from_str::<Value>) {
            meta.extend(m);
        }
        self.meta = meta;
    }

    fn scan(&mut self, dir: &Path) -> Result<()> {
        let Ok(rd) = std::fs::read_dir(dir) else { return Ok(()) };
        for e in rd.flatten() {
            let p = e.path();
            let Ok(meta) = e.metadata() else { continue };
            if meta.is_dir() {
                self.scan(&p)?;
            } else {
                let Ok(rel) = p.strip_prefix(&self.root) else { continue };
                let rel = rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
                if !rel.ends_with(".partial") {
                    self.add_entry(rel, &p, &meta);
                }
            }
        }
        Ok(())
    }

    fn add_entry(&mut self, rel: String, path: &Path, meta: &std::fs::Metadata) {
        let lower = rel.to_ascii_lowercase();
        let stamp = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos() as u64);
        let (mut w, mut h, mut text) = (0, 0, None);
        if lower.ends_with(".png") {
            (w, h) = png_size(path).unwrap_or((0, 0));
        } else if lower.ends_with(".json") {
            text = std::fs::read_to_string(path).ok();
        }
        if lower.ends_with("/seq.json")
            && let Some(t) = &text
        {
            self.confs.insert(rel.clone(), Conf::parse(t));
        }
        let (dir, name) = rel.rsplit_once('/').unwrap_or(("", &rel));
        let names = self.dirs.entry(dir.to_string()).or_default();
        if !names.iter().any(|n| n == name) {
            names.push(name.to_string());
        }
        self.files.insert(rel, PackFile { stamp, w, h, text });
    }

    fn drop_entry(&mut self, rel: &str) {
        self.files.remove(rel);
        self.confs.remove(rel);
        let (dir, name) = rel.rsplit_once('/').unwrap_or(("", rel));
        if let Some(names) = self.dirs.get_mut(dir) {
            names.retain(|n| n != name);
            if names.is_empty() {
                self.dirs.remove(dir);
            }
        }
    }

    /// Writes a pack file (atomic rename) and updates the listing.
    pub fn put(&mut self, rel: &str, data: &[u8]) -> Result<()> {
        write_atomic(&self.abs(rel), data)?;
        self.relist(rel);
        Ok(())
    }

    /// Lists a file again after a worker thread wrote it.
    pub fn relist(&mut self, rel: &str) {
        let p = self.abs(rel);
        self.drop_entry(rel);
        if let Ok(meta) = std::fs::metadata(&p) {
            self.add_entry(rel.to_string(), &p, &meta);
        }
        if rel == "pack.json" {
            self.read_meta();
        }
        self.ver += 1;
    }

    /// Deletes a pack file; one that is not there counts as deleted.
    pub fn del(&mut self, rel: &str) -> Result<()> {
        match std::fs::remove_file(self.abs(rel)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e).with_context(|| rel.to_string()),
            _ => {}
        }
        self.drop_entry(rel);
        self.ver += 1;
        Ok(())
    }

    pub fn abs(&self, rel: &str) -> PathBuf {
        let mut p = self.root.clone();
        p.extend(rel.split('/'));
        p
    }

    pub fn has(&self, rel: &str) -> bool {
        self.files.contains_key(rel)
    }

    pub fn file(&self, rel: &str) -> Option<&PackFile> {
        self.files.get(rel)
    }

    pub fn names_in(&self, dir: &str) -> &[String] {
        self.dirs.get(dir).map_or(&[], Vec::as_slice)
    }

    pub fn name(&self) -> &str {
        self.meta.get("name").and_then(Value::as_str).unwrap_or("")
    }

    /// Replacement pixels per original pixel: `pack.json`'s `scale`, else 1.
    pub fn scale(&self) -> f64 {
        let v = self.meta.get("scale");
        let s = v.and_then(Value::as_f64).or_else(|| v.and_then(Value::as_str).and_then(|t| t.parse().ok()));
        s.filter(|&x| x != 0.0 && x.is_finite()).unwrap_or(1.0)
    }

    /// The pack's default frames per original frame.
    pub fn mult(&self) -> u32 {
        self.meta.get("frame_multiplier").and_then(valid_mult).unwrap_or(1)
    }

    /// `pack.json` with `key` set, written to disk.
    pub fn set_meta(&mut self, key: &str, value: Value) -> Result<()> {
        let mut m = self.meta.clone();
        m.insert("format".into(), 1.into());
        m.insert(key.into(), value);
        self.put("pack.json", Value::Object(m).to_string().as_bytes())
    }

    /// A sequence's stored settings.
    pub fn conf(&self, g: &Group, slot: &Slot, seq: &Seq) -> &Conf {
        self.confs.get(&conf_path(g, slot, seq)).unwrap_or(&NO_CONF)
    }

    /// Writes (or, when empty, deletes) a sequence's `seq.json`.
    pub fn put_conf(&mut self, g: &Group, slot: &Slot, seq: &Seq, conf: &Conf) -> Result<()> {
        let p = conf_path(g, slot, seq);
        if conf.map.is_empty() {
            self.del(&p)
        } else {
            self.put(&p, Value::Object(conf.map.clone()).to_string().as_bytes())
        }
    }

    /// The building data as saved: the spec's original with `building.json` applied. The flag says
    /// `building.json` is not valid.
    pub fn saved_building(&self, g: &Group, slot: &Slot) -> Option<(Building, bool)> {
        let original = slot.building.as_ref()?;
        let mut b = (**original).clone();
        let mut bad = false;
        if let Some(text) = self.file(&bld_path(g, slot)).and_then(|f| f.text.as_deref()) {
            match serde_json::from_str::<Value>(text) {
                Ok(Value::Object(o)) => {
                    for k in BLD_KEYS {
                        if let Some(v) = o.get(k)
                            && !b.set_key(k, v)
                        {
                            bad = true;
                        }
                    }
                }
                _ => bad = true,
            }
        }
        Some((b, bad))
    }

    /// The keys `b` changes from the original into `building.json`, deleted when none are left.
    pub fn put_building(&mut self, g: &Group, slot: &Slot, b: &Building) -> Result<()> {
        let original = slot.building.as_ref().context("not a building")?;
        let mut o = Map::new();
        for k in BLD_KEYS {
            if !b.same_key(original, k) {
                o.insert(k.to_string(), b.key_value(k));
            }
        }
        let p = bld_path(g, slot);
        if o.is_empty() { self.del(&p) } else { self.put(&p, Value::Object(o).to_string().as_bytes()) }
    }

    /// The first file of a music slot: `music/<slot>.mp3|.ogg|.wav`.
    pub fn music_file(&self, slot: &Slot) -> Option<String> {
        MUSIC_EXT.iter().map(|e| format!("music/{}.{e}", slot.id)).find(|p| self.has(p))
    }

    /// Slots with any file in the pack, as `"group/slot"`.
    pub fn slots_with_files(&self) -> std::collections::HashSet<String> {
        let mut set = std::collections::HashSet::new();
        for p in self.files.keys() {
            let a: Vec<&str> = p.split('/').collect();
            match a.as_slice() {
                ["gfx", g, s, _, ..] => {
                    set.insert(format!("{g}/{s}"));
                }
                ["snd", s, _, ..] => {
                    set.insert(format!("sounds/{s}"));
                }
                ["music", f] => {
                    let stem = f.rsplit_once('.').map_or(*f, |(s, _)| s);
                    set.insert(format!("music/{stem}"));
                }
                _ => {}
            }
        }
        set
    }
}

/// Animated sequences take the multiplier; stills, GUI images and terrain pages always 1.
pub fn animated(slot: &Slot, seq: &Seq) -> bool {
    seq.frames.len() > 1 && slot.has_map_anchor()
}

/// Frames of yours per original frame, given the sequence's settings.
pub fn seq_mult(pack: &Pack, slot: &Slot, seq: &Seq, conf: &Conf) -> u32 {
    if !animated(slot, seq) {
        return 1;
    }
    conf.multiplier().unwrap_or_else(|| pack.mult())
}

/// The original frame shown for replacement frame `j`.
pub fn orig_of(seq: &Seq, k: u32, j: usize) -> &Frame {
    &seq.frames[(j / k as usize).min(seq.frames.len() - 1)]
}

/// The size a replacement of `fr` is expected to have.
pub fn expect_size(fr: &Frame, scale: f64) -> [i64; 2] {
    let k = half_k(fr) * scale;
    [js_round(fr[1] as f64 * k), js_round(fr[2] as f64 * k)]
}

/// `Math.round`: halves go up.
pub fn js_round(x: f64) -> i64 {
    (x + 0.5).floor() as i64
}

/// Anchor of replacement frame `j`: the settings' per-frame or common anchor, else the original
/// frame's (left, top) × scale.
pub fn frame_anchor(pack: &Pack, slot: &Slot, seq: &Seq, conf: &Conf, j: usize) -> [f64; 2] {
    conf.anchor_at(j).unwrap_or_else(|| {
        let fr = orig_of(seq, seq_mult(pack, slot, seq, conf), j);
        let k = half_k(fr) * pack.scale();
        [fr[3] as f64 * k, fr[4] as f64 * k]
    })
}

/// Width and height from a PNG's IHDR.
pub fn png_size(p: &Path) -> Option<(u32, u32)> {
    let mut f = std::fs::File::open(p).ok()?;
    let mut b = [0u8; 24];
    f.read_exact(&mut b).ok()?;
    if &b[..8] != b"\x89PNG\r\n\x1a\n" || &b[12..16] != b"IHDR" {
        return None;
    }
    Some((u32::from_be_bytes(b[16..20].try_into().ok()?), u32::from_be_bytes(b[20..24].try_into().ok()?)))
}

pub fn write_atomic(p: &Path, data: &[u8]) -> Result<()> {
    let dir = p.parent().context("no parent")?;
    std::fs::create_dir_all(dir).with_context(|| dir.display().to_string())?;
    let tmp = p.with_extension("partial");
    std::fs::write(&tmp, data).with_context(|| tmp.display().to_string())?;
    std::fs::rename(&tmp, p).with_context(|| p.display().to_string())?;
    Ok(())
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::model::test_spec;

    pub fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("s4rs-studio-{name}-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A valid PNG of `w` × `h` transparent pixels.
    pub fn png_bytes(w: u32, h: u32) -> Vec<u8> {
        crate::pngio::encode_rgba(w, h, &vec![0u8; (w * h * 4) as usize]).unwrap()
    }

    #[test]
    fn pack_json_is_created_and_keeps_unknown_keys() {
        let d = temp("meta");
        let mut p = Pack::open(&d).unwrap();
        assert_eq!((p.name(), p.scale(), p.mult()), ("my pack", 1.0, 1));
        p.put("pack.json", br#"{"format":1,"name":"x","scale":2,"author":"me"}"#).unwrap();
        p.set_meta("frame_multiplier", 3.into()).unwrap();
        let again = Pack::open(&d).unwrap();
        assert_eq!((again.name(), again.scale(), again.mult()), ("x", 2.0, 3));
        assert_eq!(again.meta.get("author").and_then(Value::as_str), Some("me"));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn scale_four_sizes_frames_and_half_frames() {
        let d = temp("scale4");
        let mut p = Pack::open(&d).unwrap();
        p.set_meta("scale", num(4.0)).unwrap();
        let again = Pack::open(&d).unwrap();
        assert_eq!(again.scale(), 4.0);
        let full: Frame = [0, 186, 146, 67, 120, 0, 0, 0];
        let half: Frame = [0, 31, 20, 15, 10, crate::model::F_HALF, 0, 0];
        assert_eq!(expect_size(&full, again.scale()), [744, 584]);
        assert_eq!(expect_size(&half, again.scale()), [62, 40]);
        assert!(SCALES.contains(&4.0));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn files_round_trip_through_the_listing() {
        let d = temp("list");
        let mut p = Pack::open(&d).unwrap();
        let s = test_spec(&d);
        let (g, slot) = s.slot(s.find("settlers-roman/JOB_X").unwrap());
        let seq = &slot.seqs[0];
        p.put(&frame_path(g, slot, seq, 0), &png_bytes(16, 24)).unwrap();
        p.put(&mask_path(g, slot, seq, 0), &png_bytes(16, 24)).unwrap();
        let f = p.file(&frame_path(g, slot, seq, 0)).unwrap();
        assert_eq!((f.w, f.h), (16, 24));
        assert_eq!(p.names_in(&seq_dir(g, slot, seq)).len(), 2);
        let listed = Pack::open(&d).unwrap();
        assert_eq!(listed.file(&mask_path(g, slot, seq, 0)).unwrap().h, 24);
        assert_eq!(listed.slots_with_files().into_iter().collect::<Vec<_>>(), vec!["settlers-roman/JOB_X".to_string()]);
        p.del(&mask_path(g, slot, seq, 0)).unwrap();
        p.del(&frame_path(g, slot, seq, 0)).unwrap();
        p.del(&frame_path(g, slot, seq, 0)).unwrap();
        assert!(p.names_in(&seq_dir(g, slot, seq)).is_empty());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn seq_json_keeps_unknown_keys_and_empty_means_deleted() {
        let d = temp("conf");
        let mut p = Pack::open(&d).unwrap();
        let s = test_spec(&d);
        let (g, slot) = s.slot(s.find("settlers-roman/JOB_X").unwrap());
        let seq = &slot.seqs[0];
        let cp = conf_path(g, slot, seq);
        p.put(&cp, br#"{"note":"mine","frame_multiplier":2,"anchor":[3.5,7]}"#).unwrap();
        let c = p.conf(g, slot, seq).clone();
        assert_eq!(c.multiplier(), Some(2));
        assert_eq!(c.anchor_at(5), Some([3.5, 7.0]));
        assert_eq!(seq_mult(&p, slot, seq, &c), 2);
        let mut c2 = c.clone();
        c2.map.remove("frame_multiplier");
        p.put_conf(g, slot, seq, &c2).unwrap();
        assert_eq!(p.file(&cp).unwrap().text.as_deref(), Some(r#"{"note":"mine","anchor":[3.5,7]}"#));
        p.put_conf(g, slot, seq, &Conf::default()).unwrap();
        assert!(!p.has(&cp) && !p.abs(&cp).exists());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn multipliers_and_numbers_read_like_javascript() {
        assert_eq!(valid_mult(&serde_json::json!(4)), Some(4));
        assert_eq!(valid_mult(&serde_json::json!(4.0)), Some(4));
        for bad in [serde_json::json!(0), serde_json::json!(9), serde_json::json!(1.5), serde_json::json!("2")] {
            assert_eq!(valid_mult(&bad), None, "{bad}");
        }
        assert_eq!(num(7.0).to_string(), "7");
        assert_eq!(num(3.5).to_string(), "3.5");
        assert_eq!(js_round(2.5), 3);
        assert_eq!(js_round(-2.5), -2);
    }

    #[test]
    fn building_json_holds_only_the_changes() {
        let d = temp("bld");
        let mut p = Pack::open(&d).unwrap();
        let s = test_spec(&d);
        let (g, slot) = s.slot(s.find("buildings-roman/BUILDING_MILL").unwrap());
        let (mut b, bad) = p.saved_building(g, slot).unwrap();
        assert!(!bad);
        b.door = [5, 5];
        b.footprint.push([9, 9]);
        p.put_building(g, slot, &b).unwrap();
        let text = p.file(&bld_path(g, slot)).unwrap().text.clone().unwrap();
        assert_eq!(text, r#"{"footprint":[[0,0],[1,0],[9,9]],"door":[5,5]}"#);
        assert_eq!(p.saved_building(g, slot).unwrap().0, b);
        let original = slot.building.as_ref().unwrap();
        p.put_building(g, slot, original).unwrap();
        assert!(!p.has(&bld_path(g, slot)));
        p.put(&bld_path(g, slot), b"{nope").unwrap();
        assert!(p.saved_building(g, slot).unwrap().1);
        std::fs::remove_dir_all(&d).unwrap();
    }
}
