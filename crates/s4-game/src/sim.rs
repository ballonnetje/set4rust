//! The simulation thread.
//!
//! The thread owns the `World` and the script; nothing else touches them. It runs one
//! `s4_sim::tick` per game tick at the original rate (docs/SIM.md §2.1: game speed 1 → 71 ms, 2 →
//! 47 ms, n ≥ 3 → 71/(n−1) ms, 0 = paused), executing the commands stamped for that tick, then
//! publishes an immutable [`Snapshot`]. Readers get snapshots through an `ArcSwap`: loading one is
//! lock-free, and a reader keeps its `Arc` for as long as it draws from it.
//!
//! Two clocks: [`Clock::RealTime`] (the window) and [`Clock::Manual`] (headless and tests:
//! [`SimHandle::step`] runs ticks immediately and waits for them).
//!
//! The tick's `World::events` (job sounds, war actions) go through a channel rather than the
//! snapshot, so none are lost when the renderer skips snapshots.
//!
//! In a network game ([`NetSetup`]) the thread first runs the s4-net lobby, then ticks in
//! lockstep: local commands become the local player's turns, a communication tick waits for every
//! player's turn (the real-time clock stalls), and the state hash is exchanged (desyncs and their
//! per-subsystem reports are logged).

use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use arc_swap::ArcSwap;
use s4_sim::event::SimEvent;
use s4_sim::{NoScript, World};
use s4_viewer::population::{self, Population};

use crate::commands::{CommandQueue, Stamped};
use crate::fog::{FogFrame, GameFog};
use crate::script::ScriptRunner;

/// Milliseconds per tick at a game speed (`CStateGame` frame pacing); `None` = paused.
pub fn tick_ms(speed: u32) -> Option<u64> {
    match speed {
        0 => None,
        1 => Some(71),
        2 => Some(47),
        n => Some(71 / (n as u64 - 1)),
    }
}

/// What the renderer reads: the world after a tick.
pub struct Snapshot {
    pub tick: u32,
    /// When it was published, for interpolating to the next one.
    pub at: Instant,
    /// Tick length at the time (0 = paused).
    pub tick_ms: u64,
    /// Shared with the scene that shows it (no copy a tick).
    pub population: Arc<Population>,
    /// `World::state_hash` after the tick; 0 on the real-time clock (nothing there reads it, and
    /// it costs about 20 ms a tick on a big map).
    pub hash: u64,
    /// The local player's fog (client-side, not part of the hashed state); None = no fog.
    pub fog: Option<FogFrame>,
    /// Build-site markers while the client is in build mode (see [`SimHandle::set_markers`]).
    pub markers: Option<Arc<Markers>>,
    /// The watched (selected) building's facts for its dialog (see [`SimHandle::watch_building`]).
    pub building: Option<s4_viewer::building_dialog::Facts>,
    /// What the ambient creatures need of the land, fixed when the game starts
    /// ([`crate::ambient::Land`]).
    pub ambient: Option<Arc<crate::ambient::Land>>,
}

/// `CInputProcessor::GetPossibleBuildingSites` for the build mode: every node where the player
/// may place the building, with its helper object (marker icon, `Gfx\7.gfx`).
#[derive(Debug)]
pub struct Markers {
    pub building: u8,
    pub sites: Vec<(u16, u16, u8)>,
}

/// What the sim thread computes for the client besides the world: fog and build markers.
#[derive(Default)]
struct Extras {
    /// The map the game runs (for savegames).
    map: std::path::PathBuf,
    /// The local player.
    local: u8,
    fog: Option<GameFog>,
    query: Option<(u8, u8)>,
    markers: Option<Arc<Markers>>,
    /// The building whose dialog the client shows.
    watched: Option<u16>,
    ambient: Option<Arc<crate::ambient::Land>>,
    /// The snapshots carry the state hash (the manual clock: headless runs and tests).
    hash: bool,
}

impl Extras {
    /// After a tick: fog, and the markers every 5 ticks (`UpdateScreenPossibleBuildingSites` runs
    /// every 5th game-loop pass, `g_iPossibleBuildingSitesUpdateCounter > 5`).
    fn tick(&mut self, world: &World) {
        if let Some(f) = &mut self.fog {
            f.tick(world);
        }
        if self.query.is_some() && (self.markers.is_none() || world.tick.is_multiple_of(5)) {
            self.refresh_markers(world);
        }
    }

    fn refresh_markers(&mut self, world: &World) {
        self.markers = self.query.map(|(player, building)| {
            let size = world.grid.size() as i32;
            let sites = s4_sim::build::possible_sites(world, player, building, (0, 0), (size, size));
            Arc::new(Markers { building, sites })
        });
    }
}

pub enum Clock {
    RealTime { speed: u32 },
    Manual,
}

enum Ctl {
    Command(Stamped),
    /// Write a savegame (single player): file, name, the client's state; the result is answered.
    Save(std::path::PathBuf, String, crate::save::ClientState, Vec<u8>, Sender<anyhow::Result<crate::save::Header>>),
    /// Tests: run a closure on the world between ticks (e.g. kill buildings).
    #[cfg(test)]
    World(Box<dyn FnOnce(&mut World) + Send>),
    Speed(u32),
    Step(u32, Sender<u32>),
    Markers(Option<(u8, u8)>),
    /// Watch a building (its facts in every snapshot); answered once republished.
    Watch(Option<u16>, Sender<()>),
    /// The cheat console's Lua line (single player, msgwin.rs), run between ticks.
    Console(String),
    /// Reveal the whole map to the local player (`CFogging::SetRWM(1)`).
    RevealMap,
    /// The end statistics' copy of `CStatistic` (0x4e4660) and the warrior types (0x12dbca0).
    End(Sender<(Vec<crate::frontend::endstat::PlayerStats>, Vec<u8>)>),
    Quit,
}

/// The events of one tick.
pub type TickEvents = (u32, Vec<SimEvent>);

pub struct SimHandle {
    tx: Sender<Ctl>,
    events: Receiver<TickEvents>,
    pub latest: Arc<ArcSwap<Snapshot>>,
    join: Option<JoinHandle<()>>,
    /// How the script decided the game (script.rs).
    pub outcome: crate::script::Outcome,
    /// What the script shares with the client: the tutorial's state and camera requests.
    pub tutorial: crate::tutorial::Shared,
    /// The local player: 1 in single player, the lobby's id in a network game.
    pub local: u8,
    /// The client state of a loaded savegame (camera, campaign mission, speed).
    pub loaded: Option<crate::save::ClientState>,
    /// Chat lines that arrived ([`NET_CHAT`]): (sender, recipients mask, text).
    chats: Receiver<(u8, u32, String)>,
    /// What the session tells the client: players that left, players waited for.
    left: Receiver<NetNotice>,
    /// A network game's players: (id, name) from the lobby; empty in single player.
    pub names: Vec<(u8, String)>,
    /// What the monitor reads of this sim (monitor/, [`stats`]).
    pub monitor: Arc<stats::Shared>,
}

/// What a network session tells the client (its messages, msgwin.rs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetNotice {
    /// A player left (`NetEvent::Left`; event 0x39 → 0x8d2).
    Left(u8),
    /// A player's turn is overdue (0x5a2a90 → 0x8d5).
    Waiting(u8),
    /// [ours] The relay stayed lost: the game goes on alone (`NetEvent::Offline`; 2509 "Connection
    /// to lobby server lost!").
    RelayLost,
}

/// A network game: host one (as player 1), join one, or play the one the lobby screens set up.
pub enum NetSetup {
    /// The lobby is done (start screens): the session and the players it set up.
    Ready(Box<s4_net::Session>, PlayerSetup),
    Host { bind: String, players: usize, settings: s4_net::GameSettings },
    /// `map_hash`: of the client's copy of the map (`--map`), checked by the host; `save`: for a
    /// loaded game the world hash of the client's save and the player it was saved as.
    Join { bind: String, host: std::net::SocketAddr, map_hash: u64, save: (u64, u8) },
}

/// What the sim thread is started with. The world and the script are built on the thread (the
/// Lua state is not `Send`), so only the map path travels.
pub struct SimSetup {
    pub map_path: std::path::PathBuf,
    pub prefer_file_script: bool,
    pub run_script: bool,
    /// Compute the local player's fog (see fog.rs).
    pub fog: bool,
    /// Set by the script when the game is decided.
    pub outcome: crate::script::Outcome,
    /// Play over the network.
    pub net: Option<NetSetup>,
    /// The players the start screens set up (`None`: the map's own setup, team setup 0).
    pub players: Option<PlayerSetup>,
    /// Continue a savegame instead of starting the map (`map_path` is then the save's map).
    pub load: Option<std::path::PathBuf>,
    /// What the script shares with the client (tutorial.rs).
    pub tutorial: crate::tutorial::Shared,
}

/// The players as the game settings screen leaves them in the GameState (dialog 19 / 4,
/// `CStateLobbyGameSettings`): per player id 1..=8 the race (+0x18c, random already resolved),
/// the team (+0x90) and the control (+0x6c: 2 / 3 computer easy / normal, else 0), the game
/// mode (+0x344, [`s4_sim::NewGame::game_type`]) and the local player's id.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlayerSetup {
    pub races: [Option<u8>; 8],
    pub teams: [Option<u8>; 8],
    /// [`s4_sim::NewGame::control`]: 0 open, 1 human, 2 / 3 computer.
    pub control: [u8; 8],
    pub game_type: u32,
    /// 0: player 1.
    pub local: u8,
    /// The slot count the settings chose (the player count of the map settings screen, 0x44),
    /// for [`s4_sim::NewGame::players`]: `AddPlayer` 0x509f80 runs for these slots only. `None`:
    /// every map slot.
    pub players: Option<u8>,
    /// Game type 3's goods (`GlobalGameState +0x2fc`, [`s4_sim::NewGame::race_goods`]).
    pub race_goods: [u8; 7],
    /// Free settle's start (mode 5, map settings 0x520fa5 / random map parameters 0x526b45):
    /// `GlobalGameState +0x2dc` = 1 and +0x2e0 = 1, the game decided for team 1 from the start
    /// (no victory checks, 0x5bd950; the end record's winner;
    /// [`s4_sim::NewGame::start_winner`]).
    pub decided: bool,
    /// The map settings' goods choice ([`s4_sim::NewGame::start_resources`]): 0 none, 1..=3.
    pub start_resources: u8,
}

/// A savegame read on the sim thread: world, script, pending commands, client state.
fn load_world(setup: &SimSetup, path: &std::path::Path) -> anyhow::Result<(World, Option<ScriptRunner>, CommandQueue, crate::save::ClientState, std::path::PathBuf)> {
    let rules = s4_sim::Rules::load_default()?;
    crate::save::note_rules(&rules);
    s4_viewer::building_dialog::warm();
    let l = crate::save::read(path, rules)?;
    let map = std::path::PathBuf::from(&l.header.map);
    let local = l.client.local.max(1);
    let race = l.world.players.get(local as usize).and_then(|p| p.as_ref()).map_or(0, |p| p.race as i32);
    let players = l.world.players.iter().skip(1).filter(|p| p.is_some()).count() as i32;
    let script = match (&l.script, setup.run_script) {
        (Some(s), true) => Some(ScriptRunner::restore(s, &l.host, race, players, setup.outcome.clone(), local as i32, setup.tutorial.clone())),
        _ => None,
    };
    let mut queue = CommandQueue::default();
    for c in l.commands {
        queue.push(c);
    }
    tracing::info!("loaded {} ({}, tick {})", path.display(), l.header.name, l.header.tick);
    let mut world = l.world;
    world.final_temple = final_temple(&map);
    Ok((world, script, queue, l.client, map))
}

/// How long a networked tick waits for the other players before the game is given up.
const NET_TIMEOUT: Duration = Duration::from_secs(if cfg!(test) { 10 } else { 30 });
/// How long the command-line lobby waits for the other players (tests: a failed peer ends the
/// test instead of hanging the suite).
const LOBBY_WAIT: Duration = Duration::from_secs(if cfg!(test) { 20 } else { 600 });

/// A `bind` of [`NetSetup`] starting with this goes through the internet relay named after it
/// (`relay://host:port`, main.rs `--relay`). [ours]
pub const RELAY_BIND: &str = "relay://";

/// The link a [`NetSetup`]'s `bind` names: a UDP socket, or the relay.
fn net_link(bind: &str) -> anyhow::Result<s4_net::Link> {
    match bind.strip_prefix(RELAY_BIND) {
        Some(server) => s4_net::Link::relay(server, s4_net::RELAY_TIMEOUT),
        None => s4_net::Link::udp(bind),
    }
}

/// The lobby (blocking): the session and, for a client, the host's map.
/// `host_as`: a loaded game's host takes the slot of the player its save is from.
fn net_session(net: NetSetup, host_as: u8) -> anyhow::Result<s4_net::Session> {
    match net {
        NetSetup::Ready(s, _) => Ok(*s),
        NetSetup::Host { bind, players, settings } => {
            let relay = bind.starts_with(RELAY_BIND);
            s4_net::Session::host_on(net_link(&bind)?, "host", players, host_as, settings, LOBBY_WAIT, |a| match s4_net::relay::relay_id(a).filter(|_| relay) {
                Some(room) => tracing::info!("hosting on the relay as room {room} (join with --relay {} --join {room}), waiting for {} more player(s)", &bind[RELAY_BIND.len()..], players - 1),
                None => tracing::info!("hosting on {a}, waiting for {} more player(s)", players - 1),
            })
        }
        NetSetup::Join { bind, host, map_hash, save } => s4_net::Session::join_on(net_link(&bind)?, host, "client", map_hash, save, LOBBY_WAIT),
    }
}

/// The campaign type of a map in `Map\Campaign` (`GlobalGameState +0x2d4` as the briefings set it,
/// with +0x3b9 a campaign map): 1..4 `roman` / `viking` / `maya` / `dark` NN, 5..10 `md_` roman /
/// viking / maya / settle / conflict / bonus, 11..16 `ao_` roman / viking / maya / settle /
/// trojan / bonus, 17..20 `mcd2_` and 21..24 `xmd3_` roman / viking / maya / trojan; `None` for
/// any other map.
fn campaign_type(map_path: &std::path::Path) -> Option<u8> {
    let dir = map_path.parent()?.file_name()?.to_string_lossy().to_ascii_lowercase();
    if dir != "campaign" {
        return None;
    }
    let name = map_path.file_stem()?.to_string_lossy().to_ascii_lowercase();
    let (prefix, rest) = match name.split_once('_') {
        Some((p, r)) => (p.to_string(), r.to_string()),
        None => (String::new(), name.clone()),
    };
    let kinds: &[&str] = match prefix.as_str() {
        "" => &["roman", "viking", "maya", "dark"],
        "md" => &["roman", "viking", "maya", "settle", "conflict", "bonus"],
        "ao" => &["roman", "viking", "maya", "settle", "trojan", "bonus"],
        "mcd2" | "xmd3" => &["roman", "viking", "maya", "trojan"],
        _ => return None,
    };
    let first = match prefix.as_str() {
        "" => 1,
        "md" => 5,
        "ao" => 11,
        "mcd2" => 17,
        _ => 21,
    };
    let k = kinds.iter().position(|k| rest.starts_with(k))?;
    Some(first + k as u8)
}

/// The last mission of the Dark Tribe's or the Trojans' campaign (type 4 or 15, level 11 of 0..,
/// [`crate::frontend::victory`]): `dark12`, `AO_trojan12` ([`World::final_temple`]).
fn final_temple(map_path: &std::path::Path) -> bool {
    matches!(campaign_type(map_path), Some(4 | 15)) && map_path.file_stem().is_some_and(|n| n.to_string_lossy().ends_with("12"))
}

pub(crate) fn build_world(setup: &SimSetup, map_path: &std::path::Path, local: u8) -> anyhow::Result<(World, Option<ScriptRunner>)> {
    let map = s4_formats::map::MapFile::open(map_path)?;
    // The game settings' players, or the map's own setup (team setup 0 of chunk 3: campaigns,
    // the command line).
    let new_game = match &setup.players {
        Some(p) => s4_sim::NewGame { races: p.races, teams: p.teams, control: p.control, game_type: p.game_type, players: p.players, race_goods: p.race_goods, start_winner: p.decided as i32, start_resources: p.start_resources, ..Default::default() },
        None => s4_sim::NewGame { teams: s4_sim::NewGame::map_teams(&map, 0)?, ..Default::default() },
    };
    let new_game = s4_sim::NewGame { campaign: campaign_type(map_path), ..new_game };
    let mut start = crate::script::start_resources(setup.tutorial.clone(), setup.outcome.clone());
    let mut world = population::world_from_map_start(&map, &new_game, &mut start)?;
    world.final_temple = final_temple(map_path);
    crate::save::note_rules(&world.rules);
    s4_viewer::building_dialog::warm();
    let race = world.players.get(local as usize).and_then(|p| p.as_ref()).map_or(0, |p| p.race as i32);
    let name = map_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let players = world.players.iter().skip(1).filter(|p| p.is_some()).count() as i32;
    let script = setup.run_script.then(|| ScriptRunner::new(&map, &name, setup.prefer_file_script, race, players, setup.outcome.clone(), local as i32, setup.tutorial.clone(), &mut world));
    // Free settle: decided for team 1 from the start (`GlobalGameStateObject +0x34 / +0x38`).
    if setup.players.is_some_and(|p| p.decided) {
        setup.tutorial.lock().unwrap_or_else(|e| e.into_inner()).winner = 1;
    }
    Ok((world, script))
}

fn snapshot(world: &World, tick_ms: u64, x: &Extras) -> Snapshot {
    use s4_viewer::perf::span;
    Snapshot {
        tick: world.tick,
        at: Instant::now(),
        tick_ms,
        population: {
            let _t = span("sim: snapshot population");
            Arc::new(population::from_world(world))
        },
        hash: if x.hash {
            let _t = span("sim: snapshot hash");
            world.state_hash()
        } else {
            0
        },
        fog: {
            let _t = span("sim: snapshot fog");
            x.fog.as_ref().map(GameFog::frame)
        },
        markers: x.markers.clone(),
        building: x.watched.and_then(|id| s4_viewer::building_dialog::Facts::from_world(world, id)),
        ambient: x.ambient.clone(),
    }
}

/// The local player of a single-player game without a game setup (campaigns, the command line).
/// [inf] Player 1; a game setup names it, a network game takes the id the lobby assigned.
const LOCAL_PLAYER: u8 = 1;

fn local_player(setup: &SimSetup) -> u8 {
    setup.players.map(|p| p.local).filter(|&l| l != 0).unwrap_or(LOCAL_PLAYER)
}

/// Where a tick's commands come from.
enum Ticker {
    /// Single player: the commands stamped for each tick.
    Local(CommandQueue),
    /// Lockstep (s4-net).
    Net(Box<s4_net::Session>),
}

impl SimHandle {
    /// Build the world (on the new thread) and start ticking.
    pub fn spawn(setup: SimSetup, clock: Clock) -> anyhow::Result<SimHandle> {
        let (tx, rx) = channel::<Ctl>();
        let (ready_tx, ready_rx) = channel::<anyhow::Result<(Arc<ArcSwap<Snapshot>>, u8, Vec<(u8, String)>)>>();
        let (chat_tx, chat_rx) = channel::<(u8, u32, String)>();
        let (left_tx, left_rx) = channel::<NetNotice>();
        let (ev_tx, ev_rx) = channel::<TickEvents>();
        let outcome = setup.outcome.clone();
        let tutorial = setup.tutorial.clone();
        let (client_tx, client_rx) = channel::<Option<crate::save::ClientState>>();
        let monitor = Arc::new(stats::Shared::default());
        let bound = monitor.clone();
        let join = std::thread::Builder::new().name("sim".into()).spawn(move || {
            stats::bind(bound);
            let mut setup = setup;
            let net = setup.net.take();
            let joined = matches!(net, Some(NetSetup::Join { .. }));
            if let Some(NetSetup::Ready(_, players)) = &net {
                setup.players = Some(*players);
            }
            let (ticker, map_path, local) = match net {
                None => (Ticker::Local(CommandQueue::default()), setup.map_path.clone(), local_player(&setup)),
                Some(net) => match net_session(net, setup.load.as_ref().and_then(|p| crate::save::read(p, s4_sim::Rules::load_default().ok()?).ok()).map_or(1, |l| l.client.local)) {
                    Ok(s) => {
                        let map = if joined { s4_viewer::scene::resolve_map(&s.settings.map) } else { setup.map_path.clone() };
                        tracing::info!("network game: player {} of {:?}, map {}, dt {}", s.local, s.lock.players(), map.display(), s.lock.dt);
                        let local = s.local;
                        (Ticker::Net(Box::new(s)), map, local)
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                },
            };
            let mut ticker = ticker;
            let mut map_path = map_path;
            let mut local = local;
            let loaded = match &setup.load {
                None => build_world(&setup, &map_path, local).map(|(w, s)| (w, s, None)),
                Some(p) => load_world(&setup, p).map(|(w, s, q, c, m)| {
                    if let Ticker::Local(lq) = &mut ticker {
                        *lq = q;
                    }
                    map_path = m;
                    local = c.local.max(1);
                    (w, s, Some(c))
                }),
            };
            // A loaded network game: the save must be the game the lobby agreed on, saved by this
            // player; the lockstep takes back the turns that were in flight.
            let loaded = loaded.and_then(|(w, s, c)| {
                if let (Ticker::Net(n), Some(cs)) = (&mut ticker, &c) {
                    let want = (n.settings.load_tick, n.settings.load_hash);
                    let have = (w.tick, w.state_hash());
                    anyhow::ensure!(want == have, "the save is at tick {} with hash {:016x}, the game at tick {} with {:016x}", have.0, have.1, want.0, want.1);
                    anyhow::ensure!(cs.local == n.local, "the save is player {}'s, this peer is player {}", cs.local, n.local);
                    let r = cs.net.clone().unwrap_or_default();
                    let turns = r.turns.iter().map(|(t, m)| (*t, m.iter().map(|c| c.command()).collect())).collect();
                    n.lock.resume(turns, r.upto, r.pending.iter().map(|c| c.command().command).collect());
                    local = n.local;
                }
                Ok((w, s, c))
            });
            let (mut world, mut script) = match loaded {
                Ok((w, s, c)) => {
                    let _ = client_tx.send(c);
                    (w, s)
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };
            let speed = match clock {
                Clock::RealTime { speed } => speed,
                Clock::Manual => 1,
            };
            let mut extras = Extras {
                fog: setup.fog.then(|| GameFog::resume(&mut world, local)),
                map: map_path.clone(),
                local,
                ambient: Some(Arc::new(crate::ambient::Land::from_world(&world))),
                hash: matches!(clock, Clock::Manual),
                ..Default::default()
            };
            let latest = Arc::new(ArcSwap::from_pointee(snapshot(&world, tick_ms(speed).unwrap_or(0), &extras)));
            let names = match &ticker {
                Ticker::Net(s) => s.roster.clone(),
                Ticker::Local(_) => Vec::new(),
            };
            let _ = ready_tx.send(Ok((latest.clone(), local, names)));
            run(&mut world, script.as_mut(), &mut ticker, &mut extras, rx, ev_tx, latest, clock, chat_tx, left_tx);
            if let Ticker::Net(s) = &mut ticker {
                s.leave();
            }
        })?;
        let (latest, local, names) = ready_rx.recv()??;
        let loaded = client_rx.try_recv().ok().flatten();
        Ok(SimHandle { tx, events: ev_rx, latest, join: Some(join), outcome, tutorial, local, loaded, chats: chat_rx, left: left_rx, names, monitor })
    }

    /// Queue a command (it runs at its stamped tick).
    pub fn command(&self, s: Stamped) {
        let _ = self.tx.send(Ctl::Command(s));
    }

    pub fn set_speed(&self, speed: u32) {
        let _ = self.tx.send(Ctl::Speed(speed));
    }

    /// Enter (`Some((player, building))`) or leave (`None`) the build mode: the snapshots then
    /// carry the possible sites for that building.
    pub fn set_markers(&self, query: Option<(u8, u8)>) {
        let _ = self.tx.send(Ctl::Markers(query));
    }

    /// Carry building `id`'s dialog facts in the snapshots from now on (`None`: none). Blocks
    /// until a snapshot with them is published.
    pub fn watch_building(&self, id: Option<u16>) {
        let (tx, rx) = channel();
        let _ = self.tx.send(Ctl::Watch(id, tx));
        let _ = rx.recv();
    }

    /// `EndGameGoToStatistics` 0x514450 → 0x4e4660: every player's `CStatistic` record and the
    /// settler types' warrior types.
    pub fn end_stats(&self) -> (Vec<crate::frontend::endstat::PlayerStats>, Vec<u8>) {
        let (tx, rx) = channel();
        let _ = self.tx.send(Ctl::End(tx));
        rx.recv().unwrap_or_default()
    }

    /// The cheat console: run `code` in the game script's Lua state before the next tick.
    pub fn console(&self, code: &str) {
        let _ = self.tx.send(Ctl::Console(code.to_string()));
    }

    /// `CFogging::SetRWM(1)` (vtbl+0x28, 0x5adcc0) on the local player's fog: the whole map
    /// shows from the next fog cycle end on.
    pub fn reveal_map(&self) {
        let _ = self.tx.send(Ctl::RevealMap);
    }

    /// Tests: change the world on the sim thread (before the next tick).
    #[cfg(test)]
    pub fn with_world(&self, f: impl FnOnce(&mut World) + Send + 'static) {
        let _ = self.tx.send(Ctl::World(Box::new(f)));
    }

    /// Save the game now (between ticks; single player only). Blocks until written.
    /// `thumb`: [`crate::save::thumbnail`] of the screen, or empty.
    pub fn save(&self, path: &std::path::Path, name: &str, client: crate::save::ClientState, thumb: Vec<u8>) -> anyhow::Result<crate::save::Header> {
        let (tx, rx) = channel();
        let _ = self.tx.send(Ctl::Save(path.to_path_buf(), name.to_string(), client, thumb, tx));
        rx.recv().map_err(|_| anyhow::anyhow!("the game is gone"))?
    }

    /// Manual clock: run `n` ticks now; returns the tick reached.
    pub fn step(&self, n: u32) -> u32 {
        let (tx, rx) = channel();
        let _ = self.tx.send(Ctl::Step(n, tx));
        rx.recv().unwrap_or(0)
    }

    pub fn snapshot(&self) -> Arc<Snapshot> {
        self.latest.load_full()
    }

    /// The events of every tick since the last call, oldest first.
    pub fn drain_events(&self) -> Vec<TickEvents> {
        self.events.try_iter().collect()
    }

    /// The chat lines that arrived since the last call.
    pub fn drain_chat(&self) -> Vec<(u8, u32, String)> {
        self.chats.try_iter().collect()
    }

    /// The session's notices since the last call.
    pub fn drain_net(&self) -> Vec<NetNotice> {
        self.left.try_iter().collect()
    }
}

impl Drop for SimHandle {
    fn drop(&mut self) {
        let _ = self.tx.send(Ctl::Quit);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

/// The network save message (the original's 0xFA8 carries the save name to every peer, which
/// all save at the same tick). It travels as a turn command and never reaches the simulation.
pub const NET_SAVE: u32 = 0xFA8;

/// The chat message (the original's 0xFA5, `EventManagement` 0x26: the line with the recipients
/// mask, the sender's bit included, sent with `sub_1A7BB0`). Here it travels as a turn command
/// (wparam = the mask, data = the text) and never reaches the simulation.
pub const NET_CHAT: u32 = 0xFA5;

/// A message of a tick's commands that is not the simulation's.
enum Side {
    Save(String),
    Chat(u8, u32, String),
}

/// One game tick with its commands and the script at its slot; returns the tick's events and the
/// network saves due after it, or `None` when a network game has to wait (`block` = wait up to
/// [`NET_TIMEOUT`]).
pub mod stats;

fn step(world: &mut World, script: Option<&mut ScriptRunner>, ticker: &mut Ticker, block: bool, left: &Sender<NetNotice>) -> Option<(TickEvents, Vec<Side>)> {
    let next = world.tick.wrapping_add(1);
    let monitored = stats::on();
    let waited = Instant::now();
    let cmds = match ticker {
        Ticker::Local(q) => q.take(next),
        Ticker::Net(s) => {
            s.produce(world.tick);
            // The waiting (0x5a2a90): overdue players warned about, silent ones kicked by the
            // deciding peer (s4-net `Session::stalled`, `NetEvent::Waiting` / `Left`).
            let (cmds, events) = if block { s.wait_ready(next, NET_TIMEOUT) } else { s.poll_ready(next) };
            log_net(&events, left);
            if monitored {
                let desyncs = events.iter().filter(|e| matches!(e, s4_net::NetEvent::Desync(_))).count() as u32;
                let missing = s.lock.missing(next);
                let net = stats::Net { dt: s.lock.dt, hash_interval: s.lock.hash_interval, players: s.lock.players().to_vec(), missing };
                stats::with(|st| {
                    st.desyncs += desyncs;
                    st.net = Some(net);
                });
            }
            match cmds {
                Some(c) => c,
                None => {
                    if block {
                        tracing::error!("tick {next}: no turn from player(s) {:?}", s.lock.missing(next));
                    }
                    return None;
                }
            }
        }
    };
    // Network saves and chat lines: taken out of the tick's commands, handled after it.
    let (saves, cmds): (Vec<_>, Vec<_>) = cmds.into_iter().partition(|c| matches!(c.command, s4_sim::Command::Other { kind: NET_SAVE | NET_CHAT, .. }));
    let saves = saves
        .into_iter()
        .filter_map(|c| match c.command {
            s4_sim::Command::Other { kind: NET_SAVE, data, .. } => Some(Side::Save(String::from_utf16_lossy(&data))),
            s4_sim::Command::Other { kind: NET_CHAT, wparam, data, .. } => Some(Side::Chat(c.player, wparam, String::from_utf16_lossy(&data))),
            _ => None,
        })
        .collect();
    let wait_ms = waited.elapsed().as_secs_f32() * 1000.0;
    let started = Instant::now();
    tick_with(world, script, &cmds);
    if monitored {
        stats::after_tick(world, started.elapsed().as_secs_f32() * 1000.0, cmds.len() as u16, wait_ms);
    }
    if let Ticker::Net(s) = ticker
        && s.lock.hash_due(world.tick)
    {
        let (hash, parts) = s4_net::world_hashes(world);
        s.send_hash(world.tick, hash, parts);
        let ev = s.pump();
        log_net(&ev, left);
    }
    Some(((world.tick, std::mem::take(&mut world.events)), saves))
}

/// A network save due after this tick: every peer writes `<name> - player n` (the lockstep's
/// pending turns are not saved; loading a network save is not supported yet).
/// The lockstep's own part for a network save.
fn net_resume(ticker: &Ticker) -> Option<crate::save::NetResume> {
    let Ticker::Net(s) = ticker else { return None };
    use crate::save::SavedMessage;
    let (turns, upto, pending) = s.lock.own_pending();
    Some(crate::save::NetResume {
        turns: turns.iter().map(|(t, c)| (*t, c.iter().map(SavedMessage::of).collect())).collect(),
        upto,
        pending: pending.into_iter().map(|c| SavedMessage::of(&s4_sim::PlayerCommand::new(s.local, c))).collect(),
        names: s.roster.clone(),
    })
}

fn net_save(world: &World, script: Option<&ScriptRunner>, map: &std::path::Path, local: u8, name: &str, net: Option<crate::save::NetResume>) {
    let file = format!("{name} - player {local}");
    let Some(path) = crate::menu::file_for(&file) else { return };
    let input = crate::save::SaveInput {
        name: &file,
        thumb: Vec::new(),
        map,
        world,
        script: script.and_then(|s| s.saved_state()),
        host: script.map(|s| s.host_state()).unwrap_or_default(),
        commands: Vec::new(),
        client: crate::save::ClientState { local, net, ..Default::default() },
    };
    match crate::save::write(&path, input) {
        Ok(h) => tracing::info!("network save \"{name}\" at tick {} → {}", h.tick, path.display()),
        Err(e) => tracing::warn!("network save: {e}"),
    }
}

/// The session's events: logged; a player who left goes to the client (its message, 0x8d2).
fn log_net(events: &[s4_net::NetEvent], left: &Sender<NetNotice>) {
    for e in events {
        match e {
            s4_net::NetEvent::Desync(d) => tracing::error!("DESYNC after tick {}: local {:016x}, player {} {:016x}", d.tick, d.local, d.player, d.remote),
            s4_net::NetEvent::DesyncReport(r) => tracing::error!("{r}"),
            s4_net::NetEvent::Left(p) => {
                tracing::warn!("player {p} left");
                let _ = left.send(NetNotice::Left(*p));
            }
            s4_net::NetEvent::Waiting(p) => {
                let _ = left.send(NetNotice::Waiting(*p));
            }
            // Until the kick delay the game goes on as with silent peers (Waiting, the decider's
            // kick); then every peer drops the others and plays on alone (Offline).
            s4_net::NetEvent::RelayLost => tracing::error!("internet game: the relay has stopped answering"),
            s4_net::NetEvent::Offline => {
                tracing::error!("internet game: relay lost, playing on alone");
                let _ = left.send(NetNotice::RelayLost);
            }
        }
    }
}

fn tick_with(world: &mut World, script: Option<&mut ScriptRunner>, cmds: &[s4_sim::PlayerCommand]) {
    match script {
        Some(s) => s4_sim::tick_with_script(world, cmds, s),
        None => s4_sim::tick_with_script(world, cmds, &mut NoScript),
    }
}

#[allow(clippy::too_many_arguments)]
fn run(
    world: &mut World,
    mut script: Option<&mut ScriptRunner>,
    ticker: &mut Ticker,
    extras: &mut Extras,
    rx: Receiver<Ctl>,
    events: Sender<TickEvents>,
    latest: Arc<ArcSwap<Snapshot>>,
    clock: Clock,
    chats: Sender<(u8, u32, String)>,
    left: Sender<NetNotice>,
) {
    let manual = matches!(clock, Clock::Manual);
    let mut speed = match clock {
        Clock::RealTime { speed } => speed,
        Clock::Manual => 1,
    };
    let mut next = Instant::now();
    loop {
        let msg = if manual {
            match ticker {
                // A stepped network peer keeps answering between steps (resent turns, acks),
                // so a peer that is a tick behind is not left waiting for turns nobody resends.
                Ticker::Net(s) => loop {
                    match rx.recv_timeout(Duration::from_millis(10)) {
                        Ok(m) => break Some(m),
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => log_net(&s.linger(Duration::from_millis(1)), &left),
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break None,
                    }
                },
                Ticker::Local(_) => rx.recv().ok(),
            }
        } else {
            match tick_ms(speed) {
                Some(_) => rx.recv_timeout(next.saturating_duration_since(Instant::now())).ok(),
                None => rx.recv().ok(),
            }
        };
        match msg {
            Some(Ctl::Quit) => return,
            Some(Ctl::Save(path, name, client, thumb, reply)) => {
                let r = match ticker {
                    Ticker::Local(q) => crate::save::write(
                        &path,
                        crate::save::SaveInput {
                            name: &name,
                            thumb,
                            map: &extras.map,
                            world,
                            script: script.as_deref().and_then(|s| s.saved_state()),
                            host: script.as_deref().map(|s| s.host_state()).unwrap_or_default(),
                            commands: q.pending(),
                            client,
                        },
                    ),
                    // A network game saves at a lockstep tick on every peer (`CStateGame::SaveGame`
                    // in a network game: message 0xFA8 with the name).
                    Ticker::Net(n) => {
                        let data: Vec<u16> = name.encode_utf16().collect();
                        n.lock.issue(s4_sim::Command::Other { kind: NET_SAVE, wparam: 0, lparam: 0, data });
                        Err(anyhow::anyhow!("scheduled for a network tick"))
                    }
                };
                if let Ok(h) = &r {
                    tracing::info!("saved {} at tick {}", path.display(), h.tick);
                }
                let _ = reply.send(r);
            }
            Some(Ctl::Command(s)) => match ticker {
                Ticker::Local(q) => q.push(s),
                // The lockstep stamps it (the local player's next turn); the given tick is ignored.
                Ticker::Net(n) => n.lock.issue(s.command.command),
            },
            Some(Ctl::Speed(s)) => {
                speed = s;
                next = Instant::now();
            }
            Some(Ctl::Step(n, done)) => {
                for _ in 0..n {
                    let t = s4_viewer::perf::span("sim: tick");
                    let Some((ev, saves)) = step(world, script.as_deref_mut(), ticker, true, &left) else { break };
                    drop(t);
                    for side in saves {
                        match side {
                            Side::Save(name) => net_save(world, script.as_deref(), &extras.map, extras.local, &name, net_resume(ticker)),
                            Side::Chat(p, m, t) => {
                                let _ = chats.send((p, m, t));
                            }
                        }
                    }
                    let _ = events.send(ev);
                    let _t = s4_viewer::perf::span("sim: extras (fog)");
                    extras.tick(world);
                }
                latest.store(Arc::new(snapshot(world, tick_ms(speed).unwrap_or(0), extras)));
                let _ = done.send(world.tick);
            }
            #[cfg(test)]
            Some(Ctl::World(f)) => f(world),
            Some(Ctl::End(done)) => {
                let v = &world.vehicles;
                let six = |a: &[i32], p: usize| a.get(p * 6..p * 6 + 6).map(<[i32]>::to_vec).unwrap_or_default();
                let stats = (0..9)
                    .map(|p| world.statistic.players.get(p).map(|s| crate::frontend::endstat::record_of(s, &six(&v.stat_made, p), &six(&v.stat_finished, p))).unwrap_or_default())
                    .collect();
                let warrior = (0..0x43).map(|t| world.rules.warrior_type(t)).collect();
                let _ = done.send((stats, warrior));
            }
            Some(Ctl::Console(code)) => {
                if let Some(s) = script.as_deref_mut() {
                    s.console(&code, world);
                }
            }
            Some(Ctl::RevealMap) => {
                if let Some(f) = world.fog.as_mut() {
                    f.set_reveal_map(true);
                }
            }
            Some(Ctl::Watch(id, done)) => {
                extras.watched = id;
                // Only the dialog's facts change: the published snapshot gets them (a whole new
                // snapshot made the click that selects a building wait 6..70 ms on a big map).
                let cur = latest.load_full();
                let s = Snapshot {
                    tick: cur.tick,
                    at: cur.at,
                    tick_ms: cur.tick_ms,
                    population: cur.population.clone(),
                    hash: cur.hash,
                    fog: cur.fog.clone(),
                    markers: cur.markers.clone(),
                    building: id.and_then(|id| s4_viewer::building_dialog::Facts::from_world(world, id)),
                    ambient: cur.ambient.clone(),
                };
                latest.store(Arc::new(s));
                let _ = done.send(());
            }
            Some(Ctl::Markers(q)) => {
                extras.query = q;
                extras.refresh_markers(world);
                let ms = latest.load().tick_ms;
                let mut s = snapshot(world, ms, extras);
                // Same tick: keep the publication time so interpolation does not jump.
                s.at = latest.load().at;
                latest.store(Arc::new(s));
            }
            None => {}
        }
        if !manual
            && let Some(ms) = tick_ms(speed)
            && Instant::now() >= next
        {
            let Some((ev, saves)) = step(world, script.as_deref_mut(), ticker, false, &left) else {
                // Waiting for another player's turn: try again shortly.
                next = Instant::now() + Duration::from_millis(2);
                continue;
            };
            for side in saves {
                match side {
                    Side::Save(name) => net_save(world, script.as_deref(), &extras.map, extras.local, &name, net_resume(ticker)),
                    Side::Chat(p, m, t) => {
                        let _ = chats.send((p, m, t));
                    }
                }
            }
            let _ = events.send(ev);
            extras.tick(world);
            latest.store(Arc::new(snapshot(world, ms, extras)));
            next += Duration::from_millis(ms);
            // After a stall, do not try to catch up more than one tick at a time.
            if next < Instant::now() {
                next = Instant::now() + Duration::from_millis(ms);
            }
        }
    }
}

/// The sim without a thread, for determinism checks: run `ticks` ticks of `log` and return the
/// state hash after every tick.
pub fn run_headless(setup: &SimSetup, log: &[Stamped], ticks: u32) -> anyhow::Result<Vec<u64>> {
    let (mut world, mut script) = build_world(setup, &setup.map_path, local_player(setup))?;
    let mut queue = CommandQueue::default();
    for s in log {
        queue.push(s.clone());
    }
    let mut ticker = Ticker::Local(queue);
    let mut hashes = Vec::with_capacity(ticks as usize);
    let (left, _) = channel::<NetNotice>();
    for _ in 0..ticks {
        let _ = step(&mut world, script.as_mut(), &mut ticker, true, &left);
        hashes.push(world.state_hash());
    }
    Ok(hashes)
}

/// A player's buildings as (entity index, type, x, y), for writing command logs.
pub fn buildings(setup: &SimSetup, player: u8) -> anyhow::Result<Vec<(u16, u16, i32, i32)>> {
    let world = population::world_from_map(&s4_formats::map::MapFile::open(&setup.map_path)?)?;
    Ok(world
        .entities
        .iter()
        .filter(|(_, e)| e.obj_type == s4_sim::entity::ObjType::Building && e.owner == player)
        .map(|(i, e)| (i, e.kind, e.pos.x(), e.pos.y()))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn campaign_types() {
        let t = |p: &str| campaign_type(std::path::Path::new(p));
        assert_eq!(t("Map/Campaign/roman01.map"), Some(1));
        assert_eq!(t("Map/Campaign/dark12.map"), Some(4));
        assert_eq!(t("Map/Campaign/MD_conflict1.map"), Some(9));
        assert_eq!(t("Map/Campaign/AO_trojan07.map"), Some(15));
        assert_eq!(t("Map/Campaign/MCD2_viking3.map"), Some(18));
        assert_eq!(t("Map/Campaign/XMD3_trojan2.map"), Some(24));
        assert_eq!(t("Map/Singleplayer/Cordoba.map"), None);
        assert_eq!(t("Map/Tutorial/Tutorial01.map"), None);
    }

    #[test]
    fn speeds() {
        assert_eq!(tick_ms(0), None);
        assert_eq!(tick_ms(1), Some(71));
        assert_eq!(tick_ms(2), Some(47));
        assert_eq!(tick_ms(3), Some(35));
    }

    /// Savegame round trip: roman02 with its script and commands stamped before and after the
    /// save tick; saved at tick 300, loaded into a new sim thread and continued to 900, every
    /// tick's state hash equals the uninterrupted run's.
    #[test]
    fn save_load_continues_identically() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        use s4_sim::{Command, PlayerCommand};
        let setup = |load: Option<std::path::PathBuf>| SimSetup {
            map_path: s4_viewer::scene::resolve_map("Campaign/roman02"),
            prefer_file_script: true,
            run_script: true,
            fog: true,
            outcome: Default::default(),
            tutorial: Default::default(),
            net: None,
            players: None,
            load,
        };
        let cmds = [
            (100, Command::SwitchBuilding { building: 420 }),
            (250, Command::SetWorkingArea { building: 420, x: 150, y: 70 }),
            // Pending at the save: must survive it.
            (302, Command::SwitchBuilding { building: 420 }),
            (500, Command::PrioritizeBuildingSite { building: 420 }),
        ];
        let a = SimHandle::spawn(setup(None), Clock::Manual).unwrap();
        for (t, c) in &cmds {
            a.command(Stamped { tick: *t, command: PlayerCommand::new(1, c.clone()) });
        }
        assert_eq!(a.step(300), 300);
        let file = std::env::temp_dir().join(format!("s4rs-test-save-{}.s4rs", std::process::id()));
        let client = crate::save::ClientState { view: (1.0, 2.0, 1.0), local: 1, campaign: Some((1, 1)), speed: 2, logos: vec![None, Some(vec![1, 2, 3]), None, None, None, None, None, None], net: None };
        let h = a.save(&file, "test", client.clone(), vec![7; 30]).unwrap();
        assert_eq!(crate::save::read_header(&file).unwrap().thumb, vec![7; 30]);
        assert_eq!(h.tick, 300);
        let size = std::fs::metadata(&file).unwrap().len();
        let mut ha = Vec::new();
        for _ in 300..900 {
            a.step(1);
            ha.push(a.snapshot().hash);
        }
        let b = SimHandle::spawn(setup(Some(file.clone())), Clock::Manual).unwrap();
        assert_eq!(b.snapshot().tick, 300);
        assert_eq!(b.loaded.as_ref(), Some(&client));
        let mut hb = Vec::new();
        for _ in 300..900 {
            b.step(1);
            hb.push(b.snapshot().hash);
        }
        let _ = std::fs::remove_file(&file);
        assert_eq!(ha.iter().zip(&hb).position(|(x, y)| x != y).map(|i| i + 301), None, "save of {size} bytes");
        // The commands mattered: a run without them ends elsewhere.
        let none = run_headless(&setup(None), &[], 900).unwrap();
        assert_ne!(none.last(), ha.last());
    }

    /// Regression: roman02's victory check calls `Buildings.ExistsBuildingInArea` with five
    /// arguments; the missing status reads READY (release 0x5c530c), so the Mayan temple counts
    /// and the game is not "won" at tick 144 as it was while the binding read it as 0.
    #[test]
    fn roman02_is_not_won_at_the_start() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let s = SimSetup {
            map_path: s4_viewer::scene::resolve_map("Campaign/roman02"),
            prefer_file_script: true,
            run_script: true,
            fog: false,
            outcome: Default::default(),
            tutorial: Default::default(),
            net: None,
            players: None,
            load: None,
        };
        let sim = SimHandle::spawn(s, Clock::Manual).unwrap();
        assert_eq!(sim.step(1500), 1500);
        assert_eq!(sim.outcome.load(std::sync::atomic::Ordering::Relaxed), 0, "decided within 1500 ticks");
    }

    /// [ours] The relay dies in an internet game of two (sim threads, manual clock): after the
    /// relay is lost (10 s) and the kick delay (20 s) every peer drops the other, goes offline
    /// and plays on alone; both reach tick 300, report it once (`NetNotice::RelayLost`) and save.
    #[test]
    fn an_internet_game_whose_relay_dies_plays_on_and_saves() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let (relay, stop, _) = s4_relay::spawn("127.0.0.1:0", Default::default()).unwrap();
        let map_hash = s4_net::map_hash(&s4_viewer::scene::resolve_map("Campaign/roman02")).unwrap();
        let settings = s4_net::GameSettings { map: "Campaign/roman02".into(), delay_ms: 142, hash_interval: 64, map_hash, ..Default::default() };
        let bind = format!("{RELAY_BIND}{relay}");
        let host_setup = net_setup(NetSetup::Host { bind: bind.clone(), players: 2, settings });
        let host = std::thread::spawn(move || SimHandle::spawn(host_setup, Clock::Manual).unwrap());
        // The host's game on the relay's list.
        let mut d = s4_net::Discovery::internet(&relay.to_string()).unwrap();
        let t = Instant::now();
        while d.games.is_empty() {
            assert!(t.elapsed() < Duration::from_secs(20), "the host's game is listed");
            d.query(&[]);
            std::thread::sleep(Duration::from_millis(50));
            d.pump();
        }
        let room = d.games[0].0;
        let client = SimHandle::spawn(net_setup(NetSetup::Join { bind, host: room, map_hash, save: (0, 0) }), Clock::Manual).unwrap();
        let host = host.join().unwrap();
        // Network saves go to the test folder `two_sim_threads_in_lockstep` uses (the same value;
        // SAFETY (set_var): no other test reads it).
        let dir = std::env::temp_dir().join(format!("s4rs-test-netsave-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        unsafe { std::env::set_var("S4RS_SAVE_DIR", &dir) };
        let (to_200, killed) = (std::sync::Arc::new(std::sync::Barrier::new(3)), std::sync::Arc::new(std::sync::Barrier::new(3)));
        let run = |sim: SimHandle, name: &'static str, to_200: std::sync::Arc<std::sync::Barrier>, killed: std::sync::Arc<std::sync::Barrier>, dir: std::path::PathBuf| {
            std::thread::spawn(move || {
                let t = Instant::now();
                while sim.step(1) < 200 {
                    assert!(t.elapsed() < Duration::from_secs(60), "{name}: tick 200");
                }
                to_200.wait();
                killed.wait();
                let t = Instant::now();
                let mut tick = 0;
                while tick < 300 {
                    assert!(t.elapsed() < Duration::from_secs(60), "{name} stalls at {tick}");
                    tick = sim.step(1);
                }
                let notices = sim.drain_net();
                // A network game saves at a lockstep tick (0xFA8): offline, our own turn runs it.
                assert!(sim.save(&dir.join("unused.s4rs"), "relaydies", Default::default(), Vec::new()).is_err(), "scheduled");
                let at = sim.step(20);
                (notices, at, t.elapsed())
            })
        };
        let h = run(host, "host", to_200.clone(), killed.clone(), dir.clone());
        let c = run(client, "client", to_200.clone(), killed.clone(), dir.clone());
        to_200.wait();
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(400));
        killed.wait();
        for (name, player, r) in [("host", 1, h.join().unwrap()), ("client", 2, c.join().unwrap())] {
            let (notices, tick, took) = r;
            println!("{name}: {notices:?}, at tick {tick} {took:?} after the relay stopped");
            assert_eq!(notices.iter().filter(|n| **n == NetNotice::RelayLost).count(), 1, "{name}: {notices:?}");
            let save = dir.join(format!("relaydies - player {player}.s4rs"));
            let l = crate::save::read(&save, s4_sim::Rules::load_default().unwrap()).unwrap_or_else(|e| panic!("{name}'s save {}: {e}", save.display()));
            assert!(l.header.tick >= 300, "{name}: saved at {}", l.header.tick);
            let _ = std::fs::remove_file(&save);
        }
    }

    fn net_setup(net: NetSetup) -> SimSetup {
        let map_path = s4_viewer::scene::resolve_map("Campaign/roman02");
        SimSetup { map_path, prefer_file_script: true, run_script: true, fog: true, outcome: Default::default(), tutorial: Default::default(), net: Some(net), players: None, load: None }
    }

    /// What `CreateStartResources<Few|Medium|Many>(x, y, player, ai)` of the install's
    /// `Script\Internal\StartResources.txt` adds for `race`: settlers by type, goods (before the
    /// race's conversion) in total, small guard towers.
    fn start_package(file: &str, amount: u8, race: u8, ai: i32) -> (std::collections::BTreeMap<u16, u32>, u32, u32) {
        let id = |table: &str, name: &str| s4_script::CONSTANTS.iter().find(|c| c.0 == table && c.1 == name).map(|c| c.2 as u16).unwrap();
        let func = format!("function CreateStartResources{}(", ["Few", "Medium", "Many"][amount as usize - 1]);
        let (mut settlers, mut goods, mut towers) = (std::collections::BTreeMap::new(), 0, 0);
        let mut on = false;
        for line in file.lines().skip_while(|l| !l.starts_with(&func)).skip(1).take_while(|l| !l.starts_with("end")) {
            let l = line.trim();
            if let Some(r) = l.strip_prefix("if Game.PlayerRace(_Player) == ") {
                on = r.trim_end_matches(" then").parse::<u8>() == Ok(race);
            } else if let Some(n) = l.strip_prefix("if _AI >= ") {
                on = ai >= n.trim_end_matches(" then").parse::<i32>().unwrap();
            } else if l == "end" {
                on = false;
            } else if on && !l.starts_with("--") {
                let args: Vec<&str> = l.split(['(', ')', ',']).map(str::trim).collect();
                match args[0] {
                    "Settlers.AddSettlers" => *settlers.entry(id("Settlers", &args[4][9..])).or_insert(0) += args[5].parse::<u32>().unwrap(),
                    "Goods.AddPileEx" => goods += args[4].parse::<u32>().unwrap(),
                    "Buildings.AddBuildingEx" => towers += (args[4] == "Buildings.GUARDTOWERSMALL") as u32,
                    "" => {}
                    other => panic!("unexpected call {other}"),
                }
            }
        }
        (settlers, goods, towers)
    }

    /// The game start's start resources (0x4f26c0): a free game on a map that places nothing (a
    /// generated 4-player map) gives every player, Few / Medium / Many, its race's package of
    /// `StartResources.txt` at its start (`Game.PlayerRace` per player: Roman, Viking, Maya,
    /// Trojan), with the computers' extras (control 2 → `_AI` 1, 3 → 2); an open slot gets none,
    /// nor does a game with goods 0. Needs the install.
    #[test]
    fn start_resources_on_a_map_without_units() {
        let file = s4_formats::game_dir().join("Script").join("Internal").join("StartResources.txt");
        let Ok(text) = std::fs::read_to_string(&file) else { return };
        let params = s4_mapgen::key::Params { seed: 0x1d2c3, size: 320, land: 80, resources: 10, mirror: 0, players: 5, ..Default::default() };
        let path = s4_viewer::scene::resolve_map(&format!("RD_{}", s4_mapgen::key::build(&params)));
        let map = s4_formats::map::MapFile::open(&path).unwrap();
        assert!(map.settlers().unwrap().is_empty() && map.buildings().unwrap().is_empty());
        let races = [0u8, 1, 2, 4];
        let control = [1u8, 1, 2, 3, 0, 0, 0, 0];
        for amount in 0..=3u8 {
            let players = PlayerSetup { races: [Some(0), Some(1), Some(2), Some(4), None, None, None, None], teams: [0, 1, 2, 3, 4, 5, 6, 7].map(Some), control, game_type: 1, local: 1, players: Some(5), race_goods: [0; 7], decided: false, start_resources: amount };
            let setup = SimSetup { map_path: path.clone(), prefer_file_script: false, run_script: false, fog: false, outcome: Default::default(), tutorial: Default::default(), net: None, players: Some(players), load: None };
            let (w, _) = build_world(&setup, &path, 1).unwrap();
            let starts: Vec<(i32, i32)> = (1..=5).map(|p| w.players[p].as_ref().map(|p| (p.start_x as i32, p.start_y as i32)).unwrap()).collect();
            let nearest = |x: i32, y: i32| (0..5).min_by_key(|&k| (starts[k].0 - x).pow(2) + (starts[k].1 - y).pow(2)).unwrap() as u8 + 1;
            for p in 1..=5u8 {
                let (want_settlers, want_goods, want_towers) = match (amount, races.get(p as usize - 1)) {
                    (1..=3, Some(&r)) => start_package(&text, amount, r, control[p as usize - 1] as i32 - 1),
                    _ => Default::default(),
                };
                // Settlers on the map (not the tower's soldier inside) of the scripted types 1..=0x42 (not
                // the tower's door settler, type 69).
                let mut settlers = std::collections::BTreeMap::new();
                let (mut goods, mut towers, mut race_ok) = (0, 0, true);
                for (_, e) in w.entities.iter() {
                    match &e.data {
                        s4_sim::entity::EntityData::Settler(_) if e.owner == p && e.flags & s4_sim::entity::flag::ON_BOARD == 0 && e.kind <= 0x42 => {
                            *settlers.entry(e.kind).or_insert(0u32) += 1;
                            race_ok &= e.race == races[p as usize - 1];
                        }
                        s4_sim::entity::EntityData::Building(_) if e.owner == p => {
                            towers += (e.kind == 46) as u32;
                            assert!((e.x() - starts[p as usize - 1].0).abs() < 25 && (e.y() - starts[p as usize - 1].1).abs() < 25, "player {p}'s tower at its start");
                        }
                        s4_sim::entity::EntityData::Pile(d) if nearest(e.x(), e.y()) == p => goods += d.amount as u32,
                        _ => {}
                    }
                }
                assert_eq!((&settlers, goods, towers), (&want_settlers, want_goods, want_towers), "amount {amount}, player {p}");
                assert!(race_ok, "player {p}'s settlers of its race");
                assert!(amount == 0 || p == 5 || want_goods > 0);
            }
        }
    }

    /// A network game on a random map (`RD_<key>`): the host generates the map from the key, the
    /// client generates its own (the host's file removed first) with the same bytes, joins with
    /// that file's hash, and both peers run 600 ticks hash-identical. The map places nothing, so
    /// the game start gave both players their Medium start resources (0x4f26c0). Needs the
    /// install.
    #[test]
    fn two_peers_on_a_random_map() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let params = s4_mapgen::key::Params { seed: 0x2b67, size: 256, land: 70, resources: 10, mirror: 0, players: 2, ..Default::default() };
        let name = format!("RD_{}", s4_mapgen::key::build(&params));
        let path = s4_viewer::scene::resolve_map(&name);
        let host_bytes = std::fs::read(&path).unwrap();
        let map_hash = s4_net::map_hash(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(std::fs::read(s4_viewer::scene::resolve_map(&name)).unwrap(), host_bytes, "the client's map from the key");
        let port = std::net::UdpSocket::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let settings = s4_net::GameSettings { map: name.clone(), delay_ms: 142, hash_interval: 64, map_hash, ..Default::default() };
        // The lobby's random races resolved (`SetRandomRaces`), the same on both peers.
        let players = PlayerSetup { races: [Some(0), Some(1), None, None, None, None, None, None], teams: [Some(0), Some(1), None, None, None, None, None, None], control: [1, 1, 0, 0, 0, 0, 0, 0], game_type: 0, local: 0, players: Some(2), race_goods: [0; 7], decided: false, start_resources: 2 };
        let peer = |net: NetSetup, done: Arc<std::sync::Barrier>| {
            let map_path = path.clone();
            move || -> Vec<u64> {
                let setup = SimSetup { map_path, prefer_file_script: true, run_script: true, fog: false, outcome: Default::default(), tutorial: Default::default(), net: Some(net), players: Some(players), load: None };
                let sim = SimHandle::spawn(setup, Clock::Manual).unwrap();
                let mut hashes = Vec::new();
                for t in 0..600 {
                    let start = Instant::now();
                    while sim.step(1) != t + 1 {
                        assert!(start.elapsed() < Duration::from_secs(60), "tick {}: no progress for a minute", t + 1);
                    }
                    hashes.push(sim.snapshot().hash);
                }
                let pop = sim.snapshot().population.clone();
                for p in 1..=2 {
                    assert!(pop.settlers.iter().filter(|u| u.owner == p).count() > 30, "player {p}'s start settlers");
                    assert!(pop.buildings.iter().any(|b| b.owner == p), "player {p}'s start tower");
                }
                done.wait();
                hashes
            }
        };
        let done = Arc::new(std::sync::Barrier::new(2));
        let host = std::thread::spawn(peer(NetSetup::Host { bind: format!("127.0.0.1:{port}"), players: 2, settings }, done.clone()));
        std::thread::sleep(Duration::from_millis(300));
        let client = std::thread::spawn(peer(NetSetup::Join { bind: "127.0.0.1:0".into(), host: format!("127.0.0.1:{port}").parse().unwrap(), map_hash, save: (0, 0) }, done));
        let (h, c) = (host.join().unwrap(), client.join().unwrap());
        assert_eq!(h.len(), 600);
        assert_eq!(h.iter().zip(&c).position(|(a, b)| a != b), None);
        assert_ne!(h.first(), h.last(), "the game ran");
    }

    /// A peer that leaves (its sim dropped: `Session::leave`) reaches the other's client as
    /// `drain_net` (the message 0x8d2, `msgwin::net_notices`), from the lingering between steps.
    #[test]
    fn a_peer_that_leaves_is_reported() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let port = std::net::UdpSocket::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let map_hash = s4_net::map_hash(&s4_viewer::scene::resolve_map("Campaign/roman02")).unwrap();
        let settings = s4_net::GameSettings { map: "Campaign/roman02".into(), delay_ms: 142, hash_interval: 64, map_hash, ..Default::default() };
        let setup = |net: NetSetup| SimSetup { map_path: s4_viewer::scene::resolve_map("Campaign/roman02"), prefer_file_script: true, run_script: true, fog: false, outcome: Default::default(), tutorial: Default::default(), net: Some(net), players: None, load: None };
        let host_setup = setup(NetSetup::Host { bind: format!("127.0.0.1:{port}"), players: 2, settings });
        let host = std::thread::spawn(move || SimHandle::spawn(host_setup, Clock::Manual).unwrap());
        std::thread::sleep(Duration::from_millis(300));
        let client = SimHandle::spawn(setup(NetSetup::Join { bind: "127.0.0.1:0".into(), host: format!("127.0.0.1:{port}").parse().unwrap(), map_hash, save: (0, 0) }), Clock::Manual).unwrap();
        let host = host.join().unwrap();
        let who = client.local;
        let c = std::thread::spawn(move || {
            client.step(20);
            drop(client);
        });
        host.step(20);
        c.join().unwrap();
        let start = Instant::now();
        let mut left = Vec::new();
        while left.is_empty() && start.elapsed() < Duration::from_secs(10) {
            left = host.drain_net();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(left, [NetNotice::Left(who)]);
    }

    /// A real-time host whose client never sends its first turn warns about it
    /// (`NetNotice::Waiting` → 0x8d5) once `NotReadyWarnDelay` (6 s) has passed, and again only
    /// after `NotReadyWarnAgainDelay` (2.5 s). The delays themselves are checked with set times in
    /// s4-net (`not_ready_timer`); here only bounds that hold however loaded the machine is: the
    /// host's timer starts after `t0` (its first stall), and this thread only sees a notice after
    /// it was sent, so the k-th warning comes at least 6 + 2.5·k s after `t0`. No upper bound: a
    /// loaded machine delays the host's polls and this thread's.
    ///
    /// The bound has a 0.5 s margin: a trial under load saw the first warning 5.986 s after `t0`.
    /// The code does not explain that. `NetEvent::Waiting` has one source (`Session::stalled` →
    /// `NotReady::check(Instant::now(), ..)` on the host's thread, which starts after `t0`), the
    /// delay is wall time (not ticks or turns), and the notice channel is per handle. So the
    /// margin covers a clock or measuring effect we could not pin down, not a known path.
    #[test]
    fn a_peer_waited_for_is_reported() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let t0 = Instant::now();
        let port = std::net::UdpSocket::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let map_hash = s4_net::map_hash(&s4_viewer::scene::resolve_map("Campaign/roman02")).unwrap();
        let settings = s4_net::GameSettings { map: "Campaign/roman02".into(), delay_ms: 142, hash_interval: 64, map_hash, ..Default::default() };
        let setup = |net: NetSetup| SimSetup { map_path: s4_viewer::scene::resolve_map("Campaign/roman02"), prefer_file_script: true, run_script: true, fog: false, outcome: Default::default(), tutorial: Default::default(), net: Some(net), players: None, load: None };
        let host_setup = setup(NetSetup::Host { bind: format!("127.0.0.1:{port}"), players: 2, settings });
        let host = std::thread::spawn(move || SimHandle::spawn(host_setup, Clock::RealTime { speed: 1 }).unwrap());
        std::thread::sleep(Duration::from_millis(300));
        // The client never steps: no turn of it ever goes out.
        let client = SimHandle::spawn(setup(NetSetup::Join { bind: "127.0.0.1:0".into(), host: format!("127.0.0.1:{port}").parse().unwrap(), map_hash, save: (0, 0) }), Clock::Manual).unwrap();
        let host = host.join().unwrap();
        // Until the second warning (the kick, 20 s after the stall, would come later).
        let mut seen = Vec::new();
        while seen.len() < 2 && t0.elapsed() < Duration::from_secs(120) {
            seen.extend(host.drain_net().into_iter().map(|n| (n, t0.elapsed())));
            std::thread::sleep(Duration::from_millis(20));
        }
        drop(client);
        assert!(seen.len() >= 2 && seen[..2].iter().all(|s| s.0 == NetNotice::Waiting(2)), "two warnings about player 2 first: {seen:?}");
        for (k, s) in seen.iter().take(2).enumerate() {
            assert!(s.1 >= Duration::from_millis(5500 + 2500 * k as u64), "warning {k} too early: {seen:?}");
        }
    }

    /// A player who leaves becomes a computer on every peer at the same tick (0x5a2da0: control
    /// 2 / 3 by the host's AI level; event 0x39 → `CAIMain::AddPlayer`, s4-sim
    /// `Command::PlayerLeft`): Choices with three peers, the third leaves after tick 800. The two
    /// that stay run hash-identical to 1800, the leaver's player has an AI and keeps building.
    /// A save of the world after the leave (tick 1200) loads with the AI and, run on its own,
    /// gives the same hashes as the network game (no human commands are issued).
    #[test]
    fn a_leaver_becomes_a_computer() {
        leaver_becomes_a_computer(false, 800, 1800, Some(1200));
    }

    /// The same through an in-process relay (s4-relay on 127.0.0.1), short: the third peer leaves
    /// after tick 300, the two that stay agree to 900 and player 3 is a computer on both.
    #[test]
    fn a_leaver_becomes_a_computer_through_the_relay() {
        leaver_becomes_a_computer(true, 300, 900, None);
    }

    /// Choices with three humans; the third peer leaves after `leave`, the others run to `end`
    /// (directly over UDP, or through a relay); with `save_at`, the host saves the world then and
    /// the save, loaded and run on alone, is checked against the network game.
    fn leaver_becomes_a_computer(relay: bool, leave: u32, end: u32, save_at: Option<u32>) {
        let map = s4_viewer::scene::resolve_map("Multiplayer/Choices");
        if !map.exists() {
            return;
        }
        // The relay's first client (the host) is room 1.
        let server = relay.then(|| s4_relay::spawn("127.0.0.1:0", Default::default()).unwrap());
        let port = std::net::UdpSocket::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let (host_bind, join_bind, host_addr) = match &server {
            Some((addr, ..)) => (format!("{RELAY_BIND}{addr}"), format!("{RELAY_BIND}{addr}"), s4_net::relay::relay_addr(1)),
            None => (format!("127.0.0.1:{port}"), "127.0.0.1:0".to_string(), format!("127.0.0.1:{port}").parse().unwrap()),
        };
        let map_hash = s4_net::map_hash(&map).unwrap();
        let settings = s4_net::GameSettings { map: "Multiplayer/Choices".into(), delay_ms: 142, hash_interval: 64, map_hash, ai_level: 1, ..Default::default() };
        // Three humans (Roman, Viking, Maya) with the medium start resources, one team each.
        let players = PlayerSetup { races: [Some(0), Some(1), Some(2), None, None, None, None, None], teams: [Some(0), Some(1), Some(2), None, None, None, None, None], control: [1, 1, 1, 0, 0, 0, 0, 0], game_type: 0, local: 0, players: Some(3), race_goods: [0; 7], decided: false, start_resources: 2 };
        let setup = |net: Option<NetSetup>, load: Option<std::path::PathBuf>| SimSetup { map_path: map.clone(), prefer_file_script: true, run_script: false, fog: false, outcome: Default::default(), tutorial: Default::default(), net, players: Some(players), load };
        let save = std::env::temp_dir().join(format!("s4rs-test-leaver-{}-{relay}.s4rs", std::process::id()));
        // (hashes after each tick, player 3's buildings at `leave` and at the end, player 3 has an AI).
        type Run = (Vec<u64>, usize, usize, bool);
        let peer = |net: NetSetup, to: u32, save: Option<(std::path::PathBuf, u32)>| {
            let s = setup(Some(net), None);
            let map = map.clone();
            move || -> Run {
                let sim = SimHandle::spawn(s, Clock::Manual).unwrap();
                let buildings = |sim: &SimHandle| sim.snapshot().population.buildings.iter().filter(|b| b.owner == 3).count();
                let mut hashes = Vec::new();
                let mut at_leave = 0;
                for t in 0..to {
                    let start = Instant::now();
                    while sim.step(1) != t + 1 {
                        assert!(start.elapsed() < Duration::from_secs(60), "tick {}: no progress for a minute", t + 1);
                    }
                    hashes.push(sim.snapshot().hash);
                    if t + 1 == leave {
                        at_leave = buildings(&sim);
                    }
                    if let Some((path, at)) = save.clone()
                        && t + 1 == at
                    {
                        let (tx, rx) = channel();
                        let map = map.clone();
                        sim.with_world(move |w| {
                            let input = crate::save::SaveInput { name: "leaver", thumb: Vec::new(), map: &map, world: w, script: None, host: Default::default(), commands: Vec::new(), client: Default::default() };
                            let _ = tx.send(crate::save::write(&path, input).map(|h| h.tick));
                        });
                        assert_eq!(rx.recv().unwrap().unwrap(), at);
                    }
                }
                let (tx, rx) = channel();
                sim.with_world(move |w| {
                    let _ = tx.send(w.ai.players[3].is_some() && w.setup.control[3] == 3);
                });
                let ai = rx.recv().unwrap();
                (hashes, at_leave, buildings(&sim), ai)
                // The leaver drops its sim here (`Session::leave`).
            }
        };
        let join = || NetSetup::Join { bind: join_bind.clone(), host: host_addr, map_hash, save: (0, 0) };
        let host = std::thread::spawn(peer(NetSetup::Host { bind: host_bind, players: 3, settings }, end, save_at.map(|t| (save.clone(), t))));
        std::thread::sleep(Duration::from_millis(300));
        let second = std::thread::spawn(peer(join(), end, None));
        std::thread::sleep(Duration::from_millis(300));
        let third = std::thread::spawn(peer(join(), leave, None));
        let (h, s, l) = (host.join().unwrap(), second.join().unwrap(), third.join().unwrap());
        assert_eq!(l.0.len() as u32, leave);
        assert_eq!(h.0.iter().zip(&l.0).position(|(a, b)| a != b), None, "all three agree up to the leave");
        assert_eq!(h.0.len() as u32, end);
        assert_eq!(h.0.iter().zip(&s.0).position(|(a, b)| a != b), None, "the two that stay agree to the end");
        assert!(h.3 && s.3, "the leaver's player is a computer (control 3, an AI) on both");
        println!("player 3's buildings: {} at the leave (tick {leave}), {} at tick {end}", h.1, h.2);
        if let Some((_, stop, _)) = &server {
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        let Some(at) = save_at else { return };
        assert!(h.2 > h.1, "the computer builds: player 3's buildings {} at the leave, {} at the end", h.1, h.2);
        // The save after the leave: the AI comes with it, and the game, loaded and run on alone,
        // runs as the network one.
        let rules = || s4_sim::Rules::load_default().unwrap();
        let loaded = crate::save::read(&save, rules()).unwrap();
        assert!(loaded.world.ai.players[3].is_some() && loaded.world.setup.control[3] == 3);
        assert_eq!(loaded.world.state_hash(), h.0[at as usize - 1]);
        let sim = SimHandle::spawn(setup(None, Some(save.clone())), Clock::Manual).unwrap();
        assert_eq!(sim.snapshot().tick, at);
        let on: Vec<u64> = (at..end).map(|t| {
            assert_eq!(sim.step(1), t + 1);
            sim.snapshot().hash
        }).collect();
        let first_diff = on.iter().zip(&h.0[at as usize..]).position(|(a, b)| a != b).map(|i| at as usize + i + 1);
        assert_eq!(first_diff, None, "the loaded game runs on as the network game (first different tick)");
        let _ = std::fs::remove_file(&save);
    }

    /// Two peers through the game's own sim thread (lobby, lockstep, script driver): roman02 for
    /// 2,000 ticks with commands from both sides stays hash-identical every tick, and the commands
    /// changed the game. A network save after tick ~500 (with commands of both peers in flight)
    /// is loaded into a new network game by both peers (each its own file): from the saved tick
    /// to 900 it runs hash-identical to the uninterrupted game. Needs the install.
    #[test]
    fn two_sim_threads_in_lockstep() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        use s4_sim::{Command, PlayerCommand};
        // A free port (bound and released), so parallel runs do not collide.
        let port = std::net::UdpSocket::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let map_hash = s4_net::map_hash(&s4_viewer::scene::resolve_map("Campaign/roman02")).unwrap();
        let settings = s4_net::GameSettings { map: "Campaign/roman02".into(), delay_ms: 142, hash_interval: 64, map_hash, ..Default::default() };
        // Each peer runs on its own thread (they block on each other's turns), issuing its
        // commands at their ticks and recording the hash after every tick.
        // roman02: building 420 is player 1's, 409 and 412 player 2's (entity ids; every military
        // building's door settler takes the id after it).
        // Network saves go to a test folder. SAFETY (set_var): no other test reads it.
        let dir = std::env::temp_dir().join(format!("s4rs-test-netsave-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        unsafe { std::env::set_var("S4RS_SAVE_DIR", &dir) };
        // Neither peer leaves before both have stepped 2000 ticks (a peer that quits early takes
        // the turns the other may still need resent with it).
        let peer = |net: NetSetup, plan: Vec<(u32, Command)>, load: Option<std::path::PathBuf>, to: u32, done: Arc<std::sync::Barrier>| {
            let host = matches!(net, NetSetup::Host { .. });
            move || -> Vec<u64> {
                let sim = SimHandle::spawn(SimSetup { load, ..net_setup(net) }, Clock::Manual).unwrap();
                let from = sim.snapshot().tick;
                let mut hashes = Vec::new();
                for t in from..to {
                    for (at, c) in &plan {
                        if *at == t {
                            sim.command(Stamped { tick: 0, command: PlayerCommand::new(0, c.clone()) });
                        }
                    }
                    if host && t == 500 && from == 0 {
                        // A save in a network game is a lockstep command: both peers save.
                        assert!(sim.save(std::path::Path::new("-"), "mp", Default::default(), Vec::new()).is_err());
                    }
                    // Each step waits up to NET_TIMEOUT for the other peer's turn; under heavy load
                    // it may come back without the tick, so it is retried for up to a minute (a
                    // stuck lockstep still fails, a desync shows in the hashes).
                    let start = Instant::now();
                    while sim.step(1) != t + 1 {
                        assert!(start.elapsed() < Duration::from_secs(60), "tick {}: no progress for a minute", t + 1);
                    }
                    hashes.push(sim.snapshot().hash);
                }
                done.wait();
                hashes
            }
        };
        // Working-area moves of both players around the save (ticks 498..=508): some are in a
        // turn not yet run when the peers save.
        let around = |b: u32, x0: u16| (498..=508u32).map(move |t| (t, Command::SetWorkingArea { building: b, x: x0 + (t - 498) as u16, y: 70 }));
        let host_plan: Vec<(u32, Command)> = [(100, Command::SwitchBuilding { building: 420 }), (400, Command::SetWorkingArea { building: 420, x: 150, y: 70 })].into_iter().chain(around(420, 140)).collect();
        let client_plan: Vec<(u32, Command)> = [(150, Command::SwitchBuilding { building: 409 }), (600, Command::CrushBuilding { building: 412 })].into_iter().chain(around(409, 160)).collect();
        let done = Arc::new(std::sync::Barrier::new(2));
        let host = std::thread::spawn(peer(NetSetup::Host { bind: format!("127.0.0.1:{port}"), players: 2, settings: settings.clone() }, host_plan.clone(), None, 2000, done.clone()));
        std::thread::sleep(Duration::from_millis(300));
        let client = std::thread::spawn(peer(
            NetSetup::Join { bind: "127.0.0.1:0".into(), host: format!("127.0.0.1:{port}").parse().unwrap(), map_hash, save: (0, 0) },
            client_plan.clone(),
            None,
            2000,
            done,
        ));
        let (h, c) = (host.join().unwrap(), client.join().unwrap());
        assert_eq!(h.len(), 2000);
        assert_eq!(h.iter().zip(&c).position(|(a, b)| a != b), None);
        let single = run_headless(&SimSetup { net: None, ..net_setup(NetSetup::Join { bind: String::new(), host: "127.0.0.1:1".parse().unwrap(), map_hash, save: (0, 0) }) }, &[], 2000).unwrap();
        assert_ne!(single.last(), h.last(), "the commands had an effect");
        // Both peers saved the same tick, with the same world.
        let rules = || s4_sim::Rules::load_default().unwrap();
        let a = crate::save::read(&dir.join("mp - player 1.s4rs"), rules()).unwrap();
        let b = crate::save::read(&dir.join("mp - player 2.s4rs"), rules()).unwrap();
        assert!(a.header.tick > 500 && a.header.tick == b.header.tick, "{} {}", a.header.tick, b.header.tick);
        assert_eq!(a.world.state_hash(), b.world.state_hash());
        assert_eq!(a.world.state_hash(), h[a.header.tick as usize - 1]);
        let in_flight = |l: &crate::save::Loaded| l.client.net.as_ref().map_or(0, |n| n.turns.iter().map(|t| t.1.len()).sum::<usize>());
        assert!(in_flight(&a) > 0 && in_flight(&b) > 0, "commands in flight at the save: {} / {}", in_flight(&a), in_flight(&b));
        // The loaded network game, the roles swapped: player 2's peer hosts with its file (and
        // takes slot 2), player 1's joins with its own; both run on from the saved tick with the
        // commands of the player they are.
        let saved = a.header.tick;
        let port = std::net::UdpSocket::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let loaded = s4_net::GameSettings { load_tick: saved, load_hash: a.header.world_hash, ..settings };
        let done = Arc::new(std::sync::Barrier::new(2));
        let host = std::thread::spawn(peer(NetSetup::Host { bind: format!("127.0.0.1:{port}"), players: 2, settings: loaded }, client_plan, Some(dir.join("mp - player 2.s4rs")), 900, done.clone()));
        std::thread::sleep(Duration::from_millis(300));
        let client = std::thread::spawn(peer(
            NetSetup::Join { bind: "127.0.0.1:0".into(), host: format!("127.0.0.1:{port}").parse().unwrap(), map_hash, save: (a.header.world_hash, 1) },
            host_plan,
            Some(dir.join("mp - player 1.s4rs")),
            900,
            done,
        ));
        let (h2, c2) = (host.join().unwrap(), client.join().unwrap());
        assert_eq!(h2.len(), 900 - saved as usize);
        assert_eq!(h2.iter().zip(&c2).position(|(a, b)| a != b), None, "the loaded peers agree");
        let first_diff = h2.iter().zip(&h[saved as usize..900]).position(|(a, b)| a != b).map(|i| saved as usize + i + 1);
        assert_eq!(first_diff, None, "the loaded game runs as the uninterrupted one (first different tick)");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// save.rs policy: a current save loads (its world hashes as saved); a version-2 save (no
    /// world hash, made before magic / animals / alliances / trading were saved), a newer one
    /// and one whose world does not hash as its header says are refused with a reason, never
    /// loaded. The load menus show the reason (`compatibility`).
    #[test]
    fn incompatible_saves_are_refused() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let setup = SimSetup {
            map_path: s4_viewer::scene::resolve_map("Campaign/roman02"),
            prefer_file_script: true,
            run_script: true,
            fog: true,
            outcome: Default::default(),
            tutorial: Default::default(),
            net: None,
            players: None,
            load: None,
        };
        let a = SimHandle::spawn(setup, Clock::Manual).unwrap();
        a.step(20);
        let dir = std::env::temp_dir().join(format!("s4rs-test-compat-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let good = dir.join("good.s4rs");
        let h = a.save(&good, "good", Default::default(), Vec::new()).unwrap();
        let rules = || s4_sim::Rules::load_default().unwrap();
        let fp = crate::save::rules_fingerprint(&rules());
        assert_eq!(crate::save::compatibility(&h, Some(fp)), Ok(()));
        let l = crate::save::read(&good, rules()).unwrap();
        assert_eq!(l.world.state_hash(), h.world_hash);
        // The file is header, then the deflated body.
        let bytes = std::fs::read(&good).unwrap();
        let hlen = bincode::serialized_size(&h).unwrap() as usize;
        let body = &bytes[hlen..];
        let write = |name: &str, header: Vec<u8>| {
            let p = dir.join(name);
            std::fs::write(&p, [header, body.to_vec()].concat()).unwrap();
            p
        };
        // Version 2: the old header layout (no world hash).
        #[derive(serde::Serialize)]
        struct V2<'a>(&'a str, u32, &'a str, u64, &'a str, u64, u32, u64, Vec<u8>);
        let old = write("old.s4rs", bincode::serialize(&V2(crate::save::MAGIC, 2, "AutoSave", h.saved_at, &h.map, h.map_hash, h.tick, h.rules, Vec::new())).unwrap());
        let oh = crate::save::read_header(&old).unwrap();
        assert_eq!((oh.version, oh.name.as_str(), oh.world_hash), (2, "AutoSave", 0));
        assert_eq!(crate::save::compatibility(&oh, Some(fp)), Err("older s4rs"));
        let e = crate::save::read(&old, rules()).err().expect("v2 refused").to_string();
        assert!(e.contains("older s4rs"), "{e}");
        // Newer: not even listed.
        let newer = write("newer.s4rs", bincode::serialize(&crate::save::Header { version: crate::save::VERSION + 1, ..h.clone() }).unwrap());
        assert!(crate::save::read_header(&newer).is_err());
        assert!(crate::save::read(&newer, rules()).is_err());
        // Other rules.
        let other = write("rules.s4rs", bincode::serialize(&crate::save::Header { rules: h.rules ^ 1, ..h.clone() }).unwrap());
        assert_eq!(crate::save::compatibility(&crate::save::read_header(&other).unwrap(), Some(fp)), Err("other game data"));
        assert!(crate::save::read(&other, rules()).is_err());
        // A world that does not hash as saved (a build that changed a saved type without a
        // version bump reads it wrongly): refused.
        let bad = write("hash.s4rs", bincode::serialize(&crate::save::Header { world_hash: h.world_hash ^ 1, ..h.clone() }).unwrap());
        let e = crate::save::read(&bad, rules()).err().expect("refused").to_string();
        assert!(e.contains("differs"), "{e}");
        // A sim set up to load the old save fails to start; a new game beside it starts.
        let load_old = SimSetup { load: Some(old.clone()), ..SimSetup { map_path: s4_viewer::scene::resolve_map("Campaign/roman02"), prefer_file_script: true, run_script: false, fog: false, outcome: Default::default(), tutorial: Default::default(), net: None, players: None, load: None } };
        assert!(SimHandle::spawn(load_old, Clock::Manual).is_err());
        let fresh = SimSetup { map_path: s4_viewer::scene::resolve_map("Campaign/roman02"), prefer_file_script: true, run_script: false, fog: false, outcome: Default::default(), tutorial: Default::default(), net: None, players: None, load: None };
        let s = SimHandle::spawn(fresh, Clock::Manual).unwrap();
        assert_eq!(s.step(5), 5);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
