# AI 倒數喚醒 (AI Wake Scheduler)

[![Rust](https://img.shields.io/badge/Rust-2021-DEA584?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/Platform-Windows%2010%20%2F%2011-0078D6?logo=windows&logoColor=white)](https://www.microsoft.com/windows)
[![UI](https://img.shields.io/badge/UI-egui%20%2F%20wgpu-22D3EE)](#)
[![Dependencies](https://img.shields.io/badge/Dependencies-Zero%20Runtime-success)](#)
[![License](https://img.shields.io/badge/License-MIT-green.svg)](#)

一個專為 AI 輔助開發者設計的 Windows 輕量排程工具，以純 Rust、`eframe` / `egui` 與 `wgpu` GPU 繪圖打造，原生 Win32 API 負責系統匣與檔案對話框。

許多 AI 開發工具（如 Google Antigravity、Anthropic Claude Code、OpenAI Codex）採用**滾動時間窗口額度機制**（例如「5 小時重設窗口」，自發送第一次請求起算）。本工具可在每天指定的固定時間，自動向各大 AI CLI 發送極簡短語（預設為「早安」），自動觸發並啟動使用額度倒數，確保你在開始工作時，AI 的使用額度已經準備就緒。

---

## ✨ 核心特色

- ⏰ **每日偵測時段**：設定開始與結束時間（`HH:mm`），時段內僅在沒有讀到倒數時依設定的分鐘間隔讀取；一旦讀到倒數即記住下次倒數結束時間，倒數期間不再重複查詢，並於倒數結束時立即重讀與喚醒；主畫面即時顯示各 CLI 的倒數。
- 🚀 **五個額度池目標平行喚醒**：同時支援 **Google Antigravity (`agy`) Gemini 額度池**、**Antigravity Gemini 4 Argon 額度池**、**Antigravity Claude / GPT 額度池**、**Anthropic Claude (`claude`)** 與 **OpenAI Codex (`codex`)**。多個 CLI 到點時平行獨立啟動，互不等待與阻塞。
- 📊 **真實額度與重置倒數**：透過 Codex 官方 app-server、Claude Code OAuth usage 端點與 Antigravity CLI 內建 `/usage`，唯讀取得剩餘百分比及重置時間；關閉背景查詢時可讀取已開啟 IDE 的 Language Server。
- 💡 **極致 Token 節省模式 (Token Saver)**：
  - 自動套用最低推理與簡短模式（`--effort low`、`model_reasoning_effort=low`）。
  - 禁用 Claude 所有 Tool 呼叫（`--tools ""`）。
  - 啟用 Codex 唯讀沙箱（`--sandbox read-only`）。
  - 隔離於空白專用工作區（`%LOCALAPPDATA%\AI倒數喚醒\workspace`），避免掃描本機龐大原始碼目錄。
  - 自動附加限制指令（「只回 OK，不要使用工具或說明」），減少輸出 Token。
- 🛡️ **結構化參數傳遞**：原生執行檔與批次檔的參數交由 Rust 標準函式庫處理，並測試特殊字元往返與命令注入防護。`.cmd`／`.bat` 僅接受單行參數；原生 `.exe` 支援多行訊息。
- 🔍 **無損安全檢查**：提供「檢查全部 CLI」功能，僅執行 `--version` 探針檢查，絕不送出 AI 訊息或消耗對話額度。
- 🪟 **輕量背景常駐與開機自啟**：純 Rust 與 egui GPU 儀表板，關閉主視窗時可自動縮至系統匣（System Tray）持續倒數；支援登入 Windows 自動啟動。
- 📦 **零外部執行階段相依**：編譯為原生單一執行檔，不需要安裝 .NET Runtime、WebView 或額外 UI 執行階段；使用系統顯示驅動程式提供的 GPU 後端。GPU 版本體積與記憶體需求較舊 Win32 介面增加。
- 📜 **執行日誌追蹤**：記錄 stdout、stderr、結束代碼與錯誤；每個輸出串流最多保留 32,768 字元，過長會標記截斷。即時畫面另限制輸出佇列，避免大量輸出拖慢介面。

---

## 🖥️ 支援的 AI CLI 工具與額度池

| AI 工具 / 額度池 | 預設執行檔命令 | 喚醒呼叫範例（節省 Token 模式） | 預設節省機制說明 |
| :--- | :--- | :--- | :--- |
| **Antigravity (Gemini)** | `agy` | `agy --model gemini-3.8-flash --effort low --disable-slash-commands --print <提示>` | 所有旗標置於 `--print` 前、低思考 effort、停用技能展開、禁止修改工作區 |
| **Antigravity (Argon)** | `agy` | `agy --model gemini-4-argon --effort low --disable-slash-commands --print <提示>` | 獨立的 Gemini 4 Argon 每週額度池，單獨勾選、單獨倒數；旗標同樣置於 `--print` 前 |
| **Antigravity (Claude / GPT)** | `agy` | `agy --model claude-sonnet-5-5-low --disable-slash-commands --print <提示>` | 明確選擇 Claude 額度池；完整模型 ID 已包含 Low 思考程度，因此不另傳 `--effort` |
| **OpenAI Codex** | `codex` | `codex exec --model gpt-6-luna --ephemeral --sandbox read-only --ignore-user-config --ignore-rules … <提示>` | 一次性非互動 exec、唯讀沙箱、不載入 MCP／使用者規則、低推理與低詳細度 |
| **Anthropic Claude** | `claude` | `claude --print --model claude-haiku-5-5 --safe-mode --tools "" --no-session-persistence --prompt-suggestions false <提示>` | 停用自訂內容與工具、不持久化 Session、不額外生成提示建議 |

預設採用低價模型：Gemini 3.8 Flash Low、Codex GPT-6 Luna、Claude Haiku 5.5；AGY Claude 池使用 Sonnet 5.5 Low；AGY Argon 池使用 Gemini 4 Argon Low。官方價格、AGY 扣額限制與選擇依據見 [模型計價紀錄](docs/MODEL_PRICING.md)。空白模型會使用本工具預設，可自行改選其他模型。

> [!TIP]
> Google Antigravity 內部將額度切分為 **Gemini Models**、**Claude and GPT models** 與 **Gemini 4 Argon** 三個獨立計數器，本工具各以一個目標對應：Gemini 與 Claude / GPT 各有 5 小時與每週視窗，Argon 只有每週視窗。三個目標可分別勾選、分別倒數；Argon 喚醒會消耗它自己的每週額度。
> 實際執行時，使用者訊息會以安全參數傳入，每個排程與每個 CLI 僅呼叫一次且失敗不自動重試，最大字數限制為 50 字元。

---

## 🚀 快速上手

### 1. 下載與安裝

#### 方法 A：使用 Windows 安裝版（推薦）
1. 前往 [Releases](https://github.com/nojackno2-ctrl/AI-Wake-Scheduler/releases/latest) 下載 **`AI-Wake-Scheduler_Setup_v2.2.0_x64.exe`**（免安裝版為 `AI-Wake-Scheduler_Portable_v2.2.0_win-x64.zip`）；`SHA256SUMS.txt` 提供校驗碼。
2. 執行安裝程式，依照精靈指示選擇安裝位置；桌面捷徑與登入 Windows 後自動啟動皆為可選項，預設不勾選。
3. 安裝程式會固定在開始功能表建立啟動與解除安裝捷徑；可在啟動捷徑上按右鍵，自行釘選到「開始」或工作列。
4. 原生獨立執行，電腦**無須預先安裝任何執行環境（如 .NET Runtime）**即可直接運行。

#### 方法 B：綠色免安裝版 / 自行編譯
- 下載或自行建置 `AI倒數喚醒.exe`，直接執行即可；為單一原生 Windows 執行檔，免安裝任何額外套件。

### 2. 初次設定與 CLI 檢查
1. 點擊頂部 **「⚙ 設定」**，在同一視窗右側開啟設定抽屜。
2. 點擊 **「檢查 CLI 連線」** 按鈕，程式會自動執行 `--version` 檢查本機 CLI 是否已就緒。
3. 若你的 CLI 安裝於特殊路徑（例如透過 npm、專屬安裝目錄等），可點擊 **「瀏覽…」** 自訂 `.exe` 完整路徑。
4. （可選）勾選 **「登入 Windows 後自動啟動」**，確保開機或重啟後仍能在背景常駐。
5. （可選）**「允許在背景無視窗查詢 Antigravity 額度（不需開啟 IDE）」**（預設開啟）：
   透過新版 `agy --output-format json --print /usage` 內建指令讀取額度池的五小時與每週倒數。
   已於 agy 驗證為 0 模型回合、0 Token；讀完程序立即結束，沒有視窗。
   關閉此選項時只查詢已啟動的 Antigravity IDE。
6. 點擊 **「儲存設定」**；「取消」會丟棄表單副本，不寫入設定檔。

### 3. 設定每日偵測時段
1. 在中欄「建立每日喚醒／編輯排程」卡片（上方為雙環額度儀表、右側為即時終端輸出）：
   - **排程名稱**：輸入易辨識的名稱（例如「早晨 AI 喚醒」）。
   - **開始時間**：設定每天開始偵測的時與分（例如 `05:30`），開始時間包含在時段內。
   - **結束時間**：設定停止偵測的時與分，預設 `24:00`；到結束時間後不再自動呼叫，隔天開始時間重新偵測。結束早於開始代表跨午夜（例如 `22:00` 至 `06:00`）；開始與結束不可相同，全天請設 `00:00` 至 `24:00`。
   - **倒數偵測**：每筆排程設定 **間隔（分）**（1–60，預設 10），在尚未讀到倒數時依每小時整點起算刻度（例如 5 分鐘為 `00、05、10、15…`，15 分鐘為 `00、15、30、45`）讀取並確認沒有倒數後呼叫；一旦讀到倒數即記住下次倒數結束時間，倒數期間不再依間隔重複讀取，並於倒數結束（且在偵測時段內）立即重讀與呼叫。舊排程的間隔預設 10 分鐘，保留原開始／結束時間。
   - **額度用盡**：任一額度視窗（例如 Antigravity 每週額度）已用 100% 且尚未重置時不會呼叫該 CLI（呼叫只會得到 HTTP 429），清單顯示「額度用盡」與剩餘時間。額度讀取失敗不當成未倒數；呼叫後保留 2 分鐘冷卻。程式須保持執行（可縮至系統匣）。
   - **工作目錄**：選擇或保持預設隔離目錄。
   - **傳送至**：點選帶品牌色的 CLI 膠囊標籤，包含 Ge、Ar、AC、Cx、CC 五個獨立目標；可一鍵全選或清除。
   - **訊息**：輸入觸發短語（預設為「早安」）。
2. 點擊 **「建立排程」**。點選左欄既有排程卡片後按鈕會切換為 **「儲存修改」**，可直接修改時間、訊息、目標與啟用狀態。

### GPU 儀表板操作

- 預設深色科技主題，頂部「☀ 淺色／☾ 深色」可即時切換；終端面板始終維持深色。主題暫存於執行期，不改寫舊版設定格式。
- 左欄排程卡片顯示每日偵測時段、24 小時時間軸、目前時間與 CLI 重置菱形標記。卡片右上角開關會立即寫入排程；點選卡片載入編輯器。
- 中欄五大 CLI 使用品牌色徽章與雙環：外環代表主要額度剩餘百分比，內環代表重置期間已過比例，下方細條顯示次要視窗。無額度資料時顯示「尚未讀取」，登入失效時可直接重新登入。
- 編輯器可拖曳時間軸的開始／結束把手，以 5 分鐘吸附並同步時間文字；支援跨午夜、`24:00` 結束、間隔膠囊與自訂 1–60 分鐘。
- 右側終端機寬度可拖曳，亦可由「更多 ⋯」收合。窄視窗會將排程與終端面板上下排列，儀表自動換列，內容可捲動。
- 設定抽屜保留自訂模型、推薦模型、思考程度連動、執行檔瀏覽、額外參數、唯讀連線探針與一般設定；取消不保留未儲存變更。
- 字型優先使用 Windows 正黑體與 Consolas；讀取失敗時回退到內建／系統字型。字級維持固定邏輯點數，只跟隨螢幕 DPI，不隨視窗大小縮放。

設定 `AIWAKE_UI_SHOTS` 可在 UI 測試輸出 wgpu PNG 截圖：

```powershell
$env:AIWAKE_UI_SHOTS = "$PWD\target\ui-shots"
cargo test -p aiwake-app --test ui_smoke --locked -- --test-threads=1
```

`--verify-ui` 會使用 `AiWakeScheduler-QA-*` 暫存資料夾並停用主介面的真實帳號額度查詢。測試專用 `AIWAKE_VERIFY_ROOT` 僅接受系統暫存目錄直屬、已存在且名稱以 `AiWakeScheduler-QA-` 開頭的資料夾；正常啟動忽略此變數。

### 4. 測試與常駐
- 可選取排程並點擊 **「立即執行」** 進行手動測試，手動呼叫不受偵測時段限制；完成後恢復時段內倒數偵測。
- 主畫面「額度觀測站」的雙環卡片支援 Codex、Claude CLI、Antigravity (Gemini) 與 Antigravity (Claude / GPT) 即時額度查詢，在開啟時自動讀取，也可按 **「重新讀取倒數」** 手動更新；本地每秒精準倒數。Claude CLI 必須先以 `claude auth login` 登入 Claude 訂閱帳號，程式只讀取本機短效 access token，不會輸出或記錄權杖，也不會為了查額度建立模型回合。
- 「關閉視窗時縮到系統匣」開啟時，關閉視窗會隱藏並持續監控；關閉該選項則正常退出。`--minimized` 會在啟動後隱藏主視窗。
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
├── AI倒數喚醒.sln            # Visual Studio 方案（NMake 專案，實際建置交給 cargo）
├── AI倒數喚醒.vcxproj
├── tools/vs-cargo.ps1        # VS 用 cargo 包裝腳本，把診斷轉成「錯誤清單」格式
├── assets/                   # 應用程式高解析度圖示 (app.ico)
├── installer/                # Inno Setup 6 繁中安裝腳本與語系檔
├── core/                     # crate aiwake-core：排程引擎、CLI 執行、TokenSaver 參數、WinHTTP 額度讀取
├── app/                      # crate aiwake-app：egui GPU 儀表板、設定抽屜、即時終端機、原生系統匣與 Run key 啟動
└── tests/
    └── Verify-Installer.ps1  # 安裝程式完整生命週期驗證腳本
```

### 在 Visual Studio 中建置與測試

以 Visual Studio 2026（或 2022）開啟 `AI倒數喚醒.sln`，需安裝「使用 C++ 的桌面開發」工作負載。方案組態下拉選單有三種設定：

| 組態 | 建置（Ctrl+Shift+B）時執行 | F5 啟動 |
| --- | --- | --- |
| `Debug` | `cargo build` | `target\debug\ai-wake-scheduler.exe`（含 PDB，可下中斷點） |
| `Release` | `cargo build --release` | `target\release\ai-wake-scheduler.exe` |
| `Test` | `cargo fmt --check` → `cargo clippy -D warnings` → `cargo test` | `target\debug\ai-wake-scheduler.exe` |

編譯錯誤、clippy 警告、格式差異與失敗測試的 panic 位置會出現在「錯誤清單」，點兩下即可跳到原始碼；完整輸出在「輸出」視窗。測試不會出現在「測試總管」。同樣的建置也可在命令列以 MSBuild 執行：
```powershell
msbuild AI倒數喚醒.sln /p:Configuration=Test /p:Platform=x64
```

### 命令列建置與測試

**執行測試套件：**
```powershell
cargo test --workspace --locked
```
預設測試包含：
- 核心單元測試（排程計算、Token 節省參數建構、JSON 持久化相容、適應性倒數）。
- App 畫面元件文字與格式測試。
- 透過 `egui_kittest` 操作正式介面的排程卡片、啟停開關、設定抽屜與假 CLI；另啟動真正的 `--verify-ui` GPU 視窗，驗證關窗隱藏、還原、退出及 `--minimized`（隔離暫存資料，不變動正式設定與 Run key）。

**本機真實 CLI 整合測試（選用）：**
```powershell
cargo test -p aiwake-core --locked -- --ignored executable_locator_resolution   # 唯讀：--version 與額度查詢，不消耗 Token
cargo test -p aiwake-core --locked -- --ignored real_cli_wake_smoke             # 會各送出一次最小提示詞，消耗少量 Token
cargo test -p aiwake-core --locked -- --ignored --exact real_argon_wake_smoke   # 只喚醒 Argon 並確認每週倒數開始
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
產出之安裝程式將位於 `dist\AI倒數喚醒_Setup_v<版本>_x64_<時間戳>.exe`，並附獨立 `.sha256`。

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
- **除錯日誌**：`%LOCALAPPDATA%\AI倒數喚醒\logs\debug-YYYYMMDD.log`（排程判斷、每次額度讀取結果、CLI 呼叫與錯誤，保留 14 天）
- **匯出除錯日誌**：主視窗或系統匣選單的「匯出除錯日誌…」會把目前額度、設定、排程與所有日誌合併成一個 `.txt`，回報問題時附上即可。檔案包含 CLI 輸出與工作目錄路徑，分享前請自行檢查。
- **CLI 更新提示**：啟動 1 分鐘後與之後每 12 小時，比對 agy、Codex、Claude Code 的本機版本（`--version`）與最新版本（agy 官方自動更新服務、npm registry）。有新版本時跳一次系統匣通知（同一版本只提示一次）並在狀態列顯示；按「檢查 CLI 更新…」（主視窗或系統匣選單）可隨時檢查，確認後會開主控台執行各 CLI 自己的 `update` 指令。
- **隔離工作區**：`%LOCALAPPDATA%\AI倒數喚醒\workspace\`（喚醒執行時的專用空白目錄）

新版啟動時會將舊排程轉為每日偵測時段，缺少結束時間時補上 `24:00`，並在每筆資料寫入 `ScheduleRevision` 遷移版本。沿用原開始時間、目標、訊息、工作目錄與執行紀錄；停用或已完成／失敗的排程不會因此重新啟用，未來的執行時間也不會提前。最新資料無異動時不重寫。舊版字串與整數列舉、屬性大小寫及 UTF-8 BOM 仍可讀取。

若資料損壞、缺少識別碼或執行時間、版本超出支援範圍，或包含無法保留的未知欄位，會停止載入並保留原檔，不以空排程覆寫；解析錯誤會嘗試另存 `.corrupted.*` 備份。資料遷移不會更新程式執行檔或安裝版本。

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

> 尚未讀到倒數時，依時段內設定的整點刻度偵測；一旦讀到倒數，就記住重置時間，在倒數結束且位於偵測時段內時立即重讀與喚醒。看板的手動／自動重新整理只更新顯示。
</details>

<details>
<summary><b>Q4: 每天自動喚醒會不會消耗很多 Token 或 API 費用？</b></summary>

> 不會。本工具預設開啟「節省 Token 模式」，指令均已強制限制為最低推理級別（Low Effort）、關閉 Tool 調用、使用唯讀沙箱並限制最大 50 字元短語，單次喚醒消耗的 Token 量極少。
</details>

---

## 📄 授權條款

本專案採用 [MIT License](LICENSE) 授權。
