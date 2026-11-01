//! `GameData\SettlerValues.xml` / `GameData\SettlerValuesMp.xml` and `GameData\VehicleInfo.xml`.
//!
//! Settlers — reader: `CSettlerMgr::LoadInfo(isMP)` (MapObjects/Settler/CSettlerMgr.cpp) opens
//! settlerValuesMp.xml in multiplayer, settlerValues.xml otherwise (the two shipped files are
//! identical). Stored in `CSettlerMgr::m_vSettlerInfos[70 * race + type]`. Per `race/settler`:
//! `role` (CDefineTranslator `*_ROLE`), `speed`, `health`, `armor`, `damage`, `damage2`, `damage3`,
//! `misc` (atoi; empty = 0), `other` (CDefineTranslator `CDecoObjMgr::*`: the plant a planter
//! seeds), `tool` (GOOD_*), every child of `search` (SEARCH_* via `CSearchRoutines::GetSearchData`)
//! and every child of `animLists` (JOB_* lists: 0 = check-in list, 1 = normal work list, more =
//! extra lists, per the file's own comment). A `searchType` directly under `settler` is ignored.
//!
//! After loading: any type of a race whose `role` stayed 0 copies the Roman entry; squad-leader
//! bonus = `misc` of Roman SETTLER_SQUADLEADER (default 10 %) as `misc*256/100`; Roman medics
//! default `misc` 3/5/7; blowgun freeze chance, catapultist percentages and saboteur
//! hit chance come from `misc` too ([`SettlerInfo::misc`]): blowgun freeze = Maya BLOWGUNWARRIOR_01..03,
//! catapultist percentages = Maya BACKPACKCATAPULTIST_01..03 (defaults 55/65/75), saboteur = Roman.
//!
//! Vehicles — reader: `CVehicleInfoMgr::ReadXMLFile` (MapObjects/Vehicle/CVehicleInfoMgr.cpp).
//! Defaults come from a static table in the exe; each XML tag overrides only when non-empty:
//! `Boards`, `Ironbars` (<= 16 each, sum > 0) = build cost, `Hitpoints` (stored halved, < 256),
//! `Armor`, `Damage`, `ReadyToFireDelay`, `MaxAmmo` (must be > 0 for armed vehicles, 0 otherwise),
//! `WalkSteps` (1..=16, not in the shipped file).

use super::defines::{AnimList, DecoKind, Good, Race, Role, SearchType, Settler, Vehicle};
use super::xml::{child, child_int, child_text, children, elements, id, text};
use super::Ctx;
use crate::Result;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettlerInfo {
    pub race: Race,
    pub settler: Settler,
    pub role: Option<Role>,
    pub speed: i32,
    pub health: i32,
    pub armor: i32,
    pub damage: i32,
    pub damage2: i32,
    pub damage3: i32,
    /// Squad leader: group attack bonus %; medics: HP healed per animation; others: see module docs.
    pub misc: i32,
    pub other: Option<DecoKind>,
    pub tool: Option<Good>,
    pub search: Vec<SearchType>,
    /// JOB_* lists as values (`AnimList::from_u16`).
    pub anim_lists: Vec<u16>,
}

impl SettlerInfo {
    pub fn anim_list(&self, i: usize) -> Option<AnimList> {
        self.anim_lists.get(i).and_then(|&v| AnimList::from_u16(v))
    }
}

pub(crate) fn parse_settlers(text_: &str, ctx: &mut Ctx, file: &str) -> Result<Vec<SettlerInfo>> {
    let doc = roxmltree::Document::parse(text_).map_err(|e| crate::Error::Format(format!("{file}: {e}")))?;
    let mut out = Vec::new();
    for race_node in children(doc.root_element(), "race") {
        let Some(race) = ctx.translator(id(race_node), Race::from_name, Race::from_u16, "race") else { continue };
        for s in children(race_node, "settler") {
            let Some(settler) = ctx.enum_by_define(id(s), Settler::from_u16, "settler") else { continue };
            let other = child_text(s, "other");
            let search = child(s, "search", 0)
                .map(|n| {
                    elements(n)
                        .filter_map(|c| ctx.translator(text(c), SearchType::from_name, SearchType::from_u16, "search"))
                        .collect()
                })
                .unwrap_or_default();
            let anim_lists = child(s, "animLists", 0)
                .map(|n| elements(n).filter_map(|c| ctx.define(text(c), "animList").map(|v| v as u16)).collect())
                .unwrap_or_default();
            out.push(SettlerInfo {
                race,
                settler,
                role: ctx.translator(child_text(s, "role"), Role::from_name, Role::from_u16, "role"),
                speed: child_int(s, "speed"),
                health: child_int(s, "health"),
                armor: child_int(s, "armor"),
                damage: child_int(s, "damage"),
                damage2: child_int(s, "damage2"),
                damage3: child_int(s, "damage3"),
                misc: child_int(s, "misc"),
                other: ctx.translator(other, DecoKind::from_name, DecoKind::from_u16, "other"),
                tool: ctx.enum_by_define(child_text(s, "tool"), Good::from_u16, "tool"),
                search,
                anim_lists,
            });
        }
    }
    Ok(out)
}

/// One `race/vehicle` of VehicleInfo.xml; `None` = tag absent/empty (exe default applies).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VehicleInfo {
    pub race: Race,
    pub vehicle: Vehicle,
    pub boards: Option<i32>,
    pub ironbars: Option<i32>,
    /// Raw XML value (the game stores half).
    pub hitpoints: Option<i32>,
    pub armor: Option<i32>,
    pub damage: Option<i32>,
    pub ready_to_fire_delay: Option<i32>,
    pub max_ammo: Option<i32>,
    pub walk_steps: Option<i32>,
}

pub(crate) fn parse_vehicles(text_: &str, ctx: &mut Ctx) -> Result<Vec<VehicleInfo>> {
    let doc = roxmltree::Document::parse(text_).map_err(|e| crate::Error::Format(format!("VehicleInfo.xml: {e}")))?;
    let mut out = Vec::new();
    for race_node in children(doc.root_element(), "race") {
        let Some(race) = ctx.translator(id(race_node), Race::from_name, Race::from_u16, "race") else { continue };
        for v in children(race_node, "vehicle") {
            let Some(vehicle) = ctx.enum_by_define(id(v), Vehicle::from_u16, "vehicle") else { continue };
            let opt = |f: &str| {
                let t = child_text(v, f);
                (!t.is_empty()).then(|| super::xml::atoi(t))
            };
            out.push(VehicleInfo {
                race,
                vehicle,
                boards: opt("Boards"),
                ironbars: opt("Ironbars"),
                hitpoints: opt("Hitpoints"),
                armor: opt("Armor"),
                damage: opt("Damage"),
                ready_to_fire_delay: opt("ReadyToFireDelay"),
                max_ammo: opt("MaxAmmo"),
                walk_steps: opt("WalkSteps"),
            });
        }
    }
    Ok(out)
}
