; AI 倒數喚醒 (AI Wake Scheduler) Inno Setup 6 腳本
#define MyAppName "AI 倒數喚醒"
#define MyAppVersion "1.2.0"
#define MyAppPublisher "AI Wake Scheduler"
#define MyAppURL "https://github.com/nojackno2-ctrl/AI-Wake-Scheduler"
#define MyAppExeName "AI倒數喚醒.exe"
#define MyAppUserModelId "nojackno2.AIWakeScheduler"

[Setup]
; 應用程式全域唯一識別碼
AppId={{5B2F4E2D-31C4-4CA5-87A9-6E2BB049DFE7}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} v{#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}/issues
AppUpdatesURL={#MyAppURL}/releases
AppReadmeFile={app}\README.md
VersionInfoVersion={#MyAppVersion}.0
VersionInfoCompany={#MyAppPublisher}
VersionInfoDescription=AI 倒數喚醒 安裝程式
VersionInfoTextVersion={#MyAppVersion}
VersionInfoProductVersion={#MyAppVersion}.0
VersionInfoProductName={#MyAppName}
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
AllowNoIcons=no
OutputDir=..\dist
OutputBaseFilename=AI倒數喚醒_Setup_v{#MyAppVersion}_x64
SetupIconFile=..\assets\app.ico
UninstallDisplayIcon={app}\{#MyAppExeName}
UninstallDisplayName={#MyAppName} v{#MyAppVersion}
Uninstallable=yes
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
DisableWelcomePage=no
DisableDirPage=no
DisableProgramGroupPage=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=admin
PrivilegesRequiredOverridesAllowed=commandline dialog
UsePreviousAppDir=yes
UsePreviousGroup=yes
UsePreviousTasks=yes
CreateUninstallRegKey=yes
SetupLogging=yes
UninstallLogging=yes
CloseApplications=yes
CloseApplicationsFilter={#MyAppExeName}
RestartApplications=no

[Languages]
Name: "chinesetraditional"; MessagesFile: "languages\ChineseTraditional.isl"
Name: "en"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "startupicon"; Description: "登入 Windows 時自動在背景啟動 (常駐系統匣)"; GroupDescription: "系統啟動選項:"; Flags: unchecked

[Files]
Source: "..\bin\publish-selfcontained\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "..\assets\app.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; WorkingDir: "{app}"; AppUserModelID: "{#MyAppUserModelId}"
Name: "{group}\{cm:UninstallProgram,{#MyAppName}}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; WorkingDir: "{app}"; AppUserModelID: "{#MyAppUserModelId}"; Tasks: desktopicon
Name: "{userstartup}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Parameters: "--minimized"; Tasks: startupicon

[Registry]
; 程式內建「開機時自動啟動」開關會自行寫入此機碼(StartupManager.cs),
; 安裝程式本身不建立它，僅在解除安裝時一併清除，避免殘留指向已移除路徑的啟動項。
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueName: "AI倒數喚醒"; ValueType: none; Flags: uninsdeletevalue

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
Type: files; Name: "{app}\*.log"
