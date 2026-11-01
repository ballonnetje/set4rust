//! `CAnimalMgr` (release .bss 0x149c488) and its land animals (`CAnimal` vtable 0x1058008,
//! `CLandAnimal` 0x105821c). Own reverse engineering of the shipped exe; decompiles in
//! `tools/re/decompiled/script/animals/`.
//!
//! **Where animals come from.** Only from the manager's spawning in [`update`]: maps place none
//! (the release `LoadSettlerData` 0x584b10 skips settler records of types 67..199, and no other
//! loader creates a `CLandAnimal`; its constructor 0x5312f0 has the one caller 0x52fe70). The other
//! creator, 0x4f4190, is a spell that turns settlers into deer (not ported).
//!
//! **Which RNG.** Spawning and behaviour draw from the **game** RNG (`CStateGame +0x5c`, the same
//! object as `GlobalGameStateObject +0x2c`; [`World::rng`]). The manager's own RNG (+0x168,
//! seeded 0x29061971 by the constructor 0x52d6a0; [`World::animal_rng`]) and the 0x12cc2a5-seeded
//! one at +0x170 only drive the ambient effects (below).
//!
//! **Not simulation: the ambient effects.** The rest of `CAnimalMgr::Update` (0x52edf0) — every
//! 11th tick a ground effect near the screen centre (0x52ff10), every 3rd a jumping fish near it
//! (0x5301c0), butterflies (0x5304d0), birds (0x5305e0, effect 0xe), seagulls (0x530730, 0xf/0x10)
//! and the duck pass (inline, effect 0x11 on the lake cells) — creates `CEffects` objects
//! (`CLogic +0x24`, their own id table of 0xfff, not entities) and future events of kind 7 that
//! move them (0x52cb30 → 0x52ce30 / 0x52d050 / 0x52d280). Two of them start from
//! `GetClosestMapPoint` of the screen centre, so this part depends on the local camera and cannot
//! feed back into the lockstep simulation. They also draw the manager's RNG a camera-dependent
//! number of times (0x52ffe0 only when the cell under the screen centre qualifies, 0x5301c0 once
//! per node it scans), so the duck pass's draws from the same RNG — and the effect table, its free
//! list and per-type counts (`CEffects` vtbl+0x28 0x4e3d60 / +0x30 0x4e3e50) — differ per client
//! too. Nothing in the simulation reads them. It all belongs to the client (see `docs/JOBS.md`).
//!
//! The land statistics the spawning reads are the AI's cell grid ([`cells`]).

use crate::entity::{Entity, EntityData, ObjType, flag};
use crate::grid::Y16X16;
use crate::hash::StateHasher;
use crate::pathing::walking::WalkState;
use crate::world::World;

pub mod cells;
pub mod fish;
mod land;
pub mod save;

pub use land::{logic_update, start_dying};

/// `ANIMAL_*` types 1..17 (`SConfigTypeString` 0x1003ed8; section names 0x10156dc).
pub const TYPES: usize = 18;
/// `ANIMAL_LAND_*` (define table 0x1003278: `{value, name}` pairs).
pub mod land_type {
    pub const NONE: i32 = 0;
    pub const GRASS: i32 = 1;
    pub const PLAIN: i32 = 2;
    pub const LIGHTFOREST: i32 = 3;
    pub const DEEPFOREST: i32 = 4;
    pub const SNOW: i32 = 5;
    pub const ROCKS: i32 = 6;
    pub const WATER: i32 = 7;
    pub const DARK: i32 = 8;
    pub const TOWN: i32 = 9;
    pub const AIRFX: i32 = 10;
    pub const LAKEFX: i32 = 11;
    pub const FLOWERFX: i32 = 12;
    pub const COAST: i32 = 13;
}
/// Land-type lists of the manager (`+0x60`, 17 vectors).
const LISTS: usize = 17;
/// What a land animal yields (`CLandAnimal` vtbl+0x3c, 0x5327f0): `GOOD_MEAT`.
pub const MEAT: u8 = 24;
/// `ANIMAL_RABBIT` / `ANIMAL_RABBIT2`: a rabbit spawn is a RABBIT2 half of the time.
const RABBIT: u16 = 9;
const RABBIT2: u16 = 16;

/// `Config\Animals.cfg` as `CAnimalMgr::LoadAnimalData` (0x52f1d0) and `Init` (0x52ddf0) read it
/// (key, default), plus the frame counts of the animal gfx jobs. Fixed per installation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AnimalRules {
    /// `[ANIMAL_DATA]`: MAX_ANIMALS 1000, MAX_MYSTIC 4, PLAIN_TREES 1, LIGHT_TREES 4, DEEP_TREES 8,
    /// LAND_POP 8, SEARCH_RAD 10, HUNT_PERCENT 50 (the install: 1000, 3, 1, 6, 15, 50, 10, 50).
    /// The AMOUNT_* effect counts are not simulation.
    pub max_animals: i32,
    pub max_mystic: i32,
    pub plain_trees: i32,
    pub light_trees: i32,
    pub deep_trees: i32,
    pub land_pop: i32,
    pub search_rad: i32,
    pub hunt_percent: i32,
    /// `[ANIMAL_DATA]` FISH_MIN (default 2; the install: 3): the jumping fish need more fish
    /// than this on a node (client side, [`fish::jump`]).
    pub fish_min: i32,
    /// Per type: its section, `None` when absent ("No config data for animal %s available"):
    /// FISH has none.
    pub types: Vec<Option<AnimalCfg>>,
    /// `GetJobFrameCount(8, job, 2)` for every job of `Gfx\8.gfx` (the animal file; 1 when empty,
    /// as 0x461fe0 returns).
    pub frames: Vec<u8>,
}

/// One `[ANIMAL_*]` section (LAND_TYPE 0, HUNTABLE 0, RUNAWAY 0, RACE −1; SPEED_DIVIDER and the
/// sounds are read too but only the client uses them; MIN_TREES / MAX_TREES / MIN_GROUND are in
/// the file but never read).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AnimalCfg {
    pub land: i32,
    pub huntable: bool,
    pub runaway: bool,
    pub race: i32,
}

impl AnimalRules {
    /// From `Config\Animals.cfg` and `Gfx\8.gfx` of the install (empty rules when missing: no
    /// animals spawn).
    pub fn load_default() -> AnimalRules {
        let cfg = crate::cfg::Cfg::load_default("Animals.cfg");
        let frames = s4_formats::gfx::GfxFile::open_default(8)
            .map(|g| (0..g.job_count()).map(|j| g.job_frames(j, 2).map_or(1, |r| r.len().clamp(1, 255) as u8)).collect())
            .unwrap_or_default();
        AnimalRules::from_cfg(&cfg, frames)
    }

    pub fn from_cfg(cfg: &crate::cfg::Cfg, frames: Vec<u8>) -> AnimalRules {
        let int = |sec: &str, key: &str, d: i32| cfg.get(sec, key).map_or(d, |v| define(v).unwrap_or(d));
        const NAMES: [&str; TYPES] = [
            "ANIMAL_NO_ANIMAL", "ANIMAL_BOAR", "ANIMAL_DARKBUG", "ANIMAL_DARKBUGBIG", "ANIMAL_DEERFE",
            "ANIMAL_DEERMA", "ANIMAL_FOX", "ANIMAL_CHICKEN", "ANIMAL_JAGUAR", "ANIMAL_RABBIT", "ANIMAL_DOLPHIN",
            "ANIMAL_NESSY", "ANIMAL_SHARK", "ANIMAL_WILDBEAR", "ANIMAL_WOLF", "ANIMAL_FISH", "ANIMAL_RABBIT2",
            "ANIMAL_BULL",
        ];
        let types = (0..TYPES)
            .map(|t| {
                let s = NAMES[t];
                (t >= 1 && cfg.has_section(s)).then(|| AnimalCfg {
                    land: int(s, "LAND_TYPE", 0),
                    huntable: int(s, "HUNTABLE", 0) != 0,
                    runaway: int(s, "RUNAWAY", 0) != 0,
                    race: int(s, "RACE", -1),
                })
            })
            .collect();
        let d = "ANIMAL_DATA";
        AnimalRules {
            max_animals: int(d, "MAX_ANIMALS", 1000),
            max_mystic: int(d, "MAX_MYSTIC", 4),
            plain_trees: int(d, "PLAIN_TREES", 1),
            light_trees: int(d, "LIGHT_TREES", 4),
            deep_trees: int(d, "DEEP_TREES", 8),
            land_pop: int(d, "LAND_POP", 8),
            search_rad: int(d, "SEARCH_RAD", 10),
            hunt_percent: int(d, "HUNT_PERCENT", 50),
            fish_min: int(d, "FISH_MIN", 2),
            types,
            frames,
        }
    }

    fn frames(&self, job: u16) -> u8 {
        self.frames.get(job as usize).copied().unwrap_or(1)
    }
}

/// A config value: a number or a `<DEFINE>` of the land-type table.
fn define(v: &str) -> Option<i32> {
    let v = v.trim();
    if let Some(name) = v.strip_prefix('<').and_then(|s| s.strip_suffix('>')) {
        const LAND: [&str; 14] = [
            "ANIMAL_LAND_NONE", "ANIMAL_LAND_GRASS", "ANIMAL_LAND_PLAIN", "ANIMAL_LAND_LIGHTFOREST",
            "ANIMAL_LAND_DEEPFOREST", "ANIMAL_LAND_SNOW", "ANIMAL_LAND_ROCKS", "ANIMAL_LAND_WATER",
            "ANIMAL_LAND_DARK", "ANIMAL_LAND_TOWN", "ANIMAL_LAND_AIRFX", "ANIMAL_LAND_LAKEFX",
            "ANIMAL_LAND_FLOWERFX", "ANIMAL_LAND_COAST",
        ];
        return LAND.iter().position(|&l| l.eq_ignore_ascii_case(name)).map(|i| i as i32);
    }
    v.parse().ok()
}

/// The type table entry (`0x1298a00 + 40 * type`) as loaded for this game.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AnimalType {
    /// +0 LAND_TYPE.
    pub land: i32,
    /// +4 HUNTABLE, +5 RUNAWAY.
    pub huntable: bool,
    pub runaway: bool,
    /// +8 / +0xc: the tree range of the land type (plain: PLAIN_TREES .. LIGHT_TREES − 1, light
    /// forest: LIGHT_TREES − 1 .. DEEP_TREES − 1, deep forest: DEEP_TREES .. 255, else 0 / 0).
    pub min_trees: i32,
    pub max_trees: i32,
    /// +0x14 RACE (−1 = any).
    pub race: i32,
    /// +0x18 SEARCH_RAD.
    pub search_rad: i32,
    /// +0x1c: live non-huntable animals of this type that have a race (capped by MAX_MYSTIC).
    pub mystic: i32,
}

/// `CAnimalMgr` state.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AnimalMgr {
    /// +0x11c / +0x120: cells per row / column (`MapSize >> 4`).
    pub cells_w: u32,
    pub cells_h: u32,
    /// +0x160: the update counter (it paces the ambient effects only).
    pub ticks: i32,
    /// +0xf8: live land animals.
    pub total: i32,
    /// +0xfc: live non-huntable ones.
    pub non_huntable: i32,
    /// +0x100: the land-animal cap.
    pub max: i32,
    /// +0x104: the cap for non-huntable ones.
    pub max_non_huntable: i32,
    /// +0x108: MAX_MYSTIC.
    pub max_mystic: i32,
    /// +0xe8..: LAND_POP, PLAIN_TREES, LIGHT_TREES, DEEP_TREES.
    pub land_pop: i32,
    pub plain_trees: i32,
    pub light_trees: i32,
    pub deep_trees: i32,
    /// The type table (0x1298a00); index 0 and unloaded types are all zero.
    pub types: Vec<AnimalType>,
    /// +0x60: per land type the loaded types that live there, in type order.
    pub lists: Vec<Vec<u8>>,
    /// The AI cell grid the spawning reads.
    pub cells: cells::LandCells,
}

impl Default for AnimalMgr {
    fn default() -> Self {
        AnimalMgr {
            cells_w: 0,
            cells_h: 0,
            ticks: 0,
            total: 0,
            non_huntable: 0,
            max: 0,
            max_non_huntable: 0,
            max_mystic: 0,
            land_pop: 0,
            plain_trees: 0,
            light_trees: 0,
            deep_trees: 0,
            types: vec![AnimalType::default(); TYPES],
            lists: vec![Vec::new(); LISTS],
            cells: Default::default(),
        }
    }
}

impl AnimalMgr {
    pub(crate) fn hash(&self, h: &mut StateHasher) {
        for v in [
            self.cells_w as i32,
            self.cells_h as i32,
            self.ticks,
            self.total,
            self.non_huntable,
            self.max,
            self.max_non_huntable,
            self.max_mystic,
            self.land_pop,
            self.plain_trees,
            self.light_trees,
            self.deep_trees,
        ] {
            h.u32(v as u32);
        }
        for t in &self.types {
            for v in [t.land, t.min_trees, t.max_trees, t.race, t.search_rad, t.mystic] {
                h.u32(v as u32);
            }
            h.bool(t.huntable);
            h.bool(t.runaway);
        }
        for l in &self.lists {
            h.u32(l.len() as u32);
            h.bytes(l);
        }
        self.cells.hash(h);
    }

    fn huntable(&self, kind: u16) -> bool {
        self.types.get(kind as usize).is_some_and(|t| t.huntable)
    }
}

/// Per-animal state (`CAnimal` + `CLandAnimal` members).
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AnimalData {
    /// +0x58: 0 waiting, 1 idle animation, 2 walking, 3 fleeing, 4 dying, 5 dead.
    pub state: u8,
    /// +0x54: the current task of the type's list ([`task`]).
    pub task: u8,
    /// +0x40: direction 0..5.
    pub dir: u8,
    /// +0x6a: the task type (0x11 stand, 0x10 idle animation, 6 walk, 0x1b die).
    pub task_type: u8,
    /// +0x26: the gfx job in `Gfx\8.gfx` (`ANIMAL_*_WALK` etc.).
    pub job: u16,
    /// +0x6c: the task's frame count (direction 2 of the job).
    pub frames: u8,
    /// +0x68: the last step: its direction, or'ed with 0x88 when blocked; 0x48 after `TakeJob`.
    pub moved: u8,
    /// +0x69: ticks per step (10).
    pub speed: u8,
    /// +0x24: animation frame. The original advances it while drawing (0x52bb90); the simulation
    /// sets it only when the death animation ends (last frame).
    pub frame: u8,
    /// The tick of the last `TakeJob` and of the last step (for drawing: frame and position
    /// interpolation, see [`AnimalData::frame_at`]).
    pub job_tick: u32,
    pub step_tick: u32,
    /// +0x70: ticks since death (deleted at 450).
    pub dead_ticks: i32,
    /// +0x7c: decisions since the last enemy search.
    pub search: i32,
    /// +0x80: steps since the last cell check.
    pub roam: i32,
    /// +0x84: flee steps in the current flee direction (−1: pick a new one).
    pub flee_steps: i32,
    /// +0x88: decisions spent on the wrong land (dark / not dark).
    pub wrong_land: i32,
    /// +0x8c: decisions without moving.
    pub stuck: i32,
    /// +0x90: the nearest enemy settler (packed position, −1 none).
    pub enemy: i32,
    /// +0x94: the last position in a suitable cell.
    pub home: i32,
    /// +0x98: the flee direction (−1 none).
    pub flee_dir: i32,
    /// +0x9c: the position at the last decision.
    pub last_pos: i32,
    /// +0x4c: `CWalkingNormal` (its `IdleWalk` steps an animal out of the way).
    pub walk: Box<WalkState>,
}

impl AnimalData {
    /// `CAnimal::CAnimal` (0x52b870) and `CLandAnimal::CLandAnimal` (0x5312f0).
    fn new() -> AnimalData {
        AnimalData {
            state: 0,
            task: 0,
            dir: 0,
            task_type: 0,
            job: 0,
            frames: 0,
            moved: 0,
            speed: 10,
            frame: 0,
            job_tick: 0,
            step_tick: 0,
            dead_ticks: 0,
            search: 0,
            roam: 0,
            flee_steps: 0,
            wrong_land: 0,
            stuck: 0,
            enemy: 0,
            home: 0,
            flee_dir: -1,
            last_pos: 0,
            walk: Box::default(),
        }
    }

    /// The frame the original would draw at `tick` (0x52bb90, run by `FillGfxInfo`): standing
    /// shows frame 0, a blocked step too; walking, idle and dying animations advance one frame
    /// per tick from `TakeJob`, wrapping at the frame count (the dead keep their last frame).
    pub fn frame_at(&self, tick: u32) -> u8 {
        if self.state == 5 {
            return self.frame;
        }
        let n = self.frames.max(1) as u32;
        let t = tick.wrapping_sub(self.job_tick);
        match self.task_type {
            6 if self.moved & 8 == 0 => (t % n) as u8,
            0x10 | 0x1b => (t % n) as u8,
            _ => 0,
        }
    }

    pub(crate) fn hash(&self, h: &mut StateHasher) {
        for v in [self.state, self.task, self.dir, self.task_type, self.frames, self.moved, self.speed, self.frame] {
            h.u8(v);
        }
        h.u16(self.job);
        h.u32(self.job_tick);
        h.u32(self.step_tick);
        for v in [
            self.dead_ticks,
            self.search,
            self.roam,
            self.flee_steps,
            self.wrong_land,
            self.stuck,
            self.enemy,
            self.home,
            self.flee_dir,
            self.last_pos,
        ] {
            h.u32(v as u32);
        }
        self.walk.hash(h);
    }
}

/// The type's task list (`CAnimalMgr` 0x52f8c0, gfx jobs = the `ANIMAL_*_*` job defines):
/// 0 stand (0x11, walk job, 1 frame), 1 idle animation (0x10), 2 walk (6), 3 flee (6, the walk
/// job again), 4 die (0x1b; only types with a death animation). Returns (task type, job, frames).
pub fn task(rules: &AnimalRules, kind: u16, index: u8) -> (u8, u16, u8) {
    let (walk, idle, die) = match kind {
        1 => (2, 1, 3),
        2 => (5, 4, 0),
        3 => (7, 6, 0),
        4 => (9, 8, 11),
        5 => (13, 12, 15),
        6 => (17, 16, 0),
        7 => (19, 18, 0),
        8 => (21, 20, 0),
        9 => (23, 22, 24),
        13 => (29, 30, 0),
        14 => (34, 35, 0),
        16 => (38, 37, 39),
        17 => (40, 41, 0),
        // The original leaves the jobs uninitialised: no land animal of these types exists.
        _ => (0, 0, 0),
    };
    match index {
        0 => (0x11, walk, 1),
        1 => (0x10, idle, rules.frames(idle)),
        2 | 3 => (6, walk, rules.frames(walk)),
        _ => (0x1b, die, rules.frames(die)),
    }
}

/// `CAnimalMgr::Init` (0x52ddf0), at the end of the game setup (`CGameRunInitEntry` 0x4fdc36):
/// the type table and land lists for the players' races, and the caps. The cap is MAX_ANIMALS or
/// `LAND_POP · land% · cells / 10000`, whichever is smaller (land% = the land nodes in percent of
/// the map, at least 1, from `CTiling` vtbl+0x34 = size² − the water nodes counted by the tiling
/// init 0x5b4a00); the non-huntable cap is `(100 − HUNT_PERCENT)` percent of it. Animals already
/// in the world (a loaded game) are counted.
pub fn init(world: &mut World) {
    let size = world.grid.size();
    let rules = world.rules.animals.clone();
    let mut m = AnimalMgr {
        cells_w: size >> 4,
        cells_h: size >> 4,
        cells: std::mem::take(&mut world.animals.cells),
        ..AnimalMgr::default()
    };
    // LoadAnimalData (0x52f1d0): a type with a race is loaded only when a player has that race.
    for (t, c) in rules.types.iter().enumerate() {
        let Some(c) = c else { continue };
        if c.race >= 0 && !world.players.iter().flatten().any(|p| p.race as i32 == c.race) {
            continue;
        }
        let (min_trees, max_trees) = match c.land {
            land_type::PLAIN => (rules.plain_trees, rules.light_trees - 1),
            land_type::LIGHTFOREST => (rules.light_trees - 1, rules.deep_trees - 1),
            land_type::DEEPFOREST => (rules.deep_trees, 0xff),
            _ => (0, 0),
        };
        m.types[t] = AnimalType {
            land: c.land,
            huntable: c.huntable,
            runaway: c.runaway,
            min_trees,
            max_trees,
            race: c.race,
            search_rad: rules.search_rad,
            mystic: 0,
        };
        if let Some(l) = usize::try_from(c.land).ok().and_then(|l| m.lists.get_mut(l)) {
            l.push(t as u8);
        }
    }
    m.max_mystic = rules.max_mystic;
    m.land_pop = rules.land_pop;
    m.plain_trees = rules.plain_trees;
    m.light_trees = rules.light_trees;
    m.deep_trees = rules.deep_trees;
    let land = world.layers.ground.iter().filter(|&&g| crate::tiling::ground_tile_type(g) & crate::tiling::TILE_LAND != 0).count() as i64;
    let percent = ((land * 100) / (size as i64 * size as i64)).max(1);
    let cells = (m.cells_w * m.cells_h) as i64;
    m.max = rules.max_animals.min(((rules.land_pop as i64 * percent * cells) / 10000) as i32);
    m.max_non_huntable = m.max * (100 - rules.hunt_percent) / 100;
    for (_, e) in world.entities.iter() {
        if e.obj_type == ObjType::Animal && e.flags & flag::DIED == 0 {
            m.total += 1;
            if !m.huntable(e.kind) {
                m.non_huntable += 1;
            }
        }
    }
    world.animals = m;
}

/// `CAnimalMgr::Update` (0x52edf0), called by `CGameRun::Run` after the map script: one spawn
/// attempt in a random cell while there are fewer animals than the cap. (The ambient effects that
/// follow are client side; see the module doc.)
pub fn update(world: &mut World) {
    let m = &mut world.animals;
    m.ticks += 1;
    if m.total < m.max && m.cells_w > 0 && m.cells_h > 0 {
        let cx = world.rng.rand() % world.animals.cells_w;
        let cy = world.rng.rand() % world.animals.cells_h;
        spawn(world, cx as i32, cy as i32);
    }
    let m = &mut world.animals;
    if m.ticks > 10 {
        m.ticks = 0;
    }
}

/// 0x52fce0: a spawn attempt in cell `(cx, cy)`. Only into a cell with at most LAND_POP / 100
/// animals. The kind of land: town (the cell has a building), dark (more than 100 dark nodes),
/// else grass (more than 100 grass nodes) by its tree count: deep forest, light forest, plain or
/// bare grass. A random type of that land (a rabbit is a RABBIT2 half of the time); a non-huntable
/// one only while under the non-huntable cap and, with a race, under MAX_MYSTIC of its type.
fn spawn(world: &mut World, cx: i32, cy: i32) {
    let flat = cy * cells::SIDE + cx;
    let animals = warmap_count(world, flat, 3);
    let m = &world.animals;
    if animals as i32 * 100 > m.land_pop {
        return;
    }
    let c = m.cells.at(flat);
    let kind = if warmap_head(world, flat, 2) != 0 {
        land_type::TOWN
    } else if c.dark() > 100 {
        land_type::DARK
    } else if c.grass() > 100 {
        let trees = c.trees() as i32;
        if trees >= m.deep_trees {
            land_type::DEEPFOREST
        } else if trees >= m.light_trees {
            land_type::LIGHTFOREST
        } else {
            1 + (trees >= m.plain_trees) as i32
        }
    } else {
        return;
    };
    let n = m.lists[kind as usize].len() as u32;
    if n == 0 {
        return;
    }
    let mut t = world.animals.lists[kind as usize][(world.rng.rand() % n) as usize] as u16;
    if t == RABBIT && world.rng.rand() % 10 < 5 {
        t = RABBIT2;
    }
    let m = &world.animals;
    let ty = m.types[t as usize];
    if !ty.huntable && (m.non_huntable >= m.max_non_huntable || (ty.race >= 0 && ty.mystic >= m.max_mystic)) {
        return;
    }
    match kind {
        land_type::PLAIN | land_type::LIGHTFOREST | land_type::DEEPFOREST => spawn_forest(world, t, cx, cy),
        land_type::DARK => spawn_dark(world, t, cx, cy),
        land_type::TOWN => spawn_town(world, t, cx, cy),
        _ => {}
    }
}

/// The nodes a spawn needs free: no blocked land, no deco object, no map object. Raw indices; one
/// off the map counts as taken.
fn free(world: &World, i: i64) -> bool {
    let l = &world.layers;
    usize::try_from(i).ok().filter(|&i| i < l.flags.len()).is_some_and(|i| l.flags[i] & 1 == 0 && l.deco_object[i] == 0 && l.map_object[i] == 0)
}

/// 0x52e830 (light / deep forest, plain): from a random node of the cell, walk the cell rows
/// (from row 2, wrapping) and columns (from 1, wrapping, not reset per row) for a tree; the first
/// tree whose node two rows up and one column left is free gets the animal there.
fn spawn_forest(world: &mut World, t: u16, cx: i32, cy: i32) {
    let size = world.grid.size() as i64;
    let base = (cy as i64 * size + cx as i64) * 16;
    let mut col = (world.rng.rand() & 0xf) as i64;
    let mut row = (world.rng.rand() & 0xf) as i64;
    for _ in 2..16 {
        if row >= 16 {
            row -= 16;
        } else if row < 2 {
            row = 2;
        }
        for _ in 1..16 {
            if col >= 16 {
                col -= 16;
            } else if col < 1 {
                col = 1;
            }
            let r = usize::try_from(base + row * size + col).ok().and_then(|i| world.layers.resource.get(i)).copied().unwrap_or(0);
            if r & 0xf0 == 0x70 && r & 0xf != 0 && free(world, base + (row - 2) * size + col - 1) {
                let (x, y) = (cx as i64 * 16 - 1 + col, cy as i64 * 16 + row - 2);
                if free(world, y * size + x) {
                    spawned(world, t, x as i32, y as i32);
                }
                return;
            }
            col += 1;
        }
        row += 1;
    }
}

/// 0x52eb90 (dark land): the first free dark node of the cell's inner 14×14.
fn spawn_dark(world: &mut World, t: u16, cx: i32, cy: i32) {
    let size = world.grid.size() as i64;
    for row in 1..15 {
        for col in 1..15 {
            let (x, y) = (cx as i64 * 16 + col, cy as i64 * 16 + row);
            let i = y * size + x;
            if free(world, i) && world.layers.flags[i as usize] & 0x04 != 0 {
                spawned(world, t, x as i32, y as i32);
                return;
            }
        }
    }
}

/// 0x52ecd0 (town): with buildings in more than 2 of the 3×3 cells around, halfway between the
/// cell's first building and the next one in its list (or the first of the six neighbour cells
/// that has one), on a free grass node that is not dark.
fn spawn_town(world: &mut World, t: u16, cx: i32, cy: i32) {
    let flat = cy * cells::SIDE + cx;
    let around = (-1..=1).flat_map(|dx| (-1..=1).map(move |dy| (dx, dy))).filter(|&(dx, dy)| warmap_head(world, flat + dy * cells::SIDE + dx, 2) != 0).count();
    if around <= 2 {
        return;
    }
    let b1 = warmap_head(world, flat, 2);
    // b1's successor in its cell list (`IEntity +0x1e`).
    let mut b2 = world.warmap.cell((flat % cells::SIDE) as usize, (flat / cells::SIDE) as usize, 2).nth(1).unwrap_or(0);
    if b2 == 0 {
        b2 = land::NEIGHBOR.iter().map(|&(dx, dy)| warmap_head(world, flat + dy * cells::SIDE + dx, 2)).find(|&b| b != 0).unwrap_or(0);
        if b2 == 0 {
            return;
        }
    }
    let (Some(p1), Some(p2)) = (world.entities.by_index(b1 as u32).map(|e| e.pos), world.entities.by_index(b2 as u32).map(|e| e.pos)) else {
        return;
    };
    // C division (towards zero).
    let x = p1.x() + (p2.x() - p1.x()) / 2;
    let y = p1.y() + (p2.y() - p1.y()) / 2;
    let size = world.grid.size() as i64;
    let i = y as i64 * size + x as i64;
    if free(world, i) && world.layers.ground[i as usize] & 0xf0 == 0x10 && world.layers.flags[i as usize] & 0x04 == 0 {
        // 0x52e6e0 checks the node again.
        spawned(world, t, x, y);
    }
}

/// The war-map list `cat` of the cell at flat index `flat` (0x142f290 + 0x14 · flat): its head,
/// 0 off the table.
fn warmap_head(world: &World, flat: i32, cat: usize) -> u16 {
    if !(0..cells::SIDE * cells::SIDE).contains(&flat) {
        return 0;
    }
    world.warmap.cell((flat % cells::SIDE) as usize, (flat / cells::SIDE) as usize, cat).next().unwrap_or(0)
}

fn warmap_count(world: &World, flat: i32, cat: usize) -> usize {
    if !(0..cells::SIDE * cells::SIDE).contains(&flat) {
        return 0;
    }
    world.warmap.cell((flat % cells::SIDE) as usize, (flat / cells::SIDE) as usize, cat).count()
}

/// `CAnimalMgr` 0x52e6e0 `(x, y, type)`: on a node without blocked land, deco or map object an
/// animal (0x52fe70) with the manager's counting (as [`spawned`]); the Vikings' animal spell
/// (`magic::spells::animals`) calls it. Returns the animal.
pub fn add_at(world: &mut World, x: i32, y: i32, kind: u16) -> Option<u16> {
    let n = world.grid.checked_index(x, y)?;
    if world.layers.flags[n] & 1 != 0 || world.layers.deco_object[n] != 0 || world.layers.map_object[n] != 0 {
        return None;
    }
    let id = create(world, x, y, kind)?;
    count_new(world, kind);
    Some(id)
}

/// A spawn site's counting (0x52e830 / 0x52eb90 / 0x52e6e0 after 0x52fe70).
fn spawned(world: &mut World, t: u16, x: i32, y: i32) {
    if create(world, x, y, t).is_none() {
        return;
    }
    count_new(world, t);
}

fn count_new(world: &mut World, t: u16) {
    let m = &mut world.animals;
    m.total += 1;
    let ty = &mut m.types[t as usize];
    if !ty.huntable {
        m.non_huntable += 1;
        if ty.race >= 0 {
            ty.mystic += 1;
        }
    }
}

/// 0x52fe70: `GetFreeSlot`, the `CLandAnimal` at `(x, y)`, its task list (0x52f8c0) and `Init`
/// (0x531c70): flags READY | VISIBLE (and 0x20000 for a huntable type), a `CWalkingNormal` with
/// no target, the map-object layer, the war map, home = here, a random direction (game RNG),
/// state 0 and the first task (`TakeJob`, which registers it for the next tick).
pub fn create(world: &mut World, x: i32, y: i32, kind: u16) -> Option<u16> {
    let slot = world.entities.free_slot()?;
    let pos = Y16X16::pack(x, y);
    let mut d = AnimalData::new();
    let mut walk = WalkState::new(crate::pathing::walking::walking_type::NORMAL, 0);
    walk.init(world, -1, 0);
    d.walk = Box::new(walk);
    d.home = pos.0 as i32;
    let mut e = Entity::new(ObjType::Animal, kind, 0, 0, pos, EntityData::Animal(d));
    e.flags |= flag::READY | flag::VISIBLE | if world.animals.huntable(kind) { 0x20000 } else { 0 };
    world.entities.insert(slot, e);
    if let Some(n) = node(world, pos.0 as i32) {
        world.layers.map_object[n] = slot;
    }
    crate::warmap::add(world, slot);
    let dir = (world.rng.rand() % 6) as u8;
    if let Some(d) = data_mut(world, slot) {
        d.dir = dir;
        d.state = 0;
        d.task = 0;
    }
    land::take_job(world, slot, true);
    Some(slot)
}

/// `CScanner::FindNearestAnimal` (0x5b08b0, the hunter's hut): the position of the nearest animal
/// of a huntable type standing on eco-sector `eco`, among the war-map animal lists of the cells
/// within `radius` of `(x, y)` (0x5b0310). Nearest by `Grid::Distance`, the first found on a tie;
/// the distance itself is not limited. Dead and dying animals count too (the hut checks).
pub fn find_nearest_huntable(world: &World, x: i32, y: i32, radius: i32, eco: u16) -> Option<Y16X16> {
    let mut best = (0x4000, None);
    for (cx, cy) in crate::warmap::cells_within(world, x, y, radius) {
        for a in world.warmap.cell(cx as usize, cy as usize, 3) {
            let Some(e) = world.entities.by_index(a as u32) else { continue };
            let on_eco = world.grid.checked_index(e.x(), e.y()).is_some_and(|n| world.tiling.eco_sector_of(n) == eco);
            if !on_eco || (e.kind as usize) >= TYPES || !world.animals.huntable(e.kind) {
                continue;
            }
            let d = crate::spiral::grid_distance(e.x() - x, e.y() - y);
            if d < best.0 {
                best = (d, Some(e.pos));
            }
        }
    }
    best.1
}

/// 0x52e7d0: an animal leaves the manager's counts and is killed.
pub fn delete(world: &mut World, id: u16) {
    let Some(kind) = world.entities.by_index(id as u32).map(|e| e.kind) else { return };
    let m = &mut world.animals;
    m.total -= 1;
    let ty = m.types.get_mut(kind as usize);
    if !ty.as_ref().is_some_and(|t| t.huntable) {
        m.non_huntable -= 1;
        if let Some(t) = ty.filter(|t| t.race >= 0) {
            t.mystic -= 1;
        }
    }
    world.kill(id, 0);
}

/// `CAnimal::Delete` (0x52c190, vtbl+0x10), run by `CMapObjectMgr::Kill` after the war-map
/// removal: off the map-object layer (flag 0x8000 marks it gone) and out of the logic schedule.
pub fn on_delete(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index_mut(id as u32) else { return };
    if e.flags & flag::ON_BOARD == 0 {
        e.flags |= flag::ON_BOARD;
        let pos = e.pos.0 as i32;
        if let Some(n) = node(world, pos)
            && world.layers.map_object[n] == id
        {
            world.layers.map_object[n] = 0;
        }
    }
    if world.entities.by_index(id as u32).is_some_and(|e| e.flags & flag::REGISTERED != 0) {
        world.unregister_from_logic_update(id);
    }
}

fn node(world: &World, packed: i32) -> Option<usize> {
    let p = Y16X16(packed as u32);
    world.grid.checked_index(p.x(), p.y())
}

pub(crate) fn data(world: &World, id: u16) -> Option<&AnimalData> {
    match &world.entities.by_index(id as u32)?.data {
        EntityData::Animal(d) => Some(d),
        _ => None,
    }
}

pub(crate) fn data_mut(world: &mut World, id: u16) -> Option<&mut AnimalData> {
    match &mut world.entities.by_index_mut(id as u32)?.data {
        EntityData::Animal(d) => Some(d),
        _ => None,
    }
}
