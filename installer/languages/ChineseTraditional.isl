; *** Inno Setup 6 繁體中文語系檔 ***
; Translation by: Inno Setup Community / Traditional Chinese (Taiwan)

[LangOptions]
LanguageName=繁體中文
LanguageID=$0404
LanguageCodePage=950
DialogFontName=Microsoft JhengHei UI
DialogFontSize=9
WelcomeFontName=Microsoft JhengHei UI
WelcomeFontSize=12
TitleFontName=Microsoft JhengHei UI
TitleFontSize=29
CopyrightFontName=Microsoft JhengHei UI
CopyrightFontSize=9

[Messages]

; *** 應用程式標題
SetupAppTitle=安裝
SetupWindowTitle=安裝 - %1
UninstallAppTitle=解除安裝
UninstallAppFullTitle=%1 解除安裝

; *** 一般訊息
ButtonBack=< 上一步(&B)
ButtonNext=下一步(&N) >
ButtonInstall=安裝(&I)
ButtonOK=確定
ButtonCancel=取消
ButtonYes=是(&Y)
ButtonYesToAll=全部皆是(&A)
ButtonNo=否(&N)
ButtonNoToAll=全部皆否(&O)
ButtonFinish=完成(&F)
ExitSetupTitle=結束安裝程式
ExitSetupMessage=安裝尚未完成。如果現在結束，程式將不會被安裝。%n%n確定要結束安裝程式嗎？
AboutSetupMenuItem=關於安裝程式(&A)...
AboutSetupTitle=關於安裝程式
AboutSetupMessage=%1 版本 %2%n%3%n%n%1 網址：%n%4
AboutSetupNote=
TranslatorNote=

; *** 精靈頁面
WizardInstalling=正在安裝
WizardInstallingSub=安裝程式正在將 %1 安裝到您的電腦中，請稍候。
WizardReady=準備安裝
WizardReadySub=安裝程式已準備好開始安裝 %1 到您的電腦中。
WizardSelectTasks=選取附加工作
WizardSelectTasksSub=您想要執行哪些附加工作？
WizardSelectTasksDesc=選取安裝 %1 時要執行的附加工作，然後按「下一步」。
WizardSelectProgramGroup=選取開始功能表資料夾
WizardSelectProgramGroupSub=安裝程式要在哪裡建立捷徑？
WizardSelectProgramGroupDesc=安裝程式將在下列開始功能表資料夾建立捷徑。
WizardSelectDir=選取目的地位置
WizardSelectDirSub=要將 %1 安裝在哪裡？
WizardSelectDirDesc=安裝程式將安裝 %1 到下列資料夾。
WizardSelectDirBrowseDesc=按「下一步」繼續。若要選擇其他資料夾，請按「瀏覽」。
WizardPreparing=正在準備安裝
WizardPreparingSub=安裝程式正在準備安裝 %1 到您的電腦中。
WizardSelectComponents=選取元件
WizardSelectComponentsSub=要安裝哪些元件？
WizardSelectComponentsDesc=選取您想要安裝的元件；清除您不想安裝的元件。按「下一步」繼續。

; *** 安裝前與權限
PrivilegesRequiredOverrideTitle=選取安裝模式
PrivilegesRequiredOverrideSub=請選取您要為誰安裝此程式。
PrivilegesRequiredOverrideText1=%1 可以為所有使用者安裝（需要系統管理員權限），或僅為您自己安裝。
PrivilegesRequiredOverrideText2=%1 可以僅為您自己安裝，或為所有使用者安裝（需要系統管理員權限）。
PrivilegesRequiredOverrideAllUsers=為所有使用者安裝(&A)
PrivilegesRequiredOverrideAllUsersRecommended=為所有使用者安裝（建議）(&A)
PrivilegesRequiredOverrideCurrentUser=僅為我安裝(&M)
PrivilegesRequiredOverrideCurrentUserRecommended=僅為我安裝（建議）(&M)

; *** 歡迎與完成
WelcomeLabel1=歡迎使用 %1 安裝精靈
WelcomeLabel2=這將會在您的電腦上安裝 %1。%n%n建議您在繼續之前先關閉所有其他應用程式。
ClickNext=按「下一步」繼續，或按「取消」結束安裝。
FinishedHeadingLabel=%1 安裝精靈完成
FinishedLabelNoIcons=%1 已安裝在您的電腦上。
FinishedLabel=%1 已安裝在您的電腦上。您可以透過已建立的捷徑啟動應用程式。
ClickFinish=按「完成」結束安裝精靈。

; *** 磁碟空間與目錄確認
DiskSpaceGB=至少需要 %1 GB 的可用磁碟空間。
DiskSpaceMB=至少需要 %1 MB 的可用磁碟空間。
DirNameTooLong=資料夾名稱或路徑太長。
InvalidDirName=資料夾名稱無效。
BadDirNameSubdir=資料夾名稱不能包含以下字元：%n%n%1
DirExists=資料夾：%n%n%1%n%n已經存在。您仍要安裝到此資料夾嗎？
DirDoesntExist=資料夾：%n%n%1%n%n不存在。您要建立該資料夾嗎？
DiskSpaceWarningTitle=磁碟空間不足
DiskSpaceWarning=安裝程式至少需要 %1 KB 的可用磁碟空間，但所選磁碟機只有 %2 KB 可用。%n%n您仍要繼續嗎？
DirNameMacroContradiction=目的地資料夾與安裝模式衝突。

; *** 檔案操作
StatusCreateDirs=正在建立資料夾...
StatusExtractFiles=正在解壓縮檔案...
StatusCreateIcons=正在建立捷徑...
StatusCreateIniEntries=正在建立 INI 項目...
StatusCreateRegistryEntries=正在建立登錄項目...
StatusRegisterFiles=正在註冊檔案...
StatusSavingVersionCreation=正在儲存版本資訊...
StatusRunProgram=正在完成安裝...
StatusClosingApplications=正在關閉正在執行的應用程式...
ApplicationsFound=下列應用程式正在使用需要更新的檔案。建議您允許安裝程式自動關閉這些應用程式。
ApplicationsFound2=下列應用程式正在使用需要更新的檔案。建議您允許安裝程式自動關閉這些應用程式。安裝完成後，安裝程式將嘗試重新啟動這些應用程式。
CloseApplications=自動關閉應用程式(&A)
DontCloseApplications=不要關閉應用程式(&D)
ErrorCloseApplications=安裝程式無法自動關閉所有應用程式。建議您在繼續之前先手動關閉使用這些檔案的所有應用程式。

; *** 解除安裝訊息
ConfirmUninstall=確定要完全移除 %1 及其所有元件嗎？
UninstallStatusLabel=正在從您的電腦中移除 %1，請稍候。
UninstalledAll=%1 已成功從您的電腦中移除。
UninstalledMost=%1 解除安裝完成。%n%n部分元件無法移除，您可以手動刪除。
UninstalledAndNeedsRestart=若要完成 %1 的解除安裝，必須重新啟動電腦。%n%n您要現在重新啟動嗎？
UninstallDataQuestion=是否同時清除本機排程資料與執行日誌？

[CustomMessages]
CreateDesktopIcon=建立桌面捷徑(&D)
AdditionalIcons=附加捷徑:
LaunchProgram=立即啟動 %1
