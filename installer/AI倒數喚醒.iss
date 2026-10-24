; AI 倒數喚醒 (AI Wake Scheduler) Inno Setup 6 腳本
#define MyAppName "AI 倒數喚醒"
#define MyAppVersion "2.3.0"
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
  #define PublishDir "..\bin\publish-release"
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

[InstallDelete]
; v1.x 為 .NET 自包含版，安裝目錄留有執行階段 DLL 與資源資料夾；
; v2 起只安裝單一原生執行檔與圖示，升級時清掉這些殘留（使用者資料不在安裝目錄）。
Type: files; Name: "{app}\*.dll"
Type: files; Name: "{app}\*.pdb"
Type: files; Name: "{app}\*.deps.json"
Type: files; Name: "{app}\*.runtimeconfig.json"
Type: files; Name: "{app}\createdump.exe"
Type: filesandordirs; Name: "{app}\zh-Hant"

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
var
  IsUpdate: Boolean;
  PreviousVersion: String;

{ 讀取舊版解除安裝資訊（依當時權限寫入 HKLM 或 HKCU）。 }
function FindPreviousInstall(var Version: String): Boolean;
var
  Key: String;
begin
  Key := 'Software\Microsoft\Windows\CurrentVersion\Uninstall\' + ExpandConstant('{#MyAppId}') + '_is1';
  Result := RegKeyExists(HKLM64, Key) or RegKeyExists(HKLM32, Key) or RegKeyExists(HKCU, Key);
  Version := '';
  if Result then
    if not RegQueryStringValue(HKLM64, Key, 'DisplayVersion', Version) then
      if not RegQueryStringValue(HKLM32, Key, 'DisplayVersion', Version) then
        RegQueryStringValue(HKCU, Key, 'DisplayVersion', Version);
end;

{ 依序比較以點分隔的數字版本，回傳 -1、0、1。 }
function CompareVersions(A, B: String): Integer;
var
  PartA, PartB, DotA, DotB: Integer;
begin
  Result := 0;
  while (Result = 0) and ((A <> '') or (B <> '')) do
  begin
    DotA := Pos('.', A);
    DotB := Pos('.', B);
    if DotA = 0 then
      DotA := Length(A) + 1;
    if DotB = 0 then
      DotB := Length(B) + 1;
    PartA := StrToIntDef(Copy(A, 1, DotA - 1), 0);
    PartB := StrToIntDef(Copy(B, 1, DotB - 1), 0);
    Delete(A, 1, DotA);
    Delete(B, 1, DotB);
    if PartA < PartB then
      Result := -1
    else if PartA > PartB then
      Result := 1;
  end;
end;

function InitializeSetup: Boolean;
begin
  Result := True;
  IsUpdate := FindPreviousInstall(PreviousVersion);
  if IsUpdate then
    Log('Update mode: previous version "' + PreviousVersion + '"')
  else
    Log('Fresh install mode');
  if IsUpdate and (PreviousVersion <> '') and (CompareVersions(PreviousVersion, '{#MyAppVersion}') > 0) then
    Result := SuppressibleMsgBox('已安裝較新的版本 v' + PreviousVersion + '，確定要改裝為較舊的 v{#MyAppVersion} 嗎？',
      mbConfirmation, MB_YESNO or MB_DEFBUTTON2, IDYES) = IDYES;
end;

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
  if IsUpdate then
    WizardForm.Caption := '更新 {#MyAppName}';
end;

{ 更新模式沿用舊的安裝位置與選項，略過歡迎、資料夾與工作頁，直接進入確認頁。 }
function ShouldSkipPage(PageID: Integer): Boolean;
begin
  Result := IsUpdate and ((PageID = wpWelcome) or (PageID = wpSelectDir) or (PageID = wpSelectTasks));
end;

procedure CurPageChanged(CurPageID: Integer);
begin
  if IsUpdate and (CurPageID = wpReady) then
  begin
    WizardForm.PageNameLabel.Caption := '準備更新';
    WizardForm.PageDescriptionLabel.Caption := '將保留原本的安裝位置、啟動選項與所有排程設定。';
    WizardForm.NextButton.Caption := '更新(&U)';
  end;
end;

function UpdateReadyMemo(Space, NewLine, MemoUserInfoInfo, MemoDirInfo, MemoTypeInfo,
  MemoComponentsInfo, MemoGroupInfo, MemoTasksInfo: String): String;
var
  FromVersion: String;
begin
  if IsUpdate then
  begin
    FromVersion := PreviousVersion;
    if FromVersion = '' then
      FromVersion := '舊版';
    if CompareVersions(PreviousVersion, '{#MyAppVersion}') = 0 then
      Result := '重新安裝 v{#MyAppVersion}（修復目前版本的檔案）'
    else
      Result := '更新版本：v' + FromVersion + ' → v{#MyAppVersion}';
    Result := Result + NewLine + NewLine + MemoDirInfo;
    if MemoTasksInfo <> '' then
      Result := Result + NewLine + NewLine + MemoTasksInfo;
    Result := Result + NewLine + NewLine + '執行中的程式會先自動關閉；排程與設定存放在使用者資料夾，不受影響。';
  end
  else
  begin
    Result := MemoDirInfo;
    if MemoGroupInfo <> '' then
      Result := Result + NewLine + NewLine + MemoGroupInfo;
    if MemoTasksInfo <> '' then
      Result := Result + NewLine + NewLine + MemoTasksInfo;
  end;
end;
