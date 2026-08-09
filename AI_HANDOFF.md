# AI HANDOFF

## 目前目標

已完成 C# Windows 桌面程式及首次 GitHub 上傳；後續維護以 `main` 與私人遠端儲存庫為準。

## 目前狀態（2026-08-10）

- 起始工作目錄為空，且尚未初始化 Git。
- 2026-08-10 使用者明確要求上傳 GitHub；本機 `gh 2.95.0` 已登入 `nojackno2-ctrl`，token 具 `repo` 與 `workflow` scope。
- 上傳前敏感字串掃描未發現 API key、token 或密碼；`nojackno2-ctrl` 帳號下未找到同名或近似名稱的既有儲存庫。
- 已初始化 Git `main`，提交 `75fa303`（`Initial AI wake scheduler`），並建立私人儲存庫 `https://github.com/nojackno2-ctrl/AI-Wake-Scheduler`；首次推送成功。
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
