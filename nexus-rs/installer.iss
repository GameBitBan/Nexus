; Inno Setup script for Nexus 2 (Rust) — nexus2-setup-x64.exe, built by .github/workflows/release2.yml
; from dist\Nexus (Nexus.exe + FFmpeg's DLLs + fonts, icons, skins):
;   iscc /DAppVersion=2.0.0 nexus-rs\installer.iss
; Same AppId as Nexus 1, so it installs over it: settings, library and playlists live in your data
; folder (%APPDATA%\nexus) and are shared by both versions.
#ifndef AppVersion
  #define AppVersion "2.0.0"
#endif

[Setup]
AppId={{8C1B6E1A-6F0D-4C55-9B2E-4E2A7C1D9A11}
AppName=Nexus
AppVersion={#AppVersion}
AppVerName=Nexus {#AppVersion}
AppPublisher=GameBitBan
AppPublisherURL=https://github.com/GameBitBan/Nexus
AppSupportURL=https://github.com/GameBitBan/Nexus/issues
AppUpdatesURL=https://github.com/GameBitBan/Nexus/releases
VersionInfoVersion={#AppVersion}
VersionInfoDescription=Nexus music player installer
SourceDir=..
; per-user install: no admin prompt, updates install silently
PrivilegesRequired=lowest
DefaultDirName={localappdata}\Programs\Nexus
DisableProgramGroupPage=yes
DisableDirPage=auto
OutputDir=dist
OutputBaseFilename=nexus2-setup-x64
SetupIconFile=icons\nexus.ico
UninstallDisplayIcon={app}\Nexus.exe
UninstallDisplayName=Nexus
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no
LicenseFile=LICENSE
InfoBeforeFile=PRIVACY.md
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible

[Tasks]
Name: "desktopicon"; Description: "Desktop shortcut"; GroupDescription: "Shortcuts:"

[Files]
Source: "dist\Nexus\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[InstallDelete]
; Nexus 1's program files (Python), replaced by Nexus 2 — your settings and music live elsewhere and are kept
Type: filesandordirs; Name: "{app}\_internal"
Type: files; Name: "{app}\nexus.py"
Type: files; Name: "{app}\Nexus.pyw"

[Icons]
Name: "{userprograms}\Nexus"; Filename: "{app}\Nexus.exe"; AppUserModelID: "Nexus.MusicPlayer"
Name: "{userdesktop}\Nexus"; Filename: "{app}\Nexus.exe"; Tasks: desktopicon; AppUserModelID: "Nexus.MusicPlayer"

[Run]
Filename: "{app}\Nexus.exe"; Description: "Open Nexus"; Flags: nowait postinstall skipifsilent
; in-app updates run the installer silently: open Nexus again afterwards
Filename: "{app}\Nexus.exe"; Parameters: "--restarted"; Flags: nowait; Check: WizardSilent

[UninstallRun]
Filename: "netsh"; Parameters: "advfirewall firewall delete rule name=Nexus-Soulseek"; Flags: runhidden; RunOnceId: "fw"

[Messages]
InfoBeforeLabel=What Nexus connects to (you can turn web lookups off in settings → advanced → offline mode):
