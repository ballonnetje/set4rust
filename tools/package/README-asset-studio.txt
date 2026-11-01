S4RS Asset Studio
=================

A desktop program to browse every sprite, sound, GUI image and music track of The Settlers IV,
compare the original with your replacement, validate it and edit building data.


How to start
------------
Windows:  double-click s4-asset-studio.exe.
Linux:    ./s4-asset-studio
macOS:    once after unpacking, in Terminal: xattr -dr com.apple.quarantine .
          then: ./s4-asset-studio

The Asset Studio finds an installed The Settlers IV History Edition by itself. If it finds none,
it asks for the game's folder: choose the folder that contains S4_Main.exe. Your choice is
remembered. It needs your own copy of the game; none of the game's files come with this program.

The first start reads your game and writes the asset spec (a few seconds, about 300 MB).


Where things are
----------------
Windows:  %APPDATA%\s4rs-asset-studio
Linux:    ~/.local/share/s4rs-asset-studio
macOS:    ~/Library/Application Support/s4rs-asset-studio

- assetspec/  what a replacement must match, and renders of the original art and sound. These
              are the game's own pictures and sounds: for your eyes only, never share them.
- pack/       your replacements.

The game does not load packs yet: for now the studio prepares them.


Options
-------
--game DIR         the game's folder
--data DIR         use another folder for the spec and the pack
--export-spec DIR  write the asset spec to DIR and stop
