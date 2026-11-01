# S4RS

A rewrite of **The Settlers IV (History Edition)** in Rust, for Windows, Linux and macOS: a
deterministic simulation, a modern GPU renderer (`wgpu`), `kira` audio, Lua mission scripts through
`mlua`, and LAN and internet play with lockstep networking.

**You need your own copy of The Settlers IV - History Edition.** This repository and its builds
contain none of the game's files. S4RS reads the graphics, sounds, maps and texts from your copy
and never changes it.

The repository has two programs, each with its own build:

| Program | What it is | Source |
| --- | --- | --- |
| **S4RS** | The game | `crates/` |
| **S4RS Asset Studio** | A desktop app to browse the game's assets and prepare replacements | `studio/` |

## Contents

- [What works](#what-works)
- [Get the game data](#get-the-game-data)
- [Install a build](#install-a-build)
  - [Windows](#windows)
  - [Linux](#linux)
  - [macOS](#macos)
- [Play](#play)
- [Asset Studio](#asset-studio)
- [Build from source](#build-from-source)
- [Develop](#develop)
- [Legal](#legal)

## What works

- The campaigns, including all add-ons, tutorials, free games and random maps.
- Computer players, magic, ships, vehicles, trade and the Dark Tribe.
- LAN games, and internet games through a relay server.
- Savegames, statistics, and every in-game menu.

All 107 campaign missions have been run to victory by automated test drivers. Those drivers add
units instead of playing a full economy, so a mission's balance for a real player is not proven by
that. S4RS has been played on Windows. The Linux build has been tested without a window; the macOS
build is compiled automatically and **has not been run on a real Mac yet**.

Saves from the original game cannot be loaded, and S4RS cannot play network games against it.

## Get the game data

S4RS needs **The Settlers IV - History Edition** from Ubisoft Connect. No other version works: the
exe must be the build with this SHA-256:

```text
S4_Main.exe  3b561269fb7ce4c281959f8f0db691cebf7cd36a04ad3594461b94290c5d3816
```

| System | How to get the folder |
| --- | --- |
| Windows | Install the game with Ubisoft Connect. S4RS finds it by itself. |
| Linux | Install it through Wine, Proton, Lutris or Bottles (found automatically), or copy the folder from a Windows PC. |
| macOS | Ubisoft Connect has no Mac version. Copy the folder from a Windows PC, or install it with CrossOver or Wine (found automatically). |

The folder is the one holding `S4_Main.exe`. On Windows that is usually
`C:\Program Files (x86)\Ubisoft\Ubisoft Game Launcher\games\thesettlers4`.

S4RS looks in this order: the `S4_DIR` environment variable, the folder you chose earlier, the
Ubisoft Connect and GOG registry entries, the usual Ubisoft folders, then Wine, Proton, Lutris,
Bottles and CrossOver prefixes. If it finds nothing, it asks you for the folder.

## Install a build

Download from the [latest release](https://github.com/ballonnetje/set4rust/releases/latest).
`SHA256SUMS` there lists the checksums.

| File | System |
| --- | --- |
| `S4RS-Setup-<version>.exe` | Windows installer |
| `S4RS-Windows-x64-<version>.zip` | Windows portable |
| `S4RS-Linux-x64-<version>.tar.gz` | Linux |
| `S4RS-macOS-universal-<version>.tar.gz` | macOS (Apple silicon and Intel) |
| `S4RS-Asset-Studio-Windows-x64-<version>.zip` | Asset Studio for Windows |
| `S4RS-Asset-Studio-Linux-x64-<version>.tar.gz` | Asset Studio for Linux |
| `S4RS-Asset-Studio-macOS-universal-<version>.tar.gz` | Asset Studio for macOS |

The portable, Linux and macOS packages copy about 800 MB of game data into a `data` folder beside
the program on the first start. The Windows installer reads from your installed game instead.

### Windows

Needs Windows 10 or 11 (64-bit) and a graphics card with Vulkan or DirectX 12.

**Installer:** run `S4RS-Setup-<version>.exe`, then start S4RS from the Start menu. No administrator
rights are needed. Windows SmartScreen may warn about an unknown publisher, because the program is
not signed: choose **More info**, then **Run anyway**.

**Portable:** unpack the zip anywhere and run `S4RS.exe`. To remove it, delete the folder.

Settings, saves and logs of the installed version are in `%APPDATA%\s4rs`. The portable version
keeps them in `user\` beside itself.

### Linux

Needs 64-bit Linux with glibc 2.36 or newer (Debian 12, Ubuntu 22.10, Fedora 37 or later), and an
X11 or Wayland desktop.

Install what the game and the Asset Studio need, on Debian or Ubuntu:

```sh
sudo apt install libasound2 libvulkan1 mesa-vulkan-drivers fonts-liberation zenity \
    libxkbcommon0 xdg-desktop-portal xdg-desktop-portal-gtk
```

Then unpack and start it:

```sh
tar -xzf S4RS-Linux-x64-*.tar.gz
cd S4RS
./S4RS
```

`zenity` or `kdialog` shows the folder dialog. Without them, name the folder yourself:

```sh
S4_DIR="/path/to/thesettlers4" ./S4RS
```

### macOS

Needs macOS 11 or newer, on Apple silicon or Intel.

The programs are not signed by Apple, so remove the quarantine flag once after unpacking:

```sh
tar -xzf S4RS-macOS-universal-*.tar.gz
cd S4RS
xattr -dr com.apple.quarantine .
./S4RS
```

Start it from Terminal. If the folder dialog does not find your copy of the game, name it yourself:

```sh
S4_DIR="/path/to/thesettlers4" ./S4RS
```

## Play

- **Videos (optional):** the game's videos are converted from your copy on the first start, which
  takes about 4 minutes in the background. That needs `ffmpeg` with an AV1 encoder:
  `winget install Gyan.FFmpeg` on Windows, `sudo apt install ffmpeg` on Debian or Ubuntu,
  `brew install ffmpeg` on macOS. Without it S4RS plays without videos.
- **Settings:** in the game's menu, or in `settings.toml`.
- **Multiplayer:** LAN games find each other on the local network. Internet games use the relay
  `set4.online:7720` by default. You can host your own, see [Develop](#develop).
- **Checking the copied data:** `S4RS --verify` checks every copied file. `S4RS --reimport` copies
  the data again.
- **Optional effects:** F5 toggles cloud shadows. F3 opens the monitor overlay.

## Asset Studio

The Asset Studio is a separate native desktop app, with its own build and its own executable. It
shows every sprite, sound, GUI image and music track of the game, compares the original with your
replacement, validates it, edits building data and exports your work.

Unpack its package and start `s4-asset-studio` (`s4-asset-studio.exe` on Windows). On macOS, first
run `xattr -dr com.apple.quarantine .` in its folder, as for the game.

It finds an installed game by itself. If there is none, it asks for the game's folder and remembers
your choice. The first start takes a few seconds and writes about 300 MB.

The Asset Studio has been tested with automated input and offscreen renders on Windows, not by
hand. On Linux and macOS it has only been compiled so far. Dropping files onto it does not work
under Wayland: use its buttons there.

`assetspec/ref/` in its data folder holds renders of the game's own art and sound. Keep them on
your own computer. The game does not load packs yet, so for now the studio prepares replacements.

Details and the pack format: [`studio/README.md`](studio/README.md).

## Build from source

Needs Rust 1.99 or later (edition 2024) from [rustup.rs](https://rustup.rs) and a C compiler for the
bundled Lua.

| System | Extra setup |
| --- | --- |
| Windows | Visual Studio Build Tools with the C++ workload |
| Debian, Ubuntu | `sudo apt install build-essential pkg-config libasound2-dev` |
| Fedora | `sudo dnf install gcc pkgconf-pkg-config alsa-lib-devel` |
| macOS | `xcode-select --install` |

**The game:**

```sh
git clone https://github.com/ballonnetje/set4rust.git
cd set4rust
cargo run --release -p s4-game
```

**A single map directly:**

```sh
cargo run --release -p s4-game -- --map Campaign/roman02
```

**The Asset Studio**, which has its own workspace:

```sh
cd studio
cargo run --release
```

**Packages:**

```sh
# Windows (needs Inno Setup 6 for the installer)
powershell -ExecutionPolicy Bypass -File tools\package\windows.ps1

# Linux, or macOS (rustup target add aarch64-apple-darwin x86_64-apple-darwin)
bash tools/package/unix.sh
```

They appear in `target/package`. Pushing a tag `v*` to GitHub builds all three systems with
`.github/workflows/build.yml` and publishes them as a GitHub Release of that tag.

## Develop

| Path | Contents |
| --- | --- |
| `crates/s4-formats` | Readers for the original files, install detection, desktop dialogs |
| `crates/s4-sim` | The deterministic simulation: settlers, economy, military, AI |
| `crates/s4-viewer` | The renderer and GUI |
| `crates/s4-game` | The game: menus, single player, campaigns, LAN and internet |
| `crates/s4-script` | Lua mission scripts |
| `crates/s4-audio`, `s4-video` | Sound, music and videos |
| `crates/s4-net`, `s4-relay` | Lockstep networking and the relay server |
| `crates/s4-mapgen` | Random maps |
| `crates/s4-portable` | The launcher that imports the game data |
| `studio/` | The Asset Studio, a separate workspace |
| `vendor/lua-src` | Lua 5.2.4 with a fixed string-hash seed, for determinism |
| `tools/package` | Packaging scripts and the packages' READMEs |

**Tests:** most tests need the game data and pass without doing anything if they don't find it, so
always set `S4_DIR`:

```sh
S4_DIR="/path/to/thesettlers4" cargo test --release --workspace --no-fail-fast
```

Comments cite the addresses of the original functions. Some also name `docs/` and `tools/re/`
files, which are the private research notes and are not in this repository.

**Your own relay server:**

```sh
cargo run --release -p s4-relay -- --bind 0.0.0.0:7720
```

Open UDP port 7720, then start the game with `--relay your.server:7720`, or set `relay_server` in
`settings.toml`.

## Legal

S4RS is an unofficial fan project. It is not affiliated with or endorsed by Ubisoft or Blue Byte.
The Settlers is a trademark of Ubisoft Entertainment.

This repository and its builds contain no graphics, sounds, music, maps, texts, videos or executables
from the game. Tables the game keeps inside `S4_Main.exe` are read from your own copy at run time.

The behaviour of the game was reconstructed by reverse engineering the shipped `S4_Main.exe`, so the
gameplay, AI and map generator code follow the original program closely, and the source contains
numeric rule tables and symbolic names taken from it. It is published for people who own the game.
