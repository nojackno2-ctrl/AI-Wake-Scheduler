# AI HANDOFF

## 2026-10-21 Claude：修正 Codex 5 小時倒數偵測不到（完成）

- 使用者回報「codex 的倒數 5 小時抓不到」。
- 證據（除錯日誌 debug-20261021.log）：喚醒後 Codex primary 視窗 `已用 0% 重置 15:00:20` 且重置時間固定不變，但被判為「未倒數」，自動模式因此每 2 分鐘（冷卻）重複呼叫 Codex。原因：`usage.rs::add_window` 要求 `usedPercent > 0` 才算倒數，而喚醒訊息用量四捨五入為 0%。同日 10:00:15（重置剛到、尚未喚醒）讀到重置時間 = 現在 + 5 小時，證實未開始的視窗重置時間會隨讀取時間滑動。
- 變更：`add_window` 另以「重置時間 + 1 分鐘容許 < 觀測時間 + 視窗長度」判定視窗已開始，即使 0% 也視為倒數中；`cli_usage_reader` 測試新增 0% 已開始／未開始兩種案例。
- 驗證：fmt、clippy `-D warnings` 通過；`cargo test --workspace --locked`（暫存 CARGO_TARGET_DIR）全數通過、3 opt-in 忽略。
- 實機驗證：重新建置 release 並從 `targeteleasei-wake-scheduler.exe` 啟動（安裝版 Program Files 未更新），12:12 啟動後 Codex 讀取顯示 `已用 0% 重置 15:00:21 倒數中`，未再呼叫 Codex。
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

## 2026-10-17 Claude：全螢幕比例修正（完成）

- 使用者回報全螢幕（4K、150%，邏輯 2560×1392）比例很差：卡片維持原高度、終端機佔半個畫面、清單過寬且 CLI 欄吃掉全部寬度。
- 變更（`app/src/main_window.rs`）：
  - 視窗放大倍率：以工作區 1440×860（DPI 縮放前）為設計基準，視窗更大時整體字型與版面等比例放大（0.1 級距、最多 1.5 倍）；新增 `base_dpi` 保存螢幕實際 DPI，`WM_GETMINMAXINFO` 最小尺寸以實際 DPI 計算不受放大影響；重建字型時釋放舊的清單列高影像清單。
  - 大螢幕版面（放大後內容寬 ≥ 2000 且高度放得下）：右欄上下堆疊排程內容與額度看板，左欄上下分配清單與終端機（終端機預設約四成五）。在 4K 150% 全螢幕時放大 1.5 倍後仍使用三欄版面。
  - 清單多出的寬度改依權重 [名稱 2、時間 1、倒數 3、CLI 3、狀態 1] 分配，不再全給 CLI 欄。
- 驗證：fmt、clippy `-D warnings` 通過；`cargo test --workspace --locked` 28 通過、3 opt-in 忽略、0 失敗。`--verify-ui` 擷圖：3840×2088（全螢幕工作區）為放大 1.5 倍的三欄、比例與預設尺寸一致；預設尺寸外觀不變。
- 未驗證：原生勾選框圖示由系統依實際 DPI 繪製，放大時不會跟著變大；大螢幕堆疊版面只在放大功能加入前擷圖過；超寬螢幕、DPI 切換未測。

## 2026-10-17 Claude：主視窗 UI 重排為自適應單頁版面（完成）

- 使用者目標「重做 UI，會自動調整欄位，一頁顯示所有資訊」。
- 變更（`app/src/main_window.rs`、`app/src/board.rs`）：
  - 標題列壓縮為一行（主標題與說明並排）；預設視窗改為 1480×900（依工作區上限縮小）。
  - 內容區寬度 ≥ 940（DPI 縮放前）時排成三欄：已儲存排程｜排程內容｜剩餘流量與重置倒數；較窄時清單移到上方、排程內容與額度看板在下方並排。編輯面板仍為同一個 `AiWakeEditorPanel` 子視窗（控制項 ID 與父子關係不變），內含兩張白底卡片；只有在空間真的不夠時才出現捲軸（判斷保留捲軸寬度容差，避免來回切換）。
  - 移除左右分隔線拖曳（欄寬改為自動）；保留終端機上下分隔線。終端機預設只拿上方內容用剩的高度（最小約工具列＋56px）。
  - 清單欄寬：寬度足夠時「CLI」欄吃掉剩餘寬度，不足時各欄等比例縮小（最小 48px）。
  - CLI 勾選框依寬度自動排成多欄格狀；「啟用此排程」放得下時與「建立排程／CLI 設定…」同列；「重新讀取倒數」移到額度看板卡片標題列右側。
  - 額度看板新增 `board::columns_for`：每欄至少約 320px、最多 3 欄，各 CLI 依目錄順序分欄排列。看板內容高度改變時會連同主框架重排（`relayout`）。
- 驗證：`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings` 通過；`cargo test --workspace --locked` 28 通過、3 opt-in 忽略、0 失敗（含端對端 UI 測試）。以 `--verify-ui` 啟動 debug 版並用 PrintWindow 擷圖人工檢視（150% DPI）：預設尺寸、1133×800、1000×720（邏輯像素）皆為三欄且排程內容、所有 CLI 額度列與終端機同頁可見、無捲軸。多次啟動期間 Claude 額度 API 回 HTTP 429（看板正確顯示受限訊息）。
- 未驗證：最小視窗 980×650 時排程內容仍需捲動（內容高度不足）；高對比模式、其他 DPI、DPI 切換、窄於三欄門檻的堆疊模式只在開發中短暫擷圖檢視，最終版本未重新擷圖；未建置 Release／安裝包、未發版。

## 2026-10-17 Codex：Rust／發布稽核（程式唯讀）

- 稽核子任務未 commit/push/merge/刪分支/tag/publish/改程式；父代理負責依使用者授權提交最終紀錄；開始工作樹乾淨。`fetch origin`（不 prune）成功，實際遠端僅 main，HEAD/origin=`15f0220bf93b95d2af4840ba32403d02fff5d5df`，無分支待合併。
- 最新正式 Release API=v2.0.0，現存 annotated tag peel=`85e9e79feae831240c71941ee8c077a46b9915f2`。tag..HEAD 僅 AI_HANDOFF.md、docs/RELEASE_v2.0.0.md 兩份文件；沒有新增 executable code，建議維持 v2.0.0，無須為本輪稽核重發版。
- gh CI：HEAD run 38988237514、release commit run 38988000420 均 success；HEAD UTC/Taipei/India/Pacific/AUS Eastern 五 job 全成功。
- 本輪 Windows Rust 1.99.0：`cargo fmt --all -- --check` exit 0；`cargo clippy --workspace --all-targets --locked --offline -- -D warnings` exit 0；`cargo test --workspace --locked --offline` 28 pass（app10/UI1/core17）、3 opt-in ignored、0 fail；`cargo build --release --locked --offline -p aiwake-app` exit 0。未跑真實 CLI、安裝、重開機測試。
- 三項遠端資產已全部重新下載至 `dist/audit-rust-2026-10-17`；SHA256 與 API digest/校驗檔完全一致。ZIP 僅 EXE+LICENSE，EXE 1,798,144 bytes、2.0.0，SHA256=F7E98974C3CC2198C09131E2266050A190B237DFF36BC799604E441D18F36CF8，與本輪本機 Release 產物相同。詳見 `D:/Github/audit-rust-2026-10-17.md`；Rust workspace/core/app 已取代追蹤中的 C#，仍依賴 Windows API 與外部已登入 CLI，不能宣稱跨平台。

## 2026-10-17 Claude：發布 v2.0.0（完成）

- 使用者要求「發布執行檔」，經詢問選擇版本 v2.0.0、發布到 GitHub Release（明確授權發布）。
- 變更：版本 1.10.0→2.0.0（`Cargo.toml`、`Cargo.lock`、`.iss`）；`http.rs` User-Agent 與 `usage.rs` Codex initialize clientInfo 改用 `env!("CARGO_PKG_VERSION")`；`.iss` 新增 `[InstallDelete]` 清除 v1.x .NET 殘留（`*.dll`、`*.pdb`、`*.deps.json`、`*.runtimeconfig.json`、`createdump.exe`、`zh-Hant\`）；README 下載檔名改為 Release 資產名稱；新增 `docs/RELEASE_v2.0.0.md`。
- 資產（`dist\v2.0.0\release\`，dist 不納入版控）：`AI-Wake-Scheduler_Setup_v2.0.0_x64.exe`（sha256 8152b6f0…6e15）、`AI-Wake-Scheduler_Portable_v2.0.0_win-x64.zip`（0a15b00c…7d8f，內含 `AI倒數喚醒.exe`＋LICENSE）、`SHA256SUMS.txt`。
- 驗證：見 `docs/FEATURE_VERIFICATION.md`「v2.0.0 發布檢查」。Inno Setup 編譯器為 Antigravity IDE 內附的 innosetup npm 套件版本（build-installer.ps1 自動找到）。
- 發布：提交 85e9e79 CI 五時區 success 後建立附註標籤 `v2.0.0` 並 `gh release create`（Latest、非 draft／prerelease）。三項資產 GitHub digest 與本機一致，重新下載後 `sha256sum -c` 通過。
- 未驗證：真正 v1.10.0 安裝版直接升級、重開機自動啟動、系統匣通知外觀、Argon 以外 CLI 的真實喚醒。

## 2026-10-17 Claude：AGY Argon 呼叫獨立成第五個目標（完成）

- 使用者目標「agy 的 argon 呼叫獨立出來」。
- 設計：新增 `CliKind::AntigravityArgon`，加在列舉最後（舊版資料檔可能以整數序號保存目標，Antigravity=0…Claude=3 不可變動）；畫面與新排程預設目標順序改由 `catalog::kinds()`（目錄順序：Gemini、Argon、Claude/GPT、Codex、Claude）決定，不再依賴 `CliKind::ALL`。
- 變更：
  - `catalog.rs`：新增 Argon 描述（`agy`、預設 `gemini-4-argon`、AGY_FLASH effort、`--print` 結尾）；Gemini 推薦清單移除 Argon，改列 `gemini-3.8-flash-low/high`（維持 ≥3 項）；新增 `kinds()`。
  - `usage.rs`：`matches_pool` 改以群組＋bucket 名稱分流，Argon 先排除再判斷 Gemini（群組名「Gemini 4 Argon」也含 gemini）；`pool_name` 改取目錄顯示名稱。
  - `command_builder.rs`、`login.rs`、`main_window.rs`（登入後清除 AGY 連線快取）納入 Argon。
  - `board.rs`：移除「Argon 每週」特例，Argon 群組內直接顯示「每週」。
  - `models.rs`：新排程預設目標與空目標補回改用 `catalog::kinds()`；舊設定由 `ensure_defaults` 自動補上 Argon 設定。
  - 既有排程的目標不會自動加入 Argon，需使用者自行勾選。
- 驗證：fmt、clippy `-D warnings` 通過；`cargo test --workspace --locked` 28 通過、2 opt-in 忽略；opt-in `executable_locator_resolution`（唯讀，不耗 Token）真實讀到 Gemini 2 視窗、Argon 1 視窗（每週 100%）、Claude/GPT 2 視窗。
- 真實 Argon 喚醒（使用者明確同意）：新增 opt-in 測試 `real_argon_wake_smoke`（只喚醒 Argon，走 `ProcessCliRunner`，再重讀額度）；實際呼叫 `agy --model gemini-4-argon --effort low --disable-slash-commands --print-timeout 180s --print …` exit 0、回覆 OK，額度重讀 `is_active_countdown=true`，重置時間更新為呼叫時間＋7 天（2026-10-24T00:39:51Z）。`real_cli_wake_smoke` 案例清單也加入 Argon，但本次未執行（會喚醒全部 CLI）。
- 未驗證：未在 VS／桌面手動檢視新增的第五個勾選框與設定列版面（UI 端對端測試已涵蓋控制項存在與執行數量）。

## 2026-10-17 Claude：新增 Visual Studio 方案，可在 VS 內編譯與測試（完成）

- 使用者目標「可以直接在 Visual Studio 編譯測試」。
- 新增：
  - `AI倒數喚醒.sln`、`AI倒數喚醒.vcxproj`（NMake/Makefile 專案，PlatformToolset v145，VS 2022 自動改用 v143）、`AI倒數喚醒.vcxproj.filters`（方案總管分資料夾顯示 .rs、Cargo、腳本、文件）。
  - 組態：`Debug`＝cargo build、`Release`＝cargo build --release、`Test`＝fmt --check → clippy -D warnings → cargo test；F5 啟動對應的 `target\<profile>\ai-wake-scheduler.exe`。VS 中繼檔放在 `target\vs\`。
  - `tools/vs-cargo.ps1`（UTF-8 BOM，相容 PowerShell 5.1）：以 `--message-format=short` 執行 cargo，把診斷、rustfmt 差異、測試 panic 位置轉為 `檔案(行,欄): error 代碼: 訊息`，供 VS 錯誤清單點擊跳轉。已處理新版 Rust panic 訊息中的執行緒 ID `(12345)`。
  - `.gitignore` 加入 `/.vs/`；README 新增「在 Visual Studio 中建置與測試」。
- 驗證（VS 2026 Community 18.10.4 的 MSBuild 命令列）：
  - `msbuild AI倒數喚醒.sln /p:Configuration=Debug|Release|Test /p:Platform=x64`：三者皆 exit 0；Test 組態 28 通過、2 opt-in 忽略；繁中輸出不亂碼。
  - 負向測試（暫時修改後已還原並確認 `git diff` 乾淨）：型別錯誤 → `fake_cli.rs(23,5): error E0308`；rustfmt 差異 → `fake_cli.rs(19): error rustfmt`；失敗單元測試 → `tokenizer.rs(59,9): error test`；三者 MSBuild 均失敗。
  - 注意：以舊時間戳的備份還原原始碼時，cargo 會因 mtime 判定未變而沿用舊產物；需更新檔案時間。
- 未驗證：VS IDE 介面內的實際操作（Ctrl+Shift+B、錯誤清單點擊、F5 除錯）只以同一 MSBuild 目標推定；Rebuild／Clean（會執行 cargo clean）未執行；VS 2022 v143 分支未測。測試不會出現在測試總管。

## 2026-10-17 Claude：專案健康檢查（僅檢查，未改程式）

- Git：`main` 乾淨，與 `d6b6180` 同步；GitHub CI 最近一次（d6b6180）成功，前一次 b83fcf5 失敗已由後續提交修復。
- 本機驗證（rustc 1.99.0，與 CI 釘選版本相同）：
  - `cargo fmt --all -- --check`：通過。
  - `cargo clippy --workspace --all-targets --locked -- -D warnings`：通過。
  - `cargo test --workspace --locked`：28 通過、2 項 opt-in 依設計忽略、0 失敗。
- 靜態掃描：無 TODO/FIXME；非測試程式的 `expect` 僅用於靜態 regex 編譯與目錄查表，`time::local_wall_clock` 的 `expect` 只有測試呼叫。
- 已處理：`docs/FEATURE_VERIFICATION.md` 改寫為純 Rust 驗證紀錄（現行指令、2026-10-16／10-17 結果、測試覆蓋對照、未驗證項目），C#/WinForms 舊內容原文移至文末「歷史紀錄」。已確認文件中的 opt-in 指令 `cargo test -p aiwake-core --test core_tests -- --ignored` 可正確解析（列出 19 項測試）；未實際執行 opt-in 真實 CLI 測試。
- 未驗證：本次未重跑 `build-release.ps1`、`build-installer.ps1`、`tests/Verify-Installer.ps1`。

## 2026-10-16 Antigravity：專案全面改為純 Rust 語言架構（完成）

- 使用者目標「整個專案改成純Rust語言」。
- 結構重構：
  - 將 Rust workspace 移至專案根目錄（根目錄 `Cargo.toml`、`Cargo.lock`、`rustfmt.toml`、`core/`、`app/`、`build-release.ps1`）。
  - 完全移除 C# 實作與方案檔（`AI倒數喚醒.sln`、`Directory.Build.props`、`src/`、`tests/AiWakeScheduler.Tests`、`tests/AiWakeScheduler.WinForms.Tests`）。
  - 更新 `app/build.rs` 圖示相對路徑為 `../assets/app.ico`。
  - 更新 `installer/AI倒數喚醒.iss` 預設 `PublishDir` 為 `..\bin\publish-release`。
  - 重寫 `build-installer.ps1`：自 `Cargo.toml` 讀取版本號並與 `.iss` 校驗，執行 `cargo test` 與 `cargo build --release -p aiwake-app`，將純 Rust 單一執行檔打包至 Inno Setup 安裝包；支援 `-VerificationBuild` 參數。安裝包大小自原 C# 自包含的 48.4 MB 巨幅縮減至 2.31 MB！
  - 調整 `build-release.ps1` 產出預設至 `dist\AI倒數喚醒.exe`。
  - 調整 `tests/Verify-Installer.ps1` 相容 PowerShell 5.1（處理 ProcessStartInfo 參數傳遞）。
  - 更新 `.github/workflows/ci.yml` 為純 Rust 跨時區矩陣測試工作流程。
  - 更新 `.gitignore`、`README.md`（純 Rust 特色、架構圖、建置指引）與 `AGENTS.md`。
  - 修復測試極限速度下的競態問題：`ui_smoke.rs` 假 CLI 探測速度小於 50ms 輪詢間隔時改以標記檔精準確認完成；`core_tests.rs` 多執行緒測試並發下的 Windows 行程啟動延遲容許值調寬。
- 驗證：
  - `cargo fmt --all -- --check`：通過。
  - `cargo clippy --workspace --all-targets --locked -- -D warnings`：0 警告 0 錯誤通過。
  - `cargo test --workspace --locked`：28/28 全數通過（App 單元測試 10/10、端對端 UI 視窗測試 1/1、Core 核心測試 17/17，2 項 opt-in 依設計忽略）。
  - `.\build-release.ps1`：成功產出 `dist\AI倒數喚醒.exe`，SHA256 校驗正常。
  - `.\build-installer.ps1`：成功編譯安裝程式 `dist\AI倒數喚醒_Setup_v1.10.0_x64_<時間戳>.exe`（2.31 MB）。
  - `.\tests\Verify-Installer.ps1`：QA 安裝包 5 階段完整生命週期（Install、UpgradeKeepStartup、UpgradeDisableStartup、UpgradeEnableStartup、Uninstall）全數 PASS，正式設定、排程與 Run key baseline 維持不變。
- 狀態：專案已完全轉換為純 Rust 語言，依授權自動 commit 與 push。

## 2026-10-16 Antigravity：更新 AGY 呼叫參數、預設模型與 Argon 額度視窗（完成）

- 使用者目標「更新AGY的呼叫跟倒數讀取」。配合附圖中的 Antigravity 額度（含 Gemini Models 5h/每週、Claude/GPT 5h/每週、Gemini 4 Argon 每週 89%）。
- 呼叫參數：CLI 1.4.0 在 `--disable-slash-commands` 時 `--mode plan` 會發出警告且無效；已從 C# (`CliCatalog.cs`) 與 Rust (`catalog.rs`) 的 `TokenSaverArguments` 移除 `"--mode", "plan"`。經真實 CLI 測試 `agy --model gemini-4-argon --effort low --disable-slash-commands --print-timeout 3m0s --print "OK"` 回傳 OK、結束碼 0。
- 預設模型：參照 `agy models` 清除已淘汰模型（`gemini-3.7-flash`、`gemini-3.6-flash`、`gemini-3.1-pro`），加入新旗艦模型 `gemini-4-argon`（支援 Low、Medium、High 與 Default 思考程度），並保留既有相容對照。
- 額度與倒數視窗修復：過去 `SelectMostConstrainedWindows` 僅依視窗長度（Duration 7天）分組，導致 Argon 每週視窗覆蓋並漏失 Gemini 共同每週視窗。已在 C# (`CliUsageReader.cs`) 與 Rust (`usage.rs`) 改為依 `(IsArgonWindow, Duration)` 分組，完整保留 5 小時、每週、Argon 每週 3 個視窗。
- 額度看板呈現：C# (`UsageBoard.cs`) 與 Rust (`board.rs`) 將 Argon 每週視窗專屬標示為「Argon 每週」。
- 驗證：
  - C#：`dotnet build AI倒數喚醒.sln -c Release` 0 警告 0 錯誤；`AiWakeScheduler.Tests` deterministic 17/17 通過；真實 CLI 整合測試（`--integration`）讀取成功，`Antigravity (Gemini)` 正確顯示 3 個視窗（5h 91%、Weekly 97%、Argon Weekly 88%），`Antigravity (Claude / GPT)` 顯示 2 個視窗（5h 100%、Weekly 100%）。
  - Rust：`cargo clippy --workspace --all-targets -- -D warnings` 通過；`cargo test -p aiwake-app --lib` 10/10 通過；`core_tests` 包含 Argon 視窗獨立保留之單元測試全數通過。
- 狀態：已完成驗證並保持 C# 與 Rust 雙語言邏輯同步。依照授權執行 commit 與 push。

## 2026-10-14 Antigravity: automatic commit and push to GitHub policy

- User authorized automatic local commits and automatic push to GitHub for all projects; AGENTS.md now records the standing policy. A Git post-commit hook has been installed in `.git/hooks/post-commit` to automatically push the current branch to `origin` upon commit. Destructive operations (force-push, release, merge, rebase, reset, branch deletion) still require separate authorization.
- Evidence: inspected branches, git remotes, and status across all repositories. Verified hook execution on Windows Git. Pushed existing unpushed commits where applicable.
- Verification: git status clean, post-commit hook configured and tested, collaboration rules updated.
- Status: auto-push policy established and active.

## 2026-10-12 Claude：改寫成 Rust（核心與桌面程式完成，與 C# 版並存）

- 使用者目標「程式改寫成Rust語言」。已確認的決策：Rust 版放在 `rust/`，與 C# 版並存到功能對等後再另行決定是否移除 C#；UI 以 Microsoft `windows` crate 直接寫原生 Win32；相依只用少量主流 crate（serde_json、chrono、regex、windows；建置時另用 winresource 嵌入圖示與資訊清單），HTTP 走 WinHTTP、不引入 async runtime。本機 Rust 為 stable 1.99.0（rustup 原已安裝，只是不在 PATH：`%USERPROFILE%\.cargo\bin`）。
- `rust/core`（crate `aiwake-core`，commit f4ce6eb 起）：對應 C# Core 全部檔案。async/Task 改為執行緒＋Condvar，取消改為 `CancelToken`，`TimeProvider` 改為 `Clock` trait。資料檔格式與 C# 版相容（PascalCase、字串列舉、`hh:mm:ss`、ISO 8601 最多 7 位小數），測試含讀取舊版格式。
- `rust/app`（crate `aiwake-app`，執行檔 `ai-wake-scheduler.exe`）：對應 WinForms 專案。主視窗（清單為 ListView、編輯器為可捲動面板、額度看板自繪、終端機為 RichEdit、可拖曳的兩條分隔線、狀態列）、設定對話框（可捲動內容＋固定在底部的儲存／取消）、系統匣與氣球通知、Run key 開機啟動與舊排程工作遷移、`--minimized`／`--verify-ui`。自繪按鈕保留主要／次要／危險三種樣式與高對比模式。視窗狀態全部用 `Cell`/`RefCell` 內部可變性，背景執行緒一律經 `UiQueue` 排回 UI 執行緒。
- 兩個版本共用資料目錄、單一執行個體 mutex 名稱與 Run key 值名稱：不能同時執行，正式模式啟動 Rust 版時若已啟用開機啟動，Run key 會被改指向 Rust 執行檔（與 C# 版的遷移行為相同）。我所有的實機執行都只用 `--verify-ui` 隔離模式，未碰正式資料、正式排程與 Run key。安裝包（Inno Setup）仍打包 C# 版，未改動。
- 驗證（本機 UTC+8，最終程式碼）：`cargo fmt --all -- --check` 通過；`cargo clippy --workspace --all-targets --locked -- -D warnings` 通過；`cargo test --workspace` 核心 17/17、App 單元 10/10（排程文字、額度看板整形與格式、啟動命令）、端對端 UI 1/1。端對端測試啟動真正的 debug 執行檔，以視窗訊息驗證：編輯器預設值、建立／修改排程並比對隔離 `schedules.json`、關閉縮到系統匣與還原、設定對話框的模型↔思考程度連動（Sol Ultra→Luna Max、完整模型 ID 只留預設）、取消不寫檔、連線檢查、儲存寫入 `settings.json`、把四個 CLI 指向假批次檔後「立即執行」→確認對話框→三個目標平行執行→終端機輸出→查看結果、清除終端機、新增、關閉即結束（結束碼 0）。
- 其他已實測：opt-in `cargo test -p aiwake-core -- --ignored executable_locator_resolution` 1/1（最終程式碼）：四個真實 CLI 的 `--version` 與額度讀取（agy /usage、Codex app-server、Claude OAuth usage 經 WinHTTP）皆 Available。`cargo build --release` 成功，執行檔 1,796,096 bytes；release 版 `--verify-ui` 可啟動並正確繪製（PrintWindow 擷圖人工檢視：主視窗、編輯器底部含真實額度看板、設定對話框、執行後終端機），`--verify-ui --minimized` 啟動後無可見視窗、行程存活。`rust/build-release.ps1`（測試＋release 建置＋複製）執行成功，產出 `dist\rust\AI倒數喚醒.exe`，SHA256 876EDB1428151F8FE78F1EE96B827CF6CBFD6F71173E1CE187A0EE03827AD3D9（`dist/` 不納入版控）。
- 未驗證：GitHub Actions 的 `rust-build-and-test` 工作（已加入 `.github/workflows/ci.yml`，尚未 push，沒有任何一次實際執行；UTC+8 以外的時區只會由該矩陣驗證）。系統匣氣球通知外觀與點擊登入、CLI 登入主控台流程、資料夾／檔案選擇對話框、分隔線拖曳、跨螢幕 DPI 變更、高對比模式、正式模式的 Run key 寫入與 schtasks 舊工作遷移、長時間常駐／睡眠恢復、經 Rust UI 的真實 CLI 喚醒（`real_cli_wake_smoke` 會消耗 Token，未執行）。
- 注意：本機「應用程式控制原則」(os error 4551) 會不定期封鎖 cargo 新產生、未簽章的測試或 build script 執行檔（曾封鎖假 CLI 的 unittest 執行檔、一次重編後的 `ui_smoke` 測試檔，以及換工具鏈後的 build script）。這是使用者機器的安全政策，不可繞過；遇到時只能如實回報。曾嘗試加入 `rust-toolchain.toml` 固定版本，結果 rustup 另外下載了一份 1.99.0 工具鏈且新 build script 被封鎖，已移除該檔並解除安裝那份重複的工具鏈；版本固定改寫在 CI 工作流程內。
- 與 C# 版的已知差異：按鈕、清單與輸入框是系統原生控制項的外觀；清單列高 34（C# 為 42）；設定對話框的模型欄寬比例改為 32/38/30；額度看板以內容比對（而非物件參考）判斷是否重排版面。`.cmd` 形式的 CLI 經 `cmd.exe /c` 執行時，節省 Token 提示詞中的換行會截斷其後的參數——這是從 C# 版原樣移植的既有行為，兩版相同，未修改。
- 測試用的隔離資料夾留在 `%TEMP%\AiWakeScheduler-QA-*`（手動啟動幾次留下的，內容只有空的設定與日誌目錄），可自行刪除。

## 2026-10-07 Codex：自動倒數模式（完成）

- 工作開始時 main=7c4a835，工作樹乾淨，已讀取規則及交接。新增 Countdown 週期與 UI 第三個選項；指定每日開始偵測時間，逐 CLI 查真實倒數，未倒數呼叫、倒數中等待，午夜後至翌日開始前不自動呼叫。
- 與舊 Interval 分開；不受額度看板自動更新開關影響，讀取失敗重試、不當成未倒數；呼叫冷卻 2 分鐘避免 API 倒數尚未反映時連發。偵測最長 30 秒一次，倒數結束時間可提前喚醒。
- 初次 Release build 發現 JobPresenter 時間格式逸出錯誤（CS1009），已修正；Release build 0 警告／0 錯誤，核心首輪 17/17 通過（新模式覆蓋每日開始、逐 CLI、多輪倒數、午夜與隔日、刷新關閉、失敗保護及持久化）。最終 `dotnet build AI倒數喚醒.sln -c Release` 0 警告／0 錯誤、核心 17/17、WinForms 7/7；新增重啟／查詢跨午夜、WinForms 實際編輯事件／持久化／呈現均通過。`dotnet format ... whitespace --verify-no-changes --no-restore` 與 `git diff --check` 通過。
- README 已加入使用方式；午夜前已啟動的 CLI 會正常完成，手動「立即執行」保留原本行為。尚未實際連續運行全天／睡眠恢復或安裝新版，未 push／發布；排程時間與查詢狀態以可控時鐘及假 CLI 驗證，未宣稱全天真實服務端端到端測試。

## 2026-10-06 Claude: schedule tests are time-zone independent

- Replaces the earlier CI-only Taipei time-zone workaround. Schedule tests now build their instants with `LocalWallClock(...)` (a `DateTimeKind.Local` wall-clock time) instead of fixed `+08:00` offsets, matching the product's use of `TimeZoneInfo.Local`. `TestCliUsageReaderAsync` keeps its fixed +08:00 instants on purpose (it compares absolute instants against the JSON).
- CI now runs the full build and both test runners on a time-zone matrix: UTC, Taipei, India (+05:30), Pacific and AUS Eastern (both DST zones).
- Local (UTC+8): core 16/16. Other zones are verified only through the CI matrix (no WSL/second zone available locally).

## 2026-10-06 Claude: CI time zone

- First CI run failed 13/16: ScheduleCalculator / ScheduleManagerAutoInterval / ScheduleManagerQuotaAwareInterval assume a UTC+8 local zone (fixed +08:00 offsets vs `TimeZoneInfo.Local` in the product) and the runner is UTC. CI now sets `tzutil /s "Taipei Standard Time"` before tests. The tests remain time-zone dependent when run locally outside UTC+8.

## 2026-10-06 Claude: add CI

- Added `.github/workflows/ci.yml`: Release build of `AI倒數喚醒.sln`, then both console test runners via `dotnet run --no-build`. Local Release: build 0/0, core 16/16, WinForms 7/7 (run on the .NET 10 runtime with roll-forward; no .NET 8 desktop runtime locally). CI not yet observed on GitHub.

## 2026-10-05 Codex: automatic local commit policy

- User authorized automatic local commits without further confirmation; AGENTS.md now records the standing policy. Push/release/history changes require separate authorization.
- Coordination: deferred edits while the release task was active; rechecked that the task completed and the working tree was clean before editing. Existing release evidence is preserved.
- Evidence: inspected branch, git status, git diff, and recent commits. This task changes collaboration documentation only; no build/test or runtime behavior is claimed.
- Verification: full staged git diff --cached --check passed; automatic commit wording and remaining authorization limits were checked. Documentation-only work; no build/runtime validation was needed.
- Status: automatic commit policy established; this record accompanies the authorized local snapshot.


## 2026-10-05 Codex：v1.10.0 發布（完成）

- 使用者已明確授權 commit、push 並發布 v1.10.0；開始時 main=febe4e9，remote main 同步，所有待提交變更為前輪功能更新及本次發布準備，未發現其他代理新增修改。
- 完成：低價預設 Gemini 3.8 Flash Low、AGY Sonnet 5.5 Low、Codex GPT-6 Luna、Claude Haiku 4.5；四目標經產品 Runner 真實回覆 OK、exit 0（wake 1/1）。官方價格及 AGY 完整扣額排名未公開的限制見 docs/MODEL_PRICING.md。
- 前輪最終 build-installer exit 0：核心 16/16、WinForms 7/7、額度整合 1/1（四目標各 2 視窗）、自包含 publish、Inno 編譯。Release 方案 0 警告／0 錯誤；格式與 diff check 通過。
- 隔離 QA 五階段 Install／UpgradeKeepStartup／UpgradeDisableStartup／UpgradeEnableStartup／Uninstall 全 PASS，正式設定／排程／Run key baseline 不變。免安裝 ZIP 解壓後 263 個 publish 檔案 hash 相同，版本及 SHA256SUMS 通過。
- 已解決嘗試：Claude 額度初次憑證過期，真實呼叫後重驗成功；舊 Codex 參數期望與視窗空白初值假設失敗已更新；漏加測試變數的編譯錯誤已修正；exe ProductVersion 的 commit 後綴及 Inno padding 已按 numeric version／Trim 校驗。
- 已完成原始碼提交 f060c30ade8a3f19207c303a0be2dce7304441a2；commit 包含模型／額度／啟動／UI 修正、測試、版本及文件，重新核心 16/16、WinForms 7/7、額度整合 1/1 與 self-contained publish 通過；exe ProductVersion 實際對應該提交，完整 build-installer exit 0，Inno 成功；push main 成功且 GitHub API 確认 remote SHA 相同。正式 ASCII 安裝包及 portable ZIP 已建置，解壓所有 263 個 publish 檔案 hash 相同，exe 來源與 setup 版本通過。GitHub 三項資產上傳完成並校驗。
- GitHub draft id=403304641 已建立，上傳三個資產完成；API 的 target_commitish=f060c30ade8a3f19207c303a0be2dce7304441a2，exe／ZIP／SHA256SUMS 三個 digest 與本機一致。draft 的 tags API 回 404，改依確切 release id 讀取成功；gh download 初期大型資產長時間顯示零位元組；準備取消前程序已自行成功結束（未實際終止）。最終三項完整下載與本機逐一 hash 相同，下载的 SHA256SUMS 逐行驗證通過。先前 b225bb8 文件的未完成描述已由此新證據更正。
- 已正式發布：https://github.com/nojackno2-ctrl/AI-Wake-Scheduler/releases/tag/v1.10.0；API latest=v1.10.0、draft=false、prerelease=false，tag 確認為 f060c30ade8a3f19207c303a0be2dce7304441a2。正式安裝包 48,397,912 bytes／SHA256 f90d02e246970488e957d92674885dcae1fa1e5a33c90bbaa32071012b76f533；portable 66,970,495 bytes／SHA256 aebf6a834f1d5cd81d74d61059c882aa9f9d980be8f04977a44afc029cf67291；SHA256SUMS digest b858cc18a6c60cbfa5fecdf47189d187afc747c6f990b3e3a4b996ac17c98721。發布證據文件已提交／推送，後續補正完整下載成功紀錄；程式碼維持標籤來源提交。
- 前輪 dist/v1.10.0-final 產物尚未發布，因 exe 帶舊 HEAD 後綴不能作正式來源證據。已提交並重新建置至 dist/v1.10.0-release，使用 ASCII 資產檔名與實際檔名 SHA256SUMS；此前測試產物保留但不作正式下載。
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

- 最近正式發布為 v1.10.0，tag／執行檔來源 f060c30；原始碼已 commit／push，GitHub 最新正式 Release 及三項資產 SHA256 均確認。未更新本機正式安裝。
- 核心：.NET 8、WinForms、繁體中文、無額外 NuGet；四個目標平行喚醒，自動排程依各 CLI 的真實五小時額度倒數。
- 一般權限 manifest／HKCU 啟動與安裝器遷移已更新，安裝／升級／解除安裝已通過；此機器無舊排程工作，尚未實測已有最高權限工作的遷移與重開機登入。
- 舊紀錄已完整移至 [歷史交接紀錄](docs/AI_HANDOFF_HISTORY.md)，包含版本發布證據、已解決問題、失敗嘗試與 UI QA 限制。
