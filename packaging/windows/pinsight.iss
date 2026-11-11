; PinSight — Windows installer (Inno Setup 6)
;
;   iscc packaging\windows\pinsight.iss
;
; Produces an installer that lets the user pick the install directory, plus an
; optional "portable" flavour built by scripts/package.ps1 (a plain zip with a
; pinsight.portable marker, no registry or %APPDATA% writes).

#define AppName "PinSight"
; Overridable from the command line: iscc /DAppVersion=1.2.3 ...
#ifndef AppVersion
  #define AppVersion "0.1.3"
#endif
#define AppPublisher "PinSight contributors"
#define AppURL "https://github.com/yiqiui/PinSight"

; Which exe to bundle. The default is the plain release binary; `package.ps1
; -Upx` overrides it with the staged, UPX-compressed copy via
;   iscc /DPinsightExe="..\..\dist\portable\PinSight\pinsight.exe" ...
#ifndef PinsightExe
  #define PinsightExe "..\..\target\release\pinsight.exe"
#endif

[Setup]
AppId={{8B7A2C40-1E5A-4C1B-9E4F-PINSIGHT01}
AppName={#AppName}
AppVersion={#AppVersion}
VersionInfoVersion={#AppVersion}
AppPublisher={#AppPublisher}
AppPublisherURL={#AppURL}
AppSupportURL={#AppURL}/issues
AppUpdatesURL={#AppURL}/releases
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
; The user chooses the directory; the wizard only asks for it.
DisableDirPage=no
DisableReadyPage=no
AllowNoIcons=yes
OutputDir=..\..\dist
OutputBaseFilename=PinSight-{#AppVersion}-setup
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog commandline
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
UninstallDisplayName={#AppName}

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
; The Simplified Chinese translation now ships *in this repo*
; (packaging/windows/Languages/ChineseSimplified.isl), so both local builds
; and the GitHub CI runner compile a bilingual installer instead of degrading
; to English-only. Inno Setup auto-selects the language matching the user's
; system UI locale at startup, and the [Code] section below skips the manual
; language page so a Chinese Windows installs in Chinese with no extra click.
; Pass /DNoChinese only if the file is somehow absent.
#ifndef NoChinese
Name: "zh"; MessagesFile: "Languages\ChineseSimplified.isl"
#endif

[CustomMessages]
en.AssocCapture=Capture the current screen
#ifndef NoChinese
zh.AssocCapture=截取当前屏幕
#endif

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "startup"; Description: "{cm:AutoStartProgram,{#AppName}}"; GroupDescription: "{cm:AutoStartProgramGroupDescription}"; Flags: unchecked

[Files]
Source: "{#PinsightExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\README.md"; DestDir: "{app}"; Flags: ignoreversion isreadme
Source: "..\..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\NOTICE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\config.example.json"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\assets\icon.png"; DestDir: "{app}\assets"; Flags: ignoreversion
Source: "..\..\assets\icon.ico"; DestDir: "{app}\assets"; Flags: ignoreversion

[Icons]
; The .ico is what Explorer and Add/Remove Programs use for the entry, and it
; ships next to the exe so a portable copy keeps the same look.
Name: "{group}\{#AppName}"; Filename: "{app}\pinsight.exe"; IconFilename: "{app}\assets\icon.ico"
Name: "{group}\{cm:UninstallProgram,{#AppName}}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\pinsight.exe"; IconFilename: "{app}\assets\icon.ico"; Tasks: desktopicon

[Registry]
; Optional "launch on logon", off by default.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; \
    ValueType: string; ValueName: "PinSight"; ValueData: """{app}\pinsight.exe"""; \
    Flags: uninsdeletevalue; Tasks: startup

[Run]
Filename: "{app}\pinsight.exe"; Description: "{cm:LaunchProgram,{#StringChange(AppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent

[Code]
// Skip the language-selection page. Inno Setup has already chosen the language
// that matches the OS UI locale (e.g. Chinese on a zh-CN machine), so we proceed
// with it automatically instead of asking the user to pick.
function ShouldSkipPage(PageID: Integer): Boolean;
begin
  Result := (PageID = wpSelectLanguage);
end;
