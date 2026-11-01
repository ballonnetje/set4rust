//! `GameData\jobInfo.xml`, `GameData\jobSoundList.xml` and `GameData\effectsInfo.xml`.
//!
//! Reader: `CEntityToDoListMgr::CEntityToDoListMgr` (0x154c610,
//! MapObjects/CEntityToDoListMgr.cpp). It keeps one `std::list<CEntityTask>` per
//! `[race][anim list]` (5 x 267).
//!
//! Before reading the XML, the constructor fills implicit lists for every settler type `k`
//! (1..=66) whose gfx file has a first job (`CGfxManager::GetSettlerFirstJob`):
//! * list `k`: one `WAIT` task (17) playing the type's first gfx job for its full frame count;
//! * list `66+k` (= JOB_<TYPE>_CHECKIN): `GO_TO_TARGET` (10) + `CHECKIN` (24) with the same job.
//!
//! jobInfo.xml then defines `race/job` lists (`job@id` = JOB_*, resolved via
//! `CConfigManager::GetDefineValue`), each a sequence of `node`s turned into
//! `CEntityTask(task, jobPart, x, y, duration, frames, dir, forward, visible, entity, 0, 0, trigger)`:
//! * `task` — `CEntityTask::*` ([`TaskType`]).
//! * `jobPart` — settler **gfx job** name ([`SETTLER_JOB_NAMES`](super::defines::SETTLER_JOB_NAMES)),
//!   i.e. the job index in the race's settler gfx file 20+race.
//! * `x`, `y` — cell offset of the node (relative to the building/target).
//! * `dir` — direction 0..5 to face, -1 = keep current (the frame count is taken for dir 0 then).
//! * `duration` — ticks, must be < 128; 0 means "the gfx job's frame count".
//! * `entity` — GOOD_* (or other define) carried/produced; unknown → 0.
//! * `trigger` — TRIGGER_* fired on this node (see BuildingTrigger.xml), via `CDefineTranslator`.
//! * `forward`, `visible` — booleans (`atoi != 0`): play frames forwards / settler visible.
//! * `useWork` — present in the file but never read by the game.
//!
//! jobSoundList.xml: `job@id` = settler gfx job (1..367), `sound@id` = SOUND_*, `sound/frame` =
//! frame on which it plays, `sound/random` = chance in percent (0 → 100). Stored in
//! `CEntityToDoListMgr::m_vJobSoundInfo[367]` (`GetJobSoundInfo` 0x154e2a0).
//!
//! effectsInfo.xml is **not loaded** by this S4_Main.exe: the exe contains no `effectsInfo.xml`
//! path (the jobSoundList reader merely reuses the error string "XML - Parsing error in
//! EffectsInfo.xml"). It is parsed here generically for completeness.

use super::defines::{AnimList, Race, Sound, TaskType};
use super::xml::{atoi, child, child_int, child_text, children, elements, id, text};
use super::Ctx;
use crate::Result;

/// One `node` of a job list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobNode {
    pub task: Option<TaskType>,
    pub task_name: String,
    /// Settler gfx job index (into the race's settler gfx file).
    pub job_part: Option<u16>,
    pub job_part_name: String,
    pub x: i32,
    pub y: i32,
    /// Ticks; 0 = the gfx job's frame count.
    pub duration: i32,
    /// 0..=5, or -1 = keep direction.
    pub dir: i32,
    pub forward: bool,
    pub visible: bool,
    /// Define value of `entity` (a GOOD_* in the shipped file); 0 if empty/unknown.
    pub entity: i32,
    pub entity_name: String,
    /// TRIGGER_* table slot, if any.
    pub trigger: Option<i32>,
    pub trigger_name: String,
    /// `useWork` (never read by the game).
    pub use_work_unread: Option<bool>,
}

/// One `race/job` of jobInfo.xml.
#[derive(Clone, Debug)]
pub struct JobList {
    pub race: Race,
    pub list: Option<AnimList>,
    pub name: String,
    pub nodes: Vec<JobNode>,
}

pub(crate) fn parse_jobs(text_: &str, ctx: &mut Ctx) -> Result<Vec<JobList>> {
    let doc = roxmltree::Document::parse(text_).map_err(|e| crate::Error::Format(format!("jobInfo.xml: {e}")))?;
    let mut out = Vec::new();
    for race_node in children(doc.root_element(), "race") {
        let Some(race) = ctx.translator(id(race_node), Race::from_name, Race::from_u16, "race") else { continue };
        for j in children(race_node, "job") {
            let name = id(j).to_string();
            let list = ctx.enum_by_define(&name, AnimList::from_u16, "job list");
            let nodes = children(j, "node")
                .map(|n| {
                    let task_name = child_text(n, "task").to_string();
                    let job_part_name = child_text(n, "jobPart").to_string();
                    let entity_name = child_text(n, "entity").to_string();
                    let trigger_name = child_text(n, "trigger").to_string();
                    let use_work = child(n, "useWork", 0).map(|u| text(u) == "true");
                    JobNode {
                        task: ctx.translator(&task_name, TaskType::from_name, TaskType::from_u16, "task"),
                        job_part: ctx.define(&job_part_name, "jobPart").map(|v| v as u16),
                        x: child_int(n, "x"),
                        y: child_int(n, "y"),
                        duration: child_int(n, "duration"),
                        dir: child_int(n, "dir"),
                        forward: child_int(n, "forward") != 0,
                        visible: child_int(n, "visible") != 0,
                        entity: if entity_name.is_empty() { 0 } else { ctx.define(&entity_name, "entity").unwrap_or(0) },
                        trigger: if trigger_name.is_empty() {
                            None
                        } else {
                            let t = super::defines::TRIGGERS.iter().find(|(t, _)| *t == trigger_name).map(|&(_, v)| v);
                            if t.is_none() {
                                ctx.warn(format!("jobInfo: unknown trigger {trigger_name}"));
                            }
                            t
                        },
                        use_work_unread: use_work,
                        task_name,
                        job_part_name,
                        entity_name,
                        trigger_name,
                    }
                })
                .collect();
            out.push(JobList { race, list, name, nodes });
        }
    }
    Ok(out)
}

/// One entry of jobSoundList.xml.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobSound {
    /// Settler gfx job index.
    pub job: u16,
    pub job_name: String,
    pub sound: Option<Sound>,
    pub frame: i32,
    /// Percent (0 in the file → 100).
    pub random: i32,
}

pub(crate) fn parse_job_sounds(text_: &str, ctx: &mut Ctx) -> Result<Vec<JobSound>> {
    let doc = roxmltree::Document::parse(text_).map_err(|e| crate::Error::Format(format!("jobSoundList.xml: {e}")))?;
    let mut out = Vec::new();
    for j in children(doc.root_element(), "job") {
        let job_name = id(j).to_string();
        let Some(job) = ctx.define(&job_name, "job sound job") else { continue };
        let s = child(j, "sound", 0);
        let random = s.map(|s| child_int(s, "random")).unwrap_or(0);
        out.push(JobSound {
            job: job as u16,
            sound: s.and_then(|s| ctx.enum_by_define(id(s), Sound::from_u16, "job sound")),
            frame: s.map(|s| child_int(s, "frame")).unwrap_or(0),
            random: if random == 0 { 100 } else { random },
            job_name,
        });
    }
    Ok(out)
}

/// A generic `<x id="...">` item of effectsInfo.xml with its integer children.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectsItem {
    /// Element name: `sound`, `effect` or `patch`.
    pub kind: String,
    pub id: String,
    pub frame: Option<i32>,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub duration: Option<i32>,
    /// `effect/building@id`, if present.
    pub building: Option<String>,
}

/// One `job` of effectsInfo.xml (file not used by the game, see module docs).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectsEntry {
    pub job: String,
    /// `(race id or "" for items directly under job, item)`.
    pub items: Vec<(String, EffectsItem)>,
}

pub(crate) fn parse_effects(text_: &str) -> Result<Vec<EffectsEntry>> {
    let doc = roxmltree::Document::parse(text_).map_err(|e| crate::Error::Format(format!("effectsInfo.xml: {e}")))?;
    let opt = |n: roxmltree::Node, f: &str| child(n, f, 0).map(|c| atoi(text(c)));
    let item = |n: roxmltree::Node| EffectsItem {
        kind: n.tag_name().name().to_string(),
        id: id(n).to_string(),
        frame: opt(n, "frame"),
        x: opt(n, "x"),
        y: opt(n, "y"),
        duration: opt(n, "duration"),
        building: child(n, "building", 0).map(|b| id(b).to_string()),
    };
    let mut out = Vec::new();
    for j in children(doc.root_element(), "job") {
        let mut items = Vec::new();
        for c in elements(j) {
            if c.has_tag_name("race") {
                for i in elements(c) {
                    items.push((id(c).to_string(), item(i)));
                }
            } else {
                items.push((String::new(), item(c)));
            }
        }
        out.push(EffectsEntry { job: id(j).to_string(), items });
    }
    Ok(out)
}
