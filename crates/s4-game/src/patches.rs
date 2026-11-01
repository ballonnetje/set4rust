//! Building patches: the animated overlays of a finished building (mill sails, a bakery sign,
//! bees, a snoring inhabitant …) and their sounds. Display state, not simulation: the release
//! keeps it in the building role (+0x4c..+0x175) but changes it only while drawing and with the
//! CRT `rand()`, so every client has its own.
//! * **Init** (`InitHousePatches` 0x540bc0, from `IBuildingRole::InitCommon`): the buildingInfo
//!   `patches` (info +0x1f8, ten records of job, slot, type, ticks, sound, chance, frame) fill
//!   their slots. `PERMANENT`: shown at frame 0, animated (looping) when `ticks` ≠ 0. `TIMED`:
//!   shown and animated once, then waits `ticks` steps, then again after `ticks + rand() %
//!   ((ticks + 1) / 2)`. `EVENT`: not shown until a trigger starts it; it then runs `ticks`
//!   frames (0: the animation's frame count, −1: 255) and stops.
//! * **Triggers** (`IBuildingRole::TakeJobTrigger` 0x540da0, `SimEvent::JobTrigger`): a
//!   BuildingTrigger.xml entry with a patch puts it in its slot as an `EVENT` patch (and plays its
//!   sound now if that is on frame 0); without one, 17..=26 start slot n with its own job, 27..=36
//!   stop it, 37..=46 clear it.
//! * **Steps** (`FillGfxInfo` 0x5407d0, while the building is drawn): one step per drawn frame in
//!   which the tick counter moved (`(u16)(tick − building +0x2c)` ≠ 0); Romans and Vikings step
//!   only every third time. A slot whose new frame is its sound frame plays the sound
//!   (`PlayEnvironmentSound` 0x50c310, chance `rand() % 100 ≤ chance` unless 100). An ended
//!   `TIMED` patch ends the step for the later slots (0x540880 returns).
//! * Not ported: the trigger effects (role +0x164.., smoke; their only sound is the lookout
//!   tower's).

use std::collections::HashMap;

use s4_formats::gamedata::GameData;
use s4_viewer::population::{Building, Population};

/// One patch slot (role +0x4c + 8·n: job, frame; +0x9c + 0x14·n: the rest).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Slot {
    /// The gfx job shown (0 = none).
    pub job: u16,
    pub frame: u32,
    /// +0x9c: animating.
    pub active: bool,
    /// +0x9d: `TIMED` wait / `EVENT` frames run.
    count: u8,
    /// +0x9e: `PatchType`.
    mode: u8,
    /// +0x9f: `TIMED` ticks.
    ticks: u8,
    /// +0xa0: the slot's own job.
    pub own_job: u16,
    /// +0xa4: `TIMED` frame count while running (0 = waiting), `EVENT` frames to run.
    frames: u8,
    /// +0xa5: the Roman / Viking step divider.
    sub: u8,
    /// +0xa8 / +0xac / +0xad.
    sound: u16,
    chance: u8,
    sound_frame: u8,
}

/// A building's patches.
#[derive(Clone, Debug, Default)]
pub struct House {
    pub slots: [Slot; 10],
    /// Building +0x2c: the tick of the last step.
    last: u32,
    race: u8,
    /// A mushroom farm: the (patch, tick) of its role last applied ([`Patches::mushroom`]) and
    /// the last pull tick seen ([`Patches::pulls`]).
    mush: (u8, u32),
    pull_seen: Option<i32>,
}

/// A mushroom farm's pull: (mushroom node, farm node, tick).
pub type Pull = ((u16, u16), (u16, u16), u32);

/// A sound due at the building's node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Due {
    pub sound: u16,
    pub x: i32,
    pub y: i32,
}

/// One trigger's patch (BuildingTrigger.xml, table 0x1298cd0 `[race][trigger]`).
#[derive(Clone, Copy, Debug, Default)]
struct Trigger {
    job: u16,
    slot: u8,
    duration: i32,
    sound: u16,
    chance: u8,
    frame: u8,
}

/// The patch records of one building type (info +0x1f8): (job, slot, type, ticks, sound, chance,
/// frame).
type Record = (u16, u8, u8, i32, u16, u8, u8);

/// All buildings' patches and the data they come from.
#[derive(Clone, Debug, Default)]
pub struct Patches {
    pub houses: HashMap<u16, House>,
    records: HashMap<(u8, u8), Vec<Record>>,
    triggers: HashMap<(u8, u8), Trigger>,
    /// `GetJobFrameCount(race + 10, job)` (0x461fe0): frames of each .dil entry of the race's
    /// building file.
    frames: HashMap<u8, Vec<u32>>,
    /// Per race: each building type's first `.dil` entry in its building file (`.jil`,
    /// 0x463a00 via 0x461f00; `None` for a type without its own entry).
    first_dils: HashMap<u8, Vec<Option<u32>>>,
    /// The CRT `rand()` (`MaybeRnd`), seeded like [`crate::sound::JobSoundRoll`].
    rand: u32,
}

impl Patches {
    pub fn new(data: &GameData, seed: u32) -> Patches {
        let mut records: HashMap<(u8, u8), Vec<Record>> = HashMap::new();
        for b in &data.buildings {
            let list: Vec<Record> = b
                .patches
                .iter()
                .filter_map(|p| {
                    let job = p.job?;
                    let s = p.sound.as_ref();
                    Some((
                        job,
                        p.slot.clamp(0, 9) as u8,
                        p.kind.map_or(0, |k| k as u8),
                        p.ticks,
                        s.and_then(|s| s.sound).map_or(0, |s| s as u16),
                        s.map_or(100, |s| s.random.clamp(0, 255) as u8),
                        s.map_or(0, |s| s.frame.clamp(0, 255) as u8),
                    ))
                })
                .collect();
            if !list.is_empty() {
                records.insert((b.race as u8, b.building as u8), list);
            }
        }
        let mut triggers = HashMap::new();
        for t in &data.triggers {
            let Some(job) = t.patch else { continue };
            let s = t.patch_sound.as_ref();
            triggers.insert(
                (t.race as u8, t.index as u8),
                Trigger {
                    job,
                    slot: t.patch_slot.clamp(0, 9) as u8,
                    duration: t.patch_duration,
                    sound: s.and_then(|s| s.sound).map_or(0, |s| s as u16),
                    chance: s.map_or(100, |s| s.random.clamp(0, 255) as u8),
                    frame: s.map_or(0, |s| s.frame.clamp(0, 255) as u8),
                },
            );
        }
        Patches { houses: HashMap::new(), records, triggers, frames: HashMap::new(), first_dils: HashMap::new(), rand: seed }
    }

    /// [`Patches::new`] from the game's data (empty when it cannot be read).
    pub fn load(seed: u32) -> Patches {
        GameData::load_default().inspect_err(|e| tracing::warn!("patches: {e}")).map(|d| Patches::new(&d, seed)).unwrap_or_default()
    }

    /// The frame counts of the race's building file, read from its `.dil` / `.gil`.
    pub fn load_frames(&mut self, race: u8) {
        if self.frames.contains_key(&race) {
            return;
        }
        let counts = building_frame_counts(race as u32 + 10).unwrap_or_else(|e| {
            tracing::warn!("patches: gfx {}: {e}", race as u32 + 10);
            Vec::new()
        });
        self.frames.insert(race, counts);
        let first = building_first_dils(race as u32 + 10).unwrap_or_else(|e| {
            tracing::warn!("patches: gfx {}: {e}", race as u32 + 10);
            Vec::new()
        });
        self.first_dils.insert(race, first);
    }

    /// A mushroom farm's pull patches as its role sets them (`LogicUpdate` 0x5447f0): a pull
    /// (patch 1) starts slot 0 (OPEN: +0x9c = 1, frames +0xa4 = its count, +0x9d / +0x9f / +0xa5
    /// / frame = 0); its arrival (patch 2) stops slot 0 and starts slot 1 (FLOW) the same way;
    /// the sphere counted (patch 0) stops slot 1. Both are `PERMANENT` patches with 0 ticks, so
    /// they show their last frame while stopped.
    pub fn mushroom(&mut self, id: u16, m: &s4_viewer::population::MushFarm) {
        let race = match self.houses.get(&id) {
            Some(h) if h.mush != (m.patch, m.patch_tick) => h.race,
            _ => return,
        };
        let counts = [0, 1].map(|n| self.houses.get(&id).map_or(1, |h| self.frame_count(race, h.slots[n].job)));
        let Some(h) = self.houses.get_mut(&id) else { return };
        h.mush = (m.patch, m.patch_tick);
        for (n, on) in [(0, m.patch == 1), (1, m.patch == 2)] {
            let s = &mut h.slots[n];
            if on {
                s.active = true;
                s.frames = counts[n].min(255) as u8;
                s.count = 0;
                s.ticks = 0;
                s.sub = 0;
                s.frame = 0;
            } else {
                s.active = false;
            }
        }
    }

    /// The mushroom farms' pulls since the last call (`CEffects` vtbl+0x1c 0x4e3b90 from the
    /// pull 0x544b40): (mushroom node, farm node, tick). A farm's first sight only records its
    /// last pull.
    pub fn pulls(&mut self, pop: &Population) -> Vec<Pull> {
        let mut out = Vec::new();
        for b in pop.buildings.iter().filter(|b| b.id != 0 && b.site.is_none()) {
            let (Some(m), Some(h)) = (&b.mushroom, self.houses.get_mut(&b.id)) else { continue };
            if h.pull_seen.is_some_and(|t| t != m.last_pull) && m.last_pull > 0 {
                out.push((m.pull_from, (b.x, b.y), m.last_pull as u32));
            }
            h.pull_seen = Some(m.last_pull);
        }
        out
    }

    /// `FillGfxInfo` 0x544ac0 after the step: slot 2 (GROW) shows its own job at frame
    /// `spheres − 1`, nothing without spheres.
    pub fn grow(&mut self, id: u16, spheres: i32) {
        let Some(h) = self.houses.get_mut(&id) else { return };
        let s = &mut h.slots[2];
        if spheres == 0 {
            s.job = 0;
        } else {
            s.job = s.own_job;
            s.frame = (spheres - 1) as u32;
        }
    }

    /// An animal or donkey ranch's pen (FX slot 0): `CAnimalRanchRole::Init` (0x533f50) and its
    /// `LogicUpdate` (0x533ff0, when the count changes) set the slot's .dil (+0x4c) to the
    /// building type's first .dil + `animals` + 1, active (+0x9c) in mode 0 (+0x9e); the frame
    /// (+0x50) is kept. It then loops like a `PERMANENT` patch ([`Patches::step`]).
    pub fn pen(&mut self, id: u16, kind: u8, animals: u8) {
        let Some(h) = self.houses.get(&id) else { return };
        let Some(first) = self.first_dils.get(&h.race).and_then(|f| f.get(kind as usize).copied().flatten()) else { return };
        let job = (first + animals as u32 + 1) as u16;
        let Some(h) = self.houses.get_mut(&id) else { return };
        let s = &mut h.slots[0];
        s.job = job;
        s.active = true;
        s.mode = 0;
    }

    #[cfg(test)]
    fn set_frames(&mut self, race: u8, counts: Vec<u32>) {
        self.frames.insert(race, counts);
    }

    fn frame_count(&self, race: u8, job: u16) -> u32 {
        self.frames.get(&race).and_then(|f| f.get(job as usize)).copied().unwrap_or(1)
    }

    fn rand(&mut self) -> u32 {
        self.rand = self.rand.wrapping_mul(214_013).wrapping_add(2_531_011);
        (self.rand >> 16) & 0x7fff
    }

    /// `InitHousePatches` (0x540bc0) for a building that became a finished one.
    pub fn init(&mut self, id: u16, race: u8, kind: u8) {
        self.load_frames(race);
        let mut h = House { race, ..Default::default() };
        for &(job, slot, mode, ticks, sound, chance, sound_frame) in self.records.get(&(race, kind)).map_or(&[][..], |r| r) {
            let s = &mut h.slots[slot as usize];
            *s = Slot { own_job: job, mode, sound, chance, sound_frame, ..Default::default() };
            match mode {
                0 => {
                    s.job = job;
                    s.active = ticks != 0;
                }
                1 => {
                    s.job = job;
                    s.count = ticks as u8;
                    s.ticks = ticks as u8;
                    s.active = true;
                }
                2 => s.frames = self.duration(race, job, ticks),
                _ => {}
            }
        }
        self.houses.insert(id, h);
    }

    /// An `EVENT` patch's frames: the given count, 0 = the animation's, −1 = 255.
    fn duration(&self, race: u8, job: u16, d: i32) -> u8 {
        match d {
            0 => self.frame_count(race, job).min(255) as u8,
            -1 => 255,
            d => d as u8,
        }
    }

    /// `TakeJobTrigger` (0x540da0) on building `id` at node (x, y). Returns the sound due now.
    pub fn trigger(&mut self, id: u16, trigger: u8, x: i32, y: i32) -> Option<Due> {
        let race = self.houses.get(&id)?.race;
        let t = self.triggers.get(&(race, trigger)).copied();
        let Some(t) = t else {
            let h = self.houses.get_mut(&id)?;
            match trigger {
                17..=26 => {
                    let s = &mut h.slots[trigger as usize - 17];
                    s.job = s.own_job;
                    s.active = true;
                }
                27..=36 => h.slots[trigger as usize - 27].active = false,
                37..=46 => {
                    let s = &mut h.slots[trigger as usize - 37];
                    s.job = 0;
                    s.frame = 0;
                    s.active = false;
                    s.count = 0;
                }
                _ => {}
            }
            return None;
        };
        let frames = self.duration(race, t.job, t.duration);
        let roll = (t.sound > 0 && t.frame == 0 && (t.chance == 100 || self.rand() % 100 <= t.chance as u32)).then_some(Due { sound: t.sound, x, y });
        let h = self.houses.get_mut(&id)?;
        let s = &mut h.slots[t.slot as usize];
        s.job = t.job;
        s.frame = 0;
        s.own_job = t.job;
        s.mode = 2;
        s.sound = t.sound;
        s.sound_frame = t.frame;
        s.chance = t.chance;
        s.frames = frames;
        s.active = true;
        s.count = 0;
        roll
    }

    /// `FillGfxInfo`'s patch part (0x5407d0) for drawn building `id` at node (x, y) on tick
    /// `tick`: the sounds due.
    pub fn step(&mut self, id: u16, tick: u32, x: i32, y: i32) -> Vec<Due> {
        let mut due = Vec::new();
        let Some(h) = self.houses.get(&id) else { return due };
        if (tick.wrapping_sub(h.last) & 0xffff) == 0 {
            return due;
        }
        let race = h.race;
        let mut h = self.houses.remove(&id).unwrap_or_default();
        h.last = tick;
        for n in 0..10 {
            let mut s = h.slots[n];
            if !s.active {
                continue;
            }
            if race <= 1 {
                s.sub += 1;
                if s.sub < 3 {
                    h.slots[n] = s;
                    continue;
                }
                s.sub = 0;
            }
            let mut stop = false;
            let advance = match s.mode {
                0 => true,
                1 if s.frames == 0 => {
                    if s.count == 0 {
                        s.active = true;
                        s.frames = self.frame_count(race, s.job).min(255) as u8;
                        s.frame = 0;
                    } else {
                        s.count -= 1;
                    }
                    false
                }
                1 => {
                    let c = self.frame_count(race, s.job);
                    s.frame += 1;
                    if c <= s.frame {
                        s.frame = 0;
                        if s.ticks != 0 {
                            let half = (s.ticks as u32).div_ceil(2);
                            s.frames = 0;
                            s.count = (self.rand() % half) as u8 + s.ticks;
                        }
                        stop = true;
                    }
                    false
                }
                2 if s.frames != 0 => {
                    s.count += 1;
                    if s.count < s.frames {
                        true
                    } else {
                        s.active = false;
                        false
                    }
                }
                _ => false,
            };
            if advance {
                let c = self.frame_count(race, s.job).max(1);
                s.frame = (s.frame + 1) % c;
            }
            h.slots[n] = s;
            if stop {
                break;
            }
            if s.sound > 0 && s.sound_frame as u32 == s.frame && (s.chance == 100 || self.rand() % 100 <= s.chance as u32) {
                due.push(Due { sound: s.sound, x, y });
            }
        }
        self.houses.insert(id, h);
        due
    }

    /// Buildings gone or no longer finished.
    pub fn retain(&mut self, mut keep: impl FnMut(u16) -> bool) {
        self.houses.retain(|&id, _| keep(id));
    }

    /// One drawn frame: buildings gone or no longer finished drop their patches, a finished one
    /// seen for the first time gets `InitHousePatches`, and one that is drawn (`drawn(x, y)` of
    /// its node) takes its `FillGfxInfo` step. Returns the sounds due.
    pub fn draw(&mut self, pop: &Population, tick: u32, drawn: impl Fn(i32, i32) -> bool) -> Vec<Due> {
        let finished = || pop.buildings.iter().filter(|b| b.id != 0 && b.site.is_none());
        let ids: std::collections::HashSet<u16> = finished().map(|b| b.id).collect();
        self.retain(|id| ids.contains(&id));
        let mut due = Vec::new();
        for b in finished() {
            self.ensure(b);
            if let Some(animals) = b.pen {
                self.pen(b.id, b.kind, animals);
            }
            if let Some(m) = &b.mushroom {
                self.mushroom(b.id, m);
            }
            if drawn(b.x as i32, b.y as i32) {
                due.extend(self.step(b.id, tick, b.x as i32, b.y as i32));
                if let Some(m) = &b.mushroom {
                    self.grow(b.id, m.spheres);
                }
            }
        }
        due
    }

    /// `SimEvent::JobTrigger`: `TakeJobTrigger` on a finished building of `pop`.
    pub fn job_trigger(&mut self, pop: &Population, building: u16, trigger: u8) -> Option<Due> {
        let b = pop.buildings.iter().find(|b| b.id == building && b.site.is_none())?;
        self.ensure(b);
        self.trigger(building, trigger, b.x as i32, b.y as i32)
    }

    fn ensure(&mut self, b: &Building) {
        if !self.houses.contains_key(&b.id) {
            self.init(b.id, b.race, b.kind);
        }
    }

    /// What each building shows: its race and per slot `(job, frame)` (job 0: nothing). The
    /// role hands every slot with a job to the gfx info (0x4621c0, slots +0x20..+0x44), shown
    /// or not, animating or not.
    pub fn shown(&self) -> s4_viewer::sprites::ShownPatches {
        self.houses.iter().map(|(&id, h)| (id, (h.race, h.slots.map(|s| (s.job, s.frame))))).collect()
    }
}

/// `GetJobFrameCount` for a building file (0x461fe0, files 10..=19): per `.dil` entry, the images
/// up to the next entry that starts elsewhere.
pub fn building_frame_counts(n: u32) -> anyhow::Result<Vec<u32>> {
    use s4_formats::gfx::container::{offsets, to_indices};
    let dir = s4_formats::path::resolve(s4_formats::game_dir().join("Gfx"));
    let gil = offsets(&std::fs::read(dir.join(format!("{n}.gil")))?, "gil")?;
    let dil = to_indices(&offsets(&std::fs::read(dir.join(format!("{n}.dil")))?, "dil")?, gil.len(), "dil")?;
    let mut bounds = dil.clone();
    bounds.sort_unstable();
    bounds.dedup();
    Ok(dil.iter().map(|&s| bounds.iter().find(|&&b| b > s).map_or(gil.len() as u32, |&b| b) - s).collect())
}

/// The first `.dil` entry of each job (building type) of a building file (`.jil`); a type whose
/// entry equals entry 0's has none of its own (0x461f00).
pub fn building_first_dils(n: u32) -> anyhow::Result<Vec<Option<u32>>> {
    use s4_formats::gfx::container::{offsets, to_indices};
    let dir = s4_formats::path::resolve(s4_formats::game_dir().join("Gfx"));
    let dil = offsets(&std::fs::read(dir.join(format!("{n}.dil")))?, "dil")?;
    let jil = to_indices(&offsets(&std::fs::read(dir.join(format!("{n}.jil")))?, "jil")?, dil.len(), "jil")?;
    Ok(jil.iter().enumerate().map(|(i, &j)| (i == 0 || j != jil[0]).then_some(j)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// User report: no fisher nets, no weathercock. `TIMED` patches are shown from the start
    /// (frame 0) and the Roman big residence's weathercock / fisher hut's net are slot 5.
    #[test]
    fn timed_patches_are_shown_from_the_start() {
        if !s4_formats::game_dir().join("Gfx").exists() {
            return;
        }
        let mut p = Patches::load(1);
        p.init(1, 0, 42);
        p.init(2, 1, 6);
        let s = p.shown();
        assert_eq!(s[&1].1[5], (146, 0), "Roman RESIDENCEBIG_WEATHERCOCK");
        assert_eq!(s[&1].1[6], (147, 0), "Roman RESIDENCEBIG_DOOR");
        assert_eq!(s[&2].1[5], (17, 0), "Viking FISHERHUT_MOVENET");
        assert!(p.frame_count(0, 146) > 1 && p.frame_count(1, 17) > 1, "{} {}", p.frame_count(0, 146), p.frame_count(1, 17));
    }

    fn patches() -> Patches {
        let mut p = Patches::default();
        // Race 2 (no divider): slot 0 permanent looping job 5 (4 frames, sound 9 on frame 2),
        // slot 1 timed job 6 (3 frames, ticks 4), slot 2 event job 7 (2 frames).
        p.records.insert((2, 1), vec![(5, 0, 0, 1, 9, 100, 2), (6, 1, 1, 4, 0, 100, 0), (7, 2, 2, 0, 0, 100, 0)]);
        p.set_frames(2, vec![1, 1, 1, 1, 1, 4, 3, 2]);
        p.triggers.insert((2, 3), Trigger { job: 7, slot: 3, duration: 0, sound: 11, chance: 100, frame: 0 });
        p
    }

    #[test]
    fn permanent_loops_and_sounds_on_its_frame() {
        let mut p = patches();
        p.init(1, 2, 1);
        assert!(p.step(1, 0, 3, 4).is_empty(), "tick unchanged since +0x2c (0)");
        assert!(p.step(1, 1, 3, 4).is_empty());
        assert_eq!(p.step(1, 2, 3, 4), vec![Due { sound: 9, x: 3, y: 4 }]);
        assert!(p.step(1, 2, 3, 4).is_empty(), "same tick: no step");
        p.step(1, 3, 3, 4);
        p.step(1, 4, 3, 4);
        assert_eq!(p.houses[&1].slots[0].frame, 0, "4 frames: back to 0");
    }

    #[test]
    fn timed_waits_runs_and_waits_again() {
        let mut p = patches();
        p.init(1, 2, 1);
        let f = |p: &Patches| (p.houses[&1].slots[1].frame, p.houses[&1].slots[1].frames, p.houses[&1].slots[1].count);
        assert_eq!(f(&p), (0, 0, 4));
        for t in 1..=4 {
            p.step(1, t, 0, 0);
        }
        assert_eq!(f(&p), (0, 0, 0));
        p.step(1, 5, 0, 0);
        assert_eq!(f(&p), (0, 3, 0), "started");
        p.step(1, 6, 0, 0);
        p.step(1, 7, 0, 0);
        assert_eq!(f(&p).0, 2);
        p.step(1, 8, 0, 0);
        let (frame, frames, wait) = f(&p);
        assert_eq!((frame, frames), (0, 0));
        assert!((4..=5).contains(&wait), "4 + rand() % 2");
    }

    #[test]
    fn event_slots_and_triggers() {
        let mut p = patches();
        p.init(1, 2, 1);
        assert_eq!(p.houses[&1].slots[2].job, 0, "an event patch is hidden");
        // START_SLOT2 (19) shows it with its own job; it runs its 2 frames and stops.
        assert_eq!(p.trigger(1, 19, 0, 0), None);
        assert_eq!(p.houses[&1].slots[2].job, 7);
        assert!(p.houses[&1].slots[2].active);
        p.step(1, 1, 0, 0);
        p.step(1, 2, 0, 0);
        assert!(!p.houses[&1].slots[2].active);
        // CLEAR_SLOT2 (39) hides it.
        p.trigger(1, 39, 0, 0);
        assert_eq!(p.houses[&1].slots[2].job, 0);
        // A trigger with a patch: slot 3, sound on frame 0 at once.
        assert_eq!(p.trigger(1, 3, 5, 6), Some(Due { sound: 11, x: 5, y: 6 }));
        assert_eq!(p.houses[&1].slots[3].job, 7);
    }

    #[test]
    fn romans_step_every_third_time() {
        let mut p = patches();
        p.records.insert((0, 1), vec![(5, 0, 0, 1, 0, 100, 0)]);
        p.set_frames(0, vec![1, 1, 1, 1, 1, 4]);
        p.init(1, 0, 1);
        for t in 1..=5 {
            p.step(1, t, 0, 0);
        }
        assert_eq!(p.houses[&1].slots[0].frame, 1);
        p.step(1, 6, 0, 0);
        assert_eq!(p.houses[&1].slots[0].frame, 2);
    }

    /// The shipped data: a Roman big residence's TIMED door (`SOUND_WORK_MELT_DOOR` on frame
    /// 5, chance 50 — prints the rate), a waterworks' PERMANENT bucket (`SOUND_WORK_GETWATER`).
    #[test]
    fn shipped_patches_sound() {
        let Ok(data) = GameData::load_default() else { return };
        if !s4_formats::game_dir().join("Gfx").join("10.dil").exists() {
            return;
        }
        let mut p = Patches::new(&data, 1);
        p.init(1, 0, 42);
        p.init(2, 0, 5);
        let mut n = [0; 2];
        for t in 1..=3000 {
            for (k, id) in [1u16, 2].into_iter().enumerate() {
                n[k] += p.step(id, t, 0, 0).len();
            }
        }
        println!("3000 steps: residence {} door sounds, waterworks {} bucket sounds; slots {:?}", n[0], n[1], p.houses[&2].slots.iter().filter(|s| s.job != 0).collect::<Vec<_>>());
        assert!(n[0] > 0);
    }

    /// The ranch pen's .dil: the type's first .dil (the building file's `.jil`) + animals + 1, kept
    /// in slot 0 and looping; the frame carries over when the count changes.
    #[test]
    fn ranch_pen_takes_slot_zero() {
        if !s4_formats::game_dir().join("Gfx").join("10.gfx").exists() {
            return;
        }
        let first = s4_formats::gfx::GfxFile::open_default(10).unwrap().job_dirs(12).unwrap().start as u32;
        assert_eq!(building_first_dils(10).unwrap()[12], Some(first));
        let mut p = Patches::default();
        p.load_frames(0);
        p.init(7, 0, 12);
        p.pen(7, 12, 3);
        assert_eq!(p.houses[&7].slots[0].job as u32, first + 4);
        for t in 1..=9 {
            p.step(7, t, 0, 0);
        }
        let f = p.houses[&7].slots[0].frame;
        assert_eq!(f, 3 % p.frame_count(0, (first + 4) as u16), "Romans: every third step");
        p.pen(7, 12, 4);
        assert_eq!((p.houses[&7].slots[0].job as u32, p.houses[&7].slots[0].frame), (first + 5, f));
    }

    #[test]
    fn shipped_building_files_have_frame_counts() {
        if !s4_formats::game_dir().join("Gfx").join("10.dil").exists() {
            return;
        }
        let c = building_frame_counts(10).unwrap();
        assert!(c.len() > 50 && c.iter().any(|&n| n > 4), "{}", c.len());
    }
}
