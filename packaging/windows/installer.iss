; Inno Setup script for the Cache Deck installer (https://jrsoftware.org/isinfo.php).
;
; Build on Windows, from the repository root, after `cargo build --release`:
;   iscc /DAppVersion=0.1.0 packaging\windows\installer.iss
; Result: out\cache-deck-<version>-windows-x86_64-setup.exe
;
; It installs for the current user only, so it asks for no administrator password.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
; AppId identifies the app to Windows across versions. Never change it, or updates
; install next to the old version instead of replacing it.
AppId={{8046B382-25FB-44CE-9540-ED6CF0F1A18D}
AppName=Cache Deck
AppVersion={#AppVersion}
AppPublisher=Shaon Dey
AppPublisherURL=https://github.com/ShaonDey
DefaultDirName={autopf}\Cache Deck
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; Paths below are relative to the repository root.
SourceDir=..\..
OutputDir=out
OutputBaseFilename=cache-deck-{#AppVersion}-windows-x86_64-setup
SetupIconFile=assets\app-icon\icon.ico
UninstallDisplayIcon={app}\Cache Deck.exe
UninstallDisplayName=Cache Deck
Compression=lzma2
SolidCompression=yes
WizardStyle=modern

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "target\release\cache-deck.exe"; DestDir: "{app}"; DestName: "Cache Deck.exe"; Flags: ignoreversion
Source: "README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Cache Deck"; Filename: "{app}\Cache Deck.exe"
Name: "{autodesktop}\Cache Deck"; Filename: "{app}\Cache Deck.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\Cache Deck.exe"; Description: "{cm:LaunchProgram,Cache Deck}"; Flags: nowait postinstall skipifsilent
