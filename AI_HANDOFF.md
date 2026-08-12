# AI HANDOFF

## 2026-08-12 v1.2.0 publication and local install

- Published GitHub Release `v1.2.0` from `7fad2ef64e57bad1cb073081ca201f351e654dad`; downloaded installer SHA-256 `48204008F77A280E2192A90C8997C8838E2868F884F6D0C2905BBE93B49D4ADB` matched GitHub and checksum.
- Installed for the current user; registry reports `1.2.0`, installed EXE reports file `1.2.0.0` / product `1.2.0+7fad2ef64e57bad1cb073081ca201f351e654dad`, and the installed process remained responsive.

## 目前目標

已完成 C# Windows 桌面程式、Inno Setup 安裝包打包，並成功完成 GitHub v1.0.0 官方版本（含 Setup 安裝檔 Asset）發布；後續維持維護狀態。

## 目前狀態（2026-08-11）

- 2026-08-12 最佳化里程碑：測試入口已拆成預設 11 項 deterministic 測試（不探測本機 CLI、不要求登入）與明確 opt-in 的 `--integration`（真實 AGY/Codex/Claude 版本探測及 Codex app-server 額度讀取）。`build-installer.ps1` 預設只執行可重現測試，另以 `-RunIntegrationTests` 選用 live integration。版本升至 1.2.0，建置腳本在執行前會嚴格比對 csproj／Inno Setup／預期版本；self-contained WinForms 明確維持 `PublishTrimmed=false`，衛星語言限制為 `zh-Hant;en`。尚待建置、兩層測試、publish 與 installer 完整驗證。
- 初次驗證：Release `--no-restore` 建置成功（0 警告／0 錯誤），預設 deterministic 測試 11/11 通過。隔離帳號執行 opt-in integration 時，四 CLI 路徑／版本探測已通過，唯一失敗是 Codex app-server 回覆 `codex account authentication required to read rate limits`；這是隔離帳號沒有使用者登入狀態的預期環境邊界，需在使用者主機登入狀態重跑，不能當作產品測試失敗或整合成功。
- 使用者主機登入狀態重跑 opt-in integration 為 1/1 通過。完整 `build-installer.ps1` 在主機環境成功：腳本內 deterministic 11/11、self-contained win-x64 publish、Inno Setup 均成功，產出 `dist\AI倒數喚醒_Setup_v1.2.0_x64.exe`（48,572,179 bytes；SHA256 `2E1DDED8A5BF833F54D3182FD1236791FD65318A3E2F6C25A8343148257080D1`）。尚待發布目錄語言／版本與靜默安裝卸載稽核。
- 最終封裝稽核完成：publish 共 263 檔／152,654,324 bytes，只有 `zh-Hant` 衛星資源目錄（英語為 neutral 主資源，不另建 `en` 目錄）；`AI倒數喚醒.exe` FileVersion 為 1.2.0.0；專案明確 `PublishTrimmed=false`。在工作區專用目錄靜默安裝成功（exit 0，265 檔，安裝後 EXE 1.2.0.0），靜默卸載成功（exit 0，0 殘留）。`git diff --check` 通過。注意：尚未 commit/push/tag/release，也尚未覆蓋使用者目前正式安裝位置；這些由整合／發布階段另行執行。

- 2026-08-12 全工作區最佳化前基準複核：Release 建置成功（0 警告、0 錯誤）；沙箱內測試因隔離帳號沒有 Codex 認證而為 11/12，改在使用者主機登入狀態重跑後 12/12 全數通過。這批既有變更可作為後續最佳化前的基準提交。
- 2026-08-12 新目標：分別最佳化 AGY、Codex、Claude 的喚醒呼叫，加入排程可修改、倒數時間與剩餘流量／額度的可驗證顯示，並最佳化主程式。第一次 AGY workspace-write 子代理因無頭模式無法取得 `command` 工具權限而未產出內容；下一次將依 delegation skill 在已明確授權的寫入模式使用 `-SkipPermissions` 重跑。不得把 CLI 未公開的額度資料推測成真實剩餘流量。
- AGY 以 `-SkipPermissions` 重跑仍在 184 秒外層逾時前未產出程式碼。Codex 子代理第一次啟動則證實目前本機 CLI 拒絕 `--sandbox workspace-write` 與 `--approve-for-me` 並用；兩次失敗都未修改程式碼，Codex 將移除衝突旗標後重跑。
- Codex 與 Claude 子代理重跑也分別在 184 秒、170 秒逾時，沒有交付產品程式碼；Codex 只留下 app-server JSON schema 暫存資料，已擷取 rate-limit 協定證據後安全刪除。主代理已依官方 Codex App Server 文件與本機 live probe 實作 `CliUsageReader`：使用 `initialize`／`initialized` 後呼叫 `account/rateLimits/read`，解析 `usedPercent`、`windowDurationMins`、`resetsAt`。本機實測曾取得 Codex `usedPercent=7`（剩餘 93%）及有效重置時間。AGY、Claude 的本機 `--help` 沒有同等非互動額度介面，UI 必須顯示不支援，不得推測。
- 主視窗已新增四個 CLI 的「剩餘流量與重置倒數」區塊；只有 Codex 顯示真實伺服器資料，其餘明確顯示 CLI 未提供。額度只在顯示視窗或手動重新整理時查詢，之後每秒只在本地更新倒數。排程按鈕依新增／編輯狀態顯示「建立排程」或「儲存修改」。AGY 節省模式追加 `--mode plan`，Claude 追加 `--prompt-suggestions false`。已新增協定解析與不支援狀態測試，尚待建置與完整測試。
- 第一次 Release 建置尚未進入編譯即失敗：workspace 沙箱無權讀取 `C:\Users\nojac\AppData\Roaming\NuGet\NuGet.Config`。這是環境權限阻擋，不是原始碼診斷；下一步以相同命令取得沙箱外讀取權限後重跑。
- 取得權限後 Release 建置成功（0 警告、0 錯誤）。完整測試 11/12：新 `CliUsageReader` 與其餘測試皆通過；唯一失敗為既有 `ExecutableLocatorResolution`，設定值 `codex` 仍先從 PATH 選到 WindowsApps 不可執行別名，導致 `Access denied`，尚未使用 Desktop-managed `OpenAI\Codex\bin\<hash>\codex.exe`。下一步調整預設命令解析優先序後重跑。
- Codex 路徑優先序修正後一度達到 12/12。加入產品 `CliUsageReader` 的真實 app-server 額度查詢後又為 11/12，抓到握手競態：連續送出 `initialize`／`initialized`／`account/rateLimits/read` 時，伺服器偶爾回覆 `Not initialized`。需等待 `id:0` 初始化成功後才送後兩者，再重跑。
- 嚴格等待 `id:0` 後 live test 改為初始化逾時；相同 PowerShell JSONL 探針 1 秒內成功並讀得 `usedPercent=11`。根因為 C# `StandardInputEncoding = Encoding.UTF8` 在第一行送出 BOM，污染唯一的 `initialize`；已改為 `new UTF8Encoding(false)`，同時保留嚴格握手，待重跑。
- UTF-8 無 BOM 修正後 Release 建置成功（0 警告、0 錯誤），完整測試連續兩次 12/12 通過；`ExecutableLocatorResolution` 現在除了四 CLI `--version`，也會以產品 `CliUsageReader` 真實呼叫 Codex app-server 並確認至少一個有效額度視窗。四個假 CLI 平行耗時分別約 1202 ms、1206 ms。
- Release EXE 已以隱藏視窗 smoke test 真實啟動，等待 5 秒（足以執行啟動畫面額度讀取）後仍 `Responding=True`，視窗標題為「AI 倒數喚醒」且有有效 MainWindowHandle，之後已關閉該測試實例。Computer Use 依技能流程初始化與重置重試均被 `EPERM: lstat C:\Users\nojac\AppData\Local\OpenAI\Codex` 阻擋，因此本次沒有可信新版畫面截圖，不得宣稱已做視覺驗證。

- 已補齊根目錄標準 MIT `LICENSE` 檔案。
- 已透過 GitHub CLI 設定專案 Description 簡介。
- 已完成 v1.0.0 正式版 Release 發布與 Asset 上傳。
- 已初始化 Git `main`，提交 `75fa303`（`Initial AI wake scheduler`），並建立儲存庫 `https://github.com/nojackno2-ctrl/AI-Wake-Scheduler`。
- 本機已安裝 .NET SDK 10.0.302；.NET 8 與 10 Windows Desktop Runtime 均存在。
- 本機 `agy.exe` 位於 `%LOCALAPPDATA%\agy\bin\agy.exe`，已由 `agy --help` 證實非互動模式為 `agy --print <訊息>`。
- 本機 `claude.exe` 位於 `%USERPROFILE%\.local\bin\claude.exe`，已由 `claude --help` 證實非互動模式為 `claude --print <訊息>`。
- PATH 中的 `codex.exe` 指向 WindowsApps 套件內檔案，但目前 Shell 實際執行會回報 `Access is denied`；程式必須允許使用者指定可執行檔路徑，並清楚記錄此類錯誤。
- 使用者已進一步確認不需要複雜任務提示，只要傳送「早安」等簡單詞語即可；預設值與操作流程應維持簡單。

## 設計方向

- 建立無外部 NuGet 相依的 .NET 8 WinForms 解決方案，讓 Visual Studio 2022 可直接開啟與建置。
- 排程持久化至使用者 LocalAppData；主程式縮到系統匣後持續倒數，到點以非互動模式同時啟動勾選的 CLI。
- 使用 `ProcessStartInfo.ArgumentList` 傳遞訊息，避免把使用者訊息拼成 Shell 指令。
- 保留每個 CLI 的 stdout、stderr、結束碼與執行紀錄；提供 CLI 路徑設定及不消耗 AI 額度的 `--version` 檢查。
- 提供可選的登入 Windows 自動啟動，讓重新登入後仍能處理已保存排程。

## 完成與驗證狀態

- 已建立 `AI倒數喚醒.sln`，包含 Core、WinForms 與無 NuGet 測試專案骨架。
- 已實作第一版資料模型、原子 JSON 儲存、CLI 定位/安全參數傳遞、平行執行與日誌、持久化排程引擎、繁中主介面、CLI 設定/`--version` 檢查、系統匣及登入自動啟動。
- Debug 與 Release 解決方案均建置成功；最終 Release 為 0 警告、0 錯誤。無 NuGet 測試 6/6 通過，包含含 Shell 符號訊息的單一參數、三 CLI 平行啟動、週期推進與到期排程。
- 第一次實際啟動在主畫面建構時失敗：`SplitContainer.SplitterDistance` 在控制項尚未取得實際寬度前與 `PanelMinSize` 衝突。不得重複在 object initializer 設定固定分隔距離；修正為視窗顯示後再設定並重新做視覺驗證。
- Computer Use 技能可列出此 WinForms 視窗，但擷取/啟用操作在目前工具執行環境回報 `node_repl exec context not found`；已改以 Win32 前景擷取取得上述實際錯誤畫面。
- 延後設定主分隔欄寬後，主視窗已成功啟動並完成 1180x760 實際截圖；主排程列表、編輯欄、預設「早安」及三個 CLI 選項均可見。
- 設定視窗實際截圖發現 CLI 表格最後一列被百分比高度撐開；已改成四列固定一致高度並重新擷取確認。
- 已補單次、每天、每週週期；週期計算會跳過已錯過的週期並保留本地時刻。主畫面已實際顯示繁中「單次」下拉與「週期」欄。
- 使用者要求避免過量消耗 Token。已預設節省 Token 模式：最多 50 字、每 CLI 一次且不重試、空白專用 workspace、Antigravity/Claude 低 effort、Claude 禁用 tools、Codex `read-only` 並覆寫低 reasoning/verbosity，預設逾時縮短為 3 分鐘。
- 官方 OpenAI Developer commands 文件已確認 `codex exec` 為 Stable 非互動命令，支援 `--sandbox read-only` 與 `--skip-git-repo-check`；Config Reference 確認 `model_reasoning_effort=low` 與 `model_verbosity=low`。
- 修正後設定視窗與 CLI 檢查結果已再次實際截圖，三列整齊且結果逐行換行。實際 `--version`：Antigravity 1.1.11 成功、Claude 2.1.220 成功、Codex WindowsApps 路徑仍為存取被拒（程式已清楚顯示且可瀏覽改路徑）。
- 使用者要求三個 CLI 獨立運作。`ScheduleManager` 會先具體化全部執行 Task，再以 `Task.WhenAll` 等待收尾；三個程序會平行啟動，不會循序等待。已加入三個假 CLI 各延遲 1 秒的時間型測試防止退化。
- 三個各延遲 1 秒的假 CLI 由同一排程平行完成，實測總時間 1206 ms（循序執行至少約 3000 ms）。
- Release EXE 已實際啟動且視窗有回應；最終程式路徑為 `src/AiWakeScheduler.WinForms/bin/Release/net8.0-windows/AI倒數喚醒.exe`。
- 刻意未真的送出「早安」，避免開發驗證消耗三個 AI 訂閱；執行路徑已用三個假 CLI 驗證，真實喚醒由使用者在 UI 明確排程或按「立即執行」。
- GitHub 上傳前重新執行 Release 建置成功（0 警告、0 錯誤），無 NuGet 測試再次 6/6 通過；平行 CLI 測試耗時 1225 ms。
- 2026-08-10 使用者釐清「時間設定」是每天固定的幾點幾分，不需要日期、單次或每週選項。主介面已改為單一 `HH:mm` 選擇器與「每天時間」列表欄，核心儲存一律正規化成 Daily 並計算下一個未來觸發點；舊排程載入時保留時分並轉為每日。另調整「立即執行」使它不改變或跳過原本的下一次每日排程。
- 第一次驗證誤將方案建置與 `dotnet run` 測試平行執行，兩者同時寫入 `AiWakeScheduler.Core\obj\Release`，造成 CS2012 檔案鎖衝突；這不是原始碼編譯診斷。後續驗證必須依序執行。
- 依序 Release 建置已成功（0 警告、0 錯誤）。首次測試為 5/6：`ScheduleManagerDueJob` 已實際回到新規格的 `Pending`，但測試迴圈仍等待舊版一次性排程的 `Completed`，逾時後觸發過時斷言；已改為等待 `Pending` 且三個結果齊全，待重跑。
- 修正過時斷言並把新排程模型預設值改為 Daily 後，最終 Release 建置成功（0 警告、0 錯誤），無 NuGet 測試 6/6 通過；三個各延遲 1 秒的假 CLI 最新平行完成耗時 1225 ms。
- Release EXE 已實際啟動。Computer Use 可找到視窗但畫面擷取仍回報 `node_repl exec context not found`；改用 Windows UI Automation 讀取真實控制項並以 Win32 前景畫面檢查。已確認設定區只剩「每天時間」`HH:mm`、列表只有「每天時間／倒數」而沒有日期或週期，版面顯示正常。
- 已完成 WinForms 專案（MainForm.cs, Program.cs, SettingsForm.cs, StartupManager.cs）優化：
  1. `MainForm.cs`：
     - 加入 `Dispose(bool disposing)` 覆寫，正確釋放 `_uiTimer`、`_notifyIcon` (及其 `ContextMenuStrip`)、`_gridRefreshLock` (SemaphoreSlim) 與快取的 GDI `Font` 物件。
     - 優化 `UiTimerOnTick`：當主視窗縮小至系統匣 (`!Visible || WindowState == Minimized`) 時暫停計時器畫面更新與倒數計算，節省背景運作 CPU 與字串記憶體分配；僅在倒數文字實際變更時才更新 DataGridView 儲存格。從系統匣還原視窗時立即觸發觸發一次刷整。
     - 強化 UI 線程安全性：`ManagerOnJobsChanged` 與 `ManagerOnBackgroundError` 使用跨執行緒呼叫防護與 delegate 異常捕捉；`RefreshGridAsync` 引入 `SemaphoreSlim` 重入保護。
     - 增加 `OpenFolder` 與檔案開啟異常防禦。
  2. `Program.cs`：
     - 完善單一執行個體 `Mutex` 的 `ReleaseMutex()` 釋放保護；確保 `ScheduleManager.DisposeAsync()` 於 `try-finally` 中執行，避免 UI 或初始化異常時遺留背景工作與資源。
  3. `SettingsForm.cs`：
     - 快取並處置 GDI `Font` 物件，避免屢次點擊 Header 重複創建 Font 洩漏 GDI 控制點；在 `ProbeAllAsync` 中加入 `IsDisposed` 防禦與異常補捉。
  4. `StartupManager.cs`：
     - 增加 `Process.GetCurrentProcess().MainModule?.FileName` 的安全備用路徑，並對登錄機碼 `UnauthorizedAccessException` / `SecurityException` 與 `null` 進行防禦性檢查與訊息包裝。
- Debug 與 Release 方案均重新建置成功（0 警告、0 錯誤）。
- 2026-08-10 分支整合：`optimize/robustness-and-daily-schedule` 已整合併入 `main`，7/7 測試驗證通過，並已清理刪除已整併的遠端與本機功能分支。
- 2026-08-10 檔案更新：已更新對外公開的繁體中文 `README.md`，包含專案簡介、核心特色徽章、三大多模型 CLI 平行喚醒與 Token 節省參數說明、快速上手與排程設定教學、Visual Studio 與 CLI 建置指引、資料路徑及常見問答（FAQ）。
- 2026-08-10 修復 CLI 搜尋與探測問題：
  1. 問題診斷：設定視窗點擊「檢查三個 CLI」時回報「✗ 找不到 Codex CLI。」。原因為 Windows 上的 OpenAI Codex Desktop / CLI 會將 `codex.exe` 安裝於 `%LOCALAPPDATA%\OpenAI\Codex\bin\<hash>\codex.exe` 等雜湊版本子目錄，而舊版 `ExecutableLocator` 僅檢查固定路徑且本機 PATH 未註冊 codex。
  2. 解決方案：
     - `ExecutableLocator.cs`：實作 `FindInSubdirectories`，自動遞迴掃描 `%LOCALAPPDATA%\OpenAI\Codex\bin`、`%LOCALAPPDATA%\Programs\OpenAI\Codex` 及常見安裝位置下的 `codex.exe`（按最後寫入時間排序取最新），並擴充 Antigravity、Claude 與 Codex 支援多種標準目錄與全域 npm / local bin。
     - `ResolveExplicitPath`：強化支援相對路徑與工作目錄內檔案直接解析。
     - `CliRunner.cs`：ProbeAsync 輸出讀取支援 stdout 為空時從 stderr 擷取版本資訊。
  3. 驗證結果：
     - Release 方案建置成功（0 警告、0 錯誤）。
     - 新增 `ExecutableLocatorResolution` 測試（包含實測本機真實 `agy`、`codex`、`claude` 之解析與 `--version` 探測，Codex 實測輸出 `codex-cli 0.147.0-alpha.6.5`，Claude `2.1.220 (Claude Code)`，Antigravity `1.1.11`，全數 Probe 成功）。
     - 單元測試 8/8 全數通過。
- 2026-08-10 完成 Windows 繁體中文安裝版（Setup.exe）製作與自動化打包：
  1. 專案資產與元資料：
     - 產生具備 16x16, 32x32, 48x48, 64x64, 128x128, 256x256 多解析度之應用程式圖示 `assets/app.ico`。
     - `AiWakeScheduler.WinForms.csproj` 加入 `<ApplicationIcon>`, `<Product>`, `<Version>1.0.0</Version>`, `<AssemblyTitle>`, `<Company>`, `<Authors>`, `<Description>` 等元資料。
     - `MainForm.cs` 加入 `GetApplicationIcon()`，主視窗與系統匣統一顯示自訂圖示。
     - `StartupManager.cs` 修復單一檔案/Self-Contained 發布下 `Assembly.Location` 的 IL3000 警告。
  2. Inno Setup 6 繁中安裝程式（`installer/`）：
     - `installer/AI倒數喚醒.iss`：支援自訂目錄、`PrivilegesRequiredOverridesAllowed=dialog`（允許使用者選擇為本機或所有使用者安裝）、桌面與開始功能表捷徑、升級前自動關閉舊版進程、安裝後立即啟動、以及乾淨反安裝。
     - `installer/languages/ChineseTraditional.isl`：完整繁體中文語系檔。
  3. 一鍵自動建置腳本（`build-installer.ps1`）：
     - 自動定位 Inno Setup `ISCC.exe` 與 .NET SDK。
     - 自動執行單元測試 8/8 通過。
     - 執行 `dotnet publish` 發布 Self-Contained Win-x64（獨立內含 .NET 8 執行環境，使用者電腦不需額外安裝 .NET Runtime）。
     - 調用 `ISCC.exe` 使用 LZMA2/Ultra64 最高壓縮比產出 `dist\AI倒數喚醒_Setup_v1.0.0_x64.exe`（約 16.77 MB）。
  4. 驗證結果：
     - 單元測試 8/8 通過。
     - `build-installer.ps1` 一鍵建置成功，產出 `dist\AI倒數喚醒_Setup_v1.0.0_x64.exe`（SHA256: `9CF0A101C6057A0CE98DEBE671A6B3D6B5EE3BECA5E07137CEB3753AE77539EA`）。
     - 靜默安裝與反安裝在隔離目錄實測通過（安裝驗證 `AI倒數喚醒.exe` 存在、反安裝後 0 檔案殘留）。
     - `.gitignore` 與 `README.md` 已同步更新。
- 2026-08-11 擴充支援 Antigravity (Claude / GPT) 額度計數器獨立喚醒：
  1. 問題診斷：
     - Google Antigravity 的配額分為兩組獨立計數器：Gemini Models（Gemini 3.6 Flash / Pro 等）與 Claude and GPT models（Claude Sonnet 4.6 Thinking / Claude Opus 4.6 / GPT-OSS 120B 等）。
     - 過去呼叫 `agy --print` 只會喚醒 AGY 預設的 Gemini 模型，使得「Gemini Models」的 5 小時倒數啟動，但「Claude and GPT models」的 5 小時額度維持未觸發。
  2. 解決方案：
     - `Models.cs`：`CliKind` 新增 `AntigravityClaude`。更新 `CliDisplayNames`（`Antigravity` -> `Antigravity (Gemini)`、`AntigravityClaude` -> `Antigravity (Claude / GPT)`），更新預設 Profile 與排程目標預設值，並在 `EnsureDefaults()` 具備向後相容補齊機制。
     - `ExecutableLocator.cs`：`AntigravityClaude` 支援定位本機 `agy.exe`。
     - `CliCommandBuilder.cs`：`AntigravityClaude` 預設指定 `--model "Claude Sonnet 4.6 (Thinking)"`，並支援在額外參數中自訂 `--model` 覆寫。
     - `MainForm.cs`：主介面「傳送至」支援 4 個 CheckBox，排程列表 CLI 簡稱與資料繫結全面支援 4 個目標。
     - `SettingsForm.cs`：設定表格自適應調整為 4 組 CLI，標籤欄拓寬至 175px，按鈕更新為「檢查全部 CLI（只執行 --version）」。
     - `Program.cs` (Tests)：新增 `AntigravityClaude` 參數建構測試、4 CLI 平行執行測試與探測測試。
     - `README.md`：同步更新 4 項目標說明與雙額度池機制介紹。
  3. 驗證結果：
     - Release 方案建置成功（0 警告、0 錯誤）。
     - 單元測試 8/8 全數通過（含 4 個假 CLI 平行耗時 1205 ms）。
     - `build-installer.ps1` 自動打包成功，產出最新 `dist\AI倒數喚醒_Setup_v1.0.0_x64.exe`（16.77 MB）。
- 2026-08-11 解耦重構與效能／Token 用量最佳化：
  1. 修正兩個實際導致喚醒失效的參數順序問題（本次最重要的發現）：
     - `agy` 的 `--print` 是「值就是提示詞」的字串旗標，單獨給會得到 `flag needs an argument: -print`；
       且 Go 的 flag 套件遇到第一個非旗標參數就停止解析。
       原本產生的 `--print --effort low ... 早安` 等於把 `--effort` 當成提示詞送出，
       實際日誌可見模型在解釋「--effort 是什麼」，`早安` 從未送達，節省 Token 的旗標也全部失效。
     - `AntigravityClaude` 更嚴重：`--print --model "Claude Sonnet 4.6 (Thinking)" ...` 使 `--model` 成為提示詞、
       模型名稱成為位置參數，之後所有旗標被丟棄，實際回覆為
       「You are currently using **Gemini 3.6 Flash**」——也就是 Claude / GPT 額度池從未真正被喚醒過。
     - 修正：`CliDescriptor.PromptFlag` 明確表示提示詞要接在哪個旗標之後，Antigravity 一律排在所有旗標最後。
     - 另發現 Claude 系列模型不接受 `--effort`（`Error: --effort is not supported for model ...`），
       `AntigravityClaude` 已移除該旗標；模型改用 `agy models` 列出的 ID `claude-sonnet-4-6`。
  2. 解耦：
     - 新增 `CliCatalog.cs`，把顯示名稱、短名稱、預設命令、參數計畫與安裝路徑候選集中為單一知識來源，
       取代原本分散在 `Models.cs`、`CliCommandBuilder.cs`、`ExecutableLocator.cs`、`MainForm.cs` 的四份 switch。
       新增一個 CLI 現在只需新增一筆描述，視窗與參數建構都不必修改。
     - 新增 `Abstractions.cs`（`IDataStore<T>`、`ICliRunner`），`ScheduleManager` 改為只依賴介面與 `TimeProvider`。
     - `CliRunner` 拆為政策層 + `ProcessRunner`（程序機制）+ `CliLogWriter`（日誌與保留策略）。
     - WinForms 新增 `AppHost`（組合根）、`JobPresenter`（顯示格式化）、`AppTheme`（共用字型色彩）、`NativeMethods`。
  3. Token 用量（旗標皆以 `--help` 實測解析通過，未捏造）：
     - Claude CLI 追加 `--safe-mode`（停用 CLAUDE.md、技能、外掛、hooks 與 MCP 伺服器）與 `--strict-mcp-config`，
       與既有的 `--tools ""` 合計移除請求中絕大多數的固定輸入；認證與內建行為不受影響。
     - Codex 追加 `--ignore-user-config`（不載入 config.toml，連帶不載入 MCP 伺服器與自訂指示）、`--ignore-rules`、
       `--ephemeral`（不寫 session 檔）、`--color never`。
     - 提示詞改為要求只回「OK」，把單價最高的輸出 Token 壓到最低。
     - 修正重開程式會重跑今天已完成喚醒的問題：改以 `FinishedAt` 與 `GetPreviousDailyOccurrence` 判斷，
       「關閉期間錯過」仍會補做一次，「今天已跑過」則直接排到明天。原本每重開一次就多花一整輪 Token。
  4. CPU／記憶體／視窗：
     - `ScheduleManager` 由每秒 `PeriodicTimer` 輪詢改為自適應等待（直接睡到下一個到期時間，上限 30 秒），
       並以號誌在排程異動時立即喚醒；系統匣待機期間 20 秒 CPU 用量實測 0.016 秒。
     - `MainForm` 清單改為就地更新（原本每次 `Rows.Clear()` + `Add()`），事件合併避免四個 CLI 同時完成時重複重繪，
       DataGridView 開啟雙緩衝，UI 計時器在視窗隱藏時完全停止。
     - 縮到系統匣時執行壓縮式 GC 並釋放工作集；`ProcessRunner` 限制單一串流最多擷取 32 KB，
       避免話多的 CLI 撐大記憶體與日誌；日誌檔保留上限 200 個。
     - 字型改為全程式共用並於結束時釋放（原本每個視窗各自 `new Font` 且未釋放）。
     - 視窗啟用 PerMonitorV2 DPI、`AutoScaleMode.Font`，標題列改用 TableLayoutPanel 取代絕對座標，
       視窗尺寸依螢幕工作區夾限，`SplitterDistance` 設定加上安全檢查。
     - csproj 設定工作站非並行 GC（常駐系統匣情境下記憶體與 CPU 較低）。
  5. 驗證結果：
     - 建置 0 警告、0 錯誤；單元測試 11/11 通過（新增 CliCatalog、自適應等待、重開不重跑、Antigravity 參數順序迴歸測試）。
     - 四個 CLI 皆以新參數實際執行成功（exit 0），Antigravity (Gemini) 與 Antigravity (Claude / GPT) 皆回覆單一「OK」。
     - GUI 以 UI Automation 驗證：僅一個視窗（無例外對話框）、排程列表 5 個儲存格正確填入；重開程式未再觸發喚醒。
- 2026-08-11 版本升至 v1.1.0 並重新打包安裝檔：
  - 因本次含實質行為修正（Antigravity Claude / GPT 額度池從未真正被喚醒），
    `AiWakeScheduler.WinForms.csproj` 與 `installer/AI倒數喚醒.iss` 版本號由 1.0.0 升至 1.1.0。
  - 產出 `dist\AI倒數喚醒_Setup_v1.1.0_x64.exe`
    （47.84 MB / 50,162,478 位元組，
    SHA256: `1394136AEDB04276A2C1FBE30667DE981ECC7F6C5E6DC7C0B2921A0AC8F7CD93`）。
  - 已確認發布的 `AiWakeScheduler.Core.dll` 內含 `claude-sonnet-4-6`、`--safe-mode`、
    `--ignore-user-config`，即安裝檔確實包含本次修正；`AI倒數喚醒.exe` 檔案版本為 1.1.0.0。
  - 已在 GitHub 建立 v1.1.0 Release 並上傳安裝檔（標記為 Latest）。
    注意：GitHub 會移除 Asset 檔名中的非 ASCII 字元，實際 Asset 名稱為
    `AI._Setup_v1.1.0_x64.exe`（v1.0.0 的 Asset 同樣是 `AI._Setup_v1.0.0_x64.exe`，屬既有行為）。
  - 安裝檔大小紀錄不一致，已查明並非本次變更造成：
    - 本文件先前記載 16.77 MB，但 GitHub v1.0.0 Release 的實際 Asset 為 20.02 MB，
      兩者皆與目前打包設定產出的 47.84 MB 不符。
    - 以 `git worktree` 檢出 tag `v1.0.0`，用與 `build-installer.ps1` 完全相同的 publish
      指令實測，輸出為 **467 個檔案／159.96 MB**；目前版本為 467 個檔案／159.98 MB。
      酬載形狀相同，證實本次變更未增加任何檔案
      （csproj 只新增 GC 與 TieredPGO 等 runtimeconfig 設定，不影響酬載大小）。
    - 結論：以儲存庫現行打包設定（`--self-contained true`、未啟用 trimming）產出的安裝檔
      就是約 47.84 MB。先前發布的 20.02 MB Asset 與文件中的 16.77 MB 是以不同設定產生的，
      與現行 `build-installer.ps1` 不一致；此差異在本次之前就已存在。
    - 若日後要縮小體積，最安全的選項是排除未使用的衛星語言資源：目前含 13 個語言資料夾
      （cs, de, es, fr, it, ja, ko, pl, pt-BR, ru, tr, zh-Hans, zh-Hant）共 15.5 MB（未壓縮），
      可用 `<SatelliteResourceLanguages>zh-Hant;en</SatelliteResourceLanguages>` 限定。
      WinForms 專案不建議啟用 PublishTrimmed。
