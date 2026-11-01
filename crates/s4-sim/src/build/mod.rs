//! Placing buildings: the placement rules (`CBuildingMgr::CheckForBuild` and
//! `CheckForBuildCalc`), the possible-sites query the build menu shows as markers
//! (`CInputProcessor::GetPossibleBuildingSites`), and construction sites (`AddBuilding` with a
//! `CBuildingSiteRole`, command 5025).
//!
//! Sources: the debug decompilation (MapObjects/Building/CBuildingMgr.cpp,
//! CBuildingSiteRole.cpp, Logic/CInputProcessor.cpp) for the logic; the shipped exe for what the
//! decompilation hides or might misplace:
//! * `CheckForBuild` (release 0x53e4c0..0x53eb54) walks `buildingPosLines` (info +0x2cc) for the
//!   footprint and `digPosLines` (+0x2d8) for the leveling height; `CheckForBuildCalc`
//!   (0x53dc90) stamps `digPosLines` too, and resets its queue *inside* the pass loop, so the
//!   slope propagation is a single pass (see [`check::calc`]);
//! * `CBuildingSiteRole::PrepareGround` (0x543680) walks `buildingPosLines`; the site's state-4
//!   update (0x541dee) walks `blockPosLines`;
//! * `IGfxEngine::CanChangeGround` → `CheckField` (0x66d240) and its ground-adjacency lists
//!   (.rdata 0x1027078, see [`site::GROUND_NEIGHBOURS`]).
//!
//! The site's orders for diggers, builders and material are in [`order`] (from the release:
//! `OrderDigger` 0x5425f0, `OrderBuilder` 0x5426b0, `OrderMaterial` 0x5427f0); the workers
//! themselves are `jobs::digger` / `jobs::builder`.
//! Dark Tribe placement is `check::check_dark_tribe` (0x53ebc0) with its own instant levelling
//! (`site::level_dark_tribe`). Ports and shipyards: `check::water_variant` (0x53f210) picks
//! the orientation (type 52..=63, 76..=79) that `CheckForBuild` then levels for. Not ported:
//! statistics.

pub mod check;
pub mod halls;
pub mod order;
pub mod scripted;
pub mod site;

use std::collections::BTreeMap;

use s4_formats::gamedata::{Building, GameData, HouseKind, Object, Race};

use crate::hash::StateHasher;

pub use check::{CheckResult, Why, check_for_build, dig_plan, possible_sites};
pub use site::{Site, add_building_site, add_work, building_logic_update, command_build};

const RACES: usize = 5;
const BUILDING_MAX: usize = 83;
const OBJECT_MAX: usize = 291;

/// What placement and construction need of a `buildingInfo.xml` record (`CBuildingInfoMgr`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BuildInfo {
    /// Info bytes +0/+1, `iHotSpotX/Y` (signed char).
    pub hotspot: (i8, i8),
    /// `buildingPosLines` (+0x2cc in the release struct), one u32 per mask row.
    pub building_lines: Vec<u32>,
    /// `digPosLines` (+0x2d8).
    pub dig_lines: Vec<u32>,
    /// `blockPosLines` (+0x2e4).
    pub block_lines: Vec<u32>,
    /// Info bytes +2/+3/+4: stone, boards, gold (construction cost).
    pub stone: u8,
    pub boards: u8,
    pub gold: u8,
    /// Info +38/+39 `flag`, +40/+41 `door` offsets.
    pub flag: (i8, i8),
    pub door: (i8, i8),
    /// Info +7 `builderNumber`, +8 `builderInfo` (x, y offset, dir) per builder spot.
    pub builders: Vec<(i8, i8, i8)>,
    /// Ports and shipyards only (the loader reads these masks for types 52..=63, 76..=79):
    /// `waterPosLines` (+0x2fc), `waterBlockPosLines` (+0x308, loader 0x5388e2) and
    /// `waterFreePosLines` (+0x320).
    pub water_lines: Vec<u32>,
    pub water_block_lines: Vec<u32>,
    pub water_free_lines: Vec<u32>,
    /// Info +0x2a / +0x2b `workingpos`.
    pub working: (i8, i8),
    /// Info +0x34..+0x37 `bitBoundingRect` minX, maxX, minY, maxY.
    pub bounds: [i8; 4],
}

impl BuildInfo {
    /// `CBuildingFlagsWalk(x, y, hotX, hotY, lines)`: the mask cells in walk order (rows top to
    /// bottom, bit 31 = column 0 first), as world coordinates.
    pub fn cells<'a>(&self, lines: &'a [u32], x: i32, y: i32) -> impl Iterator<Item = (i32, i32)> + 'a {
        let (hx, hy) = (self.hotspot.0 as i32, self.hotspot.1 as i32);
        lines.iter().enumerate().flat_map(move |(r, &m)| {
            (0..32).filter(move |c| m & (0x8000_0000u32 >> c) != 0).map(move |c| (x - hx + c, y - hy + r as i32))
        })
    }
}

/// `CDecoObjMgr`'s per-object flag info (8 bytes at +38444): repelling, blocking rings, total rings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DecoFlags {
    pub repelling: bool,
    pub rings: u8,
    pub total: u8,
}

/// Rules of this module, from `GameData` (stored in [`crate::Rules::build`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BuildRules {
    /// `[race * BUILDING_MAX + type]`.
    pub infos: Vec<BuildInfo>,
    /// `CBuildingMgr::IsMine(type)`: the **Roman** record's house kind is MINE (7).
    pub mine: Vec<bool>,
    /// Per object id.
    pub deco_flags: Vec<DecoFlags>,
}

impl BuildRules {
    pub fn new(data: &GameData) -> BuildRules {
        let mut infos = vec![BuildInfo::default(); RACES * BUILDING_MAX];
        let mut mine = vec![false; BUILDING_MAX];
        for b in &data.buildings {
            let (r, t) = (b.race as usize, b.building as usize);
            if r >= RACES || t >= BUILDING_MAX {
                continue;
            }
            let water = matches!(t, 52..=63 | 76..=79);
            infos[r * BUILDING_MAX + t] = BuildInfo {
                hotspot: (b.hotspot.x as i8, b.hotspot.y as i8),
                building_lines: b.building_pos_lines.clone(),
                dig_lines: b.dig_pos_lines.clone(),
                block_lines: b.block_pos_lines.clone(),
                stone: b.stone.clamp(0, 255) as u8,
                boards: b.boards.clamp(0, 255) as u8,
                gold: b.gold.clamp(0, 255) as u8,
                flag: (b.flag.x as i8, b.flag.y as i8),
                door: (b.door.x as i8, b.door.y as i8),
                builders: b.builders.iter().map(|s| (s.x as i8, s.y as i8, s.dir as i8)).collect(),
                water_lines: if water { b.water_pos_lines.clone() } else { Vec::new() },
                water_block_lines: if water { b.water_block_pos_lines.clone() } else { Vec::new() },
                water_free_lines: if water { b.water_free_pos_lines.clone() } else { Vec::new() },
                working: (b.working_pos.x as i8, b.working_pos.y as i8),
                bounds: [b.bounding_rect.min_x, b.bounding_rect.max_x, b.bounding_rect.min_y, b.bounding_rect.max_y].map(|v| v as i8),
            };
            if b.race == Race::ROMAN {
                mine[t] = b.kind == Some(HouseKind::MINE);
            }
        }
        // `ReadObjectInfos`: repelling non-blocking objects get one repelling ring, the rest
        // `blocking` blocked rings and `building` more rings where nothing may be built; objects
        // without a record (e.g. the construction stakes, OBJECT_PEG01) stay zero.
        let mut deco_flags = vec![DecoFlags::default(); OBJECT_MAX];
        for o in &data.objects {
            let Some(f) = deco_flags.get_mut(o.object as usize) else { continue };
            let (blocking, building) = (o.blocking.clamp(0, 255) as u8, o.game_building().clamp(0, 255) as u8);
            *f = if o.blocking <= 0 && o.game_repellent() {
                DecoFlags { repelling: true, rings: 1, total: building + 1 }
            } else {
                DecoFlags { repelling: false, rings: blocking, total: building.saturating_add(blocking) }
            };
        }
        BuildRules { infos, mine, deco_flags }
    }

    pub fn info(&self, race: u8, building: u8) -> Option<&BuildInfo> {
        self.infos.get(race as usize * BUILDING_MAX + building as usize).filter(|i| !i.building_lines.is_empty())
    }

    pub fn is_mine(&self, building: u8) -> bool {
        self.mine.get(building as usize).copied().unwrap_or(false)
    }
}

/// One of a player's 30 digging slots (`CBuildingMgr + 11981 + 30750·player + 1025·slot`): the
/// target height of every node of the 31×31 window around a site that the diggers must change
/// (0 = leave as is), index `32·dx + dy` with `dx, dy` = offset + 15.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DigSlot {
    pub heights: Vec<u8>,
}

/// Construction sites and the digging slots (state owned by this module).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Sites {
    /// `CBuildingSiteRole` per building entity index.
    pub roles: BTreeMap<u16, Site>,
    /// `[player][slot]`, 30 slots per player; `None` = free.
    pub dig_slots: BTreeMap<(u8, u8), DigSlot>,
    /// The manakopter halls (see [`halls`]): per player 0..=8 the positions halls were placed at
    /// (`CBuildingMgr +0x46fdc`, 20 each, count +0x472ac) and the tick a hall was last destroyed
    /// (`+0x472d0`).
    #[cfg_attr(feature = "serde", serde(default))]
    pub halls: halls::HallList,
}

impl Sites {
    pub(crate) fn hash(&self, h: &mut StateHasher) {
        self.halls.hash(h);
        h.u32(self.roles.len() as u32);
        for (id, s) in &self.roles {
            h.u16(*id);
            s.hash(h);
        }
        h.u32(self.dig_slots.len() as u32);
        for ((p, s), d) in &self.dig_slots {
            h.u8(*p);
            h.u8(*s);
            h.bytes(&d.heights);
        }
    }
}

/// Building type ids used here.
pub(crate) const MUSHROOMFARM: u16 = Building::MUSHROOMFARM as u16;
pub(crate) const MANACOPTERHALL: u16 = Building::MANACOPTERHALL as u16;
pub(crate) const PEG: u16 = Object::PEG01 as u16;
