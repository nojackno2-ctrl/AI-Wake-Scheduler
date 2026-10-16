# AI 倒數喚醒 (AI Wake Scheduler)

[![Rust](https://img.shields.io/badge/Rust-2021-DEA584?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/Platform-Windows%2010%20%2F%2011-0078D6?logo=windows&logoColor=white)](https://www.microsoft.com/windows)
[![UI](https://img.shields.io/badge/UI-Win32%20(Native)-blue)](#)
[![Dependencies](https://img.shields.io/badge/Dependencies-Zero%20Runtime-success)](#)
[![License](https://img.shields.io/badge/License-MIT-green.svg)](#)

一個專為 AI 輔助開發者設計的 Windows 輕量排程工具，以純 Rust 語言與原生 Win32 API 打造。

許多 AI 開發工具（如 Google Antigravity、Anthropic Claude Code、OpenAI Codex）採用**滾動時間窗口額度機制**（例如「5 小時重設窗口」，自發送第一次請求起算）。本工具可在每天指定的固定時間，自動向各大 AI CLI 發送極簡短語（預設為「早安」），自動觸發並啟動使用額度倒數，確保你在開始工作時，AI 的使用額度已經準備就緒。

---

## ✨ 核心特色

- ⏰ **極簡每日排程**：直接設定每天固定觸發時分（`HH:mm`），支援多組排程管理，主畫面即時顯示距離下一次執行的倒數計時。
- 🚀 **四大多模型目標平行喚醒**：同時支援 **Google Antigravity (`agy`) Gemini 額度池**、**Antigravity Claude / GPT 額度池**、**Anthropic Claude (`claude`)** 與 **OpenAI Codex (`codex`)**。多個 CLI 到點時平行獨立啟動，互不等待與阻塞。
- 📊 **真實額度與重置倒數**：透過 Codex 官方 app-server、Claude Code OAuth usage 端點與 Antigravity CLI 內建 `/usage`，唯讀取得剩餘百分比及重置時間；關閉背景查詢時可讀取已開啟 IDE 的 Language Server。
- 💡 **極致 Token 節省模式 (Token Saver)**：
  - 自動套用最低推理與簡短模式（`--effort low`、`model_reasoning_effort=low`）。
  - 禁用 Claude 所有 Tool 呼叫（`--tools ""`）。
  - 啟用 Codex 唯讀沙箱（`--sandbox read-only`）。
  - 隔離於空白專用工作區（`%LOCALAPPDATA%\AI倒數喚醒\workspace`），避免掃描本機龐大原始碼目錄。
  - 自動附加限制指令（「只回覆上面這句，不要使用工具」），將 Token 消耗降至極限。
- 🛡️ **安全無注入設計**：透過安全結構化傳遞參數，杜絕 Shell 注入與跳脫字元問題。
- 🔍 **無損安全檢查**：提供「檢查全部 CLI」功能，僅執行 `--version` 探針檢查，絕不送出 AI 訊息或消耗對話額度。
- 🪟 **輕量背景常駐與開機自啟**：純 Rust 與原生 Win32 控制項打造，關閉主視窗時自動縮至系統匣（System Tray）持續倒數；支援登入 Windows 自動啟動。
- 📦 **零外部執行階段相依**：100% 純 Rust 編譯為原生單一執行檔（約 2 MB），不需要安裝 .NET Runtime 或其他依賴，啟動速度極快且記憶體佔用極低。
- 📜 **完整日誌追蹤**：完整留存每次執行的 stdout、stderr、結束代碼與錯誤記錄，排查問題一目了然。

---

## 🖥️ 支援的 AI CLI 工具與額度池

| AI 工具 / 額度池 | 預設執行檔命令 | 喚醒呼叫範例（節省 Token 模式） | 預設節省機制說明 |
| :--- | :--- | :--- | :--- |
| **Antigravity (Gemini)** | `agy` | `agy --model gemini-3.8-flash --effort low --disable-slash-commands --print <提示>` | 所有旗標置於 `--print` 前、低思考 effort、停用技能展開、禁止修改工作區 |
| **Antigravity (Claude / GPT)** | `agy` | `agy --model claude-sonnet-5-5-low --disable-slash-commands --print <提示>` | 明確選擇 Claude 額度池；完整模型 ID 已包含 Low 思考程度，因此不另傳 `--effort` |
| **OpenAI Codex** | `codex` | `codex exec --model gpt-6-luna --ephemeral --sandbox read-only --ignore-user-config --ignore-rules … <提示>` | 一次性非互動 exec、唯讀沙箱、不載入 MCP／使用者規則、低推理與低詳細度 |
| **Anthropic Claude** | `claude` | `claude --print --model claude-haiku-4-5-20251001 --safe-mode --tools "" --no-session-persistence --prompt-suggestions false <提示>` | 停用自訂內容與工具、不持久化 Session、不額外生成提示建議 |

預設採用低價模型：Gemini 3.8 Flash Low、Codex GPT-6 Luna、Claude Haiku 4.5；AGY Claude 池使用 Sonnet 5.5 Low。官方價格、AGY 扣額限制與選擇依據見 [模型計價紀錄](docs/MODEL_PRICING.md)。空白模型會使用本工具預設，可自行改選其他模型。

> [!TIP]
> Google Antigravity 內部將額度切分為 **Gemini Models**、**Claude and GPT models** 與 **Gemini 4 Argon** 等獨立計數器。勾選本工具中的這兩項目標，可同時喚醒兩邊的 5 小時滾動重設窗口！
> 實際執行時，使用者訊息會以安全參數傳入，每個排程與每個 CLI 僅呼叫一次且失敗不自動重試，最大字數限制為 50 字元。

---

## 🚀 快速上手

### 1. 下載與安裝

#### 方法 A：使用 Windows 安裝版（推薦）
1. 前往 [Releases](https://github.com/nojackno2-ctrl/AI-Wake-Scheduler/releases/latest) 下載 **`AI倒數喚醒_Setup_v1.10.0_x64.exe`**；`SHA256SUMS.txt` 提供校驗碼。
2. 執行安裝程式，依照精靈指示選擇安裝位置；桌面捷徑與登入 Windows 後自動啟動皆為可選項，預設不勾選。
3. 安裝程式會固定在開始功能表建立啟動與解除安裝捷徑；可在啟動捷徑上按右鍵，自行釘選到「開始」或工作列。
4. 安裝版體積僅約 2.3 MB，原生獨立執行，電腦**無須預先安裝任何執行環境（如 .NET Runtime）**即可直接運行。

#### 方法 B：綠色免安裝版 / 自行編譯
- 下載或自行建置 `AI倒數喚醒.exe`，直接執行即可；為單一原生 Windows 執行檔，免安裝任何額外套件。

### 2. 初次設定與 CLI 檢查
1. 點擊主畫面右下角的 **「CLI 設定…」** 按鈕。
2. 點擊 **「檢查全部 CLI」** 按鈕，程式會自動執行 `--version` 檢查本機 CLI 是否已就緒。
3. 若你的 CLI 安裝於特殊路徑（例如透過 npm、專屬安裝目錄等），可點擊 **「瀏覽…」** 自訂 `.exe` 完整路徑。
4. （可選）勾選 **「登入 Windows 後自動啟動」**，確保開機或重啟後仍能在背景常駐。
5. （可選）**「允許在背景無視窗查詢 Antigravity 額度（不需開啟 IDE）」**（預設開啟）：
   透過新版 `agy --output-format json --print /usage` 內建指令讀取額度池的五小時與每週倒數。
   已於 agy 驗證為 0 模型回合、0 Token；讀完程序立即結束，沒有視窗。
   關閉此選項時只查詢已啟動的 Antigravity IDE。
6. 點擊 **「儲存」**。

### 3. 建立每日排程或自動模式
1. 在右側「排程內容」區：
   - **名稱**：輸入易辨識的名稱（例如「早晨 AI 喚醒」）。
   - **排程模式**：選擇「每天固定時間」、「自動模式（每 5 小時 1 分鐘）」或「自動倒數模式（每日偵測至 24:00）」。
   - **時間 / 首次喚醒時間**：設定觸發的時與分（例如 `05:30`）。在自動模式下，日間每 5h1m 推進，若跨越午夜 24:00 則自動於隔天 05:30 重新啟動。
   - **自動倒數模式**：設定「開始偵測時間」（例如 `08:00`），每天從該時間偵測至 24:00。各 CLI 沒有倒數就立即呼叫，已有倒數則等結束後再呼叫，持續循環；午夜停止自動呼叫，翌日指定時間恢復。額度讀取失敗會重試，不當成未倒數。偵測間隔最長 30 秒，已知倒數結束會提前喚醒；呼叫後保留 2 分鐘冷卻避免額度資料延遲造成連發。此模式不受額度看板自動更新開關影響，程式須保持執行（可縮至系統匣）。午夜前已開始的呼叫會正常完成；「立即執行」仍是手動呼叫。
   - **工作目錄**：選擇或保持預設隔離目錄。
   - **傳送至**：勾選欲啟動的 CLI 目標（Antigravity (Gemini) / Antigravity (Claude / GPT) / Codex CLI / Claude CLI）。
   - **訊息**：輸入觸發短語（預設為「早安」）。
2. 點擊 **「建立排程」**。選取既有排程後按鈕會切換為 **「儲存修改」**，可直接修改時間、訊息、目標與啟用狀態。

### 4. 測試與常駐
- 可選取排程並點擊 **「立即執行」** 進行手動測試（不會跳過或影響原定的下一次每日排程時間）。
- 主畫面「剩餘流量與重置倒數」支援 Codex、Claude CLI、Antigravity (Gemini) 與 Antigravity (Claude / GPT) 即時額度查詢，在開啟時自動讀取，也可按 **「重新讀取倒數」** 手動更新；本地每秒精準倒數。Claude CLI 必須先以 `claude auth login` 登入 Claude 訂閱帳號，程式只讀取本機短效 access token，不會輸出或記錄權杖，也不會為了查額度建立模型回合。
- 點擊視窗右上角關閉按鈕，程式會自動最小化並縮至 Windows 右下角系統匣持續監控。
- 雙擊系統匣圖示或右鍵選擇「開啟主視窗」即可還原主畫面。

---

## 🛠️ 開發與建置

### 系統需求
- **作業系統**：Windows 10 / 11
- **開發工具**：[Rust](https://rustup.rs/) stable（CI 固定使用 1.99.0）與 Visual Studio 的「使用 C++ 的桌面開發」工作負載（提供 MSVC 連結器與 Windows SDK 的資源編譯器）。
- **安裝包編譯工具（選用）**：[Inno Setup 6](https://jrsoftware.org/isinfo.php)（如需編譯 Setup.exe 安裝程式）。

### 專案結構
```
AI倒數喚醒/
├── Cargo.toml                # Cargo Workspace 設定
├── Cargo.lock
├── rustfmt.toml              # 程式碼格式化規則
├── build-release.ps1         # 測試並建置 Release 執行檔（dist\AI倒數喚醒.exe）
├── build-installer.ps1       # 一鍵自動測試、建置與編譯 Inno Setup 安裝包腳本
├── assets/                   # 應用程式高解析度圖示 (app.ico)
├── installer/                # Inno Setup 6 繁中安裝腳本與語系檔
├── core/                     # crate aiwake-core：排程引擎、CLI 執行、TokenSaver 參數、WinHTTP 額度讀取
├── app/                      # crate aiwake-app：原生 Win32 控制項介面、RichEdit 終端機、系統匣、Run key 啟動
└── tests/
    └── Verify-Installer.ps1  # 安裝程式完整生命週期驗證腳本
```

### 命令列建置與測試

**執行測試套件：**
```powershell
cargo test --workspace --locked
```
預設測試包含：
- 核心單元測試（排程計算、Token 節省參數建構、JSON 持久化相容、適應性倒數）。
- App 畫面元件文字與格式測試。
- 啟動真正編譯產物並透過 Win32 視窗訊息驗證的**端對端 UI 測試**（使用隔離暫存資料與假 CLI，不影響本機設定與 Run key）。

**本機真實 CLI 整合測試（選用）：**
```powershell
cargo test -p aiwake-core --locked -- --ignored executable_locator_resolution   # 唯讀：--version 與額度查詢，不消耗 Token
cargo test -p aiwake-core --locked -- --ignored real_cli_wake_smoke             # 會各送出一次最小提示詞，消耗少量 Token
```

**建置發行版執行檔：**
```powershell
powershell -ExecutionPolicy Bypass -File .\build-release.ps1
```
產出單一原生執行檔於 `dist\AI倒數喚醒.exe`（附 SHA256 校驗碼）。

**一鍵打包 Windows 安裝版 (Setup.exe)：**
```powershell
powershell -ExecutionPolicy Bypass -File .\build-installer.ps1
```
產出之安裝程式將位於 `dist\AI倒數喚醒_Setup_v1.10.0_x64_<時間戳>.exe`，並附獨立 `.sha256`。

**安裝生命週期驗證（可選）：**
```powershell
# 1. 建置 QA 專用安裝包（使用獨立 AppId、捷徑與 Run key）
powershell -ExecutionPolicy Bypass -File .\build-installer.ps1 -VerificationBuild -OutputDirectory bin\verification-installer

# 2. 執行五階段驗證（Install -> UpgradeKeepStartup -> UpgradeDisableStartup -> UpgradeEnableStartup -> Uninstall）
$qa = (Get-ChildItem -Path bin\verification-installer\*QA*.exe | Sort-Object LastWriteTime | Select-Object -Last 1).FullName
powershell -ExecutionPolicy Bypass -File .\tests\Verify-Installer.ps1 -SetupPath $qa -Phase Install
powershell -ExecutionPolicy Bypass -File .\tests\Verify-Installer.ps1 -SetupPath $qa -Phase UpgradeKeepStartup
powershell -ExecutionPolicy Bypass -File .\tests\Verify-Installer.ps1 -SetupPath $qa -Phase UpgradeDisableStartup
powershell -ExecutionPolicy Bypass -File .\tests\Verify-Installer.ps1 -SetupPath $qa -Phase UpgradeEnableStartup
powershell -ExecutionPolicy Bypass -File .\tests\Verify-Installer.ps1 -SetupPath $qa -Phase Uninstall
```
驗證腳本嚴格限制安裝路徑於 `bin\verification-installed`，每階段自動比對本機正式設定檔、排程檔與 Run key baseline，確保完全不變動正式環境資料。

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
