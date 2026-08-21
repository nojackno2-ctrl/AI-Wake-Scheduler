# AI HANDOFF

## 2026-08-22 v1.5.0 發布與 GitHub Release 完成（已提交並推送）

- **使用者授權**：「上傳github並發布執行檔」。
- **核心功能升級**：
  1. **自動讀取流量時機重構**：
     - **執行前**：啟動目標 CLI 前先校準最新流量與倒數狀態。
     - **倒數結束時**：排程器將 CLI 倒數結束時刻納入自適應睡眠時間推算，倒數歸零當下背景自動醒來探測，若額度已重置則立即執行該 CLI。
     - **未倒數時**：依設定間隔背景定時探測；若所有 CLI 都在倒數中則略過探測，避免不必要的 API 查詢。
     - **執行完成後**：執行完畢自動讀取最新扣抵倒數，即時更新主視窗與表格倒數欄位。
  2. **Claude 429 退避與防抖冷卻機制**：
     - Claude OAuth 查詢加入單航班鎖與 30 秒成功快取。
     - HTTP 429 優先遵守 `Retry-After` 退避（無標頭預設 5 分鐘），退避期間不重送請求，沿用最近成功快照。
     - 排程器背景探測嚴格遵守使用者設定之 `QuotaAutoRefreshMinutes` 間隔。
- **發布流程與產物**：
  1. 版本號全面升級為 `v1.5.0`（`AiWakeScheduler.WinForms.csproj`、`installer/AI倒數喚醒.iss`、`build-installer.ps1`、`CliUsageReader.cs`、`README.md`）。
  2. 執行 `build-installer.ps1`：Deterministic 測試 15/15 全數通過、Self-Contained win-x64 發布完成、Inno Setup 成功編譯出 `dist\AI倒數喚醒_Setup_v1.5.0_x64.exe`（48,619,278 bytes，SHA256: `2484E36818C8C218F386DC4907704A1BE8C633B8206143A88060DCB1BF2D8EAD`）。
  3. Git 提交並推送到遠端 `origin/main`，建立並推送 Tag `v1.5.0`。
  4. 透過 GitHub CLI 建立官方 Release `v1.5.0`，並成功上傳安裝包與校驗檔：`https://github.com/nojackno2-ctrl/AI-Wake-Scheduler/releases/tag/v1.5.0`。

## 2026-08-21 自動讀取流量時機重構（執行前、倒數結束時、沒抓到倒數、執行後）完成並驗證

- **使用者需求**：「自動讀取流量時機改成執行前，倒數結束時，沒抓到倒數」，並同意代理人建議之「執行完成後自動抓取新倒數」與「倒數結束 15 秒冷卻防抖重試」。
- **核心實作架構**：
  1. **執行前 (Before Execution)**：
     - 在 `ScheduleManager.ExecuteJobAsync` 啟動目標 CLI 前，若最新快照非近期取得，先讀取目標 CLI 的最新流量與倒數狀態。
  2. **倒數結束時 (Countdown Ended)**：
     - `CalculateNextDelay`：將正在倒數之 CLI 的倒數結束時刻納入自適應睡眠時間推算，倒數歸零當下背景迴圈立刻醒來。
     - `ScanAndStartDueJobsAsync`：當發現目標 CLI 倒數已歸零（`resetsAt <= now`），觸發探測，若額度已重置（未倒數）則立即執行該 CLI。
     - `MainForm.UiTimerOnTick`：每秒 UI 迴圈監控各快照，若 `now >= resetsAt` 立即觸發 `RefreshUsageAsync(showStatus: false)`（附帶 15 秒每 CLI 防抖冷卻）。
  3. **沒抓到倒數 (No Active Countdown)**：
     - 若 CLI 處於 100% 額度或未捕捉到有效倒數，才依 `QuotaAutoRefreshMinutes` 週期進行背景探測。
     - 若所有 CLI 皆處於正常有效倒數中，完全跳過盲目定期輪詢，徹底根除 API 負載與 HTTP 429。
  4. **執行完成後 (After Execution)**：
     - CLI 執行完畢後稍待 2 秒（測試環境 50ms）讓 API / Language Server 扣抵額度並開啟新一輪倒數，自動再次讀取最新倒數並觸發 `JobsChanged`，主視窗與表格即時同步最新倒數時間。
  5. **快照存取與設定優化**：
     - `ICliUsageReader` 新增 `GetLatestSnapshot` 與 `GetLatestSnapshots`，`CliUsageReader` 維護 `ConcurrentDictionary<CliKind, CliUsageSnapshot> _latestSnapshots`。
     - `MainForm.RefreshGridAsync` 從 `UsageReader.GetLatestSnapshots()` 自動同步最新快照。
     - `SettingsForm` 提示標籤更新為：「未倒數時的背景自動讀取間隔（分鐘，0 表示關閉；倒數中將於倒數結束自動讀取）：」。
- **測試與驗證成果**：
  1. Release 方案建置成功（0 警告、0 錯誤）。
  2. Deterministic 測試 15/15 全數通過（含未倒數節流探測、倒數結束自動喚醒探測、執行前/後用量讀取）。
  3. Real CLI 整合測試 15/15 全數通過。
  4. `git status` 與程式碼差異檢查無任何多餘殘留。
- **尚未執行**：未 commit、push、打包、安裝或發布（遵守 AGENTS.md 規則）。

- **使用者回報**：主畫面 Claude 額度顯示「Claude 額度查詢失敗（HTTP 429）」。
- **已確認根因**：`ScheduleManager` 的自適應迴圈最長每 30 秒醒一次；當存在已過每日首次時間的自動排程時，`ScanAndStartDueJobsAsync` 每輪都會重新呼叫所有啟用 CLI 的 `ICliUsageReader.ReadAsync`。這條背景探測沒有套用 `AppSettings.QuotaAutoRefreshMinutes`，因此 Claude `/api/oauth/usage` 可被每 30 秒查詢一次，另會疊加主視窗開啟、手動重讀及 UI 定時重讀，足以觸發 HTTP 429。
- **目前程式行為**：`ReadClaudeAsync` 對 401 有權杖重讀，但沒有併發合併、成功快取、`Retry-After` 退避或 429 的最近成功資料降級。
- **已完成程式變更**：
  1. `ScheduleManager` 的自動排程額度探測改為遵守 `QuotaAutoRefreshMinutes`；同一設定間隔內即使 30 秒迴圈或排程異動多次喚醒，也不再重送額度查詢，0 表示停用這條背景額度探測。
  2. `CliUsageReader` 的 Claude 查詢加入單航班鎖與 30 秒成功快取，避免主視窗與排程器同時對 `/api/oauth/usage` 發出重複請求。
  3. Claude 收到 HTTP 429 時優先遵守 `Retry-After`，缺少標頭時預設退避 5 分鐘；退避期間不再連線重試，有最近成功快照時沿用並重新校準倒數狀態，否則顯示可再次嘗試的時間。
  4. `ScheduleManagerQuotaAwareInterval` 新增迴歸情境：10 分鐘內多次喚醒排程器仍只允許一次 Claude 額度讀取，時間推進 11 分鐘後才允許第二次。
- **驗證結果**：
  1. Release 方案建置成功（0 警告、0 錯誤）。
  2. 新增的 `ScheduleManagerQuotaAwareInterval` 迴歸情境已在 sandbox 通過；同輪另有 ArgumentTokenizer、CliCatalog、CliCommandBuilder、CliUsageReader、ScheduleCalculator、JsonFileStore 通過。
  3. 完整 deterministic suite 在既有 `CliRunnerSafeArguments` 假 EXE 子程序案例再次卡住；前 7 組通過後已中止。嘗試改到主機環境重跑時，升權審核因本工作階段 Codex 用量上限拒絕，因此本輪不得宣稱 15/15 全數通過。
  4. `dotnet format --verify-no-changes --no-restore` 與 `git diff --check` 通過。
- **尚未執行**：未停止或替換目前執行中的 v1.4.0 正式安裝版，未對真實 Claude API 重打查詢，未 commit、push、打包、安裝或發布。

## 2026-08-21 v1.4.0 發布與 GitHub Release 完成（已提交並推送）

- **使用者授權**：「推送到github並發布執行檔」。
- **執行成果**：
  1. 版本號全面升級為 `v1.4.0`（`AiWakeScheduler.WinForms.csproj`、`installer/AI倒數喚醒.iss`、`build-installer.ps1`、`CliUsageReader.cs`、`README.md`）。
  2. 執行 `build-installer.ps1`：Deterministic 測試 15/15 全數通過、Self-Contained win-x64 發布完成、Inno Setup 成功編譯出 `dist\AI倒數喚醒_Setup_v1.4.0_x64.exe`（48,613,406 bytes，SHA256: `4A584C23DF8297D53F9A7D4BCB66D5EC4A5DD2A12BC045E29D177B37B5BB6A24`）。
  3. Git 提交並推送到遠端 `origin/main`，建立並推送 Tag `v1.4.0`。
  4. 透過 GitHub CLI 建立官方 Release `v1.4.0`，並成功上傳安裝包與校驗檔：`https://github.com/nojackno2-ctrl/AI-Wake-Scheduler/releases/tag/v1.4.0`。

## 2026-08-21 排程清單各 CLI 獨立倒數與自訂背景定時配額讀取完成並驗證

- **使用者要求**：「執行第1項（自動模式中，把每個CLI的倒數分開）」、「第2項也要，但是每個幾分鐘可以由使用者來決定，這個會消耗流量嗎?」。
- **流量諮詢結論**：查詢配額完全為**純唯讀查詢**（Antigravity 本機 RPC、Claude OAuth 方案查詢、Codex 限制視窗），不呼叫模型亦不建立對話回合，**0 Token 消耗，不佔用任何對話額度**。
- **修復與實作成果**：
  1. **排程清單各 CLI 獨立倒數（`JobPresenter.cs` / `MainForm.cs`）**：
     - 在 `JobPresenter.Countdown` 中傳入用量快照字典 `usageSnapshots`。
     - 若排程為自動模式（`Interval`），「倒數計時」欄位將直接分開呈現各已勾選 CLI 的獨立倒數狀態（例如：`Gemini 02:03:15；Claude 04:15:00；Claude/GPT 未倒數`）。
     - `MainForm` 之 `_uiTimer`（每 1 秒 tick）就地更新表格各 CLI 獨立倒數秒數，各 CLI 倒數即時跳動且互不干擾。
  2. **使用者自訂背景自動讀取配額間隔（`Models.cs` / `SettingsForm.cs` / `MainForm.cs`）**：
     - 在 `AppSettings` 中新增 `QuotaAutoRefreshMinutes` 欄位（預設 10 分鐘，0 代表關閉定時自動重讀）。
     - 在「設定」視窗加入「背景自動讀取配額間隔（分鐘，0 表示關閉）：[ 10 ] 分鐘」數值控制項。
     - 在 `MainForm.cs` 依據設定之分鐘數，於背景靜默自動執行 `RefreshUsageAsync(showStatus: false)` 定期校準伺服器端最新真實倒數。
  3. **測試與驗證（`Program.cs`）**：
     - 單元測試加入 `AppSettings.QuotaAutoRefreshMinutes` 複製、克隆與預設值 clamp 測試。
     - Release 建置成功（0 警告、0 錯誤）。
     - Deterministic 測試 15/15 全數通過（4 個假 CLI 平行完成 1213 ms）。
     - Integration 測試 1/1 通過（本機 Antigravity Gemini 剩餘 76% 固定倒數至 11:28:18，Claude/GPT 剩餘 100% 正確識別為未倒數）。
     - `dotnet format --verify-no-changes --no-restore` 與 `git diff --check` 通過。未 commit、push、發布。

## 2026-08-21 讀取配額間隔 5 秒連續採樣與真實倒數判定完成並驗證

- **使用者要求**：「我發現讀取流量程式會自己倒數，但實際上CLI根本沒有在倒數，為了避免這種狀況，讀取流量的時候應該要連續讀取好幾次，確認倒數真的有再跑」、「讀取配額間隔5秒」。
- **問題根因與修復成果**：
  1. **滑動時間戳記（Sliding Timestamp）辨識與多重取樣（`CliUsageReader.cs`）**：
     - 當 CLI 模型完全未被使用（`UsedPercent == 0`，100% 額度）時，Language Server/API 每次查詢都會動態產生 `now + 5h` 的滑動時間戳記。
     - 在 `CliUsageReader` 引入連續採樣驗證機制：取樣間隔明確設定為 **5 秒**（`Task.Delay(TimeSpan.FromSeconds(5))`），比對兩次取樣的 `resetsAt` 時間戳記。
     - 若時間戳記隨查詢時刻滑動，或 `UsedPercent == 0`，判定為未倒數（`IsActiveCountdown = false`）。
     - 只有在 `UsedPercent > 0` 且兩次採樣之 `resetsAt` 為固定時間戳記（差距 < 2 秒）且 `resetsAt > now` 時，才判定為真實倒數中（`IsActiveCountdown = true`）。
  2. **介面顯示優化（`MainForm.cs`）**：
     - 在 `FormatUsage` 中：
       - `IsActiveCountdown == true`：顯示即時倒數計時（如 `剩餘 81%（02:03:18 後重置，08/21 11:28）`）。
       - `IsActiveCountdown == false`（未倒數 / 額度充足）：顯示 `剩餘 100%（未倒數 / 額度充足）`，徹底消除無效之本地自體倒數。
  3. **自動模式與判定輔助（`CliUsageReader.IsCountingDown`）**：
     - `IsCountingDown` 直接檢驗 `targetWindow.IsActiveCountdown && targetWindow.ResetsAt > now`，確保自動排程在進行未倒數喚醒時精確判斷真正未倒數之 CLI。
  4. **測試與驗證（`Program.cs`）**：
     - 單元測試加入 `IsActiveCountdown` 固定與滑動時間戳記判定斷言。
     - Release 建置成功（0 警告、0 錯誤）。
     - Deterministic 測試 15/15 全數通過（4 個假 CLI 平行完成 1209 ms）。
     - Integration 測試 1/1 通過（本機 Antigravity Gemini 剩餘 81% 固定倒數至 11:28:18，Claude/GPT 剩餘 100% 正確識別為未倒數）。
     - `dotnet format --verify-no-changes --no-restore` 與 `git diff --check` 通過。未 commit、push、發布。

## 2026-08-21 自動模式「流量未倒數立即執行」與「重新讀取倒數」按鈕修復完成並驗證

- **使用者要求**：「重新讀取倒數按鈕消失了，在自動模式中，加入流量未倒數立即執行，立即執行是針對有被啟用的CLI，而且只針對沒有倒數的，而不是全部呼叫，而且是以當天首次呼叫時間之後才算」。
- **修復與實作成果**：
  1. **修復「重新讀取倒數」按鈕版面（`MainForm.cs`）**：
     - 修復「剩餘流量與重置倒數」GroupBox 內部 TableLayoutPanel 因 4 組 CLI 多行文字排版擠壓導致按鈕被裁切消失的問題。
     - TableLayoutPanel 設為 `RowCount = CliCatalog.All.Count + 1`、`Dock = DockStyle.Top`、`AutoSize = true`、`AutoSizeMode = GrowAndShrink`，為每列（含按鈕列）加入明確 `RowStyle(SizeType.AutoSize)`，GroupBox 也設定 `AutoSizeMode = GrowAndShrink`。按鈕文字統一定義為「重新讀取倒數」。
  2. **抽象與額度狀態判斷（`Abstractions.cs` / `CliUsageReader.cs`）**：
     - 定義 `ICliUsageReader` 介面，解耦 `ScheduleManager` 與實體用量讀取，支援可重現測試模擬。
     - `CliUsageReader` 實作 `ICliUsageReader` 並新增 `IsCountingDown(CliUsageSnapshot? snapshot, DateTimeOffset now)`，支援 Claude 5 小時短期視窗、Antigravity Gemini、Antigravity Claude/GPT、Codex 精準倒數判定。
  3. **排程引擎精準單獨喚醒（`ScheduleManager.cs`）**：
     - 建構函式注入 `ICliUsageReader? usageReader`。
     - 在自動模式（`Interval`）掃描時：若當前時間已過當日「首次喚醒時間」（`localNow >= todayInitial`），透過 `_usageReader` 探測排程勾選啟用（`job.Targets`）之 CLI。
     - 若發現有啟用的 CLI `IsCountingDown == false`（未在倒數中或倒數已結束），**僅單獨將未倒數之 CLI 列表抽出執行**，絕不呼叫仍在倒數中之其他 CLI。
     - 引入 `_lastQuotaWakeupByCli`（2 分鐘冷卻期）防止狀態更新延遲造成重複觸發；執行完成後合併更新 `job.LastResults`。
  4. **組合根（`AppHost.cs`）**：
     - 將 `usageReader` 注入 `ScheduleManager`。
  5. **測試與驗證（`Program.cs`）**：
     - `TestCliUsageReaderAsync`：加入 `IsCountingDown` 於未來時間、過去時間、Claude 5h/7d 視窗及錯誤狀態之完整斷言。
     - 新增 `TestScheduleManagerQuotaAwareIntervalAsync` 單元測試：
       - 驗證當日首次時間前（07:30 < 08:00）：未倒數亦不提前喚醒。
       - 驗證當日首次時間後（08:30 > 08:00）：啟用 3 個 CLI（1 個倒數中、2 個未倒數），精準只喚醒未倒數的 2 個 CLI，且不呼叫倒數中的 CLI。
  6. **驗證結果**：
     - Release 建置成功（0 警告、0 錯誤）。
     - Deterministic 測試 15/15 全數通過（含 4 個假 CLI 平行完成 1203 ms）。
     - Integration 測試 1/1 通過（真實 Antigravity Gemini 剩餘 90%、Claude/GPT 剩餘 100% 讀取成功）。
     - `dotnet format --verify-no-changes --no-restore` 與 `git diff --check` 通過。未 commit、push、發布。

## 2026-08-21 Claude CLI 真實額度倒數讀取完成並驗證

- **使用者要求**：「不管用什麼方法都要讀取道 claude 的倒數」。
- 本機 `Claude Code 2.1.220` 已登入 Claude Pro；其原生執行檔明確包含唯讀查詢 `GET /api/oauth/usage`，以及 `five_hour`、`seven_day`、`resets_at` 等欄位。官方 Claude Code 文件亦確認 `/usage` 顯示方案用量與重置時間。
- 已用本機 Claude Code 的 OAuth access token 對 `https://api.anthropic.com/api/oauth/usage` 實際唯讀驗證成功；回應同時包含五小時與七天視窗、使用百分比及精確重置時間。這證實舊有「Claude CLI 不支援機器讀取」判斷已過時。
- `CliUsageReader` 已新增 Claude 分支：從 `%USERPROFILE%\.claude\.credentials.json`（亦支援 `CLAUDE_CONFIG_DIR`）唯讀載入短效 access token，帶 Bearer 與 `oauth-2025-04-20` 標頭查詢 Anthropic usage 端點；401 時會重讀一次本機權杖以接住 Claude Code 同步輪替，但不自行寫入或記錄任何認證資料。
- 新增 `ParseClaudeUsage`，解析 `five_hour`、`seven_day` 及可選的 Opus／Sonnet／OAuth Apps／Cowork 七天視窗；單元測試固定驗證百分比、視窗長度、時區與錯誤回應，integration 測試加入真實 Claude 額度查詢。README 已同步移除舊有 Claude「不支援」說明。
- Release build 成功（0 警告、0 錯誤）。第一次在 sandbox 內執行 deterministic tests 時，前 6 組（含新的 `CliUsageReader`）通過，之後再次卡在此環境既知的損毀 EXE 程序案例而無輸出，約 120 秒後手動終止；改在主機環境重跑後 14/14 全數通過，四個假 CLI 平行完成耗時 1258 ms。
- 主機 integration 1/1 通過；產品程式實際讀得 Claude 五小時視窗剩餘 100%、重置 2026-08-21 11:19:59，以及七天視窗剩餘 74%、重置 2026-08-26 05:59:59。該輪 Antigravity 因抓不到 CSRF token 回傳 Unavailable，測試按既有設計不將其視為 Claude 功能失敗。
- `dotnet format --verify-no-changes --no-restore` 與 `git diff --check` 通過。未 commit、push、打包、安裝或發布。

## 2026-08-21 v1.3.1 發布與 GitHub Release 完成（已提交並推送）

- **使用者要求**：「推送AI倒數喚醒並發布執行檔」-> 經使用者明確授權進行 Commit、Push、打包與 Release。
- **發布流程與產物**：
  1. 版本號全面升級為 `v1.3.1`（`AiWakeScheduler.WinForms.csproj`、`installer/AI倒數喚醒.iss`、`build-installer.ps1`、`CliUsageReader.cs`、`README.md`）。
  2. 執行 `build-installer.ps1`：Deterministic 測試 14/14 全數通過、Self-Contained win-x64 發布完成、Inno Setup 成功編譯出 `dist\AI倒數喚醒_Setup_v1.3.1_x64.exe`（48,607,109 bytes，SHA256: `56CF8BB90F1A83FF89DF5DC45FE00AB35FCF784D4E5E12760D739A30E2A1F4A0`）。
  3. Git 提交並推送到遠端 `origin/main`，建立並推送 Tag `v1.3.1`。
  4. 透過 GitHub CLI 建立官方 Release `v1.3.1`，並成功上傳安裝包與校驗檔。

## 2026-08-20 開機自動啟動衝突修復與程式碼防護優化（已完成）

- **使用者要求**：「開機只執行正式安裝版跟程式碼優化」。
- **實作與系統處置**：
  1. **系統開機項清理與程序切換**：
     - 已移除登錄檔 `HKCU:\Software\Microsoft\Windows\CurrentVersion\Run\AI倒數喚醒` 中指向 Debug 版的開機項。
     - 保留啟動資料夾捷徑 `AI 倒數喚醒.lnk`（指向 `%LOCALAPPDATA%\Programs\AI 倒數喚醒\AI倒數喚醒.exe --minimized`，正式安裝版）。
     - 已停止原本常駐的 Debug 程序（PID 21924），並啟動正式安裝版（PID 11936，`Responding=True`）。
  2. **程式碼優化（`Program.cs`）**：
     - 在單一執行個體 Mutex 檢查處加入判斷：若啟動參數包含 `--minimized`（開機或背景啟動），且已有另一實例在執行中時，直接**靜默退出**（不彈出 `MessageBox` 提示），徹底避免開機彈窗干擾。
     - 若為使用者手動雙擊啟動（無 `--minimized`），則維持彈窗提示「AI 倒數喚醒已經在執行。」。
  3. **啟動管理強化（`StartupManager.cs`）**：
     - 新增 `CleanupStartupShortcuts()`，在透過 UI 設定「開機時自動啟動」開關時，自動清理啟動資料夾中的捷徑（`AI 倒數喚醒.lnk`、`AI倒數喚醒.lnk`），避免登錄檔與啟動資料夾同時存在導致未來再次產生雙重啟動衝突。
- **驗證結果**：
  - Release 建置成功（0 警告、0 錯誤）。
  - Deterministic 測試 14/14 全數通過（含 4 個假 CLI 平行完成 1249 ms）。
  - `dotnet format --verify-no-changes --no-restore` 與 `git diff --check` 通過。
  - 本機開機項與程序狀態複核確認無誤。

## 2026-08-18 v1.3.0 發布與 GitHub Release 完成（已提交並推送）

- **使用者要求**：「推送到github並發布執行檔」-> 經使用者明確授權進行 Commit、Push、打包與 Release。
- **發布流程與產物**：
  1. 版本號全面升級為 `v1.3.0`（`AiWakeScheduler.WinForms.csproj`、`installer/AI倒數喚醒.iss`、`build-installer.ps1`、`CliUsageReader.cs`、`README.md`）。
  2. 執行 `build-installer.ps1`：Deterministic 測試 14/14 全數通過、Self-Contained win-x64 發布完成、Inno Setup 成功編譯出 `dist\AI倒數喚醒_Setup_v1.3.0_x64.exe`（48,612,048 bytes，SHA256: `E7F20E5ED2E6746A4B62ADAC0D4A33C1CD43280EB70CBB39C90FBB14466734C9`）。
  3. Git 提交 `bfdcd7c` 並推送到遠端 `origin/main`。
  4. 透過 GitHub CLI 建立官方 Release `v1.3.0`，並成功上傳安裝包與校驗檔：https://github.com/nojackno2-ctrl/AI-Wake-Scheduler/releases/tag/v1.3.0

## 2026-08-18 自動模式跨日不排半夜與隔日首次時間重置完成（已包含於 v1.3.0）

- **使用者要求**：「自動模式的倒數是只限於當日嗎，假設我設定早上0530作為首次喚醒，但是我0800才起床開電腦，正常來說他要從0800開始倒數，但是一旦超國2400，就不應該繼續倒數，而是等到隔天的0530再次啟動」->「改成跨日不排半夜」。
- **實作重點**：
  1. **資料模型（`Models.cs`）**：
     - `ScheduledJob` 新增 `public TimeSpan? InitialTimeOfDay { get; set; }` 並更新 `Clone()`，確保設定的每日「首次喚醒時間」基準不因日間推進而遺失。
  2. **領域計算（`ScheduleCalculator.cs`）**：
     - 新增 `GetNextAutoIntervalOccurrence(TimeSpan initialTimeOfDay, DateTimeOffset finishedAt)`：
       - 若 `finishedAt + 5h1m` 仍在當日（未超過午夜 24:00），則排在該時間（如 08:00 $\rightarrow$ 13:01 $\rightarrow$ 18:02 $\rightarrow$ 23:03）。
       - 若推算時間跨入隔天（超過 24:00，例如 23:03 完工後算出的 04:04），不排在半夜，直接排定為隔日的首次喚醒時間（如隔天 05:30）。
  3. **排程引擎（`ScheduleManager.cs`）**：
     - `CompleteJobAsync`：使用 `GetNextAutoIntervalOccurrence` 計算下次執行時間。
     - `InitializeAsync`：
       - 若上次執行在昨日（或更早），且今日開機時間（如 08:00）已過首次喚醒時間（05:30），啟動時立即補做今日第一輪，並自 08:00 展開日間 5h1m 循環。
       - 若今日提早開機（如 04:30），安靜等待至 05:30，不提前執行。
       - 若今日已執行過且未滿 5h1m，重開程式不重複執行，維持既有倒數。
     - `UpsertAsync` 與 `ScanAndStartDueJobsAsync` 同步更新支援 `InitialTimeOfDay`。
  4. **WinForms UI（`MainForm.cs`）**：
     - `SaveScheduleAsync`：儲存自動模式時填入 `InitialTimeOfDay`。
     - `GridOnSelectionChanged`：選取排程時優先載入 `InitialTimeOfDay`，避免被目前倒數時分覆蓋。
  5. **測試驗證（`Program.cs`）**：
     - `TestScheduleCalculatorAsync`：新增當日 08:00 $\rightarrow$ 13:01、18:02 $\rightarrow$ 23:03、23:03 跨日重置隔天 05:30（不排 04:04）、20:00 跨日重置等單元測試。
     - `TestScheduleManagerAutoIntervalAsync`：引入 `FakeTimeProvider` 驗證首次到期、立即執行推進、當日重開不重跑、隔日逾期開機補做、隔日提早開機等待等全套情境。
- **驗證結果**：
  - Release 建置成功（0 警告、0 錯誤）。
  - Deterministic 測試 14/14 全數通過（含 4 個假 CLI 平行完成 1208 ms）。
  - Integration 測試 1/1 通過（Codex、Antigravity Gemini、Antigravity Claude/GPT 即時讀取成功）。
  - `dotnet format --verify-no-changes --no-restore` 與 `git diff --check` 通過。



- **使用者要求**：「我要你不管用什麼方法都要讀取道AGY的使用量倒數」->「這要可以把這個功能做到專案裡面嗎」-> 使用者批准實作。
- **實作重點**：
  1. **核心層（`CliUsageReader.cs`）**：
     - `ReadAsync` 擴充支援 `CliKind.Antigravity` 與 `CliKind.AntigravityClaude`。
     - **CSRF Token 取得機制全方位強化**：
       - 實作 `NativeProcessHelper`，透過 Win32 `NtQueryInformationProcess` 搭配跨 Windows 版本的 `RTL_USER_PROCESS_PARAMETERS` 多偏移量掃描（0x60, 0x68, 0x70, 0x78, 0x80）直接在記憶體中抓取 `--csrf_token`（微秒級完成、零子程序開銷）。
       - 內建多重 PowerShell 備用方案（`Get-CimInstance` 與 `Get-Process`，搭配 `-ExecutionPolicy Bypass`），確保各種權限與偵錯模式下皆能成功取得。
     - **Port 偵測機制強化**：結合 `language_server.log` 啟動日誌掃描與 PowerShell `Get-NetTCPConnection` Listen 連接埠查詢。
     - **平行查詢快取（3 秒 TTL）**：`_agyQueryLock` 與短期快取確保 `Antigravity` 與 `AntigravityClaude` 平行查詢時只向本機 Language Server 發送一次 RPC 請求。
     - 使用 .NET 8 原生 `HttpClient`（自訂憑證信任）發送 Connect-RPC 請求至 `POST /exa.language_server_pb.LanguageServerService/GetCascadeModelConfigData`，標頭帶上 `x-codeium-csrf-token`。
     - `Antigravity`（Gemini 模型池）與 `AntigravityClaude`（Claude / GPT 模型池）各自獨立計算剩餘額度百分比與 `resetTime` 重置時間戳記。
     - 提供公開靜態方法 `ParseAntigravityModelConfigs` 供可重現單元測試。
  2. **UI 層（`MainForm.cs`）**：
     - `RefreshUsageAsync` 狀態列更新為統計所有可用 CLI 額度更新狀態。
     - 既有版面自動無縫支援雙額度池即時倒數更新（每秒本機即時倒數，不頻繁發送網路請求）。
  3. **測試驗證（`Program.cs`）**：
     - `TestCliUsageReaderAsync` 新增 Gemini、Claude/GPT 雙額度池解析、邊界與異常測試。
     - `TestExecutableLocatorAsync`（`--integration`）加入本機真實 Language Server 額度讀取驗證，實測 Gemini 剩餘 94%（重置時間 19:10:00）、Claude/GPT 剩餘 100%（重置時間 19:24:27）。
- **驗證結果**：
  - Release 與 Debug 建置成功（0 警告、0 錯誤）。
  - Deterministic 測試 14/14 全數通過（含假 CLI 平行完成 1261 ms）。
  - Integration 測試 1/1 通過（Codex、Antigravity Gemini、Antigravity Claude/GPT 全數即時讀取成功）。
  - `dotnet format --verify-no-changes --no-restore` 與 `git diff --check` 通過。


## 2026-08-17 自動模式（每 5 個小時 + 1 分鐘再次呼叫）新增與驗證（完成，未提交）

- 使用者要求：「加入一個自動模式，功能啟用第一次喚醒後，每5個小時+1分鐘再次呼叫」。
- 實作重點：
  - **領域與計算**：`Models.cs` 新增 `ScheduleRecurrence.Interval` 與繁中顯示名稱；`ScheduleCalculator.cs` 定義常數 `AutoInterval = TimeSpan.FromHours(5) + TimeSpan.FromMinutes(1)`（301 分鐘）並擴充 `GetNextOccurrence`。
  - **排程引擎**：`ScheduleManager.cs` 支援 Interval 模式，在首次喚醒執行完畢（或手動「立即執行」完成）後自動將下次時間推進 `FinishedAt + 5小時1分鐘`；重開程式時，若前次執行在 5 小時 1 分鐘內則繼續等待（不重複扣額度），若關閉期間已逾期則啟動時立即補做一次並推進至下個 5 小時 1 分鐘。
  - **WinForms UI**：`MainForm.cs` 編輯區新增「排程模式」下拉選單（`每天固定時間` 與 `自動模式（每 5 小時 1 分鐘）`），選擇自動模式時動態切換時間欄位標籤為「首次喚醒時間」；排程清單時間欄標題更新為「時間 / 週期」並在自動模式下清晰顯示 `每5h1m (HH:mm)`，倒數欄即時倒數。
  - **測試驗證**：`Program.cs` 新增 `ScheduleManagerAutoInterval` 單元測試（驗證首次到期執行、執行後推進 5 小時 1 分鐘、立即執行推進、重啟未滿 5h1m 不重跑、重啟逾期補做）以及 `ScheduleCalculator` 301 分鐘精確推算測試。
- 驗證結果：
  - Release 建置成功（0 警告、0 錯誤）。
  - Deterministic 測試 14/14 全數通過（含 4 個假 CLI 平行完成 1209 ms）。
  - Integration 測試 1/1 通過。
  - `dotnet format --verify-no-changes --no-restore` 與 `git diff --check` 通過。

## 2026-08-14 Codex CLI 模型清單更新（完成，未提交）

- 使用者指出目前 Codex 下拉模型過舊。官方 OpenAI 模型目錄已將 `gpt-5.2-codex` 與 `gpt-5.1-codex*` 標為 deprecated；GPT-5.6 現行家族為 Sol／Terra／Luna。
- 本機 Codex CLI 0.148.0-alpha.9 以已登入主機身份呼叫只讀 App Server `model/list`（未建立模型回合、未消耗 Token），實際可選：`gpt-5.6-sol`（預設）、`gpt-5.6-terra`、`gpt-5.6-luna`、`gpt-5.5`、`gpt-5.4`、`gpt-5.4-mini`。
- App Server 同時回報模型 Effort 上限：Sol/Terra 支援 low～ultra；Luna 支援 low～max；5.5/5.4/5.4-mini 支援 low～xhigh。下一步更新靜態推薦清單、UI Effort 選項與參數正規化測試，確保不送出模型不接受的 Max/Ultra 組合。
- 已完成程式變更：Codex 下拉改為上述 6 款現行模型並保留空白的 CLI 自動預設；新增 Ultra；設定視窗會隨模型切換合法 Effort，核心執行路徑也會把舊設定或過高 Effort 降到該模型可接受的最高值。另順手套用同一機制修正 Gemini 3.1 Pro 不接受 Medium 的既知組合。尚待建置與測試，不得先宣稱完成。
- 最終補強額外參數 `--model`／`-m` 覆寫的解析，讓 Effort 依真正生效的模型正規化；Release 重新建置成功（0 警告、0 錯誤），deterministic 測試 12/12 通過，四個假 CLI 平行完成 1212 ms。

## 2026-08-14 本輪驗證完成（CLI 模型與思考程度）

- 目前工作樹的 Release `dotnet build --no-restore` 成功，0 警告、0 錯誤。
- 第一次 sandbox 內 deterministic 測試外層 120 秒逾時：前 6 組通過，停在 `CliRunnerSafeArguments` 的刻意損毀 EXE 啟動案例；直接執行測試 AppHost 也在相同位置阻塞。
- 依環境邊界改在使用者主機重跑同一份已建置測試 AppHost，12/12 全數通過；四個各延遲 1 秒的假 CLI 平行完成耗時 1256 ms。此結果確認功能與程序測試通過，sandbox 逾時屬損毀 EXE 測試的隔離環境限制。
- 本機安全核對：Codex CLI 0.148.0-alpha.9 的 `exec --help` 明列 `--model`，Claude CLI 2.1.220 的 `--help` 明列 `--model`／`--effort`；主機登入狀態執行只讀 `agy models` 仍列出全部下拉 ID（Gemini 3.7/3.6 Flash、Gemini 3.1 Pro、Claude Sonnet/Opus 4.6、GPT-OSS 120B Medium），未送出模型請求或消耗 Token。

## 2026-08-14 CLI 模型與思考程度（Reasoning Effort）自訂功能（完成，未提交，含使用者回報後的修正）

- 第一版實作依網路搜尋與臆測填入模型 ID，**未實際對照已安裝 CLI 驗證**。使用者實測 Settings 視窗後回報兩個問題：
  1. 視窗只看得到前兩個 CLI（Codex CLI、Claude CLI 兩列被裁掉，看不到也捲不到）。
  2. 「他提供的模型要真的可以呼叫」——質疑下拉清單裡的模型 ID 是否真實存在。
- 根因與修正：
  - **UI 裁切臭蟲**：`SettingsForm.BuildLayout()` 的 root `TableLayoutPanel` 把放 CLI 表格的那一列設為 `RowStyle(Percent, 100)`，同時 root 又設了 `AutoScroll = true`。這個組合會讓 TableLayoutPanel 把該列硬壓縮進「目前可見高度」，超出視窗的列直接被裁掉，而不是觸發捲動。已改為 `RowStyle(AutoSize)`，讓表格長到完整自然高度，交由外層 `AutoScroll` 捲動顯示；同時把預設視窗高度從 740 調到 820 減少捲動機會。
  - **模型 ID 不實**：改為實測核對而非猜測：
    - 用已安裝的 `agy.exe`（`C:\Users\nojac\AppData\Local\agy\bin\agy.exe`）跑 `agy models` 取得真實可用清單，並用一個明顯不存在的模型名稱丟給 `agy --print "hi" --model <fake>` 確認 agy 在打 API 之前就會本地驗證並立即報錯（不消耗任何 Token/額度），藉此安全地驗證多組 model/effort 組合是否合法。
    - 確認結果：`claude-sonnet-5`／`claude-opus-5`／`claude-fable-5`／`gemini-2.5-flash` 這些 ID **agy 完全不認識**；真正存在的是 `gemini-3.7/3.6/3.5-flash`（各自可搭配獨立的 `--effort low/medium/high`，其中 `gemini-3.1-pro` 只支援 low/high，沒有 medium）、`claude-sonnet-4-6`、`claude-opus-4-6-thinking`、`gpt-oss-120b-medium`（這三個模型 agy 會直接拒絕額外的 `--effort`，因為思考程度已內建在模型裡）。
    - `claude` CLI（`~/.local/bin/claude.exe`）的 `--help` 明確記載 `--model` 接受別名 `sonnet`/`opus`/`fable`（代表「最新版」）或完整 ID（如 `claude-fable-5`）；為避免未來新一代模型上市後清單直接失效，Settings 改用別名而非寫死版本號。同時 `--help` 記載 `--effort` 合法值為 `low|medium|high|xhigh|max`（原本用到不存在的組合）。
    - （歷史紀錄，已由本檔最上方 2026-08-14 App Server 實測結果取代）第一版當時誤判 Codex CLI 本機未安裝，並採用 `gpt-5.2-codex`／`gpt-5.1-codex-*`；這些模型現已確認 deprecated，不得再作為現行清單依據。
  - `ThinkingEffort` enum 的 `None` 改名為 `Minimal`（語意修正：真正的旗標值是「最低」而非「關閉」）並新增 `XHigh`。`CliCommandBuilder.AppendEffortArguments` 依上述實測結果重寫四個 CLI 各自的合法對應表；`AntigravityClaude` 設定檔的思考程度選項精簡為只有「預設」，因為它的三個模型沒有一個支援獨立的 `--effort`。
  - `AI 倒數喚醒.exe`（`AiWakeScheduler.WinForms`）本機同名程式已在系統匣安裝，且 computer-use 存取請求被使用者拒絕，因此**這次修正未經實際畫面截圖驗證**，僅以程式碼層級的 WinForms 版面配置原理與建置/測試結果佐證，不得宣稱已做視覺 QA。
- 驗證：Release 建置 0 警告 0 錯誤；重寫後 deterministic 測試 12/12 通過。尚待使用者實際開啟 Settings 視窗確認捲動與模型清單顯示正確。

## 2026-08-13 GitHub upload (complete)

- User authorized uploading the complete current verified worktree, including the WinForms redesign and installer-experience hardening. Scope is this repository only.
- Remote audit after `fetch --prune --tags` found local `main` and `origin/main` identical with no other pre-existing branches. Initial source commit `f76af8a` was pushed to `agent/ui-installer-experience`; Draft PR #2 targets `main`: https://github.com/nojackno2-ctrl/AI-Wake-Scheduler/pull/2
- GitHub connector PR creation returned `403 Resource not accessible by integration`; the authenticated host `gh pr create` fallback succeeded. A final handoff-only commit records the completed remote state on the same PR branch.
- This authorization does not include a GitHub Release, tag, installer execution, or installed-application update.

## 2026-08-13 installer experience hardening (complete, uncommitted)

- Scope is limited to the existing Inno Setup installer and its deterministic contract test; the concurrent uncommitted WinForms redesign remains preserved and untouched.
- The Start Menu application and uninstall shortcuts are now mandatory and carry a stable AppUserModelID plus `{app}` working directory. Desktop and Windows-startup shortcuts remain optional and default unchecked.
- Added previous-install preference reuse, explicit uninstall registration, setup/uninstall logging, no automatic application restart, Add/Remove Programs metadata, installed README/LICENSE files, and matching README guidance for manual Start/Taskbar pinning.
- Verification: deterministic tests passed 12/12, `dotnet format --verify-no-changes --no-restore` and `git diff --check` passed, self-contained publish succeeded, and Inno Setup 6.7.3 compiled `dist/AI倒數喚醒_Setup_v1.2.0_x64.exe` (48,591,174 bytes, SHA-256 `BD28FA5F9C327A6A7DBEA407C7480B4127B77CA227FB36807D334BA86BA0F68F`, file version 1.2.0). The first sandboxed canonical build was blocked only by denied access to the user NuGet.Config; the approved host rerun passed. The older self-maintained Traditional Chinese `.isl` still falls back to English for newer uncommon Inno messages, as reported by compiler warnings.
- No installer was executed, no installed application was updated, and no commit/push/release was performed.

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

## 2026-08-13 重複安裝唯讀檢查

- Windows 標準安裝位置只找到一份正式安裝：目前使用者的 `AI 倒數喚醒 v1.2.0`，
  位於 `%LOCALAPPDATA%\Programs\AI 倒數喚醒`；未找到全機或 x86 的第二份正式安裝。
- 登錄檔的 32/64 位元檢視顯示同一個 Inno Setup AppId 與同一路徑，並非兩套安裝。
- 專案輸出目錄另有 Debug、Release 與 publish 建置副本；這些不是 Windows 已安裝程式。
- 發現開機啟動來源重複且目標不同：Startup 捷徑指向正式安裝版 `1.2.0`，但 HKCU Run
  仍指向專案 Debug 版 `1.1.0`。檢查當下唯一執行中的程序也是 Debug 版（PID 22372）。
- 本次僅檢查，未停止程序、移除啟動項、刪除建置輸出或解除安裝。

## 2026-08-13 切換為正式版

- 依使用者授權移除 HKCU Run 中指向專案 Debug `1.1.0` 的舊自動啟動值；保留安裝程式建立、
  指向 `%LOCALAPPDATA%\Programs\AI 倒數喚醒\AI倒數喚醒.exe --minimized` 的 Startup 捷徑。
- 已停止 Debug 程序並啟動正式版。獨立複核只有一個 `AI倒數喚醒.exe` 程序（PID 18268），
  路徑為正式安裝目錄、FileVersion `1.2.0.0`、ProductVersion
  `1.2.0+7fad2ef64e57bad1cb073081ca201f351e654dad`，且 `Responding=True`。
- 未解除安裝、刪除專案建置副本或修改產品程式碼。

## 2026-08-13 WinForms UI 全面重構

- 新目標：在不變更功能、資料流、資訊架構與對外行為的前提下，重構主視窗與 CLI 設定視窗的完整視覺系統；不得增加 NuGet 相依。
- 設計審計完成：現況是深藍標題列、系統預設控制項與高密度雙欄工作區；排程清單、編輯器、額度、狀態列與設定流程均有功能價值，全部保留。待改善項目是控制項視覺不一致、表格層級偏弱、缺少空清單狀態、設定頁區塊界線不足，以及按鈕主要／次要／危險操作不易辨識。
- Design Read：Windows 開發者使用的排程工具，採冷靜、可信、偏 Fluent 的原生 WinForms 桌面語言。`DESIGN_VARIANCE=4`、`MOTION_INTENSITY=2`、`VISUAL_DENSITY=6`，模式為 `Redesign - Preserve`；設計技能不直接涵蓋原生產品 UI，因此僅吸收配色、字體、間距、形狀一致、完整狀態與可及性原則，以現有 WinForms 實作。
- UI 程式變更里程碑：`AppTheme` 已集中冷色中性色盤、單一藍色互動重點、字級、主要／次要／危險按鈕、輸入框、群組框與高對比模式退化策略。主視窗已重整標題、工作區間距、資料表表頭／列高／選取狀態、按鈕層級、空排程狀態、額度區塊與欄位可及名稱；設定視窗已分為 CLI 連線、啟動與資源、連線檢查三個清楚區塊。未變更事件、資料模型、排程流程或外部行為，也未增加相依。
- 第一輪 Release 建置已實際成功：0 警告、0 錯誤。尚待 deterministic tests、視覺 QA、格式與差異檢查。
- 最終驗證完成：Release build 成功（0 警告、0 錯誤）；deterministic tests 11/11 通過（四 CLI 平行測試 1213 ms）；`dotnet format --verify-no-changes --no-restore` 與 `git diff --check` 均通過。主要文字／按鈕配色的計算對比值分別為 5.38:1、6.91:1、9.74:1、6.44:1，均達 WCAG AA；另提供 Windows 高對比模式系統色退化。
- 視覺 QA 未完成：依 `computer-use` 技能初始化時仍被 `EPERM: lstat C:\Users\nojac\AppData\Local\OpenAI\Codex` 阻擋，沒有取得畫面；而目前正式安裝版仍在系統匣正常執行，為避免中斷排程，未停止正式版或啟動會撞單一執行個體鎖的工作區版本。不得宣稱已做實際畫面驗證。

## 2026-08-13 已安裝軟體更新驗證

- 以 `dist\AI倒數喚醒_Setup_v1.2.0_x64.exe` 靜默覆蓋更新目前使用者安裝；安裝程序結束碼為 `0`，未要求重新開機。
- 已安裝執行檔為 `%LOCALAPPDATA%\Programs\AI 倒數喚醒\AI倒數喚醒.exe`，FileVersion `1.2.0.0`、ProductVersion `1.2.0+f6d7415c7615e6d709e75b301e01921a8f8db56d`、SHA-256 `33FC819E4278B41C2D0B381E7447F456C95AFFFDA5F0D0AE49D1A35EFEDFF4F2`；與本次 `bin\publish-selfcontained` 產物逐位元相同。
- 已確認登錄的版本、安裝位置、解除安裝命令、專案／問題回報／更新網址正確；開始功能表的啟動與解除安裝捷徑均存在且指向目前安裝位置，既有開機啟動選項亦由安裝器保留。
- 從已安裝路徑以 `--minimized` 啟動後，PID `23496` 且 `Responding=True`。這是程序啟動與回應證據，不等同完整視覺或排程行為驗證。
- 本次使用者授權範圍是更新已安裝軟體；此交接紀錄尚未提交或推送。
