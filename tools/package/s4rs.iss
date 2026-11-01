; S4RS installer (Inno Setup 6). Built by tools\package\windows.ps1, which passes
; /DAppVersion=<crates\s4-game version> /DExe=<built s4-game.exe> /DOutDir=<target\package>.
; Per user, no admin: {localappdata}\Programs\S4RS. Only our own files: the game data come from
; the player's own The Settlers IV History Edition.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef Exe
  #define Exe "..\..\target\package-build\release\s4-game.exe"
#endif
#ifndef OutDir
  #define OutDir "..\..\target\package"
#endif

[Setup]
AppId={{33EA8DA1-BCD9-4DCF-84D0-B6B7E4A7F5A6}
AppName=S4RS
AppVersion={#AppVersion}
AppVerName=S4RS {#AppVersion}
AppPublisher=S4RS
DefaultDirName={localappdata}\Programs\S4RS
DefaultGroupName=S4RS
DisableProgramGroupPage=yes
DisableDirPage=auto
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=commandline
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir={#OutDir}
OutputBaseFilename=S4RS-Setup-{#AppVersion}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
UninstallDisplayIcon={app}\S4RS.exe
UninstallDisplayName=S4RS
InfoBeforeFile=README.txt

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#Exe}"; DestDir: "{app}"; DestName: "S4RS.exe"; Flags: ignoreversion
Source: "README.txt"; DestDir: "{app}"; Flags: ignoreversion isreadme

[Icons]
Name: "{group}\S4RS"; Filename: "{app}\S4RS.exe"; WorkingDir: "{app}"
Name: "{group}\S4RS README"; Filename: "{app}\README.txt"
Name: "{group}\S4RS logs"; Filename: "{userappdata}\s4rs\logs"
Name: "{group}\{cm:UninstallProgram,S4RS}"; Filename: "{uninstallexe}"
Name: "{userdesktop}\S4RS"; Filename: "{app}\S4RS.exe"; WorkingDir: "{app}"; Tasks: desktopicon

[Run]
Filename: "{app}\S4RS.exe"; Description: "{cm:LaunchProgram,S4RS}"; WorkingDir: "{app}"; Flags: nowait postinstall skipifsilent
