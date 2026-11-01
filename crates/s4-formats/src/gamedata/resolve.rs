//! From game ids to sprites: which gfx file, job, "direction" and frames draw a map object,
//! building, settler, vehicle, animal, pile or effect.
//!
//! Mirrors the `CGfxManager::Get*GfxInfo` accessors (Gfx/CGfxManager.cpp) together with the
//! `.sil` "first job" lookups (`GetSettlerFirstJob` 0x1362410, `GetObjectFirstJob` 0x13621a0,
//! `GetAnimalFirstJob`, `GetVehicleFirstJob`):
//!
//! | what | gfx file | job | "direction" (dil sub-index) | frame |
//! |---|---|---|---|---|
//! | building | 10 + race | `Building` value | 0 = construction (`*_BUILD`), 1 = finished (`*_STANDARD`); patches/triggers name an absolute .dil index | 0 (animations: frame) |
//! | deco object | 5 | `5.sil[Object]` (first job, e.g. `OBJECT_TREE01A_GROW_01`) | amount − 1 | animation frame |
//! | settler | 20 + race | `sil[Settler]` = its WALK job; any jobInfo `jobPart` is a job directly | facing 0..5 | animation frame |
//! | vehicle | 30 + race | `sil[Vehicle]` | facing 0..5 | frame |
//! | animal | 8 | `8.sil[Animal]` | facing 0..5 | frame |
//! | pile | 3 | `Good` value | amount − 1 (0..7) | frame |
//! | effect id < 100 | 6 (4 with 4444 palettes) | effect id | direction | frame |
//! | effect id >= 100 | 37 (36 with 4444 palettes) | id − 100 | direction | frame |
//!
//! Every name in [`super::defines`]' gfx job tables lines up with these indices on the installed
//! data (checked by `tests/gamedata.rs`): `BUILDING_JOB_NAMES[jil[b]]` starts with the building's
//! name, `OBJECT_JOB_NAMES[5.sil[o]]` with the object's, `SETTLER_JOB_NAMES[sil[s]]` is the settler's
//! `*_WALK`, `VEHICLE_JOB_NAMES[sil[v]]` its `*_MOVE`.
//!
//! The palette of the frame is chosen by the job (`.pi2[job]`), which [`GfxFile::decode`] does.

use std::ops::Range;

use super::defines::{Animal, Building, Good, Object, Race, Settler, Vehicle};
use crate::gfx::GfxFile;

/// Which part of a building to draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildingPart {
    /// `*_BUILD` (.dil +0): the construction-site image (revealed bottom-up while building;
    /// `CCachePageManager::RenderCacheObject` crops it by progress/256).
    Construction,
    /// `*_STANDARD` (.dil +1): the finished building.
    Finished,
    /// An absolute .dil index (`BuildingInfo::patches[].job`, `TriggerInfo::patch`).
    Dil(u16),
}

pub fn building_file(race: Race) -> u32 {
    10 + race as u32
}

pub fn settler_file(race: Race) -> u32 {
    20 + race as u32
}

/// `None` for the Dark Tribe (file 33 has no images).
pub fn vehicle_file(race: Race) -> Option<u32> {
    (race != Race::DARK).then(|| 30 + race as u32)
}

pub const OBJECT_FILE: u32 = 5;
pub const ANIMAL_FILE: u32 = 8;
pub const PILE_FILE: u32 = 3;
pub const ACCESSORY_FILE: u32 = 7;

/// Gfx file and job of an effect id (`GetEffectGfxInfo` 0x1363030).
pub fn effect_job(effect_id: u16, use_4444: bool) -> (u32, usize) {
    if effect_id >= 100 {
        (if use_4444 { 36 } else { 37 }, effect_id as usize - 100)
    } else {
        (if use_4444 { 4 } else { 6 }, effect_id as usize)
    }
}

/// Image indices of a building part in the race's building file (`building_file(race)`).
pub fn building_frames(gfx: &GfxFile, building: Building, part: BuildingPart) -> Option<Range<usize>> {
    let dil = match part {
        BuildingPart::Construction => gfx.job_dirs(building as usize)?.start,
        BuildingPart::Finished => {
            let d = gfx.job_dirs(building as usize)?;
            if d.len() < 2 {
                return None;
            }
            d.start + 1
        }
        BuildingPart::Dil(d) => d as usize,
    };
    gfx.dil_frames(dil)
}

/// First gfx job of a type from a file's `.sil` (`Get*FirstJob`).
pub fn first_job(gfx: &GfxFile, type_id: u16) -> Option<usize> {
    let &j = gfx.job_groups().get(type_id as usize)?;
    (j != 0 || type_id == 0).then_some(j as usize)
}

/// Image indices of a deco object in file 5 (`GetObjectGfxInfo` 0x1361f80): `amount` is 1-based.
pub fn object_frames(gfx: &GfxFile, object: Object, amount: usize) -> Option<Range<usize>> {
    gfx.job_frames(first_job(gfx, object as u16)?, amount.checked_sub(1)?)
}

/// The settler's default (walk) job in its race's settler file.
pub fn settler_job(gfx: &GfxFile, settler: Settler) -> Option<usize> {
    first_job(gfx, settler as u16)
}

/// A settler type's wait list: one task on `job`, lasting `frames` ticks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WaitJob {
    /// Settler gfx job (an index into the settler's own race file).
    pub job: usize,
    /// `GetSettlerJobFrameCount(race, job, 2)`: frames of the job's direction 2.
    pub frames: usize,
}

/// The wait list of settler type `settler_type` (1..=66) for the race whose settler file is
/// `gfx`; `race0` is the Roman file (20).
///
/// `CEntityToDoListMgr::CEntityToDoListMgr` (0x154c610, MapObjects/EntityToDoListMgr.cpp)
/// builds `SettlerJobList(race, id)` = `this[267·race + id]`. Ids 1..=66 are not in jobInfo.xml:
/// for each race and type `k` it takes `GetSettlerFirstJob(race, k)` (`sil[k]`, the WALK job) and
/// creates one `CEntityTask(17, job, …, duration = GetSettlerJobFrameCount(race, job, 2), dir −1)`.
/// Races 1..4 reuse race 0's list when race 0 has one, and also when they have no first job of
/// their own. `CSettler::TakeWaitList` (idle settlers, garrisoned tower soldiers before they
/// find a target) uses it. Ids 67..=131 are the same per-type construction for the CHECKIN
/// defaults (tasks 10 + 24); higher ids come from jobInfo.xml.
pub fn settler_wait_job(race0: &GfxFile, gfx: &GfxFile, settler_type: u16) -> Option<WaitJob> {
    if !(1..=66).contains(&settler_type) {
        return None;
    }
    let make = |f: &GfxFile, job: usize| WaitJob { job, frames: f.job_frames(job, 2).map_or(0, |r| r.len()) };
    match first_job(race0, settler_type) {
        Some(j) => Some(make(race0, j)),
        None => first_job(gfx, settler_type).map(|j| make(gfx, j)),
    }
}

/// Image indices of a settler gfx job (from a jobInfo `jobPart` or [`settler_job`]) facing `dir`.
pub fn settler_frames(gfx: &GfxFile, job: usize, dir: usize) -> Option<Range<usize>> {
    gfx.job_frames(job, dir)
}

pub fn vehicle_frames(gfx: &GfxFile, vehicle: Vehicle, dir: usize) -> Option<Range<usize>> {
    gfx.job_frames(first_job(gfx, vehicle as u16)?, dir)
}

pub fn animal_frames(gfx: &GfxFile, animal: Animal, dir: usize) -> Option<Range<usize>> {
    gfx.job_frames(first_job(gfx, animal as u16)?, dir)
}

/// Image indices of a goods pile in file 3 (`GetPileGfxInfo` 0x1362620): `amount` 1..=8.
pub fn pile_frames(gfx: &GfxFile, good: Good, amount: usize) -> Option<Range<usize>> {
    gfx.job_frames(good as usize, amount.checked_sub(1)?)
}
