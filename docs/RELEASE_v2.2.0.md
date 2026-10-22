# v2.2.0：只保留自動模式、額度用盡不再反覆呼叫、最便宜預設模型、CLI 更新提示

- **只保留自動模式**：移除「每天固定時間」與「自動倒數模式」，排程畫面不再有模式下拉，時間欄改為「開始時間」。舊排程載入時自動轉為自動模式並沿用原本的開始時間（每日模式取原排程時分）。
- **修正：額度用盡時反覆呼叫**：任一額度視窗（例如 Antigravity 每週額度）已用 100% 且尚未重置時不再呼叫該 CLI。過去 AGY(Gemini) 每週額度用完後，舊的自動倒數模式每 2 分鐘呼叫一次並得到 HTTP 429；清單現在顯示「額度用盡」與剩餘時間。
- **修正：Claude 額度 API 頻繁 HTTP 429**：移除自動倒數模式後，不再每 30 秒重讀倒數中的 CLI，固定每 10 分鐘偵測。
- **預設改用最便宜的模型**：Claude 預設由 Haiku 4.5（1／5 美元每百萬 token）改為 Haiku 5.5（0.10／0.50），舊設定的 Haiku 4.5 自動遷移一次（之後手動改回不再覆寫）。其他 CLI 預設已是各額度池最便宜的模型。
- **模型清單更新**：Codex 移除已不支援 ChatGPT 帳號的 gpt-5.5（舊設定轉為 gpt-6.1-sol）；AGY Gemini／Argon 補上 medium；Claude 加入 claude-haiku-5-5。
- **CLI 更新提示**：啟動 1 分鐘後與之後每 12 小時比對 agy、Codex、Claude Code 的本機與最新版本，有新版本時狀態列顯示並跳一次系統匣通知；主視窗與系統匣選單新增「檢查 CLI 更新…」，確認後開主控台執行各 CLI 自己的 `update`。

設定檔與排程檔可直接覆蓋升級 v2.1.0；設定檔會新增 `DefaultModelsRevision` 欄位。Claude 預設 Haiku 5.5 建議搭配 Claude Code 2.1.307 以上（較舊版本可呼叫，但會多印一行未收錄模型警告）。

## Windows x64 下載

- `AI-Wake-Scheduler_Setup_v2.2.0_x64.exe`：安裝版（可直接覆蓋升級 v2.1.0／v2.0.0／v1.x）。
- `AI-Wake-Scheduler_Portable_v2.2.0_win-x64.zip`：免安裝版；解壓縮後執行 `AI倒數喚醒.exe`。
- `SHA256SUMS.txt`：以上兩個檔案的 SHA256 校驗碼。

不需安裝任何執行環境。使用前需安裝並登入所需的 CLI（agy、codex、claude）。

## 驗證與限制

2026-10-22，於 Windows 11、Rust 1.99.0：

- `cargo fmt --check`、`cargo clippy -D warnings` 通過；`cargo test --workspace --locked` 30 通過（新增模式遷移、重現日誌的額度用盡不呼叫、預設模型一次性遷移、版本比較等測試），4 項需真實 CLI 的 opt-in 測試依設計略過。
- 真實模型呼叫：Codex 7 款、AGY Argon 4 款、Claude CLI 7 款（含 Haiku 5.5）回覆 OK；AGY Gemini Flash 與 Claude／GPT 池因每週額度已用完回 HTTP 429（額度問題，非模型 ID 問題）。
- 真實版本檢查：agy 1.4.4、Codex 0.163.0 已最新；Claude Code 偵測到新版並以 `claude update` 更新到 2.1.307。
- UI 端對端測試與擷圖：模式下拉已移除、「檢查 CLI 更新…」按鈕完整顯示。

未實測：自動模式以真實 CLI 長時間運作、點擊系統匣更新通知與更新確認對話框、由程式開啟的更新主控台、安裝包生命週期 QA、重開機自動啟動。

已發布：[v2.2.0](https://github.com/nojackno2-ctrl/AI-Wake-Scheduler/releases/tag/v2.2.0)（2026-10-22，Latest）。標籤與執行檔來源：8b2064251a063930b7842986ec4bf2a054c86e15，該提交 CI 五時區通過。GitHub 三項資產的官方 SHA256 digest 與本機一致；重新下載後 `sha256sum -c SHA256SUMS.txt` 兩項 OK。
