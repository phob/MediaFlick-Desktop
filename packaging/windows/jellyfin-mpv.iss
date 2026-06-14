#define MyAppName "Jellyfin MPV"
#define MyAppPublisher "Jellyfin"
#ifndef MyAppVersion
#define MyAppVersion "0.1.0"
#endif
#ifndef SourceDir
#define SourceDir "..\..\dist\JellyfinMPV"
#endif

[Setup]
AppId={{8F99048B-150C-47A1-988D-1E5E84F92E46}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
DefaultDirName={localappdata}\Programs\Jellyfin MPV
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
OutputDir=..\..\dist\installer
OutputBaseFilename=JellyfinMPV-Setup-{#MyAppVersion}
SetupIconFile=..\..\resources\win\jellyfin.ico
UninstallDisplayIcon={app}\jellyfin-mpv.exe
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; GroupDescription: "Additional shortcuts:"; Flags: unchecked

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\{#MyAppName}"; Filename: "{app}\jellyfin-mpv.exe"; WorkingDir: "{app}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\jellyfin-mpv.exe"; WorkingDir: "{app}"; Tasks: desktopicon

[Run]
Filename: "{app}\jellyfin-mpv.exe"; Description: "Launch {#MyAppName}"; Flags: nowait postinstall skipifsilent
