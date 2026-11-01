S4RS portable
=============

S4RS is a rewrite of The Settlers IV (History Edition) in Rust. This folder contains only
the S4RS programs. The game data come from YOUR OWN copy of the original game.


Requirements
------------
- Windows 10 or 11, 64-bit.
- The Settlers IV - History Edition, installed (Ubisoft Connect).
- About 1 GB free disk space next to this folder's programs (for the copied game data).
- A graphics card with Vulkan or DirectX 12.


How to start
------------
Unpack the zip anywhere (a USB stick works too) and run S4RS.exe.

The first start copies the game data S4RS needs (about 800 MB: graphics, sounds, maps,
texts) from your installed game into the "data" folder here. S4RS finds the game by itself
(Ubisoft Connect); if it cannot, it asks for the game's folder: choose the folder that
contains S4_Main.exe. This takes a minute; you can cancel it. Your installed game is only
read, never changed. Later starts skip the copy.

- S4RS.exe --verify    checks every copied file (SHA-256) before starting
- S4RS.exe --reimport  copies the game data again


Where things are (all inside this folder)
-----------------------------------------
- data\   the copied game data (data\manifest.txt lists every file)
- user\   settings.toml, saves\, logs\ (one log file per day, the last 10 are kept)


Asset Studio
------------
The Asset Studio, to replace the game's graphics and sounds with your own, is a separate
download: S4RS-Asset-Studio-*, next to this one.


Videos
------
The game's videos (logos, intro, campaign cut scenes) are converted from your installed game
the first time S4RS starts: about 4 minutes in the background, the logos at the start are
ready within seconds. That needs ffmpeg (free): open a command prompt and run
    winget install Gyan.FFmpeg
or put ffmpeg.exe next to s4-game.exe, then start S4RS again. Without ffmpeg S4RS plays
without videos. The converted videos go to user\video (about 260 MB). show_videos = false in
user\settings.toml turns them off.


Internet games
--------------
Internet games go through the S4RS server set4.online, port 7720 (the default; no setup
needed). LAN games find each other on the local network.


Reporting a bug
---------------
Please send:
1. what you did and what went wrong (and what you expected instead);
2. the log file of that day from user\logs;
3. if you can, a savegame from just before the problem (user\saves).


Removing it
-----------
Delete the folder. Nothing is installed elsewhere.
