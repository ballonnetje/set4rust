//! Sprite layer: map objects, buildings, settlers, vehicles and piles drawn over the terrain.
//!
//! # Design
//! Sprites stay **palette-indexed** on the GPU: an `R8Uint` atlas array holds the decoded RLE
//! pixels as the original stores them (0 = transparent, 1 = shadow, 2..=255 = palette index), and a
//! palette texture holds one 512-entry row per (gfx file, palette). The fragment shader resolves:
//! * index 0 → discard;
//! * index 1 → shadow: opaque black on every other screen pixel, `(x + y)` even, the phase
//!   `ZoomBobNormal` (0x2f72630) uses (it seeds from the destination pixel's x parity and the row
//!   parity and draws on the "even" pixels), so at zoom 1 the dither lands where the original's does;
//! * team-colour files (settlers 20–24, vehicles 30–34, 512-entry palettes), as in `BlitSettler`
//!   (0x2f69ee0): index 192..=223 → the player's 32-step gradient (`CColorGradient::SetupGradients`),
//!   224..=255 → palette entry `256 + 32*player + (i − 224)`;
//! * anything else → the palette row (alpha from the 4444 palette, so effects can fade).
//!
//! Atlas budget (summed bounding boxes): one race's settler frames are ~107–126 MB as R8, against
//! 430–504 MB as RGBA *per player colour*. That is why team colour is not baked.
//!
//! # Placement
//! A sprite anchored at node (x, y) with height h is drawn with its top-left at
//! `(24x − 12y − left, ((12y << 16) − hoff16(h)) >> 16 − top)` in zoom-1 world px, i.e. at the
//! node's terrain vertex (`crate::terrain::node_pos`). `RenderObjectLayer` passes the 16.16
//! difference `v97 − CalcFinalHeightOffset(h)` to `BlitObjectHardware` / `BlitSettlerHardware` /
//! `BlitBuildingHardware` (`BlitObjectHardware` = 0x2f951e0), which shift it right by 16 only then, so the *difference*
//! is floored (`ConvertMapIndexToScreenPosition` floors the offset first, but it is not on the draw
//! path; it serves the cursor and `GetScreenOffsetsByMapIndices`). The node → screen mapping of the
//! object pass equals the landscape pass: both start row `scrollY` at `−H` and column
//! `scrollX + y/2 (+1 on odd rows)` at `−W` (`−W − W/2` on even rows).
//! `left/top` come from the image header. Images with header flag 0x10 are drawn at half size
//! (`BlitObjectHardware`: zoom and offsets halved).
//!
//! # Pass order
//! `IGfxEngine::RenderFrame` (0x2f5b8f0) draws the whole landscape first (`DrawTexturedLandscape`
//! into the landscape surface, then `DrawTexturedLandscapeDelta`), blits that surface to the final
//! surface, and only then calls `RenderObjectLayer(0)`. Objects never interleave with terrain
//! triangles: a hill in front does not hide a tree behind it.
//!
//! # Draw order
//! `RenderObjectLayer` (debug build `globals.cpp`, the object pass of the hardware and software
//! engines) walks the visible map **row by row (y ascending), and within a row by x ascending**.
//! Per node it draws the border stone, then the *object layer* entry (buildings via
//! `BlitBuilding`, deco objects and piles via `BlitObject`), then the *entity layer* entry
//! (settlers via `BlitSettler`). Wheeled vehicles (object kind 4: `CCart`, `CWheeler`) are queued
//! and drawn at the end of the same node. Ships (kind 2, `CShip`) go to a list that is drawn after
//! all rows. Effects follow each row. Instances here are sorted by `(ship, y, x, layer)` with
//! layers object → settler → vehicle.
//!
//! # What is drawn (static, first frame)
//! * Deco objects (chunk 6 byte 0): `CWorldManager::LoadGfxData` skips ids 162/209/210/211 and calls
//!   `AddDecoObjWithoutFlags(x, y, id, 0, GROWN)`. `CDecoObjMgr::Create` then picks the gfx job:
//!   trees 1..=22, plants 162/209–211/255 and mushrooms 212–214 use `5.sil[id] + 3` (the grown
//!   job; in a running game their stage, see `population::Deco::stage`). Stones 124..=136 become
//!   `OBJECT_MINESET1` (41) with amount `id − 123` (dil = amount − 1).
//!   Everything else uses `5.sil[id]`. Frame 0 is used; the game picks a random start frame.
//! * Buildings: file `10 + race`, the finished part (`*_STANDARD`, dil +1), frame 0.
//! * Settlers: file `20 + race`, `sil[type]` (the type's walk job), direction 0, frame 0.
//! * Vehicles (settler records with kind >= 200): file `30 + race`, `sil[type]`, direction 0.
//! * Piles: file 3, job = good, dil = amount − 1 (clamped to the file's 8), frame 0.
//!
//! Race = the owner's race from the player chunk; random (255) or missing falls back to Roman.

use std::collections::HashMap;
use std::path::Path;

use s4_formats::gamedata::resolve::{self, BuildingPart};
use s4_formats::gamedata::{Building, Good, Object, Race, Settler, Vehicle};
use s4_formats::gfx::landscape::{DEFAULT_TRIANGLE_SIZE, height_offset_fixed};
use s4_formats::gfx::{DecodeOptions, GfxFile, IndexedImage, PlayerColor};
use s4_formats::logo;

use crate::animate::Pose;
use crate::gpu::View;
use crate::population::Population;
use crate::terrain::object_fog;
use crate::waves::{WAVE_JOBS, WAVE_SLOTS_USED, Wave, wave_at};


/// Atlas layer edge (texels).
pub const SPRITE_ATLAS: u32 = 2048;
/// Palette row width: 256 colours + 256 per-player ramp entries.
const PAL_W: u32 = 512;
/// First palette rows hold the 8 player gradients (entries 0..32).
const PLAYER_ROWS: u32 = 8;
/// The players' logos (`GetUserLogoGfxInfo`, `CGfxManager` vt+4 0x461cd0).
const LOGO_FILE: u32 = 1;
/// Palette-row key bit of a composed logo (file palette offsets stay far below it).
const LOGO_PALETTE: u32 = 0x8000_0000;

/// Placeholder player colours (the real ones come from the game setup via
/// `IGfxEngine::SetPlayerColor`; no constant table for them was found).
pub const PLAYER_COLORS: [[u8; 3]; 8] = [
    [228, 32, 32],
    [32, 80, 228],
    [32, 180, 48],
    [232, 216, 40],
    [160, 48, 200],
    [240, 128, 24],
    [32, 200, 210],
    [235, 235, 235],
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct SpriteKey {
    file: u32,
    image: u32,
}

/// Where a decoded sprite sits in the atlas.
#[derive(Clone, Copy, Debug)]
struct AtlasSlot {
    x: u32,
    y: u32,
    layer: u32,
    w: u32,
    h: u32,
    left: i32,
    top: i32,
    half: bool,
    pal_row: u32,
    team: bool,
}

/// Draw layer at a node, in the original's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Layer {
    /// Border stone (territory border), the first thing drawn at a node.
    Border = 0,
    /// Wave animation frame (see `waves.rs`), before the node's object.
    Wave = 1,
    Object = 2,
    Settler = 3,
    Vehicle = 4,
    /// Drawn after every row (`RenderObjectLayer`'s deferred list).
    Ship = 5,
    /// `CEffects` effects: after all objects of their row.
    Effect = 7,
    /// Accessory icons (build markers): `RenderObjectLayer`'s icon pass after all objects.
    Marker = 6,
    /// A building's patch slot 4 (footbridges): `BlitBuilding` (0x66fbb0) queues it in the
    /// deferred list (marker −1), which `RenderObjectLayer` draws after every row and before the
    /// ships.
    Deferred = 8,
}

/// File 5 job of the border stones (`MoveWavesToGfxEngine`: `SetIndexBorderstone` with
/// `GetObjectFrameCount(0x215)`, then one `PutLandscapeObject` per frame). Frame `owner − 1` is
/// the stone of player `owner` (`g_pBorderstoneGfx[owner]`, `BlitBorderstone`).
const BORDER_JOB: usize = 533;

/// `Gfx.gfx`: the accessory icons (`CGfxManager::MoveAccessoriesToGfxEngine`, file index 7 of
/// `m_pFileGfxFrames`).
const ICON_FILE: u32 = 7;

/// `Gfx\8.gfx`: the animals (`GetAnimalGfxInfo` 0x4624e0, `GUIElementEngine +0x62c`).
const ANIMAL_FILE: u32 = 8;

/// The buildings' patch slots as the client steps them: per building its race and per slot
/// `(job, frame)` ([`SpriteScene::set_patches`]).
pub type ShownPatches = HashMap<u16, (u8, [(u16, u32); 10])>;

/// An ambient effect of the client's `CAnimalMgr` part as it is drawn now
/// ([`SpriteScene::set_ambient`]; s4-game `ambient.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AmbientSprite {
    /// A jumping fish (`CEffects` record type 7, drawn by 0x4e2b20): `Gfx\8.gfx` job `job`,
    /// direction `dir`, frame `frame` (0x4624e0: 0 when past direction 2's frame count), at the
    /// node's anchor, after the objects of its row, where the fog is above 59.
    Fish { x: u16, y: u16, job: u16, dir: u8, frame: u32 },
    /// A `CEffects` moving effect or one attached to it (record types 8 and 11, drawn by 0x4e2b70
    /// / 0x4e2eb0): a butterfly, bird or seagull, a manakopter's parts (flyer_gfx.rs). Effect
    /// `gfx` (0x4625b0: `Gfx\6.gfx` job `gfx` below 100, else `Gfx\37.gfx` job `gfx − 100`),
    /// direction `dir`, frame `frame` (0 when not below direction 0's frame count) at world
    /// screen offset `px` (zoom 1), through the deferred list (+0x2c9 = 0xc0); the fog test and
    /// the row at node `(x, y)` (+0x10 / +0x14).
    Effect { gfx: u8, dir: u8, frame: u32, x: u16, y: u16, px: (i32, i32) },
}

#[derive(Clone, Copy, Debug)]
struct Placed {
    key: SpriteKey,
    x: u16,
    y: u16,
    layer: Layer,
    /// Player slot 0..8 (team colour), ignored for non-team files.
    player: u8,
    /// Animated wave: the image is chosen per tick from the slot's frames.
    wave: Option<Wave>,
    /// Drawn only where the node's fog value is above this.
    min_fog: u8,
    /// Pixel offset from the node anchor (building parts).
    off: PixOff,
    /// Animated deco object: (first image of the job, frames in the cycle, phase at game tick 0,
    /// [`DecoAnim`] mode), see [`deco_frame`]. For a settler (`entity` set): its job's frames in
    /// its direction, the frame given by its [`Pose`].
    anim: Option<(u32, u16, u16, u8)>,
    /// Construction progress clip (`SGfxObjectInfo +92`, 1..0xFFFE): only the rows below
    /// `progress · height / 0xFFFF − 10` are drawn (`BlitBuilding`'s `ZoomBobNormal` from the cut
    /// line plus `ZoomBobBuildState`'s 10-row band above it).
    crop: Option<u16>,
    /// World entity drawn here (settlers, vehicles), for interpolated positions.
    entity: Option<u16>,
    /// Building patch slot `(building, slot)`: the image is the slot's current job and frame
    /// ([`SpriteScene::set_patches`]); `key` (the `PERMANENT` patch at frame 0) stands for it
    /// while the client has no patch state for the building.
    patch: Option<(u16, u8)>,
    /// A vehicle's own image or a moving ship's second layer (see [`VPart`]).
    vpart: VPart,
}

/// The parts of a vehicle's gfx info that its turn replaces (0x594b50 / 0x55bbd0 set the image
/// and clear the second layer +0xc).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VPart {
    None,
    /// The vehicle's image: its turn sprite while it turns.
    Main,
    /// A moving ship's job +0x18 over its hull (0x462420): not drawn while it turns.
    Overlay,
}

/// A building part drawn relative to the building's anchor, as `BlitBuilding` (0x2f6b180) does:
/// top-left = anchor + `(dx, dy)` − image `(left, top)`, minus `rel`'s `(left, top)` as well when
/// set (the heraldic figure hangs off the flag image's top-left).
#[derive(Clone, Copy, Debug)]
struct PixOff {
    dx: i16,
    dy: i16,
    rel: Option<SpriteKey>,
    /// Building patch (`BlitBuilding`'s FX layers): drawn at the main image's top-left with the
    /// main image's size, its own `left/top` ignored.
    at_main: bool,
}

impl PixOff {
    const NONE: PixOff = PixOff { dx: 0, dy: 0, rel: None, at_main: false };
}

/// The rows (ascending) of `size`-wide grids where `differ(old row, new row)`.
fn changed_rows(size: usize, old: &[u8], new: &[u8], differ: impl Fn(&[u8], &[u8]) -> bool) -> Vec<usize> {
    old.chunks(size).zip(new.chunks(size)).enumerate().filter(|(_, (a, b))| a != b && differ(a, b)).map(|(y, _)| y).collect()
}

/// `RenderObjectLayer`'s order: ships after everything; else by row, the row's effects after its
/// nodes, then x, then the layer at the node.
fn draw_order(p: &Placed) -> (u8, u16, bool, u16, Layer) {
    let deferred = match p.layer {
        Layer::Deferred => 1u8,
        Layer::Ship => 2,
        _ => 0,
    };
    (deferred, p.y, p.layer == Layer::Effect, p.x, p.layer)
}

/// Flag ("standarte") object of a military building per race, `CMilitaryBuildingRole::FillGfxInfo`:
/// Roman 541, Viking 543, Maya and Trojan 542 (file 5 jobs; objects STANDARTE_R/V/M + 261).
fn flag_job(race: Race) -> Option<usize> {
    match race {
        Race::ROMAN => Some(541),
        Race::VIKING => Some(543),
        Race::MAYA | Race::TROJAN => Some(542),
        Race::DARK => None,
    }
}

/// `RenderObjectLayer`: object-layer entries need fog > 17; buildings (kind 8) and kind-16 objects
/// (`CStone`, `CPlant`, `CMushroom`, `CPile`, `CTree` unless grown, deco id 123) > 59; deco
/// objects (kind 32, grown trees) > 17. Settlers, vehicles, ships and border stones > 59, waves
/// > 58.
const FOG_DECO: u8 = 17;
const FOG_SOLID: u8 = 59;

/// GPU instance (32 bytes).
#[derive(Clone, Copy, Debug)]
pub struct SpriteInstance {
    pub pos: [f32; 2],
    pub size: [f32; 2],
    /// Atlas texel origin (x | y << 16).
    pub origin: u32,
    pub layer: u32,
    pub pal_row: u32,
    /// bit 0 team colour, bit 1 interface ([`FLAG_UI`]), bits 8..10 player slot, 12..19 fog value,
    /// 20..31 sprite width in texels.
    pub flags: u32,
}

/// A marker on the game rather than part of the world (build marks, selection and health icons,
/// the cursor building): the cloud shadows leave it alone.
const FLAG_UI: u32 = 2;

const INSTANCE_BYTES: u64 = 32;
const UNIFORM_BYTES: u64 = 32;

impl SpriteInstance {
    fn write(&self, out: &mut Vec<u8>) {
        for f in self.pos.iter().chain(&self.size) {
            out.extend_from_slice(&f.to_le_bytes());
        }
        for u in [self.origin, self.layer, self.pal_row, self.flags] {
            out.extend_from_slice(&u.to_le_bytes());
        }
    }
}

/// CPU side: the decoded, packed sprites of a map and their placed, ordered instances.
pub struct SpriteScene {
    /// `SPRITE_ATLAS²` R8 layers.
    pub atlas: Vec<Vec<u8>>,
    /// `PAL_W × rows` RGBA8 (sRGB-encoded, straight alpha).
    pub palettes: Vec<u8>,
    pub palette_rows: u32,
    slots: HashMap<SpriteKey, AtlasSlot>,
    placed: Vec<Placed>,
    heights: Vec<u8>,
    ground: Vec<u8>,
    gradient: Vec<u8>,
    /// `CalcCurrentFog` of every node (255 = clear).
    fog: Vec<u8>,
    /// Wave and border-stone entries (they depend on fog and territory), in draw order.
    waves: Vec<Placed>,
    /// Territory owner per node (0 = nobody). Stand-in for the simulation's owner map: the map's
    /// chunk 6 byte 1, ignored on water as `CWorldManager::StartUpOwner` does.
    owner: Vec<u8>,
    /// Images of the border stones, one per player.
    border_frames: std::ops::Range<usize>,
    size: usize,
    /// Image range of each wave slot (file 5).
    wave_frames: Vec<std::ops::Range<usize>>,
    /// Game tick the wave and deco frames are chosen for.
    tick: u64,
    /// Some deco objects animate (rebuild on every tick).
    animated: bool,
    /// Draw-time poses per entity, see [`SpriteScene::set_unit_poses`].
    poses: HashMap<u16, Pose>,
    /// Each placed vehicle's `+0x61` (direction 2's frame count of its job +0x26), for the
    /// draw-time update ([`crate::animate::Animator::vehicle_poses`]).
    vehicle_frames: HashMap<u16, u8>,
    /// Each placed vehicle's 16 turn sprites ([`crate::vehicle_gfx::TURN`]), shown while its
    /// pose says it turns.
    vehicle_turns: HashMap<u16, [SpriteKey; 16]>,
    /// World-space instances in draw order.
    instances: Vec<SpriteInstance>,
    /// Something they depend on changed: [`SpriteScene::instances`] rebuilds them first (once
    /// per frame however many setters ran).
    stale: bool,
    pub stats: SceneStats,
    files: Files,
    gamedata: s4_formats::gamedata::GameData,
    /// Palette row of each (file, palette offset).
    pal_index: HashMap<(u32, u32), u32>,
    /// Shelf packer state in the last atlas layer: (x, y, shelf height).
    shelf: (u32, u32, u32),
    dirty: Dirty,
    /// Build-site markers (accessory icons, file 7), drawn after everything else.
    markers: Vec<Placed>,
    /// Shown construction progress (`CBuildingSiteRole +428`) per site position.
    site_shown: HashMap<(u16, u16), i32>,
    /// The decos last placed and their entries ([`SpriteScene::place`]).
    decos: Option<PlacedDecos>,
    /// Animation cycles `(file, first image, frames)` whose images were all asked for once.
    cycles_done: std::collections::HashSet<(u32, u32, u16)>,
    /// The building under the cursor in build mode (`g_sGfxObjectCursorBuilding`).
    ghost: Vec<Placed>,
    /// Selection and health icons of the selected settlers (`BlitSettler`'s icon lists).
    selection: Vec<Placed>,
    /// The building patches' images now ([`SpriteScene::set_patches`]): per building, per slot.
    patch_state: HashMap<u16, [Option<SpriteKey>; 10]>,
    /// Map effects (`CEffects`) that have started and not yet run out.
    effects: Vec<MapEffect>,
    /// The last handle [`SpriteScene::add_lasting_effect`] gave out.
    effect_handle: u32,
    /// The ambient effects now ([`SpriteScene::set_ambient`]), placed.
    ambient: Vec<Placed>,
    /// Sprite file 1 as the release composes it from the players' logos
    /// ([`SpriteScene::set_logos`]): image → (pixels, palette row).
    logos: HashMap<u32, (IndexedImage, Vec<u8>)>,
}

/// The tribe effects' settler gfx jobs (.rdata 0x1000774, indexed by the effect; the words
/// after entry 3 are pointers of another table, so only effects 0..=3 are effects). The dying
/// warrior's three are 1..=3.
pub const TRIBE_EFFECT_JOBS: [u16; 4] = [0, 0x14d, 0x14e, 0x14f];

/// An effect of `CEffects` (record type 2, `sub_E2190` 0x4e2190, drawn by `sub_E2A60` 0x4e2a60).
#[derive(Clone, Copy, Debug)]
struct MapEffect {
    file: u32,
    /// The job's first image in direction 0, and how many images it has (the drawn frame is 0
    /// when the age modulo `frames` is not below this, `sub_625B0` 0x4625b0).
    first: u32,
    images: u16,
    /// `+6`: the job's frame count (`GetJobFrameCount` 0x461fe0, at least 1); also the lifetime
    /// `+0xc` in ticks, since `AddEffect` passes no duration.
    frames: u16,
    x: u16,
    y: u16,
    /// `+8`: the game tick it started (`GlobalGameManager +0x30` at `AddEffect`).
    start: u64,
    /// `+0xc`: its lifetime in ticks.
    life: u64,
    /// A flight (record type 6 0x4e25c0 drawn by 0x4e2ab0, or a projectile 0x4e2310), in the row
    /// of its node.
    fly: Option<Fly>,
    /// A lasting effect's handle ([`SpriteScene::add_lasting_effect`]); 0 for the others.
    handle: u32,
    /// The player colour slot (a tribe effect's +5, [`SpriteScene::add_tribe_effect`]); 0 for
    /// the others, whose files have no team colour.
    player: u8,
    /// The draw routine sets +0x2c9 to 0xc0 or more (routines 8 to 11): drawn from the deferred
    /// list after every row (0x660070: +0x2c8 == 1 → the team-colour blit, else +0x2c9 < 0xc0 →
    /// in the row, else deferred; 0x4e3580 resets +0x2c9 to 0 before each routine).
    deferred: bool,
}

/// A flying effect's path: the world screen offset at its start (+0x18 / +0x1c), its move per
/// tick (+0x20 / +0x24) and a projectile's fall per tick² (+0x30). At age `t` (`f32`) the draw
/// routines (0x4e2ab0, 0x4e2dc0, 0x4e2c80) put it at `(vx·t + x, vy·t + y + t²·fall)`.
#[derive(Clone, Copy, Debug)]
struct Fly {
    start: (f32, f32),
    vel: (f32, f32),
    fall: f32,
}

impl Fly {
    fn at(&self, t: f32) -> (f32, f32) {
        (self.vel.0 * t + self.start.0, self.vel.1 * t + self.start.1 + (t * t) * self.fall)
    }
}

/// One record of the projectile table (.rdata 0x1000640, stride 0x1c), as 0x4e2310 reads it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProjectileKind {
    /// +0: the effect (≥ 100: file 37, job − 100; else file 6).
    pub gfx: u32,
    /// +4: added to the start's screen y.
    pub y_offset: f32,
    /// +8: the arc.
    pub arc: f32,
    /// +0xc: ticks per pixel.
    pub speed: f32,
    /// +0x10: the pixels the flight starts after the shooter and ends before the target.
    pub launch: f32,
    /// +0x14 is 0x4e1f30 (one of 16 directions by the flight's vector), not 0x4e1f20 (0).
    pub aims: bool,
    /// +0x18: the draw routine (table 0x10005fc): 4 draws frame +7 (0), 5 and 10 the frame
    /// `age % frames`.
    pub routine: u8,
}

impl ProjectileKind {
    /// Kind `kind` of the simulation's table (`s4_sim::projectile::KINDS`, .rdata 0x1000640),
    /// with its +0x14: 0x4e1f30 for kinds 0, 1 and 10, 0x4e1f20 for the others.
    pub fn of(kind: u8) -> Option<ProjectileKind> {
        let k = s4_sim::projectile::KINDS.get(kind as usize)?;
        Some(ProjectileKind {
            gfx: k.gfx,
            y_offset: f32::from_bits(k.y_offset),
            arc: f32::from_bits(k.arc),
            speed: f32::from_bits(k.speed),
            launch: k.launch as f32,
            aims: matches!(kind, 0 | 1 | 10),
            routine: k.routine,
        })
    }
}

/// 0x4e2310's path between two screen positions (0x65b130) with the shooter's pixel offset:
/// the flight, its lifetime (+0xc) and its direction (+5).
fn projectile_path(kind: &ProjectileKind, f: (i32, i32), t: (i32, i32), offset: (i32, i32)) -> (Fly, u64, usize) {
    let (sx, sy) = (offset.0 as f32 + f.0 as f32, offset.1 as f32 + f.1 as f32);
    let (dx, dy) = (t.0 as f32 - sx, t.1 as f32 - sy);
    let d = ((dy * dy + dx * dx) as f64).sqrt() as f32;
    let dir = if kind.aims { aim(dx, dy) as usize } else { 0 };
    let mut fly = Fly { start: (sx, sy), vel: (0.0, 0.0), fall: 0.0 };
    let mut life = 1;
    let l = d - (kind.launch + kind.launch);
    if d > 0.0 && l > 0.0 {
        let k = (1.0 / d) * kind.launch;
        fly.start = (sx + k * dx, sy + k * dy);
        let k2 = k + k;
        let rest = (dx - k2 * dx, dy - k2 * dy);
        let n = ((kind.speed * l) as i32).max(1);
        life = n as u64 + 1;
        let inv = 1.0 / n as f32;
        let a = kind.arc * l * 4.0 * inv;
        fly.vel = (inv * rest.0, inv * rest.1 - a);
        fly.fall = a * inv;
    }
    fly.start.1 += kind.y_offset;
    (fly, life, dir)
}

/// 0x4e1f30: the direction (0..16) of a flight vector `(dx, dy)`.
pub(crate) fn aim(dx: f32, dy: f32) -> u8 {
    const C: f32 = 0.668_178_6; // 0x1070dd4 (0x1070fac is its negative)
    const T: f32 = 0.414_213_57; // 0x1070dc4, tan 22.5°
    let (a, b) = (dy * -C - dx, dy - dx * C);
    let (base, p, q) = if a >= 0.0 {
        if b >= 0.0 { (0, b, a) } else { (4, a, -b) }
    } else if b <= 0.0 {
        (8, -b, -a)
    } else {
        (12, -a, b)
    };
    if p >= q { base + (q >= p * T) as u8 } else { base + 2 + (q * T >= p) as u8 }
}

#[cfg(test)]
mod aim_tests {
    /// 0x4e1f30 splits the circle into 16 contiguous sectors, numbered in order.
    #[test]
    fn sixteen_sectors_in_order() {
        let dirs: Vec<u8> = (0..3600).map(|i| (i as f32 / 10.0).to_radians()).map(|t| super::aim(t.cos(), t.sin())).collect();
        let mut runs = vec![dirs[0]];
        for &d in &dirs {
            if d != *runs.last().unwrap() {
                runs.push(d);
            }
        }
        if runs.len() > 16 && runs[0] == *runs.last().unwrap() {
            runs.pop();
        }
        assert_eq!(runs.len(), 16, "{runs:?}");
        let mut sorted = runs.clone();
        sorted.sort();
        assert_eq!(sorted, (0..16).collect::<Vec<u8>>());
        let k = runs.iter().position(|&d| d == 0).unwrap();
        let step = (runs[(k + 1) % 16] + 16 - runs[k]) % 16;
        assert!(step == 1 || step == 15);
        for i in 0..16 {
            assert_eq!((runs[(i + 1) % 16] + 16 - runs[i]) % 16, step, "{runs:?}");
        }
    }

    /// The drawn lifetime is the simulation's `frames` (`s4_sim::projectile::flight`, exact
    /// integers) for every kind over a spread of positions; the path ends `launch` px before the
    /// target (plus the arc's return to the line: the fall cancels the lift at `n`).
    #[test]
    fn lifetime_is_the_simulations_flight() {
        let mut r = 12345u32;
        let mut next = |m: i32| {
            r = r.wrapping_mul(1_103_515_245).wrapping_add(12345);
            ((r >> 8) % (2 * m as u32 + 1)) as i32 - m
        };
        for kind in 0..11u8 {
            let k = super::ProjectileKind::of(kind).unwrap();
            for _ in 0..2000 {
                let (f, t, o) = ((next(4000), next(4000)), (next(4000), next(4000)), (next(40), next(60)));
                let (fly, life, _) = super::projectile_path(&k, f, t, o);
                assert_eq!(life, s4_sim::projectile::flight(kind, f, t, o) as u64, "kind {kind} {f:?} {t:?} {o:?}");
                if life > 1 {
                    let n = (life - 1) as f32;
                    let (ex, ey) = fly.at(n);
                    let (sx, sy) = ((o.0 + f.0) as f32, (o.1 + f.1) as f32);
                    let d = (((t.0 as f32 - sx).powi(2) + (t.1 as f32 - sy).powi(2)) as f64).sqrt() as f32;
                    let want = (t.0 as f32 - (t.0 as f32 - sx) * k.launch / d, t.1 as f32 - (t.1 as f32 - sy) * k.launch / d + k.y_offset);
                    assert!((ex - want.0).abs() < 0.5 && (ey - want.1).abs() < 0.5, "kind {kind}: end {:?} want {want:?}", (ex, ey));
                }
            }
        }
    }
}

/// What changed in the atlas and palettes since the GPU last took it.
#[derive(Clone, Debug, Default)]
pub struct Dirty {
    /// Per atlas layer: the changed rows `start..end`.
    pub atlas: std::collections::BTreeMap<u32, (u32, u32)>,
    pub palettes: bool,
}

/// Decos and the entries placed for them, in draw order.
#[derive(Default)]
struct PlacedDecos {
    decos: Vec<crate::population::Deco>,
    /// Per deco its entry (`None`: no image).
    entries: Vec<Option<Placed>>,
    placed: Vec<Placed>,
    objects: usize,
    skipped: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct SceneStats {
    pub objects: usize,
    pub buildings: usize,
    pub settlers: usize,
    pub animals: usize,
    pub vehicles: usize,
    pub piles: usize,
    pub waves: usize,
    pub border_stones: usize,
    pub unique_sprites: usize,
    pub atlas_layers: usize,
    /// Things that could not be resolved to an image (id, reason).
    pub skipped: Vec<String>,
    /// Per owner: (buildings + settlers, first position seen).
    pub owners: std::collections::BTreeMap<u8, (usize, (u16, u16))>,
}

/// Opens gfx files on demand.
struct Files {
    dir: std::path::PathBuf,
    open: HashMap<u32, GfxFile>,
}

impl Files {
    fn get(&mut self, n: u32) -> anyhow::Result<&GfxFile> {
        if !self.open.contains_key(&n) {
            let f = GfxFile::open(&self.dir, n)?;
            self.open.insert(n, f);
        }
        Ok(&self.open[&n])
    }
}

impl SpriteScene {
    /// Collect, decode and pack everything a population places (see `population.rs`).
    pub fn from_population(pop: &Population) -> anyhow::Result<Self> {
        Self::from_population_in(pop, &s4_formats::game_dir().join("Gfx"))
    }

    pub fn from_population_in(pop: &Population, gfx_dir: &Path) -> anyhow::Result<Self> {
        let mut files = Files { dir: gfx_dir.to_path_buf(), open: HashMap::new() };

        // Wave animations (see waves.rs): every frame of the slots the per-node helper uses.
        let mut wave_frames: Vec<std::ops::Range<usize>> = Vec::new();
        let border_frames;
        {
            let f = files.get(resolve::OBJECT_FILE)?;
            for &job in &WAVE_JOBS {
                wave_frames.push(f.job_frames(job, 0).unwrap_or(0..0));
            }
            border_frames = f.job_frames(BORDER_JOB, 0).unwrap_or(0..0);
        }

        // Palettes: 8 player-gradient rows, then one row per (file, palette offset).
        let mut palettes = vec![0u8; (PAL_W * PLAYER_ROWS * 4) as usize];
        for (p, rgb) in PLAYER_COLORS.iter().enumerate() {
            let pc = PlayerColor { slot: p as u8, rgb: *rgb };
            for k in 0..32 {
                let c = pc.gradient(k);
                let o = ((p as u32 * PAL_W + k as u32) * 4) as usize;
                palettes[o..o + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }

        let mut scene = SpriteScene {
            atlas: Vec::new(),
            palettes,
            palette_rows: PLAYER_ROWS,
            slots: HashMap::new(),
            placed: Vec::new(),
            heights: pop.height.clone(),
            ground: pop.ground.clone(),
            gradient: pop.gradient.clone(),
            fog: pop.gfx_bits.iter().map(|&g| object_fog(0, g as u32 & 7, (g as u32 >> 3) & 7)).collect(),
            waves: Vec::new(),
            owner: pop.owner.clone(),
            border_frames,
            size: pop.size,
            wave_frames,
            tick: pop.tick as u64,
            animated: false,
            poses: HashMap::new(),
            vehicle_frames: HashMap::new(),
            vehicle_turns: HashMap::new(),
            instances: Vec::new(),
            stale: false,
            stats: SceneStats::default(),
            files,
            gamedata: s4_formats::gamedata::GameData::load_default()?,
            pal_index: HashMap::new(),
            shelf: (SPRITE_ATLAS, 0, 0),
            dirty: Dirty::default(),
            markers: Vec::new(),
            site_shown: HashMap::new(),
            decos: None,
            cycles_done: Default::default(),
            ghost: Vec::new(),
            selection: Vec::new(),
            patch_state: HashMap::new(),
            effects: Vec::new(),
            effect_handle: 0,
            ambient: Vec::new(),
            logos: HashMap::new(),
        };
        scene.set_logos(&s4_formats::logo::installed_current(&s4_formats::game_dir()));
        let mut keys: Vec<SpriteKey> = Vec::new();
        for r in scene.wave_frames[..WAVE_SLOTS_USED].iter().chain([&scene.border_frames]) {
            keys.extend(r.clone().map(|i| SpriteKey { file: 5, image: i as u32 }));
        }
        scene.add_sprites(keys)?;
        scene.place(pop)?;
        scene.regen_waves();
        scene.stale = true;
        Ok(scene)
    }

    /// Compose sprite file 1 from the players' logos, as the release writes `Gfx\1.*` from
    /// `Logo\Current\HerFig_<slot>.png` (see `s4_formats::logo`): slot `p` → images `2p`, `2p + 1`
    /// (the flags draw image `2 (owner − 1)`). A missing logo keeps the shipped image. File 1
    /// images already packed are packed again.
    pub fn set_logos(&mut self, current: &[Option<Vec<u8>>; 8]) {
        self.logos.clear();
        for (slot, rgb) in current.iter().enumerate() {
            let Some(rgb) = rgb.as_ref().filter(|c| c.len() == logo::SIZE * logo::SIZE * 3) else { continue };
            let (pixels, palette) = logo::indexed(rgb);
            let header = s4_formats::gfx::ImageHeader { width: logo::FRAME as u16, height: logo::FRAME as u16, left: logo::HOTSPOT as i16, top: logo::HOTSPOT as i16, flags: 0, cache_id: 0, data_offset: 0 };
            let img = IndexedImage { header, pixels, stream_len: 0, terminated: true, raw: false };
            let mut row = vec![0u8; (PAL_W * 4) as usize];
            for (i, c) in palette.iter().enumerate() {
                row[i * 4..i * 4 + 4].copy_from_slice(c);
            }
            for image in [2 * slot as u32, 2 * slot as u32 + 1] {
                self.logos.insert(image, (img.clone(), row.clone()));
            }
        }
        self.cycles_done.retain(|c| c.0 != LOGO_FILE);
        let packed: Vec<SpriteKey> = self.slots.keys().filter(|k| k.file == LOGO_FILE).copied().collect();
        if packed.is_empty() {
            return;
        }
        for k in &packed {
            self.slots.remove(k);
        }
        self.pal_index.retain(|&(file, off), _| file != LOGO_FILE || off & LOGO_PALETTE == 0);
        if let Err(e) = self.add_sprites(packed) {
            tracing::warn!("logos: {e}");
        }
        self.stale = true;
    }

    /// Show a new population (the simulation's next snapshot): entities created since are
    /// placed, removed ones disappear, territory and heights follow. Only images not seen before
    /// are decoded and packed; the atlas grows, it is never repacked. The fog stays as it is.
    pub fn set_population(&mut self, pop: &Population) -> anyhow::Result<()> {
        anyhow::ensure!(pop.size == self.size, "population size {} for a {} map", pop.size, self.size);
        let t = crate::perf::span("sprites: place");
        self.place(pop)?;
        drop(t);
        self.heights.clone_from(&pop.height);
        if self.owner != pop.owner {
            // A border stone depends on the owners of its own and the neighbouring rows.
            let rows = changed_rows(self.size, &self.owner, &pop.owner, |a, b| a != b);
            self.owner.clone_from(&pop.owner);
            for (a, b) in crate::terrain::row_runs(&rows, 3) {
                self.regen_wave_rows(a.saturating_sub(1), b + 1);
            }
        }
        self.tick = self.tick.max(pop.tick as u64);
        self.stale = true;
        Ok(())
    }

    /// The building placed under the cursor in build mode: `ShowPossibleBuildingType` fills
    /// `GetBuildingGfxInfo(race, type, 1)` (the finished image, no patches) when the hovered node
    /// holds a marker, and `RenderObjectLayer` blits it with full light (`BlitBuilding(255, …)`)
    /// at the node after all rows of objects, before the icon pass. `(x, y, race, type, player)`.
    pub fn set_ghost(&mut self, ghost: Option<(u16, u16, u8, u8, u8)>) -> anyhow::Result<()> {
        let mut new = Vec::new();
        if let Some((x, y, race, kind, player)) = ghost {
            let race = Race::from_u16(race as u16).unwrap_or(Race::ROMAN);
            let file = resolve::building_file(race);
            if let Some(r) = Building::from_u16(kind as u16).and_then(|k| resolve::building_frames(self.files.get(file).ok()?, k, BuildingPart::Finished)) {
                let key = SpriteKey { file, image: r.start as u32 };
                self.add_sprites(vec![key])?;
                let player = player.saturating_sub(1) & 7;
                new.push(Placed { key, x, y, layer: Layer::Marker, player, wave: None, min_fog: 0, off: PixOff::NONE, anim: None, crop: None, entity: None, patch: None, vpart: VPart::None });
            }
        }
        if new.len() != self.ghost.len() || new.iter().zip(&self.ghost).any(|(a, b)| (a.x, a.y, a.key) != (b.x, b.y, b.key)) {
            self.ghost = new;
            self.stale = true;
        }
        Ok(())
    }

    /// Show accessory icons (`CWorldManager::SetHelperObject` → `BlitAccessoryIcon`, 0x2f6c8f0):
    /// `(x, y, icon)` with icon = image of `Gfx.gfx` (`MoveAccessoriesToGfxEngine`: file 7,
    /// images 0..96). Build markers are 10..=17 by leveling class, 18..=25 under the cursor, 74 / 75
    /// for mines. Drawn at the node anchor (with height), unshaded, after all objects.
    pub fn set_markers(&mut self, markers: &[(u16, u16, u8)]) -> anyhow::Result<()> {
        let keys: Vec<SpriteKey> = markers.iter().map(|m| SpriteKey { file: ICON_FILE, image: m.2 as u32 }).collect();
        self.add_sprites(keys)?;
        let new: Vec<Placed> = markers
            .iter()
            .map(|&(x, y, icon)| Placed {
                key: SpriteKey { file: ICON_FILE, image: icon as u32 },
                x,
                y,
                layer: Layer::Marker,
                player: 0,
                wave: None,
                min_fog: 0,
                off: PixOff::NONE,
                anim: None,
                crop: None,
                entity: None,
                patch: None,
                vpart: VPart::None,
            })
            .collect();
        if new.len() != self.markers.len() || new.iter().zip(&self.markers).any(|(a, b)| (a.x, a.y, a.key) != (b.x, b.y, b.key)) {
            self.markers = new;
            self.stale = true;
        }
        Ok(())
    }

    /// Icons over the selected settlers: `(x, y, entity, icon)` with icon = image of `Gfx.gfx`
    /// (selection kind 1 / 91..=95, health 2..=9). `BlitSettler` queues them at the settler's
    /// anchor minus the icon's own offsets, drawn unshaded after all objects; they follow the
    /// settler's interpolated position. A vehicle's icons (0x49 / 0x5a / 0x1b / 0x59, ammo,
    /// health) are drawn by the vehicle blitter (0x670be0) right after the vehicle instead.
    pub fn set_selection(&mut self, icons: &[(u16, u16, u16, u8)]) -> anyhow::Result<()> {
        let keys: Vec<SpriteKey> = icons.iter().map(|m| SpriteKey { file: ICON_FILE, image: m.3 as u32 }).collect();
        self.add_sprites(keys)?;
        let new: Vec<Placed> = icons
            .iter()
            .map(|&(x, y, entity, icon)| {
                // The building's brackets are part of its own blit (the object rows, its fog);
                // the settlers' icons go to the icon pass.
                let building = icon == crate::selection::BUILDING_ICON;
                let vehicle = self.placed.iter().find(|p| p.entity == Some(entity) && matches!(p.layer, Layer::Vehicle | Layer::Ship)).map(|p| p.layer);
                Placed {
                    key: SpriteKey { file: ICON_FILE, image: icon as u32 },
                    x,
                    y,
                    layer: if building { Layer::Object } else { vehicle.unwrap_or(Layer::Marker) },
                    player: 0,
                    wave: None,
                    min_fog: if building || vehicle.is_some() { FOG_SOLID } else { 0 },
                    off: PixOff::NONE,
                    anim: None,
                    crop: None,
                    entity: (!building).then_some(entity),
                    patch: None,
                    vpart: VPart::None,
                }
            })
            .collect();
        let same = new.len() == self.selection.len()
            && new.iter().zip(&self.selection).all(|(a, b)| (a.x, a.y, a.key, a.entity) == (b.x, b.y, b.key, b.entity));
        if !same {
            self.selection = new;
            self.stale = true;
        }
        Ok(())
    }

    /// `CEffects::AddEffect(effect, sound, x, y, 0, 0, 0)` (release 0x4e3a80) at game tick `tick`:
    /// record type 2 (`sub_E2190` 0x4e2190). The effect is the job of gfx file 6 (effects below
    /// 100) or 37 (`effect − 100`), the software object engine's files: `OEHardwareRendering` is
    /// only set when `IsHardwareObjectEngine` (0x505e20) is true, and that returns 0 in the shipped
    /// exe (0x658ca0 / 0x659770); the hardware files would be 4 / 36. Its frame count `+6` comes
    /// from direction 0 (`GetJobFrameCount` 0x461fe0, at least 1) and is also its lifetime, as no
    /// duration is passed. Drawn at age `tick − start` in `0..frames`, frame `age % frames`, then
    /// removed (`sub_E33B0` 0x4e33b0). `sub_E33B0` queues a record for drawing only when its
    /// effect byte (+1) is not 0: effect 0 is a sound without a picture (the geologist's rich
    /// sign, 0x57b308 `AddEffect(0, 0x26, ..)`); job 0 of file 6 is a 60×60 black block.
    pub fn add_effect(&mut self, effect: u8, x: u16, y: u16, tick: u64) -> anyhow::Result<()> {
        self.push_effect(effect, x, y, tick, None, 0)
    }

    /// `CEffects` vtbl+0x14 (release 0x4e3c80 → `sub_E2260` 0x4e2260, `effect, sound, x, y,
    /// player, delay`): record type 3, a tribe effect at node `(x, y)` from game tick `tick`.
    /// The job is .rdata 0x1000774[effect] ([`TRIBE_EFFECT_JOBS`]) of the player's tribe's
    /// settler file (`TribeTable[player] + 0x14`), +4 the tribe, +5 the player's colour
    /// (0x149b5e8[player]; [inf] slot `player − 1` as for settlers), +6 direction 2's frame
    /// count (at least 1) and the lifetime +0xc that count itself (0: never drawn). Draw routine
    /// 3 (`sub_E2D00` 0x4e2d00) shows direction 2's frame `age % +6` (0 when not below the
    /// count) at the node's `ConvertMapPositionToWorldScreenOffsets`, in the player's colour.
    /// The death effect (0x580bff: effects 1..=3, the owner).
    pub fn add_tribe_effect(&mut self, effect: u8, x: u16, y: u16, player: u8, race: Race, tick: u64) -> anyhow::Result<()> {
        // Effect 0 is never drawn (`sub_E33B0`, see [`SpriteScene::add_effect`]).
        if effect == 0 {
            return Ok(());
        }
        let Some(&job) = TRIBE_EFFECT_JOBS.get(effect as usize) else {
            crate::audit::miss(|| format!("tribe effect {effect} ({race:?}): no image"));
            self.stats.skipped.push(format!("tribe effect {effect} at {x},{y}"));
            return Ok(());
        };
        let file = resolve::settler_file(race);
        let f = self.files.get(file)?;
        let Some(r) = f.job_frames(job as usize, 2).filter(|r| !r.is_empty()) else { return Ok(()) };
        let count = r.len() as u16 & 0xff;
        let keys: Vec<SpriteKey> = r.clone().map(|i| SpriteKey { file, image: i as u32 }).collect();
        self.add_sprites(keys)?;
        let player = player.saturating_sub(1) & 7;
        self.effects.push(MapEffect { file, first: r.start as u32, images: count, frames: count.max(1), x, y, start: tick, life: count as u64, fly: None, handle: 0, player, deferred: false });
        self.stale = true;
        Ok(())
    }

    /// `CEffects` vtbl+0x28 (release 0x4e3d60, `effect, x, y`): a lasting effect, record type 8
    /// (0x4e2800): the same file, job and frames as [`SpriteScene::add_effect`], drawn by 0x4e2b70
    /// at frame `age % frames` at its node, with the lifetime 0x7fffffff. Returns its handle for
    /// [`SpriteScene::remove_effect`] (0 when the job has no images).
    pub fn add_lasting_effect(&mut self, effect: u8, x: u16, y: u16, tick: u64) -> anyhow::Result<u32> {
        self.effect_handle = self.effect_handle.wrapping_add(1).max(1);
        let handle = self.effect_handle;
        self.push_effect(effect, x, y, tick, Some(0x7fff_ffff), handle)?;
        Ok(if self.effects.iter().any(|e| e.handle == handle) { handle } else { 0 })
    }

    /// `CEffects` vtbl+0x24 (release 0x4e3d00): remove the effect with this handle.
    pub fn remove_effect(&mut self, handle: u32) {
        let n = self.effects.len();
        self.effects.retain(|e| handle == 0 || e.handle != handle);
        if self.effects.len() != n {
            self.stale = true;
        }
    }

    fn push_effect(&mut self, effect: u8, x: u16, y: u16, tick: u64, life: Option<u64>, handle: u32) -> anyhow::Result<()> {
        // Effect 0 is never drawn (`sub_E33B0`, see [`SpriteScene::add_effect`]).
        if effect == 0 {
            return Ok(());
        }
        let (file, job) = if effect < 100 { (6, effect as usize) } else { (37, effect as usize - 100) };
        // `sub_625B0`: from effect 86 (file 6) / 116 (file 37) on, the frame is bounded by job 0's
        // frame count instead of its own.
        let bound_job = if effect < 0x56 || (100..0x74).contains(&effect) { job } else { 0 };
        let f = self.files.get(file)?;
        let Some(r) = f.job_frames(job, 0).filter(|r| !r.is_empty()) else {
            crate::audit::miss(|| format!("effect {effect:#x}: no image"));
            self.stats.skipped.push(format!("effect {effect:#x} at {x},{y}"));
            return Ok(());
        };
        let images = f.job_frames(bound_job, 0).map_or(0, |b| b.len()).min(r.len()) as u16;
        let frames = (r.len() as u16 & 0xff).max(1);
        let keys: Vec<SpriteKey> = r.clone().map(|i| SpriteKey { file, image: i as u32 }).collect();
        self.add_sprites(keys)?;
        self.effects.push(MapEffect { file, first: r.start as u32, images, frames, x, y, start: tick, life: life.unwrap_or(frames as u64), fly: None, handle, player: 0, deferred: false });
        self.stale = true;
        Ok(())
    }

    /// `CEffects` 0x4e25c0: effect `effect` (file 6, direction 0, frame `age % frames`) flying
    /// from world screen offset `from` to `to` (zoom 1) in `ticks` ticks from tick `tick`, in the
    /// row of node `(x, y)` with its fog.
    #[allow(clippy::too_many_arguments)]
    pub fn add_flight(&mut self, effect: u8, x: u16, y: u16, from: (f32, f32), to: (f32, f32), ticks: u32, tick: u64) -> anyhow::Result<()> {
        let f = self.files.get(6)?;
        let Some(r) = f.job_frames(effect as usize, 0).filter(|r| !r.is_empty()) else { return Ok(()) };
        let frames = (r.len() as u16 & 0xff).max(1);
        let keys: Vec<SpriteKey> = r.clone().map(|i| SpriteKey { file: 6, image: i as u32 }).collect();
        self.add_sprites(keys)?;
        let k = 1.0 / ticks.max(1) as f32;
        let vel = ((to.0 - from.0) * k, (to.1 - from.1) * k);
        let fly = Fly { start: from, vel, fall: 0.0 };
        self.effects.push(MapEffect { file: 6, first: r.start as u32, images: frames, frames, x, y, start: tick, life: ticks.max(1) as u64, fly: Some(fly), handle: 0, player: 0, deferred: false });
        self.stale = true;
        Ok(())
    }

    /// `CEffects` vtbl+0x18 (0x4e3b20 → 0x4e2310): a projectile of `kind` from node `from` (+
    /// the pixel `offset`) to node `to`, appearing `delay` ticks after `tick`, drawn in the row
    /// of `from`. In `f32` as the release: the screen positions (0x65b130), `d` their distance,
    /// `L = d − 2·launch`; when `L > 0` it starts `launch` px along, takes `n = max(1, (int)
    /// (speed·L))` steps (lifetime `n + 1`) with the move `(remaining vector) / n` and the arc
    /// `a = arc·L·4 / n`: `vy −= a`, fall `a / n`; else it stands at the start for one tick. The
    /// direction is 0x4e1f30's when the kind aims; the file is 6 / 37 (0x65b1d0 false, as for
    /// [`SpriteScene::add_effect`]).
    #[allow(clippy::too_many_arguments)]
    pub fn add_projectile(&mut self, kind: &ProjectileKind, from: (u16, u16), to: (u16, u16), delay: i32, offset: (i32, i32), tick: u64) -> anyhow::Result<()> {
        let screen = |(x, y): (u16, u16)| {
            let h = self.heights.get(y as usize * self.size + x as usize).copied().unwrap_or(0);
            (24 * x as i32 - 12 * y as i32, 12 * y as i32 - s4_sim::projectile::height_px(h))
        };
        let (fly, life, dir) = projectile_path(kind, screen(from), screen(to), offset);
        let (file, job) = if kind.gfx < 100 { (6, kind.gfx as usize) } else { (37, kind.gfx as usize - 100) };
        // `sub_625B0` (0x4625b0) as in `push_effect`.
        let effect = kind.gfx;
        let bound_job = if effect < 0x56 || (100..0x74).contains(&effect) { job } else { 0 };
        let fl = self.files.get(file)?;
        let Some(r) = fl.job_frames(job, dir).filter(|r| !r.is_empty()) else {
            crate::audit::miss(|| format!("projectile {effect} dir {dir}: no image"));
            self.stats.skipped.push(format!("projectile {effect} dir {dir} at {},{}", from.0, from.1));
            return Ok(());
        };
        let images = fl.job_frames(bound_job, dir).map_or(0, |b| b.len()).min(r.len()) as u16;
        // +6: `GetJobFrameCount(file, job, 0)`, at least 1; routine 4 draws frame +7 = 0.
        let count = (fl.job_frames(job, 0).map_or(0, |c| c.len()) as u16 & 0xff).max(1);
        let frames = if matches!(kind.routine, 5 | 10) { count } else { 1 };
        let keys: Vec<SpriteKey> = r.clone().map(|i| SpriteKey { file, image: i as u32 }).collect();
        self.add_sprites(keys)?;
        let start = (tick as i64 + delay as i64).max(0) as u64;
        // Routine 10 (0x4e2c80) sets +0x2c9 = 0xc0: `RenderObjectLayer` (0x660070) puts the
        // effect on its deferred list instead of drawing it in its row.
        let deferred = kind.routine == 10;
        self.effects.push(MapEffect { file, first: r.start as u32, images, frames, x: from.0, y: from.1, start, life, fly: Some(fly), handle: 0, player: 0, deferred });
        self.stale = true;
        Ok(())
    }

    /// Each placed vehicle's `+0x61`: direction 2's frame count of its job +0x26.
    pub fn vehicle_frames(&self) -> &HashMap<u16, u8> {
        &self.vehicle_frames
    }

    /// `GetBoundingBox` (release 0x4626d0) for a ship or land vehicle (obj type 2 / 4): the
    /// width, height, left and top of image [job][0][0] of file `race + 0x1e` (job 0 when past
    /// the file's jobs), as `(width, height, left, top)` in px at zoom 1. `GetTheBestEntity`
    /// skips job 0.
    pub fn vehicle_bbox(&mut self, race: u8, job: u16) -> Option<(f32, f32, f32, f32)> {
        let file = resolve::vehicle_file(Race::from_u16(race as u16).unwrap_or(Race::ROMAN))?;
        let f = self.files.get(file).ok()?;
        let job = if job == 0 { return None } else { job as usize };
        let job = if job < f.job_count() { job } else { 0 };
        let h = f.header(f.job_frames(job, 0)?.start).ok()?;
        Some((h.width as f32, h.height as f32, h.left as f32, h.top as f32))
    }

    /// `GetBoundingBox` (release 0x4626d0) for a settler (obj type 1): 25 px wide, `left` 12.5
    /// (both times the zoom, truncated: 12 at zoom 1), and the height and top of the image
    /// `GetSettlerGfxInfo` (0x461d00) gives for the entity's gfx job `+0x26` (`job`, 0 when
    /// `>= 0x16f`), frame 1 (0 when direction 2 has fewer frames) of direction 0, as `(width,
    /// height, left, top)` in px at zoom 1. `job` `None` (raw map data, no task yet): the type's
    /// walk job.
    pub fn settler_bbox(&mut self, kind: u16, race: u8, job: Option<u16>) -> Option<(f32, f32, f32, f32)> {
        let race = Race::from_u16(race as u16).unwrap_or(Race::ROMAN);
        let file = resolve::settler_file(race);
        let f = self.files.get(file).ok()?;
        let job = match job {
            Some(j) => if j < 0x16f { j as usize } else { 0 },
            None => resolve::settler_job(f, Settler::from_u16(kind).filter(|_| kind < 67)?)?,
        };
        let count = resolve::settler_frames(f, job, 2).map_or(0, |r| r.len());
        let r = resolve::settler_frames(f, job, 0)?;
        let frame = if 1 < count { 1 } else { 0 };
        let h = f.header(r.start + frame.min(r.len().saturating_sub(1))).ok()?;
        Some((25.0, h.height as f32, 12.0, h.top as f32))
    }

    /// Atlas and palette regions changed since the last call (for the GPU upload).
    pub fn take_dirty(&mut self) -> Dirty {
        std::mem::take(&mut self.dirty)
    }

    /// Resolve what `pop` places (ordered as the original draws) and make sure every image it
    /// needs is in the atlas.
    fn place(&mut self, pop: &Population) -> anyhow::Result<()> {
        let race_of = |r: u8| Race::from_u16(r as u16).unwrap_or(Race::ROMAN);
        let files = &mut self.files;
        let gamedata = &self.gamedata;
        let mut placed = Vec::new();
        let mut stats = SceneStats::default();
        let t = crate::perf::span("place: decos");

        // Deco objects. Trees, plants and mushrooms show gfx job first + stage (`CTree` /
        // `CPlant::GetGfxInfo` draw their +0x26); stone heaps are OBJECT_MINESET1 with dil
        // `amount − 1`. Their entries depend on the decos only (most populations change none):
        // kept with them, in draw order.
        let decos_new = self.decos.as_ref().is_none_or(|c| c.decos != pop.decos);
        if decos_new {
            let _t = crate::perf::span("place: decos changed");
            // Only the decos that changed are resolved again (a tree grows, one is felled). Both
            // lists are in node order (one deco a node): walked side by side.
            let old = self.decos.take().unwrap_or_default();
            // A deco's animation runs from the tick its frame was set (`GetGfxInfo` adds the
            // ticks since its last call, which the constructor sets to the creation tick, 0 for a
            // tree): the map's decos from 0, a new or changed one (a stage, a new object) from
            // this population's tick. [inf] The release advances only while it is drawn; drawn
            // every tick, that is the same.
            let anchor = if old.decos.is_empty() { 0 } else { pop.tick };
            let mut o = old.decos.iter().zip(&old.entries).peekable();
            let mut c = PlacedDecos { decos: pop.decos.clone(), ..Default::default() };
            for d in &pop.decos {
                while o.next_if(|(od, _)| (od.y, od.x) < (d.y, d.x)).is_some() {}
                let entry = match o.peek() {
                    Some((od, e)) if *od == d => **e,
                    _ => place_deco(files.get(resolve::OBJECT_FILE)?, gamedata, d, anchor),
                };
                match entry {
                    Some(p) => {
                        c.placed.push(p);
                        c.objects += 1;
                    }
                    None => c.skipped.push(format!("object {} at {},{}", d.id, d.x, d.y)),
                }
                c.entries.push(entry);
            }
            c.placed.sort_by_key(draw_order);
            self.decos = Some(c);
        }
        if let Some(c) = &self.decos {
            stats.objects = c.objects;
            stats.skipped.clone_from(&c.skipped);
        }

        drop(t);
        let t = crate::perf::span("place: units");
        // Construction sites: `CBuildingSiteRole::FillGfxInfo` moves the shown progress (+428)
        // towards the work done, at most `builders` per call ([inf] once per population here,
        // once per drawn frame in the original), and picks the image and its clip.
        let mut shown = HashMap::new();
        for b in &pop.buildings {
            if let Some(g) = b.site {
                let mut v = self.site_shown.get(&(b.x, b.y)).copied().unwrap_or(0);
                let k = g.builders as i32;
                if v < g.work_done - k {
                    v = if v >= g.work_done - 2 * k { v + k } else { g.work_done };
                }
                shown.insert((b.x, b.y), v);
            }
        }
        self.site_shown = shown;
        let site_shown = &self.site_shown;
        let files = &mut self.files;

        // Buildings. Military buildings get their garrison and flag (see military_parts).
        for b in &pop.buildings {
            if let Some(g) = b.site {
                let race = race_of(b.race);
                let Some(kind) = Building::from_u16(b.kind as u16) else {
                    crate::audit::miss(|| format!("site of unknown building type {}", b.kind));
                    continue;
                };
                let file = resolve::building_file(race);
                let player = b.owner.saturating_sub(1) & 7;
                let v = site_shown.get(&(b.x, b.y)).copied().unwrap_or(0);
                // Part 2 (the `_BUILD` frame) up to half the work, then part 3 (frame drawn whole as
                // the build layer, the finished image clipped). `> 0xFFFF` (also negative) → 0.
                let clip = |p: i64| if (0..=0xFFFF).contains(&p) { p as u16 } else { 0 };
                let (parts, progress): (&[BuildingPart], u16) = if v >= g.work / 2 {
                    (&[BuildingPart::Construction, BuildingPart::Finished], clip(0xFFFF - g.step as i64 - g.step as i64 * (v - g.work / 2) as i64))
                } else {
                    (&[BuildingPart::Construction], clip(0xFFFF - g.step as i64 * v as i64))
                };
                if progress == 0xFFFF {
                    continue; // BlitBuilding draws nothing.
                }
                let f = files.get(file)?;
                for (n, &part) in parts.iter().enumerate() {
                    let Some(r) = resolve::building_frames(f, kind, part) else {
                        crate::audit::miss(|| format!("site {} ({race:?}) part {part:?}: no image in file {file}", kind.name()));
                        continue;
                    };
                    let last = n + 1 == parts.len();
                    let crop = (last && progress != 0).then_some(progress);
                    let key = SpriteKey { file, image: r.start as u32 };
                    placed.push(Placed { key, x: b.x, y: b.y, layer: Layer::Object, player, wave: None, min_fog: FOG_SOLID, off: PixOff::NONE, anim: None, crop, entity: None, patch: None, vpart: VPart::None });
                }
                stats.buildings += 1;
                continue;
            }
            let race = race_of(b.race);
            let Some(kind) = Building::from_u16(b.kind as u16) else {
                crate::audit::miss(|| format!("unknown building type {}", b.kind));
                continue;
            };
            let file = resolve::building_file(race);
            match resolve::building_frames(files.get(file)?, kind, BuildingPart::Finished) {
                Some(r) => {
                    let key = SpriteKey { file, image: r.start as u32 };
                    let player = b.owner.saturating_sub(1) & 7;
                    placed.push(Placed { key, x: b.x, y: b.y, layer: Layer::Object, player, wave: None, min_fog: FOG_SOLID, off: PixOff::NONE, anim: None, crop: None, entity: None, patch: None, vpart: VPart::None });
                    // The ten patch slots (`BlitBuilding` 0x66fbb0): 0..=3 right after the main
                    // image, 4 deferred, 5..=9 after the settler patches, all at the main image's
                    // position. Their images follow the client's patch state (`set_patches`);
                    // without it the `PERMANENT` ones at frame 0. A ranch's pen is slot 0, set
                    // by the client's patch state (s4-game `patches.rs`, `Patches::pen`).
                    let patches = permanent_patches(files, gamedata, file, race, kind)?;
                    let push_patches = |placed: &mut Vec<Placed>, slots: std::ops::Range<u8>| {
                        for slot in slots {
                            let image = patches.iter().find(|p| p.0 == slot as i32).map_or(u32::MAX, |p| p.1);
                            let off = PixOff { dx: 0, dy: 0, rel: Some(key), at_main: true };
                            let k = SpriteKey { file, image };
                            let layer = if slot == 4 { Layer::Deferred } else { Layer::Object };
                            let patch = (b.id != 0).then_some((b.id, slot));
                            placed.push(Placed { key: k, x: b.x, y: b.y, layer, player, wave: None, min_fog: FOG_SOLID, off, anim: None, crop: None, entity: None, patch, vpart: VPart::None });
                        }
                    };
                    push_patches(&mut placed, 0..5);
                    if !b.inhabitants.is_empty() {
                        military_parts(files, &mut placed, b, race, gamedata, kind, player, |pl| push_patches(pl, 5..10))?;
                    } else {
                        // BlitBuilding: the house worker (sub-sprite 0) between the two patch groups.
                        if let Some(w) = &b.worker {
                            match house_worker(files, b, w, player)? {
                                Some(p) => {
                                    placed.push(p);
                                    stats.settlers += 1;
                                }
                                None => crate::audit::miss(|| format!("house worker type {} job {:?} dir {} in {} ({race:?}): no image", w.kind, w.gfx_job(), w.dir, kind.name())),
                            }
                        }
                        push_patches(&mut placed, 5..10);
                        if let Some(working) = b.flag {
                            placed.extend(player_flag(files, gamedata.building(race, kind), working, b, player, pop.tick)?);
                        }
                    }
                    stats.owners.entry(b.owner).or_insert((0, (b.x, b.y))).0 += 1;
                    tracing::trace!("building {} owner {} race {race:?} file {file} at {},{}", kind.name(), b.owner, b.x, b.y);
                    stats.buildings += 1;
                }
                None => stats.skipped.push(format!("{} ({race:?}) at {},{}", kind.name(), b.x, b.y)),
            }
        }

        // Vehicles: `GetGfxInfo` of `CWheeler` (0x59a5c0, catapults 0x560c60), `CCart` (0x55ba20)
        // and `CShip` (0x590590), see `crate::vehicle_gfx`: file `race + 0x1e`, drawn when
        // `flags & 0x80100 == 0x100`; land vehicles at the end of their node, ships from the
        // deferred list. Raw map vehicles (no state): the type's first job, frame 0.
        let mut vehicle_frames = HashMap::new();
        let mut vehicle_turns = HashMap::new();
        let mut turn_keys = Vec::new();
        for v in &pop.vehicles {
            let race = race_of(v.race);
            let player = v.owner.saturating_sub(1) & 7;
            let (Some(file), Some(kind)) = (resolve::vehicle_file(race), Vehicle::from_u16(v.kind)) else {
                stats.skipped.push(format!("vehicle {} ({race:?}) at {},{}", v.kind, v.x, v.y));
                continue;
            };
            let ship = matches!(kind, Vehicle::WARSHIP | Vehicle::FERRY | Vehicle::TRANSPORTSHIP);
            let layer = if ship { Layer::Ship } else { Layer::Vehicle };
            let f = files.get(file)?;
            let Some(g) = v.vehicle else {
                if let Some(r) = resolve::vehicle_frames(f, kind, v.dir as usize) {
                    placed.push(Placed { key: SpriteKey { file, image: r.start as u32 }, x: v.x, y: v.y, layer, player, wave: None, min_fog: FOG_SOLID, off: PixOff::NONE, anim: None, crop: None, entity: Some(v.id), patch: None, vpart: VPart::None });
                    stats.vehicles += 1;
                }
                continue;
            };
            if v.flags & 0x80100 != 0x100 {
                continue;
            }
            let jobs = f.job_count();
            let bound = |j: usize| if j < jobs { j } else { 0 };
            let count = |j: usize| f.job_frames(j, 2).map_or(0, |r| r.len());
            // +0x26: the ctor (0x593f60) sets the type's first job (`.sil`), the tasks the rest.
            let job26 = v.job as usize;
            vehicle_frames.insert(v.id, count(bound(job26)).min(255) as u8);
            // [job][direction][frame], the frame 0 unless below direction 2's count.
            let range = |job: usize, dir: u8| {
                let job = bound(job);
                let r = f.job_frames(job, dir as usize % 6)?;
                let n = count(job).min(r.len());
                (n > 0).then_some((r.start as u32, n as u16))
            };
            let image = |job: usize, dir: u8, frame: u8| {
                if job >= jobs {
                    crate::audit::miss(|| format!("vehicle {} ({race:?}) job {job} past file {file}'s {jobs}", v.kind));
                }
                range(job, dir).map(|(start, n)| {
                    if frame as u16 >= n {
                        crate::audit::miss(|| format!("vehicle {} ({race:?}) job {job} dir {dir} frame {frame} past {n}", v.kind));
                    }
                    start + if (frame as u16) < n { frame as u32 } else { 0 }
                })
            };
            let cart = !ship && v.kind == crate::vehicle_gfx::CART;
            let empty_cart = cart && !g.loaded;
            let entry = |key: SpriteKey, anim, vpart| Placed { key, x: v.x, y: v.y, layer, player, wave: None, min_fog: FOG_SOLID, off: PixOff::NONE, anim, crop: g.crop, entity: Some(v.id), patch: None, vpart };
            let at = |image: u32| SpriteKey { file, image };
            let mut parts = Vec::new();
            if cart && v.flags & 0x0100_0000 != 0 {
                // A cart under construction: job 0x2e, direction 2, frame 0 (0x55ba20).
                parts.extend(image(crate::vehicle_gfx::CART_BIRTH_JOB, 2, 0).map(|i| entry(at(i), None, VPart::Main)));
            } else if ship && g.step & 8 == 0 {
                // Moving (0x462420): the hull (+0x1c) at frame 0, job +0x18 at frame +0x24 over it.
                parts.extend(image(g.jobs[1] as usize, v.dir, 0).map(|i| entry(at(i), None, VPart::Main)));
                let over = range(g.jobs[0] as usize, v.dir).map(|(start, n)| entry(at(start), Some((start, n, 0, 1)), VPart::Overlay));
                parts.extend(over);
            } else if ship {
                let job = if job26 == g.jobs[0] as usize { g.jobs[1] as usize } else { job26 };
                parts.extend(image(job, v.dir, 0).map(|i| entry(at(i), None, VPart::Main)));
            } else {
                let job = job26 + empty_cart as usize;
                parts.extend(range(job, v.dir).map(|(start, n)| entry(at(start), Some((start, n, 0, 1)), VPart::Main)));
            }
            let Some(main) = parts.first().map(|p| p.key) else {
                stats.skipped.push(format!("vehicle {} ({race:?}) job {job26} at {},{}", v.kind, v.x, v.y));
                continue;
            };
            let turns: [SpriteKey; 16] = std::array::from_fn(|i| {
                let (job, dir, frame) = crate::vehicle_gfx::turn_image(i as u8, (g.jobs[1], g.jobs[2]), empty_cart);
                image(job as usize, dir, frame).map_or(main, at)
            });
            turn_keys.extend(turns);
            vehicle_turns.insert(v.id, turns);
            placed.extend(parts);
            stats.vehicles += 1;
        }

        // Settlers (outside buildings): `CSettler::GetGfxInfo` → `GetSettlerGfxInfo` (0x461d00):
        // file 20 + race, the settler's gfx job and direction, its frame (frame 0 when out of
        // range). Raw map units draw their type's walk job.
        for s in pop.settlers.iter().filter(|s| s.drawn()) {
            let race = race_of(s.race);
            let player = s.owner.saturating_sub(1) & 7;
            let Some(st) = Settler::from_u16(s.kind).filter(|_| s.kind < 67) else {
                crate::audit::miss(|| format!("settler type {} not drawn (no type below 67)", s.kind));
                continue;
            };
            let file = resolve::settler_file(race);
            let f = files.get(file)?;
            let job = match s.gfx_job() {
                Some(j) => Some(j as usize),
                None => resolve::settler_job(f, st),
            };
            match job.and_then(|j| resolve::settler_frames(f, j, s.dir as usize % 6)).filter(|r| !r.is_empty()) {
                Some(r) => {
                    let n = r.len() as u16;
                    if s.frame as u16 >= n {
                        crate::audit::miss(|| format!("{} ({race:?}) job {:?} dir {} frame {} past {n}", st.name(), job, s.dir % 6, s.frame));
                    }
                    let frame = if (s.frame as u16) < n { s.frame as u16 } else { 0 };
                    let key = SpriteKey { file, image: r.start as u32 + frame as u32 };
                    let anim = Some((r.start as u32, n, frame, 1));
                    placed.push(Placed { key, x: s.x, y: s.y, layer: Layer::Settler, player, wave: None, min_fog: FOG_SOLID, off: PixOff::NONE, anim, crop: None, entity: Some(s.id), patch: None, vpart: VPart::None });
                    stats.owners.entry(s.owner).or_insert((0, (s.x, s.y))).0 += 1;
                    stats.settlers += 1;
                }
                None => stats.skipped.push(format!("{} ({race:?}) at {},{}", st.name(), s.x, s.y)),
            }
        }

        // Land animals: `CAnimal::GetGfxInfo` (0x52bb30) → 0x4624e0: `Gfx\8.gfx`, image [job
        // +0x26][direction +0x40][frame +0x24] (a job past the file's count is 0, a frame past
        // the frame count of direction 2 is 0), drawn when `flags & 0x80100 == 0x100`, map-object
        // fog above 59; frame and glide from the draw-time update ([`crate::animate`]).
        for a in pop.animals.iter().filter(|a| a.flags & 0x80100 == 0x100) {
            let f = files.get(ANIMAL_FILE)?;
            let count = f.job_frames(a.job as usize, 2).map_or(0, |r| r.len());
            match f.job_frames(a.job as usize, a.dir as usize % 6).filter(|r| !r.is_empty()) {
                Some(r) => {
                    let n = (r.len().min(count.max(1))) as u16;
                    let key = SpriteKey { file: ANIMAL_FILE, image: r.start as u32 };
                    placed.push(Placed { key, x: a.x, y: a.y, layer: Layer::Settler, player: 0, wave: None, min_fog: FOG_SOLID, off: PixOff::NONE, anim: Some((r.start as u32, n, 0, 1)), crop: None, entity: Some(a.id), patch: None, vpart: VPart::None });
                    stats.animals += 1;
                }
                None => stats.skipped.push(format!("animal {} job {} at {},{}", a.kind, a.job, a.x, a.y)),
            }
        }

        // Piles.
        for p in &pop.piles {
            let Some(good) = Good::from_u16(p.good as u16) else {
                crate::audit::miss(|| format!("pile of unknown good {}", p.good));
                continue;
            };
            // [inf] An empty pile (a construction site's material place before delivery) shows nothing.
            if p.amount == 0 {
                continue;
            }
            let f = files.get(resolve::PILE_FILE)?;
            let amount = (p.amount as usize).clamp(1, f.direction_count());
            match resolve::pile_frames(f, good, amount) {
                Some(r) => {
                    placed.push(Placed { key: SpriteKey { file: 3, image: r.start as u32 }, x: p.x, y: p.y, layer: Layer::Object, player: 0, wave: None, min_fog: FOG_SOLID, off: PixOff::NONE, anim: None, crop: None, entity: None, patch: None, vpart: VPart::None });
                    stats.piles += 1;
                }
                None => stats.skipped.push(format!("pile {} at {},{}", good.name(), p.x, p.y)),
            }
        }

        // Original draw order: rows, then x, then layer at the node; ships after everything. Stable
        // sort keeps the object-layer order (deco before building before pile) of equal keys.
        drop(t);
        let t = crate::perf::span("place: sort");
        placed.sort_by_key(draw_order);
        drop(t);
        let t = crate::perf::span("place: add_sprites");

        // Every image needed, including all frames of the animated cycles (the decos' only when
        // they changed). (A patch slot without a `PERMANENT` image holds the key `u32::MAX`:
        // nothing.)
        let decos = self.decos.as_ref().map_or(&[][..], |c| &c.placed[..]);
        let fresh = || placed.iter().chain(if decos_new { decos } else { &[] });
        let mut keys: Vec<SpriteKey> = fresh().map(|p| p.key).filter(|k| k.image != u32::MAX).collect();
        let done = &self.cycles_done;
        let mut cycles: Vec<(u32, u32, u16)> = fresh().filter_map(|p| p.anim.map(|a| (p.key.file, a.0, a.1))).filter(|c| !done.contains(c)).collect();
        cycles.sort();
        cycles.dedup();
        for &(file, start, n) in &cycles {
            keys.extend((start..start + n as u32).map(|image| SpriteKey { file, image }));
        }
        keys.extend(turn_keys);
        let decos_animated = decos.iter().any(|p| p.anim.is_some());
        self.add_sprites(keys)?;
        self.cycles_done.extend(cycles);
        drop(t);
        self.vehicle_frames = vehicle_frames;
        self.vehicle_turns = vehicle_turns;

        for s in &stats.skipped {
            crate::audit::miss(|| format!("unresolved {}", s.rsplit_once(" at ").map_or(&s[..], |(a, _)| a)));
        }
        stats.waves = self.stats.waves;
        stats.border_stones = self.stats.border_stones;
        stats.unique_sprites = self.slots.len();
        stats.atlas_layers = self.atlas.len();
        self.stats = stats;
        self.animated = decos_animated || placed.iter().any(|p| p.anim.is_some() && p.entity.is_none());
        // The units' entries; the decos' stay in `decos` (both sorted, merged when drawn).
        self.placed = placed;
        Ok(())
    }

    /// Decode and pack the images of `keys` that are not in the atlas yet (shelf packing into R8
    /// layers, tallest first, 1 px gutter). Returns how many were added.
    fn add_sprites(&mut self, mut keys: Vec<SpriteKey>) -> anyhow::Result<usize> {
        let t = crate::perf::span("add: filter");
        keys.retain(|k| !self.slots.contains_key(k));
        keys.sort();
        keys.dedup();
        drop(t);
        if keys.is_empty() {
            return Ok(0);
        }
        crate::perf::add_n("add: new images", keys.len() as u32);
        let t = crate::perf::span("add: decode");
        for k in &keys {
            self.files.get(k.file)?;
        }
        // Decode every new sprite in parallel (scoped threads, one GfxFile set shared); the
        // composed logos stand in for file 1.
        let logo = |k: &SpriteKey| if k.file == LOGO_FILE { self.logos.get(&k.image) } else { None };
        let from_files: Vec<SpriteKey> = keys.iter().filter(|k| logo(k).is_none()).copied().collect();
        let mut from_files = decode_parallel(&self.files.open, &from_files)?.into_iter();
        let decoded: Vec<IndexedImage> = keys
            .iter()
            .map(|k| match logo(k) {
                Some(l) => l.0.clone(),
                None => from_files.next().expect("one decoded image per file key"),
            })
            .collect();
        drop(t);
        let _t = crate::perf::span("add: pack");
        let mut order: Vec<usize> = (0..keys.len()).collect();
        order.sort_by_key(|&i| std::cmp::Reverse(decoded[i].header.height));
        let (mut cx, mut cy, mut shelf) = self.shelf;
        let mut added = 0;
        for i in order {
            let key = keys[i];
            let img = &decoded[i];
            let f = &self.files.open[&key.file];
            if crate::audit::recording() && key.file != LOGO_FILE && opaque_black(img, f, key.image as usize) {
                crate::audit::miss(|| format!("opaque black image drawn: file {} image {} ({}x{})", key.file, key.image, img.header.width, img.header.height));
            }
            let (w, h) = (img.header.width as u32, img.header.height as u32);
            if w == 0 || h == 0 || w > SPRITE_ATLAS || h > SPRITE_ATLAS {
                continue;
            }
            if cx + w + 1 > SPRITE_ATLAS {
                cx = 0;
                cy += shelf;
                shelf = 0;
            }
            if self.atlas.is_empty() || cy + h + 1 > SPRITE_ATLAS {
                self.atlas.push(vec![0u8; (SPRITE_ATLAS * SPRITE_ATLAS) as usize]);
                cx = 0;
                cy = 0;
                shelf = 0;
            }
            let layer = self.atlas.len() as u32 - 1;
            let dst = self.atlas.last_mut().unwrap();
            for row in 0..h {
                let o = ((cy + row) * SPRITE_ATLAS + cx) as usize;
                dst[o..o + w as usize].copy_from_slice(&img.pixels[(row * w) as usize..((row + 1) * w) as usize]);
            }
            let rows = self.dirty.atlas.entry(layer).or_insert((cy, cy + h));
            *rows = (rows.0.min(cy), rows.1.max(cy + h));

            let logo = if key.file == LOGO_FILE { self.logos.get(&key.image) } else { None };
            // A logo's own row: keyed past every file palette offset.
            let off = if logo.is_some() { LOGO_PALETTE | key.image } else { f.palette_offset(key.image as usize).unwrap_or(20) };
            let pal_row = match self.pal_index.get(&(key.file, off)) {
                Some(&r) => r,
                None => {
                    let r = self.palette_rows;
                    self.palette_rows += 1;
                    self.pal_index.insert((key.file, off), r);
                    let row = match logo {
                        Some(l) => l.1.clone(),
                        None => palette_row(f, key.image as usize, off)?,
                    };
                    self.palettes.extend_from_slice(&row);
                    self.dirty.palettes = true;
                    r
                }
            };
            self.slots.insert(key, AtlasSlot {
                x: cx,
                y: cy,
                layer,
                w,
                h,
                left: img.header.left as i32,
                top: img.header.top as i32,
                half: img.header.half_scale(),
                pal_row,
                team: f.has_team_colour(),
            });
            cx += w + 1;
            shelf = shelf.max(h + 1);
            added += 1;
        }
        self.shelf = (cx, cy, shelf);
        Ok(added)
    }

    /// Re-place sprites after terrain height changes.
    pub fn update_heights(&mut self, h: &dyn Fn(usize, usize) -> u8) {
        for y in 0..self.size {
            for x in 0..self.size {
                self.heights[y * self.size + x] = h(x, y);
            }
        }
        self.stale = true;
    }

    /// Advance the wave animation to game tick `tick`. Returns whether anything changed.
    pub fn set_tick(&mut self, tick: u64) -> bool {
        if tick == self.tick {
            return false;
        }
        self.tick = tick;
        // `sub_E33B0`: an effect whose age reached its duration is removed.
        let had_effects = !self.effects.is_empty();
        self.effects.retain(|e| tick < e.start + e.life);
        if self.waves.is_empty() && !self.animated && !had_effects {
            return false;
        }
        self.stale = true;
        true
    }

    /// The buildings' patch slots as the client's patch state has them: per building its race
    /// and per slot `(job, frame)`. The role hands each slot to the gfx info through 0x4621c0: a
    /// job past the race's building file's .dil count, or a frame past the job's frames, is 0.
    /// A slot with job 0 shows nothing; a building without state shows its `PERMANENT`
    /// patches at frame 0.
    pub fn set_patches(&mut self, shown: &ShownPatches) -> anyhow::Result<()> {
        let mut state = HashMap::with_capacity(shown.len());
        let mut keys = Vec::new();
        for (&id, &(race, slots)) in shown {
            let file = resolve::building_file(Race::from_u16(race as u16).unwrap_or(Race::ROMAN));
            let f = self.files.get(file)?;
            let mut out = [None; 10];
            for (n, &(job, frame)) in slots.iter().enumerate() {
                if job == 0 {
                    continue;
                }
                let Some(r) = f.dil_frames(job as usize).filter(|r| !r.is_empty()) else {
                    crate::audit::miss(|| format!("patch job {job} (race {race}): no .dil in file {file}"));
                    continue;
                };
                if frame as usize >= r.len() {
                    crate::audit::miss(|| format!("patch job {job} (race {race}, file {file}) frame {frame} past {}", r.len()));
                }
                let image = r.start as u32 + if (frame as usize) < r.len() { frame } else { 0 };
                let k = SpriteKey { file, image };
                keys.push(k);
                out[n] = Some(k);
            }
            state.insert(id, out);
        }
        if state == self.patch_state {
            return Ok(());
        }
        self.add_sprites(keys)?;
        self.patch_state = state;
        self.stale = true;
        Ok(())
    }

    /// The frame count of `file`'s job `job` in direction `dir` (`GetJobFrameCount` 0x461fe0), 0
    /// when it has none.
    pub fn job_frame_count(&mut self, file: u32, job: usize, dir: usize) -> usize {
        self.files.get(file).ok().and_then(|f| f.job_frames(job, dir)).map_or(0, |r| r.len())
    }

    /// The client's ambient effects as they are drawn now (fish, butterflies, birds, seagulls).
    pub fn set_ambient(&mut self, shown: &[AmbientSprite]) -> anyhow::Result<()> {
        let mut placed = Vec::with_capacity(shown.len());
        for s in shown {
            match *s {
                AmbientSprite::Fish { x, y, job, dir, frame } => {
                    let f = self.files.get(ANIMAL_FILE)?;
                    let count = f.job_frames(job as usize, 2).map_or(0, |r| r.len());
                    let Some(r) = f.job_frames(job as usize, dir as usize).filter(|r| !r.is_empty()) else {
                        crate::audit::miss(|| format!("fish job {job} dir {dir}: no image in file {ANIMAL_FILE}"));
                        continue;
                    };
                    if frame as usize >= count.min(r.len()) {
                        crate::audit::miss(|| format!("fish job {job} dir {dir} frame {frame} past {}", count.min(r.len())));
                    }
                    let frame = if (frame as usize) < count.min(r.len()) { frame } else { 0 };
                    let key = SpriteKey { file: ANIMAL_FILE, image: r.start as u32 + frame };
                    placed.push(Placed { key, x, y, layer: Layer::Effect, player: 0, wave: None, min_fog: FOG_SOLID, off: PixOff::NONE, anim: None, crop: None, entity: None, patch: None, vpart: VPart::None });
                }
                AmbientSprite::Effect { gfx, dir, frame, x, y, px } => {
                    if (x as usize) >= self.size || (y as usize) >= self.heights.len() / self.size.max(1) {
                        continue;
                    }
                    let (file, job) = if gfx < 100 { (6, gfx as usize) } else { (37, gfx as usize - 100) };
                    let f = self.files.get(file)?;
                    let count = f.job_frames(job, 0).map_or(0, |r| r.len());
                    let Some(r) = f.job_frames(job, dir as usize).filter(|r| !r.is_empty()) else {
                        crate::audit::miss(|| format!("effect gfx {gfx} dir {dir}: no image (file {file} job {job})"));
                        continue;
                    };
                    if frame as usize >= count.min(r.len()) {
                        crate::audit::miss(|| format!("effect gfx {gfx} (file {file} job {job}) dir {dir} frame {frame} past {}", count.min(r.len())));
                    }
                    let frame = if (frame as usize) < count.min(r.len()) { frame } else { 0 };
                    let key = SpriteKey { file, image: r.start as u32 + frame };
                    let h = self.heights.get(y as usize * self.size + x as usize).copied().unwrap_or(0);
                    let (ax, ay) = anchor_fixed(x as i32, y as i32, h);
                    let off = PixOff { dx: (px.0 - (ax >> 16)) as i16, dy: (px.1 - (ay >> 16)) as i16, rel: None, at_main: false };
                    placed.push(Placed { key, x, y, layer: Layer::Deferred, player: 0, wave: None, min_fog: FOG_SOLID, off, anim: None, crop: None, entity: None, patch: None, vpart: VPart::None });
                }
            }
        }
        let same = placed.len() == self.ambient.len() && placed.iter().zip(&self.ambient).all(|(a, b)| (a.key, a.x, a.y, a.off.dx, a.off.dy) == (b.key, b.x, b.y, b.off.dx, b.off.dy));
        if same {
            return Ok(());
        }
        self.add_sprites(placed.iter().map(|p| p.key).collect())?;
        self.ambient = placed;
        self.stale = true;
        Ok(())
    }

    /// Draw entities as `poses` say (displacement from their node anchor in world px, the
    /// settler's frame): the draw-time state between two simulation ticks
    /// ([`crate::animate`]). An entity without a pose is drawn on its node with the frame it was
    /// placed with.
    pub fn set_unit_poses(&mut self, poses: HashMap<u16, Pose>) {
        if poses == self.poses {
            return;
        }
        self.poses = poses;
        self.stale = true;
    }

    /// New per-node fog values (`CalcCurrentFog`, with each row's fade step).
    pub fn set_fog(&mut self, fog: Vec<u8>) {
        if fog == self.fog {
            return;
        }
        let _t = crate::perf::span("fog: regen_waves");
        // A wave depends on `fog > 58` of nodes in its own row and up to two rows below
        // (waves.rs): only the rows above where that changed are generated again.
        let seen = |a: &[u8], b: &[u8]| a.iter().zip(b).any(|(&x, &y)| (x > 58) != (y > 58));
        let rows = (fog.len() == self.fog.len()).then(|| changed_rows(self.size, &self.fog, &fog, seen));
        self.fog = fog;
        match rows {
            Some(rows) => {
                for (a, b) in crate::terrain::row_runs(&rows, 3) {
                    self.regen_wave_rows(a.saturating_sub(2), b);
                }
            }
            None => self.regen_waves(),
        }
        self.stale = true;
    }

    /// What grows with play, for leak checks (`s4-game --soak`): the atlas, the palettes and the
    /// per-entity / per-position maps.
    pub fn cache_sizes(&self) -> Vec<(&'static str, usize)> {
        vec![
            ("atlas layers", self.atlas.len()),
            ("atlas slots", self.slots.len()),
            ("palette rows", self.palette_rows as usize),
            ("pal index", self.pal_index.len()),
            ("gfx files open", self.files.open.len()),
            ("poses", self.poses.len()),
            ("vehicle frames", self.vehicle_frames.len()),
            ("vehicle turns", self.vehicle_turns.len()),
            ("site shown", self.site_shown.len()),
            ("patch state", self.patch_state.len()),
            ("cycles done", self.cycles_done.len()),
            ("effects", self.effects.len()),
            ("logos", self.logos.len()),
            ("instances", self.instances.len()),
        ]
    }

    /// The per-node fog values ([`SpriteScene::set_fog`]).
    pub fn fog(&self) -> &[u8] {
        &self.fog
    }

    /// Territory owner per node.
    pub fn owners(&self) -> &[u8] {
        &self.owner
    }

    /// The border-stone bit (release 0x5b37b0, `CTiling::CalculateBorderstoneBit` [label]): a node
    /// inside the 1-node margin gets byte 3's 0x80 if its owner is non-zero and one of its six
    /// neighbours (offsets 0x142ed70..0x142ed88) has another owner and ground type > 8 (not water).
    pub fn border_bit(&self, x: usize, y: usize) -> bool {
        let s = self.size;
        if x < 1 || y < 1 || x + 1 >= s || y + 1 >= s {
            return false;
        }
        let i = y * s + x;
        let own = self.owner[i];
        own > 0
            && [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (-1, -1)].iter().any(|&(dx, dy)| {
                let j = ((y as isize + dy) as usize) * s + (x as isize + dx) as usize;
                self.owner[j] != own && self.ground[j] > 8
            })
    }

    /// The per-node entries that depend on fog or territory: border stones and waves (waves.rs).
    fn regen_waves(&mut self) {
        self.regen_wave_rows(0, usize::MAX);
    }

    /// [`SpriteScene::regen_waves`] for rows `y0..=y1` only (the entries are in node order).
    fn regen_wave_rows(&mut self, y0: usize, y1: usize) {
        let size = self.size;
        let y1 = y1.min((self.heights.len() / size).saturating_sub(1));
        let mut waves = Vec::new();
        let mut stones = 0;
        for y in y0..=y1 {
            for x in 0..size {
                // RenderObjectLayer: `(byte3 & 0x80) && fog > 59` → g_pfBlitBorderstone(fog, x, y −
                // hoff, GetOwnerID). (The editor-only ground > 8 condition does not apply in game.)
                let own = self.owner[y * size + x] as usize;
                if own > 0 && own <= self.border_frames.len() && self.border_bit(x, y) {
                    let key = SpriteKey { file: 5, image: (self.border_frames.start + own - 1) as u32 };
                    waves.push(Placed { key, x: x as u16, y: y as u16, layer: Layer::Border, player: 0, wave: None, min_fog: FOG_SOLID, off: PixOff::NONE, anim: None, crop: None, entity: None, patch: None, vpart: VPart::None });
                    stones += 1;
                }
                let w = wave_at(size, x, y, |i| self.ground[i], |i| self.gradient[i], |i| self.fog[i] > 58);
                let Some(w) = w else { continue };
                let r = &self.wave_frames[w.slot as usize];
                if r.is_empty() {
                    continue;
                }
                let key = SpriteKey { file: 5, image: r.start as u32 };
                waves.push(Placed { key, x: x as u16, y: y as u16, layer: Layer::Wave, player: 0, wave: Some(w), min_fog: 58, off: PixOff::NONE, anim: None, crop: None, entity: None, patch: None, vpart: VPart::None });
            }
        }
        let a = self.waves.partition_point(|p| (p.y as usize) < y0);
        let b = self.waves.partition_point(|p| (p.y as usize) <= y1);
        let old_stones = self.waves[a..b].iter().filter(|p| p.layer == Layer::Border).count();
        self.stats.waves = self.stats.waves + (waves.len() - stones) - (b - a - old_stones);
        self.stats.border_stones = self.stats.border_stones + stones - old_stones;
        self.waves.splice(a..b, waves);
    }

    fn rebuild_instances(&mut self) {
        let _t = crate::perf::span("sprites: rebuild_instances");
        self.instances.clear();
        // The effects running at this tick: `RenderObjectLayer` (0x660070) draws them after each
        // row's objects (`CEffects` vtable +4 / +8: the effects of the row, `sub_E33B0` /
        // `sub_E3580`) with the object blitter at the node's
        // `ConvertMapPositionToWorldScreenOffsets` (0x65b130: `24x − 12y`, `12y − round(h ·
        // 1.2375)`, the objects' anchor), only where the fog is above 59 (`0x3b <`).
        // The selection entries drawn in the rows: building brackets, vehicle icons.
        let sel_objects = self.selection.iter().filter(|p| p.layer != Layer::Marker);
        let mut extra: Vec<Placed> = Vec::new();
        if !(self.effects.is_empty() && self.ambient.is_empty() && sel_objects.clone().next().is_none()) {
            // A selected building's brackets and a vehicle's icons right after its own entries.
            extra.extend(sel_objects.cloned());
            extra.extend(self.ambient.iter().copied());
            let (size, heights) = (self.size, &self.heights);
            extra.extend(self.effects.iter().filter(|e| self.tick >= e.start && self.tick < e.start + e.life).map(|e| {
                let age = self.tick - e.start;
                let f = (age % e.frames as u64) as u32;
                let image = e.first + if f < e.images as u32 { f } else { 0 };
                let off = match e.fly {
                    Some(fly) => {
                        let h = heights.get(e.y as usize * size + e.x as usize).copied().unwrap_or(0);
                        let (ax, ay) = anchor_fixed(e.x as i32, e.y as i32, h);
                        let (px, py) = fly.at(age as f32);
                        PixOff { dx: (px as i32 - (ax >> 16)) as i16, dy: (py as i32 - (ay >> 16)) as i16, rel: None, at_main: false }
                    }
                    None => PixOff::NONE,
                };
                let layer = if e.deferred { Layer::Deferred } else { Layer::Effect };
                Placed { key: SpriteKey { file: e.file, image }, x: e.x, y: e.y, layer, player: e.player, wave: None, min_fog: FOG_SOLID, off, anim: None, crop: None, entity: None, patch: None, vpart: VPart::None }
            }));
            extra.sort_by_key(draw_order);
        }
        let order = draw_order;
        // The decos' and the units' entries (each sorted by `place`), the decos first on equal
        // keys, then the others after them on equal keys: the order a stable sort of all three
        // in that order gives.
        let decos = self.decos.as_ref().map_or(&[][..], |c| &c.placed[..]);
        let (mut d, mut u) = (decos.iter().peekable(), self.placed.iter().peekable());
        let placed = std::iter::from_fn(move || match (d.peek(), u.peek()) {
            (Some(x), Some(y)) if order(y) < order(x) => u.next(),
            (Some(_), _) => d.next(),
            (None, _) => u.next(),
        });
        let (mut p, mut e) = (placed.peekable(), extra.iter().peekable());
        let rows = std::iter::from_fn(move || match (p.peek(), e.peek()) {
            (Some(x), Some(y)) if order(y) < order(x) => e.next(),
            (Some(_), _) => p.next(),
            (None, _) => e.next(),
        });
        // Merge them and the waves in draw order (a wave is never a ship).
        let (mut a, mut b) = (rows.peekable(), self.waves.iter().peekable());
        // RenderObjectLayer: the cursor building, then the settlers' selection and health icons
        // (`g_iSettlerSelections`, `g_iSettlerFitness`), then the helper icons.
        let mut c = self.ghost.iter().chain(self.selection.iter().filter(|p| p.layer == Layer::Marker)).chain(&self.markers);
        loop {
            let p = match (a.peek(), b.peek()) {
                (Some(x), Some(y)) if order(y) < order(x) => b.next(),
                (Some(_), _) => a.next(),
                (None, Some(_)) => b.next(),
                (None, None) => c.next(),
            };
            let Some(p) = p else { break };
            let node = p.y as usize * self.size + p.x as usize;
            // Accessory icons are blitted without the fog (`ZoomBobNormal`, no shading).
            let fog = if p.layer == Layer::Marker { 255 } else { self.fog.get(node).copied().unwrap_or(255) };
            if fog <= p.min_fog {
                continue;
            }
            // The vehicle blitter (0x670be0) passes its shading (`param_1 · 0x10101`) to the body's
            // blit 0x671660 only; the icons go through 0x671830 `(texture, object type 0x40, w, h,
            // x, y)`, which takes no shading and queues them with flag 0 (0x671c70 / 0x6713e0):
            // unshaded like the icon pass.
            let fog = if p.key.file == ICON_FILE && matches!(p.layer, Layer::Vehicle | Layer::Ship) { 255 } else { fog };
            let pose = p.entity.and_then(|e| self.poses.get(&e)).copied().unwrap_or_default();
            let key_now = match p.patch.and_then(|(id, slot)| self.patch_state.get(&id).map(|s| s[slot as usize])) {
                Some(None) => continue,
                Some(Some(k)) => k,
                None => p.key,
            };
            // A turning vehicle shows its turn sprite and no second layer (0x594b50 / 0x55bbd0).
            let turn_key = match (p.vpart, pose.turn, p.entity) {
                (VPart::Overlay, Some(_), _) => continue,
                (VPart::Main, Some(i), Some(e)) => self.vehicle_turns.get(&e).map(|t| t[i as usize & 15]),
                _ => None,
            };
            let key_now = match (p.anim, p.entity, pose.frame) {
                _ if turn_key.is_some() => turn_key.unwrap_or(key_now),
                // `GetSettlerGfxInfo`: a frame past the job's end shows frame 0.
                (Some((start, n, _, _)), Some(_), Some(f)) => {
                    if f as u16 >= n {
                        crate::audit::miss(|| format!("drawn frame {f} past {n} (file {} images {start}..)", p.key.file));
                    }
                    SpriteKey { file: p.key.file, image: start + if (f as u16) < n { f as u32 } else { 0 } }
                }
                (Some((start, n, phase, mode)), None, _) => SpriteKey { file: p.key.file, image: start + deco_frame(n, phase, mode, self.tick) as u32 },
                _ => key_now,
            };
            let (key, x16, y16, shift, k) = match p.wave {
                Some(w) => {
                    // BlitWave: frame (tick + row) % frames, anchor without height, offsets unshifted.
                    let r = &self.wave_frames[w.slot as usize];
                    let n = (r.end - r.start) as u64;
                    let image = r.start as u64 + (self.tick + p.y as u64) % n;
                    let x16 = (24 * p.x as i32 - 12 * p.y as i32 + 24 * w.dx as i32) << 16;
                    let y16 = (12 * p.y as i32 - 12) << 16;
                    (SpriteKey { file: p.key.file, image: image as u32 }, x16, y16, 0, 1.0)
                }
                None => {
                    let Some(s) = self.slots.get(&key_now) else {
                        // `u32::MAX`: a patch slot without a `PERMANENT` image, nothing to draw.
                        if key_now.image != u32::MAX {
                            crate::audit::miss(|| format!("no image file {} image {}", key_now.file, key_now.image));
                        }
                        continue;
                    };
                    let h = self.heights.get(node).copied().unwrap_or(0);
                    let (x16, y16) = anchor_fixed(p.x as i32, p.y as i32, h);
                    // `BlitObject` (0x2f6a540): `(a2 - (left << 16)*zoom) >> 16`, halved for flag 0x10.
                    let (shift, k) = if s.half { (15, 0.5) } else { (16, 1.0) };
                    (key_now, x16, y16, shift, k)
                }
            };
            let Some(s) = self.slots.get(&key) else { continue };
            let (mut x16, mut y16) = (x16 + ((p.off.dx as i32) << 16), y16 + ((p.off.dy as i32) << 16));
            if let Some(r) = p.off.rel.and_then(|k| self.slots.get(&k)) {
                x16 -= r.left << 16;
                y16 -= r.top << 16;
            }
            let (own_left, own_top) = if p.off.at_main { (0, 0) } else { (s.left, s.top) };
            let left = (x16 - (own_left << shift)) >> 16;
            let top = (y16 - (own_top << shift)) >> 16;
            let d = (pose.dx, pose.dy);
            // Construction clip: screen rows above `progress · h / 0xFFFF − 10` are not drawn.
            let h = s.h as f32 * k;
            let cut = p.crop.map_or(0, |c| (c as i32 * h as i32 / 0xFFFF - 10).clamp(0, h as i32));
            let texels = (cut as f32 / k) as u32;
            let ui = p.layer == Layer::Marker || p.key.file == ICON_FILE;
            self.instances.push(SpriteInstance {
                pos: [left as f32 + d.0, (top + cut) as f32 + d.1],
                size: [s.w as f32 * k, h - cut as f32],
                origin: s.x | ((s.y + texels) << 16),
                layer: s.layer,
                pal_row: s.pal_row,
                flags: (s.team as u32) | ((ui as u32) * FLAG_UI) | ((p.player as u32 & 7) << 8) | (fog as u32) << 12 | (s.w << 20),
            });
        }
    }

    /// The image an instance shows, `(file, image)` (diagnostics: atlas origin and layer back to
    /// its key).
    pub fn slot_key(&self, origin: u32, layer: u32) -> Option<(u32, u32)> {
        self.slots.iter().find(|(_, s)| s.layer == layer && s.x == origin & 0xffff && s.y <= origin >> 16 && origin >> 16 < s.y + s.h.max(1)).map(|(k, _)| (k.file, k.image))
    }

    /// All instances in draw order (world space).
    pub fn instances(&mut self) -> &[SpriteInstance] {
        if self.stale {
            self.rebuild_instances();
            self.stale = false;
        }
        &self.instances
    }

    /// The effects drawn at the current tick: `(image, node, player slot)`.
    #[cfg(test)]
    fn drawn_effects(&self) -> Vec<(SpriteKey, (u16, u16), u8)> {
        let t = self.tick;
        self.effects
            .iter()
            .filter(|e| t >= e.start && t < e.start + e.life)
            .map(|e| {
                let f = ((t - e.start) % e.frames as u64) as u32;
                (SpriteKey { file: e.file, image: e.first + if f < e.images as u32 { f } else { 0 } }, (e.x, e.y), e.player)
            })
            .collect()
    }
}

#[cfg(test)]
mod deco_anim_tests {
    use super::*;

    /// One frame a tick: a loop wraps (`(frame + Δtick) % frames`, 0x593b80); a plain object's
    /// ping-pong climbs to the last frame and falls back to 0 (0x562700); the phase keeps the
    /// frame a deco showed at its anchor tick.
    #[test]
    fn loop_and_ping_pong_one_frame_a_tick() {
        let (l, pp) = (DecoAnim::Loop as u8, DecoAnim::PingPong as u8);
        let seq = |mode, phase| (0..10).map(|t| deco_frame(4, phase, mode, t)).collect::<Vec<_>>();
        assert_eq!(seq(l, 1), [1, 2, 3, 0, 1, 2, 3, 0, 1, 2]);
        assert_eq!(seq(pp, 1), [1, 2, 3, 2, 1, 0, 1, 2, 3, 2]);
        for mode in [l, pp] {
            for (frame, anchor) in [(0, 0), (2, 1000), (3, 77), (1, 5)] {
                assert_eq!(deco_frame(4, deco_phase(4, frame, mode, anchor), mode, anchor as u64), frame, "mode {mode} frame {frame} at {anchor}");
            }
        }
        // A single frame never moves.
        assert_eq!(deco_frame(1, deco_phase(1, 0, pp, 9), pp, 123), 0);
    }
}

#[cfg(test)]
mod tribe_effect_tests {
    use super::*;

    /// Bug B4: the geologist's rich sign plays effect 0 (0x57b308 `AddEffect(0, 0x26, ..)`), a
    /// sound only. The release draws no record whose effect byte is 0 (0x4e33b0); job 0 of file
    /// 6 is a 60×60 opaque black block, and the audit flags it if anything draws it again.
    #[test]
    fn effect_zero_is_a_sound_without_a_picture() {
        let map = s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map");
        if !map.exists() {
            return;
        }
        let mut scene = crate::scene::Scene::load(Some(&map.to_string_lossy()), true, false).unwrap();
        let s = scene.sprites.as_mut().unwrap();
        s.set_tick(100);
        s.add_effect(0, 40, 50, 100).unwrap();
        s.add_tribe_effect(0, 40, 50, 1, Race::ROMAN, 100).unwrap();
        assert_eq!(s.add_lasting_effect(0, 40, 50, 100).unwrap(), 0);
        assert!(s.drawn_effects().is_empty());
        s.add_effect(0x3e, 40, 50, 100).unwrap();
        assert_eq!(s.drawn_effects().len(), 1);
        let f = s.files.get(6).unwrap();
        assert!(opaque_black(&f.decode_indexed(0).unwrap(), f, 0), "file 6 job 0 is the black block");
        let r = f.job_frames(0x3e, 0).unwrap();
        assert!(!opaque_black(&f.decode_indexed(r.start).unwrap(), f, r.start));
    }

    /// A death effect (0x4e2260 / 0x4e2d00) of player 2 (Maya): direction 2 of job 0x14f in the
    /// Maya settler file, one frame per tick in player slot 1, gone when the frames are used up;
    /// the Trojan file's has 13 frames, the others 12.
    #[test]
    fn death_effect_runs_direction_two_in_the_players_colour() {
        let map = s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map");
        if !map.exists() {
            return;
        }
        let mut scene = crate::scene::Scene::load(Some(&map.to_string_lossy()), true, false).unwrap();
        let s = scene.sprites.as_mut().unwrap();
        for (race, n) in [(Race::ROMAN, 12), (Race::VIKING, 12), (Race::MAYA, 12), (Race::DARK, 12), (Race::TROJAN, 13)] {
            let file = resolve::settler_file(race);
            for job in [0x14d, 0x14e, 0x14f] {
                assert_eq!(s.job_frame_count(file, job, 0), 0, "{race:?} job {job:#x}");
                assert_eq!(s.job_frame_count(file, job, 2), n, "{race:?} job {job:#x}");
            }
        }
        let first = s.files.get(22).unwrap().job_frames(0x14f, 2).unwrap().start as u32;
        s.add_tribe_effect(3, 40, 50, 2, Race::MAYA, 100).unwrap();
        for age in 0..12 {
            s.set_tick(100 + age);
            assert_eq!(s.drawn_effects(), vec![(SpriteKey { file: 22, image: first + age as u32 }, (40, 50), 1)], "age {age}");
        }
        s.set_tick(112);
        assert!(s.drawn_effects().is_empty());
    }
}

/// The `PERMANENT` patches of buildingInfo.xml (front walls, doors, castle towers) as (slot, image).
/// `IBuildingRole` keeps them in the building's FX list (`SBuildingFX`: .dil index + frame, frame 0
/// here); `GetBuildingGfxInfo` resolves them through the race's building file. `BlitBuilding`
/// (0x66fbb0) draws slots 0..=3 right after the main image and 5..=9 after the settler patches,
/// all at the main image's position; slot 4 is drawn from the deferred list ([`Layer::Deferred`]).
fn permanent_patches(
    files: &mut Files,
    gamedata: &s4_formats::gamedata::GameData,
    file: u32,
    race: Race,
    kind: Building,
) -> anyhow::Result<Vec<(i32, u32)>> {
    use s4_formats::gamedata::PatchType;
    let Some(info) = gamedata.building(race, kind) else { return Ok(Vec::new()) };
    let f = files.get(file)?;
    Ok(info
        .patches
        .iter()
        .filter(|p| p.kind == Some(PatchType::PERMANENT))
        .filter_map(|p| Some((p.slot, f.dil_frames(p.job? as usize)?.start as u32)))
        .collect())
}

/// A placeholder image: at least 20×20, 90 % of it opaque and 90 % of that near black (job 0 of
/// most files, e.g. the settler files' 61×42 and file 6's 60×60 blocks). The audit counts each
/// one drawn: it exists, so it is no "miss", but nothing the original shows looks like that.
fn opaque_black(img: &IndexedImage, f: &GfxFile, index: usize) -> bool {
    let area = img.header.width as usize * img.header.height as usize;
    if img.raw || area < 400 {
        return false;
    }
    let Ok(pal) = f.working_palette(index, &DecodeOptions::default()) else { return false };
    let solid = img.pixels.iter().filter(|&&p| p > 1).count();
    let black = img.pixels.iter().filter(|&&p| p > 1 && pal[p as usize][..3].iter().map(|&c| c as u32).sum::<u32>() < 40).count();
    solid * 10 >= area * 9 && black * 10 >= solid * 9
}

/// Garrison soldiers and the flag of a military building placed by the map.
///
/// * Garrison: the building's inhabitants (start towers: `CBuildingMgr::AddSoldierToStartTower`),
///   each in the slot the simulation gave it (+0x1a4); raw map data: `CMilitaryBuildingRole::
///   SettlerEnter`'s choice, swordsmen (warrior type 2) in the first free slot with `top == false`
///   and bowmen (type 3) in the first free `top` slot of buildingInfo.xml's soldier list.
///   `FillGfxInfo` hands each inhabitant to `BlitBuilding` as a settler patch
///   (`CSettler::GetPatchGfx`: file 20 + **the settler's** race, its job, frame and slot direction)
///   at the slot's pixel offset from the buildingInfo of the soldier's race for the building's
///   type (as the tower shot reads it, 0x58c46f), with the owner's team colours, drawn after the
///   building. A captured building keeps its builder's race, its soldiers theirs (playtest P2).
///   **[inf]** The job: the settler type's first job (its standing pose), frame 0; the tower idle
///   list (`TakeWaitList`) was not resolved to a gfx job.
/// * Flag: once a soldier is in, `FillGfxInfo` sets the race's standarte object (frame 0) at
///   buildingInfo's `miniFlag` pixel offset, and the owner's heraldic figure
///   (`GetUserLogoGfxInfo`: file 1, logo `owner − 1`) at the flag image's top-left.
#[allow(clippy::too_many_arguments)]
fn military_parts(
    files: &mut Files,
    placed: &mut Vec<Placed>,
    b: &crate::population::Building,
    race: Race,
    gamedata: &s4_formats::gamedata::GameData,
    building: Building,
    player: u8,
    front: impl Fn(&mut Vec<Placed>),
) -> anyhow::Result<()> {
    let Some(info) = gamedata.building(race, building).filter(|i| !i.soldiers.is_empty()) else {
        front(placed);
        return Ok(());
    };
    let mut used = vec![false; info.soldiers.len()];
    for ih in &b.inhabitants {
        let Some(st) = Settler::from_u16(ih.kind) else { continue };
        let srace = Race::from_u16(ih.race as u16).unwrap_or(race);
        let slot = match ih.slot {
            Some(s) => s as usize,
            None => {
                // Warrior type 2 (swordsmen, 29..=31) → non-top slots, 3 (bowmen, 32..=34) → top slots.
                let bowman = (32..=34).contains(&ih.kind);
                let Some(s) = (0..info.soldiers.len()).find(|&i| !used[i] && info.soldiers[i].top == bowman) else { continue };
                s
            }
        };
        // The slot of the soldier's race's building of this type, else the building's own.
        let Some(sl) = gamedata.building(srace, building).and_then(|i| i.soldiers.get(slot)).or(info.soldiers.get(slot)).copied() else { continue };
        if let Some(u) = used.get_mut(slot) {
            *u = true;
        }
        let sfile = resolve::settler_file(srace);
        let f = files.get(sfile)?;
        // The soldier's own job, frame and direction (`GetPatchGfx`; the tower logic turns it to
        // its target and plays the shot's anim list: playtest B2), animated by the draw-time
        // `Update` through its entity's pose; raw map data: the type's first job, frame 0, the
        // slot's direction.
        let (job, dir, frame, entity) = match ih.unit.as_ref().filter(|u| u.gfx_job().is_some()) {
            Some(u) => (u.gfx_job().map(|j| j as usize), u.dir as usize % 6, u.frame as u16, Some(u.id)),
            None => (resolve::settler_job(f, st), sl.direction.clamp(0, 5) as usize, 0, None),
        };
        let Some(r) = job.and_then(|job| resolve::settler_frames(f, job, dir)).filter(|r| !r.is_empty()) else {
            continue;
        };
        let n = r.len() as u16;
        let frame = if frame < n { frame } else { 0 };
        let key = SpriteKey { file: sfile, image: r.start as u32 + frame as u32 };
        let anim = entity.map(|_| (r.start as u32, n, frame, 1));
        let off = PixOff { dx: sl.x as i16, dy: sl.y as i16, rel: None, at_main: false };
        placed.push(Placed { key, x: b.x, y: b.y, layer: Layer::Object, player, wave: None, min_fog: FOG_SOLID, off, anim, crop: None, entity, patch: None, vpart: VPart::None });
    }
    // BlitBuilding: FX layers 5..9 (front walls, doors) after the settler patches, then the flag.
    front(placed);
    if !used.iter().any(|&u| u) {
        return Ok(());
    }
    if let Some(job) = flag_job(race) {
        let f = files.get(resolve::OBJECT_FILE)?;
        if let Some(r) = f.job_frames(job, 0) {
            let flag = SpriteKey { file: 5, image: r.start as u32 };
            let off = PixOff { dx: info.mini_flag.x as i16, dy: info.mini_flag.y as i16, rel: None, at_main: false };
            placed.push(Placed { key: flag, x: b.x, y: b.y, layer: Layer::Object, player, wave: None, min_fog: FOG_SOLID, off, anim: None, crop: None, entity: None, patch: None, vpart: VPart::None });
            let logos = files.get(1)?;
            let owner = b.owner.max(1) as usize;
            let image = logos.job_frames(owner - 1, 0).map_or(2 * (owner - 1), |r| r.start);
            if image < logos.image_count() {
                let rel = PixOff { dx: info.mini_flag.x as i16, dy: info.mini_flag.y as i16, rel: Some(flag), at_main: false };
                let key = SpriteKey { file: 1, image: image as u32 };
                placed.push(Placed { key, x: b.x, y: b.y, layer: Layer::Object, player, wave: None, min_fog: FOG_SOLID, off: rel, anim: None, crop: None, entity: None, patch: None, vpart: VPart::None });
            }
        }
    }
    Ok(())
}

/// The small player flag of a building whose worker is home (0x540300): file 5 job 0x220 + the
/// player's colour (jobs 0x220..=0x227; [inf] colour = player − 1, as the team colours here),
/// direction 0 while the building works, 1 when stopped, at buildingInfo's `miniFlag` offset
/// (info +0x2c / +0x30). The frame is a counter shared by all flags that advances once per game
/// tick, modulo job 0x220's frame count (12); a frame past the job's end shows frame 0.
fn player_flag(
    files: &mut Files,
    info: Option<&s4_formats::gamedata::BuildingInfo>,
    working: bool,
    b: &crate::population::Building,
    player: u8,
    tick: u32,
) -> anyhow::Result<Option<Placed>> {
    let Some(info) = info else { return Ok(None) };
    let f = files.get(resolve::OBJECT_FILE)?;
    let job = 0x220 + player as usize;
    let cycle = f.job_frames(0x220, 0).map_or(0, |r| r.len());
    let Some(r) = f.job_frames(job, if working { 0 } else { 1 }).filter(|r| !r.is_empty() && cycle > 0) else { return Ok(None) };
    let n = cycle.min(r.len()) as u16;
    let frame = (tick % n as u32) as u16;
    let key = SpriteKey { file: resolve::OBJECT_FILE, image: (r.start + frame as usize) as u32 };
    let off = PixOff { dx: info.mini_flag.x as i16, dy: info.mini_flag.y as i16, rel: None, at_main: false };
    let anim = Some((r.start as u32, n, frame, 1));
    Ok(Some(Placed { key, x: b.x, y: b.y, layer: Layer::Object, player, wave: None, min_fog: FOG_SOLID, off, anim, crop: None, entity: None, patch: None, vpart: VPart::None }))
}

/// The house worker a workup or production building draws (`FillGfxInfo` sub-sprite 0, see
/// `population::house_worker`): `GetSettlerGfxInfo` of its job, frame and direction (file 20 +
/// the settler's race), at the building's anchor; the pixel offset comes with its pose.
/// A deco object's entry: trees, plants and mushrooms show gfx job first + stage (`CTree` /
/// `CPlant::GetGfxInfo` draw their +0x26); stone heaps are OBJECT_MINESET1 with dil
/// `amount − 1`. `None` when the file has no image for it.
/// How a deco object's frame moves with the game ticks (its class's `GetGfxInfo`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum DecoAnim {
    /// `frame = (frame + Δtick) % frames`: trees, plants, mushrooms, hives, the shadow herb
    /// (0x593b80, 0x566430, 0x565df0, 0x565600, 0x566a40) and a plain `CDecoObject` with
    /// `pingPong` set.
    Loop = 0,
    /// A plain `CDecoObject` (0x562700) whose object record byte 7 (`objectInfo.xml`
    /// `pingPong != 0`) is 0 — the constructor 0x562460 sets +0x45 = 1 then: the frame climbs to
    /// the last, falls back to 0 and climbs again (direction +0x44, 1 from the constructor).
    /// Only the castle, colossus, gate, pharos, pond, alchemist and moai 02 have `pingPong`; every
    /// other plain object (grass, bushes, flowers, reed) swings back and forth.
    PingPong = 1,
}

/// The frame a deco animation shows at game tick `tick`: `phase` is where the cycle stood at
/// tick 0 (one frame a tick, as the release's draw adds the ticks since its last call).
/// [inf] The release's ping-pong from the last frame, met at a creation index of `frames − 1`,
/// wraps to 0 once before it turns; taken as turning there.
pub fn deco_frame(frames: u16, phase: u16, mode: u8, tick: u64) -> u16 {
    let n = frames.max(1) as u64;
    if mode == DecoAnim::PingPong as u8 && n > 1 {
        let period = 2 * (n - 1);
        let p = (phase as u64 + tick) % period;
        (if p < n { p } else { period - p }) as u16
    } else {
        ((phase as u64 + tick) % n) as u16
    }
}

/// The animation phase at tick 0 of a cycle of `frames` that shows `frame` at tick `anchor`.
fn deco_phase(frames: u16, frame: u16, mode: u8, anchor: u32) -> u16 {
    let period = if mode == DecoAnim::PingPong as u8 && frames > 1 { 2 * (frames as u32 - 1) } else { frames.max(1) as u32 };
    ((frame as u32 % period + period - anchor % period) % period) as u16
}

fn place_deco(f: &GfxFile, gamedata: &s4_formats::gamedata::GameData, d: &crate::population::Deco, anchor: u32) -> Option<Placed> {
    let frames = match d.id {
        41 => resolve::object_frames(f, Object::MINESET1, d.amount.max(1) as usize),
        id => Object::from_u16(id).and_then(|o| {
            let job = resolve::first_job(f, o as u16)? + d.stage as usize;
            f.job_frames(job, 0)
        }),
    };
    let r = frames.filter(|r| !r.is_empty())?;
    // A frame past the job's end shows frame 0 (`GetGfxInfo`).
    if d.frame as usize >= r.len() {
        crate::audit::miss(|| format!("deco {} stage {} frame {} past {} (file 5 images {}..)", d.id, d.stage, d.frame, r.len(), r.start));
    }
    let frame = if (d.frame as usize) < r.len() { d.frame as usize } else { 0 };
    let image = r.start + frame;
    let solid = matches!(d.id, 41 | 123..=136 | 162 | 209..=214 | 255) || (d.id <= 22 && d.stage != 3);
    let min_fog = if solid { FOG_SOLID } else { FOG_DECO };
    let key = SpriteKey { file: 5, image: image as u32 };
    // `GetGfxInfo` of trees, plants, mushrooms, hives, shadow herbs and plain deco objects
    // advances the frame by the ticks since its last call ([`DecoAnim`]). Stones (`CStone`
    // 0x5935e0) do not animate.
    let cycle = if d.cycle == 0 { r.len() } else { (d.cycle as usize).min(r.len()) };
    let plain = !matches!(d.id, 1..=22 | 41 | 124..=136 | 162 | 209..=214 | 216 | 217 | 253 | 255);
    let ping_pong = Object::from_u16(d.id).and_then(|o| gamedata.object(o)).is_some_and(|i| i.ping_pong);
    let mode = if plain && !ping_pong { DecoAnim::PingPong } else { DecoAnim::Loop } as u8;
    let anim = (!matches!(d.id, 41 | 253) && cycle > 1 && d.animated())
        .then(|| (r.start as u32, cycle as u16, deco_phase(cycle as u16, frame as u16, mode, anchor), mode));
    Some(Placed { key, x: d.x, y: d.y, layer: Layer::Object, player: 0, wave: None, min_fog, off: PixOff::NONE, anim, crop: None, entity: None, patch: None, vpart: VPart::None })
}

fn house_worker(files: &mut Files, b: &crate::population::Building, w: &crate::population::Unit, player: u8) -> anyhow::Result<Option<Placed>> {
    let Some(st) = Settler::from_u16(w.kind).filter(|_| w.kind < 67) else { return Ok(None) };
    let file = resolve::settler_file(Race::from_u16(w.race as u16).unwrap_or(Race::ROMAN));
    let f = files.get(file)?;
    let job = match w.gfx_job() {
        Some(j) => Some(j as usize),
        None => resolve::settler_job(f, st),
    };
    let Some(r) = job.and_then(|j| resolve::settler_frames(f, j, w.dir as usize % 6)).filter(|r| !r.is_empty()) else { return Ok(None) };
    let n = r.len() as u16;
    let frame = if (w.frame as u16) < n { w.frame as u16 } else { 0 };
    let key = SpriteKey { file, image: r.start as u32 + frame as u32 };
    let anim = Some((r.start as u32, n, frame, 1));
    Ok(Some(Placed { key, x: b.x, y: b.y, layer: Layer::Object, player, wave: None, min_fog: FOG_SOLID, off: PixOff::NONE, anim, crop: None, entity: Some(w.id), patch: None, vpart: VPart::None }))
}

/// Zoom-1 anchor of node (x, y) at height h in 16.16 world px, as `RenderObjectLayer` hands it to
/// the blitters: `(24x − 12y) << 16` and `(12y << 16) − CalcFinalHeightOffset(h)`.
fn anchor_fixed(x: i32, y: i32, h: u8) -> (i32, i32) {
    ((24 * x - 12 * y) << 16, ((12 * y) << 16) - height_offset_fixed(h, DEFAULT_TRIANGLE_SIZE))
}

/// One 512-entry palette row: 0..256 the working palette (4444 alpha), 256..512 the raw extra
/// entries of team-colour palettes (per-player ramps), straight alpha.
fn palette_row(f: &GfxFile, image: usize, off: u32) -> anyhow::Result<Vec<u8>> {
    let mut row = vec![0u8; (PAL_W * 4) as usize];
    // The shipped exe always blits objects in software (`IsHardwareObjectEngine` = `xor al,al`,
    // rel 0x505e20) from the opaque 565 `.p26` palette: ZoomBobNormal (rel 0x65f980) writes palette
    // words straight, without alpha. The ARGB4444 `.p24` alpha is a hardware-engine feature.
    let opt = DecodeOptions { player: None, use_4444_alpha: false, ..Default::default() };
    for (i, c) in f.working_palette(image, &opt)?.iter().enumerate() {
        row[i * 4..i * 4 + 4].copy_from_slice(c);
    }
    let pf = f.palette_file();
    if pf.entries >= 512 {
        let cols = pf.level(off, 0)?;
        for i in 256..512.min(cols.len()) {
            row[i * 4..i * 4 + 4].copy_from_slice(&pf.format.to_rgba(cols[i]));
        }
    }
    Ok(row)
}

fn decode_parallel(files: &HashMap<u32, GfxFile>, keys: &[SpriteKey]) -> anyhow::Result<Vec<IndexedImage>> {
    // A few images (a running game's new frames) decode faster than threads start.
    if keys.len() < 256 {
        return keys.iter().map(|k| Ok(files[&k.file].decode_indexed(k.image as usize)?)).collect();
    }
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(16);
    let chunk = keys.len().div_ceil(threads).max(1);
    let parts: Vec<anyhow::Result<Vec<IndexedImage>>> = std::thread::scope(|s| {
        let handles: Vec<_> = keys
            .chunks(chunk)
            .map(|ks| {
                s.spawn(move || -> anyhow::Result<Vec<IndexedImage>> {
                    ks.iter().map(|k| Ok(files[&k.file].decode_indexed(k.image as usize)?)).collect()
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().expect("decode thread panicked")).collect()
    });
    let mut out = Vec::with_capacity(keys.len());
    for p in parts {
        out.extend(p?);
    }
    Ok(out)
}

// ------------------------------------------------------------------------------------------------
// GPU

/// GPU resources of the sprite layer; draws into the main pass after the terrain.
pub struct SpriteRenderer {
    pipeline: wgpu::RenderPipeline,
    bind: wgpu::BindGroup,
    layout: wgpu::BindGroupLayout,
    atlas: wgpu::Texture,
    palette: wgpu::Texture,
    uniforms: wgpu::Buffer,
    instances: wgpu::Buffer,
    instance_cap: u64,
    count: u32,
    /// The first [`SpriteRenderer::draw`] draws these of `count`, [`SpriteRenderer::draw_ui`] the rest.
    world_count: u32,
    /// Placed instances of the current scene (world space, draw order).
    all: Vec<SpriteInstance>,
    /// While set, [`SpriteRenderer::set_scene`] (also `sync`'s) keeps the instances it has: a
    /// frame that changes the scene several times takes them once, at its end (s4-game
    /// `present`).
    pub hold: bool,
    srgb_target: bool,
    /// Optional effect: shadow pixels as 50 % black instead of the dither.
    pub smooth_shadows: bool,
    /// Draw the interface instances last, by [`SpriteRenderer::draw_ui`], instead of in their place.
    pub ui_apart: bool,
}

impl SpriteRenderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat, scene: &mut SpriteScene) -> Self {
        let (atlas, palette) = Self::textures(device, queue, scene);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sprites"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let tex = |binding, sample_type, dim| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture { sample_type, view_dimension: dim, multisampled: false },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sprites"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                tex(1, wgpu::TextureSampleType::Uint, wgpu::TextureViewDimension::D2Array),
                tex(2, wgpu::TextureSampleType::Float { filterable: false }, wgpu::TextureViewDimension::D2),
            ],
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sprite view"),
            size: UNIFORM_BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind = Self::bind_group(device, &layout, &uniforms, &atlas, &palette);
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sprites"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let attrs = wgpu::vertex_attr_array![
            0 => Float32x2, 1 => Float32x2, 2 => Uint32, 3 => Uint32, 4 => Uint32, 5 => Uint32
        ];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sprites"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: INSTANCE_BYTES,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &attrs,
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() },
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        let instance_cap = 1024;
        SpriteRenderer {
            pipeline,
            bind,
            layout,
            atlas,
            palette,
            uniforms,
            instances: Self::instance_buffer(device, instance_cap),
            instance_cap,
            count: 0,
            world_count: 0,
            all: scene.instances().to_vec(),
            hold: false,
            srgb_target: format.is_srgb(),
            smooth_shadows: false,
            ui_apart: false,
        }
    }

    fn instance_buffer(device: &wgpu::Device, cap: u64) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sprite instances"),
            size: cap * INSTANCE_BYTES,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// The atlas (all layers) and palette textures of a scene, filled.
    fn textures(device: &wgpu::Device, queue: &wgpu::Queue, scene: &SpriteScene) -> (wgpu::Texture, wgpu::Texture) {
        // Room to grow: a new atlas layer or palette row is then a write, not new textures and
        // the whole atlas uploaded again (a hitch of 10+ ms a time in a busy game).
        let layers = (scene.atlas.len() as u32 / 8 + 1) * 8;
        let layers = layers.min(device.limits().max_texture_array_layers).max(scene.atlas.len().max(1) as u32);
        let atlas = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sprite atlas"),
            size: wgpu::Extent3d { width: SPRITE_ATLAS, height: SPRITE_ATLAS, depth_or_array_layers: layers },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Uint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for i in 0..scene.atlas.len() as u32 {
            Self::write_atlas_rows(queue, &atlas, scene, i, 0, SPRITE_ATLAS);
        }
        let rows = (scene.palette_rows / 64 + 1) * 64;
        let palette = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sprite palettes"),
            size: wgpu::Extent3d { width: PAL_W, height: rows, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // Raw 8-bit values: the shader does the original's integer fog scaling on them and
            // encodes for an sRGB target itself (as the terrain shader does).
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        Self::write_palettes(queue, &palette, scene);
        (atlas, palette)
    }

    fn write_atlas_rows(queue: &wgpu::Queue, atlas: &wgpu::Texture, scene: &SpriteScene, layer: u32, y0: u32, y1: u32) {
        let data = &scene.atlas[layer as usize];
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: atlas,
                mip_level: 0,
                origin: wgpu::Origin3d { x: 0, y: y0, z: layer },
                aspect: wgpu::TextureAspect::All,
            },
            &data[(y0 * SPRITE_ATLAS) as usize..(y1 * SPRITE_ATLAS) as usize],
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(SPRITE_ATLAS), rows_per_image: Some(y1 - y0) },
            wgpu::Extent3d { width: SPRITE_ATLAS, height: y1 - y0, depth_or_array_layers: 1 },
        );
    }

    fn write_palettes(queue: &wgpu::Queue, palette: &wgpu::Texture, scene: &SpriteScene) {
        if !scene.palettes.is_empty() {
            queue.write_texture(
                palette.as_image_copy(),
                &scene.palettes,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(PAL_W * 4), rows_per_image: Some(scene.palette_rows) },
                wgpu::Extent3d { width: PAL_W, height: scene.palette_rows, depth_or_array_layers: 1 },
            );
        }
    }

    fn bind_group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        uniforms: &wgpu::Buffer,
        atlas: &wgpu::Texture,
        palette: &wgpu::Texture,
    ) -> wgpu::BindGroup {
        let atlas_view = atlas.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sprites"),
            layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&atlas_view) },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&palette.create_view(&Default::default())),
                },
            ],
        })
    }

    /// Replace the placed instances (after `SpriteScene::update_heights`).
    pub fn set_scene(&mut self, scene: &mut SpriteScene) {
        if self.hold {
            return;
        }
        let _t = crate::perf::span("sprites: set_scene copy");
        self.all.clear();
        self.all.extend_from_slice(scene.instances());
    }

    /// Replace the instances and upload what the scene added to the atlas and palettes since the
    /// last sync (after `SpriteScene::set_population`). New rows of an existing layer are written
    /// in place; a new layer or palette row means new textures (rare: the atlas only grows when an
    /// image is needed for the first time).
    pub fn sync(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, scene: &mut SpriteScene) {
        let dirty = scene.take_dirty();
        let grown = scene.atlas.len() as u32 > self.atlas.depth_or_array_layers() || scene.palette_rows > self.palette.height();
        if grown {
            let (atlas, palette) = Self::textures(device, queue, scene);
            self.bind = Self::bind_group(device, &self.layout, &self.uniforms, &atlas, &palette);
            self.atlas = atlas;
            self.palette = palette;
        } else {
            for (&layer, &(y0, y1)) in &dirty.atlas {
                Self::write_atlas_rows(queue, &self.atlas, scene, layer, y0, y1);
            }
            if dirty.palettes {
                Self::write_palettes(queue, &self.palette, scene);
            }
        }
        self.set_scene(scene);
    }

    /// Before the render pass: cull for this view and upload instances + view uniforms.
    pub fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, view: View, size: (u32, u32)) {
        let (w, h) = (size.0 as f32 / view.zoom, size.1 as f32 / view.zoom);
        let mut bytes = Vec::new();
        let mut n = 0u32;
        let mut put = |ui: Option<bool>| {
            for i in &self.all {
                if ui.is_none_or(|u| (i.flags & FLAG_UI != 0) == u) && i.pos[0] < view.x + w && i.pos[0] + i.size[0] > view.x && i.pos[1] < view.y + h && i.pos[1] + i.size[1] > view.y {
                    i.write(&mut bytes);
                    n += 1;
                }
            }
            n
        };
        let world = put(self.ui_apart.then_some(false));
        let n = if self.ui_apart { put(Some(true)) } else { world };
        if n as u64 > self.instance_cap {
            self.instance_cap = (n as u64).next_power_of_two();
            self.instances = Self::instance_buffer(device, self.instance_cap);
        }
        if !bytes.is_empty() {
            queue.write_buffer(&self.instances, 0, &bytes);
        }
        self.world_count = world;
        self.count = n;
        let mut u = Vec::with_capacity(UNIFORM_BYTES as usize);
        for f in [view.x, view.y, 2.0 * view.zoom / size.0.max(1) as f32, -2.0 * view.zoom / size.1.max(1) as f32] {
            u.extend_from_slice(&f.to_le_bytes());
        }
        for x in [self.srgb_target as u32, self.smooth_shadows as u32, 0, 0] {
            u.extend_from_slice(&x.to_le_bytes());
        }
        queue.write_buffer(&self.uniforms, 0, &u);
    }

    /// Inside the main pass, right after the terrain.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        if self.count == 0 {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind, &[]);
        pass.set_vertex_buffer(0, self.instances.slice(..self.count as u64 * INSTANCE_BYTES));
        pass.draw(0..4, 0..self.world_count);
    }

    /// After what shades the world: the interface instances [`SpriteRenderer::ui_apart`] held back.
    pub fn draw_ui(&self, pass: &mut wgpu::RenderPass<'_>) {
        if self.count > self.world_count {
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind, &[]);
            pass.set_vertex_buffer(0, self.instances.slice(..self.count as u64 * INSTANCE_BYTES));
            pass.draw(0..4, self.world_count..self.count);
        }
    }
}

const SHADER: &str = r#"
// Palette-indexed sprites. World units are the original's zoom-1 screen pixels.
struct View {
    origin: vec2<f32>,
    scale: vec2<f32>,
    srgb: u32,   // target is sRGB: encode the raw values so the stored bytes equal them
    smooth_shadow: u32, // optional effect: smooth shadows
    _p1: u32,
    _p2: u32,
};
@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var atlas: texture_2d_array<u32>;
@group(0) @binding(2) var pal: texture_2d<f32>;

struct Out {
    @builtin(position) pos: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) @interpolate(flat) origin: u32,
    @location(2) @interpolate(flat) layer: u32,
    @location(3) @interpolate(flat) row: u32,
    @location(4) @interpolate(flat) flags: u32,
    @location(5) @interpolate(flat) texscale: f32,
};

@vertex
fn vs(
    @builtin(vertex_index) vi: u32,
    @location(0) pos: vec2<f32>,
    @location(1) size: vec2<f32>,
    @location(2) origin: u32,
    @location(3) layer: u32,
    @location(4) row: u32,
    @location(5) flags: u32,
) -> Out {
    let corner = vec2<f32>(f32(vi & 1u), f32(vi >> 1u));
    let world = pos + corner * size;
    var o: Out;
    o.pos = vec4<f32>((world - view.origin) * view.scale + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
    // Texel width / drawn width: 1 normally, 2 for half-scale images.
    let tw = f32(flags >> 20u);
    o.texscale = tw / max(size.x, 1.0);
    o.local = corner * size * o.texscale;
    o.origin = origin;
    o.layer = layer;
    o.row = row;
    o.flags = flags;
    return o;
}

@fragment
fn fs(i: Out) -> @location(0) vec4<f32> {
    let w = i32(i.flags >> 20u);
    // Texel under the pixel's top-left corner: ZoomBob steps the source by whole texels per
    // destination pixel (2 for half-scale images), starting at texel 0.
    let step = vec2<f32>(dpdx(i.local.x), dpdy(i.local.y));
    let at = floor(i.local - 0.5 * step + vec2<f32>(1e-3));
    let t = vec2<i32>(clamp(i32(at.x), 0, w - 1), max(i32(at.y), 0));
    let o = vec2<i32>(i32(i.origin & 0xffffu), i32(i.origin >> 16u));
    let idx = textureLoad(atlas, o + t, i32(i.layer), 0).r;
    if (idx == 0u) {
        discard;
    }
    if (idx == 1u) {
        if (view.smooth_shadow != 0u) {
            return vec4<f32>(0.0, 0.0, 0.0, 0.5);
        }
        // ZoomBobNormal's shadow: black on screen pixels with even x + y.
        let p = vec2<u32>(i.pos.xy);
        if (((p.x + p.y) & 1u) != 0u) {
            discard;
        }
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    var c: vec4<f32>;
    let team = (i.flags & 1u) != 0u;
    let player = (i.flags >> 8u) & 7u;
    if (team && idx >= 192u && idx < 224u) {
        c = textureLoad(pal, vec2<i32>(i32(idx - 192u), i32(player)), 0);
    } else if (team && idx >= 224u) {
        c = textureLoad(pal, vec2<i32>(i32(256u + 32u * player + idx - 224u), i32(i.row)), 0);
    } else {
        c = textureLoad(pal, vec2<i32>(i32(idx), i32(i.row)), 0);
    }
    if (c.a <= 0.0) {
        discard;
    }
    // Fog (sub_2F6FD80, release rel 0x663f50): above 230 the palette is used as is, otherwise
    // every 565 channel becomes (c * fog) >> 8. (fog <= 0 would give white; never drawn.)
    let fog = (i.flags >> 12u) & 255u;
    var rgb = c.rgb;
    if (fog <= 230u) {
        let raw = vec3<u32>(round(rgb * 255.0));
        let c565 = vec3<u32>(raw.r >> 3u, raw.g >> 2u, raw.b >> 3u);
        let f = (c565 * fog) >> vec3<u32>(8u);
        let e = vec3<u32>((f.r << 3u) | (f.r >> 2u), (f.g << 2u) | (f.g >> 4u), (f.b << 3u) | (f.b >> 2u));
        rgb = vec3<f32>(e) / 255.0;
    }
    if (view.srgb != 0u) {
        rgb = to_linear(rgb);
    }
    return vec4<f32>(rgb * c.a, c.a);
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}
"#;
