S4RS for macOS
==============

S4RS is a rewrite of The Settlers IV (History Edition) in Rust. This folder contains only
the S4RS programs. The game data come from YOUR OWN copy of the original game.

These builds are compiled automatically and have not been played on a real Mac yet. Please
report what happens.


Requirements
------------
- macOS 11 or newer, Apple silicon or Intel (the programs are universal).
- A copy of the game folder of The Settlers IV - History Edition. Ubisoft Connect has no Mac
  version: copy the folder from a Windows PC, or install the game with CrossOver or Wine.
- About 1 GB free disk space next to this folder's programs (for the copied game data).


How to start
------------
The programs are not signed by Apple. After unpacking, once, in Terminal:

    cd /path/to/S4RS
    xattr -dr com.apple.quarantine .

Then start it from Terminal:

    ./S4RS

The first start copies the game data S4RS needs (about 800 MB: graphics, sounds, maps,
texts) into the "data" folder here. S4RS looks in CrossOver bottles and Wine prefixes; if it
cannot find the game, it asks for the game's folder: choose the folder that contains
S4_Main.exe. You can also give it directly:

    S4_DIR="/path/to/thesettlers4" ./S4RS

Your game is only read, never changed. Later starts skip the copy.

- ./S4RS --verify    checks every copied file (SHA-256) before starting
- ./S4RS --reimport  copies the game data again


Where things are (all inside this folder)
-----------------------------------------
- data/   the copied game data (data/manifest.txt lists every file)
- user/   settings.toml, saves/, logs/ (one log file per day, the last 10 are kept)


Asset Studio
------------
The Asset Studio, to replace the game's graphics and sounds with your own, is a separate
download: S4RS-Asset-Studio-*, next to this one.


Videos
------
The game's videos are converted from your copy of the game the first time S4RS starts. That
needs ffmpeg with an AV1 encoder: brew install ffmpeg. Without it S4RS plays without videos.
The converted videos go to user/video (about 260 MB). show_videos = false in
user/settings.toml turns them off.


Internet games
--------------
Internet games go through the S4RS server set4.online, port 7720 (the default; no setup
needed). LAN games find each other on the local network.


Removing it
-----------
Delete the folder. Nothing is installed elsewhere.
