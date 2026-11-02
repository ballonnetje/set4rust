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

- The game for Windows, Linux and macOS, and the Asset Studio as a native desktop app for all three.
- Optional cloud shadows drifting with the wind: press F5 in a game. They are off by default and change nothing in the simulation.

## Good to know

- The programs are not signed. Windows SmartScreen may warn; on macOS run `xattr -dr com.apple.quarantine .` in the unpacked folder first.
- The macOS builds are compiled automatically and have not been run on a real Mac yet.
- The Linux builds need glibc 2.36 or newer.
- The Asset Studio is new as a native app. It has been tested with automated input on Windows, not yet by hand, and only compiled for Linux and macOS.
- The Asset Studio prepares replacement packs. The game does not load them yet.
