# 功能驗證紀錄

## 2026-10-05 v1.10.0 發布檢查

- 核心 16/16、WinForms 7/7；Release 方案建置 0 警告／0 錯誤；whitespace verify 及 diff check 通過。
- 最終低價預設四目標喚醒均 exit 0 且回覆 OK（Gemini 3.8 Flash Low、AGY Sonnet 5.5 Low、Codex GPT-6 Luna、Claude Haiku 4.5），wake smoke 1/1 通過。
- 額度整合初次因 Claude 憑證過期失敗；真實呼叫後重新檢查 1/1 通過，四個目標各 2 視窗。
- 最終正式自包含安裝包建置成功；低價版 QA Install、UpgradeKeepStartup、UpgradeDisableStartup、UpgradeEnableStartup、Uninstall 全 PASS，正式設定／排程／Run key 不變。免安裝 ZIP 解壓後所有 publish 檔案 hash 相同；正式安裝器版本與 SHA256SUMS 校驗通過。產物位於 dist/v1.10.0-final，使用者已授權 commit／push／發布；將重新建置對應提交版本後發布。

## 2026-10-04 歷史 QA 紀錄

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
