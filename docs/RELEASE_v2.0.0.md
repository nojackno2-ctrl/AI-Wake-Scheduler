# v2.0.0：純 Rust 原生版、Argon 獨立目標與自動倒數模式

整個程式改用 Rust 與原生 Win32 控制項重寫，不再需要 .NET；安裝版由約 48 MB 縮小到約 2.4 MB，免安裝版只有一個約 1.8 MB 的執行檔。設定檔與排程檔格式相容，升級後原有設定與排程照常沿用。

- **純 Rust 原生程式**：單一執行檔、啟動快、記憶體佔用低；介面、系統匣、終端機輸出與開機啟動行為與 v1.10.0 相同。
- **Antigravity Argon 獨立目標**：新增「Antigravity (Argon)」，預設 `gemini-4-argon`，可單獨勾選、喚醒與倒數。Gemini 額度池不再混入 Argon 的每週額度。新排程預設勾選全部目標；既有排程的目標不變，需要時請自行勾選 Argon。
- **自動倒數模式**：每天從設定的時間開始偵測各 CLI 額度，只喚醒尚未倒數的 CLI，到 24:00 停止、隔天恢復。
- **AGY 呼叫參數**：配合 agy CLI 1.4，節省 Token 模式不再傳入已失效的 `--mode plan`；AGY 模型清單移除已淘汰的 Gemini 3.7／3.6 Flash 與 3.1 Pro。
- **升級清理**：從 v1.x 升級時，安裝程式會移除舊版留在安裝目錄的 .NET 執行階段檔案（DLL、`zh-Hant` 資源資料夾等）；使用者設定與日誌位於 `%LOCALAPPDATA%`，不受影響。

## Windows x64 下載

- `AI-Wake-Scheduler_Setup_v2.0.0_x64.exe`：安裝版（可直接覆蓋升級 v1.x）。
- `AI-Wake-Scheduler_Portable_v2.0.0_win-x64.zip`：免安裝版；解壓縮後執行 `AI倒數喚醒.exe`。
- `SHA256SUMS.txt`：以上兩個檔案的 SHA256 校驗碼。

不需安裝任何執行環境。使用前需安裝並登入所需的 CLI（agy、codex、claude）。

## 驗證與限制

2026-10-17，於 Windows 11、Rust 1.99.0：

- `cargo fmt --check`、`cargo clippy -D warnings` 通過；`cargo test --workspace --locked` 28 通過、3 項需真實 CLI 的 opt-in 測試依設計略過。CI 在五個時區矩陣通過。
- 真實唯讀額度讀取：Gemini、Argon、Claude / GPT、Codex、Claude 五個目標皆可讀取。真實 Argon 喚醒回覆 OK，重讀額度後每週倒數開始。
- 隔離 QA 安裝包五階段（安裝、保留／停用／啟用開機啟動的升級、解除安裝）全部通過，正式設定、排程與開機啟動鍵不變。升級時預先放入的 v1 殘留檔（DLL、PDB、deps/runtimeconfig、createdump、`zh-Hant\`）全部被移除。
- 執行檔與安裝程式版本資訊皆為 2.0.0；免安裝 ZIP 內的執行檔與建置產物 SHA256 相同。

未實測：以真正的 v1.10.0 安裝版直接升級（清理規則以模擬殘留檔驗證）、Windows 重開機後自動啟動、真實系統匣通知外觀、Argon 以外各 CLI 在 v2.0.0 的真實喚醒。

已發布：[v2.0.0](https://github.com/nojackno2-ctrl/AI-Wake-Scheduler/releases/tag/v2.0.0)（2026-10-17，Latest）。標籤與執行檔來源：85e9e79feae831240c71941ee8c077a46b9915f2，該提交 CI 五時區通過。GitHub 三項資產的官方 SHA256 digest 與本機一致；重新下載後 `sha256sum -c SHA256SUMS.txt` 兩項 OK。
