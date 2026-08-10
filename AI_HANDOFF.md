# AI HANDOFF

## 目前目標

已完成 C# Windows 桌面程式、Inno Setup 安裝包打包，並成功完成 GitHub v1.0.0 官方版本（含 Setup 安裝檔 Asset）發布；後續維持維護狀態。

## 目前狀態（2026-08-10）

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
