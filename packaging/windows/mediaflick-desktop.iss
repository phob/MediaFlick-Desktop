#define MyAppName "MediaFlick Desktop"
#define MyAppPublisher "MediaFlick"
#ifndef MyAppVersion
#define MyAppVersion "0.1.0"
#endif
#ifndef SourceDir
#define SourceDir "..\..\dist\MediaFlickDesktop"
#endif

[Setup]
AppId={{8DB8462A-3EBD-4B23-AC80-29A2E4445A58}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
DefaultDirName={localappdata}\Programs\MediaFlick Desktop
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
OutputDir=..\..\dist\installer
OutputBaseFilename=MediaFlickDesktop-Setup-{#MyAppVersion}
SetupIconFile=..\..\resources\win\app.ico
WizardImageFile=..\..\resources\win\installer-sidebar.bmp
WizardSmallImageFile=..\..\resources\win\installer-small.bmp
UninstallDisplayIcon={app}\mediaflick-desktop.exe
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
Name: "{autoprograms}\{#MyAppName}"; Filename: "{app}\mediaflick-desktop.exe"; WorkingDir: "{app}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\mediaflick-desktop.exe"; WorkingDir: "{app}"; Tasks: desktopicon

[Run]
Filename: "{app}\mediaflick-desktop.exe"; Description: "Launch {#MyAppName}"; Flags: nowait postinstall skipifsilent
Filename: "{app}\mediaflick-desktop.exe"; Flags: nowait skipifnotsilent; Check: AutoStartAfterUpdate

[Code]
function AutoStartAfterUpdate: Boolean;
begin
  Result := ExpandConstant('{param:MEDIAFLICKAUTOSTART|0}') = '1';
end;
