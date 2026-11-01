//! `GameData\buildingInfo.xml` and `GameData\BuildingTrigger.xml`.
//!
//! Reader: `CBuildingInfoMgr::ReadBuildingInfo` (0x14eb... in the decompilation,
//! MapObjects/Building/CBuildingInfoMgr.cpp). Results live in
//! `CBuildingInfoMgr::m_vBuildingInfos[5 races][83 buildings]` (848 bytes each, `GetBuildingInfo`
//! 0x133ac00) and `m_vTriggerInfos[5][47]` (36 bytes each, `GetTriggerInfo`).
//!
//! Field meaning, as the reader stores and checks it:
//! * `iHotSpotX/Y` — the building's anchor cell inside the bit-mask grid. Mask row `r`, bit `31-c`
//!   (MSB = column 0) is map offset `(c - hotspot_x, r - hotspot_y)`; the max-distance pass after
//!   parsing walks the masks exactly like that.
//! * `stone/boards/gold` — construction cost. If `Hitpoints` is absent, hit points default to
//!   `min(250, 8*(stone+boards+gold)/2)` (stored halved like vehicles).
//! * `lines` — number of mask rows; `buildingPosLines` (footprint), `digPosLines` (cells the
//!   diggers flatten), `blockPosLines` (blocked cells), `repealingPosLines` (sic; cells other
//!   objects are pushed away from). Water masks (`waterPosLines`, `waterBlockPosLines`,
//!   `waterFreePosLines`) are only read for ports/shipyards (`BuildingTypeExIsPort/IsShipyard`).
//! * `builderNumber` + `builderInfo{xOffset,yOffset,dir}` — builder work spots (cell offsets).
//! * `flag`, `door`, `workingpos` `{xOffset,yOffset}` — cell offsets; `miniflag` is a pixel offset.
//! * `bitBoundingRect{minX,maxX,minY,maxY}` — bounding box of the masks.
//! * `pileNumber` + `pile{xOffset,yOffset,good,type,patch,appearance,xPixelOffset,yPixelOffset}`.
//! * `settlerNumber` + `settler{xOffset,yOffset,direction,top}` — soldier slots of military
//!   buildings (pixel offsets); `top=true` are bowmen on top, the rest swordsmen.
//! * `patchSettlerSlot`, `kind` (HOUSE_KIND_*), `inhabitant` (SETTLER_*), `tool` (GOOD_*),
//!   `productiondelay` (clamped 0..=255), `dummyValue`, `influenceRadius`, `explorerRadius`,
//!   `workingAreaRadius`, `searchType` (not present in the shipped file), `Hitpoints`, `Armor`.
//! * `animLists/animList` — JOB_* lists of the inhabitant.
//! * `patches/patch{job,type,ticks,slot,sound{def,frame,random}}` — overlay animations; `job` is a
//!   building gfx part name ([`BUILDING_JOB_NAMES`](super::defines::BUILDING_JOB_NAMES)) =
//!   .dil index in the race's building gfx file. `random` 0 means 100.
//! * Not read by the game: `calcProd`, `gridChangedForExport`, `gridVersion`, `helperFile`,
//!   `helperX`, `helperY` (editor leftovers). They are kept here for completeness.

use roxmltree::Node;

use super::defines::{Building, Effect, Good, HouseKind, PatchType, Race, SearchType, Settler, Sound};
use super::xml::{atoi, child, child_int, child_text, children, elements, id, path_int, path_text};
use super::Ctx;
use crate::{Result, bail};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Offset {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub min_x: i32,
    pub max_x: i32,
    pub min_y: i32,
    pub max_y: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuilderSpot {
    pub x: i32,
    pub y: i32,
    pub dir: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PileInfo {
    /// Cell offset from the hot spot.
    pub x: i32,
    pub y: i32,
    pub good: Option<Good>,
    /// `type` (0, 1 or 4 in the shipped data; consumer not traced).
    pub kind: i32,
    pub patch: i32,
    pub appearance: i32,
    /// Pixel offset used to draw the pile.
    pub x_pixel: i32,
    pub y_pixel: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SoldierSlot {
    /// Pixel offset.
    pub x: i32,
    pub y: i32,
    pub direction: i32,
    /// Stands on top (bowman) rather than at the door (swordsman).
    pub top: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SoundRef {
    pub sound: Option<Sound>,
    pub frame: i32,
    /// Chance in percent (0 in the file means 100).
    pub random: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Patch {
    /// Building gfx part (.dil index in the race's building file); `None` if the name is unknown.
    pub job: Option<u16>,
    pub job_name: String,
    pub kind: Option<PatchType>,
    pub ticks: i32,
    pub slot: i32,
    pub sound: Option<SoundRef>,
}

/// One `race/building` entry of buildingInfo.xml.
#[derive(Clone, Debug)]
pub struct BuildingInfo {
    pub race: Race,
    pub building: Building,
    pub hotspot: Offset,
    pub stone: i32,
    pub boards: i32,
    pub gold: i32,
    pub lines: usize,
    pub building_pos_lines: Vec<u32>,
    pub dig_pos_lines: Vec<u32>,
    pub block_pos_lines: Vec<u32>,
    pub repealing_pos_lines: Vec<u32>,
    pub water_pos_lines: Vec<u32>,
    pub water_block_pos_lines: Vec<u32>,
    pub water_free_pos_lines: Vec<u32>,
    pub builders: Vec<BuilderSpot>,
    pub flag: Offset,
    pub door: Offset,
    pub working_pos: Offset,
    /// Pixel offset of the small flag.
    pub mini_flag: Offset,
    pub bounding_rect: Rect,
    pub piles: Vec<PileInfo>,
    pub soldiers: Vec<SoldierSlot>,
    pub patch_settler_slot: i32,
    pub kind: Option<HouseKind>,
    pub inhabitant: Option<Settler>,
    pub tool: Option<Good>,
    pub production_delay: u8,
    pub dummy_value: i32,
    pub influence_radius: i32,
    pub explorer_radius: i32,
    pub working_area_radius: i32,
    pub search_type: Option<SearchType>,
    /// Raw XML value (the game stores half of it). 0 = derived from the cost.
    pub hitpoints: i32,
    pub armor: i32,
    pub anim_lists: Vec<u16>,
    pub patches: Vec<Patch>,
    // Present in the XML but never read by S4_Main.exe:
    pub calc_prod: String,
    pub grid_version: i32,
    pub helper_file: String,
    pub helper: Offset,
}

impl BuildingInfo {
    /// Hit points as the game stores them (`Hitpoints/2`, or `min(250, 4*(stone+boards+gold))`).
    pub fn game_hitpoints(&self) -> i32 {
        if self.hitpoints / 2 != 0 {
            self.hitpoints / 2
        } else {
            (8 * (self.stone + self.boards + self.gold) / 2).min(250)
        }
    }

    /// Map cell offsets (relative to the hot spot) set in one of the bit masks.
    pub fn mask_cells(&self, lines: &[u32]) -> Vec<Offset> {
        let mut v = Vec::new();
        for (r, &m) in lines.iter().enumerate() {
            for c in 0..32 {
                if m & (0x8000_0000 >> c) != 0 {
                    v.push(Offset { x: c - self.hotspot.x, y: r as i32 - self.hotspot.y });
                }
            }
        }
        v
    }
}

fn mask(node: Node, name: &str, lines: usize) -> Vec<u32> {
    let Some(n) = child(node, name, 0) else { return Vec::new() };
    (0..lines).map(|i| child(n, "value", i).map(|v| atoi(super::xml::text(v)) as u32).unwrap_or(0)).collect()
}

fn offset(node: Node, name: &str) -> Offset {
    Offset { x: path_int(node, name, "xOffset"), y: path_int(node, name, "yOffset") }
}

fn sound_ref(node: Node, ctx: &mut Ctx) -> Option<SoundRef> {
    let s = child(node, "sound", 0)?;
    let def = child_text(s, "def");
    let random = child_int(s, "random");
    Some(SoundRef {
        sound: if def.is_empty() { None } else { ctx.enum_by_define(def, Sound::from_u16, "sound") },
        frame: child_int(s, "frame"),
        random: if random == 0 { 100 } else { random },
    })
}

pub(crate) fn parse_buildings(text: &str, ctx: &mut Ctx) -> Result<(u32, Vec<BuildingInfo>)> {
    let doc = roxmltree::Document::parse(text).map_err(|e| crate::Error::Format(format!("buildingInfo.xml: {e}")))?;
    let root = doc.root_element();
    if !root.has_tag_name("buildinglist") {
        bail!("buildingInfo.xml: unexpected root <{}>", root.tag_name().name());
    }
    let mut version = 0;
    let mut out = Vec::new();
    for race_node in elements(root) {
        if race_node.has_tag_name("BuildingXMLVersion") {
            version = atoi(super::xml::text(race_node)) as u32;
            continue;
        }
        let Some(race) = ctx.translator(id(race_node), Race::from_name, Race::from_u16, "race") else { continue };
        for b in children(race_node, "building") {
            let Some(building) = ctx.enum_by_define(id(b), Building::from_u16, "building") else { continue };
            let lines = child_int(b, "lines").clamp(0, 64) as usize;
            let builders = (0..child_int(b, "builderNumber").max(0) as usize)
                .map(|k| {
                    let n = child(b, "builderInfo", k);
                    let g = |f| n.map(|n| child_int(n, f)).unwrap_or(0);
                    BuilderSpot { x: g("xOffset"), y: g("yOffset"), dir: g("dir") }
                })
                .collect();
            let piles = (0..child_int(b, "pileNumber").max(0) as usize)
                .map(|m| {
                    let p = child(b, "pile", m);
                    let g = |f| p.map(|n| child_int(n, f)).unwrap_or(0);
                    let good = p.map(|n| child_text(n, "good")).unwrap_or("");
                    PileInfo {
                        x: g("xOffset"),
                        y: g("yOffset"),
                        good: ctx.enum_by_define(good, Good::from_u16, "pile good"),
                        kind: g("type"),
                        patch: g("patch"),
                        appearance: g("appearance"),
                        x_pixel: g("xPixelOffset"),
                        y_pixel: g("yPixelOffset"),
                    }
                })
                .collect();
            let soldiers = (0..child_int(b, "settlerNumber").max(0) as usize)
                .map(|n| {
                    let s = child(b, "settler", n);
                    let g = |f| s.map(|n| child_int(n, f)).unwrap_or(0);
                    SoldierSlot {
                        x: g("xOffset"),
                        y: g("yOffset"),
                        direction: g("direction"),
                        top: s.map(|n| child_text(n, "top")) == Some("true"),
                    }
                })
                .collect();
            let inhabitant = child_text(b, "inhabitant");
            let anim_lists = child(b, "animLists", 0)
                .map(|a| {
                    elements(a)
                        .filter_map(|l| {
                            let name = super::xml::text(l);
                            ctx.define(name, "animList").map(|v| v as u16)
                        })
                        .collect()
                })
                .unwrap_or_default();
            let patches = child(b, "patches", 0)
                .map(|ps| {
                    elements(ps)
                        .map(|p| {
                            let job_name = child_text(p, "job").to_string();
                            Patch {
                                job: ctx.define(&job_name, "patch job").map(|v| v as u16),
                                kind: ctx.translator(child_text(p, "type"), PatchType::from_name, PatchType::from_u16, "patch type"),
                                ticks: child_int(p, "ticks"),
                                slot: child_int(p, "slot"),
                                sound: sound_ref(p, ctx),
                                job_name,
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();
            let search = child_text(b, "searchType");
            out.push(BuildingInfo {
                race,
                building,
                hotspot: Offset { x: child_int(b, "iHotSpotX"), y: child_int(b, "iHotSpotY") },
                stone: child_int(b, "stone"),
                boards: child_int(b, "boards"),
                gold: child_int(b, "gold"),
                lines,
                building_pos_lines: mask(b, "buildingPosLines", lines),
                dig_pos_lines: mask(b, "digPosLines", lines),
                block_pos_lines: mask(b, "blockPosLines", lines),
                repealing_pos_lines: mask(b, "repealingPosLines", lines),
                water_pos_lines: mask(b, "waterPosLines", lines),
                water_block_pos_lines: mask(b, "waterBlockPosLines", lines),
                water_free_pos_lines: mask(b, "waterFreePosLines", lines),
                builders,
                flag: offset(b, "flag"),
                door: offset(b, "door"),
                working_pos: offset(b, "workingpos"),
                mini_flag: offset(b, "miniflag"),
                bounding_rect: Rect {
                    min_x: path_int(b, "bitBoundingRect", "minX"),
                    max_x: path_int(b, "bitBoundingRect", "maxX"),
                    min_y: path_int(b, "bitBoundingRect", "minY"),
                    max_y: path_int(b, "bitBoundingRect", "maxY"),
                },
                piles,
                soldiers,
                patch_settler_slot: child_int(b, "patchSettlerSlot"),
                kind: ctx.translator(child_text(b, "kind"), HouseKind::from_name, HouseKind::from_u16, "house kind"),
                inhabitant: if inhabitant.is_empty() {
                    None
                } else {
                    ctx.enum_by_define(inhabitant, Settler::from_u16, "inhabitant")
                },
                tool: ctx.enum_by_define(child_text(b, "tool"), Good::from_u16, "tool"),
                production_delay: child_int(b, "productiondelay").clamp(0, 255) as u8,
                dummy_value: child_int(b, "dummyValue"),
                influence_radius: child_int(b, "influenceRadius"),
                explorer_radius: child_int(b, "explorerRadius"),
                working_area_radius: child_int(b, "workingAreaRadius"),
                search_type: ctx.translator(search, SearchType::from_name, SearchType::from_u16, "search"),
                hitpoints: child_int(b, "Hitpoints"),
                armor: child_int(b, "Armor"),
                anim_lists,
                patches,
                calc_prod: child_text(b, "calcProd").to_string(),
                grid_version: child_int(b, "gridVersion"),
                helper_file: child_text(b, "helperFile").to_string(),
                helper: Offset { x: child_int(b, "helperX"), y: child_int(b, "helperY") },
            });
        }
    }
    Ok((version, out))
}

/// Visual effect started by a trigger.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TriggerEffect {
    pub effect: Option<Effect>,
    /// Pixel offset from the building's anchor.
    pub x: i16,
    pub y: i16,
    pub smoke: i32,
    pub duration: i32,
    /// `effect/frame` exists in the file but the reader never reads it.
    pub frame_unread: i32,
    pub sound: Option<SoundRef>,
}

/// One `race/trigger` of BuildingTrigger.xml (`m_vTriggerInfos[race][trigger]`).
#[derive(Clone, Debug)]
pub struct TriggerInfo {
    pub race: Race,
    pub name: String,
    /// Slot in the per-race table (0..47), from `TRIGGERS`.
    pub index: i32,
    /// Building part to start (`patch/def`, a .dil index), with slot and duration.
    pub patch: Option<u16>,
    pub patch_name: String,
    pub patch_slot: i32,
    pub patch_duration: i32,
    pub patch_sound: Option<SoundRef>,
    pub kind: Option<PatchType>,
    pub effect: Option<TriggerEffect>,
}

pub(crate) fn parse_triggers(text: &str, ctx: &mut Ctx) -> Result<Vec<TriggerInfo>> {
    let doc = roxmltree::Document::parse(text).map_err(|e| crate::Error::Format(format!("BuildingTrigger.xml: {e}")))?;
    let root = doc.root_element();
    let mut out = Vec::new();
    for race_node in children(root, "race") {
        let Some(race) = ctx.translator(id(race_node), Race::from_name, Race::from_u16, "race") else { continue };
        for t in children(race_node, "trigger") {
            let name = id(t).to_string();
            let Some(index) = super::defines::TRIGGERS.iter().find(|(n, _)| *n == name).map(|&(_, v)| v) else {
                ctx.warn(format!("BuildingTrigger: unknown trigger {name}"));
                continue;
            };
            let patch = child(t, "patch", 0);
            let patch_name = path_text(t, "patch", "def").to_string();
            let effect = child(t, "effect", 0).map(|e| TriggerEffect {
                effect: ctx.enum_by_define(child_text(e, "def"), Effect::from_u16, "effect"),
                x: child_int(e, "x") as i16,
                y: child_int(e, "y") as i16,
                smoke: child_int(e, "smoke"),
                duration: child_int(e, "duration"),
                frame_unread: child_int(e, "frame"),
                sound: sound_ref(e, ctx),
            });
            out.push(TriggerInfo {
                race,
                index,
                patch: if patch_name.is_empty() { None } else { ctx.define(&patch_name, "trigger patch").map(|v| v as u16) },
                patch_slot: patch.map(|p| child_int(p, "slot")).unwrap_or(0),
                patch_duration: patch.map(|p| child_int(p, "duration")).unwrap_or(0),
                patch_sound: patch.and_then(|p| sound_ref(p, ctx)),
                kind: ctx.translator(child_text(t, "type"), PatchType::from_name, PatchType::from_u16, "trigger type"),
                effect,
                patch_name,
                name,
            });
        }
    }
    Ok(out)
}
