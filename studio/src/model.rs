//! The asset spec as the app reads it: `index.json` and `groups/*.json`, written by [`crate::spec`].

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::spec;

/// `[image, w, h, left, top, flags, strip_x, strip_y]`, see [`spec::FRAME_FIELDS`].
pub type Frame = [i32; 8];

pub const F_TEAM: i32 = spec::F_TEAM as i32;
pub const F_SHADOW: i32 = spec::F_SHADOW as i32;
pub const F_HALF: i32 = spec::F_HALF as i32;
pub const F_RAW: i32 = spec::F_RAW as i32;

/// The simulation's tick; the original shows one frame per tick.
pub const TICK_MS: f64 = 71.0;
pub const GAME_FPS: f64 = 1000.0 / TICK_MS;

/// 0.5 for frames drawn at half size, else 1.
pub fn half_k(fr: &Frame) -> f64 {
    if fr[5] & F_HALF != 0 { 0.5 } else { 1.0 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Gfx,
    Sound,
    Music,
}

#[derive(Debug, Deserialize)]
pub struct Category {
    pub id: String,
    pub label: String,
    pub groups: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct GroupSummary {
    id: String,
}

#[derive(Debug, Deserialize)]
struct Index {
    #[serde(default)]
    game_dir: String,
    categories: Vec<Category>,
    groups: Vec<GroupSummary>,
    #[serde(default)]
    pack_layout: Map<String, Value>,
    #[serde(default)]
    not_covered: Vec<String>,
    #[serde(default)]
    player_colors: Option<Vec<[u8; 3]>>,
}

#[derive(Debug, Deserialize)]
pub struct Group {
    pub id: String,
    pub label: String,
    pub category: String,
    pub kind: Kind,
    #[serde(default)]
    pub file: Option<u32>,
    #[serde(default)]
    pub keep_frames: Option<String>,
    /// Palette offset -> per player the 32 colours of palette indices 224..256 (`rrggbb` × 32).
    #[serde(default)]
    pub team_ramps: HashMap<String, Vec<String>>,
    #[serde(default)]
    pub slots: Vec<Slot>,
}

impl Group {
    /// "frames", "variants" or "tracks".
    pub fn unit(&self) -> &'static str {
        match self.kind {
            Kind::Gfx => "frames",
            Kind::Sound => "variants",
            Kind::Music => "tracks",
        }
    }

    /// Whether the original's strip can be recoloured per player.
    pub fn has_team_ramp(&self, seq: &Seq) -> bool {
        seq.mask.is_some() && seq.pal.is_some_and(|p| self.team_ramps.contains_key(&p.to_string()))
    }
}

/// A sound or similar event on a frame.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SoundRef {
    pub name: String,
    pub frame: i32,
    pub chance: i32,
}

#[derive(Debug, Deserialize)]
pub struct Slot {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub job: Option<i64>,
    #[serde(default)]
    pub job_lists: Vec<String>,
    #[serde(default)]
    pub length_from_frames: Vec<String>,
    #[serde(default)]
    pub sound: Option<SoundRef>,
    #[serde(default)]
    pub volume: Option<i64>,
    #[serde(default)]
    pub uses: Vec<String>,
    #[serde(default)]
    pub note: Option<String>,
    /// Present on GUI images and terrain pages, which have no map anchor (true: exact size).
    #[serde(default)]
    pub exact_size: Option<bool>,
    #[serde(default)]
    pub building: Option<Arc<Building>>,
    #[serde(default)]
    pub seqs: Vec<Seq>,
}

impl Slot {
    /// True when the exact size rule applies to every frame.
    pub fn exact(&self) -> bool {
        self.exact_size == Some(true)
    }

    /// GUI images and terrain pages have no map anchor and no editor.
    pub fn has_map_anchor(&self) -> bool {
        self.exact_size.is_none()
    }
}

#[derive(Debug, Deserialize)]
pub struct Seq {
    pub key: String,
    pub label: String,
    #[serde(default, rename = "ref")]
    pub reference: Option<String>,
    #[serde(default)]
    pub mask: Option<String>,
    #[serde(default)]
    pub pal: Option<i64>,
    #[serde(default)]
    pub frames: Vec<Frame>,
    #[serde(default)]
    pub part_name: Option<String>,
    /// buildingInfo.xml patch: `{type, ticks, slot, sound?}`, kept as it is for the export.
    #[serde(default)]
    pub patch: Option<Value>,
    #[serde(default)]
    pub rate: Option<i64>,
    #[serde(default)]
    pub channels: Option<i64>,
    #[serde(default)]
    pub secs: Option<f64>,
    #[serde(default)]
    pub missing: bool,
}

impl Seq {
    pub fn patch_sound(&self) -> Option<SoundRef> {
        let p = self.patch.as_ref()?.get("sound")?;
        serde_json::from_value(p.clone()).ok()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pile {
    pub cell: [i32; 2],
    pub good: Option<String>,
    pub px: [i32; 2],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Step {
    pub task: String,
    pub cell: [i32; 2],
    pub from: [i32; 2],
    pub to: [i32; 2],
    pub start: i32,
    pub ticks: Option<i32>,
    pub dir: i32,
    pub job: String,
    pub frames: i32,
    pub forward: bool,
    pub visible: bool,
    pub good: Option<String>,
    pub trigger: Option<String>,
    pub sound: Option<SoundRef>,
}

impl Step {
    /// The task without its `CEntityTask::` prefix.
    pub fn task_name(&self) -> &str {
        self.task.strip_prefix("CEntityTask::").unwrap_or(&self.task)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkList {
    pub name: String,
    pub ticks: i32,
    pub steps: Vec<Step>,
}

/// buildingInfo.xml data the art lines up with: cell offsets from the anchor node.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Building {
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub inhabitant: Option<String>,
    #[serde(default)]
    pub footprint: Vec<[i32; 2]>,
    #[serde(default)]
    pub block: Vec<[i32; 2]>,
    #[serde(default)]
    pub dig: Vec<[i32; 2]>,
    pub door: [i32; 2],
    pub flag: [i32; 2],
    #[serde(default)]
    pub work: Option<[i32; 2]>,
    #[serde(default)]
    pub builders: Vec<[i32; 3]>,
    #[serde(default)]
    pub piles: Vec<Pile>,
    #[serde(default)]
    pub mini_flag_px: Option<[i32; 2]>,
    #[serde(default)]
    pub soldiers_px: Vec<[i32; 4]>,
    #[serde(default)]
    pub work_lists: Arc<Vec<WorkList>>,
}

/// Building data the editor can change: keys of the spec's `building` and of `building.json`.
pub const BLD_KEYS: [&str; 10] = ["footprint", "block", "dig", "door", "flag", "work", "builders", "piles", "mini_flag_px", "soldiers_px"];

impl Building {
    /// The value of a [`BLD_KEYS`] key as JSON.
    pub fn key_value(&self, key: &str) -> Value {
        let v = match key {
            "footprint" => serde_json::to_value(&self.footprint),
            "block" => serde_json::to_value(&self.block),
            "dig" => serde_json::to_value(&self.dig),
            "door" => serde_json::to_value(self.door),
            "flag" => serde_json::to_value(self.flag),
            "work" => serde_json::to_value(self.work),
            "builders" => serde_json::to_value(&self.builders),
            "piles" => serde_json::to_value(&self.piles),
            "mini_flag_px" => serde_json::to_value(self.mini_flag_px),
            "soldiers_px" => serde_json::to_value(&self.soldiers_px),
            _ => Ok(Value::Null),
        };
        v.unwrap_or(Value::Null)
    }

    pub fn same_key(&self, other: &Building, key: &str) -> bool {
        match key {
            "footprint" => self.footprint == other.footprint,
            "block" => self.block == other.block,
            "dig" => self.dig == other.dig,
            "door" => self.door == other.door,
            "flag" => self.flag == other.flag,
            "work" => self.work == other.work,
            "builders" => self.builders == other.builders,
            "piles" => self.piles == other.piles,
            "mini_flag_px" => self.mini_flag_px == other.mini_flag_px,
            "soldiers_px" => self.soldiers_px == other.soldiers_px,
            _ => true,
        }
    }

    /// Sets key `key` from `building.json`; false when the value has the wrong shape.
    pub fn set_key(&mut self, key: &str, v: &Value) -> bool {
        fn get<T: serde::de::DeserializeOwned>(v: &Value, to: &mut T) -> bool {
            match serde_json::from_value(v.clone()) {
                Ok(x) => {
                    *to = x;
                    true
                }
                Err(_) => false,
            }
        }
        match key {
            "footprint" => get(v, &mut self.footprint),
            "block" => get(v, &mut self.block),
            "dig" => get(v, &mut self.dig),
            "door" => get(v, &mut self.door),
            "flag" => get(v, &mut self.flag),
            "work" => get(v, &mut self.work),
            "builders" => get(v, &mut self.builders),
            "piles" => get(v, &mut self.piles),
            "mini_flag_px" => get(v, &mut self.mini_flag_px),
            "soldiers_px" => get(v, &mut self.soldiers_px),
            _ => true,
        }
    }

    /// `key` set back to `original`'s value.
    pub fn reset_key(&mut self, original: &Building, key: &str) {
        match key {
            "footprint" => self.footprint = original.footprint.clone(),
            "block" => self.block = original.block.clone(),
            "dig" => self.dig = original.dig.clone(),
            "door" => self.door = original.door,
            "flag" => self.flag = original.flag,
            "work" => self.work = original.work,
            "builders" => self.builders = original.builders.clone(),
            "piles" => self.piles = original.piles.clone(),
            "mini_flag_px" => self.mini_flag_px = original.mini_flag_px,
            "soldiers_px" => self.soldiers_px = original.soldiers_px.clone(),
            _ => {}
        }
    }
}

/// One slot of the spec.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SlotRef {
    pub g: usize,
    pub s: usize,
}

/// The whole spec, read once at start and never changed.
pub struct Spec {
    pub dir: PathBuf,
    /// The game folder the original music is read from.
    pub game: PathBuf,
    pub game_dir_label: String,
    pub categories: Vec<Category>,
    pub groups: Vec<Group>,
    pub pack_layout: Map<String, Value>,
    pub not_covered: Vec<String>,
    pub player_colors: Vec<[u8; 3]>,
    /// Every good name the spec mentions, for the pile inspector's suggestions.
    pub goods: Vec<String>,
    by_id: HashMap<String, usize>,
}

impl Spec {
    pub fn load(dir: &Path, game: &Path) -> Result<Spec> {
        let p = dir.join("index.json");
        let text = std::fs::read(&p).with_context(|| p.display().to_string())?;
        let index: Index = serde_json::from_slice(&text).with_context(|| p.display().to_string())?;
        let groups: Vec<Result<Group>> = std::thread::scope(|s| {
            let handles: Vec<_> = index
                .groups
                .iter()
                .map(|g| {
                    s.spawn(move || {
                        let p = dir.join("groups").join(format!("{}.json", g.id));
                        let text = std::fs::read(&p).with_context(|| p.display().to_string())?;
                        serde_json::from_slice::<Group>(&text).with_context(|| p.display().to_string())
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap_or_else(|_| anyhow::bail!("spec reader panicked"))).collect()
        });
        let groups = groups.into_iter().collect::<Result<Vec<_>>>()?;
        Ok(Spec::new(dir, game, index, groups))
    }

    fn new(dir: &Path, game: &Path, index: Index, groups: Vec<Group>) -> Spec {
        let by_id = groups.iter().enumerate().map(|(i, g)| (g.id.clone(), i)).collect();
        let mut goods = BTreeSet::new();
        for b in groups.iter().flat_map(|g| &g.slots).filter_map(|s| s.building.as_ref()) {
            goods.extend(b.piles.iter().filter_map(|p| p.good.clone()));
            goods.extend(b.work_lists.iter().flat_map(|l| &l.steps).filter_map(|s| s.good.clone()));
        }
        let player_colors = index.player_colors.filter(|c| c.len() == 8).unwrap_or_else(|| spec::PLAYER_RGB.to_vec());
        Spec {
            dir: dir.to_path_buf(),
            game: game.to_path_buf(),
            game_dir_label: index.game_dir,
            categories: index.categories,
            groups,
            pack_layout: index.pack_layout,
            not_covered: index.not_covered,
            player_colors,
            goods: goods.into_iter().collect(),
            by_id,
        }
    }

    /// A spec from values, for tests.
    #[cfg(test)]
    pub fn from_json(dir: &Path, game: &Path, index: Value, groups: Vec<Value>) -> Spec {
        let index: Index = serde_json::from_value(index).expect("index");
        let groups = groups.into_iter().map(|g| serde_json::from_value(g).expect("group")).collect();
        Spec::new(dir, game, index, groups)
    }

    pub fn group_index(&self, id: &str) -> Option<usize> {
        self.by_id.get(id).copied()
    }

    pub fn group(&self, id: &str) -> Option<&Group> {
        self.group_index(id).map(|i| &self.groups[i])
    }

    pub fn slot(&self, r: SlotRef) -> (&Group, &Slot) {
        let g = &self.groups[r.g];
        (g, &g.slots[r.s])
    }

    /// `"group/slot"` to a slot.
    pub fn find(&self, id: &str) -> Option<SlotRef> {
        let (g, s) = id.split_once('/')?;
        let gi = self.group_index(g)?;
        let si = self.groups[gi].slots.iter().position(|x| x.id == s)?;
        Some(SlotRef { g: gi, s: si })
    }

    /// The settler group whose jobs a building group's workers play.
    pub fn settler_group(&self, g: &Group) -> Option<&Group> {
        let id = match g.id.strip_prefix("buildings-") {
            Some(race) => format!("settlers-{race}"),
            None => g.id.clone(),
        };
        self.group(&id)
    }

    /// A `ref` of the spec on disk: the spec folder, or the game's own folder for `game/...`.
    pub fn ref_path(&self, reference: &str) -> PathBuf {
        match reference.strip_prefix("game/") {
            Some(rest) => s4_formats::path::resolve(self.game.join(rest)),
            None => self.dir.join(reference),
        }
    }
}

/// A small spec for tests: a building group, a settler group, a GUI group, sounds and music.
#[cfg(test)]
pub fn test_spec(dir: &Path) -> Spec {
    use serde_json::json;
    let frames = |n: i32, w: i32, h: i32, left: i32, top: i32, flags: i32| -> Vec<Value> {
        (0..n).map(|i| json!([i, w, h, left, top, flags, i * (w + 1), 0])).collect()
    };
    let step = |task: &str, from: [i32; 2], to: [i32; 2], start: i32, ticks: i32| {
        json!({"task": task, "cell": [1, 2], "from": from, "to": to, "start": start, "ticks": ticks, "dir": 1, "job": "JOB_X",
               "frames": 8, "forward": true, "visible": true, "good": "GOOD_PLANK", "trigger": null, "sound": null})
    };
    let building = json!({
        "kind": "PRODUCTION", "inhabitant": "SETTLER_CARRIER",
        "footprint": [[0, 0], [1, 0]], "block": [[0, 1]], "dig": [[1, 1]], "door": [0, 2], "flag": [1, 3], "work": null,
        "builders": [[2, 2, 1]], "piles": [{"cell": [2, 0], "good": "GOOD_PLANK", "px": [5, 6]}],
        "mini_flag_px": [10, -20], "soldiers_px": [[3, 4, 1, 1]],
        "work_lists": [{"name": "WL", "ticks": 20, "steps": [step("CEntityTask::GO_VIRTUAL", [0, 24], [24, 24], 0, 9), step("CEntityTask::WORK_VIRTUAL", [24, 24], [24, 24], 9, 11)]}]
    });
    let index = json!({
        "game_dir": "game", "categories": [
            {"id": "buildings", "label": "Buildings", "groups": ["buildings-roman"]},
            {"id": "settlers", "label": "Settlers", "groups": ["settlers-roman"]},
            {"id": "gui", "label": "Interface", "groups": ["gui-roman"]},
            {"id": "sounds", "label": "Sound effects", "groups": ["sounds"]},
            {"id": "music", "label": "Music", "groups": ["music"]}],
        "groups": [{"id": "buildings-roman"}, {"id": "settlers-roman"}, {"id": "gui-roman"}, {"id": "sounds"}, {"id": "music"}],
        "pack_layout": {"manifest": "pack.json"}, "not_covered": ["Maps"], "player_colors": spec::PLAYER_RGB,
    });
    let ramp = "112233".repeat(32);
    let groups = vec![
        json!({"id": "buildings-roman", "label": "Buildings: Roman", "category": "buildings", "kind": "gfx", "file": 10,
            "keep_frames": "building patches play for their frame count", "team": false,
            "slots": [{"id": "BUILDING_MILL", "name": "BUILDING_MILL", "owner": null, "building": building, "seqs": [
                {"key": "0", "label": "0 construction", "ref": "ref/b/m/0.png", "mask": null, "frames": frames(1, 30, 20, 10, 12, 0), "part_name": "A"},
                {"key": "1", "label": "1 finished", "ref": "ref/b/m/1.png", "mask": null, "frames": frames(1, 40, 30, 16, 24, 2), "part_name": "B"},
                {"key": "2", "label": "2 part", "ref": "ref/b/m/2.png", "mask": null, "frames": frames(3, 8, 8, 0, 0, 0), "part_name": "C",
                 "patch": {"type": "LOOP", "ticks": 3, "slot": 1, "sound": {"name": "SND_MILL", "frame": 1, "chance": 100}}}]}]}),
        json!({"id": "settlers-roman", "label": "Settlers: Roman", "category": "settlers", "kind": "gfx", "file": 20,
            "keep_frames": "s4-sim JobData reads every job", "team": true, "team_ramps": {"512": vec![ramp; 8]},
            "slots": [{"id": "JOB_X", "name": "JOB_X", "owner": "SETTLER_CARRIER", "job": 3, "job_lists": ["L1"], "length_from_frames": ["L1"],
                "sound": {"name": "SND_STEP", "frame": 2, "chance": 50}, "seqs": [
                {"key": "0", "label": "direction 0", "ref": "ref/s/x/0.png", "mask": "ref/s/x/0.team.png", "pal": 512, "frames": frames(4, 16, 24, 8, 20, 1)},
                {"key": "1", "label": "direction 1", "ref": "ref/s/x/1.png", "mask": null, "frames": frames(4, 16, 24, 8, 20, 0)}]}]}),
        json!({"id": "gui-roman", "label": "In-game: Roman", "category": "gui", "kind": "gfx", "file": 9, "keep_frames": null, "team": false,
            "slots": [{"id": "0001", "name": "image 1", "owner": null, "exact_size": true, "uses": ["GUISet dialog 3"], "seqs": [
                {"key": "0", "label": "image", "ref": "ref/g/1/0.png", "mask": null, "frames": frames(1, 64, 32, 0, 0, 8)}]}]}),
        json!({"id": "sounds", "label": "Sound effects", "category": "sounds", "kind": "sound", "file": null, "keep_frames": null, "team": false,
            "slots": [{"id": "SND_STEP", "name": "SND_STEP", "owner": null, "volume": 80, "note": "n", "uses": ["settler job"], "seqs": [
                {"key": "00", "label": "variant 0 (bank slot 1)", "ref": "ref/sounds/SND_STEP/00.wav", "rate": 22050, "channels": 1, "secs": 0.5},
                {"key": "01", "label": "variant 1 (bank slot 2)", "ref": null, "missing": true}]}]}),
        json!({"id": "music", "label": "Music", "category": "music", "kind": "music", "file": null, "keep_frames": null, "team": false,
            "slots": [{"id": "track1", "name": "Track1.mp3", "owner": null, "uses": ["S1"], "seqs": [{"key": "0", "label": "track", "ref": "game/Snd/Track1.mp3"}]}]}),
    ];
    Spec::from_json(dir, dir, index, groups)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spec_reads_every_kind() {
        let s = test_spec(Path::new("spec"));
        assert_eq!(s.groups.len(), 5);
        let r = s.find("buildings-roman/BUILDING_MILL").unwrap();
        let (g, slot) = s.slot(r);
        assert_eq!(g.kind, Kind::Gfx);
        assert!(slot.has_map_anchor());
        assert_eq!(slot.building.as_ref().unwrap().work_lists[0].steps[0].task_name(), "GO_VIRTUAL");
        assert_eq!(s.settler_group(g).unwrap().id, "settlers-roman");
        assert_eq!(s.goods, vec!["GOOD_PLANK"]);
        assert_eq!(s.ref_path("ref/a.png"), Path::new("spec").join("ref/a.png"));
        assert!(s.find("gui-roman/0001").is_some_and(|r| !s.slot(r).1.has_map_anchor()));
        assert!(s.find("nope/x").is_none());
    }

    #[test]
    fn building_keys_round_trip() {
        let s = test_spec(Path::new("spec"));
        let b = s.groups[0].slots[0].building.as_ref().unwrap();
        for k in BLD_KEYS {
            let mut c = (**b).clone();
            assert!(c.set_key(k, &b.key_value(k)), "{k}");
            assert!(c.same_key(b, k), "{k}");
        }
        let mut c = (**b).clone();
        assert!(!c.set_key("door", &serde_json::json!("x")));
        assert!(c.set_key("work", &Value::Null));
    }
}
