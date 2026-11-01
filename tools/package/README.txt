S4RS
====

S4RS is a rewrite of The Settlers IV (History Edition) in Rust. It plays with the game data
of YOUR OWN copy of the original game; this installer contains only the S4RS programs.


Requirements
------------
- Windows 10 or 11, 64-bit.
- The Settlers IV - History Edition, installed (Ubisoft Connect). S4RS reads its data files
  (graphics, sounds, maps, texts) and never changes them.
- A graphics card with Vulkan or DirectX 12.


First start
-----------
S4RS looks for the game by itself: the Ubisoft Connect install entries and the usual
Ubisoft folder. If it cannot find it, it asks you for the game's folder: choose the folder
that contains S4_Main.exe (for example
C:\Program Files (x86)\Ubisoft\Ubisoft Game Launcher\games\thesettlers4).
Your choice is remembered. Advanced: the environment variable S4_DIR overrides it.


Where things are
----------------
- Program:   %LOCALAPPDATA%\Programs\S4RS
- Settings:  %APPDATA%\s4rs\settings.toml
- Saves:     %APPDATA%\s4rs\saves
- Logs:      %APPDATA%\s4rs\logs   (one file per day, the last 10 are kept)
(Paste %APPDATA%\s4rs into the Explorer address bar to open the folder.)


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
then start S4RS again. Without ffmpeg S4RS plays without videos. The converted videos go to
%APPDATA%\s4rs\video (about 260 MB). show_videos = false in settings.toml turns them off.


Internet games
--------------
Internet games go through the S4RS server set4.online, port 7720 (the default; no setup
needed). LAN games find each other on the local network.


Reporting a bug
---------------
Please send:
1. what you did and what went wrong (and what you expected instead);
2. the log file of that day from %APPDATA%\s4rs\logs (S4RS shows the file's name when it
   crashes);
3. if you can, a savegame from just before the problem (%APPDATA%\s4rs\saves).


Uninstalling
------------
Windows Settings > Apps > S4RS, or "Uninstall S4RS" in the Start menu. Your saves, settings
and logs in %APPDATA%\s4rs stay; delete that folder to remove them too.
