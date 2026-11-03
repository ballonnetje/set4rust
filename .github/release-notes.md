S4RS is a rewrite of The Settlers IV (History Edition) in Rust, for Windows, Linux and macOS.

**You need your own copy of The Settlers IV - History Edition.** These downloads contain none of the game's files. See the [README](https://github.com/ballonnetje/set4rust#readme) for how to install and play.

## Downloads

| System | The game | Asset Studio |
| --- | --- | --- |
| Windows x64 | `S4RS-Setup-*.exe` (installer) or `S4RS-Windows-x64-*.zip` (portable) | `S4RS-Asset-Studio-Windows-x64-*.zip` |
| Linux x64 | `S4RS-Linux-x64-*.tar.gz` | `S4RS-Asset-Studio-Linux-x64-*.tar.gz` |
| macOS (Apple silicon and Intel) | `S4RS-macOS-universal-*.tar.gz` | `S4RS-Asset-Studio-macOS-universal-*.tar.gz` |

`SHA256SUMS` lists the checksums.

## In this release

- Dark land is now drawn as the game has it. Land the Dark Tribe darkens during play turns dark on screen, so its mushroom farms no longer seem to stand on green. Land made green again, by gardeners or the priest's terrain spell, turns green on screen too.
- A defeated player stops, as in the original. A beaten computer loses its AI, and every building of a defeated player except towers and castles is switched off, so building and production end.
- Earlier: the game for Windows, Linux and macOS, the Asset Studio as a native desktop app, and optional cloud shadows (F5).

## Good to know

- The programs are not signed. Windows SmartScreen may warn; on macOS run `xattr -dr com.apple.quarantine .` in the unpacked folder first.
- The macOS builds are compiled automatically and have not been run on a real Mac yet.
- The Linux builds need glibc 2.36 or newer.
- The Asset Studio is new as a native app. It has been tested with automated input on Windows, not yet by hand, and only compiled for Linux and macOS.
- The Asset Studio prepares replacement packs. The game does not load them yet.
