//! Savegames in our own format (not the original's `.sav` chunks): a versioned header, then the
//! world, the script part, the commands still waiting for their tick, the script host's state
//! and the client's (camera, local player, campaign mission), serialized with serde (bincode)
//! and deflated.
//!
//! What a save keeps follows the original's split: the world (`CGameRun::Save`'s managers), the
//! script (`CGameScriptManager::Save`: source, `Vars.Save1..9`, event requests; every other Lua
//! global is gone after a load, as in the original), and the fog of war (chunk 135: the explored
//! area survives). The rules (game data) are not saved: they are loaded again and must have the
//! fingerprint the save was made with.
//!
//! **Compatibility policy** (a save never loads into wrong state; it loads exactly or is refused
//! with a reason the load menus show, [`compatibility`]):
//! * [`VERSION`] is bumped whenever anything the header or body serializes changes. bincode has
//!   no field names, so `#[serde(default)]` cannot fill a field an older save lacks: a save of
//!   another version is refused (an older one as "made by an older s4rs", a newer one too).
//! * The rules fingerprint must match (game data or rule tables changed → refused).
//! * The header keeps the world's `state_hash` at saving; after reading the body (which must be
//!   used up exactly) the loaded world must hash the same. A build that changed a saved type
//!   without bumping [`VERSION`] therefore fails here instead of running a misread world.
//! * Migration: a save of version [`OLDEST_MIGRATED`]..[`VERSION`] is read in its own layout (the
//!   body's types made generic over the changed part, an old mirror type for it) and converted
//!   (`From`), the new fields at their defaults; the world hash must still match. Version 30
//!   (2026-10-24) added `GameSetup::out`; 29 is read through [`GameSetupV29`].
//! * Version 3 (2026-10-21) added the header's world hash; version-2 saves (made before the
//!   magic, animals, alliances, trading and donkey data were saved) are refused. Version 5
//!   (2026-10-22): the game setup (controls, game type), the statistic's strength maxima and
//!   start snapshot.

use std::io::{Read, Write};
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use s4_sim::{Entities, Grid, Layers, NetMessage, Player, PlayerCommand, Random16, Rules, Schedule, World};

use crate::commands::Stamped;

pub const MAGIC: &str = "s4rs-save";
/// Bumped whenever a saved type changes (see the module doc).
pub const VERSION: u32 = 30;
/// The oldest version still read: its body is read in its own layout and migrated ([`read`]).
pub const OLDEST_MIGRATED: u32 = 29;
/// Thumbnail size (RGB) in the header.
pub const THUMB: (u32, u32) = (160, 120);

/// What the save list shows without reading the body.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Header {
    pub magic: String,
    pub version: u32,
    /// The player's name for the save.
    pub name: String,
    /// Seconds since the Unix epoch.
    pub saved_at: u64,
    /// The map file and its hash (`s4_net::map_hash`).
    pub map: String,
    pub map_hash: u64,
    pub tick: u32,
    /// Fingerprint of the rules (game data) the world ran with.
    pub rules: u64,
    /// A screenshot, [`THUMB`] sized RGB (empty without one).
    pub thumb: Vec<u8>,
    /// `World::state_hash` of the saved world (0 in headers before version 3).
    pub world_hash: u64,
}

/// The header of versions 1 and 2 (no world hash).
#[derive(Deserialize)]
struct HeaderV2 {
    magic: String,
    version: u32,
    name: String,
    saved_at: u64,
    map: String,
    map_hash: u64,
    tick: u32,
    rules: u64,
    thumb: Vec<u8>,
}

/// What every header starts with.
#[derive(Deserialize)]
struct HeaderStart {
    magic: String,
    version: u32,
}

/// Scale an RGBA screenshot `w`×`h` down to [`THUMB`] RGB (box filter).
pub fn thumbnail(rgba: &[u8], w: u32, h: u32) -> Vec<u8> {
    let (tw, th) = THUMB;
    let mut out = Vec::with_capacity((tw * th * 3) as usize);
    for ty in 0..th {
        let (y0, y1) = (ty * h / th, ((ty + 1) * h / th).max(ty * h / th + 1));
        for tx in 0..tw {
            let (x0, x1) = (tx * w / tw, ((tx + 1) * w / tw).max(tx * w / tw + 1));
            let mut acc = [0u32; 3];
            let mut n = 0;
            for y in y0..y1.min(h) {
                for x in x0..x1.min(w) {
                    let o = ((y * w + x) * 4) as usize;
                    for (k, a) in acc.iter_mut().enumerate() {
                        *a += rgba.get(o + k).copied().unwrap_or(0) as u32;
                    }
                    n += 1;
                }
            }
            for a in acc {
                out.push((a / n.max(1)) as u8);
            }
        }
    }
    out
}

/// The client's part: what the screen showed.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct ClientState {
    pub view: (f32, f32, f32),
    pub local: u8,
    pub campaign: Option<(u8, u32)>,
    pub speed: u32,
    /// The session's `Logo\Current` pictures, per slot (`CUserLogoChunk` 0x4c4a10 saves the
    /// composed `Gfx\1.*` files and 0x4c4790 writes them back at load: the flags show the saved
    /// game's logos). Empty for saves without them.
    pub logos: Vec<Option<Vec<u8>>>,
    /// A network save: the lockstep's own part (s4-net `Lockstep::own_pending`), so the game
    /// restarts with the commands that were in flight.
    pub net: Option<NetResume>,
}

/// The local player's lockstep state in a network save.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct NetResume {
    /// Own turns not yet run: (communication tick, commands), empty turns included.
    pub turns: Vec<(u32, Vec<SavedMessage>)>,
    /// The last communication tick the turns were produced for.
    pub upto: u32,
    /// Commands issued but not yet in a turn.
    pub pending: Vec<SavedMessage>,
    /// The players' names (the session's roster; the release keeps them in its save's chunk
    /// 130): a loaded game's lobby shows them on the slots still waiting for their player.
    pub names: Vec<(u8, String)>,
}

/// The network saves in `dir` of the game whose world hash after loading is `hash`: (file, the
/// player it was saved by), newest first.
pub fn network_saves_in(dir: &Path, hash: u64) -> Vec<(std::path::PathBuf, u8)> {
    let Ok(rules) = Rules::load_default() else { return Vec::new() };
    crate::menu::list_saves_in(dir)
        .into_iter()
        .filter(|(_, h)| h.world_hash == hash)
        .filter_map(|(p, _)| read(&p, rules.clone()).ok().filter(|l| l.client.net.is_some()).map(|l| (p, l.client.local)))
        .collect()
}

/// [`network_saves_in`] the save folder.
pub fn network_saves(hash: u64) -> Vec<(std::path::PathBuf, u8)> {
    dir().map_or_else(Vec::new, |d| network_saves_in(&d, hash))
}

/// A command as its net message (the `Command` enum has no serde derive).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct SavedMessage {
    pub kind: u32,
    pub player: u8,
    pub wparam: u32,
    pub lparam: u32,
    pub data: Vec<u16>,
}

impl SavedMessage {
    pub fn of(c: &PlayerCommand) -> SavedMessage {
        let m = c.to_message();
        SavedMessage { kind: m.kind, player: m.player, wparam: m.wparam, lparam: m.lparam, data: m.data }
    }

    pub fn command(&self) -> PlayerCommand {
        PlayerCommand::from_message(&NetMessage { kind: self.kind, player: self.player, wparam: self.wparam, lparam: self.lparam, data: self.data.clone() })
    }
}

/// The script host's own state (script.rs `GameHost`): `Game.Random`'s stand-in seed, the players
/// declared lost, the decision and the team event 0x37 carried (`CStateGame +0x68`, the end
/// record's winner).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct HostState {
    pub rng: u32,
    pub lost: u32,
    pub outcome: u8,
    pub winner: u8,
}

/// `s4_script::SavedState`, mirrored.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct ScriptState {
    pub source: Option<(Vec<u8>, String)>,
    pub vars: [f64; 9],
    pub requests: Vec<Vec<u8>>,
}

impl ScriptState {
    pub fn from_saved(s: &s4_script::SavedState) -> Self {
        ScriptState { source: s.source.clone(), vars: s.vars, requests: s.requests.iter().map(|r| r.slots.clone()).collect() }
    }

    pub fn to_saved(&self) -> s4_script::SavedState {
        s4_script::SavedState {
            source: self.source.clone(),
            vars: self.vars,
            requests: self.requests.iter().map(|r| s4_script::EventRequests { slots: r.clone() }).collect(),
        }
    }
}

/// The world without the rules and the tick's events (both not saved).
#[derive(Serialize)]
struct WorldRef<'a> {
    grid: &'a Grid,
    layers: &'a Layers,
    tiling: &'a s4_sim::tiling::Tiling,
    entities: &'a Entities,
    schedule: &'a Schedule,
    rng: &'a Random16,
    animal_rng: &'a Random16,
    players: &'a Vec<Option<Player>>,
    tick: u32,
    deco_pools: &'a s4_sim::deco::DecoPools,
    counters: &'a s4_sim::world::Counters,
    sites: &'a s4_sim::build::Sites,
    buildings: &'a s4_sim::buildings::BuildingRoles,
    economy: &'a s4_sim::eco::Economy,
    settler_lists: &'a s4_sim::settler_lists::SettlerLists,
    warmap: &'a s4_sim::warmap::WarMap,
    statistic: &'a s4_sim::statistic::Statistic,
    future: &'a s4_sim::future::FutureEvents,
    groups: &'a s4_sim::group::Groups,
    alliances: &'a s4_sim::alliances::Alliances,
    setup: &'a s4_sim::world::GameSetup,
    world_step: &'a s4_sim::wear::WorldStep,
    animals: &'a s4_sim::animals::AnimalMgr,
    magic: &'a s4_sim::magic::Magic,
    ai: &'a s4_sim::ai::Ai,
    vehicles: &'a s4_sim::vehicles::VehicleMgr,
    flying: &'a s4_sim::flying::FlyingMgr,
    fog: &'a Option<Box<s4_sim::fog::Fogging>>,
}

/// `GameSetup` as version 29 saved it (no `out`).
#[derive(Deserialize)]
pub struct GameSetupV29 {
    control: [u8; 9],
    game_type: u32,
    race_goods: [u8; 7],
    start_winner: i32,
}

impl From<GameSetupV29> for s4_sim::world::GameSetup {
    fn from(o: GameSetupV29) -> Self {
        s4_sim::world::GameSetup { control: o.control, game_type: o.game_type, race_goods: o.race_goods, start_winner: o.start_winner, ..Default::default() }
    }
}

/// The saved world; `S` is the game setup's layout of the save's version.
#[derive(Deserialize)]
struct WorldOwned<S = s4_sim::world::GameSetup> {
    grid: Grid,
    layers: Layers,
    tiling: s4_sim::tiling::Tiling,
    entities: Entities,
    schedule: Schedule,
    rng: Random16,
    animal_rng: Random16,
    players: Vec<Option<Player>>,
    tick: u32,
    deco_pools: s4_sim::deco::DecoPools,
    counters: s4_sim::world::Counters,
    sites: s4_sim::build::Sites,
    buildings: s4_sim::buildings::BuildingRoles,
    economy: s4_sim::eco::Economy,
    #[serde(default)]
    settler_lists: s4_sim::settler_lists::SettlerLists,
    #[serde(default)]
    warmap: Box<s4_sim::warmap::WarMap>,
    #[serde(default)]
    statistic: s4_sim::statistic::Statistic,
    #[serde(default)]
    future: s4_sim::future::FutureEvents,
    #[serde(default)]
    groups: s4_sim::group::Groups,
    #[serde(default)]
    alliances: s4_sim::alliances::Alliances,
    setup: S,
    world_step: s4_sim::wear::WorldStep,
    #[serde(default)]
    animals: s4_sim::animals::AnimalMgr,
    #[serde(default)]
    magic: s4_sim::magic::Magic,
    #[serde(default)]
    ai: s4_sim::ai::Ai,
    #[serde(default)]
    vehicles: s4_sim::vehicles::VehicleMgr,
    #[serde(default)]
    flying: s4_sim::flying::FlyingMgr,
    fog: Option<Box<s4_sim::fog::Fogging>>,
}

impl<S: Into<s4_sim::world::GameSetup>> WorldOwned<S> {
    fn into_world(self, rules: Rules) -> World {
        World {
            grid: self.grid,
            layers: self.layers,
            tiling: self.tiling,
            entities: self.entities,
            schedule: self.schedule,
            rng: self.rng,
            animal_rng: self.animal_rng,
            players: self.players,
            tick: self.tick,
            deco_pools: self.deco_pools,
            counters: self.counters,
            rules,
            sites: self.sites,
            buildings: self.buildings,
            economy: self.economy,
            settler_lists: self.settler_lists,
            warmap: self.warmap,
            statistic: self.statistic,
            future: self.future,
            groups: self.groups,
            alliances: self.alliances,
            setup: self.setup.into(),
            world_step: self.world_step,
            animals: self.animals,
            magic: self.magic,
            ai: self.ai,
            vehicles: self.vehicles,
            flying: self.flying,
            loading_buildings: false,
            delete_cause: 0,
            final_temple: false,
            events: Vec::new(),
            script_events: Default::default(),
            fog: self.fog,
        }
    }
}

/// A pending command as its net message (the `Command` enum has no serde derive).
#[derive(Serialize, Deserialize)]
struct SavedCommand {
    tick: u32,
    kind: u32,
    player: u8,
    wparam: u32,
    lparam: u32,
    data: Vec<u16>,
}

#[derive(Serialize)]
struct BodyRef<'a> {
    world: WorldRef<'a>,
    script: Option<ScriptState>,
    host: HostState,
    commands: Vec<SavedCommand>,
    client: ClientState,
}

#[derive(Deserialize)]
struct BodyOwned<S = s4_sim::world::GameSetup> {
    world: WorldOwned<S>,
    script: Option<ScriptState>,
    host: HostState,
    commands: Vec<SavedCommand>,
    client: ClientState,
}

impl<S: Into<s4_sim::world::GameSetup>> BodyOwned<S> {
    /// The body in the current layout.
    fn migrate(self) -> BodyOwned {
        let w = self.world;
        BodyOwned {
            world: WorldOwned {
                grid: w.grid,
                layers: w.layers,
                tiling: w.tiling,
                entities: w.entities,
                schedule: w.schedule,
                rng: w.rng,
                animal_rng: w.animal_rng,
                players: w.players,
                tick: w.tick,
                deco_pools: w.deco_pools,
                counters: w.counters,
                sites: w.sites,
                buildings: w.buildings,
                economy: w.economy,
                settler_lists: w.settler_lists,
                warmap: w.warmap,
                statistic: w.statistic,
                future: w.future,
                groups: w.groups,
                alliances: w.alliances,
                setup: w.setup.into(),
                world_step: w.world_step,
                animals: w.animals,
                magic: w.magic,
                ai: w.ai,
                vehicles: w.vehicles,
                flying: w.flying,
                fog: w.fog,
            },
            script: self.script,
            host: self.host,
            commands: self.commands,
            client: self.client,
        }
    }
}

/// A loaded savegame.
pub struct Loaded {
    pub header: Header,
    pub world: World,
    pub script: Option<ScriptState>,
    pub host: HostState,
    pub commands: Vec<Stamped>,
    pub client: ClientState,
}

/// Fingerprint of the rules: FNV-1a over their `Debug` form (every table, deterministic).
pub fn rules_fingerprint(rules: &Rules) -> u64 {
    format!("{rules:?}").bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, x| (h ^ x as u64).wrapping_mul(0x0100_0000_01b3))
}

/// Everything a save writes, borrowed from the sim thread.
pub struct SaveInput<'a> {
    pub name: &'a str,
    /// [`thumbnail`] of the screen (or empty).
    pub thumb: Vec<u8>,
    pub map: &'a Path,
    pub world: &'a World,
    pub script: Option<s4_script::SavedState>,
    pub host: HostState,
    pub commands: Vec<Stamped>,
    pub client: ClientState,
}

/// Write a savegame: the header (bincode), then the deflated body (bincode).
pub fn write(path: &Path, s: SaveInput<'_>) -> Result<Header> {
    let w = s.world;
    let header = Header {
        magic: MAGIC.into(),
        version: VERSION,
        name: s.name.into(),
        saved_at: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()),
        map: s.map.to_string_lossy().into_owned(),
        map_hash: s4_net::map_hash(s.map).unwrap_or(0),
        tick: w.tick,
        rules: rules_fingerprint(&w.rules),
        thumb: s.thumb,
        world_hash: w.state_hash(),
    };
    let body = BodyRef {
        world: WorldRef {
            grid: &w.grid,
            layers: &w.layers,
            tiling: &w.tiling,
            entities: &w.entities,
            schedule: &w.schedule,
            rng: &w.rng,
            animal_rng: &w.animal_rng,
            players: &w.players,
            tick: w.tick,
            deco_pools: &w.deco_pools,
            counters: &w.counters,
            sites: &w.sites,
            buildings: &w.buildings,
            economy: &w.economy,
            settler_lists: &w.settler_lists,
            warmap: &w.warmap,
            statistic: &w.statistic,
            future: &w.future,
            groups: &w.groups,
            alliances: &w.alliances,
            setup: &w.setup,
            world_step: &w.world_step,
            animals: &w.animals,
            magic: &w.magic,
            ai: &w.ai,
            vehicles: &w.vehicles,
            flying: &w.flying,
            fog: &w.fog,
        },
        script: s.script.as_ref().map(ScriptState::from_saved),
        host: s.host,
        commands: s
            .commands
            .iter()
            .map(|c| {
                let m = c.command.to_message();
                SavedCommand { tick: c.tick, kind: m.kind, player: m.player, wparam: m.wparam, lparam: m.lparam, data: m.data }
            })
            .collect(),
        client: s.client,
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    {
        let mut f = std::io::BufWriter::new(std::fs::File::create(&tmp).with_context(|| tmp.display().to_string())?);
        bincode::serialize_into(&mut f, &header)?;
        // Buffered in front of the encoder: bincode's many small writes straight into it took
        // 3 s for roman02's 7.4 MB body.
        let mut z = std::io::BufWriter::with_capacity(1 << 16, flate2::write::DeflateEncoder::new(f, flate2::Compression::fast()));
        bincode::serialize_into(&mut z, &body)?;
        z.into_inner().map_err(|e| e.into_error())?.finish()?.flush()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(header)
}

/// Read only the header (for the save list).
pub fn read_header(path: &Path) -> Result<Header> {
    read_header_from(&mut std::io::BufReader::new(std::fs::File::open(path).with_context(|| path.display().to_string())?), path)
}

/// The header of any version up to [`VERSION`] (older ones without the world hash); a newer
/// version is an error. The reader is left after the header.
fn read_header_from<R: Read + std::io::Seek>(f: &mut R, path: &Path) -> Result<Header> {
    let start: HeaderStart = bincode::deserialize_from(&mut *f).with_context(|| format!("{}: not an s4rs savegame", path.display()))?;
    if start.magic != MAGIC {
        bail!("{}: not an s4rs savegame", path.display());
    }
    if start.version > VERSION {
        bail!("{}: made by a newer s4rs (savegame version {}, this build reads {VERSION})", path.display(), start.version);
    }
    f.seek(std::io::SeekFrom::Start(0))?;
    // The header did not change from the oldest migrated version on.
    if start.version >= OLDEST_MIGRATED {
        return Ok(bincode::deserialize_from(&mut *f)?);
    }
    let o: HeaderV2 = bincode::deserialize_from(&mut *f)?;
    Ok(Header { magic: o.magic, version: o.version, name: o.name, saved_at: o.saved_at, map: o.map, map_hash: o.map_hash, tick: o.tick, rules: o.rules, thumb: o.thumb, world_hash: 0 })
}

static CURRENT_RULES: std::sync::OnceLock<Option<u64>> = std::sync::OnceLock::new();

/// The fingerprint of this build's rules (loaded once: ~200 ms, so the game notes them from its
/// world, [`note_rules`], and `main` warms it off the main thread for the start screens).
pub fn current_rules() -> Option<u64> {
    *CURRENT_RULES.get_or_init(|| Rules::load_default().ok().map(|r| rules_fingerprint(&r)))
}

/// The rules a game's world was made with (this build's): their fingerprint is
/// [`current_rules`], so the save pages do not load them again on the main thread.
pub fn note_rules(rules: &Rules) {
    CURRENT_RULES.get_or_init(|| Some(rules_fingerprint(rules)));
}

/// Whether this build can load a save with header `h` (rules fingerprint `rules`); the reason
/// shown in the load menus when not.
pub fn compatibility(h: &Header, rules: Option<u64>) -> std::result::Result<(), &'static str> {
    if h.version < OLDEST_MIGRATED {
        Err("older s4rs")
    } else if h.version > VERSION {
        Err("newer s4rs")
    } else if rules.is_some_and(|r| r != h.rules) {
        Err("other game data")
    } else {
        Ok(())
    }
}

/// Read a savegame; `rules` are the game's (their fingerprint must match the save's). Refused
/// as the module doc says, never loaded half.
pub fn read(path: &Path, rules: Rules) -> Result<Loaded> {
    let mut f = std::io::BufReader::new(std::fs::File::open(path).with_context(|| path.display().to_string())?);
    let header = read_header_from(&mut f, path)?;
    let fp = rules_fingerprint(&rules);
    if let Err(why) = compatibility(&header, Some(fp)) {
        bail!("{}: cannot be loaded, made with {why} (savegame version {}, rules {:016x}; this build: version {VERSION}, rules {fp:016x})", path.display(), header.version, header.rules);
    }
    let mut body = Vec::new();
    flate2::read::DeflateDecoder::new(f).read_to_end(&mut body)?;
    let mut rest = &body[..];
    let unreadable = || format!("{}: unreadable (made by an incompatible build)", path.display());
    // A save of an older version is read in its own layout and migrated.
    let b: BodyOwned = match header.version {
        29 => bincode::deserialize_from::<_, BodyOwned<GameSetupV29>>(&mut rest).with_context(unreadable)?.migrate(),
        _ => bincode::deserialize_from(&mut rest).with_context(unreadable)?,
    };
    if !rest.is_empty() {
        bail!("{}: {} bytes left after the body (made by an incompatible build)", path.display(), rest.len());
    }
    let commands = b
        .commands
        .into_iter()
        .map(|c| Stamped { tick: c.tick, command: PlayerCommand::from_message(&NetMessage { kind: c.kind, player: c.player, wparam: c.wparam, lparam: c.lparam, data: c.data }) })
        .collect();
    let mut world = b.world.into_world(rules);
    let hash = world.state_hash();
    if hash != header.world_hash {
        bail!("{}: the loaded world differs from the saved one (hash {hash:016x}, saved {:016x}; made by an incompatible build)", path.display(), header.world_hash);
    }
    // Saves of builds that left a destroyed building's role behind for the next entity at its
    // index (a new site there was never supplied; playtest P4).
    let stale = s4_sim::buildings::drop_stale_roles(&mut world);
    if stale != 0 {
        tracing::info!("{}: dropped {stale} roles of destroyed buildings", path.display());
    }
    Ok(Loaded { header, world, script: b.script, host: b.host, commands, client: b.client })
}

/// The save folder (`%APPDATA%\s4rs\saves`).
/// `S4RS_SAVE_DIR` overrides it (tests).
pub fn dir() -> Option<std::path::PathBuf> {
    if let Some(d) = std::env::var_os("S4RS_SAVE_DIR") {
        return Some(d.into());
    }
    crate::progress::path().and_then(|p| p.parent().map(|d| d.join("saves")))
}
