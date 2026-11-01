S4RS for Linux
==============

S4RS is a rewrite of The Settlers IV (History Edition) in Rust. This folder contains only
the S4RS programs. The game data come from YOUR OWN copy of the original game.


Requirements
------------
- 64-bit Linux (x86_64) with glibc 2.36 or newer (Debian 12, Ubuntu 22.10, Fedora 37 or
  later).
- The Settlers IV - History Edition (Ubisoft Connect), installed through Wine, Proton, Lutris
  or Bottles, or a copy of its game folder (for example from a Windows PC).
- About 1 GB free disk space next to this folder's programs (for the copied game data).
- A graphics driver with Vulkan: Mesa (RADV for AMD, ANV for Intel) or NVIDIA's driver,
  and the Vulkan loader (Debian / Ubuntu: mesa-vulkan-drivers libvulkan1).
- Sound: ALSA (libasound2). PulseAudio and PipeWire work through their ALSA plugin
  (pipewire-alsa or the PulseAudio ALSA plugin, installed on most desktops).
- An X11 or Wayland desktop.
- Fonts for the game's texts: Liberation Sans (fonts-liberation) or Microsoft's Arial
  (ttf-mscorefonts-installer).
- Optional: zenity or kdialog, for the folder dialog, the progress window and error
  messages. Without them everything is written to the terminal.


How to start
------------
Unpack the archive anywhere and run ./S4RS (from a terminal, or by double-clicking it).

The first start copies the game data S4RS needs (about 800 MB: graphics, sounds, maps,
texts) from your game into the "data" folder here. S4RS looks for the game in the usual
Wine prefixes (WINEPREFIX, ~/.wine, Lutris' ~/Games/*, Bottles, Steam's Proton prefixes);
if it cannot find it, it asks for the game's folder: choose the folder that contains
S4_Main.exe. You can also give it directly:

    S4_DIR="/path/to/thesettlers4" ./S4RS

The copy takes a minute; you can cancel it. Your game is only read, never changed. Later
starts skip the copy.

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
The game's videos (logos, intro, campaign cut scenes) are converted from your installed game
the first time S4RS starts: a few minutes in the background, the logos at the start are ready
within seconds. That needs ffmpeg with an AV1 encoder (Debian / Ubuntu: sudo apt install ffmpeg;
Fedora: ffmpeg from RPM Fusion), or an ffmpeg binary next to s4-game. Without it S4RS plays
without videos. The converted videos go to
user/video (about 260 MB). show_videos = false in user/settings.toml turns them off.


Internet games
--------------
Internet games go through the S4RS server set4.online, port 7720 (the default; no setup
needed). LAN games find each other on the local network.


Reporting a bug
---------------
Please send:
1. what you did and what went wrong (and what you expected instead);
2. the log file of that day from user/logs;
3. if you can, a savegame from just before the problem (user/saves);
4. your distribution and graphics card.


Removing it
-----------
Delete the folder. Nothing is installed elsewhere.
