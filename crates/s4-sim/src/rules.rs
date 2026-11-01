//! Static game rules the simulation reads: the parts of `GameData\*.xml`, the exe's built-in tables
//! and the gfx job table that change behaviour. Built once from `s4_formats` data; not part of
//! the state hash (it never changes during a game).

use s4_formats::gamedata::{self, GameData, HouseKind, Race};
use s4_formats::gfx::GfxFile;

/// `BUILDING_MAX`.
pub const BUILDING_MAX: usize = 83;
/// `SETTLER_MAX`.
pub const SETTLER_MAX: usize = 67;
/// `OBJECT_MAX`.
pub const OBJECT_MAX: usize = 291;
/// Jobs per object kept in [`Rules::object_frames`] (a tree has one per stage 0..10).
pub const OBJECT_JOBS: usize = 12;
/// Races 0..4 (Roman, Viking, Maya, Dark Tribe, Trojan).
pub const RACES: usize = 5;

/// `CSettlerMgr::m_uSettlerWarriorTypes`, filled by the `CSettlerMgr` constructor from a
/// `{type, warrior type, level, 0}` table. Found by content in the shipped `S4_Main.exe` at file
/// offset 0xc19a48 (the constructor's assert requires SWORDSMAN_01 to be non-zero; the table has
/// 29 → 2). Types absent from the table are 0 (civilians).
/// Warrior types: 1 mushroom farmer / slaved settler, 2 swordsman (melee), 3 bowman (ranged),
/// 4 medic / axe / blowgun / shaman / catapultist, 5 squad leader, 6 priest, 7 specialists
/// (saboteur, pioneer, thief, geologist, gardener, dark gardener), 14 donkey.
pub const WARRIOR_TYPES: [(u8, u8, u8); 31] = [
    (20, 0, 0),
    (29, 2, 1),
    (30, 2, 2),
    (31, 2, 3),
    (32, 3, 1),
    (33, 3, 2),
    (34, 3, 3),
    (35, 4, 1),
    (36, 4, 2),
    (37, 4, 3),
    (38, 4, 1),
    (39, 4, 2),
    (40, 4, 3),
    (41, 4, 1),
    (42, 4, 2),
    (43, 4, 3),
    (44, 5, 4),
    (45, 6, 0),
    (46, 7, 0),
    (47, 7, 0),
    (48, 7, 0),
    (49, 7, 0),
    (50, 7, 0),
    (52, 7, 0),
    (53, 1, 0),
    (54, 4, 0),
    (55, 1, 0),
    (60, 14, 0),
    (61, 4, 1),
    (62, 4, 2),
    (63, 4, 3),
];

/// What the sim needs from one `CBuildingInfoMgr::GetBuildingInfo(race, type)` record.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BuildingRule {
    /// Info bytes +40/+41 (`door` x/y offset, signed char).
    pub door: (i8, i8),
    /// Info bytes +38/+39 (`flag` x/y offset).
    pub flag: (i8, i8),
    /// Info byte +0xdd: soldier slots without `top` (swordsman capacity, warrior type 2).
    pub melee_slots: u8,
    /// Info byte +0xde: soldier slots with `top` (bowman capacity, warrior type 3).
    pub ranged_slots: u8,
    /// Info +0xdc count, +0xe0 + 12·i: the soldier slots (`settler` entries, in XML order).
    pub soldier_slots: Vec<SoldierSlot>,
    /// Info byte +0xdf: index of the first `top` slot, −1 if none (set by the buildingInfo
    /// loader 0x537b80 at 0x539722, together with +0xdd / +0xde).
    pub first_top_slot: i8,
    /// Info byte +0x1f4: hit points as stored (`Hitpoints / 2`,
    /// `s4_formats::gamedata::BuildingInfo::game_hitpoints`); `IBuildingRole::InitCommon`
    /// (0x540b20) adds them to the building's health.
    pub hitpoints: u8,
    /// Mask cells relative to the building position, in the original's walk order (rows top to
    /// bottom, bit 31 to 0): `buildingPosLines` (footprint), `blockPosLines`, `repealingPosLines`.
    pub building_cells: Vec<(i8, i8)>,
    pub block_cells: Vec<(i8, i8)>,
    pub repealing_cells: Vec<(i8, i8)>,
    /// Info byte +6: the record has water masks — set by the buildingInfo loader for the port
    /// (52..=57, 78, 79) and shipyard (58..=63, 76, 77) types only, which alone read the water
    /// masks (0x5386a2..0x5386d0).
    pub has_water_masks: bool,
    /// Info +0x2fc: `waterPosLines` cells (`DestroyBuilding` 0x53ce10 also matches them).
    pub water_cells: Vec<(i8, i8)>,
    /// Info +0x314: per row `waterPosLines | waterBlockPosLines` (loader 0x538792 / 0x53895b):
    /// `SetToWorld` makes them blocked water, `RemoveFromWorld` frees them.
    pub water_block_cells: Vec<(i8, i8)>,
    /// Info +0x320: `waterFreePosLines` cells.
    pub water_free_cells: Vec<(i8, i8)>,
    /// Where the release's `RemoveFromWorld` calls `ClearBlockedWater` for every water-free cell
    /// (0x536fc3..0x536fe9 pass the +0x314 walker's final position, not the cell's): relative
    /// to the building position, see [`walker_end`].
    pub water_free_stale: (i8, i8),
    /// Info +0x2a / +0x2b: the `workingpos` offset (loader 0x539179 / 0x5391dd).
    pub working: (i8, i8),
    /// Info byte +477: `HOUSE_KIND_*`, which picks the role (`CBuildingMgr::CreateBuildingRole`).
    pub house_kind: u8,
    /// `pile` entries (info +57 count, 16 bytes each from +60): offset, good, `type`.
    pub piles: Vec<PileRule>,
    /// Info +496 `dummyValue`: the beds of a residence (10 / 25 / 50; `GetNrOfCurrentTotalBeds`).
    pub beds: i32,
    /// Info +2 / +3 / +4: the stone, boards and gold the building costs.
    pub cost_stone: u8,
    pub cost_boards: u8,
    pub cost_gold: u8,
    /// Info bytes +0x34..+0x37: `bitBoundingRect` minX, maxX, minY, maxY (signed char; loader
    /// 0x537b80 at 0x53921f..0x5392e5). `CrushBuilding` / the destroy path pick the small or big
    /// house effect by `(maxY − minY)·(maxX − minX) < 90`.
    pub bounding_rect: [i8; 4],
    /// Info +0x1e4 `influenceRadius` (loader 0x539a31; the eco-sector AI's manager slot 0x8c).
    pub influence_radius: i32,
}

/// One soldier slot of a military building (info +0xe0 + 12·i: x, y (pixel offsets, ints),
/// +8 direction, +9 `top`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SoldierSlot {
    pub x: i32,
    pub y: i32,
    pub dir: u8,
    pub top: bool,
}

/// One buildingInfo `pile` entry (info +60.. per entry: x, y, good, type).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PileRule {
    pub x: i8,
    pub y: i8,
    pub good: u8,
    /// 0: product (placed around the ensign by the role), 1: delivery (at the offset), 4: storage.
    pub kind: u8,
    /// `patch` (→ the pile's variant byte), `appearance` (1: patch pile), pixel offsets.
    pub patch: u8,
    pub appearance: u8,
    pub pixel: (i16, i16),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rules {
    /// `[race * BUILDING_MAX + type]`; missing records read as zero (the original returns its
    /// zero-initialised table entry).
    pub buildings: Vec<BuildingRule>,
    /// `CBuildingMgr::IsMilitary(type)`: the **Roman** record's house kind is MILITARY (10) or
    /// CASTLE (15), whatever the owner's race.
    pub military: [bool; BUILDING_MAX],
    /// `SettlerInfo::m_bHealth` per `[race][type]` (Roman fallback as `CSettlerMgr::LoadInfo`).
    pub settler_health: Vec<[u8; SETTLER_MAX]>,
    /// `SettlerInfo` +3 damage and +4 armor per `[race][type]` (fights: 0x589250, 0x580020).
    pub settler_damage: Vec<[u8; SETTLER_MAX]>,
    pub settler_armor: Vec<[u8; SETTLER_MAX]>,
    /// Settler info +7 `misc` (the settler-info loader 0x583910 writes it at 0x5841b9; medics:
    /// hit points healed per heal).
    pub settler_misc: Vec<[u8; SETTLER_MAX]>,
    /// `SettlerInfo` +5, the tool a carrier needs to become this type (`GOOD_*`, 0 = none), per
    /// `[race][type]`.
    pub settler_tool: Vec<[u8; SETTLER_MAX]>,
    /// `SettlerInfo::m_uRole` per `[race][type]` (`ROLE_*`, 0 = none): the role
    /// `CSettlerMgr::CreateSettlerRole` creates.
    pub settler_role: Vec<[u8; SETTLER_MAX]>,
    /// `CBuildingSupplyPriority` defaults per race (`Config\Transport.cfg`, see `eco`).
    pub supply: Vec<crate::eco::SupplyMap>,
    /// `CSettlerMgr::SettlerWarriorType`.
    pub warrior_type: [u8; SETTLER_MAX],
    /// `CGfxManager::GetObjectFrameCount(GetObjectFirstJob(type) + k)` for k = 0..11 (trees use
    /// one job per growth / felling stage, 0..10): frames of
    /// direction 0 of that job in `Gfx\5.gfx` (`GetJobFrameCount(5, job, 0)`). Deco objects size
    /// their shared-instance pools and animation cycles from these.
    pub object_frames: Vec<[u8; OBJECT_JOBS]>,
    /// To-do lists, job sounds and settler gfx job frames (`crate::jobs::JobData`).
    pub jobs: crate::jobs::JobData,
    /// Placement and construction-site data (`build` module).
    pub build: crate::build::BuildRules,
    /// buildingInfo values of the worker-driving building roles (`buildings::work`).
    pub work: crate::buildings::work::WorkRules,
    /// `Logic/*` config values the military code reads.
    pub logic: LogicCfg,
    /// `Config\Animals.cfg` and the animal gfx frame counts (`animals`).
    pub animals: crate::animals::AnimalRules,
    /// The Dark Tribe's frame counts and mana (`dark`).
    pub dark: crate::dark::DarkRules,
    /// `Config\Magic.cfg` (`magic`).
    pub magic: crate::magic::MagicCfg,
    /// `CVehicleInfoMgr`'s table (`vehicles::info`).
    pub vehicles: crate::vehicles::info::VehicleRules,
}

/// Config values registered under `Logic` by static initialisers of the exe (key, default):
/// `MaxTowerDoorHealth` 50 (0x4247b0 → 0x11cdb5c), `MedicNumberOfHealings` 3 (0x424c40 →
/// 0x11cdbbc), `MedicTicksToRegeneration` 150 (0x424ce0 → 0x11cdbc8). The install's
/// `Config\Balancing.cfg` `[LOGIC]` sets the two medic values (1 and 28); that the config
/// manager fills the registered values from that file is an inference (its loader was not
/// traced).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogicCfg {
    pub max_tower_door_health: u8,
    pub medic_number_of_healings: u8,
    pub medic_ticks_to_regeneration: u32,
    /// `LOGIC/FightingStrengthDivisor` (0x421560 → 0x11abe20, default 1000; Balancing.cfg 1000).
    pub fighting_strength_divisor: i32,
    /// `LOGIC/TowerBowmanExtraDmg` (0x424dd0 → 0x11cdbd4, default 2; Balancing.cfg 1): added to
    /// the damage of a tower bowman's far shot.
    pub tower_bowman_extra_dmg: i32,
    /// `LOGIC/TowerStoneThrowerExtraDmg` (0x424e70 → 0x11cdbe0, default 4; Balancing.cfg 2): the
    /// first top slot's throw at an enemy next to the door.
    pub tower_stone_thrower_extra_dmg: i32,
    /// `LOGIC/TowerDoorGuardExtraDmg` (0x424f10 → 0x11cdbec, default 0; Balancing.cfg 0).
    pub tower_door_guard_extra_dmg: i32,
}

impl Default for LogicCfg {
    fn default() -> Self {
        LogicCfg {
            max_tower_door_health: 50,
            medic_number_of_healings: 3,
            medic_ticks_to_regeneration: 150,
            fighting_strength_divisor: 1000,
            tower_bowman_extra_dmg: 2,
            tower_stone_thrower_extra_dmg: 4,
            tower_door_guard_extra_dmg: 0,
        }
    }
}

impl LogicCfg {
    pub fn from_cfg(cfg: &crate::cfg::Cfg) -> LogicCfg {
        let d = LogicCfg::default();
        let get = |k: &str, d: i64| cfg.get("LOGIC", k).and_then(|v| v.parse::<i64>().ok()).unwrap_or(d);
        LogicCfg {
            max_tower_door_health: get("MaxTowerDoorHealth", d.max_tower_door_health as i64).clamp(0, 255) as u8,
            medic_number_of_healings: get("MedicNumberOfHealings", d.medic_number_of_healings as i64).clamp(0, 255) as u8,
            medic_ticks_to_regeneration: get("MedicTicksToRegeneration", d.medic_ticks_to_regeneration as i64).max(0) as u32,
            fighting_strength_divisor: get("FightingStrengthDivisor", d.fighting_strength_divisor as i64) as i32,
            tower_bowman_extra_dmg: get("TowerBowmanExtraDmg", d.tower_bowman_extra_dmg as i64) as i32,
            tower_stone_thrower_extra_dmg: get("TowerStoneThrowerExtraDmg", d.tower_stone_thrower_extra_dmg as i64) as i32,
            tower_door_guard_extra_dmg: get("TowerDoorGuardExtraDmg", d.tower_door_guard_extra_dmg as i64) as i32,
        }
    }
}

/// The mask walker 0x535ac0 (`{&vector, x0, y0, word, index, x, y}`, started with `x0 = pos.x −
/// hotspot.x − 1`, `y0 = pos.y − hotspot.y` and the first row loaded): where it stands once it
/// has run out, relative to the building position. Every row is read (y ends on the last row);
/// x is just past the last row's lowest set bit, or back at `x0` when that row is empty.
pub fn walker_end(lines: &[u32], hotspot: (i32, i32)) -> (i8, i8) {
    let x0 = -hotspot.0 - 1;
    let y = -hotspot.1 + lines.len().saturating_sub(1) as i32;
    let x = match lines.last() {
        Some(&w) if w != 0 => x0 + 1 + (31 - w.trailing_zeros() as i32),
        _ => x0,
    };
    (x as i8, y as i8)
}

impl Rules {
    /// `gfx5` is `Gfx\5.gfx` (the landscape-object file).
    pub fn new(data: &GameData, gfx5: &GfxFile) -> Rules {
        let mut buildings = vec![BuildingRule::default(); RACES * BUILDING_MAX];
        for b in &data.buildings {
            let (r, t) = (b.race as usize, b.building as usize);
            if r >= RACES || t >= BUILDING_MAX {
                continue;
            }
            let top = b.soldiers.iter().filter(|s| s.top).count();
            let cells = |lines: &[u32]| b.mask_cells(lines).iter().map(|c| (c.x as i8, c.y as i8)).collect();
            let water = matches!(t, 52..=63 | 76..=79);
            let row = |v: &[u32], i: usize| v.get(i).copied().unwrap_or(0);
            let water_block: Vec<u32> = if water { (0..b.lines).map(|i| row(&b.water_pos_lines, i) | row(&b.water_block_pos_lines, i)).collect() } else { Vec::new() };
            let only_water = |v: &[u32]| if water { cells(v) } else { Vec::new() };
            buildings[r * BUILDING_MAX + t] = BuildingRule {
                door: (b.door.x as i8, b.door.y as i8),
                flag: (b.flag.x as i8, b.flag.y as i8),
                melee_slots: (b.soldiers.len() - top) as u8,
                ranged_slots: top as u8,
                soldier_slots: b.soldiers.iter().map(|s| SoldierSlot { x: s.x, y: s.y, dir: s.direction as u8, top: s.top }).collect(),
                first_top_slot: b.soldiers.iter().position(|s| s.top).map_or(-1, |i| i as i8),
                hitpoints: b.game_hitpoints().clamp(0, 255) as u8,
                building_cells: cells(&b.building_pos_lines),
                block_cells: cells(&b.block_pos_lines),
                repealing_cells: cells(&b.repealing_pos_lines),
                has_water_masks: water,
                water_cells: only_water(&b.water_pos_lines),
                water_block_cells: cells(&water_block),
                water_free_cells: only_water(&b.water_free_pos_lines),
                water_free_stale: walker_end(&water_block, (b.hotspot.x, b.hotspot.y)),
                working: (b.working_pos.x as i8, b.working_pos.y as i8),
                house_kind: b.kind.map_or(0, |k| k as u8),
                beds: b.dummy_value,
                cost_stone: b.stone.clamp(0, 255) as u8,
                cost_boards: b.boards.clamp(0, 255) as u8,
                cost_gold: b.gold.clamp(0, 255) as u8,
                bounding_rect: [b.bounding_rect.min_x, b.bounding_rect.max_x, b.bounding_rect.min_y, b.bounding_rect.max_y].map(|v| v as i8),
                influence_radius: b.influence_radius,
                piles: b
                    .piles
                    .iter()
                    .map(|p| PileRule {
                        x: p.x as i8,
                        y: p.y as i8,
                        good: p.good.map_or(0, |g| g as u8),
                        kind: p.kind as u8,
                        patch: p.patch as u8,
                        appearance: p.appearance as u8,
                        pixel: (p.x_pixel as i16, p.y_pixel as i16),
                    })
                    .collect(),
            };
        }
        let mut military = [false; BUILDING_MAX];
        for b in data.buildings.iter().filter(|b| b.race == Race::ROMAN) {
            if let Some(m) = military.get_mut(b.building as usize) {
                *m = matches!(b.kind, Some(HouseKind::MILITARY | HouseKind::CASTLE));
            }
        }
        let mut settler_health = vec![[0u8; SETTLER_MAX]; RACES];
        let mut settler_damage = vec![[0u8; SETTLER_MAX]; RACES];
        let mut settler_armor = vec![[0u8; SETTLER_MAX]; RACES];
        let mut settler_misc = vec![[0u8; SETTLER_MAX]; RACES];
        let mut settler_tool = vec![[0u8; SETTLER_MAX]; RACES];
        let mut settler_role = vec![[0u8; SETTLER_MAX]; RACES];
        for &race in Race::ALL {
            for t in 0..SETTLER_MAX {
                let info = gamedata::Settler::from_u16(t as u16).and_then(|s| data.settler(race, s));
                settler_health[race as usize][t] = info.map_or(0, |i| i.health.clamp(0, 255) as u8);
                settler_damage[race as usize][t] = info.map_or(0, |i| i.damage.clamp(0, 255) as u8);
                settler_armor[race as usize][t] = info.map_or(0, |i| i.armor.clamp(0, 255) as u8);
                settler_misc[race as usize][t] = info.map_or(0, |i| i.misc as u8);
                settler_tool[race as usize][t] = info.and_then(|i| i.tool).map_or(0, |g| g as u8);
                settler_role[race as usize][t] = info.and_then(|i| i.role).map_or(0, |r| r as u8);
            }
        }
        let mut warrior_type = [0u8; SETTLER_MAX];
        for &(t, w, _) in &WARRIOR_TYPES {
            warrior_type[t as usize] = w;
        }
        let groups = gfx5.job_groups();
        let object_frames = (0..OBJECT_MAX)
            .map(|t| {
                let first = groups.get(t).copied().unwrap_or(0) as usize;
                std::array::from_fn(|k| {
                    gfx5.job_frames(first + k, 0).map_or(0, |r| r.len().min(255) as u8)
                })
            })
            .collect();
        Rules {
            buildings,
            military,
            settler_health,
            settler_damage,
            settler_armor,
            settler_misc,
            settler_tool,
            settler_role,
            supply: vec![Default::default(); RACES],
            warrior_type,
            object_frames,
            jobs: Default::default(),
            build: Default::default(),
            work: Default::default(),
            logic: Default::default(),
            animals: Default::default(),
            dark: Default::default(),
            magic: Default::default(),
            vehicles: Default::default(),
        }
    }

    /// Load everything from the default install.
    pub fn load_default() -> s4_formats::Result<Rules> {
        let data = GameData::load_default()?;
        let gfx5 = GfxFile::open_default(5)?;
        let mut rules = Rules::new(&data, &gfx5);
        rules.jobs = crate::jobs::JobData::load(&data)?;
        rules.build = crate::build::BuildRules::new(&data);
        let transport = crate::cfg::Cfg::load_default("Transport.cfg");
        rules.supply = (0..RACES as u8).map(|r| crate::eco::default_supply(&transport, r)).collect();
        rules.work = crate::buildings::work::WorkRules::new(&data);
        rules.logic = LogicCfg::from_cfg(&crate::cfg::Cfg::load_default("Balancing.cfg"));
        rules.animals = crate::animals::AnimalRules::load_default();
        rules.dark = crate::dark::DarkRules::new(&data);
        rules.magic = crate::magic::MagicCfg::load_default();
        rules.vehicles = crate::vehicles::info::VehicleRules::new(&data);
        rules.vehicles.load_gfx();
        Ok(rules)
    }

    pub fn building(&self, race: u8, building: u8) -> &BuildingRule {
        static NONE: BuildingRule = BuildingRule {
            door: (0, 0),
            flag: (0, 0),
            melee_slots: 0,
            ranged_slots: 0,
            soldier_slots: Vec::new(),
            first_top_slot: -1,
            hitpoints: 0,
            building_cells: Vec::new(),
            block_cells: Vec::new(),
            repealing_cells: Vec::new(),
            has_water_masks: false,
            water_cells: Vec::new(),
            water_block_cells: Vec::new(),
            water_free_cells: Vec::new(),
            water_free_stale: (0, 0),
            working: (0, 0),
            house_kind: 0,
            piles: Vec::new(),
            beds: 0,
            cost_stone: 0,
            cost_boards: 0,
            cost_gold: 0,
            bounding_rect: [0; 4],
            influence_radius: 0,
        };
        self.buildings.get(race as usize * BUILDING_MAX + building as usize).unwrap_or(&NONE)
    }

    pub fn is_military(&self, building: u16) -> bool {
        self.military.get(building as usize).copied().unwrap_or(false)
    }

    /// `CSettlerMgr::GetSettlerInfo(race, type)` +5: the tool good (0 = none).
    pub fn settler_tool(&self, race: u8, settler: u16) -> u8 {
        self.settler_tool.get(race as usize).and_then(|r| r.get(settler as usize)).copied().unwrap_or(0)
    }

    /// `SettlerInfo::m_uRole`.
    pub fn settler_role(&self, race: u8, settler: u16) -> u8 {
        self.settler_role.get(race as usize).and_then(|r| r.get(settler as usize)).copied().unwrap_or(0)
    }

    pub fn settler_damage(&self, race: u8, settler: u16) -> u8 {
        self.settler_damage.get(race as usize).and_then(|r| r.get(settler as usize)).copied().unwrap_or(0)
    }

    pub fn settler_armor(&self, race: u8, settler: u16) -> u8 {
        self.settler_armor.get(race as usize).and_then(|r| r.get(settler as usize)).copied().unwrap_or(0)
    }

    pub fn settler_misc(&self, race: u8, settler: u16) -> u8 {
        self.settler_misc.get(race as usize).and_then(|r| r.get(settler as usize)).copied().unwrap_or(0)
    }

    /// .data 0x12dbc40 `[race]`, written by the settler-info loader 0x583910 (0x584855..0x58492a)
    /// for races 0..4: the critical-hit factor (/256) of soldiers grouped with a squad leader
    /// ([`crate::jobs::warrior::hit`]). Race 0 gets `v · 256 / 100` with `v` the Roman squad
    /// leader's (type 0x2c) info +7, 10 when that is 0; every other race the **Roman** value if its
    /// own squad leader's +7 is not 0, else 0 (that is the release code).
    pub fn leader_bonus(&self, race: u8) -> i32 {
        let v = match self.settler_misc(0, 0x2c) {
            0 => 10,
            v => v as i32,
        };
        let roman = (v << 8) / 100;
        match race {
            0 => roman,
            1..=4 if self.settler_misc(race, 0x2c) != 0 => roman,
            _ => 0,
        }
    }

    /// Settler info +7 of `(race, type)` as a chance out of 65536 (`(v << 16) / 100`), or
    /// `default` when it is 0: how the settler-info loader 0x583910 fills the indexed .data
    /// tables (0x584981..0x584a50).
    fn info_chance(&self, race: u8, settler: u16, default: u32) -> u32 {
        match self.settler_misc(race, settler) {
            0 => default,
            v => ((v as u32) << 16) / 100,
        }
    }

    /// .data 0x11cdad0 `[type]` for the blowgun warriors 0x29..0x2b: the freeze chance out of
    /// 65536, from the **Maya** (race 2) info +7 of the type, else the static 36044 / 42598 /
    /// 49152 (loader 0x584991 / 0x5849b0 / 0x5849cf). The shipped data has 0 there.
    pub fn freeze_chance(&self, settler: u16) -> u32 {
        const DEFAULT: [u32; 3] = [36044, 42598, 49152];
        let k = settler.wrapping_sub(0x29) as usize;
        DEFAULT.get(k).map_or(0, |&d| self.info_chance(2, settler, d))
    }

    /// .data 0x1165374 `[type]` for the backpack catapultists 0x3d..0x3f: the splash percentage,
    /// from the **Maya** (race 2, not the Trojan) info +7 of the type, else 55 / 65 / 75, which the
    /// loader also writes back into that info byte (0x5849d5..0x584a30). The shipped data has 0
    /// for the Maya types (the Trojan ones have 50 / 55 / 50 and are not read).
    pub fn splash_percent(&self, settler: u16) -> i32 {
        const DEFAULT: [i32; 3] = [55, 65, 75];
        let k = settler.wrapping_sub(0x3d) as usize;
        DEFAULT.get(k).map_or(0, |&d| match self.settler_misc(2, settler) {
            0 => d,
            v => v as i32,
        })
    }

    /// .data 0x11cdb80: the saboteur's hit chance out of 65536, from the Roman saboteur's (type
    /// 0x2e) info +7, else 49152 (loader 0x584a50; read by `CSaboteurRole` 0x57fde7). The
    /// shipped data has 0 there: 75 %.
    pub fn saboteur_chance(&self) -> u32 {
        self.info_chance(0, 0x2e, 49152)
    }

    pub fn settler_health(&self, race: u8, settler: u16) -> u8 {
        self.settler_health.get(race as usize).and_then(|r| r.get(settler as usize)).copied().unwrap_or(0)
    }

    pub fn warrior_type(&self, settler: u16) -> u8 {
        self.warrior_type.get(settler as usize).copied().unwrap_or(0)
    }

    pub fn object_frames(&self, object: u16, k: usize) -> u8 {
        self.object_frames.get(object as usize).and_then(|f| f.get(k)).copied().unwrap_or(0)
    }
}
