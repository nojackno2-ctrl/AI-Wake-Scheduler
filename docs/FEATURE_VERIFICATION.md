# 功能驗證紀錄

本檔記錄純 Rust 架構（Cargo workspace：`core/` 核心函式庫、`app/` eframe/egui 桌面程式與 Win32 系統匣）的驗證結果。2026-10-16 以前以 C#／WinForms 實作的驗證紀錄保留在文末「歷史紀錄」，其中的 `dotnet build`、WinForms 測試及自包含 publish 已不適用於現行程式碼。

## 2026-10-23 Codex：核心與 egui 重構後檢查

- 起點：`main / 1bc7b80`，工作樹乾淨。檢查 core 全部模組與 app 的排程操作、設定、額度、終端機、背景工作、系統匣及 Windows 共用函式；同步檢查 Visual Studio、建置／安裝腳本與文件。
- 可重現問題與修正：
  - 儲存失敗仍發布使用者編輯、刪除或準備執行的 Running 狀態：改為在鎖內建立候選資料，持久化成功才發布；偵測與重置快取也只在成功修改後清除。設定採相同順序。已實際執行的完成結果則保留於記憶體並重試持久化，詳見下方複核補強。
  - 手動執行不受關閉取消控制：共用管理器停止訊號；程序啟動前檢查取消，程序等待錯誤路徑也終止並回收直接子程序。
  - 同筆排程重複目標導致手動平行呼叫兩次：載入與儲存時保留順序去重；原測試實際重現 2 次，修正後 1 次。
  - 已用盡額度缺少有效重置時間仍放行：持續阻擋，直到讀到可用額度。Claude HTTP 429 與跨越重置的快取不再授權喚醒，保留真正觀測時間，跨重置直接重新查詢。
  - 子程序輸出無界佇列及無界收尾：改為 64 個片段的背壓佇列與有限收尾；管線提前關閉時避免忙碌迴圈。egui 最多排隊 256 個輸出片段，每次 tick 最多處理 256 個事件；保留開始與完成事件的順序，過量畫面片段提供略過提示。
  - 日誌只在啟動時清理一次：隔離測試寫入 240 筆後原有 241 個檔案；改為首次及每 32 次寫入清理，執行日誌約保留 200–231 筆，除錯日誌另依既有天數政策保留。
  - 額外參數的空引號遭丟棄：保留空字串參數。極端時間長度、Codex 視窗長度與 Retry-After 改用可失敗的換算，避免溢位 panic。
- 回歸證據：首批 5 項測試在修正前全數失敗；重複目標與日誌保留測試亦先重現失敗。最終新增 18 項測試，並擴充既有 UI 測試，以失敗儲存替身、可控時鐘、記憶體執行器、假 CLI 與暫存目錄驗證。
- 最終本機驗證：依序 `cargo check --workspace --locked`、`cargo test --workspace --locked`、`cargo build --workspace --locked` 通過；61 項成功（app 17、UI 2、core 單元 1／整合 41），4 項選用測試預設略過。`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`git diff --check` 與 Visual Studio Debug 方案建置通過。
- 邊界：未送出真實模型訊息、未變更正式安裝與正式資料、未執行全天掛機或資源用量基準測試。程序樹仍使用 Windows 快照清理，未驗證持續衍生孫程序的競態；安裝 QA 與真實唯讀 CLI 查詢另列下方。未發布版本。

### 2026-10-23 Codex：主代理複核補強

- 批次執行：`.cmd`／`.bat` 改用 `Command::new(executable).args(...)`，交由 [Rust 1.99 的批次專用跳脫](https://github.com/rust-lang/rust/blob/1.99.0/library/std/src/sys/args/windows.rs) 處理；移除一般執行路徑的自訂 `raw_arg`。節省 Token 附加提示改為單行，保留使用者訊息；批次參數含 CR/LF 時回報明確錯誤，原生 `.exe` 保留多行內容。
- 主控台：登入／更新仍需 `|| pause`，因此保留受限制的 shell helper；所有值加引號、使用 `/e:ON /v:OFF /d /s /c`，明確拒絕路徑或參數中的雙引號、百分比、CR/LF 與 NUL。這項輸入限制僅適用登入／更新主控台。
- 完成存檔：執行結果與完成時間不再因存檔失敗回復成 `Running`；發布正確完成狀態、通知 UI、回報背景錯誤並保留待寫入標記。排程迴圈每秒重試，恢復後先持久化再繼續掃描；儲存錯誤不再永久終止排程執行緒，也不再以第二次空結果收尾覆寫實際結果。
- 設定補償：UI 改用可注入啟動設定替身的套用入口。設定存檔失敗會還原原啟動偏好；補償失敗則合併回報原始存檔錯誤與補償錯誤，記憶體設定仍保留原值。隔離模式不呼叫真實啟動設定。
- 實際回歸：`regression_batch_arguments_round_trip_without_injection` 以 `.cmd`／`.bat` 轉呼叫假原生 CLI，讀回 JSON 驗證雙引號＋`&` 注入載荷、已定義的 `%AIWAKE_FAKE_VAR%` 與 `!AIWAKE_FAKE_VAR!`、`^`、空白、空參數、尾端反斜線、中文，以及各 CLI 預設節省 Token 參數；額外命令標記檔均未建立。`regression_batch_rejects_multiline_but_native_preserves_it` 驗證 CR、LF、CRLF；主控台測試實際執行含特殊字元路徑並驗證不安全輸入被拒絕。
- 持久化回歸：可切換失敗 store 與受控 runner 在完成當下故意失敗，確認至少三次儲存嘗試仍保留相同完成結果；恢復儲存後無須新操作即可寫回，並確認下一刻度自動排程再次執行。設定測試注入啟動替身，覆蓋補償成功、補償失敗、首次啟動設定失敗、恢復後成功、未變更與隔離分支，均未操作真實 Run key。
- 取消回歸：`regression_manual_shutdown_cancels_fake_cli` 等待假 CLI 寫出 ready marker 後才 shutdown，驗證兩秒內完成取消、結果為「執行已取消。」且未產生完成輸出。
- 已執行：`cargo check -p aiwake-core -p aiwake-app --locked --offline`、`cargo test -p aiwake-core --test core_tests --locked --offline regression_ -- --test-threads=1`（13 通過）、`cargo test -p aiwake-app --lib --locked --offline host::tests::`（3 通過）、分別以 `--exact` 執行 `cli_runner_safe_arguments`／`cli_command_builder`／`cli_usage_reader`（各 1 通過）、`cargo build -p aiwake-core -p aiwake-app --locked --offline`；`rustc -vV` 確認 1.99.0／`x86_64-pc-windows-msvc`，無 MSVC 連結錯誤。
- 格式檢查：`cargo fmt --all -- --check` 通過。
- UI 裁切：700×620 視窗編輯名稱後按儲存，原先會先被裁切外的排程卡片點擊判定覆寫成舊值；改用 `container_contains_pointer()`。回歸實際操作最小視窗的文字輸入、儲存、設定開啟與取消，讀回 JSON 名稱確認成功；8 張 wgpu 擷圖位於 `target/audit-ui-shots`，已檢視儀表、最小視窗與設定。
- 驗證修正：fmt 揪出主代理 UI 修正的換行；Clippy 揪出測試模組應置底與條件內等待的空迴圈樣式，已整理並重跑完整 check／test／build 與 Clippy 通過。

### 本機、打包與檢查範圍

| 範圍 | 本輪證據 |
| --- | --- |
| 核心排程、時間、資料模型與儲存 | 可控時鐘、跨日／時段邊界、重置／冷卻／去重、遷移及失敗持久化回歸 |
| CLI 目錄、定位、參數、程序、登入、更新、額度與 HTTP | 原生／批次假 CLI 往返與取消、額度解析／快取、五目標真實唯讀查詢及三 CLI 版本檢查 |
| 路徑、Win32 輔助、錯誤、日誌、匯出與假 CLI | 現行原始碼複核、日誌／匯出／容量回歸、真實版本探針與隔離執行 |
| app 組裝、編輯器、設定、儀表、終端、主題、控制項、呈現 | 單元測試、egui 真實操作／wgpu 擷圖、最小尺寸存檔與設定回歸 |
| 原生對話框、系統匣、啟動與程式入口 | 原始碼複核、隔離 Debug／Release 視窗生命週期；Run key 設定補償以替身測試 |
| Visual Studio、建置／安裝與文件 | MSBuild Debug 通過，4 份 PowerShell 語法與專案 XML 檢查，Release 建置、五階段安裝 QA，README 預設模型與參數限制同步 |

- 真實唯讀：`cargo test -p aiwake-core --test core_tests --locked executable_locator_resolution -- --ignored --exact --nocapture` 與 `cli_update_check_real` 均通過。五個目標皆可讀取實際額度；AGY Gemini／Claude-GPT 每週用盡，未呼叫模型，也未更新任何 CLI。
- Release：`powershell.exe -NoProfile -ExecutionPolicy Bypass -File ./build-release.ps1 -SkipTests -OutputDirectory dist/audit-20261023` 通過。執行檔 9,852,416 bytes，版本 2.2.0，SHA256 `14f027b7714fe85f11c884a17f32e1796e542fdb79fbc3f65e1c833331a804e0`；隔離最小化啟動、系統匣還原、關閉縮回與正常結束（exit 0）通過。
- 安裝 QA：`build-installer.ps1 -SkipTests -VerificationBuild -OutputDirectory dist/audit-20261023/qa` 通過；`tests/Verify-Installer.ps1` 的 Install／UpgradeKeepStartup／UpgradeDisableStartup／UpgradeEnableStartup／Uninstall 全通過，每階段確認正式設定、排程與啟動值不變。另執行 Install／SHA256 比對／Uninstall，確認實際安裝後執行檔與上述 Release 雜湊一致；產物附 `SHA256SUMS.txt`。清理前已確認建置目錄在工作區內且不是連結，並備份既有 publish／生命週期證據至 `dist/audit-20261023/previous-*`。
- 探針除錯：Release 臨時 PowerShell 探針先因 `IntPtr.Zero` 布林轉換與未等待視窗初始化而誤判；改為明確比較零指標、等待隱藏狀態後，完整生命週期通過。這是探針修正，未據此改動產品。
- 遠端驗證：程式碼提交 `613e2ada9149276e1e9562f2cfc269a2061cf5bd` 的 [CI 39938772825](https://github.com/nojackno2-ctrl/AI-Wake-Scheduler/actions/runs/39938772825) 已完成，UTC／台北／印度／太平洋／澳洲東部五時區全部成功，五個 Test 步驟均為 success。先前起點 `1bc7b80` 的 Pacific 工作為 hosted runner lost communication，未取得失敗測試日誌，不將該基準錯誤歸因於測試。後續收尾僅更新驗證文件與 README 日誌容量說明，產品程式碼不變。

## 現行驗證指令

| 項目 | 指令 | 說明 |
| --- | --- | --- |
| 格式 | `cargo fmt --all -- --check` | 依 `rustfmt.toml` |
| Lint | `cargo clippy --workspace --all-targets --locked -- -D warnings` | 警告視為錯誤 |
| 單元與整合測試 | `cargo test --workspace --locked` | 預設不呼叫真實 CLI |
| 真實 CLI 測試（opt-in） | `cargo test -p aiwake-core --test core_tests -- --ignored` | 需本機已安裝並登入 CLI，會消耗少量 Token |
| 單一執行檔 | `./build-release.ps1` | 產出 `dist/AI倒數喚醒.exe` 與 SHA256 |
| 安裝程式 | `./build-installer.ps1`（QA 用 `-VerificationBuild`） | 先跑測試，再以 Inno Setup 打包 |
| 安裝生命週期 | `./tests/Verify-Installer.ps1` | 獨立 QA 身分，不動正式設定 |

CI（`.github/workflows/ci.yml`）在 windows-latest 以釘選的 Rust 1.99.0，於 UTC、台北、印度、太平洋、澳洲東部五個時區矩陣執行 fmt、clippy、build、test。

## 2026-10-23 v2.3.0 發布檢查（Claude）

- 版本 2.3.0（`Cargo.toml`、`Cargo.lock`、`.iss`、README 下載檔名）。
- fmt、clippy `--all-targets -D warnings` 通過；`cargo test --workspace --locked` 62 通過、4 opt-in 略過。
- `build-installer.ps1 -SkipTests -OutputDirectory dist\v2.3.0`（獨立 `powershell.exe -File`）：安裝程式 5,118,653 bytes，SHA256 04a5c614…108e；執行檔 9,854,976 bytes，FileVersion／ProductVersion 2.3.0，SHA256 8be78840…c752。免安裝 ZIP（d42a8978…59dc）內僅 `AI倒數喚醒.exe`（9,854,976 bytes）與 LICENSE。
- Release 執行檔以 `--verify-ui` 啟動 8 秒仍在執行，除錯日誌記錄「啟動 v2.3.0」；隔離 QA 資料夾已清除。
- 未驗證：本版安裝包生命週期 QA（`tests/Verify-Installer.ps1`）未重跑；新額度卡未以真實額度目視檢查。

## 2026-10-22 v2.2.0 發布檢查（Claude）

- 版本 2.2.0（`Cargo.toml`、`Cargo.lock`、`.iss`、README 下載檔名）。
- fmt、clippy `-D warnings` 通過；`cargo test --workspace --locked` 30 通過、4 opt-in 略過（暫存 CARGO_TARGET_DIR）。
- `build-installer.ps1 -SkipTests -OutputDirectory dist\v2.2.0`（以獨立 `powershell.exe -File` 執行；在 PowerShell 5.1 工作階段內以 `&` 呼叫時，cargo 的 stderr 進度會被當成錯誤而中止）：安裝程式 2,441,426 bytes，ProductVersion 2.2.0.0，SHA256 6ef96e03…ad75；執行檔 1,842,176 bytes，FileVersion／ProductVersion 2.2.0。免安裝 ZIP（f16a1f00…e7bc）內僅 `AI倒數喚醒.exe`（1,842,176 bytes）與 LICENSE。
- Release 執行檔以 `--verify-ui` 啟動 8 秒仍在執行，除錯日誌記錄「啟動 v2.2.0」並讀取真實額度。
- 未驗證：安裝包生命週期 QA（`tests/Verify-Installer.ps1`）本版未重跑；系統匣更新通知與更新對話框未實際點擊。

## 2026-10-21 v2.1.0 發布檢查（Claude）

- 版本 2.1.0（`Cargo.toml`、`Cargo.lock`、`.iss`、README 下載檔名）。
- fmt、clippy `-D warnings` 通過；`cargo test --workspace --locked` 29 通過、3 opt-in 略過（因 `target\debug` 執行檔被佔用，以暫存 CARGO_TARGET_DIR 執行）。
- `build-installer.ps1 -SkipTests`（測試已先行執行）：安裝程式 2,438,078 bytes，ProductVersion 2.1.0.0；執行檔 1,830,400 bytes，FileVersion／ProductVersion 2.1.0，SHA256 C216D5FD…1576。免安裝 ZIP 內僅 `AI倒數喚醒.exe`（1,830,400 bytes）與 LICENSE。
- Release 執行檔以 `--verify-ui` 啟動，除錯日誌記錄「啟動 v2.1.0」。
- 未驗證：安裝包生命週期 QA（`tests/Verify-Installer.ps1`）本版未重跑；自動模式真實長時間運作；系統匣匯出項目。

## 2026-10-17 v2.0.0 發布檢查（Claude）

- 版本 2.0.0（`Cargo.toml`、`.iss`；HTTP User-Agent 與 Codex app-server clientInfo 改取 `CARGO_PKG_VERSION`）。
- `build-installer.ps1`：測試 28 通過、3 opt-in 略過；正式安裝程式 2,425,162 bytes，ProductVersion 2.0.0.0；執行檔 1,798,144 bytes，FileVersion／ProductVersion 2.0.0，兩次建置 SHA256 相同（F7E98974…6CF8）。
- 安裝程式新增 `[InstallDelete]`，清除 v1.x .NET 自包含版殘留。QA 安裝包五階段全 PASS；在 Install 後放入模擬殘留（DLL、PDB、deps.json、runtimeconfig.json、createdump.exe、`zh-Hant\`），UpgradeKeepStartup 後全部移除，只留 exe、ico、LICENSE、README 與解除安裝程式；解除安裝後 QA 目錄為空。
- 免安裝 ZIP 解壓後執行檔 SHA256 與建置產物相同。
- 未驗證：以真正的 v1.10.0 安裝版升級、重開機自動啟動、系統匣通知外觀、Argon 以外 CLI 的 v2.0.0 真實喚醒。

## 2026-10-17 AGY Argon 獨立目標（Claude）

- 新增 `CliKind::AntigravityArgon`（顯示「Antigravity (Argon)」），預設 `gemini-4-argon`，畫面排在 Gemini 之後；列舉值加在最後以保留舊版整數序號。
- `cargo fmt --check`、`cargo clippy -D warnings` 通過；`cargo test --workspace --locked` 28 通過、2 項 opt-in 忽略。
- 新增測試：Argon 指令組成與 `--print` 位置、Argon 目錄與 effort、Gemini 不再列出 Argon、Gemini／Argon 額度池分流（含 Argon bucket 放在 Gemini 群組內的格式）、舊設定檔補上 Argon 設定、UI 預設勾選與執行數量依目錄計算。
- 真實唯讀額度（`--ignored executable_locator_resolution`）：Gemini 2 視窗（5h、每週）、Argon 1 視窗（每週）、Claude / GPT 2 視窗、Codex 與 Claude 各 2 視窗，全部可讀。
- 真實喚醒（使用者同意後執行 opt-in `real_argon_wake_smoke`）：經 `ProcessCliRunner` 實際呼叫 `agy --model gemini-4-argon --effort low --disable-slash-commands --print-timeout 180s --print "只回 OK…"`，exit 0、回覆 OK；隨後重讀額度，Argon 每週視窗 `is_active_countdown=true`，重置時間由 2026-10-24T00:37:57Z 變為 00:39:51Z（呼叫當下＋7 天），剩餘仍四捨五入為 100%。

## 2026-10-17 專案健康檢查（Claude）

- 提交：`d6b6180`，本機 rustc 1.99.0（與 CI 相同）。
- `cargo fmt --all -- --check`：通過。
- `cargo clippy --workspace --all-targets --locked -- -D warnings`：通過。
- `cargo test --workspace --locked`：28 通過、2 項 opt-in 依設計忽略、0 失敗（app 單元 10/10、UI 端對端 1/1、core 17/17）。
- GitHub CI：`d6b6180` 五時區矩陣成功。
- 未重跑：`build-release.ps1`、`build-installer.ps1`、`Verify-Installer.ps1`、opt-in 真實 CLI 測試。

## 2026-10-16 純 Rust 重構驗證（Antigravity）

- fmt、clippy（0 警告）通過；`cargo test --workspace --locked` 28/28 通過，2 項 opt-in 忽略。
- `build-release.ps1` 產出 `dist/AI倒數喚醒.exe`，SHA256 校驗正常。
- `build-installer.ps1` 產出 `AI倒數喚醒_Setup_v1.10.0_x64_<時間戳>.exe`（2.31 MB，C# 自包含版為 48.4 MB）。
- `Verify-Installer.ps1`：Install、UpgradeKeepStartup、UpgradeDisableStartup、UpgradeEnableStartup、Uninstall 全 PASS；正式設定、排程與 Run key baseline 不變。
- 修正測試競態：`ui_smoke.rs` 假 CLI 完成判定改用標記檔；`core_tests.rs` 並發下的行程啟動延遲容許值調寬。

## 功能覆蓋對照

| 功能 | 自動化測試 | 結果與限制 |
| --- | --- | --- |
| 參數切分與安全參數 | `core_tests::argument_tokenizer`、`cli_runner_safe_arguments` | 通過 |
| 各 CLI 指令組成、模型與 effort | `core_tests::cli_command_builder`、`cli_catalog` | 通過；未逐一實測每個候選模型的推論 |
| 額度讀取與倒數解析（含 Gemini／Argon／Claude-GPT 三個 AGY 額度池分流） | `core_tests::cli_usage_reader`、`antigravity_usage_source_cache`、opt-in `executable_locator_resolution` | 通過；2026-10-17 真實唯讀額度五個目標皆可讀 |
| 排程計算（每日、跨日、DST） | `core_tests::schedule_calculator`、CI 五時區矩陣 | 通過 |
| 排程管理：到期執行、重啟不重複、逾期合併、邊界 | `core_tests::schedule_manager_due_job`、`_restart_does_not_refire`、`_coalesces_overdue_jobs`、`_boundaries_and_state` | 通過（可控時鐘） |
| 自動間隔、依額度調整、每日自動倒數模式 | `core_tests::schedule_manager_auto_interval`、`_quota_aware_interval`、`_adaptive_wait`、`_countdown` | 通過（可控時鐘）；未實測全天真實 CLI 運行或休眠恢復 |
| JSON 持久化 | `core_tests::json_file_store` | 通過 |
| 安裝程式約定 | `core_tests::installer_contract` | 通過 |
| 執行檔定位、真實喚醒 | `core_tests::executable_locator_resolution`、`real_cli_wake_smoke`、`real_argon_wake_smoke`（opt-in） | 2026-10-17 唯讀額度與 Argon 單獨喚醒通過；其他 CLI 的真實喚醒本次未執行 |
| 額度看板文字、倒數與更新時間 | `board` 單元測試 4 項 | 通過 |
| 排程清單呈現與倒數欄 | `presenter` 單元測試 4 項 | 通過 |
| 開機啟動指令引號、主控台輸出解碼 | `startup` 單元測試 2 項 | 通過；未實測重開機登入 |
| 主視窗編輯器、建立／修改排程、縮到系統匣、設定對話框（取消／儲存／模型 effort 連動）、連線檢查、立即執行與終端機輸出 | `app/tests/ui_smoke.rs`（以 `--verify-ui` 隔離模式啟動真正執行檔，跨行程操作控制項並檢查隔離 JSON） | 通過；未驗證真實系統匣圖示點擊與通知外觀 |
| 安裝、升級、解除安裝 | `tests/Verify-Installer.ps1` | 2026-10-16 通過；本次未重跑 |

## 未驗證項目

以下項目尚無自動化或近期手動驗證，不能視為端到端成功：

- Windows 重開機後自動登入啟動。
- 真實系統匣圖示點擊與通知外觀。
- 自動倒數模式整日真實 CLI 運行、電腦休眠後恢復。
- 每個候選模型的實際推論；純 Rust 版尚未重跑 opt-in 真實 CLI 測試。
- 純 Rust 版已安裝程式的桌面手動操作（C# 時期曾以 Computer Use 驗證）。

---

## 歷史紀錄（C#／WinForms 實作，2026-10-16 前）

以下內容原文保留，描述的是已移除的 C# 實作；其中的方案檔、WinForms 測試數量與安裝包大小不適用於現行版本。

### 2026-10-07 自動倒數模式

- Release 方案建置：0 警告／0 錯誤；核心 17/17、WinForms 7/7，格式與 diff check 通過。
- 新 Countdown 模式：可控本機時鐘／假額度來源／CLI 驗證每日開始前不查不呼叫、開始時只呼叫未倒數的 CLI、等待實際倒數結束、多輪循環、午夜停止與隔日恢復、全域刷新關閉仍偵測、讀取失敗保護、冷卻防連發、JSON 持久化、重啟保留倒數及查詢跨午夜停止呼叫。
- WinForms 實際控制項與儲存事件驗證第三個模式選項、開始時間標籤、模式持久化、每日視窗及各 CLI 倒數文字。
- 未實測新模式全天真實 CLI 運行、休眠恢復或新版安裝；不把可控時鐘測試當成全天端到端驗證。

### 2026-10-05 v1.10.0 發布檢查

- 核心 16/16、WinForms 7/7；Release 方案建置 0 警告／0 錯誤；whitespace verify 及 diff check 通過。
- 最終低價預設四目標喚醒均 exit 0 且回覆 OK（Gemini 3.8 Flash Low、AGY Sonnet 5.5 Low、Codex GPT-6 Luna、Claude Haiku 4.5），wake smoke 1/1 通過。
- 額度整合初次因 Claude 憑證過期失敗；真實呼叫後重新檢查 1/1 通過，四個目標各 2 視窗。
- 最終正式自包含安裝包建置成功；低價版 QA Install、UpgradeKeepStartup、UpgradeDisableStartup、UpgradeEnableStartup、Uninstall 全 PASS，正式設定／排程／Run key 不變。免安裝 ZIP 解壓後所有 publish 檔案 hash 相同；正式安裝器版本與 SHA256SUMS 校驗通過。產物位於 dist/v1.10.0-final，已依授權提交、推送，從 f060c30ade8a3f19207c303a0be2dce7304441a2 完整重建至 dist/v1.10.0-release，核心 16/16、WinForms 7/7、額度整合 1/1 再次通過；exe ProductVersion 含相同提交。GitHub v1.10.0 已正式發布，latest／tag／三項資產 SHA256 digest 與大小均確認。大型資產下載初期長時間顯示零位元組，但 helper 隨後自行成功結束；三項完整下載與本機 SHA256 相同，下載校驗檔逐行通過，未實際取消程序。GitHub API 官方 SHA256、本機解壓比對及完整下載三者均通過。

### 2026-10-04 歷史 QA 紀錄

2026-10-04，Codex；已在獨立 QA 目錄實測安裝並解除安裝，未更新正式安裝、提交或發布。測試安裝包仍使用原版本 1.9.0。

| 功能 | 驗證方式 | 結果 |
| --- | --- | --- |
| 三個 CLI／四個目標模型與參數 | 現行 CLI 模型目錄、參數測試；本次工作前段實際四目標最小 OK 呼叫 | 四者 exit 0；未逐一推論所有預設模型 |
| Codex、Claude、AGY Gemini／Claude 額度與倒數 | 使用者重新登入 Claude 後執行 `--integration` | 1/1 通過，四個目標各 2 個額度視窗 |
| 微量用量與快取倒數 | 核心解析及 Claude 快取 regression | 通過；四捨五入顯示 0% 不會停止真實倒數 |
| AGY CLI 路徑切換 | 兩個不同來源的假 CLI 行程與快取 regression | 通過；同源重用，換源重新讀取 |
| 每日／自動排程、重啟補執行、跨日、邊界、平行喚醒、取消與逾時 | 核心 deterministic suite | 16/16 通過 |
| 設定儲存、取消、模型 effort 切換 | WinForms 事件測試及已安裝 QA 的真實滑鼠操作 | 通過；Sol Ultra 切換 Luna Max；Luna 下拉不含 Ultra |
| 設定按鈕可見與可點擊 | 小視窗、長 CLI 檢查結果、極端捲動 regression；新版 QA 滑鼠儲存 | 通過；修正儲存／取消被捲動容器裁切，按鈕固定於底部 |
| 排程新增修改與持久化 | 隔離 AppHost 事件測試及已安裝 QA 實際建立每日、改自動模式 | 通過；4 個目標、停用狀態及時間寫入隔離 JSON，正式排程不變 |
| 關閉至系統匣、還原與 UI 計時器 | WinForms 關閉事件與還原處理函式 | 通過；未驗證真實 Windows 系統匣圖示點擊與通知外觀 |
| stdout／stderr、長度上限、清除 | 原生 RichEdit 視窗測試 | 通過；修正唯讀狀態造成舊內容刪除失效 |
| 登入連結顯示、目標路由、防重複點擊 | UsageBoard 繪製及事件測試 | 通過；Claude 真人重新登入後額度成功，未自動操作授權頁 |
| 開機啟動鍵啟用／停用及路徑引用 | 本次前段實際 HKCU 開關，測後還原原值 | 通過；未實測重開機登入或已有舊管理員工作的遷移 |
| 方案建置 | `dotnet build 'AI倒數喚醒.sln' -c Release --no-restore` | 成功，0 警告／0 錯誤 |
| 自包含輸出與安裝器 | `./build-installer.ps1 -OutputDirectory './bin/verification-build'` | exit 0；核心 16/16、視窗 7/7、自包含 publish、Inno Setup 編譯及 SHA256 全部完成 |
| 實際安裝、升級、解除安裝 | `tests/Verify-Installer.ps1`，獨立 VerificationBuild AppId／Run key／捷徑／安裝根 | Install、UpgradeKeepStartup、UpgradeEnableStartup、UpgradeDisableStartup、Uninstall 均 PASS；正式資料及 Run key baseline 不變，QA 已解除安裝 |
| 已安裝版本桌面操作 | 使用者重新授權後，以 Computer Use 操作隔離視窗 | 主畫面真實四目標額度、CLI 全部檢查、模型切換、設定儲存、排程建立修改及正常退出通過 |

安裝生命週期及最新設定按鈕桌面驗證使用 `bin/verification-lifecycle/QA-Lifecycle-Footer.exe`；它使用獨立 QA 身分及 `--verify-ui`，不是正式更新包。

最新一般測試安裝包：`bin/verification-build/AI倒數喚醒_Setup_v1.9.0_x64_20261004-090432.exe`（46.16 MB）。
SHA256：`664C960A05AEA6E75EC984B291D6ECD81F45140BD11E4074EBD55A47DFA747B6`。
舊產物及 checksum 保留於 ignored `bin/`。

目前所有已執行的最新測試通過。未實測項目：Windows 重開機登入、已有最高權限舊工作的遷移、真實系統匣圖示操作與通知外觀、每個候選模型的實際推論。這些限制不能視為端到端成功。
