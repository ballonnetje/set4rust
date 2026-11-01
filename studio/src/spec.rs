//! The asset spec: everything a replacement asset has to match, written as JSON, plus
//! reference renders of the originals.
//!
//! The output folder is for local work only: `ref/` holds renders of the original game's art and
//! sound, which must never be published or shipped. The JSON (`index.json`, `groups/*.json`)
//! describes slots: one slot is one thing the game draws or plays (a settler job, a building, a
//! GUI image, a terrain page, a sound id, a music track). A slot has sequences (`seqs`): the
//! directions of a job, the parts of a building, the variants of a sound. A gfx sequence lists its
//! frames as `[image, width, height, left, top, flags, strip_x, strip_y]` (see
//! [`FRAME_FIELDS`]); `left`/`top` is the anchor: the frame's top-left corner is drawn at
//! `(anchor_x - left, anchor_y - top)` (`s4_formats::gfx::Image`).
//!
//! Reference strips (`ref/<group>/<slot>/<seq>.png`) are 8-bit indexed PNGs with the frames side
//! by side (1 px apart, wrapped at [`STRIP_MAX_W`]), team colour shown in [`REF_TEAM`]'s blue and
//! shadow pixels as 50 % black. Where a sequence has team-colour pixels, `<seq>.team.png` holds
//! their palette index (192..=255, 0 elsewhere) at the same positions, so a page can recolour the
//! reference per player like the game: 192..224 from the player colour's gradient, 224..256 from
//! the group's `team_ramps` for the sequence's palette (`pal`).
//!
//! The replacement pack layout the Asset Studio writes (and a later game loader reads) is
//! described in `studio/README.md` and in `index.json`'s `pack_layout`.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use s4_formats::gamedata::defines;
use s4_formats::gamedata::resolve;
use s4_formats::gamedata::{Animal, Building, BuildingInfo, Effect, GameData, Good, Object, Race, Settler, Vehicle};
use s4_formats::gfx::landscape::{self, LandscapeTextures, Rgb16, TextureSet};
use s4_formats::gfx::{DecodeOptions, GfxFile, IndexedImage, Layout, PIX_SHADOW, PlayerColor};
use s4_formats::gui::GuiSet;
use s4_formats::snd::{self, SoundBank};

use crate::json::Json;

/// Version of the spec files' layout.
pub const FORMAT: i64 = 1;
/// Reference strips wrap to a new row past this width.
pub const STRIP_MAX_W: u32 = 4096;
/// Team colour the reference renders show (player slot 1).
pub const REF_TEAM: PlayerColor = PlayerColor { slot: 1, rgb: [32, 80, 228] };
/// The player colours the renderer uses (copy of `s4_viewer::sprites::PLAYER_COLORS`).
pub const PLAYER_RGB: [[u8; 3]; 8] =
    [[228, 32, 32], [32, 80, 228], [32, 180, 48], [232, 216, 40], [160, 48, 200], [240, 128, 24], [32, 200, 210], [235, 235, 235]];

pub const FRAME_FIELDS: [&str; 8] = ["image", "w", "h", "left", "top", "flags", "sx", "sy"];
/// Frame flag: the original frame has team-colour pixels (palette 192..=255 of a 512-entry file).
pub const F_TEAM: u8 = 1;
/// Frame flag: the original frame has shadow pixels.
pub const F_SHADOW: u8 = 2;
/// Frame flag: drawn at half size (header flag 0x10; only file 5).
pub const F_HALF: u8 = 4;
/// Frame flag: an opaque, exact-size image (raw GUI backgrounds, terrain pages).
pub const F_RAW: u8 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum What {
    Settlers(Race),
    Buildings(Race),
    Vehicles(Race),
    Animals,
    Objects,
    Piles,
    Icons,
    Effects,
    Effects2,
    Gui,
    Terrain,
    Sounds,
    Music,
}

struct Group {
    id: &'static str,
    label: &'static str,
    category: &'static str,
    /// Gfx file number (sprite groups).
    file: Option<u32>,
    what: What,
}

const fn g(id: &'static str, label: &'static str, category: &'static str, file: Option<u32>, what: What) -> Group {
    Group { id, label, category, file, what }
}

const CATEGORIES: &[(&str, &str)] = &[
    ("settlers", "Settlers"),
    ("buildings", "Buildings"),
    ("vehicles", "Vehicles & ships"),
    ("animals", "Animals"),
    ("objects", "Landscape objects"),
    ("piles", "Goods piles"),
    ("icons", "Icons over settlers"),
    ("effects", "Effects"),
    ("gui", "Interface"),
    ("terrain", "Terrain"),
    ("sounds", "Sound effects"),
    ("music", "Music"),
];

const GROUPS: &[Group] = &[
    g("settlers-roman", "Settlers: Roman", "settlers", Some(20), What::Settlers(Race::ROMAN)),
    g("settlers-viking", "Settlers: Viking", "settlers", Some(21), What::Settlers(Race::VIKING)),
    g("settlers-maya", "Settlers: Maya", "settlers", Some(22), What::Settlers(Race::MAYA)),
    g("settlers-dark", "Settlers: Dark Tribe", "settlers", Some(23), What::Settlers(Race::DARK)),
    g("settlers-trojan", "Settlers: Trojan", "settlers", Some(24), What::Settlers(Race::TROJAN)),
    g("buildings-roman", "Buildings: Roman", "buildings", Some(10), What::Buildings(Race::ROMAN)),
    g("buildings-viking", "Buildings: Viking", "buildings", Some(11), What::Buildings(Race::VIKING)),
    g("buildings-maya", "Buildings: Maya", "buildings", Some(12), What::Buildings(Race::MAYA)),
    g("buildings-dark", "Buildings: Dark Tribe", "buildings", Some(13), What::Buildings(Race::DARK)),
    g("buildings-trojan", "Buildings: Trojan", "buildings", Some(14), What::Buildings(Race::TROJAN)),
    g("vehicles-roman", "Vehicles: Roman", "vehicles", Some(30), What::Vehicles(Race::ROMAN)),
    g("vehicles-viking", "Vehicles: Viking", "vehicles", Some(31), What::Vehicles(Race::VIKING)),
    g("vehicles-maya", "Vehicles: Maya", "vehicles", Some(32), What::Vehicles(Race::MAYA)),
    g("vehicles-trojan", "Vehicles: Trojan", "vehicles", Some(34), What::Vehicles(Race::TROJAN)),
    g("animals", "Animals", "animals", Some(8), What::Animals),
    g("objects", "Trees, stones, plants, waves", "objects", Some(5), What::Objects),
    g("piles", "Goods piles", "piles", Some(3), What::Piles),
    g("icons", "Accessory icons", "icons", Some(7), What::Icons),
    g("effects", "Effects (ids < 100)", "effects", Some(6), What::Effects),
    g("effects-2", "Effects (ids >= 100)", "effects", Some(37), What::Effects2),
    g("gui-menu", "Main menu", "gui", Some(0), What::Gui),
    g("gui-roman", "In-game: Roman", "gui", Some(9), What::Gui),
    g("gui-viking", "In-game: Viking", "gui", Some(19), What::Gui),
    g("gui-maya", "In-game: Maya", "gui", Some(29), What::Gui),
    g("gui-trojan", "In-game: Trojan", "gui", Some(39), What::Gui),
    g("gui-ao", "Add-on start screens", "gui", Some(28), What::Gui),
    g("gui-md", "Mission CD start screens", "gui", Some(18), What::Gui),
    g("gui-md2", "Mission CD 2 / XMD3 start screens", "gui", Some(40), What::Gui),
    g("terrain", "Terrain texture pages (classic set)", "terrain", None, What::Terrain),
    g("sounds", "Sound effects", "sounds", None, What::Sounds),
    g("music", "Music", "music", None, What::Music),
];

/// What the original's frame counts feed besides drawing (searched in s4-sim: every
/// `GfxFile::job_frames` / `dil_frames` read). Conservative: the whole file is marked.
fn keep_frames(what: What) -> Option<&'static str> {
    Some(match what {
        What::Settlers(_) => {
            "s4-sim JobData reads every job x direction frame count; jobInfo.xml nodes with duration 0 last as many ticks as the animation has frames"
        }
        What::Objects => "s4-sim Rules reads landscape object animation lengths (file 5)",
        What::Effects => "s4-sim reads effect animation lengths (file 6: vehicles, Dark Tribe)",
        What::Animals => "s4-sim reads animal animation lengths (direction 2)",
        What::Vehicles(_) => "s4-sim reads vehicle animation lengths (direction 2)",
        What::Buildings(_) => "s4-sim reads building part lengths (Dark Tribe); building patches play for their frame count",
        _ => return None,
    })
}

/// Shared inputs.
struct Ctx {
    game: PathBuf,
    out: PathBuf,
    data: GameData,
    /// (gui gfx file, image) -> uses.
    gui_uses: HashMap<(u32, u16), Vec<String>>,
}

/// How many groups [`run_with_progress`] reports.
pub fn group_count() -> usize {
    GROUPS.len()
}

pub fn run(game: &Path, out: &Path) -> Result<()> {
    run_with_progress(game, out, &|_| {})
}

/// Writes the spec; `done` is called with the number of finished groups.
pub fn run_with_progress(game: &Path, out: &Path, done: &(dyn Fn(usize) + Sync)) -> Result<()> {
    prepare_out(out)?;
    let data = GameData::load(game.join("GameData")).context("GameData")?;
    let ctx = Ctx { game: game.to_path_buf(), out: out.to_path_buf(), data, gui_uses: gui_uses(game)? };

    let t = std::time::Instant::now();
    let finished = std::sync::atomic::AtomicUsize::new(0);
    let results: Vec<Result<Json>> = std::thread::scope(|s| {
        let handles: Vec<_> = GROUPS
            .iter()
            .map(|grp| {
                let (ctx, finished) = (&ctx, &finished);
                s.spawn(move || {
                    let r = export_group(ctx, grp);
                    done(finished.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1);
                    r
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap_or_else(|_| bail!("export thread panicked"))).collect()
    });
    let mut summaries = Vec::new();
    for (grp, r) in GROUPS.iter().zip(results) {
        let summary = r.with_context(|| format!("group {}", grp.id))?;
        summaries.push(summary);
    }

    let categories: Vec<Json> = CATEGORIES
        .iter()
        .map(|&(id, label)| {
            let groups: Vec<Json> = GROUPS.iter().filter(|g| g.category == id).map(|g| Json::from(g.id)).collect();
            Json::obj().with("id", id).with("label", label).with("groups", groups)
        })
        .collect();
    let index = Json::obj()
        .with("format", FORMAT)
        .with("generator", "s4-asset-studio")
        .with("game_dir", game.display().to_string())
        .with("note", "ref/ holds renders of the original game's assets: for local reference only, never publish or ship them")
        .with("frame_fields", Json::Arr(FRAME_FIELDS.iter().map(|&f| f.into()).collect()))
        .with("frame_flags", Json::obj().with("team", F_TEAM).with("shadow", F_SHADOW).with("half", F_HALF).with("raw", F_RAW))
        .with(
            "geometry",
            Json::obj()
                .with("node_dx", 24)
                .with("row_dx", -12)
                .with("row_dy", 12)
                .with("note", "map node (x, y) is drawn at (24x - 12y, 12y - height offset) at zoom 1 (s4-viewer terrain::node_pos); a cell offset (dx, dy) from a sprite's anchor node lies at (24dx - 12dy, 12dy)"),
        )
        .with("ref_team_rgb", REF_TEAM.rgb)
        .with("player_colors", Json::Arr(PLAYER_RGB.iter().map(|&c| Json::from(c)).collect()))
        .with(
            "pack_layout",
            Json::obj()
                .with("manifest", "pack.json: {\"format\": 1, \"name\": \"...\", \"scale\": 1 or 2, \"frame_multiplier\": 1..8 (default for animations)}")
                .with("gfx", "gfx/{group}/{slot}/{seq}/{frame:000}.png (RGBA), original frame count x frame multiplier")
                .with("team_mask", "gfx/{group}/{slot}/{seq}/{frame:000}.team.png (grey, 255 = full team colour, same size as the frame)")
                .with("seq", "gfx/{group}/{slot}/{seq}/seq.json: {\"anchor\": [x, y]} for every frame or {\"anchors\": [[x, y], ...]} per frame, \"frame_multiplier\": k (frames per original frame); without an anchor it is (left, top) x scale of the original frame the replacement frame belongs to (frame j -> original j / k)")
                .with("sound", "snd/{slot}/{seq}.wav (PCM WAV)")
                .with("music", "music/{slot}.mp3|.ogg|.wav"),
        )
        .with("categories", categories)
        .with("groups", summaries)
        .with("not_covered", Json::Arr(NOT_COVERED.iter().map(|&s| s.into()).collect()));
    write_json(&out.join("index.json"), &index)?;
    eprintln!("asset spec written to {} in {:.1} s", out.display(), t.elapsed().as_secs_f64());
    Ok(())
}

/// Original content the runtime also reads that this spec does not cover (yet).
const NOT_COVERED: &[&str] = &[
    "Mouse cursors: 36 cursor resources inside S4_Main.exe (s4_formats::cursor)",
    "Texts: Txt/s4_texts.dat* (interface and mission strings, all languages)",
    "Maps: Map/** (single player, multiplayer, campaign)",
    "Mission and campaign scripts: Script/*.txt",
    "Game data tables: GameData/*.xml, Config/*.cfg (their numbers are the mechanics; rewrite them in the project's own format)",
    "Tables copied from S4_Main.exe into the code (defines, sound table, LZHL tables, terrain tables)",
    "Player logos (Gfx 1 / 35): built at runtime from Logo/*.png",
    "Terrain texture layout: the pages are covered, but the renderer samples fixed hexagon / transition regions of each page (s4_formats::gfx::landscape::column_pieces)",
    "Manuals: Handbook/, Handbuch/",
];

/// Refuse to clear a folder that is not an earlier spec output.
fn prepare_out(out: &Path) -> Result<()> {
    if out.exists() {
        // Earlier builds wrote the spec from s4-tools.
        let is_spec = std::fs::read_to_string(out.join("index.json")).is_ok_and(|s| s.contains("\"s4-asset-studio\"") || s.contains("\"s4-tools asset-spec\""));
        let empty = std::fs::read_dir(out)?.next().is_none();
        if !is_spec && !empty {
            bail!("{} exists and is not an asset-spec output; choose another folder", out.display());
        }
        for sub in ["groups", "ref"] {
            let p = out.join(sub);
            if p.exists() {
                std::fs::remove_dir_all(&p).with_context(|| format!("clearing {}", p.display()))?;
            }
        }
    }
    std::fs::create_dir_all(out.join("groups"))?;
    Ok(())
}

fn write_json(path: &Path, j: &Json) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, j.to_string_compact()).with_context(|| path.display().to_string())
}

fn export_group(ctx: &Ctx, grp: &Group) -> Result<Json> {
    let slots = match grp.what {
        What::Terrain => terrain_slots(ctx, grp)?,
        What::Sounds => sound_slots(ctx, grp)?,
        What::Music => music_slots(ctx)?,
        _ => gfx_slots(ctx, grp)?,
    };
    let (mut seqs, mut frames) = (0usize, 0usize);
    // Palette offset -> one image using it, for the team ramps below.
    let mut pals: Vec<(i64, i64)> = Vec::new();
    for s in &slots {
        if let Json::Obj(f) = s
            && let Some((_, Json::Arr(ss))) = f.iter().find(|(k, _)| k == "seqs")
        {
            seqs += ss.len();
            for q in ss {
                let Json::Obj(qf) = q else { continue };
                let Some((_, Json::Arr(fr))) = qf.iter().find(|(k, _)| k == "frames") else { continue };
                frames += fr.len();
                if let Some((_, Json::Int(pal))) = qf.iter().find(|(k, _)| k == "pal")
                    && let Some(Json::Arr(f0)) = fr.first()
                    && let Some(&Json::Int(image)) = f0.first()
                    && !pals.iter().any(|&(p, _)| p == *pal)
                {
                    pals.push((*pal, image));
                }
            }
        }
    }
    let kind = match grp.what {
        What::Sounds => "sound",
        What::Music => "music",
        _ => "gfx",
    };
    let n_slots = slots.len();
    let mut body = Json::obj()
        .with("id", grp.id)
        .with("label", grp.label)
        .with("category", grp.category)
        .with("kind", kind)
        .with("file", grp.file)
        .with("keep_frames", keep_frames(grp.what))
        .with("team", matches!(grp.what, What::Settlers(_) | What::Vehicles(_)));
    let summary = body.clone().with("slots", n_slots).with("seqs", seqs).with("frames", frames);
    if !pals.is_empty() {
        body.set("team_ramps", team_ramps(ctx, grp, &pals)?);
    }
    body.set("slots", slots);
    write_json(&ctx.out.join("groups").join(format!("{}.json", grp.id)), &body)?;
    Ok(summary)
}

/// Per palette offset, per player slot 0..8: the 32 hand-made colours the game substitutes for
/// palette indices 224..256 (palette entries `256 + 32*slot ..`, see `PlayerColor`), as one hex
/// string `rrggbb` × 32 per player (every job of a settler file has its own palette).
fn team_ramps(ctx: &Ctx, grp: &Group, pals: &[(i64, i64)]) -> Result<Json> {
    let f = GfxFile::open(ctx.game.join("Gfx"), grp.file.context("team colour without a file")?)?;
    let mut out = Json::obj();
    for &(pal, image) in pals {
        let mut players = Vec::new();
        for (slot, &rgb) in PLAYER_RGB.iter().enumerate() {
            let opt = DecodeOptions { player: Some(PlayerColor { slot: slot as u8, rgb }), ..DecodeOptions::default() };
            let wp = f.working_palette(image as usize, &opt)?;
            players.push(Json::from(wp[224..].iter().map(|c| format!("{:02x}{:02x}{:02x}", c[0], c[1], c[2])).collect::<String>()));
        }
        out.set(&pal.to_string(), players);
    }
    Ok(out)
}

// -------------------------------------------------------------------------------------------
// Sprite files
// -------------------------------------------------------------------------------------------

fn safe_id(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect()
}

fn job_name(what: What, job: usize) -> Option<String> {
    let s: Option<&str> = match what {
        What::Settlers(_) => defines::SETTLER_JOB_NAMES.get(job).copied(),
        What::Objects => defines::OBJECT_JOB_NAMES.get(job).copied(),
        What::Vehicles(_) => defines::VEHICLE_JOB_NAMES.get(job).copied(),
        What::Animals => defines::ANIMAL_JOB_NAMES.get(job).copied(),
        What::Icons => defines::ACCESSORY_NAMES.get(job).copied(),
        What::Piles => u16::try_from(job).ok().and_then(Good::from_u16).map(|g| g.name()),
        What::Effects => u16::try_from(job).ok().and_then(Effect::from_u16).map(|e| e.name()),
        What::Effects2 => u16::try_from(job + 100).ok().and_then(Effect::from_u16).map(|e| e.name()),
        What::Buildings(_) => u16::try_from(job).ok().and_then(Building::from_u16).map(|b| b.name()),
        _ => None,
    };
    s.filter(|s| !s.is_empty()).map(str::to_string)
}

/// Which type (settler, object, animal, vehicle) owns each job, from the file's `.sil`.
fn owners(f: &GfxFile, what: What) -> Vec<Option<&'static str>> {
    let name = |t: u16| -> Option<&'static str> {
        match what {
            What::Settlers(_) => Settler::from_u16(t).map(|s| s.name()),
            What::Objects => Object::from_u16(t).map(|o| o.name()),
            What::Animals => Animal::from_u16(t).map(|a| a.name()),
            What::Vehicles(_) => Vehicle::from_u16(t).map(|v| v.name()),
            _ => None,
        }
    };
    let mut firsts: Vec<(usize, u16)> = (0..f.job_groups().len() as u16)
        .filter_map(|t| resolve::first_job(f, t).filter(|_| t != 0).map(|j| (j, t)))
        .collect();
    firsts.sort_unstable();
    (0..f.job_count())
        .map(|j| {
            let i = firsts.partition_point(|&(first, _)| first <= j);
            i.checked_sub(1).and_then(|i| name(firsts[i].1))
        })
        .collect()
}

struct Seq {
    key: String,
    label: String,
    images: Vec<usize>,
    extra: Json,
}

fn gfx_slots(ctx: &Ctx, grp: &Group) -> Result<Vec<Json>> {
    let file = grp.file.expect("sprite group without a file");
    let f = GfxFile::open(ctx.game.join("Gfx"), file)?;
    let owners = owners(&f, grp.what);
    let race_uses = match grp.what {
        What::Settlers(r) => settler_job_uses(&ctx.data, r),
        _ => HashMap::new(),
    };
    let job_sounds: HashMap<u16, Json> = match grp.what {
        What::Settlers(_) => ctx
            .data
            .job_sounds
            .iter()
            .filter_map(|s| {
                let sound = s.sound?;
                let random = if s.random == 0 { 100 } else { s.random };
                Some((s.job, Json::obj().with("name", sound.name()).with("frame", s.frame).with("chance", random)))
            })
            .collect(),
        _ => HashMap::new(),
    };

    let mut used = HashSet::new();
    let mut slots = Vec::new();
    let mut add = |id_base: String, name: String, owner: Option<&str>, seqs: Vec<Seq>, extra: Json| -> Result<()> {
        if seqs.is_empty() {
            return Ok(());
        }
        let mut id = safe_id(&id_base);
        if !used.insert(id.clone()) {
            id = format!("{id}_{}", slots.len());
            used.insert(id.clone());
        }
        let mut seq_json = Vec::new();
        for s in seqs {
            seq_json.push(write_seq(ctx, grp, &f, &id, s)?);
        }
        let mut slot = Json::obj().with("id", id).with("name", name).with("owner", owner);
        if let Json::Obj(fields) = extra {
            for (k, v) in fields {
                slot.set(&k, v);
            }
        }
        slot.set("seqs", seq_json);
        slots.push(slot);
        Ok(())
    };

    match f.layout {
        Layout::Flat => {
            for i in 0..f.image_count() {
                let uses = ctx.gui_uses.get(&(file, i as u16)).cloned().unwrap_or_default();
                let exact = !uses.is_empty() || f.header(i)?.is_raw();
                let extra = Json::obj()
                    .with("exact_size", exact)
                    .with("uses", Json::Arr(uses.into_iter().map(Json::from).collect()));
                let seq = Seq { key: "0".into(), label: "image".into(), images: vec![i], extra: Json::obj() };
                add(format!("{i:04}"), format!("image {i}"), None, vec![seq], extra)?;
            }
        }
        Layout::Buildings => {
            let race = match grp.what {
                What::Buildings(r) => r,
                _ => bail!("building layout in a non-building group"),
            };
            // Settler frame counts for the house workers' routines (jobInfo durations of 0).
            let settlers = GfxFile::open(ctx.game.join("Gfx"), resolve::settler_file(race)).ok();
            for b in 0..f.job_count() {
                let Some(dirs) = f.job_dirs(b) else { continue };
                let info = u16::try_from(b).ok().and_then(Building::from_u16).and_then(|bb| ctx.data.building(race, bb));
                let mut seqs = Vec::new();
                for (k, d) in dirs.clone().enumerate() {
                    let Some(fr) = f.dil_frames(d) else { continue };
                    if fr.is_empty() {
                        continue;
                    }
                    let part = defines::BUILDING_JOB_NAMES.get(d).copied().unwrap_or("");
                    let role = match k {
                        0 => "construction",
                        1 => "finished",
                        _ => "part",
                    };
                    let mut extra = Json::obj().with("part_name", part);
                    if let Some(p) = info.and_then(|i| i.patches.iter().find(|p| p.job == Some(d as u16))) {
                        let mut pj = Json::obj().with("type", p.kind.map(|k| k.name())).with("ticks", p.ticks).with("slot", p.slot);
                        if let Some(s) = &p.sound
                            && let Some(sound) = s.sound
                        {
                            let chance = if s.random == 0 { 100 } else { s.random };
                            pj.set("sound", Json::obj().with("name", sound.name()).with("frame", s.frame).with("chance", chance));
                        }
                        extra.set("patch", pj);
                    }
                    seqs.push(Seq { key: k.to_string(), label: format!("{k} {role}"), images: fr.collect(), extra });
                }
                let name = job_name(grp.what, b).unwrap_or_else(|| format!("building {b}"));
                let extra = info.map(|i| building_json(i, &ctx.data, settlers.as_ref())).unwrap_or_else(Json::obj);
                add(name.clone(), name, None, seqs, extra)?;
            }
        }
        Layout::Jobs => {
            for job in 0..f.job_count() {
                let Some(dirs) = f.job_dirs(job) else { continue };
                let mut seqs = Vec::new();
                for (k, d) in dirs.enumerate() {
                    let Some(fr) = f.dil_frames(d) else { continue };
                    if fr.is_empty() {
                        continue;
                    }
                    let label = match grp.what {
                        What::Objects => format!("stage {k}"),
                        What::Piles => format!("amount {}", k + 1),
                        What::Icons => "icon".to_string(),
                        _ => format!("direction {k}"),
                    };
                    seqs.push(Seq { key: k.to_string(), label, images: fr.collect(), extra: Json::obj() });
                }
                let name = job_name(grp.what, job).unwrap_or_else(|| format!("job {job}"));
                let mut extra = Json::obj().with("job", job);
                if let Some((lists, timed)) = race_uses.get(&(job as u16)) {
                    extra.set("job_lists", Json::Arr(lists.iter().map(|s| Json::from(s.as_str())).collect()));
                    extra.set("length_from_frames", Json::Arr(timed.iter().map(|s| Json::from(s.as_str())).collect()));
                }
                if let Some(s) = job_sounds.get(&(job as u16)) {
                    extra.set("sound", s.clone());
                }
                add(name.clone(), name, owners.get(job).copied().flatten(), seqs, extra)?;
            }
        }
    }
    Ok(slots)
}

/// Per settler gfx job of `race`: the jobInfo.xml lists that play it, and those of them where a
/// node has duration 0 (its length is the animation's frame count).
fn settler_job_uses(data: &GameData, race: Race) -> HashMap<u16, (Vec<String>, Vec<String>)> {
    let mut m: HashMap<u16, (Vec<String>, Vec<String>)> = HashMap::new();
    for list in data.jobs.iter().filter(|l| l.race == race) {
        for node in &list.nodes {
            let Some(jp) = node.job_part else { continue };
            let e = m.entry(jp).or_default();
            if !e.0.contains(&list.name) {
                e.0.push(list.name.clone());
            }
            if node.duration == 0 && !e.1.contains(&list.name) {
                e.1.push(list.name.clone());
            }
        }
    }
    m
}

fn mask_cells(lines: &[u32], hx: i32, hy: i32) -> Json {
    let mut cells = Vec::new();
    for (r, &m) in lines.iter().enumerate() {
        for c in 0..32 {
            if m & (1u32 << (31 - c)) != 0 {
                cells.push(Json::from([c - hx, r as i32 - hy]));
            }
        }
    }
    Json::Arr(cells)
}

/// buildingInfo.xml data that the art must line up with (cell offsets from the anchor node).
fn building_json(b: &BuildingInfo, data: &GameData, settlers: Option<&GfxFile>) -> Json {
    let (hx, hy) = (b.hotspot.x, b.hotspot.y);
    let cell = |o: s4_formats::gamedata::building::Offset| Json::from([o.x, o.y]);
    Json::obj().with(
        "building",
        Json::obj()
            .with("kind", b.kind.map(|k| k.name()))
            .with("inhabitant", b.inhabitant.map(|s| s.name()))
            .with("footprint", mask_cells(&b.building_pos_lines, hx, hy))
            .with("block", mask_cells(&b.block_pos_lines, hx, hy))
            .with("dig", mask_cells(&b.dig_pos_lines, hx, hy))
            .with("door", cell(b.door))
            .with("flag", cell(b.flag))
            // Buildings without a work spot carry leftovers around 980 (the game keeps the
            // offset in a signed byte); null there.
            .with("work", (b.working_pos.x.abs() < 128 && b.working_pos.y.abs() < 128).then(|| cell(b.working_pos)))
            .with("builders", Json::Arr(b.builders.iter().map(|s| Json::from([s.x, s.y, s.dir])).collect()))
            .with(
                "piles",
                Json::Arr(
                    b.piles
                        .iter()
                        .map(|p| {
                            Json::obj()
                                .with("cell", [p.x, p.y])
                                .with("good", p.good.map(|g| g.name()))
                                .with("px", [p.x_pixel, p.y_pixel])
                        })
                        .collect(),
                ),
            )
            // Buildings without a mini flag carry leftovers in the hundreds (storage area 471, 791).
            .with("mini_flag_px", (b.mini_flag.x.abs() < 256 && b.mini_flag.y.abs() < 256).then(|| Json::from([b.mini_flag.x, b.mini_flag.y])))
            .with(
                "soldiers_px",
                Json::Arr(b.soldiers.iter().map(|s| Json::from([s.x, s.y, s.direction, s.top as i32])).collect()),
            )
            .with("work_lists", Json::Arr(b.anim_lists.iter().filter_map(|&l| work_list(b, l, data, settlers)).collect())),
    )
}

/// The house worker's routine for anim list `list` (jobInfo.xml), step by step as
/// `CHouseWorkerRole` plays it (s4-sim `jobs/house_worker.rs`): the worker starts on the door
/// cell (`SettlerEnter`), `GO_VIRTUAL` moves it in a straight line to the node's cell, the other
/// virtual tasks play in place. `ticks` replays the role's update loop (TakeJob's first
/// registration, then LogicUpdateJob's); `null` for tasks outside the virtual routine.
fn work_list(b: &BuildingInfo, list: u16, data: &GameData, settlers: Option<&GfxFile>) -> Option<Json> {
    let jl = data.jobs.iter().find(|j| j.race == b.race && j.list.is_some_and(|a| a as u16 == list))?;
    let cell_px = |x: i32, y: i32| [24 * x - 12 * y, 12 * y];
    let mut at = cell_px(b.door.x, b.door.y);
    let (mut start, mut steps) = (0i32, Vec::new());
    for n in &jl.nodes {
        let frames = match (settlers, n.job_part) {
            (Some(f), Some(jp)) => f.job_frames(jp as usize, n.dir.max(0) as usize).map_or(0, |r| r.len() as i32),
            _ => 0,
        };
        let duration = if n.duration == 0 { frames } else { n.duration };
        let task = n.task.map_or(u8::MAX, |t| t as u8);
        let to = if task == 0 { cell_px(n.x, n.y) } else { at };
        let ticks = worker_ticks(task, duration, frames, at, to);
        let sound = n.job_part.and_then(|jp| data.job_sounds.iter().find(|s| s.job == jp)).and_then(|s| {
            let chance = if s.random == 0 { 100 } else { s.random };
            Some(Json::obj().with("name", s.sound?.name()).with("frame", s.frame).with("chance", chance))
        });
        steps.push(
            Json::obj()
                .with("task", n.task_name.as_str())
                .with("cell", [n.x, n.y])
                .with("from", at)
                .with("to", to)
                .with("start", start)
                .with("ticks", ticks)
                .with("dir", n.dir)
                .with("job", n.job_part_name.as_str())
                .with("frames", frames)
                .with("forward", n.forward)
                .with("visible", n.visible)
                .with("good", (!n.entity_name.is_empty()).then_some(n.entity_name.as_str()))
                .with("trigger", (!n.trigger_name.is_empty()).then_some(n.trigger_name.as_str()))
                .with("sound", sound),
        );
        start += ticks.unwrap_or(0);
        at = to;
    }
    Some(Json::obj().with("name", jl.name.as_str()).with("ticks", start).with("steps", steps))
}

/// Ticks a house worker's virtual task lasts: `CHouseWorkerRole::TakeJob`'s registration, then
/// `LogicUpdateJob` re-registering until the task ends (a delay ≤ 0 schedules the next tick).
/// `counter` = the node's duration, `cycle` = its animation's frame count.
fn worker_ticks(task: u8, counter: i32, cycle: i32, from: [i32; 2], to: [i32; 2]) -> Option<i32> {
    let reg = |d: i32| d.max(1);
    match task {
        // GO_VIRTUAL: 9 × the distance in rows / columns, the offset stepping evenly each tick.
        0 => {
            let (dx, dy) = (to[0] - from[0], to[1] - from[1]);
            let (a, b) = (dy.abs(), (dx - dy).abs());
            let n = 9 * ((2 * b + 24) / 48).max((2 * a + 12) / 24);
            Some(reg(n))
        }
        // WORK_VIRTUAL, PRODUCE_VIRTUAL: whole cycles until the duration is used up.
        1 | 3 => replay(counter, cycle - 1, |c| {
            let v = (*c).min(cycle);
            *c -= v;
            (*c > 0).then_some(v)
        }),
        // WAIT_VIRTUAL: steps of 31.
        2 => replay(counter, counter.min(31), |c| {
            *c -= 31;
            (*c > 0).then(|| (*c).min(31))
        }),
        // PUT_GOOD_VIRTUAL / GET_GOOD_VIRTUAL: half cycles.
        4 | 5 => {
            let v = cycle / 2;
            if v <= 0 {
                return None;
            }
            replay(counter, counter / 2, |c| {
                *c -= v;
                (*c >= v).then_some(if task == 4 { v - 1 } else { v })
            })
        }
        _ => None,
    }
}

/// Total ticks of a task registered first after `first` ticks, then after each delay `next`
/// returns while it updates the counter (`None` = the task ends). A delay ≤ 0 is the next tick.
/// `None` when the counter does not run down (a frame count of 0 or 1).
fn replay(mut counter: i32, first: i32, mut next: impl FnMut(&mut i32) -> Option<i32>) -> Option<i32> {
    let mut t = first.max(1);
    for _ in 0..1000 {
        match next(&mut counter) {
            Some(d) => t += d.max(1),
            None => return Some(t),
        }
    }
    None
}

/// Frames of one sequence, its reference strip and team mask.
fn write_seq(ctx: &Ctx, grp: &Group, f: &GfxFile, slot: &str, s: Seq) -> Result<Json> {
    let team_file = f.has_team_colour();
    let mut decoded: Vec<IndexedImage> = Vec::with_capacity(s.images.len());
    for &i in &s.images {
        decoded.push(f.decode_indexed(i)?);
    }
    let sizes: Vec<(u32, u32)> =
        decoded.iter().map(|d| (d.header.width.max(1) as u32, d.header.height.max(1) as u32)).collect();
    let (sw, sh, pos) = pack_strip(&sizes);
    let mut px = vec![0u8; (sw * sh) as usize];
    let mut mask = vec![0u8; (sw * sh) as usize];
    let mut any_team = false;
    let mut frames = Vec::new();
    for ((ix, &(x0, y0)), &image) in decoded.iter().zip(&pos).zip(&s.images) {
        let h = ix.header;
        let (w, hh) = (h.width as usize, h.height as usize);
        let mut flags = 0u8;
        for y in 0..hh {
            for x in 0..w {
                let p = ix.pixels[y * w + x];
                let o = (y0 as usize + y) * sw as usize + x0 as usize + x;
                px[o] = p;
                if !ix.raw {
                    if p == PIX_SHADOW {
                        flags |= F_SHADOW;
                    } else if team_file && p >= 192 {
                        flags |= F_TEAM;
                        mask[o] = p;
                    }
                }
            }
        }
        if h.half_scale() {
            flags |= F_HALF;
        }
        if ix.raw {
            flags |= F_RAW;
        }
        any_team |= flags & F_TEAM != 0;
        frames.push(Json::from([image as i32, w as i32, hh as i32, h.left as i32, h.top as i32, flags as i32, x0 as i32, y0 as i32]));
    }

    let rel = format!("ref/{}/{}/{}", grp.id, slot, s.key);
    let opt = DecodeOptions { player: Some(REF_TEAM), ..DecodeOptions::default() };
    let pal = f.working_palette(s.images[0], &opt)?;
    let index1 = if decoded[0].raw {
        Index1::Colour
    } else if f.is_gui() {
        Index1::Black
    } else {
        Index1::Shadow
    };
    write_indexed_png(&ctx.out.join(format!("{rel}.png")), sw, sh, &px, &pal, index1)?;
    if any_team {
        write_grey_png(&ctx.out.join(format!("{rel}.team.png")), sw, sh, &mask)?;
    }

    let mut j = Json::obj()
        .with("key", s.key)
        .with("label", s.label)
        .with("ref", format!("{rel}.png"))
        .with("mask", any_team.then(|| format!("{rel}.team.png")))
        .with("pal", if any_team { f.palette_offset(s.images[0]).map(|o| o as i64) } else { None })
        .with("frames", frames);
    if let Json::Obj(fields) = s.extra {
        for (k, v) in fields {
            j.set(&k, v);
        }
    }
    Ok(j)
}

/// Left-to-right placement with a 1 px gap, wrapping at [`STRIP_MAX_W`]: (width, height, positions).
pub fn pack_strip(sizes: &[(u32, u32)]) -> (u32, u32, Vec<(u32, u32)>) {
    let (mut x, mut y, mut row_h, mut w) = (0u32, 0u32, 0u32, 1u32);
    let mut pos = Vec::with_capacity(sizes.len());
    for &(fw, fh) in sizes {
        if x > 0 && x + fw > STRIP_MAX_W {
            y += row_h + 1;
            x = 0;
            row_h = 0;
        }
        pos.push((x, y));
        w = w.max(x + fw);
        x += fw + 1;
        row_h = row_h.max(fh);
    }
    (w, (y + row_h).max(1), pos)
}

fn png_writer(path: &Path, w: u32, h: u32) -> Result<png::Encoder<'static, std::io::BufWriter<std::fs::File>>> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let file = std::fs::File::create(path).with_context(|| path.display().to_string())?;
    Ok(png::Encoder::new(std::io::BufWriter::new(file), w, h))
}

/// How index 1 is drawn: the shadow of world sprites, the opaque black GUI files unpack it to
/// (`UnpackGfxTransparent`), or a plain palette colour in raw images.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Index1 {
    Shadow,
    Black,
    Colour,
}

/// 8-bit indexed PNG: index 0 transparent, index 1 per [`Index1`], the rest from the working
/// palette (its alpha comes from the 4444 palette where the file has one).
fn write_indexed_png(path: &Path, w: u32, h: u32, px: &[u8], pal: &[[u8; 4]; 256], index1: Index1) -> Result<()> {
    let mut plte = Vec::with_capacity(768);
    let mut trns = Vec::with_capacity(256);
    for (i, c) in pal.iter().enumerate() {
        let (rgb, a) = match i {
            0 => ([0, 0, 0], 0),
            1 if index1 == Index1::Shadow => ([0, 0, 0], 128),
            1 if index1 == Index1::Black => ([0, 0, 0], 255),
            _ => ([c[0], c[1], c[2]], c[3]),
        };
        plte.extend_from_slice(&rgb);
        trns.push(a);
    }
    let mut e = png_writer(path, w, h)?;
    e.set_color(png::ColorType::Indexed);
    e.set_depth(png::BitDepth::Eight);
    e.set_palette(plte);
    e.set_trns(trns);
    let mut wr = e.write_header()?;
    wr.write_image_data(px)?;
    wr.finish()?;
    Ok(())
}

fn write_grey_png(path: &Path, w: u32, h: u32, px: &[u8]) -> Result<()> {
    let mut e = png_writer(path, w, h)?;
    e.set_color(png::ColorType::Grayscale);
    e.set_depth(png::BitDepth::Eight);
    let mut wr = e.write_header()?;
    wr.write_image_data(px)?;
    wr.finish()?;
    Ok(())
}

fn write_rgba_png(path: &Path, w: u32, h: u32, px: &[u8]) -> Result<()> {
    let mut e = png_writer(path, w, h)?;
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    let mut wr = e.write_header()?;
    wr.write_image_data(px)?;
    wr.finish()?;
    Ok(())
}

/// GUI images the dialog layouts (`Menu\GUISet*.DAT`) place, with the rectangle they must fill.
/// `DrawControl` / `UpdateGui` require the image to be exactly the rectangle's size, so a
/// reference whose image has another size is not a real use (e.g. `pressed_image` 0 on controls
/// without a pressed state) and is left out.
fn gui_uses(game: &Path) -> Result<HashMap<(u32, u16), Vec<String>>> {
    let mut m: HashMap<(u32, u16), Vec<String>> = HashMap::new();
    let mut sizes: HashMap<u32, Vec<(u16, u16)>> = HashMap::new();
    let menu = s4_formats::path::resolve(game.join("Menu"));
    let mut names: Vec<String> = std::fs::read_dir(&menu)
        .with_context(|| menu.display().to_string())?
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .collect();
    names.sort();
    for name in names {
        let Some(file) = s4_formats::gui::gfx_file_for(&name) else { continue };
        let set = GuiSet::open(menu.join(&name)).with_context(|| name.clone())?;
        let set_name = name.trim_end_matches(".DAT").trim_end_matches(".dat");
        let image_sizes = match sizes.entry(file) {
            std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
            std::collections::hash_map::Entry::Vacant(e) => {
                let f = GfxFile::open(game.join("Gfx"), file)?;
                e.insert((0..f.image_count()).map(|i| f.header(i).map(|h| (h.width, h.height))).collect::<Result<_, _>>()?)
            }
        };
        let fits = |img: u16, w: u16, h: u16| image_sizes.get(img as usize) == Some(&(w, h));
        for d in set.dialogs.iter().flatten() {
            if d.flags & 2 == 0 && fits(d.background, d.width, d.height) {
                m.entry((file, d.background))
                    .or_default()
                    .push(format!("{set_name} dialog {} background, {}x{}", d.slot, d.width, d.height));
            }
            for c in d.controls.iter().filter(|c| c.has_image()) {
                for (img, what) in [(c.image, ""), (c.pressed_image, " (pressed)")] {
                    if fits(img, c.width, c.height) {
                        m.entry((file, img)).or_default().push(format!(
                            "{set_name} dialog {} {} {}{what}, {}x{}",
                            d.slot,
                            c.kind_name(),
                            c.link,
                            c.width,
                            c.height
                        ));
                    }
                }
            }
        }
    }
    Ok(m)
}

// -------------------------------------------------------------------------------------------
// Terrain, sounds, music
// -------------------------------------------------------------------------------------------

fn terrain_slots(ctx: &Ctx, grp: &Group) -> Result<Vec<Json>> {
    let path = landscape::texture_path(&ctx.game, TextureSet::Classic, true, Rgb16::Rgb565);
    let tex = LandscapeTextures::open(&path, Rgb16::Rgb565).with_context(|| path.display().to_string())?;
    let pages = tex.hardware_pages().context("no 16-bit texture record")?;
    let mut slots = Vec::new();
    for (p, page) in pages.iter().enumerate() {
        let id = format!("page_{p:02}");
        let rel = format!("ref/{}/{id}/0.png", grp.id);
        write_rgba_png(&ctx.out.join(&rel), page.size, page.size, &page.rgba)?;
        let name = if p >= 22 { format!("page {p} (dark land of page {})", p - 22) } else { format!("page {p}") };
        let s = page.size as i32;
        let seq = Json::obj()
            .with("key", "0")
            .with("label", "page")
            .with("ref", rel)
            .with("mask", Json::Null)
            .with("frames", vec![Json::from([p as i32, s, s, 0, 0, F_RAW as i32, 0, 0])]);
        slots.push(
            Json::obj()
                .with("id", id)
                .with("name", name)
                .with("owner", Json::Null)
                .with("exact_size", true)
                .with("uses", vec![Json::from("terrain renderer: fixed hexagon and transition regions of this page (keep the layout)")])
                .with("seqs", vec![seq]),
        );
    }
    Ok(slots)
}

/// `[SECTION] { KEY = value }` files of `Config\` (comments start with `;`).
pub fn parse_cfg(text: &str) -> BTreeMap<String, Vec<(String, String)>> {
    let mut m: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    let mut cur = String::new();
    for line in text.lines() {
        let line = line.split(';').next().unwrap_or("").trim();
        if let Some(s) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            cur = s.trim().to_string();
        } else if let Some((k, v)) = line.split_once('=') {
            m.entry(cur.clone()).or_default().push((k.trim().to_string(), v.trim().trim_matches('"').to_string()));
        }
    }
    m
}

fn read_cfg(game: &Path, name: &str) -> BTreeMap<String, Vec<(String, String)>> {
    let p = s4_formats::path::resolve(game.join("Config").join(name));
    std::fs::read(p).map(|b| parse_cfg(&String::from_utf8_lossy(&b))).unwrap_or_default()
}

fn sound_slots(ctx: &Ctx, grp: &Group) -> Result<Vec<Json>> {
    let bank = SoundBank::open(ctx.game.join("Snd"))?;
    let cfg = read_cfg(&ctx.game, "SoundFX.cfg");
    let volumes: HashMap<&str, &str> =
        cfg.get("SOUND_VOLUMES").map(|v| v.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect()).unwrap_or_default();

    // Who plays each sound id (data-driven uses; code-driven ones are not listed).
    let mut uses: HashMap<&str, Vec<String>> = HashMap::new();
    for s in &ctx.data.job_sounds {
        if let Some(sound) = s.sound {
            let chance = if s.random == 0 { 100 } else { s.random };
            uses.entry(sound.name()).or_default().push(format!("settler job {} on frame {} ({chance} %)", s.job_name, s.frame));
        }
    }
    for b in &ctx.data.buildings {
        for p in &b.patches {
            if let Some(sr) = &p.sound
                && let Some(sound) = sr.sound
            {
                uses.entry(sound.name()).or_default().push(format!(
                    "{} {} patch {} on frame {}",
                    b.race.name(),
                    b.building.name(),
                    p.job_name,
                    sr.frame
                ));
            }
        }
    }

    let mut slots = Vec::new();
    for sid in snd::SOUNDS.iter().skip(1) {
        let mut seqs = Vec::new();
        for (k, slot) in sid.slots().enumerate() {
            let key = format!("{k:02}");
            let mut q = Json::obj().with("key", key.clone()).with("label", format!("variant {k} (bank slot {slot})"));
            match bank.raw_wav(slot)? {
                Some(_) => {
                    let s = bank.sample(slot)?;
                    let rel = format!("ref/{}/{}/{key}.wav", grp.id, sid.name);
                    let p = ctx.out.join(&rel);
                    std::fs::create_dir_all(p.parent().unwrap())?;
                    std::fs::write(&p, s.to_wav())?;
                    q.set("ref", rel);
                    q.set("rate", s.rate);
                    q.set("channels", s.channels);
                    q.set("secs", s.duration_secs());
                }
                None => {
                    q.set("ref", Json::Null);
                    q.set("missing", true);
                }
            }
            seqs.push(q);
        }
        let volume = volumes.get(sid.name).and_then(|v| v.parse::<i64>().ok()).unwrap_or(100);
        let mut u: Vec<Json> = uses.remove(sid.name).unwrap_or_default().into_iter().map(Json::from).collect();
        u.dedup_by(|a, b| a.to_string_compact() == b.to_string_compact());
        slots.push(
            Json::obj()
                .with("id", sid.name)
                .with("name", sid.name)
                .with("owner", Json::Null)
                .with("volume", volume)
                .with("note", "the game picks a random variant each time the id plays")
                .with("uses", u)
                .with("seqs", seqs),
        );
    }
    Ok(slots)
}

fn music_slots(ctx: &Ctx) -> Result<Vec<Json>> {
    let cfg = read_cfg(&ctx.game, "SoundTracks.cfg");
    let mut uses: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (section, entries) in &cfg {
        if section.ends_with("_FADETIME") {
            continue;
        }
        for (_, v) in entries {
            let file = v.rsplit(['/', '\\']).next().unwrap_or(v).to_ascii_lowercase();
            let u = uses.entry(file).or_default();
            if !u.contains(section) {
                u.push(section.clone());
            }
        }
    }
    let snd = s4_formats::path::resolve(ctx.game.join("Snd"));
    let mut files: Vec<String> = std::fs::read_dir(&snd)?
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.to_ascii_lowercase().ends_with(".mp3"))
        .collect();
    files.sort_by_key(|n| n.to_ascii_lowercase());
    Ok(files
        .into_iter()
        .map(|name| {
            let stem = name[..name.len() - 4].to_ascii_lowercase();
            let u = uses.get(&name.to_ascii_lowercase()).cloned().unwrap_or_default();
            let seq = Json::obj().with("key", "0").with("label", "track").with("ref", format!("game/Snd/{name}"));
            Json::obj()
                .with("id", safe_id(&stem))
                .with("name", name.clone())
                .with("owner", Json::Null)
                .with("uses", Json::Arr(u.into_iter().map(Json::from).collect()))
                .with("seqs", vec![seq])
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_spec_folders_are_cleared() {
        let d = std::env::temp_dir().join(format!("s4rs-prepare-{}", std::process::id()));
        std::fs::create_dir_all(d.join("ref")).unwrap();
        std::fs::write(d.join("index.json"), r#"{"generator":"s4-tools asset-spec"}"#).unwrap();
        prepare_out(&d).unwrap();
        assert!(!d.join("ref").exists() && d.join("groups").is_dir());
        std::fs::write(d.join("index.json"), r#"{"generator":"s4-asset-studio"}"#).unwrap();
        prepare_out(&d).unwrap();
        std::fs::write(d.join("index.json"), "{}").unwrap();
        assert!(prepare_out(&d).is_err(), "a folder of something else stays");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn house_worker_ticks_follow_the_role() {
        // GO_VIRTUAL from the anchor to cell (3, 6) = (0, 72) px: 9 · 6 = 54 (s4-sim sim_jobs.rs).
        assert_eq!(worker_ticks(0, 0, 13, [0, 0], [0, 72]), Some(54));
        // One cell to the right (24 px): 9; no move: the next tick.
        assert_eq!(worker_ticks(0, 0, 13, [0, 72], [24, 72]), Some(9));
        assert_eq!(worker_ticks(0, 0, 13, [24, 72], [24, 72]), Some(1));
        // WORK_VIRTUAL of 35 ticks with a 12-frame cycle: registered after 11, then 12 and 12;
        // the update after that takes the last 11 and ends the task.
        assert_eq!(worker_ticks(1, 35, 12, [0, 0], [0, 0]), Some(11 + 12 + 12));
        // WAIT_VIRTUAL 10: one registration of 10.
        assert_eq!(worker_ticks(2, 10, 8, [0, 0], [0, 0]), Some(10));
        // GET_GOOD_VIRTUAL over 8 frames: 4, then 4; PUT_GOOD_VIRTUAL: 4, then 3.
        assert_eq!(worker_ticks(5, 8, 8, [0, 0], [0, 0]), Some(8));
        assert_eq!(worker_ticks(4, 8, 8, [0, 0], [0, 0]), Some(7));
        // A cycle that cannot run the counter down has no length.
        assert_eq!(worker_ticks(4, 8, 1, [0, 0], [0, 0]), None);
    }

    #[test]
    fn strip_wraps_rows() {
        let (w, h, pos) = pack_strip(&[(3000, 10), (2000, 20), (100, 5)]);
        assert_eq!(pos, vec![(0, 0), (0, 11), (2001, 11)]);
        assert_eq!((w, h), (3000, 31));
    }

    #[test]
    fn cfg_sections() {
        let m = parse_cfg("; c\n[A]\n{\n\tX = 4 ; v\n\tY = \"snd/a.mp3\"\n}\n[B]\n{\n}\n");
        assert_eq!(m["A"], vec![("X".to_string(), "4".to_string()), ("Y".to_string(), "snd/a.mp3".to_string())]);
        assert!(!m.contains_key("B"));
    }

    #[test]
    fn mask_rows_are_msb_first() {
        let j = mask_cells(&[0x8000_0000, 0x4000_0000], 1, 1);
        assert_eq!(j.to_string_compact(), "[[-1,-1],[0,0]]");
    }
}
