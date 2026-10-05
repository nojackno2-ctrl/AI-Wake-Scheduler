# v1.10.0：低價喚醒預設、額度讀取與一般權限啟動

更新三個 CLI 的模型設定，並改用 AGY 內建 `/usage` 讀取 Gemini／Claude 額度；不需啟動 IDE。修正微量用量倒數、跨 CLI 來源快取、設定按鈕被裁切與長終端輸出無法截斷的問題。

- 預設使用 Gemini 3.8 Flash Low、Codex GPT-6 Luna、Claude Haiku 4.5，AGY Claude 池使用 Sonnet 5.5 Low。新預設已實際回覆 OK；官方價格及 AGY 扣額限制見 [計價紀錄](https://github.com/nojackno2-ctrl/AI-Wake-Scheduler/blob/main/docs/MODEL_PRICING.md)。
- Codex 新增 GPT-6.1 Sol、GPT-6 Astra／Sol／Luna；Luna 自動限制至 Max。AGY Claude 更新 Sonnet／Opus 5.5 完整模型 ID，Claude CLI 加入固定版本選項。舊內建模型設定自動轉換，自訂模型保留。
- 主程式以一般使用者權限啟動；開機啟動改用目前使用者的 HKCU Run key。安裝器保留升級前啟動選擇，取消勾選會移除啟動鍵；支援清理舊版排程工作。
- 設定的儲存／取消按鈕固定於底部；終端長輸出保留上限正常生效。

## Windows x64 下載

- `AI-Wake-Scheduler_Setup_v1.10.0_x64.exe`：安裝版。
- `AI-Wake-Scheduler_Portable_v1.10.0_win-x64.zip`：免安裝版；完整解壓縮後執行 `AI倒數喚醒.exe`。
- `SHA256SUMS.txt`：以上兩個檔案的 SHA256 校驗碼。

兩種版本皆內建 .NET 執行環境，不需另外安裝 .NET 8 Desktop Runtime。使用前需安裝並登入所需 CLI。

## 驗證與限制

2026-10-05：核心測試 16/16、WinForms 測試 7/7 通過；Release 方案建置 0 警告／0 錯誤，格式與 Git 差異檢查通過。

真實喚醒測試四目標全部回覆 OK、退出碼 0（1/1 通過），額度整合 1/1 通過，四目標各取得 2 個額度視窗。低價預設的完整建置重新通過核心 16/16、視窗 7/7、額度整合 1/1。隔離 QA 安裝、保留／停用／啟用開機啟動的升級、解除安裝五階段全部通過；正式設定／排程／啟動鍵不變。免安裝 ZIP 解壓內容 hash 比對、正式安裝器版本及 SHA256 校驗通過。Windows 重開機登入、已有最高權限舊工作的遷移、真實系統匣通知外觀與所有候選模型推論尚未實測。
