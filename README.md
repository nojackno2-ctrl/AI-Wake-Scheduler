# AI 倒數喚醒

一個以 C# / WinForms 製作的 Windows 排程工具。它會在指定時間向 Antigravity CLI、Codex CLI、Claude CLI 傳送一則簡短訊息（預設為「早安」），用途是啟動 AI 訂閱的使用倒數。

## 功能

- 建立並保存多筆每日排程；時間設定只需選擇每天的幾點幾分。
- 每筆排程可獨立選擇 Antigravity、Codex、Claude；三者到點時各自平行啟動，不會等待前一個 CLI 結束。
- 主畫面即時顯示每天執行時分與距離下一次執行的倒數。
- 可按「立即執行」手動觸發已保存排程。
- 縮到系統匣後繼續執行；可選擇登入 Windows 後自動啟動。
- 可自訂三個 CLI 的命令、完整路徑及額外參數。
- 預設開啟節省 Token 模式：低推理、低詳細度、短回覆、禁止 Claude 工具、Codex 唯讀沙箱。
- 「檢查三個 CLI」只執行 `--version`，不會送出 AI 訊息或消耗對話額度。
- 每次執行保存 stdout、stderr、結束碼與錯誤日誌至 `%LOCALAPPDATA%\AI倒數喚醒\logs`。

## Visual Studio 編譯

1. 使用 Visual Studio 2022 開啟 `AI倒數喚醒.sln`。
2. 確認已安裝「.NET 桌面開發」工作負載及 .NET 8 SDK。
3. 將 `AiWakeScheduler.WinForms` 設為啟始專案。
4. 選擇 `Debug` 或 `Release` 後按 F5。

命令列亦可執行：

```powershell
dotnet build '.\AI倒數喚醒.sln' --configuration Release
dotnet run --project '.\tests\AiWakeScheduler.Tests\AiWakeScheduler.Tests.csproj' --configuration Release
```

專案不使用第三方 NuGet 套件。

## CLI 呼叫方式

- Antigravity：`agy --print --effort low --disable-slash-commands <短提示>`
- Codex：`codex exec --skip-git-repo-check --sandbox read-only -c model_reasoning_effort=\"low\" -c model_verbosity=\"low\" <短提示>`
- Claude：`claude --print --effort low --tools "" --no-session-persistence <短提示>`

實際執行時，訊息會透過 `ProcessStartInfo.ArgumentList` 當作單一參數傳入，不會直接拼接成 Shell 指令。

節省 Token 模式會在使用者輸入的短詞後附加「只回覆上面這句，不要使用工具」。每個排程、每個 CLI 只呼叫一次，失敗不自動重試；訊息最多 50 個字元。新排程預設在 `%LOCALAPPDATA%\AI倒數喚醒\workspace` 空白目錄執行，避免掃描大型專案內容。

## 使用提醒

- 排程器位於此程式內，因此程式必須保持執行；按右上角關閉時預設只會縮到系統匣。
- 若電腦重新啟動，請在「CLI 設定」開啟登入 Windows 自動啟動。程式重新啟動後會立刻處理仍為等待狀態且已到期的排程。
- 第一次使用前先按「CLI 設定」→「檢查三個 CLI」。若 PATH 指向無法執行的 WindowsApps 別名，請按「瀏覽」指定實際可執行的 `.exe`。
- 排程真的到點時會產生 AI 請求。測試排程時可只勾選一個 CLI，確認後再啟用其餘項目。
