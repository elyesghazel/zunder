; zunder installer for Windows. Built by .github/workflows/release.yml:
;   iscc /DZunderVersion=0.2.1 /DSrc=<unpacked bundle> /DOut=<folder> zunder.iss
; Installs for the current user (no admin), so updates can run silently from zunder itself.

#ifndef ZunderVersion
  #define ZunderVersion "0.0.0"
#endif

[Setup]
AppId={{6E1D5C3A-8F2B-4C7E-9A51-2D4B7F0E3C91}
AppName=zunder
AppVersion={#ZunderVersion}
AppVerName=zunder {#ZunderVersion}
AppPublisher=Elyes Ghazel
AppPublisherURL=https://github.com/elyesghazel/zunder
AppSupportURL=https://github.com/elyesghazel/zunder/issues
AppUpdatesURL=https://github.com/elyesghazel/zunder/releases
DefaultDirName={localappdata}\Programs\zunder
DefaultGroupName=zunder
DisableProgramGroupPage=yes
DisableDirPage=auto
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir={#Out}
; fixed name, so https://github.com/…/releases/latest/download/zunder-windows-setup.exe always
; points at the newest version (the update notice in zunder downloads it from there)
OutputBaseFilename=zunder-windows-setup
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
SetupIconFile=..\..\assets\icons\zunder.ico
UninstallDisplayIcon={app}\bin\zunder.exe
UninstallDisplayName=zunder
LicenseFile={#Src}\LICENSE

[Tasks]
Name: "autostart"; Description: "Start zunder with Windows"
Name: "hotkey"; Description: "Open zunder with Ctrl+Alt+Space"
Name: "desktopicon"; Description: "Desktop shortcut"; Flags: unchecked

[Files]
; the debug build stays in the zip, the installer only needs the real one
Source: "{#Src}\*"; DestDir: "{app}"; Excludes: "zunder-console.exe,zunder-debug.cmd"; Flags: recursesubdirs ignoreversion

[Icons]
; Windows only honors a shortcut key on Start menu and desktop shortcuts
Name: "{autoprograms}\zunder"; Filename: "{app}\bin\zunder.exe"; HotKey: ctrl+alt+space; Tasks: hotkey
Name: "{autoprograms}\zunder"; Filename: "{app}\bin\zunder.exe"; Tasks: not hotkey
Name: "{autodesktop}\zunder"; Filename: "{app}\bin\zunder.exe"; Tasks: desktopicon
Name: "{userstartup}\zunder"; Filename: "{app}\bin\zunder.exe"; Parameters: "server"; Tasks: autostart

[Run]
Filename: "{app}\bin\zunder.exe"; Description: "Open zunder now"; Flags: nowait postinstall skipifsilent
; a silent update (from zunder's update notice) starts the daemon again in the background
Filename: "{app}\bin\zunder.exe"; Parameters: "server"; Flags: nowait runasoriginaluser; Check: WizardSilent

[UninstallRun]
Filename: "{sys}\taskkill.exe"; Parameters: "/IM zunder.exe /F"; Flags: runhidden; RunOnceId: "StopZunder"

[Code]
// the running zunder.exe would lock its files during an update
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Code: Integer;
begin
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/IM zunder.exe /F', '', SW_HIDE, ewWaitUntilTerminated, Code);
  Result := '';
end;
