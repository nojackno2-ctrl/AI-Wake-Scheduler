; AI 倒數喚醒 (AI Wake Scheduler) Inno Setup 6 腳本
#define MyAppName "AI 倒數喚醒"
#define MyAppVersion "1.10.0"
#define MyAppPublisher "AI Wake Scheduler"
#define MyAppURL "https://github.com/nojackno2-ctrl/AI-Wake-Scheduler"
#define MyAppExeName "AI倒數喚醒.exe"
#define MyAppUserModelId "nojackno2.AIWakeScheduler"
#ifdef VerificationBuild
  #undef MyAppName
  #define MyAppName "AI 倒數喚醒 QA"
  #undef MyAppUserModelId
  #define MyAppUserModelId "nojackno2.AIWakeScheduler.QA"
  #define MyAppId "{{59A78022-7C7F-4CEE-B6B2-9B6D5588A57B}"
  #define StartupValueName "AI倒數喚醒-QA"
  #define LegacyTaskName "AI倒數喚醒-QA"
  #define StartupArguments "--minimized --verify-ui"
  #define ShortcutArguments "--verify-ui"
#else
  #define MyAppId "{{5B2F4E2D-31C4-4CA5-87A9-6E2BB049DFE7}"
  #define StartupValueName "AI倒數喚醒"
  #define LegacyTaskName "AI倒數喚醒"
  #define StartupArguments "--minimized"
  #define ShortcutArguments ""
#endif
#ifndef PublishDir
  #define PublishDir "..\bin\publish-selfcontained"
#endif

[Setup]
; 應用程式全域唯一識別碼
AppId={#MyAppId}
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
Source: "{#PublishDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "..\assets\app.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Parameters: "{#ShortcutArguments}"; WorkingDir: "{app}"; AppUserModelID: "{#MyAppUserModelId}"
Name: "{group}\{cm:UninstallProgram,{#MyAppName}}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Parameters: "{#ShortcutArguments}"; WorkingDir: "{app}"; AppUserModelID: "{#MyAppUserModelId}"; Tasks: desktopicon

[Registry]
; 與一般使用者權限主程式一致，啟動項目只寫入目前使用者。
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueName: "{#StartupValueName}"; ValueType: string; ValueData: """{app}\{#MyAppExeName}"" {#StartupArguments}"; Flags: uninsdeletevalue; Tasks: startupicon
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueName: "{#StartupValueName}"; ValueType: none; Flags: deletevalue; Check: not WizardIsTaskSelected('startupicon')

[Run]
; 安裝器有權限時清理舊版最高權限工作，避免與 Run key 重複啟動。
Filename: "{sys}\schtasks.exe"; Parameters: "/Delete /TN ""{#LegacyTaskName}"" /F"; Flags: runhidden
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent runasoriginaluser

[UninstallRun]
Filename: "{sys}\schtasks.exe"; Parameters: "/Delete /TN ""{#LegacyTaskName}"" /F"; Flags: runhidden

[UninstallDelete]
Type: files; Name: "{app}\*.log"

[Code]
procedure InitializeWizard;
var
  StartupCommand: String;
  ExitCode: Integer;
begin
  { Preserve startup selected in the app or an older installer before removing the legacy task. }
  if RegQueryStringValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', '{#StartupValueName}', StartupCommand) then
    WizardSelectTasks('startupicon')
  else if Exec(ExpandConstant('{sys}\schtasks.exe'), '/Query /TN "{#LegacyTaskName}"', '', SW_HIDE, ewWaitUntilTerminated, ExitCode) and (ExitCode = 0) then
    WizardSelectTasks('startupicon');
end;
