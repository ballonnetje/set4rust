# S4RS Asset Studio

A native desktop app to browse every sprite, sound, GUI image and music track of The Settlers IV,
compare the original with your replacement, validate it, and export the result. It reads the game
data of your own copy of The Settlers IV History Edition and ships none of it.

**The game does not load packs yet.** The studio prepares them: the format below is what a later
game loader reads. Building data edited here is stored in the pack, and nothing in the game reads
it so far.

**`assetspec/ref/` holds renders of the game's own art and sound. It must stay local:** never copy it
into a pack, never publish or ship it. The JSON next to it describes the originals (sizes, anchors,
frame counts, footprints) and holds no pixels.

## Build

It is a Cargo workspace of its own, with its own `Cargo.lock`. From this folder:

```text
cargo build --release          target/release/s4-asset-studio(.exe)
cargo test --release
```

The UI is [egui](https://github.com/emilk/egui) on wgpu, so it needs a GPU driver with Vulkan, Metal,
DirectX 12 or OpenGL ES 3 at run time.

| System | To build | To run |
| --- | --- | --- |
| Windows 10 / 11 | Rust; the MSVC build tools | nothing else |
| Linux (Debian, Ubuntu) | Rust, a C linker, `pkg-config`, `libasound2-dev` | `libasound2`, `libxkbcommon0`, `libwayland-client0` or the X11 libraries (`libx11-6`, `libxcursor1`, `libxrandr2`, `libxi6`, `libxkbcommon-x11-0`), Vulkan or OpenGL drivers (`mesa-vulkan-drivers`), `xdg-desktop-portal` with a backend (or `zenity`) for the file dialogs |
| Linux (Fedora) | Rust, a C linker, `pkgconf`, `alsa-lib-devel` | the same libraries under their Fedora names |
| macOS 11 or later | Rust, Xcode command line tools | nothing else |

Wayland and X11 are loaded at run time, so they need no development packages. On Linux the file
dialogs use the XDG desktop portal, so no GTK development package is needed either; without a portal
they fall back to `zenity`.

## Run

```text
s4-asset-studio [--game DIR] [--data DIR]
s4-asset-studio --export-spec DIR [--game DIR]     write the asset spec and stop
s4-asset-studio --version | --help
```

| Option | Meaning |
| --- | --- |
| `--game DIR` | the game's folder, the one with `S4_Main.exe` |
| `--data DIR` | where the asset spec, your pack and the settings live |
| `--export-spec DIR` | write the asset spec to `DIR` and stop |

Started without arguments it finds the game the way the game itself does: `S4_DIR`, the remembered
folder, Ubisoft Connect and GOG entries in the registry, the usual folders, Wine prefixes. With no
install found the window opens on a screen that asks for the folder, says what a wrong folder lacks
and remembers the choice. The first start writes the asset spec into the data folder (a few seconds,
about 300 MB), with progress in the window. It is written again when the game folder or the app's
version changes.

The Windows release has no console window. `--help`, `--version` and error text go to the terminal
it was started from, or to where they are redirected.

### Data folder

| System | Default |
| --- | --- |
| Windows | `%APPDATA%\s4rs-asset-studio` |
| macOS | `~/Library/Application Support/s4rs-asset-studio` |
| Linux | `$XDG_DATA_HOME/s4rs-asset-studio`, else `~/.local/share/s4rs-asset-studio` |

```text
assetspec/        the spec: a cache, safe to delete
  index.json
  groups/*.json
  ref/            renders of the game's art and sound: keep them local
  source.txt      which build and game folder wrote it
pack/             your work
settings.json     theme, layer order, window size, the last slot
game_dir.txt      the game folder you chose
```

## Using it

* **Sidebar.** Categories, groups and slots with progress; search (`/`) over id, name and owner;
  filter all, to do, problems, done. The header shows the total replaced.
* **Compare.** Per sequence the original and your version side by side, animated. Speed defaults to
  the game's 14.1 fps for the original and k× that for yours; zoom; map grid, anchor cross, footprint
  overlay and an onion skin; team colour per player, with your frames mixed by the formula below and
  the original recoloured with its own team ramps. The panel on the right holds the slot's data.
* **Edit** (every slot with a map anchor). One large view of a sequence with all layers. The wheel
  zooms around the pointer, the middle or right button pans. The right panel holds the layers (show
  or hide, drag the grip or use the arrows to reorder; the order is remembered and the Compare cards
  use it too), the inspector and the gameplay data with reset buttons. Edits go to a working copy:
  nothing is written until Save (Ctrl+S), Discard drops them, and leaving the slot, reloading the
  pack, actions that write and closing the window all ask first.
  * Layers: original image, your image, map grid, building area, blocked, dig, door, flag, work spot,
    builder spots with facing, piles with their good, soldier slots (filled = bowmen on top), mini
    flag, worker route, animated worker, anchor.
  * Tools: Select (drag dots; cells snap to map nodes, pixel data moves by pixels), Paint and Erase
    (cells of building area, blocked or dig), Add (piles, builders, soldier slots; door, flag, work
    spot and mini flag move to the click), Set anchor (the clicked pixel of your frame becomes the
    anchor; dragging or nudging your image does the same). Anchor changes shift all frames unless
    "change this frame only" is on.
  * On the finished image the building's animation parts play at its top-left as the game draws
    them, continuously (the routine's slot triggers do not drive them yet), and the house worker walks
    its routine with the original settler animations. The timeline shows the steps coloured by task
    with trigger and sound marks; clicking it or a row jumps there.
* **Replacing frames.** Drop images on the window or use "Add frames…". Files are sorted by name and
  saved as frames `000…`; files with `team` or `mask` in the name become masks; other image formats
  (JPEG, GIF, BMP, WebP, TIFF, TGA) are converted to PNG. While paused, one image replaces only the
  shown frame. A drop asks which sequence it is for when the slot has more than one (the window system
  does not say where in the window the files landed). Dropping `n × m` frames offers to set the
  sequence to `m` frames per original frame. Linux on Wayland has no file drop: use the buttons.
* **Sounds.** Original and yours play in the app. Audio files for a sound variant are stored as WAV
  (other formats are decoded to 16-bit PCM at their own sample rate and channel count; MP3, OGG,
  FLAC, AAC / M4A and AIFF are read); music is stored as given (MP3, OGG, WAV). One track plays at a
  time.
* **Validation**, from the PNG headers and the settings files: frame count against the original ×
  multiplier (an error in "keep frame counts" groups), exact sizes, team mask present where the
  original has team colour and of the right size, canvas size without an anchor, valid `seq.json`.
* **Export** (header button). See below.

### Keys

| Key | Action |
| --- | --- |
| Space | play / pause |
| ← → | step one frame of the finest sequence (with nothing selected) |
| J / K | next / previous slot |
| R | reload the pack |
| / | search |
| Ctrl+S | save the editor's changes |

On the Edit tab also:

| Key | Action |
| --- | --- |
| V, B, E, A, N | Select, Paint, Erase, Add, Set anchor |
| Del | delete the selection |
| arrows | nudge the selection (Shift ×10) |
| Ctrl+Z, Ctrl+Y | undo, redo |
| Esc | deselect |

## Known limits

* Linux under Wayland has no file drop: use the "Add frames…" and "Choose file…" buttons.
* One audio track plays at a time.
* AVIF and SVG images and Opus audio cannot be read.

## The pack

```text
pack/
  pack.json                                    {"format": 1, "name": "...", "scale": 1,
                                                "frame_multiplier": 1}
  gfx/<group>/<slot>/<seq>/000.png             RGBA frames, numbered from 000
  gfx/<group>/<slot>/<seq>/000.team.png        optional team mask (grey, same size as the frame)
  gfx/<group>/<slot>/<seq>/seq.json            optional anchor and frame multiplier
  gfx/<group>/<slot>/building.json             optional building data changes
  snd/<SOUND_ID>/<variant>.wav                 PCM WAV (variant 00, 01, ...)
  music/<track>.mp3 | .ogg | .wav
```

* **Scale.** `scale` 1 is the original pixel size, 2 twice as detailed. A replacement frame is the
  original's displayed size × scale (half-size frames: half the size × scale). The studio shows
  replacements at game size.
* **Frame multiplier.** `pack.json` `frame_multiplier` is the default for every animated sequence
  (more than one frame, not a GUI image or terrain page); `seq.json` `{"frame_multiplier": k}`
  overrides it per sequence. Stills always have 1.
* **Anchor.** Without an anchor in `seq.json` the anchor of your frame `j` is `(left, top)` × scale of
  the original frame it belongs to (`j / k`), which fits art traced on the same canvas. With a
  different canvas, give `{"anchor": [x, y]}` for all frames or `{"anchors": [[x, y], ...]}` per frame
  of yours: the pixel that sits on the map node.
* **Exact sizes.** GUI images placed by a dialog layout, raw GUI backgrounds and terrain pages must be
  exactly the original size × scale. Terrain pages must also keep the layout: the renderer samples
  fixed hexagon and transition regions of each page.
* **Team colour.** Paint team areas in grey (white = full player colour) and add a mask (255 = team
  colour). Colour = `mix(rgb, player × luma(rgb), mask)` with `luma = 0.299 r + 0.587 g + 0.114 b`.
* **Shadows** are ordinary semi-transparent pixels in the RGBA image.
* **Building data** (`building.json`): keys of the spec's `building` (`footprint`, `block`, `dig`,
  `door`, `flag`, `work`, `builders`, `piles`, `mini_flag_px`, `soldiers_px`) whose value differs from
  the original. These are gameplay data, not art: the studio marks every change.

Unknown keys in `pack.json` and `seq.json` are kept when the studio writes them. Effects files 4 and
36 are 4444-palette copies of 6 and 37: one replacement covers both. Player logos (files 1 and 35) are
built at run time from `Logo\*.png` and are not slots.

Keep the pack in version control of its own: it gets large (tens of thousands of PNGs).

### Frame counts are mechanics

The simulation reads animation lengths from the original gfx files: all settler jobs and directions,
landscape objects, effects in file 6, animals and vehicles (direction 2), building parts (Dark Tribe).
`jobInfo.xml` nodes with duration 0 last as many ticks as the animation has frames. Groups marked "keep
frame counts" keep the original count per sequence as the game's timing.

**Smoother animations: whole multiples.** A replacement may have `k` frames per original frame
(`frame_multiplier`, 1..8; the studio offers 1..4): `n × k` frames for an original of `n`. The game
keeps `n` for timing and shows original frame `i` as your frames `i·k .. i·k + k − 1`. One tick is
71 ms, so the original plays at 14.1 fps and a 2× replacement at 28.2 fps. Events stay on their
original frame: a sound on frame `f` plays on your frame `f·k`. Any other count is an error in "keep
frame counts" groups and a warning elsewhere.

## The spec

* `index.json`: format version, categories and groups with counts, frame tuple layout, map geometry,
  the pack layout and the list of original content the spec does not cover yet.
* `groups/<group>.json`: the slots of one group (`settlers-roman`, `buildings-maya`, `gui-roman`,
  `terrain`, `sounds`, `music`, ...).
* `ref/<group>/<slot>/<seq>.png`: reference strip, frames side by side (1 px gap, wrapped at 4096 px),
  8-bit indexed, team colour shown as player 2's blue, shadow as 50 % black. `<seq>.team.png` marks the
  team-colour pixels with their palette index (192..255, 0 elsewhere). With the group's `team_ramps`
  (per palette `pal`, per player the colours of indices 224..255 as `rrggbb` × 32) and `index.json`
  `player_colors`, the studio recolours the original like the game: 192..224 from the player colour's
  gradient, 224..256 from the ramp. Sounds are decoded to PCM WAV.

A **slot** is one thing to replace: a settler job, a building, an object, an animal, an effect, a GUI
image, a terrain page, a sound id, a music track. A slot has **sequences**: the 6 directions of a job,
the parts of a building (0 construction, 1 finished, 2+ animation parts and patches), growth stages of
a tree, amounts of a pile, the variants of a sound. A gfx sequence lists its frames as
`[image, w, h, left, top, flags, sx, sy]`: size, anchor (the frame's top-left corner is drawn at
`anchor - (left, top)`), flags (1 team colour, 2 shadow, 4 drawn at half size, 8 opaque exact-size
image) and position in the reference strip.

Extra data per slot, where it exists:

* settler jobs: owner, the `jobInfo.xml` lists that play the job, the lists whose node lasts as many
  ticks as the animation has frames (`length_from_frames`), the sound played on a frame;
* buildings (cell offsets from the anchor node): building area, blocked and dug cells, door, flag, work
  spot, builder spots, piles, soldier slots, mini flag (pixels), per part the patch (type, slot, ticks,
  sound on frame), and `work_lists`: the house worker's routine step by step with cell, pixel position,
  task, animation, direction, good, trigger, sound, start tick and length. Unset work spots and mini
  flags are `null`. Pile `px` does not match the pile's cell and nothing uses it: its meaning is
  unverified;
* GUI images: every dialog control or background that places the image, whose rectangle it must fill
  exactly;
* sounds: variants (rate, channels, length), volume, and the data-driven uses;
* music: the playlists that use the track.

Map geometry: a cell offset `(dx, dy)` from a sprite's anchor node lies at `(24·dx − 12·dy, 12·dy)` px
at zoom 1.

## Export

A zip of this slot, a group, every slot with files in the pack, or all slots (the complete data set,
images where you have them). Saved data only: unsaved editor changes are not included. Contents, each
optional:

* your frames and team masks;
* sprite sheets per sequence (pages of at most 4096 × 8192) with an atlas in the TexturePacker "JSON
  hash" layout (pivot = anchor, plus `anchor` in pixels; the masks as a second sheet in the same layout);
* `data.json` per slot, format `s4studio-export/1`: per sequence the frame multiplier, frames, ticks,
  fps, frame time, events at your frame numbers, every frame's file, size, expected size, anchor and
  the original frame it stands for; building cells and pixel positions, worker routines, parts, job
  lists;
* `coords.csv`, `routines.csv`, `frames.csv`;
* your sounds and music;
* opt-in, the original frames as PNG (one per original frame at its own size, team pixels in the
  selected player's colours, in separate `reference/<seq>/` folders). These are for your own reference:
  not for the pack, for publishing or for artists to trace. The warning is in the dialog, in
  `index.json` and in `README.txt`.

Sequences can be chosen: per checkbox for one slot, by key for more (buildings 0 construction, 1
finished, 2+ parts; units 0–5 directions). `index.json` holds the scope, pack settings and conventions,
`README.txt` the layout. Original game sounds are never exported. The zip is stored, without zip64: at
most 65535 entries and 4 GB, beyond that export a group at a time.

## Tests

`cargo test --release` needs no game data and no GPU. Three tests do more when asked:

| Variable | Effect |
| --- | --- |
| `S4_SHOTS_DIR=dir` | renders every screen to PNGs through wgpu |
| `S4_SHOT_SPEC=dir` | with `S4_DIR`: uses a real spec. The shots then hold the game's art: keep them out of the repository |
| `S4_AUDIO_TEST=file` | plays a file on the real audio device |

## Third-party fonts

Inter and Phosphor Icons, see `assets/fonts/README.md`.
