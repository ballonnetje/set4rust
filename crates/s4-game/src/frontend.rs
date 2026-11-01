//! The start screens: `Menu\GUISetStartscreens.dat` (gfx file 0), the states before a game.
//! Release addresses (shipped exe) of each screen's init / GUI proc / state event handler:
//!
//! * Main menu (`CStateMainMenu`, dialog 0; init 0x48f310, proc 0x490360, events 0x522b70):
//!   twelve buttons whose event, text and visibility come from a table per installed-content
//!   status (`MAIN_BUTTONS`). Event 43 → single player.
//! * Single player type (`CStateLocalType`, dialog 7; init 0x48d100, proc 0x48d2b0, events
//!   0x522270): 2404 free game (event 0x62 → map settings), 2401 campaigns (0x61,
//!   `CStateCampaign3X3`), 2400 Dark Tribe campaign (0x60), 2403 back (0x63); 2413 / 2414 set the
//!   AI difficulty (0x12987ec) to 0 / 1, and the init selects the one matching it.
//! * Map settings (`CStateLobbyMapSettings`: local dialog 18, network 3 (0x5214f0); init
//!   0x48e1c0, list 0x48d600, proc 0x48e5e0, events 0x520c90): the players, teams, mode, goods
//!   and team setup buttons ([`setup`]), map category radios 2449 / 2450 / 2451 (`MapFilePaths`
//!   folders; 2449 only enabled locally), a
//!   seven-row list with scroll 2431 / 2432 and the mouse wheel (rows from the table at 0xffd710:
//!   name, size " %d ", players "1-%d", "%d" for a fixed count (+0x47); the mode marks, conflict / cooperation or free settle / economic; the selected row in font template 2), the preview
//!   (`CStateLobbyGameSettings::DrawMap` into the owner image of 2495), start 2433 (event 0x40),
//!   back 2415.
//! * Game settings, local (`CStateLobbyGameSettings`, dialog 19 (0x51df40: dialog 4 in a network
//!   game, see `net`); proc 0x48ba20, events 0x51d460): the slots with the computers, the
//!   selected row's team / race / colour arrows, the slot type buttons, start 2142, back 2141.
//! * Three-races campaign (`CStateCampaign3X3`, dialog 8; constructor 0x511300, init 0x47bf90,
//!   proc 0x47c150, events 0x5113e0): mission buttons Roman 2044..2046, Viking 2038..2040, Maya
//!   2041..2043 (event 0x66, wParam mission, lParam race 0 / 1 / 2 → campaign type 1 / 2 / 3),
//!   back 2037 (0x67); missions after `status + 1` disabled (progress.rs).
//! * Dark Tribe campaign (`CStateCampaignDark`, dialog 9; constructor 0x5115b0, init 0x47d7b0,
//!   proc 0x47da10, events 0x5116f0): missions 2081..2092 (event 0x64, wParam level → type 4),
//!   back 2080 (0x65).
//! * Briefing (`CStateBriefing`, dialog 10; init 0x47cb40, proc 0x47cda0, events 0x510970;
//!   `InitBriefingTexts`): title 2068, goal 2069, the story scrolled in 2057 (2070 / 2071 and the
//!   wheel), tips 2058 and the campaign picture 2074 behind the toggle 2073, header 2054 = string
//!   880, the speech `Snd\Campaign%s\<name>%02i.mp3` (0x510660: `%s` = `L""` when `Language` is 1,
//!   German, else `L"EN"` at 0x10560a4; the name from the wide table at 0x149bf38, index
//!   campaign type − 1);
//!   back 2055 (0x70), start 2056 (0x71).
//!
//! * The add-on campaigns (main menu events 7001 Mission CD, 8001 Add-On, 9001 Mission CD 2,
//!   9501 Mission CD 3): their own GUI sets, see [`addon`].
//!
//! * Random maps: the map settings' tab 2507 → [`random`].
//!
//! Not ported: the videos (left out), the mode check-mark columns,
//! the map list's sort buttons (2476..2480, event 0x48), the game name and password fields.

pub mod addon;
pub mod endstat;
pub mod race_end;
pub mod credits;
pub mod load;
pub mod msgbox;
pub mod victory;
mod net;
pub mod random;
pub mod setup;
pub mod splash;

use std::path::PathBuf;

use s4_formats::text::TextTable;
use s4_viewer::client::{self, GuiMouse};
use s4_viewer::gpu::Gpu;
use s4_viewer::gui::Gui;
use s4_viewer::guistate::UiEvent;

use crate::sound::gui::{Procs, StartSet, SUB_BUTTON_UP};

const SET: &str = "GUISetStartscreens.dat";
const MAIN: usize = 0;
const LOCAL_TYPE: usize = 7;
/// The map settings screen (the network game's dialog; [`LOCAL_MAP`] locally).
const MAP_SETTINGS: usize = 3;
const LOCAL_MAP: usize = 18;
/// The local game settings (dialog 4's twin without chat and ready).
const LOCAL_GAME: usize = 19;
const CAMPAIGN_3X3: usize = 8;
const CAMPAIGN_DARK: usize = 9;
const BRIEFING: usize = 10;
/// "Load Game" (main menu event 46): our savegames.
const LOAD: usize = 6;
/// The tutorial menu (`CStateTutorial`, vtable 0x10575bc; procedure 0x494330, init 0x4941a0).
const TUTORIAL: usize = 11;
/// Its twelve mission buttons 0xaee..=0xaf9: their texts (0x4941a0) and the description shown
/// while the cursor is on one (code 5 → 0xb03; code 6 empties it).
const TUTORIAL_BUTTONS: [(u16, usize, usize); 12] = [
    (0xaee, 0x82b, 0x820), (0xaef, 0x82c, 0x821), (0xaf0, 0x82d, 0x822), (0xaf1, 0x82e, 0x823), (0xaf2, 0x82f, 0x824), (0xaf3, 0x830, 0x825),
    (0xaf4, 0x831, 0x826), (0xaf5, 0x832, 0x827), (0xaf6, 0x833, 0x828), (0xaf7, 0x829, 0x81d), (0xaf8, 0x82a, 0x81e), (0xaf9, 0x834, 0x81f),
];
/// The load list's rows: (name, date) link pairs (release table at file offset 0xbfca80).
fn load_row(i: usize) -> (u16, u16) {
    if i == 0 { (2343, 2344) } else { (2344 + i as u16, 2361 + i as u16) }
}
/// The start screens are 800×600 panel pixels.
pub const SCREEN: (u32, u32) = (800, 600);

/// What the start screens ask the shell to do.
pub enum Action {
    /// Start a map; a campaign mission carries its (campaign type, level), a free game the
    /// players its game settings set up.
    Start(PathBuf, Option<(u8, u32)>, Option<crate::sim::PlayerSetup>),
    /// Continue a savegame.
    Load(PathBuf),
    /// Start a network game the lobby set up: the session, the map, the races per player id.
    StartNet(Box<s4_net::Session>, PathBuf, crate::sim::PlayerSetup),
    Quit,
}

/// The briefing's campaign picture (2074): the image per (campaign type, level) from the table at
/// release 0xffd378 (`[type · 12 + level]`), set by the briefing init (0x47cd72).
fn briefing_picture(campaign: u8, level: u32) -> u16 {
    let first = match campaign {
        1 => 257,
        2 => 263,
        3 => 260,
        _ => 266,
    };
    first + level as u16
}

/// `InitBriefingTexts` string tables (release .rdata, file offset 0xc18c78): per campaign type
/// (1 Roman, 2 Viking, 3 Maya, 4 Dark Tribe) the first id of the goal (2069), story (2057), tips
/// (2058) and title (2068) strings; mission `level` adds `level`.
pub(crate) fn briefing_strings(campaign: u8) -> [usize; 4] {
    match campaign {
        1 => [3363, 3372, 3375, 3378],
        2 => [3736, 3745, 3748, 3751],
        3 => [2563, 2572, 2575, 2578],
        _ => [826, 802, 814, 838],
    }
}

type Speech = (kira::AudioManager, kira::sound::streaming::StreamingSoundHandle<kira::sound::FromFileError>);

/// `InitGuiDlgMainscreen` (release 0x48f310) for the installed content: per button link, (event,
/// text id, visible). The table is `[button k][status s]` of (event, text, visible) at 0x12938e0
/// (0xC0 per button, 0xC per status); `GuiDlgMainscreenProc` (0x490360, code 3) sends the event of
/// the clicked button's slot. Status = 7 when the Mission CD, the Add-On and Mission CD 2 are
/// present (`CMissionCD` / `CAddOn` / `CMissionCD2`: their Gfx files exist, 0x4fd450 / 0x4fd520 /
/// 0x4fd5c0) and German-only content is allowed (0x4fcb70: `GAMESETTINGS EnableGermanOnlyContent`,
/// default 1); + 8 with `CMissionCD3` (0x4fd690: `Gfx .*`, `Gfx!.*`), so the History Edition
/// is status 15. Statuses 8..=15 are 0..=7 with the button (9501, 3842) inserted before single
/// player and the rest moved down one slot (the copy loop in the init). The last slot's
/// visibility is applied to link 2955, which dialog 0 does not have, so 2551 stays visible. Texts
/// 3841 / 3842 are past the text files' 3838 records: the string engine 0x529eb0 takes them from
/// the exe ("New World" / "Great Crusades", `s4_formats::text::BUILTIN`).
const MAIN_BUTTONS: [(u16, u32, usize, bool); 12] = [
    (2535, 47, 2101, true),   // Tutorial
    (2548, 8001, 47, true),   // Add-On campaigns
    (2531, 7001, 2976, true), // Mission CD campaigns
    (2532, 9001, 3841, true), // Mission CD 2 campaigns
    (2534, 9501, 3842, true), // Mission CD 3 campaigns
    (2536, 43, 2071, true),   // Single player
    (2537, 44, 2057, true),   // Multiplayer
    (2538, 50, 2032, true),   // Manual
    (2533, 51, 2076, false),  // Tips
    (2549, 46, 2030, true),   // Load game
    (2550, 49, 2000, true),   // Credits
    (2551, 45, 2016, true),   // Exit
];

/// `CStateCampaignDark`'s mission choice 0x511741: the video before the briefing of `level`.
fn dark_video(level: u32) -> Option<u8> {
    match level {
        0 => Some(2),
        2 => Some(3),
        4 => Some(4),
        9 => Some(5),
        _ => None,
    }
}

/// A map of the list (the release entry: name +0, players +0x30, size +0x34, the mode flags
/// +0x40..+0x42, a fixed player count +0x47; read by the list fill 0x48d600).
#[derive(Clone)]
struct MapEntry {
    name: String,
    path: PathBuf,
    players: u32,
    size: u32,
    /// +0x40..+0x44: the modes the map allows ([`setup::MapRead::modes`]).
    modes: [bool; 5],
    /// +0x47: the player count is fixed (`!MapRead::can_players`): the list shows "%d", else
    /// "%d-%d" from 1.
    fixed: bool,
}

pub struct Frontend {
    texts: Option<TextTable>,
    screen: usize,
    /// Map settings: category (release 0x149c1dc), list, top row, selected entry.
    category: u32,
    maps: Vec<MapEntry>,
    top: usize,
    selected: Option<usize>,
    progress: crate::progress::Progress,
    /// The mission the briefing is for: (campaign type, level).
    mission: Option<(u8, u32)>,
    /// The briefing speech (`CSoundManager::PlaySoundFile(path, 100, 64)`).
    speech: Option<Speech>,
    language: u32,
    /// The LAN screens' discovery or lobby.
    net: net::NetUi,
    /// The joined game's name (the game list's entry): what a client's lobby shows in 2145.
    joined_name: String,
    /// Map settings opened from the game list (dialog 3 in multiplayer mode).
    multiplayer: bool,
    /// The game list is the internet one (dialog 1's 2520): games from the relay server.
    internet: bool,
    /// A network save chosen in the load list, waiting for dialog 1's LAN / Internet.
    pending_save: Option<PathBuf>,
    /// The load screen: saves (newest first), top row, selected entry.
    saves: Vec<(PathBuf, crate::save::Header)>,
    load_top: usize,
    load_sel: Option<usize>,
    /// The game kind the load list keeps (`CStateLoadGame +4`, chosen in dialog 5).
    load_kind: u32,
    /// The screen the message box (dialog 20) returns to on its event 0x72.
    msg_back: usize,
    /// `CStateSlideshow` after the main menu's exit (event 0x2d).
    slideshow: bool,
    /// The Add-On splash (splash.rs) is shown; the campaign whose screen follows it.
    splash: Option<u8>,
    /// The selected map read for the map settings, and the choices made there.
    map_read: Option<setup::MapRead>,
    choice: setup::Choice,
    /// The single player screen's difficulty (2413 easy / 2414 normal): the computers' level.
    difficulty: u8,
    /// The game settings' selected row (`SelectedPlayerIdLobby`).
    row: Option<usize>,
    /// The add-on campaign pack whose GUI set is shown ([`addon`]).
    pack: Option<addon::Pack>,
    /// The UDP port a hosted game listens on and a typed address without a port goes to
    /// (`lobby::LAN_PORT`; tests take a free one so parallel runs never meet).
    lan_port: u16,
    /// Where a joining peer looks for its saves of a loaded network game (`None`: the save
    /// folder; tests point it elsewhere).
    save_dir: Option<PathBuf>,
    /// What the release keeps in `Logo\Current\HerFig_<slot>.png` (the logos sprite file 1 is
    /// composed from, `s4_formats::logo`), here in memory: the game takes it at the start.
    pub logos: [Option<Vec<u8>>; 8],
    /// The game settings' logo list (`Logo\*.PNG`, 0x51c7e0), read when they open.
    logo_list: Vec<std::path::PathBuf>,
    /// The end statistics (`CStateEndStatistic`, endstat.rs): the record, how the campaign
    /// mission ended (victory.rs: what follows), mode 1.
    end: Option<(endstat::EndRecord, Option<victory::Outcome>, bool)>,
    /// Its detail dialog 21 (0x47e030) while shown.
    end_detail: Option<endstat::Detail>,
    /// Game type 3's end screen instead (`CStateEcoStatistic`, race_end.rs): the goods race rows.
    race_goods: Option<race_end::Goods>,
    /// The credits (`CStateCredits`, credits.rs) and the time of their last frame
    /// ([`Frontend::credits_shown`] for the menu music).
    credits: Option<(credits::Credits, std::time::Instant)>,
    /// The random map parameters (`CStateMDRandomMapParameters`, random.rs) while shown.
    random: Option<random::Random>,
    /// The GUI sounds the procedures played since the shell last took them
    /// ([`Frontend::take_sounds`]).
    sounds: Vec<u16>,
    /// A video queued before the next screen (`CStateVideo` 0x528360, docs/VIDEO.md): its number
    /// and that screen, for the shell ([`Frontend::take_video`], [`Frontend::video_done`]).
    video: Option<(u8, AfterVideo)>,
}

/// The state queued after a video.
#[derive(Clone, Copy, Debug, PartialEq)]
enum AfterVideo {
    /// The campaign briefing (0x510610).
    Briefing,
    /// The pack's briefing dialog (the Add-On's 0x50dda0).
    PackBriefing(usize),
    /// What `EndGameGoToStatistics` queued after the video.
    End(Option<victory::After>),
}

/// `MapFilePaths::GetMapDirectoryPath`: 0 single player, 1 multiplayer, 2 user maps.
/// A map file's list entry for the game kind (`multiplayer`), or None when the kind cannot use
/// it: read once per file (by path, modified time and length) and kept for the session. Every
/// list read every map's headers again (~36 ms a list; the app's start warms them,
/// [`warm_map_lists`]).
fn map_entry(p: &std::path::Path, multiplayer: bool) -> Option<MapEntry> {
    type Key = (PathBuf, bool);
    type Entry = (std::time::SystemTime, u64, Option<MapEntry>);
    static CACHE: std::sync::Mutex<Option<std::collections::HashMap<Key, Entry>>> = std::sync::Mutex::new(None);
    let meta = std::fs::metadata(p).ok()?;
    let stamp = (meta.modified().ok()?, meta.len());
    let key = (p.to_path_buf(), multiplayer);
    if let Some(e) = CACHE.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(Default::default).get(&key).filter(|e| (e.0, e.1) == stamp) {
        return e.2.clone();
    }
    let read = || -> Option<MapEntry> {
        let m = s4_formats::map::MapFile::open(p).ok()?;
        let g = m.general().ok()?;
        // 0x5201e0: only the maps the game kind can use (+0x341 local, +0x340 network).
        let r = setup::MapRead::open(p, 0, !multiplayer).ok()?;
        if !(if multiplayer { r.network } else { r.local }) {
            return None;
        }
        let name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        Some(MapEntry { name, path: p.to_path_buf(), players: g.player_count, size: g.size, modes: r.modes, fixed: !r.can_players })
    };
    let e = read();
    CACHE.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(Default::default).insert(key, (stamp.0, stamp.1, e.clone()));
    e
}

/// Read every map list's entries ahead (the app's start, off the main thread).
pub fn warm_map_lists() {
    for category in 0..3 {
        let Ok(rd) = std::fs::read_dir(map_dir(category)) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("map")) {
                map_entry(&p, false);
                map_entry(&p, true);
            }
        }
    }
}

fn map_dir(category: u32) -> PathBuf {
    let name = match category {
        1 => "Multiplayer",
        2 => "User",
        _ => "Singleplayer",
    };
    s4_formats::path::resolve(s4_formats::game_dir().join("Map").join(name))
}

impl Frontend {
    /// Open the main menu on the GPU's GUI layer (no world is drawn).
    pub fn new(gpu: &mut Gpu, language: u32) -> anyhow::Result<Frontend> {
        let texts = TextTable::open(s4_formats::game_dir(), language).inspect_err(|e| tracing::warn!("texts: {e}")).ok();
        gpu.terrain = None;
        gpu.sprites = None;
        gpu.camera_window = None;
        gpu.show_gui = true;
        gpu.ui = Some(Gui::from_set(SET, SCREEN.1, &[])?);
        let progress = crate::progress::Progress::load();
        let mut f = Frontend {
            texts,
            screen: MAIN,
            category: 0,
            maps: Vec::new(),
            top: 0,
            selected: None,
            progress,
            mission: None,
            speech: None,
            language,
            net: net::NetUi::None,
            joined_name: String::new(),
            internet: false,
            pending_save: None,
            multiplayer: false,
            saves: Vec::new(),
            load_top: 0,
            load_sel: None,
            load_kind: 1,
            msg_back: MAIN,
            slideshow: false,
            splash: None,
            map_read: None,
            choice: Default::default(),
            difficulty: 0,
            row: None,
            pack: None,
            lan_port: s4_net::lobby::LAN_PORT,
            save_dir: None,
            logos: s4_formats::logo::installed_current(&s4_formats::game_dir()),
            logo_list: Vec::new(),
            end: None,
            end_detail: None,
            race_goods: None,
            credits: None,
            random: None,
            sounds: Vec::new(),
            video: None,
        };
        f.open(gpu, MAIN);
        Ok(f)
    }

    /// The GUI set shown: the procedures behind its dialog numbers (sound/gui.rs).
    fn start_set(&self) -> StartSet {
        if self.random.is_some() {
            return StartSet::MissionCd;
        }
        match self.pack {
            None => StartSet::Main,
            Some(addon::Pack::Ao) => StartSet::AddOn,
            Some(addon::Pack::Md) => StartSet::MissionCd,
            Some(addon::Pack::Mcd2) => StartSet::MissionCd2,
            Some(addon::Pack::Xmd3) => StartSet::MissionCd3,
        }
    }

    /// The procedures' sounds for the GUI calls so far, before the set they belong to goes.
    fn flush_calls(&mut self, gpu: &mut Gpu) {
        let procs = Procs::Start(self.start_set());
        let calls = crate::sound::drain_gui_calls(gpu);
        self.sounds.extend(calls.iter().flat_map(|c| crate::sound::gui::sounds(procs, c)));
    }

    /// A `PlaySound(id, 100, ..)` of a procedure outside the GUI calls (a list scroll that
    /// moved), after the calls before it.
    fn play(&mut self, gpu: &mut Gpu, id: u16) {
        self.flush_calls(gpu);
        self.sounds.push(id);
    }

    /// The GUI sounds played since the last call, in order.
    pub fn take_sounds(&mut self, gpu: &mut Gpu) -> Vec<u16> {
        self.flush_calls(gpu);
        std::mem::take(&mut self.sounds)
    }

    /// `EndGameGoToStatistics` 0x514450 → `CStateEndStatistic` 0x512c50: dialog 13 with the
    /// record; its OK goes on to the main menu, or a campaign mission's selection screen.
    pub fn end_statistic(gpu: &mut Gpu, language: u32, rec: endstat::EndRecord, campaign: Option<victory::Outcome>) -> anyhow::Result<Frontend> {
        let mut f = Frontend::new(gpu, language)?;
        f.end = Some((rec, campaign, true));
        f.open(gpu, endstat::DIALOG);
        Ok(f)
    }

    /// `EndGameGoToStatistics` for game type 3 → `CStateEcoStatistic` 0x5128e0: dialog 22 with
    /// the goods race's rows (the info object 0x11ab0b0 as D77's fill left it).
    pub fn race_end(gpu: &mut Gpu, language: u32, rec: endstat::EndRecord, goods: race_end::Goods, campaign: Option<victory::Outcome>) -> anyhow::Result<Frontend> {
        let mut f = Frontend::new(gpu, language)?;
        f.end = Some((rec, campaign, false));
        f.race_goods = Some(goods);
        f.open(gpu, race_end::DIALOG);
        Ok(f)
    }

    /// The fill 0x491f70 in the current mode.
    fn race_fill(&mut self, gpu: &mut Gpu) {
        if let (Some((rec, _, mode1)), Some(goods), Some(ui)) = (self.end.as_ref(), self.race_goods.as_ref(), gpu.ui.as_mut()) {
            let texts = &self.texts;
            let strings = |id: u32| texts.as_ref().and_then(|t| t.get(id as usize)).unwrap_or("").to_string();
            race_end::fill(&mut ui.state, rec, goods, &strings, random_map(&rec.path), *mode1);
        }
    }

    /// The end statistics' event 0x74 (the goods race's 0x77): the next state.
    fn end_done(&mut self, gpu: &mut Gpu) {
        self.race_goods = None;
        let Some((_, mission, _)) = self.end.take() else { return };
        // `EndGameGoToStatistics` 0x514450: what it queued after the statistics, a won
        // campaign's last video first (0x5145c7 / 0x514643 / 0x514668).
        let after = mission.map(victory::after);
        match mission.and_then(victory::video) {
            Some(n) => self.video = Some((n, AfterVideo::End(after))),
            None => self.after_end(gpu, after),
        }
    }

    fn after_end(&mut self, gpu: &mut Gpu, after: Option<victory::After>) {
        match after {
            Some(victory::After::Victory(c)) => {
                self.open(gpu, victory::DIALOG);
                if let Some(ui) = gpu.ui.as_mut() {
                    victory::init(&mut ui.state, c);
                }
                render(gpu);
            }
            // [inf] The submenu codes (6..=0x11) open the screens the add-on briefings go back to.
            Some(victory::After::Campaign(c)) => match addon::Pack::of(c) {
                Some(pack) => {
                    self.enter_pack(gpu, pack);
                    let screen = pack.back_from_briefing(c);
                    if screen != 0 {
                        self.open_pack(gpu, screen);
                    }
                }
                None => self.open(gpu, if c == 4 { CAMPAIGN_DARK } else { CAMPAIGN_3X3 }),
            },
            Some(victory::After::Splash(c, param)) => self.open_splash(gpu, c, param),
            Some(victory::After::Menu) | None => self.open(gpu, MAIN),
        }
    }

    /// The video the last call queued (its number, docs/VIDEO.md): the shell plays it (or not),
    /// then calls [`Frontend::video_done`].
    pub fn take_video(&mut self) -> Option<u8> {
        self.video.map(|v| v.0)
    }

    /// The queued video ended (or was skipped): the screen queued after it.
    pub fn video_done(&mut self, gpu: &mut Gpu) {
        match self.video.take().map(|v| v.1) {
            Some(AfterVideo::Briefing) => self.open(gpu, BRIEFING),
            Some(AfterVideo::PackBriefing(b)) => self.open_pack(gpu, b),
            Some(AfterVideo::End(after)) => self.after_end(gpu, after),
            None => {}
        }
    }

    /// `CStateAOSplash` 0x5101c0: the Add-On's set, dialog 8 with the init 0x46b900, its speech.
    fn open_splash(&mut self, gpu: &mut Gpu, campaign: u8, param: i32) {
        self.enter_pack(gpu, addon::Pack::Ao);
        let Some((_, story, n)) = splash::content(param) else { return };
        let story = self.text(story as usize);
        if let Some(ui) = gpu.ui.as_mut() {
            let mut ev = Vec::new();
            for d in ui.state.open.clone() {
                ui.state.close(d, &mut ev);
            }
            ui.state.open(splash::DIALOG, &mut ev);
            splash::init(&mut ui.state, param, story);
        }
        self.screen = splash::DIALOG;
        self.splash = Some(campaign);
        self.play_file(Some(splash::speech(&s4_formats::game_dir(), n)));
        render(gpu);
    }

    /// Event 0x1f57 (0x80 or Esc): the speech stopped, the campaign's screen.
    fn close_splash(&mut self, gpu: &mut Gpu) {
        let Some(c) = self.splash.take() else { return };
        self.speech = None;
        if let Some(pack) = addon::Pack::of(c) {
            let screen = pack.back_from_briefing(c);
            self.open_pack(gpu, screen);
        }
    }

    /// After a tutorial mission (no statistics): `OpenMainMenu` code 3, the tutorial menu.
    pub fn after_tutorial(gpu: &mut Gpu, language: u32) -> anyhow::Result<Frontend> {
        let mut f = Frontend::new(gpu, language)?;
        f.open(gpu, TUTORIAL);
        Ok(f)
    }

    /// A key down (event 0xb): `CStateVictoryScreen` 0x528180 ends on any key, as on events 8 /
    /// 10, with the main menu queued after it. Returns whether the screen took it.
    pub fn key_down(&mut self, gpu: &mut Gpu) -> bool {
        if self.screen == victory::DIALOG {
            self.open(gpu, MAIN);
            return true;
        }
        false
    }

    /// Esc (key event 0xb, 0x1b) on the end statistics (0x512e00): event 0x74. Returns whether
    /// the screen took it.
    pub fn escape(&mut self, gpu: &mut Gpu) -> bool {
        // `CStateMDRandomMapParameters` 0x5265a0: key 0x1b → event 0x1b6a (its back).
        if self.random.is_some() {
            let d = self.screen;
            let _ = self.random_command(gpu, d, random::BACK, 0);
            return true;
        }
        // The credits (0x511b00: key 0x1b → event 0x6d).
        if self.credits.is_some() {
            self.close_credits(gpu);
            return true;
        }
        // `CStateAOSplash` 0x510330: the speech stopped, the state ends.
        if self.splash.is_some() {
            self.close_splash(gpu);
            return true;
        }
        // `CStateLoadType` 0x51b060: key 0x1b → event 0x5d (the main menu); `CStateLoadGame`
        // 0x51a430: → 0x5e (the load type). The add-on sets' screens reuse these ids.
        match self.screen {
            _ if self.pack.is_some() => {}
            load::DIALOG => {
                self.open(gpu, MAIN);
                return true;
            }
            LOAD => {
                self.open(gpu, load::DIALOG);
                return true;
            }
            // `CStateMainMenu` 0x522b70 has no key 0x1b case: Esc does nothing there.
            MAIN => return true,
            // The other states send their back button's event: `CStateLocalType` 0x522270 99
            // (2403), `CStateCampaign3X3` 0x5113e0 0x67 (2037), `CStateCampaignDark` 0x5116f0
            // 0x65 (2080), `CStateBriefing` 0x510970 0x70 (2055), `CStateTutorial` 0x527c10 0x69
            // (0xaed).
            LOCAL_TYPE | CAMPAIGN_3X3 | CAMPAIGN_DARK | BRIEFING | TUTORIAL => {
                let back = match self.screen {
                    LOCAL_TYPE => 2403,
                    CAMPAIGN_3X3 => 2037,
                    CAMPAIGN_DARK => 2080,
                    BRIEFING => 2055,
                    _ => 0xaed,
                };
                let screen = self.screen;
                self.command(gpu, screen, back, 0);
                return true;
            }
            // The lobby states, their back buttons' events: `CStateLobbyMultiplayerType` 0x521c10
            // 0x36 (2523), `CStateLobbyConnect` 0x51ba50 0x3a (2273), `CStateLobbyMapSettings`
            // 0x520c90 0x3f (2415), `CStateLobbyGameSettings` 0x51d460 0x4d (2141) unless
            // 0x149c13c [inf] = while 2141 is enabled. (With their message boxes up, flags
            // 0x15ab034 / 0x15ab035, they send 0x72 instead: ours show none.)
            net::MP_TYPE | net::GAME_LIST | MAP_SETTINGS | LOCAL_MAP | net::LOBBY | LOCAL_GAME => {
                let screen = self.screen;
                let back = match screen {
                    net::MP_TYPE => 2523,
                    net::GAME_LIST => 2273,
                    MAP_SETTINGS | LOCAL_MAP => 2415,
                    _ => 2141,
                };
                let enabled = gpu.ui.as_ref().is_some_and(|u| u.state.index_of(screen, back).is_some_and(|k| u.state.effects.get(&(screen, k)).is_none_or(|e| e & 4 == 0)));
                // The back buttons ask the shell for nothing.
                if enabled {
                    let _ = self.command(gpu, screen, back, 0);
                }
                return true;
            }
            // Over the message box Esc is the opener state's: `CStateTutorial` → event 0x69 (the
            // main menu), `CStateBriefing` → 0x70 (as its back button); `CStateMainMenu` ignores
            // it.
            msgbox::DIALOG => {
                match self.msg_back {
                    TUTORIAL => self.open(gpu, MAIN),
                    net::GAME_LIST => self.open(gpu, net::GAME_LIST),
                    BRIEFING => {
                        self.command(gpu, BRIEFING, 2055, 0);
                    }
                    _ => {}
                }
                return true;
            }
            _ => {}
        }
        if self.end.is_none() {
            return false;
        }
        match self.screen {
            endstat::DETAIL => self.end_back(gpu),
            // 0x512e00 / `CStateEcoStatistic` 0x512a50: Esc → 0x74 / 0x77.
            endstat::DIALOG | race_end::DIALOG => self.end_done(gpu),
            _ => return false,
        }
        render(gpu);
        true
    }

    /// Open the message box (dialog 20) with `text` over every other dialog; its OK opens `back`.
    fn message(&mut self, gpu: &mut Gpu, text: String, back: usize) {
        self.open(gpu, msgbox::DIALOG);
        self.msg_back = back;
        if let Some(ui) = gpu.ui.as_mut() {
            let texts = &self.texts;
            let strings = |id: u32| texts.as_ref().and_then(|t| t.get(id as usize)).unwrap_or("").to_string();
            msgbox::init(&mut ui.state, &strings, text);
        }
        render(gpu);
    }

    /// Space (event 0xd, key 0x20): `CStateSlideshow` 0x527820 shows its next slide; none, so the
    /// game ends (true).
    pub fn space(&mut self) -> bool {
        self.slideshow
    }

    /// Event 0x75: dialog 21 closed, dialog 13 opened again (its init: mode 1).
    fn end_back(&mut self, gpu: &mut Gpu) {
        self.end_detail = None;
        self.open(gpu, endstat::DIALOG);
    }

    /// Back from a campaign mission (`CStateGame::EndGame` → `CStateMainMenu`, which goes on to
    /// the campaign's selection screen): dialog 8, or 9 for the Dark Tribe.
    pub fn after_mission(gpu: &mut Gpu, language: u32, campaign: u8) -> anyhow::Result<Frontend> {
        let mut f = Frontend::new(gpu, language)?;
        match addon::Pack::of(campaign) {
            Some(pack) => f.enter_pack(gpu, pack),
            None => f.open(gpu, if campaign == 4 { CAMPAIGN_DARK } else { CAMPAIGN_3X3 }),
        }
        Ok(f)
    }

    /// Main menu events 9001 / 9501: the pack's GUI set and its map screen.
    fn enter_pack(&mut self, gpu: &mut Gpu, pack: addon::Pack) {
        self.flush_calls(gpu);
        match Gui::from_set(pack.links().set, SCREEN.1, &[]) {
            Ok(ui) => gpu.ui = Some(ui),
            Err(e) => {
                tracing::warn!("{}: {e}", pack.links().set);
                return;
            }
        }
        self.pack = Some(pack);
        self.open_pack(gpu, 0);
    }

    /// Back to the start screens' GUI set (the add-on object's vtable +0xc) and the main menu.
    fn leave_pack(&mut self, gpu: &mut Gpu) {
        self.flush_calls(gpu);
        self.pack = None;
        match Gui::from_set(SET, SCREEN.1, &[]) {
            Ok(ui) => gpu.ui = Some(ui),
            Err(e) => tracing::warn!("{SET}: {e}"),
        }
        self.open(gpu, MAIN);
    }

    /// A dialog of the pack's set with its init (0x494cb0 / 0x4b02e0 map screen, 0x4947f0 /
    /// 0x4afd40 briefing).
    fn open_pack(&mut self, gpu: &mut Gpu, dialog: usize) {
        let Some(pack) = self.pack else { return };
        let l = pack.links();
        let Some(ui) = gpu.ui.as_mut() else { return };
        let mut ev = Vec::new();
        for d in ui.state.open.clone() {
            ui.state.close(d, &mut ev);
        }
        ui.state.open(dialog, &mut ev);
        self.screen = dialog;
        if matches!(pack, addon::Pack::Md | addon::Pack::Ao) && dialog == 0 {
            // 0x4965b0 / 0x46a6e0: the texts, the Mission CD bonus while its status is set, the
            // difficulty.
            self.progress = crate::progress::Progress::load();
            for &(link, id) in pack.menu().1 {
                let t = self.text(id);
                set_text(gpu, 0, link, &t);
            }
            if pack == addon::Pack::Md {
                set_visible(gpu, 0, addon::MD_BONUS, self.progress.status(10) != 0);
            }
            let radio = if self.difficulty == 0 { l.easy } else { l.normal };
            if let Some(ui) = gpu.ui.as_mut() {
                ui.state.select(0, radio, &mut Vec::new());
            }
        } else if let Some(screen) = pack.screens().iter().find(|s| s.dialog == dialog) {
            // The campaign screens' inits: title, mission labels, mission k + 1 after k wins.
            self.progress = crate::progress::Progress::load();
            let t = self.text(screen.title.1);
            set_text(gpu, dialog, screen.title.0, &t);
            for &(link, campaign, level, label) in screen.missions {
                let t = self.text(label);
                set_text(gpu, dialog, link, &t);
                set_enabled(gpu, dialog, link, self.progress.status(campaign) >= level);
            }
        } else if dialog == 0 {
            self.progress = crate::progress::Progress::load();
            let status = |t: u8| self.progress.status(t);
            for &(first, campaign) in &l.missions {
                for k in 0..l.missions_per {
                    set_visible(gpu, 0, first + k, addon::visible(pack, campaign, k as u32, &status));
                }
            }
            if pack == addon::Pack::Mcd2 {
                for &(icon, campaign, k) in &addon::MCD2_MARKERS {
                    set_visible(gpu, 0, icon, addon::visible(pack, campaign, k, &status));
                }
            }
            let radio = if self.difficulty == 0 { l.easy } else { l.normal };
            if let Some(ui) = gpu.ui.as_mut() {
                ui.state.select(0, radio, &mut Vec::new());
            }
        } else if dialog == l.brief
            && let Some((campaign, level)) = self.mission
            && let Some([goal, story, _, title]) = addon::briefing(campaign, level)
        {
            let b = l.brief;
            let (header, goal, story, title) = (self.text(880), self.text(goal), self.text(story), self.text(title));
            set_text(gpu, b, l.header, &header);
            set_text(gpu, b, l.title, &title);
            set_text(gpu, b, l.goal, &goal);
            set_text(gpu, b, l.story, &story);
            set_text(gpu, b, l.tips, "");
            if let Some(ui) = gpu.ui.as_mut() {
                ui.state.scroll.insert((b, l.story), 0);
            }
            if pack == addon::Pack::Ao {
                self.play_file(addon::speech(campaign, level, self.language));
            }
            match pack {
                addon::Pack::Ao | addon::Pack::Md | addon::Pack::Mcd2 => {
                    if let (Some(img), Some(ui)) = (addon::picture(campaign, level), gpu.ui.as_mut()) {
                        ui.state.images.insert((b, l.picture), (img, 0));
                    }
                }
                addon::Pack::Xmd3 => {
                    let img = addon::map_path(campaign, level).and_then(|p| preview(&p).ok());
                    if let (Some(img), Some(ui)) = (img, gpu.ui.as_mut()) {
                        ui.state.owner_images.insert((b, l.picture), img);
                    }
                }
            }
            set_visible(gpu, b, l.picture, false);
        }
        render(gpu);
    }

    /// The pack's buttons (procs 0x495270 / 0x4b0450 and 0x494a50 / 0x4aff80).
    fn pack_command(&mut self, gpu: &mut Gpu, dialog: usize, link: u16, effects: u8) -> Option<Action> {
        let pack = self.pack?;
        let l = pack.links();
        let b = l.brief;
        match (dialog, link) {
            (0, x) if x == l.easy => self.difficulty = 0,
            (0, x) if x == l.normal => self.difficulty = 1,
            (0, x) if x == l.back => self.leave_pack(gpu),
            // The Mission CD menu: a campaign's screen or the bonus's briefing.
            (0, x) if !pack.screens().is_empty() => match pack.menu().0.iter().find(|m| m.0 == x).map(|m| m.1) {
                Some(Some(screen)) => self.open_pack(gpu, screen),
                Some(None) => {
                    self.mission = Some((10, 0));
                    self.open_pack(gpu, b);
                }
                None => {}
            },
            (d, x) if pack.screens().iter().any(|s| s.dialog == d && s.back == x) => self.open_pack(gpu, 0),
            (d, x) if d != b => {
                if let Some(m) = pack.mission(d, x) {
                    self.mission = Some(m);
                    // 0x50fcf1: the Trojans' first mission has TroIntro (8) before the briefing.
                    if m == (15, 0) {
                        self.video = Some((8, AfterVideo::PackBriefing(b)));
                    } else {
                        self.open_pack(gpu, b);
                    }
                }
            }
            (_, x) if x == l.brief_back => {
                self.speech = None;
                let back = self.mission.map_or(0, |m| pack.back_from_briefing(m.0));
                self.open_pack(gpu, back);
            }
            (_, x) if x == l.up || x == l.down => {
                if let Some(ui) = gpu.ui.as_mut()
                    && ui.scroll_text(b, l.story, if x == l.up { -1 } else { 1 })
                {
                    render(gpu);
                }
            }
            (_, x) if x == l.tips_toggle => {
                let pressed = effects & 1 != 0;
                let tips = match self.mission.and_then(|(c, lv)| addon::briefing(c, lv)) {
                    Some(s) if pressed => self.text(s[2]),
                    _ => String::new(),
                };
                set_text(gpu, b, l.tips, &tips);
                set_visible(gpu, b, l.picture, pressed);
                render(gpu);
            }
            (_, x) if x == l.start => {
                self.speech = None;
                let (campaign, level) = self.mission?;
                let path = addon::map_path(campaign, level)?;
                if !path.exists() {
                    tracing::warn!("{}: {}", self.text(2394), path.display());
                    return None;
                }
                let players = setup::campaign_setup(&path, self.difficulty);
                return Some(Action::Start(path, Some((campaign, level)), players));
            }
            _ => {}
        }
        None
    }

    /// Fit the 800×600 screens into the window, centred (a modern extra: the original switched
    /// the display mode to 800×600).
    pub fn fit(gpu: &mut Gpu) {
        let (w, h) = gpu.size();
        let s = (w as f32 / SCREEN.0 as f32).min(h as f32 / SCREEN.1 as f32).max(0.1);
        gpu.gui_scale = s;
        gpu.gui_origin = (((w as f32 - SCREEN.0 as f32 * s) / 2.0).floor().max(0.0), ((h as f32 - SCREEN.1 as f32 * s) / 2.0).floor().max(0.0));
    }

    fn text(&self, id: usize) -> String {
        self.texts.as_ref().and_then(|t| t.get(id)).unwrap_or("").to_string()
    }

    /// Close the current screen's dialog and open `dialog` with its init.
    fn open(&mut self, gpu: &mut Gpu, dialog: usize) {
        let Some(ui) = gpu.ui.as_mut() else { return };
        let mut ev = Vec::new();
        for d in ui.state.open.clone() {
            ui.state.close(d, &mut ev);
        }
        ui.state.open(dialog, &mut ev);
        self.screen = dialog;
        match dialog {
            // The procedure 0x493c80's code 0: mode 0, the fill 0x491f70.
            race_end::DIALOG if self.race_goods.is_some() => {
                if let Some((_, _, mode1)) = self.end.as_mut() {
                    *mode1 = false;
                }
                self.race_fill(gpu);
            }
            // The procedure 0x491a30's code 0: mode 1, the fill 0x490fe0, SelectControl(13, 0xa19).
            endstat::DIALOG if self.end.is_some() => {
                if let (Some((rec, _, mode1)), Some(ui)) = (self.end.as_mut(), gpu.ui.as_mut()) {
                    *mode1 = true;
                    let texts = &self.texts;
                    let strings = |id: u32| texts.as_ref().and_then(|t| t.get(id as usize)).unwrap_or("").to_string();
                    endstat::fill(&mut ui.state, rec, &strings, random_map(&rec.path), true);
                    let mut ev = Vec::new();
                    ui.state.select(endstat::DIALOG, endstat::MODE1, &mut ev);
                }
            }
            MAIN => {
                for &(link, _, text, visible) in &MAIN_BUTTONS {
                    let t = self.text(text);
                    set_text(gpu, MAIN, link, &t);
                    set_visible(gpu, MAIN, link, visible);
                }
            }
            TUTORIAL => {
                // The init 0x4941a0: the title 0xaec and the twelve buttons' texts.
                let t = self.text(0x835);
                set_text(gpu, TUTORIAL, 0xaec, &t);
                for &(link, text, _) in &TUTORIAL_BUTTONS {
                    let t = self.text(text);
                    set_text(gpu, TUTORIAL, link, &t);
                }
            }
            LOCAL_TYPE => {
                // The init (0x48d100): 2405 = string 888, 2402 = 2071; the difficulty radio (0 here).
                let (a, b) = (self.text(888), self.text(2071));
                set_text(gpu, LOCAL_TYPE, 2405, &a);
                set_text(gpu, LOCAL_TYPE, 2402, &b);
                let radio = if self.difficulty == 0 { 2413 } else { 2414 };
                if let Some(ui) = gpu.ui.as_mut() {
                    ui.state.select(LOCAL_TYPE, radio, &mut Vec::new());
                }
            }
            CAMPAIGN_3X3 => {
                // The init (0x47bf90): header, the nine mission buttons' texts, then
                // EnableControl(…, 0) for the missions after status + 1.
                self.progress = crate::progress::Progress::load();
                let header = self.text(2324);
                set_text(gpu, CAMPAIGN_3X3, 2036, &header);
                for (first, campaign, strings) in [(2044u16, 1u8, 2065usize), (2038, 2, 2103), (2041, 3, 2036)] {
                    for k in 0..3u16 {
                        let t = self.text(strings + k as usize);
                        set_text(gpu, CAMPAIGN_3X3, first + k, &t);
                        set_enabled(gpu, CAMPAIGN_3X3, first + k, self.progress.open(campaign, k as u32));
                    }
                }
            }
            CAMPAIGN_DARK => {
                // The init (0x47d7b0): title 2264, the twelve names, missions after status + 1 off.
                self.progress = crate::progress::Progress::load();
                let title = self.text(2264);
                set_text(gpu, CAMPAIGN_DARK, 2079, &title);
                const NAMES: [usize; 12] = [792, 793, 796, 797, 798, 799, 3739, 3740, 3741, 3742, 3743, 3744];
                for (k, &id) in NAMES.iter().enumerate() {
                    let t = self.text(id);
                    set_text(gpu, CAMPAIGN_DARK, 2081 + k as u16, &t);
                    set_enabled(gpu, CAMPAIGN_DARK, 2081 + k as u16, self.progress.open(4, k as u32));
                }
            }
            BRIEFING => {
                // The init (0x47cb40): 2072 / 2074 / 2075 hidden, texts, 2058 cleared, the picture.
                let Some((campaign, level)) = self.mission else { return };
                let ids = briefing_strings(campaign).map(|i| i + level as usize);
                let (goal, story, title) = (self.text(ids[0]), self.text(ids[1]), self.text(ids[3]));
                let header = self.text(880);
                set_visible(gpu, BRIEFING, 2072, false);
                set_text(gpu, BRIEFING, 2057, &story);
                set_text(gpu, BRIEFING, 2054, &header);
                set_text(gpu, BRIEFING, 2068, &title);
                set_text(gpu, BRIEFING, 2069, &goal);
                set_visible(gpu, BRIEFING, 2074, false);
                set_visible(gpu, BRIEFING, 2075, false);
                if let Some(ui) = gpu.ui.as_mut() {
                    ui.state.images.insert((BRIEFING, 2074), (briefing_picture(campaign, level), 0));
                }
                set_text(gpu, BRIEFING, 2058, "");
                if let Some(ui) = gpu.ui.as_mut() {
                    ui.state.scroll.insert((BRIEFING, 2057), 0);
                }
                self.play_speech(campaign, level);
            }
            MAP_SETTINGS | LOCAL_MAP => {
                // The init (0x48e1c0): the labels; the single player maps only locally.
                for (link, id) in [(2416, 2017), (2444, 2062), (2448, 2073), (2474, 2033), (2482, 2053), (2481, 2050), (2483, 2055)] {
                    let t = self.text(id);
                    set_text(gpu, dialog, link, &t);
                }
                set_enabled(gpu, dialog, 2449, dialog == LOCAL_MAP);
                self.top = 0;
                // 0x48e5e0 code 0 sets `DAT_015aabac` around its init, and its code 1 plays nothing
                // while it is set: the category select here is silent (code 3 of 2449..=2451
                // plays nothing anyway).
                let before = gpu.ui.as_ref().map_or(0, |u| u.state.calls.len());
                self.refresh_list(gpu);
                if let Some(ui) = gpu.ui.as_mut() {
                    ui.state.calls.truncate(before);
                }
            }
            net::MP_TYPE | net::GAME_LIST | net::LOBBY | LOCAL_GAME => self.open_net(gpu, dialog),
            load::DIALOG => {
                let t = self.text(0x4d0);
                if let Some(ui) = gpu.ui.as_mut() {
                    load::init(&mut ui.state, t);
                }
            }
            LOAD => {
                // InitGuiDlgMainLoad: the title (string 1232, release 0x48cac1), the list.
                let t = self.text(1232);
                set_text(gpu, LOAD, 2338, &t);
                // The list 0x51a5c0 keeps the saves of the chosen kind (0x51abb0); the init
                // selects the first (0x15aaba8 = 0).
                let kind = self.load_kind;
                self.saves = crate::menu::list_saves().into_iter().filter(|s| load::keeps(&s.1, kind)).collect();
                self.load_top = 0;
                self.load_sel = (!self.saves.is_empty()).then_some(0);
                self.fill_saves(gpu);
            }
            _ => {}
        }
        render(gpu);
    }

    /// `CStateLobbyMapSettings::RefreshMapList`: every `*.map` of the category's folder (the user
    /// category adds a second folder in the original; not known here), sorted by name
    /// (`Compare` case 0, `stricmp`), the first selected.
    fn refresh_list(&mut self, gpu: &mut Gpu) {
        let mut maps = Vec::new();
        if let Ok(rd) = std::fs::read_dir(map_dir(self.category)) {
            for e in rd.flatten() {
                let p = e.path();
                if !p.extension().is_some_and(|x| x.eq_ignore_ascii_case("map")) {
                    continue;
                }
                // 0x5201e0: only the maps the game kind can use.
                if let Some(m) = map_entry(&p, self.multiplayer) {
                    maps.push(m);
                }
            }
        }
        maps.sort_by_key(|m| m.name.to_ascii_lowercase());
        self.maps = maps;
        self.selected = (!self.maps.is_empty()).then_some(0);
        // The category radio group (2449 / 2450 / 2451).
        let radio = [2449, 2450, 2451][self.category.min(2) as usize];
        let dlg = self.map_dlg();
        if let Some(ui) = gpu.ui.as_mut() {
            ui.state.select(dlg, radio, &mut Vec::new());
        }
        self.apply_map(gpu);
    }

    /// The map settings dialog of the game kind (0x5214f0).
    fn map_dlg(&self) -> usize {
        if self.multiplayer { MAP_SETTINGS } else { LOCAL_MAP }
    }

    /// 0x521540: the selected map read with team setup `setup`; players, teams, mode and goods
    /// from it.
    fn read_map(&mut self, setup: usize) {
        let local = !self.multiplayer;
        self.map_read = self.selected.and_then(|i| setup::MapRead::open(&self.maps[i].path, setup, local).inspect_err(|e| tracing::warn!("map: {e}")).ok());
        self.choice = self.map_read.as_ref().map(|m| setup::Choice::of(m, setup)).unwrap_or_default();
    }

    /// 0x48dda0 / 0x48d570: the choices' texts.
    fn show_choice(&self, gpu: &mut Gpu) {
        let dlg = self.map_dlg();
        let c = self.choice;
        set_text(gpu, dlg, 2443, &c.players.to_string());
        set_text(gpu, dlg, 2447, &c.teams.to_string());
        let mode = setup::mode_text(c.mode).map(|t| self.text(t)).unwrap_or_default();
        let goods = setup::goods_text(c.goods).map(|t| self.text(t)).unwrap_or_default();
        set_text(gpu, dlg, 2484, &mode);
        set_text(gpu, dlg, 2485, &goods);
        let name = self.map_read.as_ref().map(|m| m.setup_name.clone()).unwrap_or_default();
        set_text(gpu, dlg, 2486, &name);
        // 0x48d600: the buttons the map and the mode allow.
        if let Some(m) = &self.map_read {
            let [players, teams, goods, setups] = c.enabled(m);
            for (links, on) in [([2441, 2442], players), ([2445, 2446], teams), ([2489, 2490], goods), ([2491, 2492], setups)] {
                for l in links {
                    set_enabled(gpu, dlg, l, on);
                }
            }
        }
    }

    /// `ApplyMapData` + `GuiDlgMapSettingsUpdate` + `FillList`.
    fn apply_map(&mut self, gpu: &mut Gpu) {
        self.fill_list(gpu);
        self.read_map(0);
        self.show_choice(gpu);
        let dlg = self.map_dlg();
        let preview = self.selected.and_then(|i| preview(&self.maps[i].path).inspect_err(|e| tracing::warn!("preview: {e}")).ok());
        if let Some(ui) = gpu.ui.as_mut() {
            match preview {
                Some(img) => ui.state.owner_images.insert((dlg, 2495), img),
                None => ui.state.owner_images.remove(&(dlg, 2495)),
            };
        }
        // 2433 (start) is enabled when there is a map.
        set_enabled(gpu, dlg, 2433, !self.maps.is_empty());
    }

    /// The list fill (0x48d600): seven rows of (name 2417+i, size 2434+i, players 2424+i).
    fn fill_list(&self, gpu: &mut Gpu) {
        let dlg = self.map_dlg();
        for i in 0..7 {
            let row = self.top + i;
            let links = [2417 + i as u16, 2434 + i as u16, 2424 + i as u16];
            match self.maps.get(row) {
                Some(m) => {
                    let font = if Some(row) == self.selected { 2 } else { 1 };
                    let players = if m.fixed { m.players.to_string() } else { format!("1-{}", m.players) };
                    for (l, t) in links.iter().zip([m.name.clone(), format!(" {} ", m.size), players]) {
                        set_text(gpu, dlg, *l, &t);
                        set_visible(gpu, dlg, *l, true);
                        if let Some(ui) = gpu.ui.as_mut() {
                            ui.state.fonts.insert((dlg, *l), font);
                        }
                    }
                }
                None => {
                    for l in links {
                        set_visible(gpu, dlg, l, false);
                    }
                }
            }
            // The three mode marks (0x48d600): 2453 + i conflict (+0x40), 2460 + i +0x41
            // (cooperation) locally / +0x44 (free settle) in a network game (0x149c1d4), 2467 + i
            // economic (+0x42). [inf] Both of the middle column's flags stay clear in those games.
            let modes = self.maps.get(self.top + i).map_or([false; 5], |m| m.modes);
            let middle = if self.multiplayer { modes[4] } else { modes[1] };
            for (l, on) in [(2453 + i as u16, modes[0]), (2460 + i as u16, middle), (2467 + i as u16, modes[2])] {
                set_visible(gpu, dlg, l, on);
            }
        }
    }

    /// Typed text (`\u{8}` backspace, `\r` return) for the focused edit field.
    pub fn key_text(&mut self, gpu: &mut Gpu, text: &str) -> Option<Action> {
        // `SetEditProperties`: the chat line takes 0x60 characters.
        let max = if gpu.ui.as_ref().and_then(|u| u.state.focus) == Some((net::LOBBY, 2153)) { 0x60 } else { 32 };
        let ev = gpu.ui.as_mut().map(|u| u.state.type_text(text, max)).unwrap_or_default();
        render(gpu);
        if self.random.is_some() {
            for e in &ev {
                if let UiEvent::EditDone { link, .. } = *e {
                    self.random_edit(gpu, link);
                }
            }
            return None;
        }
        self.net_tick(gpu, &ev)
    }

    /// Once per frame: the network screens' lobby and discovery. Returns a game to start.
    pub fn tick(&mut self, gpu: &mut Gpu) -> Option<Action> {
        // The credits' frames (0x511c90: one 0x47d5c0 per shown frame, frames 16 ms apart).
        if let (Some((c, last)), Some(ui)) = (self.credits.as_mut(), gpu.ui.as_mut())
            && last.elapsed() >= std::time::Duration::from_millis(16)
        {
            *last = std::time::Instant::now();
            let texts = &self.texts;
            let strings = |id: u32| texts.as_ref().and_then(|t| t.get(id as usize)).unwrap_or("").to_string();
            c.step(&mut ui.state, &strings);
            render(gpu);
        }
        self.net_tick(gpu, &[])
    }

    /// Main menu event 49 → `CStateCredits` (0x511a00 → 0x511cf0): the main menu closed, dialogs
    /// 14..=17 opened. [not ported here] its music (playlist 5, `sub_29AF10(5, ..)`: audio's).
    fn open_credits(&mut self, gpu: &mut Gpu) {
        let Some(ui) = gpu.ui.as_mut() else { return };
        let mut ev = Vec::new();
        for d in ui.state.open.clone() {
            ui.state.close(d, &mut ev);
        }
        let texts = &self.texts;
        let strings = |id: u32| texts.as_ref().and_then(|t| t.get(id as usize)).unwrap_or("").to_string();
        self.credits = Some((credits::Credits::open(&mut ui.state, &strings), std::time::Instant::now()));
        self.screen = credits::BACKGROUND;
        render(gpu);
    }

    /// Event 0x6d (a mouse button, or Esc): the credits closed, the main menu again.
    /// Whether the credits are on (their state's music: playlist 5, `sound::MenuMusic::credits`).
    pub fn credits_shown(&self) -> bool {
        self.credits.is_some()
    }

    fn close_credits(&mut self, gpu: &mut Gpu) {
        if self.credits.take().is_some() {
            self.open(gpu, MAIN);
            render(gpu);
        }
    }

    /// `GuiDlgLoadFillList`: eighteen rows of (name, date), the selected one in font 2.
    fn fill_saves(&self, gpu: &mut Gpu) {
        for i in 0..18 {
            let (a, b) = load_row(i);
            match self.saves.get(self.load_top + i) {
                Some((_, h)) => {
                    // Ours: a save this build cannot load shows why instead of its date
                    // (save.rs policy) and cannot be loaded.
                    let when = match crate::save::compatibility(h, crate::save::current_rules()) {
                        Ok(()) => date(h.saved_at),
                        Err(why) => format!("not loadable: {why}"),
                    };
                    // Ours: the map's name after the save's.
                    let map = std::path::Path::new(&h.map).file_stem().map(|m| m.to_string_lossy().into_owned()).unwrap_or_default();
                    set_text(gpu, LOAD, a, &format!("{}  ({map})", h.name));
                    set_text(gpu, LOAD, b, &when);
                    set_visible(gpu, LOAD, a, true);
                    set_visible(gpu, LOAD, b, true);
                    if let Some(ui) = gpu.ui.as_mut() {
                        ui.state.fonts.insert((LOAD, a), if Some(self.load_top + i) == self.load_sel { 2 } else { 1 });
                        ui.state.fonts.insert((LOAD, b), 12);
                    }
                }
                None => {
                    set_visible(gpu, LOAD, a, false);
                    set_visible(gpu, LOAD, b, false);
                }
            }
        }
        let loadable = self.load_sel.and_then(|i| self.saves.get(i)).is_some_and(|s| crate::save::compatibility(&s.1, crate::save::current_rules()).is_ok());
        set_enabled(gpu, LOAD, 2342, loadable);
        set_enabled(gpu, LOAD, 2340, self.saves.len() > 18);
        set_enabled(gpu, LOAD, 2341, self.saves.len() > 18);
    }

    /// A mouse event at screen point `p`.
    pub fn mouse(&mut self, gpu: &mut Gpu, p: (f64, f64), what: GuiMouse) -> Option<Action> {
        // `CStateCredits` 0x511b00: events 8 / 10 (left / right button up, as the GUI procedure
        // 0x672f90 numbers them) end them.
        if self.credits.is_some() {
            if matches!(what, GuiMouse::LeftUp | GuiMouse::RightUp) {
                self.close_credits(gpu);
            }
            return None;
        }
        // `CStateSlideshow` 0x527820: event 7 (left button down) shows the next slide, and
        // without one the game ends (0x12947a2 with no state queued: `GameTick` 0x503390 drops
        // the state). Nothing else reaches the screen, which keeps its last frame.
        if self.slideshow {
            return matches!(what, GuiMouse::LeftDown).then_some(Action::Quit);
        }
        // `CStateVictoryScreen` 0x528180: events 8 / 10 (a button's release) end it.
        if self.screen == victory::DIALOG {
            if matches!(what, GuiMouse::LeftUp | GuiMouse::RightUp) {
                self.open(gpu, MAIN);
            }
            return None;
        }
        let (events, _) = client::gui_mouse(gpu, p, what);
        let mut out = None;
        for e in events {
            if let UiEvent::Command { dialog, link, effects } = e
                && let Some(a) = self.command(gpu, dialog, link, effects)
            {
                out = Some(a);
            }
            // The tutorial menu's codes 5 / 6 (0x494330): a mission's description in 0xb03.
            if let UiEvent::Highlight { dialog: TUTORIAL, link, on } = e
                && let Some(&(_, _, desc)) = TUTORIAL_BUTTONS.iter().find(|b| b.0 == link)
            {
                let t = if on { self.text(desc) } else { String::new() };
                set_text(gpu, TUTORIAL, 0xb03, &t);
                render(gpu);
            }
        }
        out
    }

    /// `CStateBriefing`'s speech (0x510660): `Snd\Campaign<"" if German, else EN>\<name>%02i.mp3`.
    fn play_speech(&mut self, campaign: u8, level: u32) {
        let name = match campaign {
            1 => "roman",
            2 => "viking",
            3 => "maya",
            _ => "dark",
        };
        let dir = if self.language == 1 { "Campaign" } else { "CampaignEN" };
        let path = s4_formats::game_dir().join("Snd").join(dir).join(format!("{name}{:02}.mp3", level + 1));
        self.play_file(Some(path));
    }

    /// `CSoundManager::PlaySoundFile(path, 100, 64)` for a briefing.
    fn play_file(&mut self, path: Option<PathBuf>) {
        self.speech = None;
        let Some(path) = path else { return };
        let path = s4_formats::path::resolve(path);
        let played = (|| -> anyhow::Result<Speech> {
            let mut m = crate::sound::new_manager()?;
            let data = kira::sound::streaming::StreamingSoundData::from_file(&path)?;
            let h = m.play(data)?;
            Ok((m, h))
        })();
        match played {
            Ok(s) => self.speech = Some(s),
            Err(e) => tracing::warn!("Unable to play briefing file {}: {e}", path.display()),
        }
    }

    /// The mouse wheel: the map list (GUI code 10) or the briefing story (proc 0x47cda0
    /// code 10).
    pub fn wheel(&mut self, gpu: &mut Gpu, up: bool) {
        // The end statistics' detail, code 10 (0x480250): up 0x4801e0, down 0x480210.
        if self.screen == endstat::DETAIL
            && let (Some(d), Some(ui)) = (self.end_detail.as_mut(), gpu.ui.as_mut())
        {
            if endstat::scroll(&mut ui.state, d, !up) {
                render(gpu);
                self.play(gpu, SUB_BUTTON_UP);
            }
            return;
        }
        // The splash's code 10 (0x46bc40): as 0x7d / 0x7e.
        if self.splash.is_some() {
            if let Some(ui) = gpu.ui.as_mut()
                && ui.scroll_text(splash::DIALOG, splash::TEXT, if up { -1 } else { 1 })
            {
                render(gpu);
            }
            return;
        }
        if let Some(pack) = self.pack {
            let (story, b) = (pack.links().story, pack.links().brief);
            if self.screen == b
                && let Some(ui) = gpu.ui.as_mut()
                && ui.scroll_text(b, story, if up { -1 } else { 1 })
            {
                render(gpu);
            }
            return;
        }
        if self.screen == BRIEFING {
            if let Some(ui) = gpu.ui.as_mut()
                && ui.scroll_text(BRIEFING, 2057, if up { -1 } else { 1 })
            {
                render(gpu);
            }
            return;
        }
        if !matches!(self.screen, MAP_SETTINGS | LOCAL_MAP) {
            return;
        }
        if up && self.top > 0 {
            self.top -= 1;
        } else if !up && self.top + 7 < self.maps.len() {
            self.top += 1;
        } else {
            return;
        }
        self.play(gpu, SUB_BUTTON_UP);
        self.fill_list(gpu);
        render(gpu);
    }

    fn command(&mut self, gpu: &mut Gpu, dialog: usize, link: u16, effects: u8) -> Option<Action> {
        if self.splash.is_some() && dialog == splash::DIALOG {
            match link {
                splash::UP | splash::DOWN => {
                    if let Some(ui) = gpu.ui.as_mut()
                        && ui.scroll_text(splash::DIALOG, splash::TEXT, if link == splash::UP { -1 } else { 1 })
                    {
                        render(gpu);
                    }
                }
                splash::OK => self.close_splash(gpu),
                _ => {}
            }
            return None;
        }
        if self.random.is_some() {
            return self.random_command(gpu, dialog, link, effects);
        }
        if self.pack.is_some() {
            return self.pack_command(gpu, dialog, link, effects);
        }
        if matches!(dialog, net::MP_TYPE | net::GAME_LIST | net::LOBBY | LOCAL_GAME)
            || (dialog == MAP_SETTINGS && self.multiplayer && matches!(link, 2415 | 2433))
            || (dialog == LOCAL_MAP && link == 2433)
        {
            return self.net_command(gpu, dialog, link);
        }
        // Both map settings dialogs share their links (proc 0x48e5e0).
        let dialog = if dialog == LOCAL_MAP { MAP_SETTINGS } else { dialog };
        if dialog == endstat::DIALOG && self.end.is_some() {
            // 0xa71: the map's name (0xa65) to the clipboard (vtable +4; its `RandomMap` +0x20
            // result is unused).
            if link == endstat::SAVE_MAP {
                let name = gpu.ui.as_ref().and_then(|u| u.state.texts.get(&(endstat::DIALOG, 0xa65)).cloned()).unwrap_or_default();
                crate::clipboard::set_text(&name);
                return None;
            }
            match endstat::command(link) {
                Some(endstat::Step::Done) => self.end_done(gpu),
                Some(endstat::Step::Mode(m)) => {
                    if let (Some((rec, _, mode1)), Some(ui)) = (self.end.as_mut(), gpu.ui.as_mut()) {
                        *mode1 = m;
                        endstat::table(&mut ui.state, rec, m);
                    }
                }
                Some(endstat::Step::Player(p)) => {
                    self.open(gpu, endstat::DETAIL);
                    if let (Some((rec, _, _)), Some(ui)) = (self.end.as_ref(), gpu.ui.as_mut()) {
                        let texts = &self.texts;
                        let strings = |id: u32| texts.as_ref().and_then(|t| t.get(id as usize)).unwrap_or("").to_string();
                        self.end_detail = Some(endstat::detail(&mut ui.state, rec, p, &strings));
                    }
                }
                None => {}
            }
            return None;
        }
        // The goods race end screen's procedure 0x493c80: OK → 0x77, the mode radios → the fill.
        if dialog == race_end::DIALOG && self.race_goods.is_some() {
            match link {
                race_end::OK => self.end_done(gpu),
                // 0xaeb: the map's name through `RandomMap` vtable +0x20 (the key of `RD_<key>`)
                // to the clipboard (vtable +0).
                race_end::SAVE_MAP => {
                    let name = gpu.ui.as_ref().and_then(|u| u.state.texts.get(&(race_end::DIALOG, 0xace)).cloned()).unwrap_or_default();
                    crate::clipboard::set_text(name.strip_prefix("RD_").unwrap_or(&name));
                }
                race_end::MODE0 | race_end::MODE1 => {
                    if let Some((_, _, mode1)) = self.end.as_mut() {
                        *mode1 = link == race_end::MODE1;
                    }
                    self.race_fill(gpu);
                }
                _ => {}
            }
            return None;
        }
        // The detail's procedure 0x480250: back (event 0x75), the scroll buttons.
        if dialog == endstat::DETAIL && self.end.is_some() {
            match link {
                endstat::BACK => self.end_back(gpu),
                endstat::DOWN | endstat::UP => {
                    if let (Some(d), Some(ui)) = (self.end_detail.as_mut(), gpu.ui.as_mut())
                        && endstat::scroll(&mut ui.state, d, link == endstat::DOWN)
                    {
                        self.play(gpu, SUB_BUTTON_UP);
                    }
                }
                _ => {}
            }
            return None;
        }
        match (dialog, link) {
            (MAIN, _) => match MAIN_BUTTONS.iter().find(|b| b.0 == link).map(|b| b.1) {
                Some(43) => self.open(gpu, LOCAL_TYPE),
                // Multiplayer: our LAN / internet screens (the HE's Ubisoft check is not reproduced).
                Some(44) => self.open(gpu, net::MP_TYPE),
                // Event 0x2e: `CStateLoadType` (0x51af20), the kind of save first.
                Some(46) => self.open(gpu, load::DIALOG),
                Some(7001) => self.enter_pack(gpu, addon::Pack::Md),
                Some(8001) => self.enter_pack(gpu, addon::Pack::Ao),
                Some(9001) => self.enter_pack(gpu, addon::Pack::Mcd2),
                Some(9501) => self.enter_pack(gpu, addon::Pack::Xmd3),
                // Event 0x2d: `CStateSlideshow` (0x527630 type 1: `downsell%d.bmp`, 0..=9, from
                // the game folder, 0x50c020). The History Edition ships none, so its update
                // 0x527900 draws nothing and the next left press or Space ends the game.
                // [not ported] Drawing such bitmaps: with them the press also ends it here.
                Some(45) => self.slideshow = true,
                // Event 0x2f: `CStateTutorial`, its menu (dialog 0xb).
                Some(47) => self.open(gpu, TUTORIAL),
                // Event 49: `CStateCredits` (0x5118a0).
                Some(49) => self.open_credits(gpu),
                // Events 0x32 / 0x33 (0x522b70): `ShowHtml` 0x5053a0 of the manual / the tips; a
                // failure shows its message in dialog 20 (the manual only).
                Some(ev @ (50 | 51)) => {
                    let path = msgbox::manual(&s4_formats::game_dir(), self.language, ev == 51);
                    let shown = path.as_deref().map_or(Err(0x958), msgbox::show_html);
                    if let (Err(id), 50) = (shown, ev) {
                        tracing::warn!("ShowHtml {path:?}: {}", self.text(id as usize));
                        self.message(gpu, self.text(id as usize), MAIN);
                    }
                }
                Some(ev) => tracing::info!("main menu event {ev}: not ported"),
                None => {}
            },
            (LOCAL_TYPE, 2404) => {
                self.multiplayer = false;
                self.category = 0; // event 0x62 (0x522270) sets 0x149c1dc = 0
                self.open(gpu, LOCAL_MAP);
            }
            // The difficulty radios (0x12987ec = 0 / 1): the computers' level.
            (LOCAL_TYPE, 2413) => self.difficulty = 0,
            (LOCAL_TYPE, 2414) => self.difficulty = 1,
            (LOCAL_TYPE, 2403) => self.open(gpu, MAIN),
            // 0x494330: back (0xaed → event 0x69: the main screen again); a mission button →
            // event 0x68 (i) → 0x527c10: `Map\Tutorial\Tutorial%02i.map` (i + 1) as a single player
            // game of type 4; a missing map closes 0xb and opens the message box (string 0x966).
            (TUTORIAL, 0xaed) => self.open(gpu, MAIN),
            (TUTORIAL, 0xaee..=0xaf9) => {
                let n = (link - 0xaee) as u32 + 1;
                let path = s4_viewer::scene::resolve_map(&crate::tutorial::map_name(n));
                if !path.exists() {
                    tracing::warn!("Tutorial Map '{}' not found!", path.display());
                    self.message(gpu, self.text(0x966), TUTORIAL);
                    return None;
                }
                return Some(Action::Start(path, None, None));
            }
            // Event 0x72: the opener's screen again.
            (msgbox::DIALOG, msgbox::OK) => {
                let back = self.msg_back;
                self.open(gpu, back);
            }
            (load::DIALOG, load::BACK) => self.open(gpu, MAIN),
            (load::DIALOG, l) if load::kind_of_button(l).is_some() => {
                self.load_kind = load::kind_of_button(l).unwrap_or(1);
                self.open(gpu, LOAD);
            }
            // 0x923 → event 0x5e: the load type again (0x51a4e3).
            (LOAD, 2339) => self.open(gpu, load::DIALOG),
            (LOAD, 2340) if self.load_top > 0 => {
                self.load_top -= 1;
                self.play(gpu, SUB_BUTTON_UP);
                self.fill_saves(gpu);
                render(gpu);
            }
            (LOAD, 2341) if self.load_top + 18 < self.saves.len() => {
                self.load_top += 1;
                self.play(gpu, SUB_BUTTON_UP);
                self.fill_saves(gpu);
                render(gpu);
            }
            (LOAD, 2342) => {
                let s = self.load_sel.and_then(|i| self.saves.get(i)).filter(|s| crate::save::compatibility(&s.1, crate::save::current_rules()).is_ok())?.clone();
                // A network save (`<name> - player n`, sim.rs `net_save`): hosted again as a
                // loaded network game, the host as the player it was saved by (the others join
                // it with their own saves). [inf] The release asks for the game type first
                // (dialog 5, 0x48cee0); [ours] dialog 1 then picks LAN or Internet (net.rs).
                if s.1.name.rsplit_once(" - player ").is_some_and(|(_, n)| n.parse::<u8>().is_ok()) {
                    self.pending_save = Some(s.0);
                    self.open(gpu, net::MP_TYPE);
                    return None;
                }
                return Some(Action::Load(s.0));
            }
            (LOAD, 2343..=2378) => {
                let i = (0..18).find(|&i| { let (a, b) = load_row(i); a == link || b == link });
                if let Some(i) = i
                    && self.load_top + i < self.saves.len()
                {
                    self.load_sel = Some(self.load_top + i);
                    self.fill_saves(gpu);
                    render(gpu);
                }
            }
            (LOCAL_TYPE, 2400) => self.open(gpu, CAMPAIGN_DARK),
            (LOCAL_TYPE, 2401) => self.open(gpu, CAMPAIGN_3X3),
            (CAMPAIGN_3X3, 2037) | (CAMPAIGN_DARK, 2080) => self.open(gpu, LOCAL_TYPE),
            (CAMPAIGN_3X3, 2038..=2046) => {
                // CStateCampaign3X3::OnEvent: race 0 / 1 / 2 → campaign type 1 / 2 / 3.
                let (campaign, level) = match link {
                    2044..=2046 => (1, link - 2044),
                    2038..=2040 => (2, link - 2038),
                    _ => (3, link - 2041),
                };
                self.mission = Some((campaign, level as u32));
                self.open(gpu, BRIEFING);
            }
            (CAMPAIGN_DARK, 2081..=2092) => {
                // 0x511741: levels 0 / 2 / 4 / 9 have a video (Intro, Cut1..3) before the briefing.
                let level = (link - 2081) as u32;
                self.mission = Some((4, level));
                match dark_video(level) {
                    Some(n) => self.video = Some((n, AfterVideo::Briefing)),
                    None => self.open(gpu, BRIEFING),
                }
            }
            (BRIEFING, 2055) => {
                self.speech = None;
                let back = if self.mission.is_some_and(|m| m.0 == 4) { CAMPAIGN_DARK } else { CAMPAIGN_3X3 };
                self.open(gpu, back);
            }
            (BRIEFING, 2056) => {
                self.speech = None;
                let (campaign, level) = self.mission?;
                let path = crate::progress::map_path(campaign, level);
                // `CStateBriefing` 0x510970 event 0x71: a missing map closes 10 and opens the
                // message box (string 0x95a).
                if !path.exists() {
                    tracing::warn!("Campaign map '{}' not found!", path.display());
                    self.message(gpu, self.text(0x95a), BRIEFING);
                    return None;
                }
                // 0x510c5d: `sub_1069B0(1, 0, 0, 1, 0, difficulty 0x12987ec, -1, …)`, the map's
                // free and computer slots computers at the single player screen's difficulty (2
                // easy, 3 normal; the Dark Tribe missions the same).
                let players = setup::campaign_setup(&path, self.difficulty);
                return Some(Action::Start(path, Some((campaign, level)), players));
            }
            (BRIEFING, 2070) => self.wheel(gpu, true),
            (BRIEFING, 2071) => self.wheel(gpu, false),
            (BRIEFING, 2073) => {
                // `GuiDlgMainBriefingProc` (0x47cda0) 2073: pressed, the tips text in 2058 and the
                // campaign picture 2074 shown; released, both cleared.
                let pressed = effects & 1 != 0;
                let tips = match self.mission {
                    Some((c, l)) if pressed => self.text(briefing_strings(c)[2] + l as usize),
                    _ => String::new(),
                };
                set_text(gpu, BRIEFING, 2058, &tips);
                set_visible(gpu, BRIEFING, 2074, pressed);
                render(gpu);
            }
            (MAP_SETTINGS, 2415) => self.open(gpu, LOCAL_TYPE),
            (MAP_SETTINGS, 2417..=2423) => {
                let row = self.top + (link - 2417) as usize;
                if row < self.maps.len() {
                    self.selected = Some(row);
                    self.apply_map(gpu);
                    render(gpu);
                }
            }
            (MAP_SETTINGS, 2431) => self.wheel(gpu, true),
            (MAP_SETTINGS, 2432) => self.wheel(gpu, false),
            // The random tab (proc 0x48e5e0 → event 0x1b69 → 0x520c90).
            (MAP_SETTINGS, 2507) => self.open_random(gpu),
            (MAP_SETTINGS, 2449..=2451) => {
                let c = (link - 2449) as u32;
                if c != self.category {
                    self.category = c;
                    self.top = 0;
                    self.refresh_list(gpu);
                    render(gpu);
                }
            }
            // The choices (events 0x44 / 0x43 / 0x45 / 0x46 / 0x42 of 0x520c90): + is the
            // higher link of each pair except the mode, goods and setup pairs (− first).
            (MAP_SETTINGS, 2441 | 2442 | 2445 | 2446 | 2487..=2492) if self.map_read.is_some() => {
                let m = self.map_read.clone()?;
                let c = &mut self.choice;
                // A disabled button (0x48d600) sends nothing.
                let [players, teams, goods, setups] = c.enabled(&m);
                let on = match link {
                    2441 | 2442 => players,
                    2445 | 2446 => teams,
                    2489 | 2490 => goods,
                    2491 | 2492 => setups,
                    _ => true,
                };
                if !on {
                    return None;
                }
                match link {
                    2441 => c.players(true, m.slots.len() as u32),
                    2442 => c.players(false, m.slots.len() as u32),
                    2445 => c.teams(true),
                    2446 => c.teams(false),
                    2487 => c.mode(false, &m),
                    2488 => c.mode(true, &m),
                    2489 => c.goods(false),
                    2490 => c.goods(true),
                    _ => {
                        c.setup(link == 2492, m.setups);
                        let setup = c.setup;
                        self.read_map(setup);
                    }
                }
                self.show_choice(gpu);
                render(gpu);
            }
            _ => {}
        }
        None
    }
}

/// `CStateLobbyGameSettings::DrawMap`: the map preview sampled to 160×160 (rows and columns
/// 3..157) and sheared into the 240-wide picture (`x = column + (160 − row) / 2`), then the player
/// start positions as accessory icons 52 + k (`GetAccessoryGfxInfo`, the digits).
fn preview(path: &std::path::Path) -> anyhow::Result<(u32, u32, Vec<u8>)> {
    let m = s4_formats::map::MapFile::open(path)?;
    let (side, px) = m.preview_rgba()?;
    let starts: Vec<(u32, u32)> = m.players().unwrap_or_default().iter().map(|p| (p.start_x, p.start_y)).collect();
    draw_map(side, &px, m.general()?.size, &starts)
}

/// `RandomMap` vtable +0x24 on the map's name (the file stem here: a generated map's file is
/// `RD_<key>.map`).
fn random_map(path: &str) -> bool {
    let name = path.rsplit(['\\', '/']).next().unwrap_or(path);
    s4_mapgen::key::is_random_name(name.split('.').next().unwrap_or(name))
}

/// [`preview`]'s drawing of an RGBA preview of `side` × `side` for a map of `size` with the
/// players' starts (also the random map's 0x527090).
fn draw_map(side: u32, px: &[u8], size: u32, starts: &[(u32, u32)]) -> anyhow::Result<(u32, u32, Vec<u8>)> {
    let (w, h) = (240u32, 160u32);
    let mut img = vec![0u8; (w * h * 4) as usize];
    let k = side as f32 / 160.0;
    for i in 3..157u32 {
        for j in 3..157u32 {
            let s = (((i as f32 * k) as u32 * side + (j as f32 * k) as u32) * 4) as usize;
            let (x, y) = (j + (160 - i) / 2, i);
            let d = ((y * w + x) * 4) as usize;
            img[d..d + 4].copy_from_slice(&[px[s], px[s + 1], px[s + 2], 255]);
        }
    }
    let size = size.max(1) as f32;
    let icons = s4_formats::gfx::GfxFile::open(s4_formats::game_dir().join("Gfx"), 7)?;
    for (k, &(start_x, start_y)) in starts.iter().enumerate().take(8) {
        let scale = 160.0 / size;
        let row = (start_y as f32 * scale) as i32;
        let col = (start_x as f32 * scale) as i32 + (160 - row) / 2;
        let Ok(icon) = icons.decode(52 + k) else { continue };
        for y in 0..icon.height as i32 {
            for x in 0..icon.width as i32 {
                let s = ((y as u32 * icon.width + x as u32) * 4) as usize;
                let (tx, ty) = (col + x, row + y);
                if icon.rgba[s + 3] == 0 || tx < 0 || ty < 0 || tx >= w as i32 || ty >= h as i32 {
                    continue;
                }
                let d = ((ty as u32 * w + tx as u32) * 4) as usize;
                img[d..d + 4].copy_from_slice(&icon.rgba[s..s + 4]);
            }
        }
    }
    Ok((w, h, img))
}

fn set_text(gpu: &mut Gpu, dialog: usize, link: u16, t: &str) {
    if let Some(ui) = gpu.ui.as_mut() {
        ui.state.texts.insert((dialog, link), t.to_string());
    }
}

/// `IGuiEngine::SetControlVisibility`: effects bit 8.
fn set_visible(gpu: &mut Gpu, dialog: usize, link: u16, visible: bool) {
    set_effect(gpu, dialog, link, 8, !visible);
}

/// `IGuiEngine::EnableControl`: effects bit 4 (greyed).
fn set_enabled(gpu: &mut Gpu, dialog: usize, link: u16, enabled: bool) {
    set_effect(gpu, dialog, link, 4, !enabled);
}

fn set_effect(gpu: &mut Gpu, dialog: usize, link: u16, bit: u8, on: bool) {
    let Some(ui) = gpu.ui.as_mut() else { return };
    let Some(i) = ui.state.set.dialog(dialog).and_then(|d| d.controls.iter().position(|c| c.link == link)) else { return };
    if let Some(e) = ui.state.effects.get_mut(&(dialog, i)) {
        if on {
            *e |= bit;
        } else {
            *e &= !bit;
        }
    }
}

/// Recompose the panel and hand it to the overlay.
pub fn render(gpu: &mut Gpu) {
    if gpu.gui.is_some() {
        // Composed once by the next frame or mouse event (`Gpu::flush_gui`).
        gpu.gui_dirty = true;
        return;
    }
    let format = gpu.format();
    let Some(ui) = gpu.ui.as_mut() else { return };
    match ui.render() {
        Ok(panel) => match &mut gpu.gui {
            Some(g) => g.set_panel(&gpu.device, &gpu.queue, panel),
            None => gpu.gui = Some(s4_viewer::gui::GuiOverlay::new(&gpu.device, &gpu.queue, format, panel)),
        },
        Err(e) => tracing::warn!("GUI: {e}"),
    }
}

/// A save's time as `YYYY-MM-DD HH:MM` (UTC). [inf] The original's date format is not known.
pub(crate) fn date(secs: u64) -> String {
    let days = (secs / 86400) as i64;
    let (h, m) = ((secs % 86400) / 3600, (secs % 3600) / 60);
    // Civil from days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{m:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A free game against computers (lead): single player → normal difficulty → free game,
    /// a multiplayer map with team setups (AncientGlory), its fourth setup ("1 vs 1 vs 1 vs 1",
    /// four teams), three players; the game settings show the human and two computers at
    /// normal; slot 2's type button makes it easy, its row is selected and gets the Vikings; the
    /// start gives control 2 / 3 and the teams, and the world has them.
    #[test]
    fn random_map_parameters_to_a_game() {
        if !s4_formats::game_dir().join("Menu").join(random::SET).exists() {
            return;
        }
        let mut gpu = Gpu::headless(800, 600).unwrap();
        let mut f = Frontend::new(&mut gpu, 0).unwrap();
        f.command(&mut gpu, MAIN, 2536, 0);
        f.command(&mut gpu, LOCAL_TYPE, 2414, 0);
        f.command(&mut gpu, LOCAL_TYPE, 2404, 0);
        f.command(&mut gpu, LOCAL_MAP, 2507, 0);
        assert_eq!(f.screen, random::LOCAL);
        let d = random::LOCAL;
        let text = |gpu: &Gpu, l: u16| gpu.ui.as_ref().unwrap().state.texts.get(&(d, l)).cloned().unwrap_or_default();
        assert_eq!(text(&gpu, random::SIZE), "512x512");
        // Size 256, 3 players, land 30, then the preview and the key.
        for _ in 0..4 {
            f.command(&mut gpu, d, 0x7ad, 0);
        }
        f.command(&mut gpu, d, 0x7c9, 0);
        for _ in 0..4 {
            f.command(&mut gpu, d, 0x7b9, 0);
        }
        assert_eq!((text(&gpu, random::SIZE), text(&gpu, random::PLAYERS)), ("256x256".to_string(), "3".to_string()));
        let key = text(&gpu, random::KEY);
        let p = s4_mapgen::key::entered(&key).expect("a valid key");
        assert_eq!((p.size, p.players, p.land), (256, 3, 30));
        f.command(&mut gpu, d, random::PREVIEW_TOGGLE, 1);
        assert!(gpu.ui.as_ref().unwrap().state.owner_images.contains_key(&(d, random::PREVIEW)));
        // A typed key: refused (start disabled), then a valid one.
        let greyed = |gpu: &Gpu| {
            let st = &gpu.ui.as_ref().unwrap().state;
            st.index_of(d, random::START).is_some_and(|k| st.effects.get(&(d, k)).is_some_and(|e| e & 4 != 0))
        };
        gpu.ui.as_mut().unwrap().state.texts.insert((d, random::KEY), "ZZZZZZZZ".into());
        f.random_edit(&mut gpu, random::KEY);
        assert!(greyed(&gpu));
        gpu.ui.as_mut().unwrap().state.texts.insert((d, random::KEY), key.clone());
        f.random_edit(&mut gpu, random::KEY);
        assert!(!greyed(&gpu));
        // Start: the game settings with the random slots, then the game on the generated map.
        let r_goods = f.random.as_ref().unwrap().goods as u8;
        f.command(&mut gpu, d, random::START, 0);
        assert_eq!(f.screen, LOCAL_GAME);
        let Some(Action::Start(path, None, Some(players))) = f.command(&mut gpu, LOCAL_GAME, 2142, 0) else { panic!("no start") };
        assert_eq!(players.start_resources, r_goods, "the random dialog's goods as start resources");
        assert!(path.ends_with(format!("RD_{key}.map")));
        assert_eq!(players.players, Some(3));
        let map = s4_formats::map::MapFile::open(&path).unwrap();
        let ng = s4_sim::NewGame { races: players.races, teams: players.teams, control: players.control, game_type: players.game_type, players: players.players, ..Default::default() };
        let w = s4_viewer::population::world_from_map_with(&map, &ng).unwrap();
        assert_eq!(w.players.iter().flatten().count(), 3);
        // 0x5069b0 clears the four texts for a random map: no goal.
        assert_eq!(crate::briefing::goal(None, &path, 1, &|_| Some("x".into())), "");
        assert!(random_map(&path.to_string_lossy()));
        // Back from a new random screen: the single player type screen.
        f.open(&mut gpu, LOCAL_MAP);
        f.command(&mut gpu, LOCAL_MAP, 2507, 0);
        assert!(f.escape(&mut gpu));
        assert_eq!((f.screen, f.random.is_none()), (LOCAL_TYPE, true));
    }

    /// Free settle on a random map (mode 5: one player, one team): the start is decided for team
    /// 1 (GameState +0x2dc / +0x2e0) and the first `Statistic::Update` freezes the start snapshot.
    #[test]
    fn free_settle_starts_decided() {
        if !s4_formats::game_dir().join("Menu").join(random::SET).exists() {
            return;
        }
        let mut gpu = Gpu::headless(800, 600).unwrap();
        let mut f = Frontend::new(&mut gpu, 0).unwrap();
        f.command(&mut gpu, MAIN, 2536, 0);
        f.command(&mut gpu, LOCAL_TYPE, 2404, 0);
        f.command(&mut gpu, LOCAL_MAP, 2507, 0);
        let d = random::LOCAL;
        for _ in 0..4 {
            f.command(&mut gpu, d, 0x7ad, 0);
        }
        f.command(&mut gpu, d, 0x7c5, 0);
        f.command(&mut gpu, d, 0x7c5, 0);
        assert_eq!(f.random.as_ref().map(|r| (r.mode, r.players, r.teams)), Some((5, 1, 1)));
        f.command(&mut gpu, d, random::START, 0);
        let Some(Action::Start(path, None, Some(players))) = f.command(&mut gpu, LOCAL_GAME, 2142, 0) else { panic!("no start") };
        assert!(players.decided && players.game_type == 5);
        let map = s4_formats::map::MapFile::open(&path).unwrap();
        let ng = s4_sim::NewGame { races: players.races, teams: players.teams, control: players.control, game_type: players.game_type, players: players.players, start_winner: players.decided as i32, ..Default::default() };
        let mut w = s4_viewer::population::world_from_map_with(&map, &ng).unwrap();
        assert!(!w.statistic.start_frozen);
        s4_sim::tick(&mut w, &[]);
        assert!(w.statistic.start_frozen);
        // Conflict (mode 1) is not.
        assert!(!setup::player_setup(&[], 1, 1).decided);
    }

    /// The main campaigns' briefing start (0x510c5d): the map's computer slots are computers
    /// (control 2 at easy), and the game makes their AI.
    #[test]
    fn campaign_missions_start_with_computers() {
        let path = crate::progress::map_path(1, 0);
        if !path.exists() {
            return;
        }
        let mut gpu = Gpu::headless(800, 600).unwrap();
        let mut f = Frontend::new(&mut gpu, 0).unwrap();
        f.difficulty = 0;
        f.mission = Some((1, 0));
        f.screen = BRIEFING;
        let Some(Action::Start(p, Some((1, 0)), Some(players))) = f.command(&mut gpu, BRIEFING, 2056, 0) else { panic!("no start") };
        assert_eq!(p, path);
        assert_eq!((players.local, players.control[0], players.control[1]), (1, 1, 2));
        let map = s4_formats::map::MapFile::open(&path).unwrap();
        let ng = s4_sim::NewGame { races: players.races, teams: players.teams, control: players.control, game_type: players.game_type, players: players.players, ..Default::default() };
        let w = s4_viewer::population::world_from_map_with(&map, &ng).unwrap();
        assert!(w.ai.players[2].is_some(), "player 2's AI");
        assert!(w.ai.players[1].is_none());
        // Normal: control 3. The Dark Tribe's first mission too.
        f.difficulty = 1;
        let Some(Action::Start(_, _, Some(players))) = f.command(&mut gpu, BRIEFING, 2056, 0) else { panic!("no start") };
        assert_eq!(players.start_resources, 0, "campaign maps place their own");
        assert_eq!(players.control[1], 3);
        if crate::progress::map_path(4, 0).exists() {
            f.mission = Some((4, 0));
            let Some(Action::Start(_, Some((4, 0)), Some(players))) = f.command(&mut gpu, BRIEFING, 2056, 0) else { panic!("no dark start") };
            assert!(players.control.iter().skip(1).any(|&c| c == 3), "{:?}", players.control);
        }
    }

    #[test]
    fn skirmish_with_computers_and_teams() {
        let map = s4_formats::game_dir().join("Map").join("Multiplayer").join("Choices.map");
        if !map.exists() {
            return;
        }
        // Choices (options 0x13): the player buttons are enabled (AncientGlory's 0x11 has them
        // disabled, 0x48d600).
        let mut gpu = Gpu::headless(800, 600).unwrap();
        let mut f = Frontend::new(&mut gpu, 0).unwrap();
        f.command(&mut gpu, MAIN, 2536, 0);
        f.command(&mut gpu, LOCAL_TYPE, 2414, 0);
        f.command(&mut gpu, LOCAL_TYPE, 2404, 0);
        assert_eq!(f.screen, LOCAL_MAP);
        f.command(&mut gpu, LOCAL_MAP, 2450, 0);
        let i = f.maps.iter().position(|m| m.name == "Choices").expect("listed locally");
        f.selected = Some(i);
        f.apply_map(&mut gpu);
        for _ in 0..3 {
            f.command(&mut gpu, LOCAL_MAP, 2492, 0);
        }
        let text = |gpu: &Gpu, l: u16| gpu.ui.as_ref().unwrap().state.texts.get(&(LOCAL_MAP, l)).cloned().unwrap_or_default();
        assert_eq!((text(&gpu, 2486), text(&gpu, 2443), text(&gpu, 2447)), ("1 vs 1 vs 1 vs 1".to_string(), "4".to_string(), "4".to_string()));
        f.command(&mut gpu, LOCAL_MAP, 2442, 0);
        assert_eq!((f.choice.players, f.choice.teams), (3, 3), "the teams follow the players down");
        assert_eq!(text(&gpu, 2484), "Conflict Mode");
        f.command(&mut gpu, LOCAL_MAP, 2433, 0);
        assert_eq!(f.screen, LOCAL_GAME);
        let kinds = |f: &Frontend| match &f.net {
            net::NetUi::Local(l) => l.slots.iter().map(|s| (s.kind, s.level, s.team)).collect::<Vec<_>>(),
            _ => panic!("no local lobby"),
        };
        use s4_net::lobby::{SLOT_AI, SLOT_HUMAN};
        assert_eq!(kinds(&f), [(SLOT_HUMAN, 0, 0), (SLOT_AI, 1, 1), (SLOT_AI, 1, 2)]);
        f.command(&mut gpu, LOCAL_GAME, 2224, 0); // slot 3's type: normal → easy
        f.command(&mut gpu, LOCAL_GAME, 2266, 0); // select row 2
        let race = |f: &Frontend| match &f.net {
            net::NetUi::Local(l) => l.slots[1].race,
            _ => 0,
        };
        // Race + twice from the map's race (the cycle 0, 1, 2, 4, 5 random).
        let cycle = [0u8, 1, 2, 4, 5];
        let k = cycle.iter().position(|&r| r == race(&f)).unwrap();
        f.command(&mut gpu, LOCAL_GAME, 2182, 0);
        f.command(&mut gpu, LOCAL_GAME, 2182, 0);
        let chosen = cycle[(k + 2) % 5];
        assert_eq!(race(&f), chosen, "row 2's race changed, not the own");
        assert_eq!(kinds(&f)[2], (SLOT_AI, 0, 2));
        let images = |gpu: &Gpu, l: u16| gpu.ui.as_ref().unwrap().state.images.get(&(LOCAL_GAME, l)).copied();
        assert_eq!((images(&gpu, 2223), images(&gpu, 2224)), (Some((97, 98)), Some((93, 94))), "normal / easy slot type images");
        // Logos: not for a computer's row; the own logo + twice (0 → 1 → 2): each time the row
        // that had the new logo gives way to the first free index (0x51cec2), and only the own
        // `Logo\Current` slot changes (0x51d234), so row 2 (logo 0) shows the own slot's.
        let logos = |f: &Frontend| match &f.net {
            net::NetUi::Local(l) => l.slots.iter().map(|s| s.logo).collect::<Vec<_>>(),
            _ => panic!("no local lobby"),
        };
        assert_eq!(logos(&f), [0, 1, 2]);
        f.command(&mut gpu, LOCAL_GAME, 2186, 0);
        assert_eq!(logos(&f), [0, 1, 2], "row 2 is a computer's");
        f.command(&mut gpu, LOCAL_GAME, 2265, 0);
        f.command(&mut gpu, LOCAL_GAME, 2186, 0);
        f.command(&mut gpu, LOCAL_GAME, 2186, 0);
        assert_eq!(logos(&f), [2, 0, 1]);
        let list = s4_formats::logo::list(&s4_formats::game_dir());
        let rgb = |i: usize| s4_formats::logo::read_rgb(&list[i]).ok();
        assert_eq!((&f.logos[0], &f.logos[1], &f.logos[2]), (&rgb(2), &rgb(1), &rgb(2)));
        let shown = |gpu: &Gpu, l: u16| gpu.ui.as_ref().unwrap().state.owner_images.get(&(LOCAL_GAME, l)).map(|i| i.2.clone());
        let composed = |i: usize| rgb(i).map(|c| s4_formats::logo::compose_rgba(&c));
        assert_eq!((shown(&gpu, 2257), shown(&gpu, 2258), shown(&gpu, 2259), shown(&gpu, 2256)), (composed(2), composed(2), composed(1), composed(2)));
        f.command(&mut gpu, LOCAL_GAME, 2187, 0);
        assert_eq!(logos(&f), [1, 0, 2]);
        assert_eq!(f.logos[0], rgb(1));
        let Some(Action::Start(path, None, Some(p))) = f.command(&mut gpu, LOCAL_GAME, 2142, 0) else { panic!("no start") };
        assert_eq!(p.start_resources, f.choice.goods as u8, "the map settings' goods as start resources");
        assert_eq!(path, map);
        assert_eq!((p.local, &p.control[..4], &p.teams[..4]), (1, &[1, 3, 2, 0][..], &[Some(0), Some(1), Some(2), None][..]));
        if chosen != 5 {
            assert_eq!(p.races[1], Some(chosen));
        }
        let setup = crate::sim::SimSetup { map_path: map.clone(), prefer_file_script: false, run_script: false, fog: false, outcome: Default::default(), tutorial: Default::default(), net: None, players: Some(p), load: None };
        let (w, _) = crate::sim::build_world(&setup, &map, 1).unwrap();
        assert_eq!(&w.setup.control[1..5], &[1, 3, 2, 0]);
        // The game start (0x4ff5f0, `ai::add_players_at_start`): the computers get an AI, normal
        // (control 3) at level 2, easy (control 2) at level 1; the human none.
        assert_eq!((1..=4).map(|p| w.ai.is_ai(p)).collect::<Vec<_>>(), [false, true, true, false]);
        assert_eq!((w.ai.vars.records[2].level, w.ai.vars.records[3].level), (2, 1));
        assert!(w.alliances.hostile(1, 2) && w.alliances.hostile(2, 3), "three teams");
        assert_eq!(w.players[2].as_ref().map(|p| p.race), p.races[1]);
    }

    /// The Mission CD 2 campaigns (main menu 9001): the map screen with only the Viking and
    /// Maya first missions (no wins), Viking 1's briefing with its title and goal, the start of
    /// `MCD2_viking1.map` as campaign type 18 level 0 with computers at the difficulty; back
    /// twice leads to the main menu's set again. Mission CD 3 (9501) shows all sixteen missions.
    #[test]
    fn mission_cd_2_and_3_campaigns() {
        if !s4_formats::game_dir().join("Menu").join("GUISetMD2Startscreens.DAT").exists() {
            return;
        }
        let mut gpu = Gpu::headless(800, 600).unwrap();
        let mut f = Frontend::new(&mut gpu, 0).unwrap();
        f.progress = Default::default();
        f.command(&mut gpu, MAIN, 2532, 0);
        assert_eq!((f.pack, f.screen), (Some(addon::Pack::Mcd2), 0));
        let hidden = |gpu: &Gpu, d: usize, l: u16| {
            let ui = gpu.ui.as_ref().unwrap();
            let i = ui.state.set.dialog(d).unwrap().controls.iter().position(|c| c.link == l).unwrap();
            ui.state.effects.get(&(d, i)).is_some_and(|e| e & 8 != 0)
        };
        if crate::progress::Progress::load().addon.is_empty() {
            assert!(!hidden(&gpu, 0, 1832) && !hidden(&gpu, 0, 1842), "Viking / Maya 1 shown");
            assert!(hidden(&gpu, 0, 1827) && hidden(&gpu, 0, 1833), "Roman 1, Viking 2 not yet");
        }
        f.command(&mut gpu, 0, 1825, 0); // normal
        f.command(&mut gpu, 0, 1832, 0);
        assert_eq!((f.screen, f.mission), (1, Some((18, 0))));
        let text = |gpu: &Gpu, l: u16| gpu.ui.as_ref().unwrap().state.texts.get(&(1, l)).cloned().unwrap_or_default();
        assert_eq!(text(&gpu, 1807), f.text(2904));
        assert_eq!(text(&gpu, 1808), f.text(2857));
        let Some(Action::Start(path, Some((18, 0)), Some(p))) = f.command(&mut gpu, 1, 1804, 0) else { panic!("no start") };
        assert!(path.file_name().unwrap().to_string_lossy().eq_ignore_ascii_case("mcd2_viking1.map"));
        assert!(p.control.contains(&3), "computers at normal: {:?}", p.control);
        f.command(&mut gpu, 1, 1803, 0);
        assert_eq!(f.screen, 0);
        f.command(&mut gpu, 0, 1826, 0);
        assert_eq!((f.pack, f.screen), (None, MAIN));
        f.command(&mut gpu, MAIN, 2534, 0);
        assert_eq!(f.pack, Some(addon::Pack::Xmd3));
        assert!((2939..=2954).all(|l| !hidden(&gpu, 0, l)), "Mission CD 3: every mission");
        f.command(&mut gpu, 0, 2951, 0);
        assert_eq!(f.mission, Some((23, 0)));
        assert_eq!(text(&gpu, 2919), f.text(3781));
    }

    /// The Mission CD (main menu 7001): menu → Roman screen (its first mission enabled, the
    /// second not without a win) → briefing (title 3187) → start `md_roman1.map` as type 5;
    /// back leads to the Roman screen, the menu, the main menu.
    #[test]
    fn mission_cd_campaigns() {
        if !s4_formats::game_dir().join("Menu").join("GUISetMDStartscreens.DAT").exists() {
            return;
        }
        let mut gpu = Gpu::headless(800, 600).unwrap();
        let mut f = Frontend::new(&mut gpu, 0).unwrap();
        f.command(&mut gpu, MAIN, 2531, 0);
        assert_eq!((f.pack, f.screen), (Some(addon::Pack::Md), 0));
        f.command(&mut gpu, 0, 1918, 0);
        assert_eq!(f.screen, 1);
        let effect = |gpu: &Gpu, d: usize, l: u16| {
            let ui = gpu.ui.as_ref().unwrap();
            let i = ui.state.set.dialog(d).unwrap().controls.iter().position(|c| c.link == l).unwrap();
            ui.state.effects.get(&(d, i)).copied().unwrap_or(0)
        };
        assert_eq!(effect(&gpu, 1, 1903) & 4, 0, "mission 1 enabled");
        if crate::progress::Progress::load().wins(5) == 0 {
            assert_ne!(effect(&gpu, 1, 1904) & 4, 0, "mission 2 not yet");
        }
        f.command(&mut gpu, 1, 1903, 0);
        assert_eq!((f.screen, f.mission), (5, Some((5, 0))));
        let title = gpu.ui.as_ref().unwrap().state.texts.get(&(5, 1871)).cloned().unwrap_or_default();
        assert_eq!(title, f.text(3187));
        let Some(Action::Start(path, Some((5, 0)), Some(_))) = f.command(&mut gpu, 5, 1868, 0) else { panic!("no start") };
        assert!(path.file_name().unwrap().to_string_lossy().eq_ignore_ascii_case("md_roman1.map"));
        f.command(&mut gpu, 5, 1867, 0);
        assert_eq!(f.screen, 1);
        f.command(&mut gpu, 1, 1915, 0);
        assert_eq!(f.screen, 0);
        f.command(&mut gpu, 0, 1925, 0);
        assert_eq!((f.pack, f.screen), (None, MAIN));
    }

    /// Lead / user report: an AutoSave from an older build blocked the start ("saved with other
    /// game data"). The start-screen load list shows such a save as "not loadable: …" and its
    /// load button gives nothing; a loadable save next to it still loads. Nothing else on the
    /// start screens reads saves (a mission start is `Action::Start` with the map only).
    #[test]
    fn load_list_refuses_incompatible_saves() {
        if !s4_formats::game_dir().join("Menu").exists() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("s4rs-test-loadlist-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let rules = crate::save::current_rules().expect("rules");
        let header = |version: u32, saved_at: u64, rules: u64| crate::save::Header {
            magic: crate::save::MAGIC.into(),
            version,
            name: format!("v{version}"),
            saved_at,
            map: "Campaign/roman02.map".into(),
            map_hash: 0,
            tick: 1,
            rules,
            thumb: Vec::new(),
            world_hash: 0,
        };
        // Version 2's header: the same fields without the world hash.
        #[derive(serde::Serialize)]
        struct V2(String, u32, String, u64, String, u64, u32, u64, Vec<u8>);
        std::fs::write(dir.join("AutoSave.s4rs"), bincode::serialize(&V2(crate::save::MAGIC.into(), 2, "AutoSave".into(), 100, "Campaign/roman02.map".into(), 0, 1, rules ^ 1, Vec::new())).unwrap()).unwrap();
        std::fs::write(dir.join("good.s4rs"), bincode::serialize(&header(crate::save::VERSION, 200, rules)).unwrap()).unwrap();
        std::fs::write(dir.join("rules.s4rs"), bincode::serialize(&header(crate::save::VERSION, 150, rules ^ 1)).unwrap()).unwrap();
        let mut gpu = Gpu::headless(800, 600).unwrap();
        let mut f = Frontend::new(&mut gpu, 0).unwrap();
        f.open(&mut gpu, LOAD);
        f.saves = crate::menu::list_saves_in(&dir);
        f.fill_saves(&mut gpu);
        let cur = format!("v{}", crate::save::VERSION);
        assert_eq!(f.saves.iter().map(|s| s.1.name.as_str()).collect::<Vec<_>>(), [cur.as_str(), cur.as_str(), "AutoSave"]);
        let text = |gpu: &Gpu, l: u16| gpu.ui.as_ref().unwrap().state.texts.get(&(LOAD, l)).cloned().unwrap_or_default();
        for (row, why) in [(1, Some("other game data")), (2, Some("older s4rs")), (0, None)] {
            let (a, b) = load_row(row);
            f.command(&mut gpu, LOAD, a, 0);
            assert_eq!(f.load_sel, Some(row));
            let act = f.command(&mut gpu, LOAD, 2342, 0);
            match why {
                Some(why) => {
                    assert_eq!(text(&gpu, b), format!("not loadable: {why}"));
                    assert!(act.is_none(), "row {row} must not load");
                }
                None => assert!(matches!(act, Some(Action::Load(p)) if p == dir.join("good.s4rs"))),
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The tutorial menu: the main screen's tutorial button (event 0x2f) opens dialog 0xb with its
    /// title and twelve texts (0x4941a0); the cursor on a mission shows its description in 0xb03
    /// and off it empties it; a mission button starts `Map\Tutorial\TutorialNN.map`; back goes to
    /// the main screen (0x494330).
    /// The credits (main menu 2550 → event 49 → `CStateCredits`): dialogs 14..=17, the first
    /// entry's heading, the panel climbing after the 60-frame pause, Esc back to the menu.
    #[test]
    fn credits_screen() {
        if !s4_formats::game_dir().join("Gfx").exists() {
            return;
        }
        let mut gpu = Gpu::headless(800, 600).unwrap();
        let mut f = Frontend::new(&mut gpu, 0).unwrap();
        f.command(&mut gpu, MAIN, 2550, 0);
        let open = |gpu: &Gpu| gpu.ui.as_ref().unwrap().state.open.clone();
        assert!([14, 15, 16, 17].iter().all(|d| open(&gpu).contains(d)) && !open(&gpu).contains(&MAIN), "{:?}", open(&gpu));
        let heading = gpu.ui.as_ref().unwrap().state.texts.get(&(17, 0x81c)).cloned().unwrap_or_default();
        assert_eq!(heading, f.text(2660));
        let y0 = gpu.ui.as_ref().unwrap().state.set.dialog(17).unwrap().y;
        for _ in 0..66 {
            let texts = &f.texts;
            let strings = |id: u32| texts.as_ref().and_then(|t| t.get(id as usize)).unwrap_or("").to_string();
            let (c, _) = f.credits.as_mut().unwrap();
            c.step(&mut gpu.ui.as_mut().unwrap().state, &strings);
        }
        let y1 = gpu.ui.as_ref().unwrap().state.set.dialog(17).unwrap().y;
        assert!(y1 < y0, "the panel climbs: {y0} → {y1}");
        render(&mut gpu);
        if let Ok(img) = gpu.ui.as_mut().unwrap().compose() {
            let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/gui/credits.png");
            let _ = s4_viewer::client::write_png(&out, img.width, img.height, &img.rgba);
        }
        assert!(f.escape(&mut gpu));
        assert!(open(&gpu).contains(&MAIN) && !open(&gpu).contains(&17));
    }

    /// Load game (main menu 2549 → event 0x2e → `CStateLoadType`): dialog 5 with its title and
    /// the network button's tooltip 0x4ea; a type button opens dialog 6 keeping that kind; the
    /// list's back and Esc return to dialog 5, Esc there to the menu. Captures
    /// `out/gui/load_type.png`.
    #[test]
    fn load_type_screen() {
        if !s4_formats::game_dir().join("Gfx").exists() {
            return;
        }
        let mut gpu = Gpu::headless(800, 600).unwrap();
        let mut f = Frontend::new(&mut gpu, 0).unwrap();
        f.command(&mut gpu, MAIN, 2549, 0);
        assert_eq!(f.screen, load::DIALOG);
        let st = &gpu.ui.as_ref().unwrap().state;
        assert_eq!(st.texts.get(&(load::DIALOG, load::TITLE)).cloned().unwrap_or_default(), f.text(0x4d0));
        assert_eq!(st.tooltips.get(&(load::DIALOG, load::NETWORK)), Some(&(0x4ea, 0)));
        render(&mut gpu);
        if let Ok(img) = gpu.ui.as_mut().unwrap().compose() {
            let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/gui/load_type.png");
            let _ = s4_viewer::client::write_png(&out, img.width, img.height, &img.rgba);
        }
        for (link, kind) in [(load::CAMPAIGN, 3), (load::NETWORK, 2), (load::LOCAL, 1)] {
            f.command(&mut gpu, load::DIALOG, link, 0);
            assert_eq!((f.screen, f.load_kind), (LOAD, kind));
            assert!(f.saves.iter().all(|s| load::kind_of(&s.1) == kind));
            assert_eq!(f.load_sel, (!f.saves.is_empty()).then_some(0));
            f.command(&mut gpu, LOAD, 2339, 0);
            assert_eq!(f.screen, load::DIALOG);
        }
        f.command(&mut gpu, load::DIALOG, load::CAMPAIGN, 0);
        assert!(f.escape(&mut gpu));
        assert_eq!(f.screen, load::DIALOG);
        assert!(f.escape(&mut gpu));
        assert_eq!(f.screen, MAIN);
        f.command(&mut gpu, MAIN, 2549, 0);
        f.command(&mut gpu, load::DIALOG, load::BACK, 0);
        assert_eq!(f.screen, MAIN);
    }

    /// The message box (dialog 20, init 0x48f170) over the tutorial menu as `CStateTutorial`
    /// opens it for a missing map: OK / title / text, OK (event 0x72) → dialog 0xb again; over
    /// the main menu Esc is ignored. Captures `out/gui/message_box.png`.
    #[test]
    fn message_box() {
        if !s4_formats::game_dir().join("Gfx").exists() {
            return;
        }
        let mut gpu = Gpu::headless(800, 600).unwrap();
        let mut f = Frontend::new(&mut gpu, 0).unwrap();
        f.command(&mut gpu, MAIN, 2535, 0);
        f.message(&mut gpu, f.text(0x966), TUTORIAL);
        let st = &gpu.ui.as_ref().unwrap().state;
        assert_eq!(st.open, [msgbox::DIALOG]);
        let text = |l: u16| st.texts.get(&(msgbox::DIALOG, l)).cloned().unwrap_or_default();
        assert_eq!([text(msgbox::OK), text(msgbox::TITLE), text(msgbox::TEXT)], [f.text(0x90d), f.text(0x94e), f.text(0x966)]);
        if let Ok(img) = gpu.ui.as_mut().unwrap().compose() {
            let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/gui/message_box.png");
            let _ = s4_viewer::client::write_png(&out, img.width, img.height, &img.rgba);
        }
        f.command(&mut gpu, msgbox::DIALOG, msgbox::OK, 0);
        assert_eq!(f.screen, TUTORIAL);
        assert_eq!(gpu.ui.as_ref().unwrap().state.texts.get(&(TUTORIAL, 0xaec)).cloned().unwrap_or_default(), f.text(0x835));
        f.message(&mut gpu, f.text(0x966), TUTORIAL);
        assert!(f.escape(&mut gpu));
        assert_eq!(f.screen, MAIN);
        f.message(&mut gpu, f.text(0x958), MAIN);
        assert!(f.escape(&mut gpu));
        assert_eq!(f.screen, msgbox::DIALOG);
        f.command(&mut gpu, msgbox::DIALOG, msgbox::OK, 0);
        assert_eq!(f.screen, MAIN);
    }

    /// Exit (main menu 2551 → event 0x2d → `CStateSlideshow` without slides): nothing ends the
    /// game until the next left press (event 7) or Space; the credits end on a button's release
    /// (events 8 / 10), not its press.
    #[test]
    fn exit_slideshow_and_credits_release() {
        if !s4_formats::game_dir().join("Gfx").exists() {
            return;
        }
        let mut gpu = Gpu::headless(800, 600).unwrap();
        let mut f = Frontend::new(&mut gpu, 0).unwrap();
        assert!(!f.space());
        assert!(f.command(&mut gpu, MAIN, 2551, 0).is_none());
        assert!(f.space());
        assert!(f.mouse(&mut gpu, (400.0, 300.0), GuiMouse::Move).is_none());
        assert!(f.mouse(&mut gpu, (400.0, 300.0), GuiMouse::RightDown).is_none());
        assert!(matches!(f.mouse(&mut gpu, (400.0, 300.0), GuiMouse::LeftDown), Some(Action::Quit)));
        let mut f = Frontend::new(&mut gpu, 0).unwrap();
        f.command(&mut gpu, MAIN, 2550, 0);
        f.mouse(&mut gpu, (400.0, 300.0), GuiMouse::LeftDown);
        assert!(f.credits.is_some());
        f.mouse(&mut gpu, (400.0, 300.0), GuiMouse::RightUp);
        assert!(f.credits.is_none() && f.screen == MAIN);
    }

    /// After the statistics of a won last mission of the Vikings (type 2, level 2): the victory
    /// screen (dialog 12, picture 0x9ff = image 256), a button's release → the main menu; any
    /// key does the same; after a tutorial the tutorial menu (code 3). Captures
    /// `out/gui/victory.png`.
    #[test]
    fn victory_screen() {
        if !s4_formats::game_dir().join("Gfx").exists() {
            return;
        }
        let mut gpu = Gpu::headless(800, 600).unwrap();
        let won = Some(victory::Outcome { campaign: 2, level: 2, won: true });
        let ended = |gpu: &mut Gpu, o: Option<victory::Outcome>| {
            let mut f = Frontend::new(gpu, 0).unwrap();
            f.end = Some((endstat::EndRecord::default(), o, true));
            f.end_done(gpu);
            f
        };
        let mut f = ended(&mut gpu, won);
        assert_eq!(f.screen, victory::DIALOG);
        assert_eq!(gpu.ui.as_ref().unwrap().state.images.get(&(victory::DIALOG, victory::PICTURE)), Some(&(0x100, 0)));
        if let Ok(img) = gpu.ui.as_mut().unwrap().compose() {
            let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/gui/victory.png");
            let _ = s4_viewer::client::write_png(&out, img.width, img.height, &img.rgba);
        }
        assert!(f.mouse(&mut gpu, (400.0, 300.0), GuiMouse::LeftDown).is_none());
        assert_eq!(f.screen, victory::DIALOG);
        f.mouse(&mut gpu, (400.0, 300.0), GuiMouse::LeftUp);
        assert_eq!(f.screen, MAIN);
        let mut f = ended(&mut gpu, won);
        assert!(f.key_down(&mut gpu) && f.screen == MAIN && !f.key_down(&mut gpu));
        let lost = Some(victory::Outcome { campaign: 2, level: 2, won: false });
        assert_eq!(ended(&mut gpu, lost).screen, CAMPAIGN_3X3);
        let f = Frontend::after_tutorial(&mut gpu, 0).unwrap();
        assert_eq!(f.screen, TUTORIAL);
    }

    /// Esc on the start screens: nothing on the main menu, the back button's event elsewhere
    /// (single player → menu, the campaign screens → single player, tutorial → menu).
    #[test]
    fn escape_goes_back() {
        if !s4_formats::game_dir().join("Gfx").exists() {
            return;
        }
        let mut gpu = Gpu::headless(800, 600).unwrap();
        let mut f = Frontend::new(&mut gpu, 0).unwrap();
        assert!(f.escape(&mut gpu) && f.screen == MAIN);
        for (screen, back) in [(LOCAL_TYPE, MAIN), (CAMPAIGN_3X3, LOCAL_TYPE), (CAMPAIGN_DARK, LOCAL_TYPE), (TUTORIAL, MAIN), (load::DIALOG, MAIN), (net::MP_TYPE, MAIN), (net::GAME_LIST, net::MP_TYPE), (LOCAL_MAP, LOCAL_TYPE)] {
            f.open(&mut gpu, screen);
            assert!(f.escape(&mut gpu));
            assert_eq!(f.screen, back, "Esc on {screen}");
        }
    }

    /// A won Trojan mission at level 6 (type 15): after the statistics the Add-On splash (its set's
    /// dialog 8: picture 0x91, story 0x1f9, title hidden), the text scrolls, 0x80 → the
    /// campaign's screen; Esc does the same. Captures `out/gui/ao_splash.png`.
    #[test]
    fn ao_splash() {
        if !s4_formats::game_dir().join("Menu").join("GUISetAOStartscreens.DAT").exists() {
            return;
        }
        let mut gpu = Gpu::headless(800, 600).unwrap();
        let shown = |gpu: &mut Gpu| {
            let mut f = Frontend::new(gpu, 0).unwrap();
            f.end = Some((endstat::EndRecord::default(), Some(victory::Outcome { campaign: 15, level: 6, won: true }), true));
            f.end_done(gpu);
            f
        };
        let mut f = shown(&mut gpu);
        assert_eq!((f.pack, f.screen), (Some(addon::Pack::Ao), splash::DIALOG));
        let st = &gpu.ui.as_ref().unwrap().state;
        assert_eq!(st.images.get(&(splash::DIALOG, splash::PICTURE)), Some(&(0x91, 0)));
        assert_eq!(st.texts.get(&(splash::DIALOG, splash::TEXT)).cloned().unwrap_or_default(), f.text(0x1f9));
        if let Ok(img) = gpu.ui.as_mut().unwrap().compose() {
            let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/gui/ao_splash.png");
            let _ = s4_viewer::client::write_png(&out, img.width, img.height, &img.rgba);
        }
        f.command(&mut gpu, splash::DIALOG, splash::DOWN, 0);
        assert_eq!(gpu.ui.as_ref().unwrap().state.scroll.get(&(splash::DIALOG, splash::TEXT)).copied(), Some(1));
        f.command(&mut gpu, splash::DIALOG, splash::OK, 0);
        let back = addon::Pack::Ao.back_from_briefing(15);
        assert_eq!((f.splash, f.screen), (None, back));
        let mut f = shown(&mut gpu);
        assert!(f.escape(&mut gpu));
        assert_eq!((f.splash, f.screen), (None, back));
    }

    /// The videos queued before a screen (docs/VIDEO.md), the screen opened only by `video_done`:
    /// Dark Tribe mission 1 → Intro (2) → briefing, mission 2 → the briefing at once; Trojan
    /// mission 1 → TroIntro (8) → the Add-On briefing; the Dark Tribe's last won → Extro (6) → the
    /// main menu; the Trojans' 12th won → TroExtro (10) → their screen.
    #[test]
    fn videos_before_screens() {
        if !s4_formats::game_dir().join("Menu").join("GUISetAOStartscreens.DAT").exists() {
            return;
        }
        let mut gpu = Gpu::headless(800, 600).unwrap();
        let mut f = Frontend::new(&mut gpu, 0).unwrap();
        f.open(&mut gpu, CAMPAIGN_DARK);
        f.command(&mut gpu, CAMPAIGN_DARK, 2081, 0);
        assert_eq!((f.take_video(), f.screen), (Some(2), CAMPAIGN_DARK));
        f.video_done(&mut gpu);
        assert_eq!((f.take_video(), f.screen, f.mission), (None, BRIEFING, Some((4, 0))));
        f.open(&mut gpu, CAMPAIGN_DARK);
        f.command(&mut gpu, CAMPAIGN_DARK, 2082, 0);
        assert_eq!((f.take_video(), f.screen), (None, BRIEFING));

        let trojan = addon::Pack::Ao.screens().iter().find(|s| s.missions.iter().any(|m| m.1 == 15)).unwrap();
        let first = trojan.missions.iter().find(|m| (m.1, m.2) == (15, 0)).unwrap().0;
        f.enter_pack(&mut gpu, addon::Pack::Ao);
        f.open_pack(&mut gpu, trojan.dialog);
        f.command(&mut gpu, trojan.dialog, first, 0);
        assert_eq!((f.take_video(), f.screen), (Some(8), trojan.dialog));
        f.video_done(&mut gpu);
        assert_eq!((f.take_video(), f.screen, f.mission), (None, addon::Pack::Ao.links().brief, Some((15, 0))));

        for (campaign, level, video, screen) in [(4, 11, 6, MAIN), (15, 11, 10, addon::Pack::Ao.back_from_briefing(15))] {
            let mut f = Frontend::new(&mut gpu, 0).unwrap();
            f.end = Some((endstat::EndRecord::default(), Some(victory::Outcome { campaign, level, won: true }), true));
            f.end_done(&mut gpu);
            assert_eq!(f.take_video(), Some(video));
            f.video_done(&mut gpu);
            assert_eq!((f.take_video(), f.screen), (None, screen));
        }
    }

    #[test]
    fn tutorial_menu() {
        if !s4_formats::game_dir().join("Map").join("Tutorial").join("Tutorial03.map").exists() {
            return;
        }
        let mut gpu = Gpu::headless(800, 600).unwrap();
        let mut f = Frontend::new(&mut gpu, 0).unwrap();
        f.command(&mut gpu, MAIN, 2535, 0);
        assert_eq!(f.screen, TUTORIAL);
        let text = |gpu: &Gpu, l: u16| gpu.ui.as_ref().unwrap().state.texts.get(&(TUTORIAL, l)).cloned().unwrap_or_default();
        assert_eq!(text(&gpu, 0xaec), f.text(0x835));
        assert_eq!(text(&gpu, 0xaee), f.text(0x82b));
        assert_eq!(text(&gpu, 0xaf9), f.text(0x834));
        assert!(!text(&gpu, 0xaee).is_empty());
        let Some(Action::Start(path, None, None)) = f.command(&mut gpu, TUTORIAL, 0xaf0, 0) else { panic!("no start") };
        assert!(path.ends_with("Tutorial03.map") && crate::tutorial::is_tutorial_map(&path), "{}", path.display());
        f.command(&mut gpu, TUTORIAL, 0xaed, 0);
        assert_eq!(f.screen, MAIN);
    }

    /// The start screens' GUI sounds (sound/gui.rs) through mouse clicks: each set's procedures,
    /// the calls of a set kept when the next one replaces it, the list scroll only when it moves,
    /// and the code 2 of a typed character.
    #[test]
    fn start_screen_sounds() {
        if !s4_formats::game_dir().join("Menu").join(random::SET).exists() {
            return;
        }
        let mut gpu = Gpu::headless(800, 600).unwrap();
        let mut f = Frontend::new(&mut gpu, 0).unwrap();
        let click = |f: &mut Frontend, gpu: &mut Gpu, dialog: usize, link: u16| -> Vec<u16> {
            f.take_sounds(gpu);
            let p = client::control_point(gpu, dialog, link).unwrap_or_else(|| panic!("no control {link} in {dialog}"));
            f.mouse(gpu, p, GuiMouse::Move);
            f.mouse(gpu, p, GuiMouse::LeftDown);
            f.mouse(gpu, p, GuiMouse::LeftUp);
            f.take_sounds(gpu)
        };
        // Main menu (0x490360): 7, 6, 11; the single player screen's init selects its
        // difficulty radio (0x48d2b0: 7, 11).
        assert_eq!(click(&mut f, &mut gpu, MAIN, 2536), [7, 6, 11, 7, 11]);
        assert_eq!(click(&mut f, &mut gpu, LOCAL_TYPE, 2404), [7, 6, 11]);
        assert_eq!(f.screen, LOCAL_MAP);
        // The list scroll (a type that fires on the press): 11 only when the list moved.
        assert_eq!(click(&mut f, &mut gpu, LOCAL_MAP, 2431), [7, 6], "at the top");
        if f.maps.len() > 7 {
            assert_eq!(click(&mut f, &mut gpu, LOCAL_MAP, 2432), [7, 11, 6]);
        }
        // The random tab: the category tab released (6), the random one pressed (7); 0x48e5e0 has
        // no code-3 sound for 2507; then the Mission CD's set.
        let s = click(&mut f, &mut gpu, LOCAL_MAP, 2507);
        assert_eq!((f.screen, &s[..]), (random::LOCAL, &[6u16, 7][..]));
        assert_eq!(f.start_set(), StartSet::MissionCd);
        // A typed and a deleted character in the game name 0x7c1 (0x4981b0 code 2: 11 twice each).
        gpu.ui.as_mut().unwrap().state.focus = Some((random::LOCAL, random::NAME));
        f.take_sounds(&mut gpu);
        f.key_text(&mut gpu, "a\u{8}");
        assert_eq!(f.take_sounds(&mut gpu), [11, 11, 11, 11]);
        // Back 0x7d4 (11) to the start screens' set.
        assert_eq!(&click(&mut f, &mut gpu, random::LOCAL, random::BACK)[..3], [7, 6, 11]);
        assert_eq!(f.start_set(), StartSet::Main);
    }
}

