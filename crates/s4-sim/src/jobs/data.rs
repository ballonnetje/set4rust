//! Static job data: `CEntityToDoListMgr` (the to-do lists per race and list id, job sounds) and
//! the parts of `CGfxManager` the job code reads (first gfx job per settler type, frame counts).
//! Built once; part of [`crate::Rules`].

use super::task::{EntityTask, task_type};
use s4_formats::gamedata::{GameData, Race, Settler};
use s4_formats::gfx::GfxFile;
use std::sync::Arc;

pub const RACES: usize = 5;
/// `JOB_MAX` + 1 (list ids 0..=266).
pub const JOB_LIST_MAX: usize = 267;
/// `SIV_MAX_JOB` (settler gfx jobs 1..366).
pub const SIV_MAX_JOB: usize = 367;
/// Settler types 0..66.
pub const SETTLER_TYPES: usize = 67;

/// `SJobSoundInfo` (`CEntityToDoListMgr::m_vJobSoundInfo[job]`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct JobSoundInfo {
    /// `SIV_SOUNDS` id, 0 = none.
    pub sound: u16,
    /// +4: chance in percent (100 = always).
    pub random: u8,
    /// +5: animation frame the sound plays on.
    pub frame: u8,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JobData {
    /// The list objects. Slots of different races may point at the same object (see [`JobData::new`]).
    lists: Vec<Arc<[EntityTask]>>,
    /// `[race * JOB_LIST_MAX + list]` → index into `lists`.
    slots: Vec<Option<u32>>,
    /// `CGfxManager::GetSettlerFirstJob(race, type)`.
    first_job: Vec<[u16; SETTLER_TYPES]>,
    /// `GetSettlerJobFrameCount(race, job, dir)`: `[race][job][dir]`.
    frames: Vec<Vec<[u8; 6]>>,
    /// `SettlerInfo::m_vAnimLists` per `[race][type]` (Roman fallback as `CSettlerMgr::LoadInfo`).
    anim_lists: Vec<Vec<Vec<u16>>>,
    /// `SettlerInfo::m_vSearchInfos` search types (`SEARCH_*`) per `[race][type]`.
    searches: Vec<Vec<Vec<u16>>>,
    /// `SettlerInfo::m_uRole` (`CSettlerMgr::CreateSettlerRole`) per `[race][type]`.
    roles: Vec<Vec<u8>>,
    /// `SettlerInfo` +6 `other` (`CDecoObjMgr::*` kind a planter plants) per `[race][type]`.
    others: Vec<Vec<u8>>,
    sounds: Vec<JobSoundInfo>,
}

impl JobData {
    /// `CEntityToDoListMgr::CEntityToDoListMgr` (release 0x5674c0; the object at 0x14540c0). `gfx[race]` is `Gfx\<20 + race>.gfx`.
    ///
    /// 1. For every race and settler type 1..66 with a first gfx job: list `type` = one WAIT task
    ///    (that job, duration 1, frame count of direction 2). If race 0 already has the list, the
    ///    other races **share race 0's list object** (so they wait with race 0's gfx job index).
    /// 2. The same for list `66 + type` (1..65) = GO_TO_TARGET + CHECKIN (the `*_CHECKIN` lists).
    /// 3. Every `<node>` of jobInfo.xml is **appended** to the list object of its `race/job` slot,
    ///    created when the slot is empty. Nodes therefore follow an implicit list's tasks, and a
    ///    node appended through a shared slot lands in race 0's list too. Reproduced as is.
    pub fn new(data: &GameData, gfx: &[&GfxFile]) -> JobData {
        let frame_count = |race: usize, job: usize, dir: usize| -> u8 {
            gfx.get(race).and_then(|g| g.job_frames(job, dir)).map_or(0, |r| r.len() as u8)
        };
        let mut first_job = vec![[0u16; SETTLER_TYPES]; RACES];
        let mut frames = vec![Vec::new(); RACES];
        for race in 0..RACES {
            let Some(g) = gfx.get(race) else { continue };
            for (t, fj) in first_job[race].iter_mut().enumerate() {
                let job = g.job_groups().get(t).copied().unwrap_or(0) as usize;
                *fj = if g.job_dirs(job).is_some() { job as u16 } else { 0 };
            }
            frames[race] = (0..g.job_count()).map(|j| std::array::from_fn(|d| frame_count(race, j, d))).collect();
        }

        let mut lists: Vec<Vec<EntityTask>> = Vec::new();
        let mut slots = vec![None; RACES * JOB_LIST_MAX];
        let implicit = |slots: &mut Vec<Option<u32>>, lists: &mut Vec<Vec<EntityTask>>, list: usize, kind: usize, checkin: bool| {
            for race in 0..RACES {
                let first = first_job[race][kind];
                if first == 0 || slots[list].is_some() {
                    slots[race * JOB_LIST_MAX + list] = slots[list];
                    continue;
                }
                let n = frame_count(race, first as usize, 2);
                let tasks = if checkin {
                    vec![
                        EntityTask::new(task_type::GO_TO_TARGET, first, 0, 0, -1, n, 1, true, true, 0, 0, 0, 0),
                        EntityTask::new(task_type::CHECKIN, first, 0, 0, -1, n, 1, false, true, 0, 0, 0, 0),
                    ]
                } else {
                    vec![EntityTask::new(task_type::WAIT, first, 0, 0, 1, n, -1, true, true, 0, 0, 0, 0)]
                };
                slots[race * JOB_LIST_MAX + list] = Some(lists.len() as u32);
                lists.push(tasks);
            }
        };
        for k in 1..SETTLER_TYPES {
            implicit(&mut slots, &mut lists, k, k, false);
        }
        for m in 1..SETTLER_TYPES - 1 {
            implicit(&mut slots, &mut lists, 66 + m, m, true);
        }

        for jl in &data.jobs {
            let Some(list) = jl.list else { continue }; // "isn't part of S4_AnimListDefines.h"
            let (race, list) = (jl.race as usize, list as usize);
            if race >= RACES || list >= JOB_LIST_MAX {
                continue;
            }
            for n in &jl.nodes {
                let job = n.job_part.unwrap_or(0xFFFF);
                let count = frame_count(race, job as usize, if n.dir == -1 { 0 } else { n.dir as usize });
                let duration = if n.duration == 0 { count as i32 } else { n.duration };
                let t = EntityTask::new(
                    n.task.map_or(0xFF, |t| t as u8),
                    job,
                    n.x as i16,
                    n.y as i16,
                    duration as i8,
                    count,
                    n.dir as i8,
                    n.forward,
                    n.visible,
                    n.entity as u8,
                    0,
                    0,
                    n.trigger.unwrap_or(0) as u8,
                );
                let slot = &mut slots[race * JOB_LIST_MAX + list];
                let obj = *slot.get_or_insert_with(|| {
                    lists.push(Vec::new());
                    (lists.len() - 1) as u32
                });
                lists[obj as usize].push(t);
            }
        }

        let anim_lists = Race::ALL
            .iter()
            .map(|&r| {
                (0..SETTLER_TYPES)
                    .map(|t| {
                        Settler::from_u16(t as u16)
                            .and_then(|s| data.settler(r, s))
                            .map_or_else(Vec::new, |i| i.anim_lists.clone())
                    })
                    .collect()
            })
            .collect();

        let searches = Race::ALL
            .iter()
            .map(|&r| {
                (0..SETTLER_TYPES)
                    .map(|t| {
                        Settler::from_u16(t as u16)
                            .and_then(|s| data.settler(r, s))
                            .map_or_else(Vec::new, |i| i.search.iter().map(|&k| k as u16).collect())
                    })
                    .collect()
            })
            .collect();

        let roles = Race::ALL
            .iter()
            .map(|&r| {
                (0..SETTLER_TYPES)
                    .map(|t| Settler::from_u16(t as u16).and_then(|s| data.settler(r, s)).and_then(|i| i.role).map_or(0, |o| o as u8))
                    .collect()
            })
            .collect();
        let others = Race::ALL
            .iter()
            .map(|&r| {
                (0..SETTLER_TYPES)
                    .map(|t| Settler::from_u16(t as u16).and_then(|s| data.settler(r, s)).and_then(|i| i.other).map_or(0, |o| o as u8))
                    .collect()
            })
            .collect();

        let mut sounds = vec![JobSoundInfo::default(); SIV_MAX_JOB];
        for s in &data.job_sounds {
            if let Some(slot) = sounds.get_mut(s.job as usize) {
                *slot = JobSoundInfo {
                    sound: s.sound.map_or(0, |v| v as u16),
                    random: s.random.clamp(0, 255) as u8,
                    frame: s.frame.clamp(0, 255) as u8,
                };
            }
        }
        JobData { lists: lists.into_iter().map(Into::into).collect(), slots, first_job, frames, anim_lists, searches, roles, others, sounds }
    }

    /// Load jobInfo.xml / jobSoundList.xml (via `data`) and the five settler gfx files.
    pub fn load(data: &GameData) -> s4_formats::Result<JobData> {
        let gfx: Vec<GfxFile> = (0..RACES as u32).map(|r| GfxFile::open_default(20 + r)).collect::<Result<_, _>>()?;
        Ok(JobData::new(data, &gfx.iter().collect::<Vec<_>>()))
    }

    /// `CEntityToDoListMgr::SettlerJobList(race, list)`.
    pub fn list(&self, race: u8, list: u16) -> Option<&Arc<[EntityTask]>> {
        let i = (race as usize).checked_mul(JOB_LIST_MAX)? + list as usize;
        let obj = (*self.slots.get(i)?)?;
        self.lists.get(obj as usize)
    }

    /// Whether two slots share one list object (for tests of the sharing rule).
    pub fn same_list_object(&self, a: (u8, u16), b: (u8, u16)) -> bool {
        let s = |(r, l): (u8, u16)| self.slots.get(r as usize * JOB_LIST_MAX + l as usize).copied().flatten();
        s(a).is_some() && s(a) == s(b)
    }

    /// `CGfxManager::GetSettlerFirstJob(race, type)`.
    pub fn first_job(&self, race: u8, settler: u16) -> u16 {
        self.first_job.get(race as usize).and_then(|r| r.get(settler as usize)).copied().unwrap_or(0)
    }

    /// `CGfxManager::GetSettlerJobFrameCount(race, job, dir)`.
    pub fn frame_count(&self, race: u8, job: u16, dir: u8) -> u8 {
        self.frames
            .get(race as usize)
            .and_then(|r| r.get(job as usize))
            .and_then(|d| d.get(dir as usize))
            .copied()
            .unwrap_or(0)
    }

    /// `SettlerInfo::m_vAnimLists[i]` of `(race, type)`.
    pub fn anim_list(&self, race: u8, settler: u16, i: usize) -> Option<u16> {
        self.anim_lists.get(race as usize)?.get(settler as usize)?.get(i).copied()
    }

    /// Search type `i` of the settler (`SettlerInfo::m_vSearchInfos[i]`, a `SEARCH_*` value).
    pub fn search(&self, race: u8, settler: u16, i: usize) -> Option<u16> {
        self.searches.get(race as usize)?.get(settler as usize)?.get(i).copied()
    }

    /// `SettlerInfo::m_uRole`: the role a settler of this type gets (`jobs::role` ids).
    pub fn settler_role(&self, race: u8, settler: u16) -> u8 {
        self.roles.get(race as usize).and_then(|r| r.get(settler as usize)).copied().unwrap_or(0)
    }

    /// `SettlerInfo` `other`: the deco kind the settler plants (0 = none / VINE).
    pub fn other(&self, race: u8, settler: u16) -> u8 {
        self.others.get(race as usize).and_then(|r| r.get(settler as usize)).copied().unwrap_or(0)
    }

    /// `CEntityToDoListMgr::GetJobSoundInfo(job)`.
    pub fn sound(&self, job: u16) -> JobSoundInfo {
        self.sounds.get(job as usize).copied().unwrap_or_default()
    }

    pub fn is_empty(&self) -> bool {
        self.lists.is_empty()
    }
}
