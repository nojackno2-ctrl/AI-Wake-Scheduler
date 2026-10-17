# 功能驗證紀錄

本檔記錄純 Rust 架構（Cargo workspace：`core/` 核心函式庫、`app/` 原生 Win32 桌面程式）的驗證結果。2026-10-16 以前以 C#／WinForms 實作的驗證紀錄保留在文末「歷史紀錄」，其中的 `dotnet build`、WinForms 測試及自包含 publish 已不適用於現行程式碼。

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

## 2026-10-17 AGY Argon 獨立目標（Claude）

- 新增 `CliKind::AntigravityArgon`（顯示「Antigravity (Argon)」），預設 `gemini-4-argon`，畫面排在 Gemini 之後；列舉值加在最後以保留舊版整數序號。
- `cargo fmt --check`、`cargo clippy -D warnings` 通過；`cargo test --workspace --locked` 28 通過、2 項 opt-in 忽略。
- 新增測試：Argon 指令組成與 `--print` 位置、Argon 目錄與 effort、Gemini 不再列出 Argon、Gemini／Argon 額度池分流（含 Argon bucket 放在 Gemini 群組內的格式）、舊設定檔補上 Argon 設定、UI 預設勾選與執行數量依目錄計算。
- 真實唯讀額度（`--ignored executable_locator_resolution`）：Gemini 2 視窗（5h、每週）、Argon 1 視窗（每週）、Claude / GPT 2 視窗、Codex 與 Claude 各 2 視窗，全部可讀。
- 未驗證：本次未實際送出 Argon 喚醒（會消耗 Argon 每週額度）；2026-10-16 曾以 `agy --model gemini-4-argon --effort low --disable-slash-commands --print-timeout 3m0s --print "OK"` 驗證回傳 OK。

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
| 執行檔定位、真實喚醒 | `core_tests::executable_locator_resolution`、`real_cli_wake_smoke`（opt-in） | 本次未執行 |
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
