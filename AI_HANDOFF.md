# AI HANDOFF

## 2026-10-24 Claude：診斷 Claude 遇到 429 不喚醒（僅調查，未改程式）

- 使用者回報「Claude 遇到 429 不工作」。
- 證據（debug-20261023.log／debug-20261024.log）：Claude 最後成功喚醒為 10-23 21:50；23:20 起額度讀取交替出現「HTTP 429（Retry-After 約 1 小時）」與「登入憑證已過期（401）」，排程每次皆記錄「Claude 額度無法確認，不呼叫」，至 10-24 06:20 仍未呼叫。02:30 倒數結束後也因此沒有喚醒。
- `~/.claude/.credentials.json` 的 `expiresAt` 為 10-23 22:57:41，檔案最後修改 10-23 14:57，之後無人刷新。
- 根因：本程式只讀 access token、不刷新（`usage.rs` `read_claude`）；token 只有在執行 Claude CLI 時才會被刷新，但 `manager.rs` 在額度不可用時一律不呼叫 → 形成死結。過期 token 打 `/api/oauth/usage` 時伺服器回 429 或 401。
- 暫解：手動執行一次 Claude CLI 讓它刷新 token。建議修正（未實作）：讀取前檢查 `expiresAt`，過期或 401／429 時改允許喚醒（或先跑輕量 CLI 指令刷新），避免死結。

## 2026-10-23 Claude：發布 v2.3.0（完成）

- 使用者要求「發布執行檔」（明確授權發布）；版本選 v2.3.0（egui 新介面、倒數記憶、每週額度顯示，資料格式相容）。
- 變更：版本 2.2.0→2.3.0（`Cargo.toml`、`Cargo.lock`、`.iss`、README 下載檔名）；新增 `docs/RELEASE_v2.3.0.md`；`docs/FEATURE_VERIFICATION.md` 新增 v2.3.0 發布檢查。
- 資產（`dist\v2.3.0\release\`，不納入版控）：Setup（04a5c614…108e）、Portable ZIP（d42a8978…59dc）、SHA256SUMS.txt。
- 驗證：見 `docs/FEATURE_VERIFICATION.md`「v2.3.0 發布檢查」。
- 發布：提交 ce3bfec CI run 39970791834 五時區 success 後建立附註標籤 `v2.3.0`（peel=ce3bfec40106de08fa457a5162dfc5fd031c29d7）並 `gh release create`（Latest、非 draft／prerelease）。三項資產 GitHub digest 與本機一致；重新下載後 `sha256sum -c` 兩項 OK。
- 未驗證：安裝包生命週期 QA 本版未重跑；未更新使用者電腦上的正式安裝版。

## 2026-10-23 Claude：額度卡片顯示每週倒數（完成）

- 使用者需求：每週倒數也要顯示、位置統一；沒有 5 小時倒數時圓環改顯示每週額度。
- 變更：`board.rs` 新增 `primary_and_weekly`（5 小時視窗倒數中才佔圓環，否則改用每週列；每週以 7 天週期或「每週」標籤判定）；`main_window.rs` 額度卡在圓環上方標示目前視窗名稱，圓環下方固定一列「每週 xx% ／倒數」加進度條，無資料時顯示「—」保留位置；其餘視窗列排除圓環與每週列。
- 驗證：`cargo test --workspace` 全數通過（含新增 `weekly_takes_the_dial_without_five_hour_countdown`），`cargo clippy --workspace` 無警告。
- 未驗證：未實際啟動程式目視檢查卡片排版。
- 後續修正（使用者截圖回饋）：圓環固定顯示 5 小時視窗（未倒數也一樣），只有沒有 5 小時視窗時才改顯示每週；圓環「額度用盡」只看圓環本身的視窗，每週用盡由每週列標紅；卡片下方只保留每週列，其他視窗（如 Claude Opus 7 天）不再列出。fmt、clippy `--all-targets`、`cargo test --workspace` 通過；仍未目視檢查。
- 再修正（使用者：每週 0% 要更明顯）：每週用盡時卡片改紅框淡紅底，圓環上方「每週額度」、中間紅字「用盡」、下方紅字每週重置倒數，5 小時弧線改灰；每週列標籤改為「⚠ 每週已用盡」。fmt、clippy、`cargo test --workspace`、`cargo build --release` 通過；未目視檢查。

## 2026-10-23 Claude：修正 Claude 5 小時倒數讀取不到（完成）

- 使用者回報「讀取不到 claude 的倒數」。
- 證據（debug-20261023.log）：Claude 每次喚醒成功後讀取仍為 `Claude（5 小時） 已用 0% 重置 10-24 02:30:00 未倒數`，重置時間固定不變，自動模式因此每 10 分鐘重複呼叫 Claude。原因：`parse_claude_usage` 要求 `utilization > 0` 才算倒數，喚醒訊息用量回報為 0；10-21 只修了 Codex 的 `add_window`，Claude 未套用。
- 變更：`usage.rs` 抽出共用 `window_started`（重置時間 + 1 分鐘 < 觀測時間 + 視窗長度即已開始），Codex 與 Claude 共用；`core_tests` 新增 Claude 0% 已開始／重置時間滑動兩種案例。
- 驗證：fmt、clippy `-D warnings` 通過；`cargo test --workspace --locked`（暫存 CARGO_TARGET_DIR）全數通過、4 opt-in 忽略、0 失敗。
- 未驗證：未以實機新版本觀察 Claude 讀取；未確認 Claude API 在視窗未開始時是否回傳 null 或滑動的 resets_at（兩者皆判為未倒數）。正在執行的程式需重新建置並重啟才會套用。

## 2026-10-23 Codex：完整程式檢查與最佳化（完成）

- 目標：檢查核心排程／並行／資料保存／CLI 程序及 egui 介面，修正可重現問題並驗證；起始 main／1bc7b80 工作樹乾淨。
- 基準佐證：`rustc -vV` 為 1.99.0、x86_64-pc-windows-msvc；依序 `cargo check --workspace --locked`、`cargo test --workspace --locked`、`cargo build --workspace --locked` 通過，共 43 測試通過（app 13、UI 2、core 28），4 項真實 CLI 選用測試略過。
- `MSBuild AI倒數喚醒.sln /t:Build /p:Configuration=Debug /p:Platform=x64` 通過。使用 agent-delegation-tools 委派 Codex 審查與實作，主代理複核及驗證；委派不可操作 Git 或交接檔。
- 已修正：儲存失敗狀態、手動關閉取消、重複目標去重、用盡額度與 Claude 過期／429 快取、輸出背壓、日誌保留、解析溢位；批次檔雙引號＋`&` 注入改由標準函式庫處理，實際參數往返／標記檔回歸通過；完成存檔失敗保留結果並重試、設定存檔失敗補償啟動偏好。
- 主代理最終驗證：依序 check／test／build 通過，共 61 測試通過（app 17、UI 2、core 42），4 項選用略過；fmt、Clippy `-D warnings` 通過。fmt 換行、Clippy 測試模組順序與條件內 wait 空迴圈樣式均已修正並複驗全套通過。Release 建置成功：dist/audit-20261023/AI倒數喚醒.exe（9,852,416 bytes），SHA256 14f027b7714fe85f11c884a17f32e1796e542fdb79fbc3f65e1c833331a804e0。
- 真實唯讀驗證：`cargo test -p aiwake-core --test core_tests --locked executable_locator_resolution -- --ignored --exact --nocapture` 通過，五個 CLI 目標的版本探針與帳號額度皆可讀；`cli_update_check_real` 亦通過，僅提示版本，未更新 CLI。
- UI／安裝回歸已解決：700×620 存檔先重現舊值覆蓋，改用 container_contains_pointer 後成功；8 張擷圖已檢視。Release 隔離生命週期、QA 五階段及安裝後 exe 雜湊比對全 PASS；正式設定、排程、啟動值逐步確認不變。既有產物已備份，附 SHA256SUMS.txt；臨時探針零指標／初始化誤判已修正。程式碼已提交／推送 613e2ad，CI 39938772825 的 UTC／台北／印度／太平洋／澳洲東部五時區全部 success；收尾僅補驗證文件。未變更正式安裝／資料、未呼叫真實模型、未驗證全天掛機；詳見 docs/FEATURE_VERIFICATION.md。

## 2026-10-23 Antigravity + Claude Opus 5.5：以 eframe/egui 重構現代 GPU 圖形化介面（完成）

- 使用者要求：「我覺得介面還是很醜，我要圖形化的介面」並指定「呼叫 Claude opus 5.5 來討論怎麼設計介面」。開始時 `main` 工作樹乾淨。
- 設計討論與使用者確認決策：
  1. **框架選型**：改用 `eframe` / `egui` `0.37.0`（`wgpu` DX12 後端，讀取系統內建 `msjh.ttc`、`msjhbd.ttc`、`consola.ttf`、`seguisym.ttf`，維持單一執行檔與零額外執行階段依賴），外圍保留單一實例 Mutex、`AppHost`、`tray.rs` 與原生 COM 檔案對話框。
  2. **主題風格**：預設採用「深色科技感主題（Dark Mode，`#0B1020` 背景／`#151C2E` 卡片／`#22D3EE` 青色強調）」，並在頂部導覽列提供 `☀ 淺色 / ☾ 深色` 一鍵切換（淺色模式下終端機仍維持深色底）。
  3. **5 大 CLI 圓環儀表板（Ring Gauge Cards）**：雙環設計——外環（粗）顯示 5h 主要額度剩餘 %，內環（細）顯示重置倒數進度，底部附每週額度細長條與「重新登入」按鈕，卡片上方搭配品牌色圓形徽章（`Ge` `#3B82F6` / `Ar` `#B58500` / `AC` `#C04FD0` / `Cx` `#10A37F` / `CC` `#E2683C`）。
  4. **排程卡片、24h 時間軸、控制項與設定抽屜**：
     - 左欄改為圖形化排程卡片（含狀態呼吸燈、右上角 Toggle 滑動開關點擊立即生效並存檔、24 小時視覺化偵測時段條＋已知重置時間菱形標記、CLI 徽章列）。
     - 中下排程編輯器採用圓角現代輸入框、可拖曳把手的 24 小時互動時間軸（5 分鐘吸附）、分段式檢測間隔選擇器（`5分`/`10分`/`15分`/`30分`/`60分`＋自訂數值）與點擊式 CLI Pill/Chip 標籤。
     - 設定頁改為主視窗右側滑出抽屜（`SettingsSheet`），免開獨立視窗，底部固定「儲存設定／檢查 CLI 連線／取消」操作列。
- 變更：
  - `Cargo.toml`、`app/Cargo.toml`、`Cargo.lock`：加入 `eframe = "0.37"` 與開發依賴 `egui_kittest = "0.37"`。
  - `app/src/theme.rs`、`app/src/widgets.rs`、`app/src/terminal.rs`、`app/src/settings_window.rs`、`app/src/main_window.rs`、`app/src/board.rs`、`app/src/tray.rs`、`app/src/ui.rs`、`app/src/lib.rs`、`README.md`：完整實作 `eframe` / `egui` 圖形化控制中心、雙同心圓環儀表板、24h 互動時間軸、自繪 Toggle Switch、設定右側抽屜與隱藏式 `AiWakeTrayHost` 系統匣視窗。
  - `app/tests/ui_smoke.rs`：新增 `egui_editor_settings_fake_cli_and_rendering`（透過 `egui_kittest` 驅動排程建立、卡片即時啟停存檔、編輯儲存、深淺色切換、設定模型／思考程度連動、假 CLI `--version` 探針與立即執行終端機輸出，並輸出 6 張 `wgpu` 渲染截圖人工檢視確認無破版或字元缺漏）與 `native_wgpu_window_close_restore_and_minimized`（驗證真實 `ai-wake-scheduler.exe` 關閉縮到系統匣、還原、直接結束及 `--minimized` 隱藏啟動）。
- 驗證：依序執行 `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo test --workspace --locked`（app 單元測試 13、`ui_smoke` 整合測試 2、core 測試 28，共 43 項通過，4 項 opt-in 忽略）、`cargo build --workspace --locked` 與 `git diff --check` 全部通過。
- 限制：未重新打包正式安裝版或發布新版本。

## 2026-10-23 Antigravity：取消強制更新 CLI 阻擋呼叫，改回提示通知（完成）

- 使用者要求：「取消強制更新CLI的設定，改成提示就好」。開始時 `main` 工作樹乾淨。
- 變更：
  - `core/src/runner.rs`、`core/src/cli_update.rs`：移除 `ProcessCliRunner` 呼叫前的版本檢查閘門（`update_checker`、`with_update_checker` 與 `CliUpdateStatus::require_current`），手動與排程呼叫不再因 CLI 有新版本或無法查詢版本而被阻擋。
  - `app/src/main_window.rs`、`README.md`：`on_cli_updates_checked` 與 `show_cli_updates` 的狀態列文字、系統匣氣泡通知及對話框訊息改回單純提示「CLI 有新版本可更新」與詢問是否開啟更新主控台，移除「必須更新才能呼叫／更新前已阻擋呼叫」等強制字眼；同步更新 README 說明。
  - `app/tests/ui_smoke.rs`、`core/tests/core_tests.rs`：端對端 UI 測試 `ID_RUN_NOW` 恢復驗證假 CLI 正常執行成功並輸出 `FAKE-CLI-OK`；移除強制版本阻擋相關測試與 `fake_current_runner`。
- 驗證：依序執行 `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo test --workspace --locked`（app 10、隔離 UI `ui_smoke` 1、core 28，共 39 項通過，4 項 opt-in 忽略）、`cargo build --workspace --locked` 與 `git diff --check` 全部通過。
- 限制：未重新打包正式安裝版或發布新版本。

## 2026-10-23 Antigravity：讀取間隔改為僅在未讀到倒數時讀取，並記住下次倒數結束時間（完成）

- 使用者要求：「讀取間格改成沒有讀取到倒數就讀取，然後會記住下一次倒數結束德時間」（經確認：倒數結束時間一到且在偵測時段內即立即重讀並喚醒，未倒數才依間隔重試；同時套用於排程偵測與主視窗額度看板並共用記住的倒數結束時間）。
- 變更：
  - `core/src/usage.rs`：新增 `active_reset_at(snapshot, now)`，統一取得指定 CLI 目前有效倒數的下次結束（重置）時間（優先取已用盡視窗最晚重置時間，否則取短週期／主要視窗有效倒數之重置時間）。
  - `core/src/manager.rs`：在 `Inner` 恢復並升級 `known_reset_at: Mutex<HashMap<(Guid, CliKind), Moment>>`、`remember_reset`、`known_reset`（可引用 `UsageReader::latest_snapshot` 中已讀到的有效倒數，同時避免 UI 在重置時刻先刷新時洗掉排程尚未處理的重置事件）與 `next_job_schedule_time`。排程在偵測時段內：若目標 CLI 已讀到倒數且 `reset > now`，略過間隔刻度讀取並將下次排程時間對齊 `reset`；當 `now >= reset` 時不受整點間隔限制立即重讀並喚醒；僅在尚未讀到有效倒數時才依 `probe_interval_minutes` 整點刻度讀取。
  - `app/src/main_window.rs`：新增 `sync_usage_snapshots_from_reader()` 即時同步排程背景讀取到的最新額度快照；背景自動重讀（`refresh_usage(false)`）改為僅查詢 `active_reset_at` 為 `None`（尚未讀到倒數或倒數已結束）的 CLI，保留仍在倒數中的 CLI 快照與結束時間；手動點擊「重新讀取倒數」（`refresh_usage(true)`）則重讀全部 CLI。同步更新 `EDITOR_NOTE` 與 `README.md`。
  - `core/tests/core_tests.rs`：更新並強化 `schedule_manager_obeys_daily_boundaries_and_resets`、`schedule_manager_countdown_reset_skips_interval_and_wakes_at_reset` 與 `schedule_manager_reset_survives_ui_snapshot_refresh`，驗證讀到倒數後在間隔刻度不重複讀取、倒數結束時間一到立即重讀並呼叫。
- 驗證：依序執行 `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo test --workspace --locked`（app 10、隔離 UI `ui_smoke` 1、core 29，共 40 項通過，4 項 opt-in 忽略）、`cargo build --workspace --locked` 與 `git diff --check` 全部通過。
- 限制：未以真實 CLI 長時間掛機驗證，未重新打包正式安裝版或發布新版本。

## 2026-10-23 Antigravity：主視窗三欄版面重作、固定字體大小與全欄位自動調整不裁切（完成）

- 使用者要求：「UI重作，終端輸出改到最右邊，然後視窗大小的調整不要改變字體的大小，所有的欄位要自動調整，不要被切到」。開始時 `main` 工作樹乾淨。
- 變更：
  - `app/src/main_window.rs`：移除依視窗尺寸縮放字型的 `update_zoom`、`DESIGN_CLIENT_SIZE` 與 `MAX_ZOOM`，字型僅在啟動與跨螢幕 `WM_DPICHANGED` 時依螢幕 DPI 建立，調整視窗大小時字體大小完全固定。將「即時終端輸出」面板移至主視窗最右欄（全高卡片），與左中主區以垂直可拖曳分隔線（`Drag::Vertical`、`IDC_SIZEWE`）隔開；左中主區維持「已儲存排程」與「排程內容＋剩餘流量與重置倒數」並排（極窄時自動堆疊）。「排程內容」在寬度足夠時將「開始時間／結束／間隔(分)」排在同一列並自動調整控制項與勾選框欄數。排程清單 `resize_columns` 依標題與列內容動態計算五欄寬度與列高（`LVM_SETIMAGELIST`），並於 `on_list_custom_draw` 以 `DT_WORDBREAK | DT_EDITCONTROL` 自繪儲存格自動換行，確保所有欄位完整顯示不出現刪節號或水平裁切。
  - `app/src/board.rs`：額度看板分欄門檻調為 `260px`（預設視窗下 5 個 CLI 自動排成雙欄 3 列，免捲動即可完整顯示），長條圖寬度依欄寬動態縮放，重置時間戳記僅在剩餘寬度足夠時繪製，避免任何文字被切邊。
  - `app/src/settings_window.rs`、[`app/src/w32.rs`](file:///d:/Github/AI-Wake-Scheduler/app/src/w32.rs)：設定視窗長標籤改為自動換行（`DT_WORDBREAK`），表格欄寬安全夾限；`Font::measure_wrapped` 加入 `DT_EDITCONTROL`。
  - `app/tests/ui_smoke.rs`、`README.md`：新增預設／放大／縮小視窗下之版面順序、控制項邊界不裁切與 `WM_GETFONT` 字型控制代碼不變斷言；更新 README 介面位置說明。
- 驗證：依序執行 `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo test --workspace --locked`（app 10、隔離 UI `ui_smoke` 1、core 29，共 40 項通過，4 項 opt-in 忽略）、`cargo build --workspace --locked` 與 `git diff --check` 全部通過；以 `AIWAKE_UI_SHOTS` 擷取預設、放大、縮小、執行後及設定對話框畫面並人工檢視確認無任何欄位被裁切（暫存擷圖已於提交前清除）。
- 限制：未重新打包正式安裝版或發布新版本。

## 2026-10-22 Codex：CLI 強制更新與整點間隔設定（完成）

- 使用者要求舊版 CLI 不得呼叫，並確認間隔依每小時刻度對齊（5 分鐘＝00、05、10…；15 分鐘＝00、15、30、45）。開始時 main／593d15c 工作樹乾淨。
- 共用執行入口強制每次確認版本；舊版、無法查詢或版本程序失敗即阻擋手動與自動呼叫，更新並確認版本後才放行。UI 提示更新前禁止呼叫，避免查詢失敗卻顯示全部最新；沿用使用者開啟更新主控台的流程。
- 每筆排程新增 ProbeIntervalMinutes（1–60、預設 10）與 UI「間隔（分）」；ScheduleRevision=3，舊資料補預設間隔並保留歷史。檢查與自動呼叫共用整點分鐘刻度（容許當分鐘的執行延遲），任意間隔每小時重新由 00 分起算；不在其他分鐘補做、重置不提前觸發，移除自動呼叫後的額外額度查詢。看板重新整理仍只更新顯示。保留時段、額度用盡、去重與兩分鐘冷卻；手動立即執行仍可操作，但不可繞過版本檢查。
- 驗證：依序 `cargo check --workspace --locked`、`cargo test --workspace --locked`、`cargo build --workspace --locked` 通過（app 10、隔離 UI 1、core 29，共 40 通過；4 項真實 CLI 選用測試略過）；fmt、Clippy `-D warnings`、diff check 通過。`rustc -vV` 確認 x86_64-pc-windows-msvc，無連結錯誤。回歸涵蓋所有 CLI 的版本閘門、更新後恢復、版本程序非零結束碼、5／15 分鐘排程不同刻度、同刻度去重、非刻度啟動／重置、跨日與結束邊界、舊資料與間隔儲存還原。
- 除錯紀錄：第一次 UI 測試發現新控制項 ID 與開始時間重複，改用 116；第二次因假 CLI 無版本號而被正確阻擋，UI 回歸改為驗證阻擋與錯誤摘要，成功呼叫路徑由核心注入已確認版本來源驗證。最終全套通過。擷圖位於 dist/cli-gate-slots-20261022，已檢視主畫面確認欄位與說明完整顯示。
- 限制：未以真實模型喚醒或長時間排程驗證、未更新正式安裝／正式資料、未發布版本；AGENTS.md、Cargo.lock 與安裝程式無差異。

## 2026-10-22 Codex：舊排程資料安全遷移（完成）

- 範圍：修改核心、回歸測試與說明；委派工作未操作 Git，主代理依授權檢查後提交與推送。保留其他聊天的交接紀錄，不修改正式資料或安裝程式。
- 證據：`store.rs` 原將 JSON null 視為空資料；`models.rs` 原會替缺少 Id／ScheduledAt 的資料產生預設值；`manager.initialize` 原每次重算所有狀態並無條件保存，缺少遷移版本與穩定性驗證。
- 已實作：每筆 ScheduleRevision、舊整數／名稱列舉相容、必要欄位與版本檢查、未知欄位拒絕載入以免遺失；保留停用與完成狀態、未來時間及執行紀錄，無變更不寫入，完整載入並保存成功才啟動排程。新增三項回歸測試涵蓋 50 筆舊週期／狀態／列舉組合、再次載入不重寫、最新時間與終止狀態、混合損壞資料與重複識別碼不覆寫。
- 驗證：依序 `cargo check --workspace --locked`、`cargo test --workspace --locked`、`cargo build --workspace --locked` 全部通過（app 10、隔離 UI 1、core 22 通過；4 項真實 CLI 選用測試略過）。`cargo fmt --all -- --check`、`git diff --check` 通過；`rustc -vV` host 為 x86_64-pc-windows-msvc，無連結錯誤，使用原 target 即可完成。主代理另執行 `cargo test -p aiwake-core --locked schedule_`（12 通過）與 `cargo clippy --workspace --all-targets --locked -- -D warnings` 通過。
- 限制：未以正式資料或安裝版實測遷移，未執行真實 CLI 額度測試；只升級排程資料，不升級執行檔。另一聊天的「開始／結束時段排程」已明確暫停，未將其功能納入本輪。AGENTS.md、Cargo.lock 與安裝程式無差異。
- 環境：預設 Shell 沙箱啟動失敗（helper_unknown_error），唯讀檢查改用獲核准的提升執行；未變更系統或正式資料。

## 2026-10-22 Codex：開始／結束時段排程（完成）

- 已完成：只設定每日開始／結束時間（開始含、結束不含，支援跨午夜，全天為 00:00–24:00）；移除模式顯示及舊週期演算法。每筆啟用排程在時段內每 10 分鐘重讀倒數，已知重置時間到即重讀，確認未倒數才呼叫；任一額度已用盡、讀取失敗、空額度視窗或無讀取器時不呼叫。保留每 CLI 兩分鐘冷卻、執行中去重與手動立即執行。
- 資料相容：ScheduleRevision=2、新增 EndTimeOfDay（缺欄位預設 24:00），舊週期列舉只供讀取相容；沿用 cf0d1c5 安全遷移與完整歷史紀錄保護。含秒數的舊開始時間轉為分鐘，原始歷史 ScheduledAt 保留。
- 邊界修正：探測完成、排入執行、工作執行緒及呼叫前皆確認時段；自動執行後重讀亦受結束時間限制。排程獨立記住重置時間（含呼叫後新倒數），避免 UI 刷新滑動時間戳記時遺失提前偵測事件。
- 驗證：依序 `cargo check --workspace --locked`、`cargo test --workspace --locked`、`cargo build --workspace --locked` 全通過（app 10、隔離 UI 1、core 25，共 36 通過，4 項真實 CLI 選用測試略過）；fmt、Clippy `-D warnings`、diff check 通過。MSVC host 確認；`MSBuild AI倒數喚醒.sln /t:Build /p:Configuration=Debug /p:Platform=x64` 成功，無連結錯誤。
- 主代理另以 AIWAKE_UI_SHOTS 重跑隔離 UI 測試 1/1 通過並檢視畫面，確認開始／結束欄位與按鈕完整顯示；驗證結束時間寫入、修改保留、重新選取與新增重設。`build-release.ps1 -SkipTests -OutputDirectory dist/time-window-20261022` 成功；產物 `dist/time-window-20261022/AI倒數喚醒.exe`，SHA-256 `d04d20fde0abf9dca8ce85d2e04b71bf4e00c9b8de601a6e54121096189dea31`。
- 協作：外部委派因模型容量不足 exit 10，曾因其他聊天同檔修改暫停；使用者要求繼續後，確認另一聊天已完成 cf0d1c5 且閒置才接手。舊筆記原文移至歷史交接，已比對完整保留。
- 限制：未以真實 CLI 長時間執行新排程、未更新正式安裝／正式設定、未發布版本；現有已啟動的正式程式仍為先前安裝版。

## 2026-10-22 Codex：AGY 重複失敗為本機舊安裝版（已升級）

- 新證據：本機 debug-20261022.log 顯示 14:37 啟動 Program Files 的 v2.1.0，只有一個舊倒數模式排程；14:37:33、14:39:43 均已讀到 Gemini 每週 100%、10-24 08:30:47 重置，卻仍呼叫。因此此次直接原因是尚未更新安裝版，並非目前 v2.2.0 的單排程額度閘門失效。
- 已取消原先委派的擴大修復工作（exit 130，無原始碼變更）。多排程限制仍維持既有未解決狀態，不在此次安裝修復內。
- 驗證：安裝包 SHA-256 為 6ef96e03cd6c33e0dd8350716cf21dcbc659e5fac8a8706f3e8864703477ad75，符合 SHA256SUMS.txt；`cargo test -p aiwake-core --locked schedule_manager_migrates_to_auto_and_skips_exhausted` 1 通過。
- 正式設定與排程已備份於本機資料目錄 backups/20261022-before-v2.2.0（未納入 Git）。第一次安裝因 RestartManager 無法關閉舊程式而中止，版本仍 2.1.0；第二次使用安裝器 FORCECLOSEAPPLICATIONS 重試成功（安裝日誌 Installation process succeeded、無須重新啟動 Windows），已由原 Program Files 路徑重啟；檔案版本與 14:43:24 啟動日誌均為 2.2.0。

- 升級前後 settings.json／schedules.json 雜湊一致；但升級前備份的 schedules.json 已是空陣列，新版載入 0 個排程，未重建或擅自新增工作。新版實際讀到 Gemini 每週 100%、10-24 08:30:47 重置，Claude 亦已可讀取。未完成正式排程的現場略過驗證，也未驗證額度重置後喚醒；保留上述回歸測試證據。

## 2026-10-22 Codex：應用程式顯示版本號（完成）

- 使用者要求應用程式顯示版本號；開始時 main 工作樹乾淨，HEAD 為 ab4d3fc。
- `app/src/main_window.rs` 的共用標題透過 `concat!` 與 `env!("CARGO_PKG_VERSION")` 產生「AI 倒數喚醒 v2.2.0」，同步供視窗標題、頂端自繪標題與系統匣提示使用，往後隨 Cargo 版本更新。
- 驗證：`rustc -vV` 確認 `x86_64-pc-windows-msvc`；依序 `cargo check --workspace --locked`、`cargo test --workspace --locked`、`cargo build --workspace --locked` 全部通過（30 通過、4 選用整合測試略過）；`cargo fmt --all -- --check` 與 `git diff --check` 通過。
- 視窗證據：PowerShell 隱藏啟動後讀取 `MainWindowTitle` 得到空字串，無法作為驗證；改於既有 `ui_smoke` 加入跨行程視窗文字斷言，再執行 `cargo test -p aiwake-app --test ui_smoke --locked` 通過，確認實際標題為「AI 倒數喚醒 v2.2.0（功能驗證）」。
- 限制：未人工檢視頂端文字與系統匣提示的繪製；未更新正式安裝版或發布資產。

## 2026-10-22 Claude：發布 v2.2.0（完成）

- 使用者要求「發布執行檔」（明確授權發布）；版本選 v2.2.0（新增功能、設定與排程自動遷移相容）。
- 變更：版本 2.1.0→2.2.0（`Cargo.toml`、`Cargo.lock`、`.iss`、README 下載檔名）；新增 `docs/RELEASE_v2.2.0.md`；`docs/FEATURE_VERIFICATION.md` 新增 v2.2.0 發布檢查。
- 資產（`dist\v2.2.0\release\`，dist 不納入版控）：Setup（sha256 6ef96e03…ad75）、Portable ZIP（f16a1f00…e7bc）、SHA256SUMS.txt。
- 驗證：見 `docs/FEATURE_VERIFICATION.md`「v2.2.0 發布檢查」。
- 發布：提交 8b20642 CI run 39715734634 五時區 success 後建立附註標籤 `v2.2.0`（peel=8b2064251a063930b7842986ec4bf2a054c86e15）並 `gh release create`（Latest、非 draft／prerelease）。三項資產 GitHub digest 與本機一致；重新下載後 `sha256sum -c` 兩項 OK。後續 8ab0644 僅修正文件路徑字元。
- 未驗證：安裝包生命週期 QA 本版未重跑、從 v2.1.0 安裝版實際升級、系統匣更新通知與更新對話框點擊。

## 2026-10-22 Claude：預設改為最便宜模型、新增 CLI 更新提示（完成）

- 使用者要求「預設呼叫為最便宜的」與「還要加入各 cli 更新提示」。
- 最便宜預設（commit 9a5a476）：依官方價格頁核對（Claude pricing、OpenAI API pricing、Gemini API pricing）。Claude 預設 Haiku 4.5（1/5 美元）→ Haiku 5.5（0.10/0.50）；Codex gpt-6-luna（0.10/0.50）已是 model/list 最低；AGY Gemini 只有 3.8 Flash；Argon 只有一款；AGY Claude/GPT 維持 Sonnet 5.5 Low（Opus 較貴，GPT-OSS 無官方價且 11-02 下架）。節省模式一律用 low 推理。新增 `AppSettings.default_models_revision`（JSON `DefaultModelsRevision`）：舊設定的 Haiku 4.5 只遷移一次，之後手動改回不再被覆寫。`docs/MODEL_PRICING.md` 已更新。
- CLI 更新提示：
  - 新增 `core/src/cli_update.rs`：`CliTool`（agy、Codex、Claude；三個 AGY 目標共用 agy）；本機版本由 `<cli> --version` 取第一個 x.y.z；最新版本來源 agy＝`https://antigravity-cli-auto-updater-974169037036.us-central1.run.app/manifests/windows_amd64.json`（取自 agy.exe 內建字串，實測回 1.4.4）、Codex＝npm `@openai/codex/latest`、Claude＝npm `@anthropic-ai/claude-code/latest`；數字逐段比較；`create_update_start_info` 開看得見的主控台跑 `<cli> update || pause`。`login::create_console_start_info` 抽出共用。
  - UI：清單下方「檢查 CLI 更新…」按鈕（ID 107）與系統匣選單項目（ID 204）；啟動 60 秒後檢查、之後每 12 小時（TIMER_UPDATE_CHECK）；有新版本時狀態列顯示、同一版本只跳一次系統匣通知，點通知或按鈕顯示版本清單並以是／否詢問是否開啟更新主控台；主控台全部關閉後清除執行檔與 AGY 連線快取並重新檢查、重讀額度。檢查結果寫入除錯日誌（分類「更新」）。
- 驗證：fmt、clippy `-D warnings` 通過；`cargo test --workspace --locked` app 10、UI 1、core 19 通過、4 opt-in 忽略、0 失敗；新增 opt-in `cli_update_check_real` 實際執行：agy 1.4.4 已最新、Codex 0.163.0 已最新、Claude Code 2.1.289 → 2.1.306 有新版本。UI 測試擷圖確認按鈕顯示於第二列且未截斷。
- 未驗證：未實際點擊按鈕／系統匣通知與是否對話框。
- 後續（使用者要求）：執行 `claude update`，Claude Code 2.1.289 → 2.1.307（exit 0，`claude --version` 確認）；更新後以 claude-haiku-5-5 + `--effort low` 呼叫回 OK，未收錄模型警告已消失。

## 2026-10-22 Claude：實測所有模型可否呼叫並更新模型清單（完成）

- 使用者要求「檢查所有模型都是可以呼叫嗎？更新到最新」。
- 來源：`agy models`（agy 1.4.4）、Codex App Server `model/list`（codex-cli 0.160.0）、Claude Code 2.1.289。
- 實測（使用者明確要求，以本工具相同的節省 Token 參數、每個模型送一次「只回 OK」）：
  - Codex：gpt-6.1-sol、gpt-6-astra、gpt-6-sol、gpt-6-luna、gpt-5.6-sol、gpt-5.6-terra、gpt-5.6-luna 全部 exit 0 回 OK；gpt-5.5 回 HTTP 400「not supported when using Codex with a ChatGPT account」，且已不在 model/list。
  - AGY Argon：gemini-4-argon（含 low/medium/high）全部 OK。
  - AGY Gemini Flash（3.8 base/low/medium/high）與 AGY Claude/GPT 池（sonnet/opus 5.5 low/medium/high、gpt-oss-120b-medium）全部 exit 3 `RESOURCE_EXHAUSTED (429)`：兩個池的每週額度已用 100%（Gemini 10-24、Claude/GPT 10-25 重置），屬額度問題而非模型 ID 問題；模型 ID 皆在 `agy models` 清單中。重置後需重測。
  - Claude CLI：haiku-4-5-20251001、haiku、sonnet、opus、claude-sonnet-5-5、claude-opus-5-5 OK（第一輪 haiku/sonnet 遇到暫時性 token refresh 錯誤，重試成功）；claude-haiku-5-5 可呼叫且接受 --effort，但 2.1.289 模型目錄未收錄、會印警告；fable、claude-fable-5-1 回使用額度／credits 限制（Pro 方案需額外用量額度）。
- 變更：Codex 清單移除 gpt-5.5（並移除不再使用的 CODEX_XHIGH）；舊設定的 gpt-5.5 由 `ensure_defaults` 轉為 gpt-6.1-sol；AGY Gemini 與 Argon 推薦清單補上 medium；Claude 推薦清單加入 claude-haiku-5-5，預設維持 Haiku 4.5（CLI 已收錄、最便宜）。測試同步更新。
- 驗證：fmt、clippy `-D warnings` 通過；`cargo test --workspace --locked`（暫存 CARGO_TARGET_DIR）29 通過、3 opt-in 忽略、0 失敗。
- 未驗證：AGY Gemini Flash 與 Claude/GPT 池模型在額度重置後是否可正常回覆；未更新 Claude Code 本身。

## 2026-10-22 Claude：依除錯日誌修正 AGY 額度用盡仍反覆呼叫，並只保留自動模式（完成）

- 使用者要求「讀取日誌修復問題，並把所有模式改成只有自動模式，其他模式刪掉」。
- 證據（`%LOCALAPPDATA%\AI倒數喚醒\logs\debug-20261021.log`）：排程為「自動倒數模式」。AGY(Gemini) 每週額度已用 100%（重置 10-24 08:30），5 小時視窗沒有重置時間而判為「未倒數」，因此每 2 分鐘（冷卻）呼叫一次，每次 stderr 都是 `RESOURCE_EXHAUSTED (code 429)`、結束碼 3，logs 目錄已累積 479 個 Antigravity 執行日誌。另外倒數模式對倒數中的 CLI 每 30 秒重讀額度，Claude 額度 API 因此頻繁 HTTP 429。10-22 04:08 的日誌也顯示 Claude 登入憑證已過期（需使用者自行重新登入，非程式問題）。
- 變更：
  - `usage.rs` 新增 `exhausted_until`：任一視窗已用 ≥100% 且重置時間在未來，傳回最晚的重置時間。
  - `manager.rs`：自動模式讀取後若額度用盡則記錄「額度已用盡，至 … 重置前不呼叫」並略過；移除自動倒數模式（`scan_countdown_jobs`、`countdown_probes`、當日偵測視窗檢查）與每日模式的分支；`initialize`／`upsert` 一律把排程轉為 `Interval`，開始時間沿用 `initial_time_of_day`（每日等舊模式取原排程時分）；完成後有額度讀取器排 +10 分鐘，否則 +301 分鐘。
  - `models.rs`：`ScheduledJob` 預設改為 `Interval`；列舉其他值保留僅供讀取舊資料。
  - UI：移除「排程模式」下拉（`ID_RECURRENCE` 刪除），時間欄標籤改為「開始時間」；標題與說明文字改為自動模式；清單倒數欄對額度用盡的 CLI 顯示「額度用盡 剩餘時間」。
  - 測試：`schedule_manager_countdown` 改為 `schedule_manager_migrates_to_auto_and_skips_exhausted`（倒數／每日模式遷移、保留開始時間、重現日誌情境：AGY 每週 100% 不呼叫、Codex 未倒數才呼叫、10 分鐘後重讀仍不呼叫 AGY）；`schedule_manager_due_job` 斷言改為自動模式；presenter 與 UI 端對端測試移除模式下拉。
  - README 模式說明更新。
- 驗證：`cargo fmt --all`、`cargo clippy --workspace --all-targets --locked -- -D warnings` 通過；`cargo test --workspace --locked`（暫存 CARGO_TARGET_DIR）app 10、UI 1、core 18 通過，3 opt-in 忽略，0 失敗。
- 未驗證：未以真實 CLI 長時間執行新版本、未重建 release／安裝包、未發版；使用者電腦上正在執行的舊版需重新建置並重啟才會套用。同一時間只有第一個自動模式排程走額度偵測，其餘自動模式排程仍走 301 分鐘的盲呼叫（既有行為，未變更）。

## 2026-10-21 Claude：修正 Codex 5 小時倒數偵測不到（完成）

- 使用者回報「codex 的倒數 5 小時抓不到」。
- 證據（除錯日誌 debug-20261021.log）：喚醒後 Codex primary 視窗 `已用 0% 重置 15:00:20` 且重置時間固定不變，但被判為「未倒數」，自動模式因此每 2 分鐘（冷卻）重複呼叫 Codex。原因：`usage.rs::add_window` 要求 `usedPercent > 0` 才算倒數，而喚醒訊息用量四捨五入為 0%。同日 10:00:15（重置剛到、尚未喚醒）讀到重置時間 = 現在 + 5 小時，證實未開始的視窗重置時間會隨讀取時間滑動。
- 變更：`add_window` 另以「重置時間 + 1 分鐘容許 < 觀測時間 + 視窗長度」判定視窗已開始，即使 0% 也視為倒數中；`cli_usage_reader` 測試新增 0% 已開始／未開始兩種案例。
- 驗證：fmt、clippy `-D warnings` 通過；`cargo test --workspace --locked`（暫存 CARGO_TARGET_DIR）全數通過、3 opt-in 忽略。
- 實機驗證：重新建置 release 並從 `target
eleasei-wake-scheduler.exe` 啟動（安裝版 Program Files 未更新），12:12 啟動後 Codex 讀取顯示 `已用 0% 重置 15:00:21 倒數中`，未再呼叫 Codex。
- 另發現（未處理）：AGY(Gemini) 每週額度已用 100%、5 小時視窗無重置時間，自動模式每 2 分鐘仍會呼叫 AGY。
- 未驗證：喚醒後 1 分鐘內的讀取仍可能判為未倒數，由 2 分鐘冷卻與 10 分鐘偵測涵蓋。

## 2026-10-21 Claude：發布 v2.1.0（完成）

- 使用者要求「發布到 GitHub」（明確授權發布）；版本選 v2.1.0（新增功能、資料格式相容）。
- 變更：版本 2.0.0→2.1.0（`Cargo.toml`、`Cargo.lock`、`.iss`、README 下載檔名）；新增 `docs/RELEASE_v2.1.0.md`；`docs/FEATURE_VERIFICATION.md` 新增 v2.1.0 發布檢查。
- 資產（`dist\v2.1.0\release\`，dist 不納入版控）：Setup（sha256 35814071…3d5b）、Portable ZIP（3174d447…e346）、SHA256SUMS.txt。
- 驗證：見 `docs/FEATURE_VERIFICATION.md`「v2.1.0 發布檢查」。
- 發布：提交 c0b4020 CI run 39522588975 五時區 success 後建立附註標籤 `v2.1.0`（peel=c0b4020509a126cf4522d3b4b41c00a0659aa067）並 `gh release create`（Latest、非 draft／prerelease）。三項資產 GitHub digest 與本機一致；重新下載後 `sha256sum -c` 兩項 OK。
- 未驗證：安裝包生命週期 QA 本版未重跑、從 v2.0.0 安裝版實際升級、自動模式真實長時間運作。

## 2026-10-21 Claude：除錯日誌與匯出（完成）

- 使用者目標「加入可以輸出所有日誌來除錯」。
- 變更：
  - 新增 `core/src/debug_log.rs`：全域除錯日誌，`init(paths)` 後才寫入 `logs\debug-YYYYMMDD.log`（未初始化即無副作用，測試不寫檔）；初始化時清除 14 天前的除錯日誌；`export(paths, summary, dest)` 合併摘要、settings.json、schedules.json 與 logs 底下所有 .log（依修改時間排序，單檔超過 2 MB 只取尾端），以 UTF-8 BOM 寫出。
  - 記錄點：`CliUsageReader::read` 每次讀取的結果（`CliUsageSnapshot::describe`）或錯誤；`manager.rs` 載入／儲存／刪除／手動執行、自動模式讀取倒數的原因與每個 CLI 的判斷（倒數中、無法讀取、冷卻中、立即呼叫、下次讀取時間）、倒數模式呼叫、到期執行／略過補做、每個 CLI 開始與結果（結束碼、錯誤、日誌路徑）、完成後下次時間、背景錯誤；`host.rs` 啟動資訊。CLI 子程序的逐行輸出不寫入除錯日誌（已完整存在各次執行日誌）。
  - UI：清單下方新增「匯出除錯日誌…」按鈕（ID 106）與系統匣選單項目（ID 203）；`ui::save_file`（IFileSaveDialog，預設檔名 `AI倒數喚醒-除錯日誌-時間.txt`）；匯出成功後狀態列顯示並以檔案總管選取該檔。
  - README 資料路徑段補充除錯日誌與匯出說明。
- 驗證：fmt、clippy `-D warnings` 通過；`cargo test --workspace --locked`（暫存 CARGO_TARGET_DIR，因正式 debug exe 被佔用）29 通過（新增 `debug_log_write_and_export`）、3 opt-in 忽略、0 失敗。以 `--verify-ui` 啟動 debug 版：除錯日誌實際寫入啟動、載入排程與 5 個 CLI 額度讀取；按鈕排在第二列且未被截斷；實際點擊匯出→存檔對話框→寫出檔案，內容含 BOM、版本、各 CLI 額度、設定／排程區段與除錯日誌，檔案總管開啟並選取該檔。
- 未驗證：系統匣選單項目未實際點擊；未在有排程實際執行 CLI 的情境下檢視執行相關紀錄（由程式碼與單元測試推定）；大量日誌匯出效能未測（在 UI 執行緒同步執行）。

## 2026-10-21 Claude：自動模式改為開始後每 10 分鐘偵測倒數（完成）

- 使用者目標「自動模式改成啟動時間開始後，每 10 分鐘讀取一次所有倒數狀態，沒有倒數則呼叫他」。
- 變更：
  - `calculator.rs`：新增 `auto_probe_interval()`（10 分鐘）與 `get_next_auto_probe_occurrence`（當日 +10 分鐘，跨過 24:00 排到隔天開始時間）；與既有 301 分鐘共用 `next_within_day`。
  - `manager.rs`：自動模式在當日開始時間後，固定每 10 分鐘讀取「所有」目標（含快取顯示倒數中的 CLI，不再略過），當日第一次（開始時間到或開機補做）立即讀取；未倒數者呼叫（仍受 2 分鐘冷卻）。不再使用設定的「額度自動重新整理分鐘」（該設定只影響看板）。倒數剛結束（快照觀測時間早於重置時間且重置已到）仍立即讀取該 CLI。全部倒數中時 `scheduled_at` 改為下一次讀取時點、不再寫入 `finished_at`；執行完成後有額度讀取器時排到 +10 分鐘，無讀取器時維持 301 分鐘。
  - UI 文字：模式下拉「自動模式（開始後每 10 分鐘偵測倒數）」、清單時間欄「10分偵測 (HH:MM)」、`models.rs` 顯示名稱、README。
  - 測試：`schedule_manager_quota_aware_interval` 第 3 段改以 `quota_auto_refresh_minutes = 0` 證明 10 分鐘節流與設定無關；新增第 5 段（倒數中也每 10 分鐘重讀、只呼叫未倒數者、scheduled_at=下一次讀取）與第 6 段（跨日計算）。
- 驗證：`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings` 通過；`cargo test --workspace --locked`（因 `target\debug\ai-wake-scheduler.exe` 正在執行被鎖，改用暫存 CARGO_TARGET_DIR）28 通過、3 opt-in 忽略、0 失敗。
- 未驗證：未以真實 CLI／真實額度 API 長時間執行；每 10 分鐘讀取所有 CLI 會比以往更常呼叫 Claude 額度 API，是否觸發 HTTP 429 未實測；未手動檢視新的下拉文字寬度。

## 歷史紀錄與目前限制

- 2026-10-17 與更早的原始筆記完整移至 [歷史交接紀錄](docs/AI_HANDOFF_HISTORY.md)，未刪除其他代理的證據。舊 C#／WinForms 與舊模式屬已過時紀錄；目前主體為 Rust workspace 與原生 Win32 UI。
- 目前正式發布為 v2.2.0；本輪時間範圍功能尚未發布或更新正式安裝。
