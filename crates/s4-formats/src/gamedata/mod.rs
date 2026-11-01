//! The mechanics tables in `GameData\*.xml`, typed, plus the id → sprite resolution ([`resolve`]).
//!
//! | file | reader in S4_Main (decompilation) | module |
//! |---|---|---|
//! | buildingInfo.xml | `CBuildingInfoMgr::ReadBuildingInfo` | [`building`] |
//! | BuildingTrigger.xml | second half of `ReadBuildingInfo` (`m_vTriggerInfos`) | [`building`] |
//! | jobInfo.xml, jobSoundList.xml | `CEntityToDoListMgr::CEntityToDoListMgr` 0x154c610 | [`job`] |
//! | effectsInfo.xml | **not loaded** by this exe (no path string) | [`job`] |
//! | objectInfo.xml | `CDecoObjMgr::ReadObjectInfos` | [`object`] |
//! | SettlerValues.xml / SettlerValuesMp.xml | `CSettlerMgr::LoadInfo(isMP)` | [`settler`] |
//! | VehicleInfo.xml | `CVehicleInfoMgr::ReadXMLFile` | [`settler`] |
//!
//! The installed exe's path strings are `GameData\BuildingInfo.xml`, `BuildingTrigger.xml`,
//! `ObjectInfo.xml`, `jobSoundList.xml`, `JobInfo.xml`, `settlerValues.xml`, `settlerValuesMp.xml`,
//! `VehicleInfo.xml` (Windows, case-insensitive).
//!
//! # String ids
//! The readers resolve ids two ways, reproduced here:
//! * `CConfigManager::GetDefineValue` — a map built from `{id, name}` tables in the exe
//!   (BUILDING_, SETTLER_, GOOD_, OBJECT_, SOUND_, EFFECT_, VEHICLE_, ANIMAL_, JOB_ lists and the gfx
//!   job names). See [`define_value`] and [`defines`].
//! * `CDefineTranslator::GetValueOfDefine` — races, `*_ROLE`, `SEARCH_*`, `HOUSE_KIND_*`,
//!   `CEntityTask::*`, `CDecoObjMgr::*`, `TRIGGER_*`, patch types.
//!
//! Ids the exe does not know are reported in [`GameData::warnings`] (the game skips them too).
//!
//! `defines.rs` is generated from the installed exe's tables (found by scanning .rdata/.data for
//! `{u32 id, char* name}` runs) and from `CDefineTranslator`'s constructor; regenerate it with
//! `tools/gen_defines` (see its README).

pub mod building;
pub mod defines;
pub mod job;
pub mod object;
pub mod resolve;
pub mod settler;
mod xml;

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

pub use building::{BuildingInfo, TriggerInfo};
pub use defines::*;
pub use job::{EffectsEntry, JobList, JobNode, JobSound};
pub use object::ObjectInfo;
pub use settler::{SettlerInfo, VehicleInfo};

use crate::Result;

/// `CConfigManager::GetDefineValue`: value of a define name from the exe's tables.
pub fn define_value(name: &str) -> Option<i32> {
    static MAP: OnceLock<HashMap<&'static str, i32>> = OnceLock::new();
    let map = MAP.get_or_init(|| {
        let mut m = HashMap::new();
        for t in defines::CONFIG_DEFINE_TABLES {
            for &(n, v) in t.iter() {
                m.insert(n, v);
            }
        }
        m
    });
    map.get(name).copied()
}

/// Parsing context: collects warnings for ids the game would not resolve either.
#[derive(Default)]
pub(crate) struct Ctx {
    pub warnings: Vec<String>,
}

impl Ctx {
    pub fn warn(&mut self, w: String) {
        self.warnings.push(w);
    }

    /// `GetDefineValue`; warns on an unknown non-empty name.
    pub fn define(&mut self, name: &str, what: &str) -> Option<i32> {
        if name.is_empty() {
            return None;
        }
        let v = define_value(name);
        if v.is_none() {
            self.warn(format!("unknown {what} define {name:?}"));
        }
        v
    }

    pub fn enum_by_define<T>(&mut self, name: &str, from: fn(u16) -> Option<T>, what: &str) -> Option<T> {
        let v = self.define(name, what)?;
        let r = u16::try_from(v).ok().and_then(from);
        if r.is_none() {
            self.warn(format!("{what} {name:?} = {v} is not a valid value"));
        }
        r
    }

    /// `CDefineTranslator::GetValueOfDefine`: unknown **and empty** names yield 0 in the game
    /// (not -1), so this returns the value-0 variant then (or `None` if 0 has no variant) and warns
    /// on unknown non-empty names.
    pub fn translator<T>(
        &mut self,
        name: &str,
        from: fn(&str) -> Option<T>,
        zero: fn(u16) -> Option<T>,
        what: &str,
    ) -> Option<T> {
        if let Some(v) = from(name) {
            return Some(v);
        }
        if !name.is_empty() {
            self.warn(format!("unknown {what} {name:?} (game uses 0)"));
        }
        zero(0)
    }
}

/// Every GameData table.
pub struct GameData {
    pub building_xml_version: u32,
    pub buildings: Vec<BuildingInfo>,
    pub triggers: Vec<TriggerInfo>,
    pub jobs: Vec<JobList>,
    pub job_sounds: Vec<JobSound>,
    pub object_xml_version: u32,
    pub objects: Vec<ObjectInfo>,
    pub settlers: Vec<SettlerInfo>,
    pub settlers_mp: Vec<SettlerInfo>,
    pub vehicles: Vec<VehicleInfo>,
    /// effectsInfo.xml (not used by the game).
    pub effects: Vec<EffectsEntry>,
    /// Ids the game would not resolve (`file: message`).
    pub warnings: Vec<String>,
}

fn read_text(dir: &Path, name: &str) -> Result<String> {
    let p = crate::path::resolve(dir.join(name));
    let bytes = std::fs::read(&p).map_err(|e| crate::Error::Format(format!("{}: {e}", p.display())))?;
    // Files are UTF-8 (some declare it) and ASCII in practice; strip a BOM if present.
    let s = String::from_utf8_lossy(&bytes).into_owned();
    Ok(s.strip_prefix('\u{feff}').map(str::to_string).unwrap_or(s))
}

impl GameData {
    /// Load all tables from a `GameData` directory.
    pub fn load(dir: impl AsRef<Path>) -> Result<GameData> {
        let dir = dir.as_ref();
        let mut warnings = Vec::new();
        let run = |file: &str| -> Result<(String, Ctx)> { Ok((read_text(dir, file)?, Ctx::default())) };
        let mut finish = |file: &str, ctx: Ctx| warnings.extend(ctx.warnings.into_iter().map(|w| format!("{file}: {w}")));

        let (t, mut c) = run("buildingInfo.xml")?;
        let (building_xml_version, buildings) = building::parse_buildings(&t, &mut c)?;
        finish("buildingInfo.xml", c);
        let (t, mut c) = run("BuildingTrigger.xml")?;
        let triggers = building::parse_triggers(&t, &mut c)?;
        finish("BuildingTrigger.xml", c);
        let (t, mut c) = run("jobInfo.xml")?;
        let jobs = job::parse_jobs(&t, &mut c)?;
        finish("jobInfo.xml", c);
        let (t, mut c) = run("jobSoundList.xml")?;
        let job_sounds = job::parse_job_sounds(&t, &mut c)?;
        finish("jobSoundList.xml", c);
        let (t, mut c) = run("objectInfo.xml")?;
        let (object_xml_version, objects) = object::parse_objects(&t, &mut c)?;
        finish("objectInfo.xml", c);
        let (t, mut c) = run("SettlerValues.xml")?;
        let settlers = settler::parse_settlers(&t, &mut c, "SettlerValues.xml")?;
        finish("SettlerValues.xml", c);
        let (t, mut c) = run("SettlerValuesMp.xml")?;
        let settlers_mp = settler::parse_settlers(&t, &mut c, "SettlerValuesMp.xml")?;
        finish("SettlerValuesMp.xml", c);
        let (t, mut c) = run("VehicleInfo.xml")?;
        let vehicles = settler::parse_vehicles(&t, &mut c)?;
        finish("VehicleInfo.xml", c);
        let (t, _) = run("effectsInfo.xml")?;
        let effects = job::parse_effects(&t)?;

        Ok(GameData {
            building_xml_version,
            buildings,
            triggers,
            jobs,
            job_sounds,
            object_xml_version,
            objects,
            settlers,
            settlers_mp,
            vehicles,
            effects,
            warnings,
        })
    }

    /// Load from the default install (`crate::game_dir()/GameData`).
    pub fn load_default() -> Result<GameData> {
        Self::load(crate::game_dir().join("GameData"))
    }

    pub fn building(&self, race: Race, building: Building) -> Option<&BuildingInfo> {
        self.buildings.iter().find(|b| b.race == race && b.building == building)
    }

    /// Settler info as the game resolves it: a type missing for `race` (role 0) falls back to the
    /// Roman entry (`CSettlerMgr::LoadInfo`).
    pub fn settler(&self, race: Race, settler: Settler) -> Option<&SettlerInfo> {
        let find = |r| self.settlers.iter().find(|s| s.race == r && s.settler == settler && s.role.is_some());
        find(race).or_else(|| find(Race::ROMAN))
    }

    pub fn job_list(&self, race: Race, list: AnimList) -> Option<&JobList> {
        self.jobs.iter().find(|j| j.race == race && j.list == Some(list))
    }

    pub fn object(&self, object: Object) -> Option<&ObjectInfo> {
        self.objects.iter().find(|o| o.object == object)
    }
}
