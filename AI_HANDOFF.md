# AI HANDOFF

## 2026-10-05 Codex：v1.10.0 發布（已授權，進行中）

- 使用者已明確授權 commit、push 並發布 v1.10.0；當前 main=febe4e9，remote main 同步，所有待提交變更為前輪功能更新及本次發布準備，未發現其他代理新增修改。
- 完成：低價預設 Gemini 3.8 Flash Low、AGY Sonnet 5.5 Low、Codex GPT-6 Luna、Claude Haiku 4.5；四目標經產品 Runner 真實回覆 OK、exit 0（wake 1/1）。官方價格及 AGY 完整扣額排名未公開的限制見 docs/MODEL_PRICING.md。
- 前輪最終 build-installer exit 0：核心 16/16、WinForms 7/7、額度整合 1/1（四目標各 2 視窗）、自包含 publish、Inno 編譯。Release 方案 0 警告／0 錯誤；格式與 diff check 通過。
- 隔離 QA 五階段 Install／UpgradeKeepStartup／UpgradeDisableStartup／UpgradeEnableStartup／Uninstall 全 PASS，正式設定／排程／Run key baseline 不變。免安裝 ZIP 解壓後 263 個 publish 檔案 hash 相同，版本及 SHA256SUMS 通過。
- 已解決嘗試：Claude 額度初次憑證過期，真實呼叫後重驗成功；舊 Codex 參數期望與視窗空白初值假設失敗已更新；漏加測試變數的編譯錯誤已修正；exe ProductVersion 的 commit 後綴及 Inno padding 已按 numeric version／Trim 校驗。
- 前輪 dist/v1.10.0-final 產物尚未發布，因 exe 帶舊 HEAD 後綴不能作正式來源證據。將提交程式碼後重新建置至獨立 dist/v1.10.0-release，使用 ASCII 資產檔名與實際檔名 SHA256SUMS，驗證來源提交及 GitHub 上傳內容後發布。
- 未實測：Windows 重開機登入、已有最高權限舊工作的遷移、真實系統匣通知外觀、所有候選模型推論。保留這些限制於發布說明。
## 2026-10-04 Codex：全部功能更新與桌面驗證（完成，驗證限制見下）

- 最新狀態（取代下列歷次待驗證描述）：模型、額度讀取、微量倒數、來源快取、終端保留、設定 footer、一般權限啟動與 QA 安裝生命週期均已完成更新並驗證。使用者已允許 QA 安裝及桌面操作；Install／保留啟動升級／啟用與停用啟動升級／Uninstall 全通過，正式資料及 Run key baseline 不變，QA 已解除安裝。最終完整 build-installer exit 0：核心 16/16、視窗 7/7，產出 bin/verification-build/AI倒數喚醒_Setup_v1.9.0_x64_20261004-090432.exe，SHA256 664C960A05AEA6E75EC984B291D6ECD81F45140BD11E4074EBD55A47DFA747B6。README 與 docs/FEATURE_VERIFICATION.md 已同步；未提交／發布／更新正式安裝。未重開機、未實測已有最高權限舊工作遷移與真實系統匣通知外觀，不能稱全情境端到端成功。
- 本輪 continuation：上一輪屬 progress（完成真實整合與打包）。使用者已明確允許「安裝驗證及桌面操作」。新增 VerificationBuild 安裝器模式，使用獨立 AppId／名稱／Run key／舊工作名稱，QA 啟動參數 --verify-ui 避免正式資料；編譯中。Computer Use list_windows 已恢復，此時未發現主程式視窗。待在 workspace/bin/verification-installed 實際安裝／升級／卸載；正式設定與排程 hash、Run key 狀態先保存在 ignored QA 目錄，供後驗。
- QA 兩次 Inno 編譯 exit 0；第二版 QA 捷徑加入 --verify-ui。新增 Verify-Installer.ps1，嚴格限制 QA ProductName／固定 workspace/bin 安裝根／獨立 AppId，四個生命週期階段均比對正式設定、排程及 Run key。首次安裝前因 Inno ProductName 固定 60 字且尾端空白被護欄拒絕（未安裝）；已驗證元資料並改 Trim 後比對，待執行。
- 真實首次安裝 exit 0，檔案、HKCU QA uninstall key、QA Run key 正確；第一個檢查因腳本假設 /GROUP 覆蓋 DisableProgramGroupPage 的預設群組而失敗，實際捷徑在「AI 倒數喚醒 QA」。已改檢查實際群組，不覆寫原 baseline。另發現 QA AppUserModelID 與正式版共用會干擾 app discovery，已分離為 nojackno2.AIWakeScheduler.QA，待重編／升級；目前未啟動正式程式。
- 實測 UpgradeKeepStartup／UpgradeDisableStartup 各 PASS：預設升級保留選擇，明確 /TASKS 取消後刪除 QA Run key；正式三項 baseline 均相同。QA 分離 AppUserModelID 版本編譯 exit 0，已升級並重新啟用 QA startup（PASS UpgradeEnableStartup），準備以 Computer Use 開啟註冊的 QA 捷徑。重開機未執行。
- QA 捷徑檔案實際 TargetPath 指向 verification-installed、Arguments=--verify-ui，內含新 QA AppUserModelID；sky list_apps 仍回舊 ID，從 returned app id 啟動失敗 ShellExecuteW=2。未猜測新的 app id 或操控正式版。改用產品已支援的命令列 --verify-ui 啟動已安裝 exe 做執行驗證，接著由 sky list_windows 重新選取實際隔離視窗。
- 新桌面證據：已安裝 exe PID 29860、主視窗「功能驗證」、工作目錄實際在 Temp/AiWakeScheduler-QA-*，四個額度看板均顯示真實剩餘量。原 Hidden QA PID 24708 已確認路徑後清理。真實滑鼠下拉選取 Sol＋Ultra→Luna 自動 Max，Luna effort 清單無 Ultra，之前 UIA edit.set_value 未觸發事件的疑慮已由實際選取解決。一次 popup 點擊被 helper 擋為非目標視窗，activate＋重新觀察後一次重試成功。設定目前未儲存，已點擊全部 CLI 檢查待結果。
- CLI 檢查按鈕實測全部 ✓：AGY 1.2.16、Codex 0.160.0、Claude 2.1.289。真實點擊「儲存」已返回主畫面；隔離 settings.json 實際 Codex Model=gpt-6-luna、ThinkingEffort=Max，正式資料未改。待排程 UI 與 QA 卸載清理驗證。
- 排程 UI 已實測：先取消啟用避免送出訊息，按建立排程後表格顯示已停用；隔離 JSON 實際 Daily、Enabled=false、4 個 targets。再透過下拉改自動模式與儲存修改，JSON 實際 Interval、Enabled=false、InitialTimeOfDay=08:49。未觸發真實 CLI 呼叫。待正常退出與 QA 卸載。
- 新問題：設定「縮到系統匣」取消後，底部儲存 UIA click 未生效，modal 與原 JSON 值仍在。實際畫面顯示儲存／取消被 AutoScroll 根容器裁切（只剩按鈕一部分）；正在改為獨立 DockBottom 按鈕列並加入小視窗／長探針結果 regression，避免僅以程序測試忽略可點擊性。這是實測發現的產品問題，不是已修復宣稱。
- 已修正 footer：儲存／取消移出捲動 root，改 DockBottom，根內容不與按鈕重疊。小視窗＋長探針結果＋水平／垂直極端捲動 regression 通過，WinForms 7/7、自包含 publish 成功。舊 QA 視窗以 Return 儲存 false 的 MinimizeToTray 後，真實關閉主視窗成功，PID 29860 已終止；準備重編 QA 安裝包並驗證新 footer。
- 舊 QA 真實 Uninstall PASS：QA exe、Start Menu 群組、HKCU uninstall key、QA Run key 均清除，正式 baseline 三項維持原值。尚未對新版 footer 做重新安裝與桌面驗證。
- Footer QA 編譯 exit 0，新版 fresh Install PASS，已重新啟動 --verify-ui 待桌面確認。format verify 發現 StartupManager 與 tests 中多敘述同列的 whitespace gate 失敗；僅對本次已修改的四個檔案执行 whitespace formatter，尚待最終 verify。
- 最新 footer 實測已完成：新 QA 設定視窗完整固定顯示儲存／取消；真實點擊儲存成功返回主畫面，隔離 JSON MinimizeToTray=false；正常關閉後 PID 5360 終止。新版 UpgradeKeepStartup、UpgradeDisableStartup、Uninstall 均 PASS，QA Windows 登錄／捷徑／exe 已清除，正式設定、排程與 Run key baseline 相同。全方案 whitespace verify／diff check 已通過。未實際重開機，已有最高權限舊工作的遷移尚未實測。

- 自動 goal continuation 的目標為「更新所有功能」。前一輪有實際程式變更與成功測試，屬於 progress；本轮重新讀取 Git、規則、handoff，現有修改均為本次 Codex 工作，未發現其他代理新增修改。
- Computer Use 技能初始化與 list_windows 可正常使用（舊版本 EPERM 限制現已不適用）。未發現本程式在執行，且目前使用者正式 schedules.json 不存在；未停止或改動既有排程。
- 實際開啟 Release framework-dependent exe 得到「You must install or update .NET」對話框；已關閉提示。這是缺少 .NET 8 Desktop Runtime，不是 WinForms 功能成功證據。改建置 bin/verification 自包含 win-x64 版本供桌面 QA，未安裝或覆蓋發行產物。
- 已更新一般權限啟動：manifest asInvoker、StartupManager 使用 HKCU Run key、既有最高權限工作遷移後才寫新鍵，遷移失敗會顯示警告；設定視窗僅在開機偏好變動時重寫啟動項目，AppHost 讀實際 Run key 狀態。安裝器 startupicon 改寫同一鍵並移除舊工作。這些更新已成功 self-contained publish，尚待啟動項目的測試。
- 新增 `--verify-ui` 隔離資料目錄／mutex 模式供桌面 QA（不執行正式啟動項目遷移）；首個 shell 啟動 PID 2324 已消失，未視為仍在執行。重新以 Computer Use 開啟標準權限 bin/verification-standard：主畫面與完整 accessibility tree 可操作，右側捲動後可見倒數與設定按鈕。原先 elevated 版只能取畫面而無法操作（舊視窗已不在 list_windows）。
- 主畫面新證據：Claude 查詢目前 HTTP 429，UI 明確顯示重試時間而非假額度；未重複查詢以免加重限制。其他目標尚待逐项 UI 檢查。首次一般版使用正式空資料目錄，尚未建立排程、改設定或開機項目。
- 一般權限版再次桌面啟動：完整 UIA tree、設定視窗及捲動區域已驗證；UIA set_value 修改 Codex 文字為 gpt-6-luna，但 effort 仍顯示 Ultra。尚未證明是使用者輸入路徑的事件錯誤或自動化設定底層 edit 未觸發 WinForms TextChanged；不能宣稱模型切換 UI 正確。新增直接 WinForms 事件測試來驗證此項與排程 CRUD／設定儲存。当前設定視窗仍為未儲存的測試變更，正式資料未改。
- 新增零 NuGet WinForms 測試專案與 InternalsVisibleTo，self-contained publish 後實際執行 5/5 通過：模型 Sol Ultra→Luna Max、完整模型 effort 抑制、真實儲存事件與取消隔離、独立倒數文字、實際新增／修改／持久化排程、HKCU 開機開關（測後還原原鍵）。UIA set_value 異常未在 ComboBox.TextChanged 重現。同名 ApplicationConfiguration 警告未被命名空間消除，改用明確 WinForms 初始化呼叫，待重建。
- 新里程碑：AGY 額度快取加入 executable／工作目錄／自動查詢模式，避免設定變更沿用其他來源；清理 StartupManager 未讀 stdout 的潛在阻塞。安裝器保留原啟動偏好、取消勾選會移除 Run key、清理舊工作後才啟動一般權限程式。建置腳本取消強制終止程序與刪除舊安裝包，納入 WinForms 測試並產出時間戳檔名。已找到本機 Inno Setup 編譯器，待編譯新版 QA 安裝包（不安裝）。
- 新驗證：self-contained publish 已無 ApplicationConfiguration 警告；核心 15/15、視窗 5/5 通過。Inno Setup 指向 bin/verification-final、輸出 bin/verification-installer/Verification-20261004.exe，exit 0（未安裝）。新增跨 CLI 來源快取 regression 通過；扩展視窗測試 5/6，系統匣關閉／還原、登入連結繪製／防重複點擊通過，終端長輸出保留測試失敗，正在診斷。開機鍵測試改為 --startup-integration opt-in，避免一般建置更動 Windows 啟動設定。
- 終端失敗已解決：210064 字長輸出超過上限，RichEdit 唯讀狀態拒絕 SelectedText 刪除；建立 native handle 仍重現。修正為同步刪除時暫時可寫、finally 還原唯讀，測試 6/6 通過。核心 16/16（含跨來源快取）通過。最新整合 Codex 2 視窗成功，Claude 本機憑證過期導致 0/1，尚未重新登入；不把之前成功當作目前全部可用。尚需最新 publish／安裝器 QA 包同步這項終端修正。
- 補充：claude auth status 的非敏感欄位為 loggedIn=true、authMethod=claude.ai，但額度 HTTP 401 仍拒絕舊憑證，已透過 async 問使用者完成 claude auth login 後回覆。讀取器不持有或寫入 refresh token。另修正微量 Claude 用量在快取更新時倒數被 rounded 0% 清除的問題，新增 regression；--verify-ui 設定不再寫正式 Windows 啟動鍵。打包腳本可指定 OutputDirectory，個別 SHA256 檔不覆寫舊 checksum，待完整腳本驗證。
- 最新結果取代以上待驗證狀態：使用者回覆「已登入」後 `--integration` 1/1 通過，Codex／Claude／AGY 兩池各 2 視窗。完整 `build-installer.ps1 -OutputDirectory ./bin/verification-build` exit 0，包含核心 16/16、視窗 6/6、自包含 publish、Inno Setup 編譯与 SHA256；測試包 AI倒數喚醒_Setup_v1.9.0_x64_20261004-083148.exe。最終方案 Release build 0 警告／0 錯誤，diff check 通過。未安裝／升版／提交／發布。
- 使用者按實體 Escape 停止 Computer Use，工具明確要求停止；本輪未再呼叫桌面工具。尚未完成完整人工 UI 輸入、真實系統匣通知、重開機與安裝／解除安裝；先前未儲存測試設定視窗不再操作。完整已驗證／未驗證清單見 docs/FEATURE_VERIFICATION.md；不得宣稱全部端到端功能已驗證。

## 2026-10-04 Codex：更新三個 CLI 的模型與倒數讀取（完成）

- 使用者要求「更新三個CLI的模型」；開始時 main 工作樹乾淨，最近提交 febe4e9（v1.9.0）。Git 因搬移後擁有者 SID 不同拒絕存取，使用每次命令 `-c safe.directory=D:/Github/AI-Wake-Scheduler`，未改全域設定。
- 唯讀核對：`agy models` 成功，Gemini 清單仍為 3.8/3.7/3.6 Flash、3.1 Pro；Claude 池已換成 Sonnet/Opus 5.5 各 low/medium/high 完整 ID，GPT-OSS 保留。舊 4.6 ID 不在清單。
- 本機 Codex app-server `initialize` → `model/list` 成功（nextCursor=null）：GPT-6.1 Sol、6 Astra、6 Sol 支援 low～ultra；6 Luna 支援 low～max；另有 5.6 Sol/Terra/Luna、5.5。移除清單與 effort 表中的 5.4 系列，舊設定移到 6.1 Sol／6 Luna。注意 app-server 可能是快取目錄，未宣稱每個推薦模型已實際推論。
- Claude `--help` 確認 sonnet/opus/fable 別名與 low～max；官方 https://code.claude.com/docs/en/model-config 確認 Sonnet/Opus 5.5、Fable 5.1 完整 ID 及 effort。Codex 官方 https://developers.openai.com/api/docs/models/all 核對新系列。
- 已更新 CliCatalog：Codex 新清單與 effort 上限；AGY Claude 改用含程度後綴完整 ID、預設 Sonnet 5.5 Low；Claude 保留別名並加入完整版本。Gemini 現行清單核對後無需變動。AGY 舊 Sonnet／Opus 4.6 設定轉換為新 ID，自訂模型保留。
- 使用者追加「倒數讀取也要更新」。初輪 `dotnet run ... -- --integration`：Codex 帳戶額度與 AGY 兩池（各 5h/7d）成功；自動拉起路徑失敗（連線拒絕）。新證據：本次 `agy models` 日誌在 doRefreshQuota 啟動同時關閉服務，舊 models 代管方式已失效。
- 已淘汰的第二個嘗試：串流 stdin 保持開啟可讓服務存活，但 RPC 仍回 HTTP 401 missing CSRF token。該程式碼已移除，不再重試；歷史 9/2 的 models 解法現在也已過時。
- 第二輪整合：Codex、Claude、既有 AGY 兩池都可讀；新串流啟動方式保持存活但 RPC 回 HTTP 401 missing CSRF token，尚未完成。Release build 0 警告／0 錯誤。使用者追加要求所有功能驗證、刪除過時項目。新版 `agy changelog` 明列內建 `/usage`（與 9/2 當時無此命令的版本不同），開始驗證新內建查詢，不沿用舊失敗結論。
- 新內建 `agy --output-format json --print /usage` 實測：command.name=usage、num_turns=0、total_tokens=0，兩池各 5h/7d，已直接採用此 JSON 資料（groups[].name、buckets[].name/remaining_fraction/reset_time）。移除 AntigravityLauncher 與代管 RPC 重試；關閉自動啟動時仍保留已開 IDE 的 RPC 查詢。過去 models／串流啟動嘗試均已被這個方案取代。
- `--integration` 1/1 通過，Codex/Claude/AGY 兩池各 2 個額度視窗；完成微量用量修正後再次通過。新增 opt-in `--wake-smoke`，經產品 CliRunner 平行喚醒 Gemini 3.8 Flash、AGY Sonnet 5.5 Low、Codex GPT-6.1 Sol、Claude Sonnet 5.5：四者 exit 0 並回覆 OK，1/1 通過。測試程式初次用錯 CliRunResult 欄位與 JSON raw string 括號各造成編譯失敗，均已修正後重新建置通過。
- Release build 0 警告／0 錯誤；最終 deterministic 15/15（含微量使用、滿額滑動時間、旧設定轉換、平行執行與排程邊界）；format verify 與 diff check 通過。微量使用即使畫面四捨五入為 0% 仍依原始用量啟動倒數。README 與設定 UI 已同步；失效 Launcher 已刪除，過時歷史筆記移到 docs/AI_HANDOFF_HISTORY.md 完整保留。
- 未實測 WinForms 點擊、系統匣通知、開機登入、重新登入瀏覽器與安裝／解除安裝；不能宣稱全部功能經端到端驗證。未升版、打包、改已安裝程式、commit、push 或發布。

## 目前狀態與限制

- 最近正式發布為 v1.9.0（febe4e9）；本輪完成 QA 打包及隔離安裝／升級／卸載，尚未升版、更新正式安裝、commit 或 push。
- 核心：.NET 8、WinForms、繁體中文、無額外 NuGet；四個目標平行喚醒，自動排程依各 CLI 的真實五小時額度倒數。
- 一般權限 manifest／HKCU 啟動與安裝器遷移已更新，安裝／升級／解除安裝已通過；此機器無舊排程工作，尚未實測已有最高權限工作的遷移與重開機登入。
- 舊紀錄已完整移至 [歷史交接紀錄](docs/AI_HANDOFF_HISTORY.md)，包含版本發布證據、已解決問題、失敗嘗試與 UI QA 限制。
