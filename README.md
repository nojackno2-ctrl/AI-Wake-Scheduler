# AI 倒數喚醒 (AI Wake Scheduler)

[![.NET 8.0](https://img.shields.io/badge/.NET-8.0-512BD4?logo=dotnet&logoColor=white)](https://dotnet.microsoft.com/)
[![Platform](https://img.shields.io/badge/Platform-Windows%2010%20%2F%2011-0078D6?logo=windows&logoColor=white)](https://www.microsoft.com/windows)
[![UI](https://img.shields.io/badge/UI-WinForms%20(Native)-blue)](https://learn.microsoft.com/dotnet/desktop/winforms/)
[![Dependencies](https://img.shields.io/badge/Dependencies-Zero%20NuGet-success)](#)
[![License](https://img.shields.io/badge/License-MIT-green.svg)](#)

一個專為 AI 輔助開發者設計的 Windows 輕量排程工具。

許多 AI 開發工具（如 Google Antigravity、Anthropic Claude Code、OpenAI Codex）採用**滾動時間窗口額度機制**（例如「5 小時重設窗口」，自發送第一次請求起算）。本工具可在每天指定的固定時間，自動向各大 AI CLI 發送極簡短語（預設為「早安」），自動觸發並啟動使用額度倒數，確保你在開始工作時，AI 的使用額度已經準備就緒。

---

## ✨ 核心特色

- ⏰ **極簡每日排程**：直接設定每天固定觸發時分（`HH:mm`），支援多組排程管理，主畫面即時顯示距離下一次執行的倒數計時。
- 🚀 **四大多模型目標平行喚醒**：同時支援 **Google Antigravity (`agy`) Gemini 額度池**、**Antigravity Claude / GPT 額度池**、**Anthropic Claude (`claude`)** 與 **OpenAI Codex (`codex`)**。多個 CLI 到點時平行獨立啟動，互不等待與阻塞。
- 📊 **真實額度與重置倒數**：透過 Codex 官方 app-server 唯讀讀取剩餘百分比與重置時間；AGY／Claude CLI 未提供可機器解析介面時會明確標示「不支援」，不以推測值冒充帳戶資料。
- 💡 **極致 Token 節省模式 (Token Saver)**：
  - 自動套用最低推理與簡短模式（`--effort low`、`model_reasoning_effort=low`）。
  - 禁用 Claude 所有 Tool 呼叫（`--tools ""`）。
  - 啟用 Codex 唯讀沙箱（`--sandbox read-only`）。
  - 隔離於空白專用工作區（`%LOCALAPPDATA%\AI倒數喚醒\workspace`），避免掃描本機龐大原始碼目錄。
  - 自動附加限制指令（「只回覆上面這句，不要使用工具」），將 Token 消耗降至極限。
- 🛡️ **安全無注入設計**：透過 .NET 原生 `ProcessStartInfo.ArgumentList` 結構化傳遞參數，杜絕 Shell 注入與跳脫字元問題。
- 🔍 **無損安全檢查**：提供「檢查全部 CLI」功能，僅執行 `--version` 探針檢查，絕不送出 AI 訊息或消耗對話額度。
- 🪟 **輕量背景常駐與開機自啟**：原生 Windows Forms 打造，關閉主視窗時自動縮至系統匣（System Tray）持續倒數；支援登入 Windows 自動啟動。
- 📦 **零外部 NuGet 相依**：100% 依賴 .NET 8 原生基礎類別庫（BCL），無任何第三方套件負擔，編譯乾淨且維護容易。
- 📜 **完整日誌追蹤**：完整留存每次執行的 stdout、stderr、結束代碼與錯誤記錄，排查問題一目了然。

---

## 🖥️ 支援的 AI CLI 工具與額度池

| AI 工具 / 額度池 | 預設執行檔命令 | 喚醒呼叫範例（節省 Token 模式） | 預設節省機制說明 |
| :--- | :--- | :--- | :--- |
| **Antigravity (Gemini)** | `agy` | `agy --effort low --disable-slash-commands --mode plan --print <提示>` | 所有旗標置於 `--print` 前、低思考 effort、停用技能展開、禁止修改工作區 |
| **Antigravity (Claude / GPT)** | `agy` | `agy --model claude-sonnet-4-6 --disable-slash-commands --mode plan --print <提示>` | 明確選擇 Claude 額度池；該模型不支援 `--effort`，因此不傳入無效旗標 |
| **OpenAI Codex** | `codex` | `codex exec --ephemeral --sandbox read-only --ignore-user-config --ignore-rules … <提示>` | 一次性非互動 exec、唯讀沙箱、不載入 MCP／使用者規則、低推理與低詳細度 |
| **Anthropic Claude** | `claude` | `claude --print --safe-mode --tools "" --no-session-persistence --prompt-suggestions false <提示>` | 停用自訂內容與工具、不持久化 Session、不額外生成提示建議 |

> [!TIP]
> Google Antigravity 內部將額度切分為 **Gemini Models** 與 **Claude and GPT models** 兩組獨立計數器。勾選本工具中的這兩項目標，可同時喚醒兩邊的 5 小時滾動重設窗口！
> 實際執行時，使用者訊息會以安全參數傳入，每個排程與每個 CLI 僅呼叫一次且失敗不自動重試，最大字數限制為 50 字元。

---

## 🚀 快速上手

### 1. 下載與安裝

#### 方法 A：使用 Windows 安裝版（推薦）
1. 前往 Releases 下載最新版 **`AI倒數喚醒_Setup_v1.2.0_x64.exe`**。
2. 執行安裝程式，依照精靈指示選擇安裝位置與是否建立桌面捷徑。
3. 安裝完成後可勾選立即啟動，或於開始功能表 / 桌面捷徑啟動。
4. 安裝版已內建完整獨立執行環境（Self-Contained），你的電腦**無須預先安裝 .NET 8 Runtime** 即可直接運行。

#### 方法 B：綠色免安裝版 / 自行編譯
- 下載免安裝可執行檔或從原始碼建置，直接執行 `AI倒數喚醒.exe`。

### 2. 初次設定與 CLI 檢查
1. 點擊主畫面右下角的 **「CLI 設定…」** 按鈕。
2. 點擊 **「檢查全部 CLI」** 按鈕，程式會自動執行 `--version` 檢查本機 CLI 是否已就緒。
3. 若你的 CLI 安裝於特殊路徑（例如透過 npm、專屬安裝目錄等），可點擊 **「瀏覽…」** 自訂 `.exe` 完整路徑。
4. （可選）勾選 **「登入 Windows 後自動啟動」**，確保開機或重啟後仍能在背景常駐。
5. 點擊 **「儲存」**。

### 3. 建立每日排程
1. 在右側「排程內容」區：
   - **名稱**：輸入易辨識的名稱（例如「早晨 AI 喚醒」）。
   - **每天時間**：設定每天觸發的時與分（例如 `08:00`）。
   - **工作目錄**：選擇或保持預設隔離目錄。
   - **傳送至**：勾選欲啟動的 CLI 目標（Antigravity (Gemini) / Antigravity (Claude / GPT) / Codex CLI / Claude CLI）。
   - **訊息**：輸入觸發短語（預設為「早安」）。
2. 點擊 **「建立排程」**。選取既有排程後按鈕會切換為 **「儲存修改」**，可直接修改時間、訊息、目標與啟用狀態。

### 4. 測試與常駐
- 可選取排程並點擊 **「立即執行」** 進行手動測試（不會跳過或影響原定的下一次每日排程時間）。
- 主畫面「剩餘流量與重置倒數」會在開啟時讀取一次，也可按 **「重新讀取額度」** 手動更新；本地倒數不會重複呼叫模型。
- 點擊視窗右上角關閉按鈕，程式會自動最小化並縮至 Windows 右下角系統匣持續監控。
- 雙擊系統匣圖示或右鍵選擇「開啟主視窗」即可還原主畫面。

---

## 🛠️ 開發與建置

### 系統需求
- **作業系統**：Windows 10 / 11
- **開發工具**：Visual Studio 2022（需勾選「.NET 桌面開發」工作負載）或 [.NET 8.0 SDK](https://dotnet.microsoft.com/download/dotnet/8.0)
- **安裝包編譯工具（選用）**：[Inno Setup 6](https://jrsoftware.org/isinfo.php)（如需產出 Setup.exe）

### 專案結構
```
AI倒數喚醒/
├── AI倒數喚醒.sln                # Visual Studio 方案檔
├── Directory.Build.props         # 專案共用建置設定
├── build-installer.ps1           # 一鍵自動測試、發布與編譯安裝包腳本
├── assets/                       # 應用程式高解析度圖示 (app.ico)
├── installer/                    # Inno Setup 6 繁中安裝腳本與語系檔
├── src/
│   ├── AiWakeScheduler.Core/     # 核心邏輯庫（排程引擎、CLI 執行器、參數建構器、JSON 儲存）
│   └── AiWakeScheduler.WinForms/ # Windows Forms 繁體中文桌面使用者介面
└── tests/
    └── AiWakeScheduler.Tests/    # 原生輕量化單元與整合測試（0 NuGet 相依）
```

### 命令列建置與測試

**建置專案 (Release 模式)：**
```powershell
dotnet build '.\AI倒數喚醒.sln' --configuration Release
```

**執行測試套件：**
```powershell
dotnet run --project '.\tests\AiWakeScheduler.Tests\AiWakeScheduler.Tests.csproj' --configuration Release
```

預設測試完全不依賴本機 CLI 或登入狀態，可在 CI 與隔離環境重現。若要額外驗證本機已安裝的 AGY、Codex、Claude 以及 Codex 登入額度介面，請明確選用整合測試：

```powershell
dotnet run --project '.\tests\AiWakeScheduler.Tests\AiWakeScheduler.Tests.csproj' --configuration Release -- --integration
```

**一鍵打包 Windows 安裝版 (Setup.exe)：**
```powershell
powershell -ExecutionPolicy Bypass -File .\build-installer.ps1
```
產出之安裝程式將位於 `dist\AI倒數喚醒_Setup_v1.0.0_x64.exe`。

---

## 📁 資料與日誌路徑

本程式的所有使用者設定、排程資料與執行日誌均存放於本機使用者目錄：

- **根目錄**：`%LOCALAPPDATA%\AI倒數喚醒\`
- **排程設定**：`%LOCALAPPDATA%\AI倒數喚醒\schedules.json`
- **CLI 設定**：`%LOCALAPPDATA%\AI倒數喚醒\settings.json`
- **執行日誌**：`%LOCALAPPDATA%\AI倒數喚醒\logs\`（包含每次執行的標準輸出與錯誤訊息）
- **隔離工作區**：`%LOCALAPPDATA%\AI倒數喚醒\workspace\`（喚醒執行時的專用空白目錄）

---

## ❓ 常見問題 (FAQ)

<details>
<summary><b>Q1: 為什麼關閉視窗後，程式仍然在背景執行？</b></summary>

> 本程式為排程常駐工具，點擊右上角關閉視窗時會自動縮小至「Windows 系統匣」。若要完全關閉程式，請在系統匣圖示點擊滑鼠右鍵並選擇「結束」。
</details>

<details>
<summary><b>Q2: Codex CLI 檢查時出現「Access is denied」或無法執行？</b></summary>

> 在 Windows 上，透過 Microsoft Store 安裝的應用程式別名（位於 `WindowsApps`）有時會因權限限制導致外部程式無法直接調用。請前往「CLI 設定」，點擊「瀏覽」並直接指定實際的 `codex.exe` 實體執行路徑即可解決。
</details>

<details>
<summary><b>Q3: 如果電腦在排程時間處於睡眠或關機狀態，開機後會發生什麼事？</b></summary>

> 當程式啟動或重新登入時，排程引擎會自動偵測已到期但尚未執行的排程，並在背景補行處理，隨後自動計算並推進至下一個未來的每日觸發時間。
</details>

<details>
<summary><b>Q4: 每天自動喚醒會不會消耗很多 Token 或 API 費用？</b></summary>

> 不會。本工具預設開啟「節省 Token 模式」，指令均已強制限制為最低推理級別（Low Effort）、關閉 Tool 調用、使用唯讀沙箱並限制最大 50 字元短語，單次喚醒消耗的 Token 量極少。
</details>

---

## 📄 授權條款

本專案採用 [MIT License](LICENSE) 授權。
