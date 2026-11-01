//! The ambient creatures of `CAnimalMgr::Update` (release 0x52edf0): butterflies, birds and
//! seagulls. In the original they are `CEffects` records (`CLogic +0x24`, type 8, created by
//! vtbl+0x28 = 0x4e3d60) moved by future events of kind 7 (0x4e5440 → 0x52cb30). They draw the
//! manager's own RNG (+0x168, seeded 0x29061971) and a second one (`+0x170 → [0]`, seeded
//! 0x12cc2a5), and nothing in the simulation reads them, so they live in the client
//! (`s4_sim::animals` module doc). Their sounds — `EFFECT_BIRD` `SOUND_AMB_BIRD`,
//! `EFFECT_SEAGULL1/2` `SOUND_ANIMAL_SEAGUL`, interval 10 — come from the mover 0x52cb30.
//!
//! **Land** (fixed when the game starts, `CAnimalMgr::Init` 0x52ddf0 / `LoadAnimalData`
//! 0x52f1d0, see [`Land`]): the butterfly nodes, the bird and seagull cells, the amounts.
//!
//! **Each tick** ([`Ambient::tick`]): the due kind-7 events first (`FutureEvents::Execute` runs
//! in `Logic::Update`, before `Animal::Update`), then the update counter and the spawns:
//! butterflies (0x5304d0) when no effect 0xb..=0xd exists, birds (0x5305e0) when no 0xe,
//! seagulls (0x530730) when neither 0xf nor 0x10.
//!
//! **A move event** (0x52cb30): the effect's position and type (vtbl+0x34 0x4e3e70), the type's
//! move (butterfly 0x52ce30, bird 0x52d050, seagull 0x52d280; each schedules the next event with
//! the move's duration), then, if the effect still lives and the type has a `SOUND`: the
//! previous sound (vtbl+0x48 0x4e4070) no longer plays (0x50d6a0), the handle is cleared, and if
//! the effect's 16-node square is within one square of the square under the screen centre
//! (`GetClosestMapPoint(w/2, h/2)`), `interval == 100 || rand() % 100 < interval` and
//! `IsPositionVisible` (`CFogging` vtbl+0x20): `PlayEnvironmentSound` (0x50c310) at the effect's
//! node, keeping the handle (vtbl+0x4c 0x4e4050).
//!
//! **Jumping fish** (0x5301c0, on the updates where the counter is a multiple of 3, before the
//! butterflies): two manager-RNG draws pick a cell around the screen centre's (`(x >> 4) + (r &
//! 3) − 2`); one with fish ([`Land::fish_cells`]) is walked row by row, one draw per node, until
//! a node not on dark land holds more than `FISH_MIN` fish (resource type 0); there a type-7
//! record starts (vtbl+0x20 → 0x4e2780): `Gfx\8.gfx` job `FISH_JOB`, direction = that node's draw
//! % 6, as many ticks as the job has frames in direction 2, drawn (0x4e2b20) with frame `age %
//! frames` at the node.
//!
//! **Drawing** ([`Ambient::sprites`]): a creature (0x4e2b70) shows `Gfx\6.gfx` job = its type,
//! direction +5, frame `(+7 + age) % +6` at the screen offset interpolated from the move's start
//! to its end over +3 ticks; `move_to` (0x4e28e0) carries the frame on (`+7 = (+7 + age) % +6`).
//!
//! **Water animals** (0x52ff10 every 11th update, [`Ambient::ground_fx`], and 0x52ffe0): in a
//! cell near the centre that is mostly not land ([`Land::wet_cells`]), now and then a dolphin or
//! shark ([`Land::water_animals`]) or, rarely, Nessy shows as the same type-7 record as the fish,
//! where a 5 × 5 patch is open water away from ships (the water flags, `Population::water`).
//! The ducks (inline
//! in 0x52edf0) never spawn: their cell list (+0x154) is only ever reset (Init 0x52df34), never
//! filled. A loaded game starts both RNGs and the counter afresh. [inf] A record is removed when
//! its age reaches its lifetime at the next update; the original's draw pass (0x4e33b0) removes
//! it when its row band is drawn.

use std::collections::{BTreeMap, HashMap};

use s4_audio::VoiceId;
use s4_sim::World;
use s4_viewer::population::Population;

/// Effect types (`CEffects` record +1).
pub const BUTTERFLY: [u8; 3] = [0xb, 0xc, 0xd];
pub const BIRD: u8 = 0xe;
pub const SEAGULL: [u8; 2] = [0xf, 0x10];
/// `AddEffect(0x40, 0x45, …)`: a creature on dark land vanishes.
pub const VANISH: (u16, u16) = (0x40, 0x45);

/// `EFFECT_*` sections in table order (0x1019f4c), types 0xb..=0x12.
const EFFECTS: [&str; 8] =
    ["EFFECT_BUTTERFLY1", "EFFECT_BUTTERFLY2", "EFFECT_BUTTERFLY3", "EFFECT_BIRD", "EFFECT_SEAGULL1", "EFFECT_SEAGULL2", "EFFECT_DUCKMA", "EFFECT_DUCKFE"];

/// What `CAnimalMgr::Init` and `LoadAnimalData` fix for a game.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Land {
    pub size: i32,
    /// +0x124: nodes `y << 16 | x` holding a flower deco object (0xb8..=0xbc) where 0x52efb0
    /// allows a butterfly, in the cells that hold such an object.
    pub butterfly_nodes: Vec<u32>,
    /// +0x13c: cells `cy << 16 | cx` of land type PLAIN (0x43bee0 case 2: `PLAIN_TREES <= trees <
    /// LIGHT_TREES`, the AI cell's tree count).
    pub bird_cells: Vec<u32>,
    /// +0x148: cells with some water (bits 0x02) that are COAST themselves or have a COAST
    /// neighbour (0x530470; COAST = 0x43bee0 case 0xd: some water and grass).
    pub seagull_cells: Vec<u32>,
    /// `AMOUNT_BUTTERFLY`, `AMOUNT_BIRDS`, `AMOUNT_SEAGULL`, each times `max(1, land nodes /
    /// 0x6400)` (0x52f2e4..).
    pub amounts: [i32; 3],
    /// Per type 0xb..=0x12: `SOUND` (−1 → 0) and `SOUND_INTERVAL` (default 100).
    pub sounds: [(u16, u8); 8],
    /// The AI cell grid's width and height in cells (0x149c5a4 / 0x149c5a8).
    pub cells: (i32, i32),
    /// Per cell `cy · 64 + cx`: its fish sum (resource type 0, 0x1267bc0 + cell · 0x2a) is not 0.
    /// The sums are made once when the game starts and never updated (s4-sim `animals::cells`).
    pub fish_cells: Vec<bool>,
    /// `FISH_MIN` (Animals.cfg, 0x149c5ec).
    pub fish_min: i32,
    /// Per cell `cy · 64 + cx` (0x1395c60, written once by the tiling's init 0x5b4a00): half the
    /// nodes of the 16-node square whose ground is no land tile (`CLandscapeProperties` bit 0,
    /// counted by 0x5b3cb0 into 0x1559d88).
    pub wet_cells: Vec<u8>,
    /// The `ANIMAL_*` types of land type `ANIMAL_LAND_WATER` (7) without a `RACE`, in type order
    /// (`LoadAnimalData` 0x52f1d0 → the vector 0x149c504): dolphin, shark.
    pub water_animals: Vec<u8>,
}

impl Land {
    /// From the world as the game starts (after the AI cell statistics are built).
    pub fn from_world(w: &World) -> Land {
        let cfg = s4_audio::cfg::Cfg::parse(&std::fs::read(s4_formats::path::resolve(s4_formats::game_dir().join("Config").join("Animals.cfg"))).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default());
        Land::new(w, &cfg)
    }

    pub fn new(w: &World, cfg: &s4_audio::cfg::Cfg) -> Land {
        let size = w.grid.size() as i32;
        let l = &w.layers;
        let d = "ANIMAL_DATA";
        // The static config variables 0x118297c / 0x11841bc (defaults 1 and 6) that 0x43bee0 reads.
        let (plain, light) = (cfg.int(d, "PLAIN_TREES", 1), cfg.int(d, "LIGHT_TREES", 6));
        let land = l.ground.iter().filter(|&&g| s4_sim::tiling::ground_tile_type(g) & s4_sim::tiling::TILE_LAND != 0).count() as i32;
        let factor = (land / 0x6400).max(1);
        let amounts = [cfg.int(d, "AMOUNT_BUTTERFLY", 0) * factor, cfg.int(d, "AMOUNT_BIRDS", 0) * factor, cfg.int(d, "AMOUNT_SEAGULL", 0) * factor];
        let sounds = EFFECTS.map(|sec| {
            let sound = cfg.get(sec, "SOUND").map_or(0, |v| {
                let v = v.trim();
                v.strip_prefix('<').and_then(|v| v.strip_suffix('>')).and_then(s4_audio::SoundId::by_name).map_or(0, |s| s.0)
            });
            (sound, cfg.int(sec, "SOUND_INTERVAL", 100).clamp(0, 255) as u8)
        });
        let cells = &w.animals.cells;
        let cell = |cx: i32, cy: i32| if (0..64).contains(&cx) && (0..64).contains(&cy) { Some(cells.get(cx, cy)) } else { None };
        let coast = |cx: i32, cy: i32| cell(cx, cy).is_some_and(|c| c.bits & 2 != 0 && c.grass() != 0);
        let flower = |x: i32, y: i32| {
            let id = l.deco_object[(y * size + x) as usize];
            id != 0 && w.entities.by_index(id as u32).is_some_and(|e| (0xb8..=0xbc).contains(&e.kind))
        };
        let ground = |x: i32, y: i32| (x >= 0 && y >= 0 && x < size && y < size).then(|| l.ground[(y * size + x) as usize] & 0xf0);
        let side = size >> 4;
        let m = &w.animals;
        let fish_cells = (0..64 * 64).map(|i| m.cells.get(i % 64, i / 64).resources[0] != 0).collect();
        let cells_wh = (m.cells_w as i32, m.cells_h as i32);
        let mut out = Land { size, amounts, sounds, cells: cells_wh, fish_cells, fish_min: w.rules.animals.fish_min, ..Land::default() };
        // 0x52ddf0: columns outside, rows inside.
        for cx in 0..side {
            for cy in 0..side {
                if (0..16).any(|i| (0..16).any(|j| flower(cx * 16 + i, cy * 16 + j))) {
                    for i in 0..16 {
                        for j in 0..16 {
                            let (x, y) = (cx * 16 + i, cy * 16 + j);
                            // 0x52efb0: grass with grass among the 12 ring-2 neighbours.
                            if flower(x, y) && ground(x, y) == Some(0x10) && (0..12).any(|k| ground(x + dx(k), y + dy(k)) == Some(0x10)) {
                                out.butterfly_nodes.push((y as u32) << 16 | x as u32);
                            }
                        }
                    }
                }
                if cell(cx, cy).is_some_and(|c| (plain..light).contains(&(c.trees() as i32))) {
                    out.bird_cells.push((cy as u32) << 16 | cx as u32);
                }
                let water = cell(cx, cy).is_some_and(|c| c.bits & 2 != 0);
                // 0x530470: the six neighbours, `0 <= n <= cells` (one past the map reads an
                // unused, empty cell).
                let near = [(1, 0), (1, 1), (0, 1), (-1, 0), (-1, -1), (0, -1)]
                    .iter()
                    .any(|&(ox, oy)| (0..=side).contains(&(cy + oy)) && (0..=side).contains(&(cx + ox)) && coast(cx + ox, cy + oy));
                if water && (near || coast(cx, cy)) {
                    out.seagull_cells.push((cy as u32) << 16 | cx as u32);
                }
            }
        }
        let mut wet = vec![0u32; 64 * 64];
        for n in 0..(size * size) as usize {
            let (x, y) = (n as i32 % size, n as i32 / size);
            if s4_sim::tiling::ground_tile_type(l.ground[n]) & s4_sim::tiling::TILE_LAND == 0 && x >> 4 < 64 && y >> 4 < 64 {
                wet[((y >> 4) * 64 + (x >> 4)) as usize] += 1;
            }
        }
        out.wet_cells = wet.iter().map(|&c| (c >> 1) as u8).collect();
        out.water_animals = (1..crate::sound::animals::TYPES.len())
            .filter(|&t| {
                let sec = crate::sound::animals::TYPES[t];
                cfg.get(sec, "LAND_TYPE").is_some_and(|v| v.trim() == "<ANIMAL_LAND_WATER>") && cfg.int(sec, "RACE", -1) < 0
            })
            .map(|t| t as u8)
            .collect();
        out
    }
}

/// `g_sSurroundingHexPoints` from entry 7 (0x1169984: the ring-2 points first).
fn dx(k: usize) -> i32 {
    s4_sim::spiral::dx(7 + k)
}
fn dy(k: usize) -> i32 {
    s4_sim::spiral::dy(7 + k)
}

/// The LCG both RNGs use: `state = state·0x6c078965 + 1`, the high half.
#[derive(Clone, Copy, Debug)]
struct Lcg(u32);

impl Lcg {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(0x6c07_8965).wrapping_add(1);
        self.0
    }
    fn hi(&mut self) -> u32 {
        self.next() >> 16
    }
}

/// An ambient effect (`CEffects` record, type 8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Effect {
    /// +1.
    pub kind: u8,
    /// +0x10/+0x14 where the current move started, +0x18/+0x1c where it ends (nodes).
    pub from: (i32, i32),
    pub to: (i32, i32),
    /// +0x20/+0x24 and +0x28/+0x2c: the same as world screen offsets (zoom 1).
    from_px: (i32, i32),
    to_px: (i32, i32),
    /// +8 the tick the move started, +3 its length in ticks.
    pub start: u32,
    pub ticks: u8,
    /// +5: the drawing direction 0..=15 (0x4e1f30).
    pub dir: u8,
    /// +6: the frame count (`Gfx\6.gfx` job `kind`, direction 0; at least 1) and +7 the frame
    /// the current move started with.
    pub frames: u8,
    pub frame0: u8,
    /// +0x401b0 + 4·id: the handle of its last sound.
    voice: Option<VoiceId>,
}

impl Effect {
    /// vtbl+0x34 (0x4e3e70): the node at `now`, interpolated along the move.
    pub fn at(&self, now: u32) -> (i32, i32) {
        let e = now.saturating_sub(self.start) as i32;
        let n = self.ticks as i32;
        if e >= n {
            self.to
        } else if e > 0 {
            (self.from.0 + (self.to.0 - self.from.0) * e / n, self.from.1 + (self.to.1 - self.from.1) * e / n)
        } else {
            self.from
        }
    }
}

/// A jumping fish or a water animal (`CEffects` record type 7, 0x4e2780).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fish {
    /// `Gfx\8.gfx` job.
    pub job: u16,
    pub x: i32,
    pub y: i32,
    /// +5: the direction 0..=5.
    pub dir: u8,
    /// +8: the tick it started; +0xc its lifetime (the job's frames in direction 2).
    pub start: u32,
    pub frames: u8,
}

/// The node state an ambient creature reads.
pub struct Nodes<'a> {
    pub size: i32,
    pub ground: &'a [u8],
    pub gradient: &'a [u8],
    pub flags: &'a [u8],
    pub height: &'a [u8],
    /// The resource layer (`y · size + x`; type in the high nibble, amount in the low).
    pub resource: &'a [u8],
    /// The water flags (0x14438ac, `y · size + x`).
    pub water: &'a [u16],
}

impl<'a> Nodes<'a> {
    pub fn of(pop: &'a Population) -> Nodes<'a> {
        Nodes { size: pop.size as i32, ground: &pop.ground, gradient: &pop.gradient, flags: &pop.flags, height: &pop.height, resource: &pop.resource, water: &pop.water }
    }
    fn index(&self, x: i32, y: i32) -> Option<usize> {
        (x >= 0 && y >= 0 && x < self.size && y < self.size).then(|| (y * self.size + x) as usize)
    }
    fn ground(&self, x: i32, y: i32) -> Option<u8> {
        self.index(x, y).and_then(|i| self.ground.get(i)).map(|g| g & 0xf0)
    }
    fn dark(&self, x: i32, y: i32) -> bool {
        self.index(x, y).and_then(|i| self.flags.get(i)).is_some_and(|f| f & 4 != 0)
    }
    /// 0x52efb0: grass, and grass among the 12 ring-2 neighbours.
    fn butterfly_land(&self, x: i32, y: i32) -> bool {
        self.ground(x, y) == Some(0x10) && (0..12).any(|k| self.ground(x + dx(k), y + dy(k)) == Some(0x10))
    }
    /// 0x52f050: not dark, ground 0x1X / 0x2X / 0x7X, and such a ground among the 12 ring-2
    /// neighbours inside the map.
    fn bird_land(&self, x: i32, y: i32) -> bool {
        let ok = |g: Option<u8>| matches!(g, Some(0x10 | 0x20 | 0x70));
        !self.dark(x, y) && ok(self.ground(x, y)) && (0..12).any(|k| ok(self.ground(x + dx(k), y + dy(k))))
    }
    /// 0x52f150: water (ground 0x0X) without gradient bit 0x20, and such a node among the 12
    /// ring-2 neighbours (the index unsigned below the node count).
    fn seagull_land(&self, x: i32, y: i32) -> bool {
        let n = self.size as i64;
        let wet = |i: i64| {
            (0..n * n).contains(&i) && self.ground.get(i as usize).is_some_and(|g| g & 0xf0 == 0) && self.gradient.get(i as usize).is_some_and(|g| g & 0x20 == 0)
        };
        let i = y as i64 * n + x as i64;
        wet(i) && (0..12).any(|k| wet(i + dy(k) as i64 * n + dx(k) as i64))
    }
    /// `ConvertMapPositionToWorldScreenOffsets` (0x65b130): `(x − y/2)·24`, `12y −
    /// round(h·0.0515625·24)` (height 0 outside the map), truncated.
    fn screen(&self, x: i32, y: i32) -> (i32, i32) {
        let h = self.index(x, y).and_then(|i| self.height.get(i)).copied().unwrap_or(0) as f32;
        let hoff = (h * 0.051_562_5_f32 * 24.0).round_ties_even();
        (((x as f32 - y as f32 * 0.5) * 24.0) as i32, (y as f32 * 12.0 - hoff) as i32)
    }
}

/// 0x4e1fe0: the integer square root.
fn isqrt(mut v: u32) -> u32 {
    let mut root = 0u32;
    let mut bit = if v < 0x10000 { 0x4000 } else { 0x4000_0000 };
    while bit != 0 {
        if bit + root <= v {
            v -= bit + root;
            root = (root >> 1) + bit;
        } else {
            root >>= 1;
        }
        bit >>= 2;
    }
    root
}

/// 0x4e1f30: the drawing direction 0..=15 of a move by `(dx, dy)` screen pixels.
fn direction(dx: f32, dy: f32) -> u8 {
    const K: f32 = 0.668_178_6;
    const T: f32 = 0.414_213_57;
    let a = dy * -K - dx;
    let b = dy - dx * K;
    let (c, p, q) = if a < 0.0 {
        if b > 0.0 { (12, b, -a) } else { (8, -a, -b) }
    } else if b < 0.0 {
        (4, -b, a)
    } else {
        (0, a, b)
    };
    if p <= q { c + u8::from(q * T <= p) } else { c + 2 + u8::from(q <= p * T) }
}

/// What a tick asks of the sound side.
pub trait Sink {
    /// 0x50d6a0: whether the sound with this handle still plays.
    fn playing(&self, v: VoiceId) -> bool;
    /// CRT `rand() % 100 < interval` (100 plays without drawing).
    fn roll(&mut self, interval: u8) -> bool;
    /// `CFogging::IsPositionVisible`.
    fn visible(&self, x: i32, y: i32) -> bool;
    /// `PlayEnvironmentSound(sound, x, y)`: the handle.
    fn play(&mut self, sound: u16, x: i32, y: i32) -> Option<VoiceId>;
    /// `CEffects::AddEffect(effect, sound, x, y)`'s sound (the effect itself goes to
    /// [`Ambient::added`]).
    fn effect(&mut self, effect: u16, sound: u16, x: i32, y: i32);
    /// `sub_25A2E0 == 3`: the node is drawn inside the output.
    fn on_screen(&self, x: i32, y: i32) -> bool;
}

/// The cap, count and excess of a group (`+0x170` struct: butterflies [2..=4], birds [5..=7],
/// seagulls [8..=10]; cap = AMOUNT + 1).
#[derive(Clone, Copy, Debug, Default)]
struct Group {
    cap: i32,
    count: i32,
    over: i32,
}

/// The client's `CAnimalMgr` ambient part.
#[derive(Clone, Debug)]
pub struct Ambient {
    pub land: std::sync::Arc<Land>,
    /// +0x168 / +0x16c.
    rng: Lcg,
    /// `[+0x170][0]`.
    fx_rng: Lcg,
    /// +0x160.
    counter: i32,
    groups: [Group; 3],
    pub effects: HashMap<u16, Effect>,
    next_id: u16,
    free: Vec<u16>,
    /// Kind-7 events by due tick: (effect id, direction), run last-added first.
    events: BTreeMap<u32, Vec<(u16, u8)>>,
    /// The jumping fish, by record id.
    pub fish: BTreeMap<u16, Fish>,
    /// The frame counts the records take ([`Frames`]).
    frames: Frames,
    /// `CEffects::AddEffect` calls not yet on the scene: (effect, x, y, tick).
    pub added: Vec<(u16, i32, i32, u32)>,
    /// The tick being run.
    now: u32,
    pub log: Log,
}

/// The gfx frame counts the records need (`GetJobFrameCount` 0x461fe0), at least 1 each.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frames {
    /// Per type 0xb..=0x12: `Gfx\6.gfx` job `type`, direction 0 (0x4e2800).
    pub types: [u8; 8],
    /// `Gfx\8.gfx` job [`FISH_JOB`], direction 2 (0x4e2780).
    pub fish: u8,
    /// The water animals' (types 10..=12: dolphin, Nessy, shark) first `Gfx\8.gfx` job
    /// ([`ANIMAL_JOB`]), direction 2.
    pub water: [u8; 3],
}

impl Default for Frames {
    fn default() -> Self {
        Frames { types: [1; 8], fish: 1, water: [1; 3] }
    }
}

/// The fish's gfx job (`s4_sim::animals::fish::FISH_JOB`).
pub const FISH_JOB: u16 = s4_sim::animals::fish::FISH_JOB;

/// The first `Gfx\8.gfx` job of each `ANIMAL_*` type: the release's table at GUIElementEngine
/// +0x77c (entry `type`, a pointer turned into a job index by `- 0x14 >> 2`). [inf] Read off
/// the job defines, which are in type order (`ANIMAL_DOLPHIN_SWIM` 25, `ANIMAL_NESSY_VIEW_LEFT`
/// 26, `ANIMAL_SHARK_SWIM` 28, `ANIMAL_FISH_SWIM` 36), as for [`FISH_JOB`].
pub const ANIMAL_JOB: [u16; 18] = [0, 1, 4, 6, 8, 12, 16, 18, 20, 22, 25, 26, 28, 29, 34, 36, 37, 40];

/// Counters for logs and tests.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Log {
    pub moves: u64,
    /// Sound checks reached (the effect lives, has a sound, its last one ended).
    pub checks: u64,
    /// Of those, effects outside the 3×3 squares around the centre square.
    pub far: u64,
    pub played: u64,
    pub vanished: u64,
    /// Jumping fish started.
    pub fish: u64,
    /// Water animals (dolphins, sharks, Nessy) started (0x52ffe0).
    pub water_animals: u64,
}

impl Ambient {
    pub fn new(land: std::sync::Arc<Land>, frames: Frames) -> Ambient {
        let groups = [0, 1, 2].map(|g| Group { cap: land.amounts[g] + 1, ..Group::default() });
        Ambient {
            land,
            rng: Lcg(0x2906_1971),
            fx_rng: Lcg(0x012c_c2a5),
            counter: 0,
            groups,
            effects: HashMap::new(),
            next_id: 1,
            free: Vec::new(),
            events: BTreeMap::new(),
            fish: BTreeMap::new(),
            frames,
            added: Vec::new(),
            now: 0,
            log: Log::default(),
        }
    }

    /// A free record id (`CEffects` +0x4226, 1..0xfff).
    fn take_id(&mut self) -> Option<u16> {
        self.free.pop().or_else(|| {
            (self.next_id < 0x1000).then(|| {
                self.next_id += 1;
                self.next_id - 1
            })
        })
    }

    /// What is drawn at tick `now` (the records' age is `now − +8`).
    pub fn sprites(&self, now: u32) -> Vec<s4_viewer::sprites::AmbientSprite> {
        use s4_viewer::sprites::AmbientSprite;
        let mut out = Vec::with_capacity(self.effects.len() + self.fish.len());
        for f in self.fish.values() {
            let Some(age) = now.checked_sub(f.start).filter(|&a| a < f.frames as u32) else { continue };
            out.push(AmbientSprite::Fish { x: f.x as u16, y: f.y as u16, job: f.job, dir: f.dir, frame: age % f.frames.max(1) as u32 });
        }
        let mut ids: Vec<_> = self.effects.keys().copied().collect();
        ids.sort_unstable();
        for id in ids {
            let e = &self.effects[&id];
            let Some(age) = now.checked_sub(e.start) else { continue };
            if e.from.0 < 0 || e.from.1 < 0 {
                continue;
            }
            // 0x4e2b70: `min(age, +3)` of the way from +0x20/+0x24 to +0x28/+0x2c.
            let n = e.ticks as i32;
            let u = (age.min(i32::MAX as u32) as i32).min(n);
            let px = if n == 0 || u < 1 { e.from_px } else { (e.from_px.0 + (e.to_px.0 - e.from_px.0) * u / n, e.from_px.1 + (e.to_px.1 - e.from_px.1) * u / n) };
            let frame = (e.frame0 as u32 + age) % e.frames.max(1) as u32;
            out.push(AmbientSprite::Effect { gfx: e.kind, dir: e.dir, frame, x: e.from.0 as u16, y: e.from.1 as u16, px });
        }
        out
    }

    fn count(&self, kind: u8) -> usize {
        self.effects.values().filter(|e| e.kind == kind).count()
    }

    /// vtbl+0x28 (0x4e3d60 → 0x4e2800).
    fn create(&mut self, nodes: &Nodes, kind: u8, x: i32, y: i32, now: u32) -> Option<u16> {
        let id = self.take_id()?;
        let px = nodes.screen(x, y);
        let frames = self.frames.types.get(kind.wrapping_sub(0xb) as usize).copied().unwrap_or(1).max(1);
        self.effects.insert(id, Effect { kind, from: (x, y), to: (x, y), from_px: px, to_px: px, start: now, ticks: 0, dir: 0, frames, frame0: 0, voice: None });
        Some(id)
    }

    /// vtbl+0x20 (0x4e3c10 → 0x4e2780): a jumping fish at `(x, y)` from `now`.
    fn create_fish(&mut self, x: i32, y: i32, dir: u8, now: u32) {
        if let Some(id) = self.take_id() {
            self.fish.insert(id, Fish { job: FISH_JOB, x, y, dir, start: now, frames: self.frames.fish.max(1) });
            self.log.fish += 1;
        }
    }

    /// vtbl+0x20 (0x4e3c10 → 0x4e2780) for a water animal of `kind` (10..=12).
    fn create_water_animal(&mut self, kind: u8, x: i32, y: i32, dir: u8, now: u32) {
        let job = ANIMAL_JOB.get(kind as usize).copied().unwrap_or(0);
        let frames = self.frames.water.get((kind as usize).wrapping_sub(10)).copied().unwrap_or(1).max(1);
        if let Some(id) = self.take_id() {
            self.fish.insert(id, Fish { job, x, y, dir, start: now, frames });
            self.log.water_animals += 1;
        }
    }

    /// vtbl+0x24 (0x4e3d00).
    fn delete(&mut self, id: u16) {
        if self.effects.remove(&id).is_some() || self.fish.remove(&id).is_some() {
            self.free.push(id);
        }
    }

    /// vtbl+0x3c (0x4e3f70 → 0x4e28e0): finish the current move where it got to, start one to
    /// `(x, y)`; its length is `isqrt(dx² + dy²)·100 >> 9` ticks (1..=255) in screen pixels.
    fn move_to(&mut self, nodes: &Nodes, id: u16, x: i32, y: i32, now: u32) -> u8 {
        let Some(e) = self.effects.get_mut(&id) else { return 0 };
        let el = now.saturating_sub(e.start) as i32;
        let n = e.ticks as i32;
        e.frame0 = ((e.frame0 as i32 + el) % e.frames.max(1) as i32) as u8;
        if el < n {
            if el > 0 {
                e.from = (e.from.0 + (e.to.0 - e.from.0) * el / n, e.from.1 + (e.to.1 - e.from.1) * el / n);
                e.from_px = (e.from_px.0 + (e.to_px.0 - e.from_px.0) * el / n, e.from_px.1 + (e.to_px.1 - e.from_px.1) * el / n);
            }
        } else {
            e.from = e.to;
            e.from_px = e.to_px;
        }
        e.to = (x, y);
        e.to_px = nodes.screen(x, y);
        let (ddx, ddy) = (e.to_px.0 - e.from_px.0, e.to_px.1 - e.from_px.1);
        let d = isqrt((ddy.wrapping_mul(ddy)).wrapping_add(ddx.wrapping_mul(ddx)) as u32) as i32;
        e.ticks = ((d * 100) >> 9).clamp(1, 255) as u8;
        e.dir = direction(ddx as f32, ddy as f32);
        e.start = now;
        e.ticks
    }

    /// `CFutureEvents::AddEvent(7, delay, 0, x << 16 | id, dir << 16 | y)`: due `delay` ticks
    /// later; a delay of 0 lands in the bucket already run this tick, so it waits 32.
    fn schedule(&mut self, id: u16, dir: u8, delay: u32, now: u32) {
        let due = now + if delay == 0 { 32 } else { delay };
        self.events.entry(due).or_default().push((id, dir));
    }

    /// One logic tick at `now` (the tick just run), the screen centre node `centre`.
    pub fn tick(&mut self, nodes: &Nodes, now: u32, centre: Option<(i32, i32)>, sink: &mut impl Sink) {
        self.now = now;
        // FutureEvents::Execute: the kind-7 events due now, the last added first.
        while let Some((&due, _)) = self.events.first_key_value() {
            if due > now {
                break;
            }
            let mut list = self.events.remove(&due).unwrap_or_default();
            while let Some((id, dir)) = list.pop() {
                self.mover(nodes, id, dir, now, centre, sink);
            }
        }
        // Records whose age reached their lifetime.
        let gone: Vec<u16> = self.fish.iter().filter(|(_, f)| now.saturating_sub(f.start) >= f.frames as u32).map(|(&id, _)| id).collect();
        for id in gone {
            self.delete(id);
        }
        // Animal::Update's ambient part.
        self.counter += 1;
        if self.counter > 10 {
            self.counter = 0;
            self.ground_fx(nodes, now, centre);
        }
        if self.counter % 3 == 0 {
            self.jumping_fish(nodes, now, centre);
        }
        if BUTTERFLY.iter().all(|&k| self.count(k) == 0) {
            self.spawn_butterflies(nodes, now);
        }
        if self.count(BIRD) == 0 {
            self.spawn(nodes, now, 1);
        }
        if SEAGULL.iter().all(|&k| self.count(k) == 0) {
            self.spawn(nodes, now, 2);
        }
    }

    /// 0x52ff10, every 11th update: two draws pick a cell around the centre's
    /// (`(x >> 4) + (r >> 16 & 7) − 4`, likewise y); on the map and with more than 100 in its
    /// [`Land::wet_cells`] byte (`2·b > 200`), 0x52ffe0 tries a water animal there. Without a
    /// centre node only the two draws (the original goes on with stale coordinates, as for the
    /// fish).
    fn ground_fx(&mut self, nodes: &Nodes, now: u32, centre: Option<(i32, i32)>) {
        let (r1, r2) = (self.rng.next(), self.rng.next());
        let Some((x, y)) = centre else { return };
        let cx = (x >> 4) + ((r1 >> 16) & 7) as i32 - 4;
        let cy = (y >> 4) + ((r2 >> 16) & 7) as i32 - 4;
        let land = &self.land;
        if cx < 0 || cx >= land.cells.0 || cy < 0 || cy >= land.cells.1 {
            return;
        }
        if 2 * land.wet_cells.get((cy * 64 + cx) as usize).copied().unwrap_or(0) as u32 > 200 {
            self.water_animal(nodes, now, cx, cy);
        }
    }

    /// 0x52ffe0 `(cx, cy)`: one draw below 0x28f (of 0x10000) → Nessy (11), direction 2; else
    /// none without water animal types, or a draw for the type ([`Land::water_animals`]) and
    /// one for the direction (% 6). Two draws pick a node `(cx·16 + u, cy·16 + v)`, `u`, `v` <
    /// 16; the 5 × 5 nodes from there (flat, row by row, unchecked against the map's edge) must
    /// all be open water (`& 0xc300 == 0xc000`) with no or a far ship (closeness nibble 0 or
    /// 0xb); then the animal starts two nodes in from that corner (vtbl+0x20) if that is on the
    /// map.
    fn water_animal(&mut self, nodes: &Nodes, now: u32, cx: i32, cy: i32) {
        let (kind, dir) = if (self.rng.next() >> 16) < 0x28f {
            (11u8, 2u8)
        } else {
            let n = self.land.water_animals.len() as u32;
            if n == 0 {
                return;
            }
            let (a, b) = (self.rng.hi(), self.rng.hi());
            (self.land.water_animals[(a % n) as usize], (b % 6) as u8)
        };
        let (u, v) = ((self.rng.hi() & 0xf) as i32, (self.rng.hi() & 0xf) as i32);
        let size = nodes.size as i64;
        let base = (size * cy as i64 + cx as i64) * 16 + size * v as i64 + u as i64;
        for row in 0..5i64 {
            for col in 0..5i64 {
                let Some(&wf) = usize::try_from(base + row * size + col).ok().and_then(|i| nodes.water.get(i)) else { return };
                if wf & 0xc300 != 0xc000 || !matches!(wf & 0xf, 0 | 0xb) {
                    return;
                }
            }
        }
        let (x, y) = (u + 2 + cx * 16, v + 2 + cy * 16);
        if x < nodes.size && y < nodes.size {
            self.create_water_animal(kind, x, y, dir, now);
        }
    }

    /// 0x5301c0. Without a centre node (`GetClosestMapPoint` failing) the original goes on with
    /// stale coordinates; here only the two draws are made.
    fn jumping_fish(&mut self, nodes: &Nodes, now: u32, centre: Option<(i32, i32)>) {
        let (r1, r2) = (self.rng.hi(), self.rng.hi());
        let Some((x, y)) = centre else { return };
        let cx = (x >> 4) + (r1 & 3) as i32 - 2;
        let cy = (y >> 4) + (r2 & 3) as i32 - 2;
        let land = &self.land;
        if cx < 0 || cy < 0 || cx >= land.cells.0 || cy >= land.cells.1 || !land.fish_cells.get((cy * 64 + cx) as usize).copied().unwrap_or(false) {
            return;
        }
        let fish_min = land.fish_min;
        for row in 0..16 {
            for col in 0..16 {
                let r = self.rng.hi();
                let (nx, ny) = (cx * 16 + col, cy * 16 + row);
                let Some(i) = nodes.index(nx, ny) else { continue };
                let res = nodes.resource.get(i).copied().unwrap_or(0);
                let fish = if res & 0xf0 == 0 { (res & 0xf) as i32 } else { 0 };
                if fish > fish_min && !nodes.dark(nx, ny) {
                    self.create_fish(nx, ny, (r % 6) as u8, now);
                    return;
                }
            }
        }
    }

    /// The Maya's spell 5 (0x4f4846..0x4f48b0, in the logic tick `now`, before this tick's
    /// [`Ambient::tick`]): with the manager there, a butterfly of kind `0xb + fx_rng % 3` at the
    /// vanished bowman's node (vtbl+0x28), counted in group 0 when made, and its kind-7 event
    /// `AddEvent(7, 1, 0, x << 16 | id, y)`: first moved one tick later.
    pub fn butterfly(&mut self, nodes: &Nodes, x: i32, y: i32, now: u32) {
        let kind = (self.fx_rng.hi() % 3) as u8 + 0xb;
        if let Some(id) = self.create(nodes, kind, x, y, now) {
            self.add(0);
            self.schedule(id, 0, 1, now);
        }
    }

    /// 0x5304d0.
    fn spawn_butterflies(&mut self, nodes: &Nodes, now: u32) {
        let list = self.land.butterfly_nodes.clone();
        let n = (self.land.amounts[0].max(0) as usize).min(list.len());
        for _ in 0..n {
            let node = list[(self.rng.hi() as usize) % list.len()];
            let kind = (self.fx_rng.hi() % 3) as u8 + 0xb;
            let (x, y) = ((node & 0xffff) as i32, (node >> 16) as i32);
            if let Some(id) = self.create(nodes, kind, x, y, now) {
                self.add(0);
                let delay = self.rng.hi() % 20;
                self.schedule(id, 0, delay, now);
            }
        }
    }

    /// 0x5305e0 (`g` 1, birds on 0x52f050 land) and 0x530730 (`g` 2, seagulls on 0x52f150):
    /// AMOUNT times a random cell of the list, its first suitable node (row by row), a new
    /// effect there and its first event in `rand % 20` ticks. A cell without a suitable node
    /// draws another one; the original loops for ever when none has one, here 1000 draws.
    fn spawn(&mut self, nodes: &Nodes, now: u32, g: usize) {
        let cells = if g == 1 { self.land.bird_cells.clone() } else { self.land.seagull_cells.clone() };
        if cells.is_empty() {
            return;
        }
        for _ in 0..self.land.amounts[g].max(0) {
            for _ in 0..1000 {
                let c = cells[(self.rng.hi() as usize) % cells.len()];
                let (cx, cy) = ((c & 0xffff) as i32, (c >> 16) as i32);
                let hit = (0..256).map(|i| (cx * 16 + (i & 15), cy * 16 + (i >> 4))).find(|&(x, y)| if g == 1 { nodes.bird_land(x, y) } else { nodes.seagull_land(x, y) });
                let Some((x, y)) = hit else { continue };
                if let Some(id) = self.create(nodes, if g == 1 { BIRD } else { SEAGULL[0] }, x, y, now) {
                    self.add(g);
                    let delay = self.rng.hi() % 20;
                    self.schedule(id, 0, delay, now);
                }
                break;
            }
        }
    }

    fn add(&mut self, g: usize) {
        let gr = &mut self.groups[g];
        gr.count += 1;
        if gr.cap <= gr.count {
            gr.over += 1;
        }
    }

    /// Leave the group: excess and count down (by one, not below 0 when `floor`).
    fn leave(&mut self, g: usize, floor: bool) {
        let gr = &mut self.groups[g];
        if !floor || gr.over > 0 {
            gr.over -= 1;
        }
        if !floor || gr.count > 0 {
            gr.count -= 1;
        }
    }

    /// 0x52cb30: one kind-7 event.
    fn mover(&mut self, nodes: &Nodes, id: u16, dir: u8, now: u32, centre: Option<(i32, i32)>, sink: &mut impl Sink) {
        let Some(e) = self.effects.get(&id).copied() else { return };
        let (x, y) = e.at(now);
        self.log.moves += 1;
        let alive = match e.kind {
            0xb..=0xd => self.move_butterfly(nodes, id, x, y, e.dir, now, sink),
            0xe => self.move_bird(nodes, id, x, y, dir, now, sink),
            0xf | 0x10 => self.move_seagull(nodes, id, x, y, dir, now, sink),
            _ => return,
        };
        let (sound, interval) = self.land.sounds[(e.kind - 0xb) as usize];
        if !alive || sound == 0 {
            return;
        }
        let Some(ef) = self.effects.get_mut(&id) else { return };
        if ef.voice.is_some_and(|v| sink.playing(v)) {
            return;
        }
        ef.voice = None;
        self.log.checks += 1;
        let Some((cx, cy)) = centre.filter(|&(cx, cy)| cx >= 0 && cy >= 0) else { return };
        if !((cx >> 4) - 1..=(cx >> 4) + 1).contains(&(x >> 4)) || !((cy >> 4) - 1..=(cy >> 4) + 1).contains(&(y >> 4)) {
            self.log.far += 1;
            return;
        }
        if !sink.roll(interval) || !sink.visible(x, y) {
            return;
        }
        self.log.played += 1;
        ef.voice = sink.play(sound, x, y);
    }

    /// The common start of the movers: a creature in excess and off screen goes; one on dark
    /// land vanishes with effect 0x40 / sound 0x45.
    fn leaves(&mut self, nodes: &Nodes, g: usize, id: u16, x: i32, y: i32, sink: &mut impl Sink) -> bool {
        if self.groups[g].over != 0 && !sink.on_screen(x, y) {
            self.delete(id);
            self.leave(g, false);
            return true;
        }
        if nodes.dark(x, y) {
            self.delete(id);
            self.added.push((VANISH.0, x, y, self.now));
            sink.effect(VANISH.0, VANISH.1, x, y);
            self.groups[g].count -= 1;
            self.log.vanished += 1;
            return true;
        }
        false
    }

    /// 0x52ce30.
    #[allow(clippy::too_many_arguments)]
    fn move_butterfly(&mut self, nodes: &Nodes, id: u16, x: i32, y: i32, draw_dir: u8, now: u32, sink: &mut impl Sink) -> bool {
        if self.leaves(nodes, 0, id, x, y, sink) {
            return false;
        }
        let mut k = (self.fx_rng.hi() % 12) as usize;
        let mut t = (x + dx(k), y + dy(k));
        let mut tries = 0;
        while !nodes.butterfly_land(t.0, t.1) {
            if tries > 11 {
                k = if (draw_dir as usize) + 6 < 13 { draw_dir as usize + 6 } else { draw_dir as usize - 6 };
                t = (x + dx(k), y + dy(k));
                break;
            }
            k = if k + 1 < 13 { k + 1 } else { 0 };
            t = (x + dx(k), y + dy(k));
            tries += 1;
        }
        if !nodes.butterfly_land(t.0, t.1) {
            // Spiral entries 19 ..< First(30): the first grass node, else the last one tried.
            let last = s4_sim::spiral::first(30);
            let e = (19..last).find(|&e| nodes.ground(x + s4_sim::spiral::dx(e), y + s4_sim::spiral::dy(e)) == Some(0x10)).unwrap_or(last - 1);
            t = (x + s4_sim::spiral::dx(e), y + s4_sim::spiral::dy(e));
        }
        self.step(nodes, id, t, k as u8, now)
    }

    /// 0x52d050.
    #[allow(clippy::too_many_arguments)]
    fn move_bird(&mut self, nodes: &Nodes, id: u16, x: i32, y: i32, dir: u8, now: u32, sink: &mut impl Sink) -> bool {
        if self.leaves(nodes, 1, id, x, y, sink) {
            return false;
        }
        let mut k = dir as usize;
        if self.fx_rng.next() < 0x4000_0000 {
            k = if self.fx_rng.next() < 0x8000_0000 { inc(k) } else { dec(k) };
        }
        let mut t = (x + dx(k), y + dy(k));
        let ok = nodes.bird_land(t.0, t.1);
        let r = self.fx_rng.hi();
        if !ok {
            let mut found = false;
            for _ in 0..12 {
                k = if r < 0x8000 { inc(k) } else { dec(k) };
                t = (x + dx(k), y + dy(k));
                if nodes.bird_land(t.0, t.1) {
                    found = true;
                    break;
                }
            }
            if !found {
                k = back(dir as usize);
                t = (x + dx(k), y + dy(k));
            }
        }
        self.step(nodes, id, t, k as u8, now)
    }

    /// 0x52d280.
    #[allow(clippy::too_many_arguments)]
    fn move_seagull(&mut self, nodes: &Nodes, id: u16, x: i32, y: i32, dir: u8, now: u32, sink: &mut impl Sink) -> bool {
        let size = nodes.size;
        if x as u32 >= size as u32 || y as u32 >= size as u32 {
            self.delete(id);
            self.leave(2, true);
            return false;
        }
        if self.groups[2].over != 0 && !sink.on_screen(x, y) {
            self.delete(id);
            self.leave(2, false);
            return false;
        }
        let turn = self.fx_rng.hi() < 0x4000;
        let mut k = dir as usize;
        if turn {
            k = if self.fx_rng.next() < 0x8000_0000 { inc(k) } else { dec(k) };
        }
        let cells = &self.land.seagull_cells;
        let ok = |t: (i32, i32)| cells.contains(&(((t.1 >> 4) as u32) << 16).wrapping_add((t.0 >> 4) as u32)) && nodes.seagull_land(t.0, t.1);
        let mut t = (x + dx(k), y + dy(k));
        let r = self.fx_rng.hi();
        if !ok(t) {
            let mut found = false;
            for _ in 0..12 {
                k = if r < 0x8000 { inc(k) } else { dec(k) };
                t = (x + dx(k), y + dy(k));
                if ok(t) {
                    found = true;
                    break;
                }
            }
            if !found {
                k = back(dir as usize);
                t = (x + dx(k), y + dy(k));
            }
        }
        if t.0 as u32 >= size as u32 || t.1 as u32 >= size as u32 {
            self.delete(id);
            self.leave(2, true);
            return false;
        }
        self.fx_rng.next();
        if turn && let Some(e) = self.effects.get_mut(&id) {
            // vtbl+0x2c (0x4e3dc0): SEAGULL1 ↔ SEAGULL2, with the new kind's frame count (+6,
            // 0x4e2140: its job's frames in direction 0, at least 1) and frame +7 = 0.
            e.kind = if e.kind == SEAGULL[0] { SEAGULL[1] } else { SEAGULL[0] };
            e.frames = self.frames.types.get(e.kind.wrapping_sub(0xb) as usize).copied().unwrap_or(1).max(1);
            e.frame0 = 0;
        }
        self.step(nodes, id, t, k as u8, now)
    }

    /// The move and the next event; a failed `AddEvent` deletes the creature (never here).
    fn step(&mut self, nodes: &Nodes, id: u16, t: (i32, i32), k: u8, now: u32) -> bool {
        let ticks = self.move_to(nodes, id, t.0, t.1, now);
        if ticks != 0 {
            self.schedule(id, k, ticks as u32, now);
        }
        true
    }
}

/// The direction turns: `k + 1` wraps after 12 (so 12 occurs), `k − 1` below 0 to 11.
fn inc(k: usize) -> usize {
    if k + 1 < 13 { k + 1 } else { 0 }
}
fn dec(k: usize) -> usize {
    if k == 0 { 11 } else { k - 1 }
}
/// The way back after 12 failed turns: `dir + 6` if below 13, else `dir − 6`.
fn back(dir: usize) -> usize {
    if dir + 6 < 13 { dir + 6 } else { dir - 6 }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Records what the creatures ask; `playing` holds the voices still sounding.
    #[derive(Default)]
    struct Stub {
        played: Vec<(u16, i32, i32)>,
        effects: Vec<(u16, u16, i32, i32)>,
        playing: Vec<VoiceId>,
        fogged: bool,
    }

    impl Sink for Stub {
        fn playing(&self, v: VoiceId) -> bool {
            self.playing.contains(&v)
        }
        fn roll(&mut self, _interval: u8) -> bool {
            true
        }
        fn visible(&self, _x: i32, _y: i32) -> bool {
            !self.fogged
        }
        fn play(&mut self, sound: u16, x: i32, y: i32) -> Option<VoiceId> {
            self.played.push((sound, x, y));
            Some(VoiceId(self.played.len() as u64))
        }
        fn effect(&mut self, effect: u16, sound: u16, x: i32, y: i32) {
            self.effects.push((effect, sound, x, y));
        }
        fn on_screen(&self, _x: i32, _y: i32) -> bool {
            true
        }
    }

    const BIRD_SOUND: u16 = 1;

    /// A 64×64 map of grass, 4 birds in cell (1, 1), nothing else.
    fn birds() -> (Ambient, Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>) {
        let mut sounds = [(0, 100); 8];
        sounds[3] = (BIRD_SOUND, 10);
        let land = Land { size: 64, bird_cells: vec![1 << 16 | 1], amounts: [0, 4, 0], sounds, ..Land::default() };
        let n = 64 * 64;
        (Ambient::new(std::sync::Arc::new(land), Frames::default()), vec![0x10; n], vec![0; n], vec![0; n], vec![0; n])
    }

    /// 0x5305e0 / 0x52d050 / 0x52cb30: AMOUNT birds at the first bird-land node of the cell, each
    /// first moved within 20 ticks (32 for a delay of 0); every move goes to a ring-2 (or, after
    /// direction 11, the first ring-3) neighbour and lasts `isqrt(d²)·100 >> 9` ticks; a sound
    /// needs the bird within one square of the centre square, and waits for the last one to end.
    /// The Maya's spell 5 butterfly (0x4f4846): one manager-RNG draw (`+0x170`) picks kind 0xb..=0xd,
    /// the effect stands at the bowman's node, joins group 0 and moves first one tick later.
    #[test]
    fn spell_butterfly() {
        let (mut a, ground, gradient, flags, height) = birds();
        let nodes = Nodes { size: 64, ground: &ground, gradient: &gradient, flags: &flags, height: &height, resource: &[], water: &[] };
        let mut expect = a.fx_rng;
        let kind = (expect.hi() % 3) as u8 + 0xb;
        a.butterfly(&nodes, 20, 21, 7);
        assert_eq!(a.fx_rng.0, expect.0);
        let (&id, e) = a.effects.iter().next().unwrap();
        assert_eq!((e.kind, e.from, e.to), (kind, (20, 21), (20, 21)));
        assert_eq!(a.groups[0].count, 1);
        assert_eq!(a.events.get(&8), Some(&vec![(id, 0)]));
    }

    #[test]
    fn birds_spawn_move_and_sound_near_the_centre() {
        let (mut a, ground, gradient, flags, height) = birds();
        let nodes = Nodes { size: 64, ground: &ground, gradient: &gradient, flags: &flags, height: &height, resource: &[], water: &[] };
        let mut s = Stub::default();
        a.tick(&nodes, 1, Some((24, 24)), &mut s);
        assert_eq!(a.effects.len(), 4);
        assert!(a.effects.values().all(|e| e.kind == BIRD && e.to == (16, 16)), "the cell's first node");
        assert!(a.events.keys().all(|&t| (2..=21).contains(&t) || t == 33));
        let mut last: HashMap<u16, (i32, i32)> = a.effects.iter().map(|(&id, e)| (id, e.to)).collect();
        for t in 2..400 {
            a.tick(&nodes, t, Some((24, 24)), &mut s);
            for (id, e) in &a.effects {
                if e.start == t {
                    let (dx, dy) = (e.to.0 - last[id].0, e.to.1 - last[id].1);
                    let ring2 = (0..12).any(|k| (super::dx(k), super::dy(k)) == (dx, dy));
                    assert!(ring2 || (dx, dy) == (3, 1), "step {dx},{dy}");
                    let (p, q) = (nodes.screen(last[id].0, last[id].1), nodes.screen(e.to.0, e.to.1));
                    let d = isqrt(((q.0 - p.0).pow(2) + (q.1 - p.1).pow(2)) as u32) as i32;
                    assert_eq!(e.ticks as i32, ((d * 100) >> 9).clamp(1, 255));
                    last.insert(*id, e.to);
                }
            }
        }
        assert_eq!(a.effects.len(), 4, "no bird leaves grass land");
        assert!(a.log.moves > 100);
        // Every played sound was within one square of the centre square (1, 1).
        assert!(!s.played.is_empty());
        assert!(s.played.iter().all(|&(id, x, y)| id == BIRD_SOUND && (0..=2).contains(&(x >> 4)) && (0..=2).contains(&(y >> 4))));
        // Still playing: no new sound for that bird.
        let mut b = birds().0;
        let mut s2 = Stub { playing: (1..=1000).map(VoiceId).collect(), ..Stub::default() };
        for t in 1..400 {
            b.tick(&nodes, t, Some((24, 24)), &mut s2);
        }
        assert_eq!(s2.played.len(), 4, "one each, then the handles never end");
        // Centre in square (3, 3): only birds that wandered into squares 2..=4 sound.
        let mut c = birds().0;
        let mut s3 = Stub::default();
        for t in 1..400 {
            c.tick(&nodes, t, Some((60, 60)), &mut s3);
        }
        assert!(c.log.far > 0 && s3.played.len() < s.played.len());
        assert!(s3.played.iter().all(|&(_, x, y)| (2..=4).contains(&(x >> 4)) && (2..=4).contains(&(y >> 4))));
    }

    /// Dark land under a bird: it vanishes with effect 0x40 / sound 0x45; when none is left the
    /// next update spawns AMOUNT again.
    #[test]
    fn birds_vanish_on_dark_land_and_respawn() {
        let (mut a, ground, gradient, mut flags, height) = birds();
        let mut s = Stub::default();
        let nodes = Nodes { size: 64, ground: &ground, gradient: &gradient, flags: &flags, height: &height, resource: &[], water: &[] };
        a.tick(&nodes, 1, None, &mut s);
        flags.iter_mut().for_each(|f| *f |= 4);
        let nodes = Nodes { size: 64, ground: &ground, gradient: &gradient, flags: &flags, height: &height, resource: &[], water: &[] };
        for t in 2..=40 {
            a.tick(&nodes, t, None, &mut s);
        }
        assert_eq!(s.effects.len(), 4);
        assert!(s.effects.iter().all(|&(e, snd, x, y)| (e, snd, x, y) == (0x40, 0x45, 16, 16)));
        assert_eq!(a.log.vanished, 4);
        // Respawned in the tick the last one went, but the land is dark: no bird land.
        assert!(a.effects.is_empty());
    }

    /// 0x52ffe0: with the cell's whole 5 × 5 patch open water and no ship near, a water animal
    /// starts two nodes in (its type and direction from the draws, Nessy below 0x28f); a ship's
    /// closeness in the patch or a missing type list stops it. Draws: 5 (type, direction,
    /// node) or 3 (Nessy, node).
    #[test]
    fn water_animals_in_open_water() {
        let land = Land { size: 64, cells: (4, 4), wet_cells: vec![128; 64 * 64], water_animals: vec![10, 12], ..Land::default() };
        let frames = Frames { water: [9, 7, 5], ..Frames::default() };
        let (ground, gradient, flags, height) = (vec![0u8; 64 * 64], vec![0u8; 64 * 64], vec![0u8; 64 * 64], vec![0u8; 64 * 64]);
        let water = vec![0xc000u16; 64 * 64];
        let nodes = Nodes { size: 64, ground: &ground, gradient: &gradient, flags: &flags, height: &height, resource: &[], water: &water };
        let mut nessy = 0;
        for seed in 0..400u32 {
            let mut a = Ambient::new(std::sync::Arc::new(land.clone()), frames);
            a.rng = Lcg(seed.wrapping_mul(0x9e37_79b9));
            let mut r = a.rng;
            let is_nessy = (r.next() >> 16) < 0x28f;
            let (kind, dir) = if is_nessy { (11, 2) } else { ([10u8, 12][(r.hi() % 2) as usize], (r.hi() % 6) as u8) };
            let (u, v) = ((r.hi() & 0xf) as i32, (r.hi() & 0xf) as i32);
            a.water_animal(&nodes, 7, 1, 2);
            assert_eq!(a.rng.0, r.0, "seed {seed}: the draws");
            let f = *a.fish.values().next().expect("an animal");
            assert_eq!((f.job, f.x, f.y, f.dir, f.start), (ANIMAL_JOB[kind as usize], 16 + u + 2, 32 + v + 2, dir, 7));
            assert_eq!(f.frames, frames.water[kind as usize - 10]);
            nessy += is_nessy as u32;
        }
        assert!(nessy < 20, "Nessy is rare: {nessy}");
        // A ship's closeness (low nibble 9) anywhere in the patch: nothing.
        let mut busy = water.clone();
        for y in 32..52 {
            for x in 16..36 {
                busy[y * 64 + x] |= 9;
            }
        }
        let nodes = Nodes { water: &busy, ..nodes };
        let mut a = Ambient::new(std::sync::Arc::new(land.clone()), frames);
        a.water_animal(&nodes, 7, 1, 2);
        assert!(a.fish.is_empty());
        // No water animal types: one draw (unless Nessy) and nothing.
        let mut a = Ambient::new(std::sync::Arc::new(Land { water_animals: vec![], ..land }), frames);
        a.rng = Lcg(1);
        a.water_animal(&nodes, 7, 1, 2);
        assert!(a.fish.is_empty());
    }

    #[test]
    fn isqrt_and_direction() {
        assert_eq!((isqrt(0), isqrt(48 * 48), isqrt(37 * 37 + 5), isqrt(0x1_0000)), (0, 48, 37, 256));
        // Screen right, down, left, up.
        assert_eq!([direction(10.0, 0.0), direction(0.0, 10.0), direction(-10.0, 0.0), direction(0.0, -10.0)].len(), 4);
        let all: std::collections::BTreeSet<u8> = (0..64).map(|i| {
            let a = i as f32 / 64.0 * std::f32::consts::TAU;
            direction(a.cos() * 100.0, a.sin() * 100.0)
        }).collect();
        assert_eq!(all.len(), 16, "{all:?}");
    }
}
