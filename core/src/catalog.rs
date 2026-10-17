//! 所有支援 CLI 的描述目錄。
//!
//! 這是「某個 CLI 長什麼樣」的唯一知識來源。參數建構、路徑搜尋與 UI 顯示
//! 都只依賴這份目錄，新增一個 CLI 只需新增一筆描述。

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use crate::models::{CliKind, ThinkingEffort};
use ThinkingEffort as E;

/// 常用安裝根目錄的快取，所有 CLI 搜尋共用同一份結果。
pub struct ExecutableSearchPaths {
    pub local_app_data: String,
    pub app_data: String,
    pub user_profile: String,
    pub program_files: String,
    pub program_files_x86: String,
}

impl ExecutableSearchPaths {
    pub fn current() -> &'static ExecutableSearchPaths {
        static CURRENT: OnceLock<ExecutableSearchPaths> = OnceLock::new();
        CURRENT.get_or_init(|| {
            let read = |name: &str| std::env::var(name).unwrap_or_default();
            ExecutableSearchPaths {
                local_app_data: read("LOCALAPPDATA"),
                app_data: read("APPDATA"),
                user_profile: read("USERPROFILE"),
                program_files: read("ProgramFiles"),
                program_files_x86: read("ProgramFiles(x86)"),
            }
        })
    }

    /// 在指定目錄下尋找檔案，依最後寫入時間由新到舊排序（優先命中最新版本的雜湊子目錄）。
    pub fn search(&self, base_directory: &Path, file_name: &str, max_depth: usize) -> Vec<String> {
        fn walk(directory: &Path, file_name: &str, depth: usize, found: &mut Vec<(std::time::SystemTime, PathBuf)>) {
            let Ok(entries) = std::fs::read_dir(directory) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let Ok(metadata) = entry.metadata() else {
                    continue;
                };
                if metadata.is_dir() {
                    if depth > 0 {
                        walk(&path, file_name, depth - 1, found);
                    }
                } else if entry.file_name().to_string_lossy().eq_ignore_ascii_case(file_name) {
                    found.push((metadata.modified().unwrap_or(std::time::UNIX_EPOCH), path));
                }
            }
        }

        if base_directory.as_os_str().is_empty() || !base_directory.is_dir() {
            return Vec::new();
        }
        let mut found = Vec::new();
        walk(base_directory, file_name, max_depth, &mut found);
        found.sort_by_key(|entry| std::cmp::Reverse(entry.0));
        found.into_iter().map(|(_, path)| path.to_string_lossy().into_owned()).collect()
    }
}

/// 單一 CLI 的完整描述：顯示名稱、預設命令、參數計畫與可執行檔候選路徑。
pub struct CliDescriptor {
    pub kind: CliKind,
    /// 設定與結果視窗使用的完整名稱。
    pub display_name: &'static str,
    /// 排程清單欄位使用的短名稱。
    pub short_name: &'static str,
    /// 找不到自訂路徑時，最後回退到 PATH 搜尋的命令名稱。
    pub default_command: &'static str,
    /// 永遠加入的基本參數（子命令必須排在第一個）。
    pub base_arguments: &'static [&'static str],
    /// 提示詞要附加在哪個旗標之後。
    ///
    /// None 表示提示詞是結尾的位置參數（Codex、Claude CLI）。
    /// Antigravity 的 `--print` 是「值就是提示詞」的字串旗標
    /// （單獨給會得到 `flag needs an argument: -print`），
    /// 而且 Go 的 flag 套件遇到第一個非旗標參數就停止解析，
    /// 所以它必須排在所有旗標的最後面。
    pub prompt_flag: Option<&'static str>,
    /// 節省 Token 模式追加的參數。
    /// 目標是移除請求中最肥的部分：MCP 工具結構描述、專案說明檔、技能與外掛，
    /// 並把推理與輸出量壓到最低。
    pub token_saver_arguments: &'static [&'static str],
    /// 使用者未自訂 --model 時採用的預設模型（None 表示交給 CLI 自行決定）。
    pub default_model: Option<&'static str>,
    /// UI 下拉選單推薦的最新模型清單（首項為空代表本工具預設，亦可自訂輸入）。
    pub preset_models: &'static [&'static str],
    /// 此 CLI 支援的思考程度 / 推理強度選項。
    pub supported_efforts: &'static [ThinkingEffort],
    /// 已知模型各自支援的思考程度；自訂模型則回退到 `supported_efforts`。
    pub model_supported_efforts: &'static [(&'static str, &'static [ThinkingEffort])],
    /// 依應用程式逾時值產生 CLI 自身的逾時參數，讓子程序自己收尾而不是被強制終止。
    pub timeout_arguments: Option<fn(Duration) -> Vec<String>>,
    /// 已知安裝位置的候選路徑（依優先順序）。
    pub executable_candidates: fn(&ExecutableSearchPaths) -> Vec<String>,
}

impl CliDescriptor {
    pub fn get_supported_efforts(&self, model: Option<&str>) -> &'static [ThinkingEffort] {
        let normalized = match model.map(str::trim) {
            Some(value) if !value.is_empty() => Some(value),
            _ => self.default_model,
        };
        normalized
            .filter(|value| !value.trim().is_empty())
            .and_then(|value| {
                self.model_supported_efforts
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case(value))
                    .map(|(_, efforts)| *efforts)
            })
            .unwrap_or(self.supported_efforts)
    }

    /// 把舊設定或不相容的 Effort 降到該模型可接受的最接近等級。
    pub fn normalize_effort(&self, model: Option<&str>, effort: ThinkingEffort) -> ThinkingEffort {
        let supported = self.get_supported_efforts(model);
        if supported.contains(&effort) {
            return effort;
        }

        let requested_rank = effort_rank(effort);
        let mut fallback = E::Default;
        let mut fallback_rank = i32::MIN;
        for &candidate in supported {
            if candidate == E::Default {
                continue;
            }
            let candidate_rank = effort_rank(candidate);
            if candidate_rank <= requested_rank && candidate_rank > fallback_rank {
                fallback = candidate;
                fallback_rank = candidate_rank;
            }
        }

        if fallback != E::Default {
            return fallback;
        }
        supported.iter().copied().find(|&candidate| candidate != E::Default).unwrap_or(E::Default)
    }
}

fn effort_rank(effort: ThinkingEffort) -> i32 {
    match effort {
        E::Minimal => 0,
        E::Low => 1,
        E::Medium => 2,
        E::High => 3,
        E::XHigh => 4,
        E::Max => 5,
        E::Ultra => 6,
        E::Default => -1,
    }
}

const ONLY_DEFAULT: &[ThinkingEffort] = &[E::Default];
const AGY_FLASH: &[ThinkingEffort] = &[E::Default, E::Low, E::Medium, E::High];
const AGY_PRO: &[ThinkingEffort] = &[E::Default, E::Low, E::High];
const CODEX_ULTRA: &[ThinkingEffort] = &[E::Default, E::Low, E::Medium, E::High, E::XHigh, E::Max, E::Ultra];
const CODEX_MAX: &[ThinkingEffort] = &[E::Default, E::Low, E::Medium, E::High, E::XHigh, E::Max];
const CODEX_XHIGH: &[ThinkingEffort] = &[E::Default, E::Low, E::Medium, E::High, E::XHigh];

static DESCRIPTORS: [CliDescriptor; 5] = [
    CliDescriptor {
        kind: CliKind::Antigravity,
        display_name: "Antigravity (Gemini)",
        short_name: "AGY(Gemini)",
        default_command: "agy",
        base_arguments: &[],
        prompt_flag: Some("--print"),
        // --disable-slash-commands 停用技能展開；--mode plan 唯讀工作區；
        // 推理程度由 command_builder 依設定或節省模式動態附加。
        token_saver_arguments: &["--disable-slash-commands"],
        // 以 `agy models` 實際輸出核對：基底模型名稱需搭配獨立的 --effort 旗標
        // （帶後綴的完整 ID，如 gemini-3.8-flash-high，是另一種寫法，這裡固定用前者）。
        // Gemini 4 Argon 另有獨立的每週額度，由 AntigravityArgon 負責喚醒與倒數。
        default_model: Some("gemini-3.8-flash"),
        preset_models: &["", "gemini-3.8-flash", "gemini-3.8-flash-low", "gemini-3.8-flash-high"],
        supported_efforts: AGY_FLASH,
        model_supported_efforts: &[
            ("gemini-3.8-flash", AGY_FLASH),
            ("gemini-3.8-flash-high", ONLY_DEFAULT),
            ("gemini-3.8-flash-medium", ONLY_DEFAULT),
            ("gemini-3.8-flash-low", ONLY_DEFAULT),
            ("gemini-3.7-flash", AGY_FLASH),
            ("gemini-3.7-flash-high", ONLY_DEFAULT),
            ("gemini-3.7-flash-medium", ONLY_DEFAULT),
            ("gemini-3.7-flash-low", ONLY_DEFAULT),
            ("gemini-3.6-flash", AGY_FLASH),
            ("gemini-3.6-flash-high", ONLY_DEFAULT),
            ("gemini-3.6-flash-medium", ONLY_DEFAULT),
            ("gemini-3.6-flash-low", ONLY_DEFAULT),
            ("gemini-3.1-pro", AGY_PRO),
            ("gemini-3.1-pro-high", ONLY_DEFAULT),
            ("gemini-3.1-pro-low", ONLY_DEFAULT),
        ],
        timeout_arguments: Some(agy_timeout_arguments),
        executable_candidates: antigravity_candidates,
    },
    CliDescriptor {
        kind: CliKind::AntigravityArgon,
        display_name: "Antigravity (Argon)",
        short_name: "AGY(Argon)",
        default_command: "agy",
        base_arguments: &[],
        prompt_flag: Some("--print"),
        token_saver_arguments: &["--disable-slash-commands"],
        // Gemini 4 Argon 的額度與 Gemini Models 分開計算（只有每週視窗），
        // 獨立成一個目標才能各自喚醒、各自倒數。基底 ID 搭配獨立 --effort。
        default_model: Some("gemini-4-argon"),
        preset_models: &["", "gemini-4-argon", "gemini-4-argon-low", "gemini-4-argon-high"],
        supported_efforts: AGY_FLASH,
        model_supported_efforts: &[
            ("gemini-4-argon", AGY_FLASH),
            ("gemini-4-argon-high", ONLY_DEFAULT),
            ("gemini-4-argon-medium", ONLY_DEFAULT),
            ("gemini-4-argon-low", ONLY_DEFAULT),
        ],
        timeout_arguments: Some(agy_timeout_arguments),
        executable_candidates: antigravity_candidates,
    },
    CliDescriptor {
        kind: CliKind::AntigravityClaude,
        display_name: "Antigravity (Claude / GPT)",
        short_name: "AGY(Claude)",
        default_command: "agy",
        base_arguments: &[],
        prompt_flag: Some("--print"),
        token_saver_arguments: &["--disable-slash-commands"],
        // 2026-10-04 以 `agy models` 核對；使用包含思考程度的完整 ID，
        // 不再附加獨立 --effort，避免後綴與旗標互相衝突。
        default_model: Some("claude-sonnet-5-5-low"),
        preset_models: &[
            "claude-sonnet-5-5-low",
            "claude-sonnet-5-5-medium",
            "claude-sonnet-5-5-high",
            "claude-opus-5-5-low",
            "claude-opus-5-5-medium",
            "claude-opus-5-5-high",
            "gpt-oss-120b-medium",
        ],
        supported_efforts: ONLY_DEFAULT,
        model_supported_efforts: &[],
        timeout_arguments: Some(agy_timeout_arguments),
        executable_candidates: antigravity_candidates,
    },
    CliDescriptor {
        kind: CliKind::Codex,
        display_name: "Codex CLI",
        short_name: "Codex",
        default_command: "codex",
        // --ephemeral 不寫入 session 檔（省磁碟與記憶體）、--color never 讓擷取到的輸出不含 ANSI 控制碼。
        base_arguments: &["exec", "--skip-git-repo-check", "--ephemeral", "--color", "never"],
        prompt_flag: None,
        // --ignore-user-config 會跳過 config.toml，連帶不載入任何 MCP 伺服器與自訂指示，
        // 這是 Codex 這一側最大的 Token 節省來源（驗證通過：認證仍走 CODEX_HOME，不受影響）。
        token_saver_arguments: &[
            "--sandbox",
            "read-only",
            "--ignore-user-config",
            "--ignore-rules",
            "-c",
            "model_verbosity=\"low\"",
        ],
        // 2026-10-04 由本機 Codex App Server `model/list` 取得的可見清單。
        // 空白設定使用本工具的低價喚醒預設；使用者可明確指定其他模型。
        default_model: Some("gpt-6-luna"),
        preset_models: &[
            "",
            "gpt-6.1-sol",
            "gpt-6-astra",
            "gpt-6-sol",
            "gpt-6-luna",
            "gpt-5.6-sol",
            "gpt-5.6-terra",
            "gpt-5.6-luna",
            "gpt-5.5",
        ],
        supported_efforts: CODEX_ULTRA,
        model_supported_efforts: &[
            ("gpt-6.1-sol", CODEX_ULTRA),
            ("gpt-6-astra", CODEX_ULTRA),
            ("gpt-6-sol", CODEX_ULTRA),
            ("gpt-6-luna", CODEX_MAX),
            ("gpt-5.6-sol", CODEX_ULTRA),
            ("gpt-5.6-terra", CODEX_ULTRA),
            ("gpt-5.6-luna", CODEX_MAX),
            ("gpt-5.5", CODEX_XHIGH),
        ],
        timeout_arguments: None,
        executable_candidates: codex_candidates,
    },
    CliDescriptor {
        kind: CliKind::Claude,
        display_name: "Claude CLI",
        short_name: "Claude",
        default_command: "claude",
        base_arguments: &["--print"],
        prompt_flag: None,
        // --safe-mode 停用 CLAUDE.md、技能、外掛、hooks 與 MCP 伺服器（認證與內建工具照常運作）；
        // --strict-mcp-config 再次確保不載入任何 MCP 工具結構描述；
        // --tools "" 移除全部內建工具定義。三者合計可拿掉請求中絕大多數的固定輸入。
        // 注意：--tools 是可變長度參數，後面必須緊接一個旗標，否則會吃掉下一個位置參數。
        token_saver_arguments: &[
            "--safe-mode",
            "--strict-mcp-config",
            "--tools",
            "",
            "--no-session-persistence",
            "--prompt-suggestions",
            "false",
        ],
        // 2026-10-04 官方 model-config 文件；保留最新版別名並提供固定版本。
        // CLI 版本需求：Sonnet 5.5 >= 2.1.284、Opus 5.5 >= 2.1.280、Fable 5.1 >= 2.1.257。
        default_model: Some("claude-haiku-4-5-20251001"),
        preset_models: &[
            "",
            "claude-haiku-4-5-20251001",
            "haiku",
            "sonnet",
            "opus",
            "fable",
            "claude-sonnet-5-5",
            "claude-opus-5-5",
            "claude-fable-5-1",
        ],
        supported_efforts: CODEX_MAX,
        // Haiku uses manual thinking and does not support adaptive effort.
        model_supported_efforts: &[
            ("claude-haiku-4-5-20251001", ONLY_DEFAULT),
            ("claude-haiku-4-5", ONLY_DEFAULT),
            ("haiku", ONLY_DEFAULT),
        ],
        timeout_arguments: None,
        executable_candidates: claude_candidates,
    },
];

pub fn all() -> &'static [CliDescriptor] {
    &DESCRIPTORS
}

/// 依畫面顯示順序列出全部 CLI 種類（新排程的預設目標）。
pub fn kinds() -> Vec<CliKind> {
    DESCRIPTORS.iter().map(|descriptor| descriptor.kind).collect()
}

pub fn get(kind: CliKind) -> &'static CliDescriptor {
    DESCRIPTORS.iter().find(|descriptor| descriptor.kind == kind).expect("目錄應涵蓋所有 CliKind")
}

/// 將逾時值轉為 Go 風格的期間字串（Antigravity CLI 使用）。
fn agy_timeout_arguments(timeout: Duration) -> Vec<String> {
    let seconds = timeout.as_secs_f64().ceil() as u64;
    vec!["--print-timeout".to_string(), format!("{}s", seconds.max(1))]
}

fn join(root: &str, parts: &[&str]) -> String {
    let mut path = PathBuf::from(root);
    for part in parts {
        path.push(part);
    }
    path.to_string_lossy().into_owned()
}

fn antigravity_candidates(paths: &ExecutableSearchPaths) -> Vec<String> {
    let local = &paths.local_app_data;
    let mut list = vec![join(local, &["agy", "bin", "agy.exe"])];
    list.extend(paths.search(Path::new(&join(local, &["agy", "bin"])), "agy.exe", 2));
    list.push(join(local, &["Programs", "agy", "bin", "agy.exe"]));
    list.push(join(local, &["Programs", "agy", "agy.exe"]));
    list.push(join(local, &["Programs", "Antigravity", "bin", "agy.exe"]));
    list.push(join(local, &["Programs", "Antigravity", "agy.exe"]));
    list.push(join(&paths.user_profile, &[".local", "bin", "agy.exe"]));
    list.push(join(&paths.user_profile, &[".antigravity", "bin", "agy.exe"]));
    list.push(join(&paths.user_profile, &[".gemini", "antigravity", "bin", "agy.exe"]));
    list.push(join(&paths.app_data, &["npm", "agy.cmd"]));
    list.push(join(&paths.app_data, &["npm", "agy.exe"]));
    list
}

fn codex_candidates(paths: &ExecutableSearchPaths) -> Vec<String> {
    let local = &paths.local_app_data;
    let mut list = Vec::new();

    // 1. OpenAI Codex bin 子目錄（雜湊目錄，取最新版本）
    list.extend(paths.search(Path::new(&join(local, &["OpenAI", "Codex", "bin"])), "codex.exe", 2));
    list.push(join(local, &["OpenAI", "Codex", "bin", "codex.exe"]));
    list.push(join(local, &["OpenAI", "Codex", "codex.exe"]));

    // 2. Programs 目錄中的 OpenAI Codex
    list.extend(paths.search(Path::new(&join(local, &["Programs", "OpenAI", "Codex"])), "codex.exe", 2));
    list.push(join(local, &["Programs", "OpenAI", "Codex", "bin", "codex.exe"]));
    list.push(join(local, &["Programs", "OpenAI", "Codex", "codex.exe"]));

    // 3. Programs / Codex 歷史路徑
    list.extend(paths.search(Path::new(&join(local, &["Programs", "Codex"])), "codex.exe", 2));
    list.push(join(local, &["Programs", "Codex", "codex.exe"]));
    list.push(join(local, &["Programs", "Codex", "bin", "codex.exe"]));

    // 4. 使用者家目錄與 Local bin
    list.push(join(&paths.user_profile, &[".local", "bin", "codex.exe"]));
    list.push(join(&paths.user_profile, &[".local", "bin", "codex.cmd"]));
    list.push(join(&paths.user_profile, &[".codex", "bin", "codex.exe"]));

    // 5. npm 全域安裝
    list.push(join(&paths.app_data, &["npm", "codex.cmd"]));
    list.push(join(&paths.app_data, &["npm", "codex.exe"]));

    // 6. Program Files 安裝路徑
    for root in [&paths.program_files, &paths.program_files_x86] {
        if root.trim().is_empty() {
            continue;
        }
        list.extend(paths.search(Path::new(&join(root, &["OpenAI", "Codex"])), "codex.exe", 2));
        list.push(join(root, &["OpenAI", "Codex", "codex.exe"]));
        list.push(join(root, &["OpenAI", "Codex", "bin", "codex.exe"]));
    }
    list
}

fn claude_candidates(paths: &ExecutableSearchPaths) -> Vec<String> {
    let local = &paths.local_app_data;
    let mut list = vec![
        join(&paths.user_profile, &[".local", "bin", "claude.exe"]),
        join(&paths.user_profile, &[".local", "bin", "claude.cmd"]),
        join(local, &["Programs", "Claude", "claude.exe"]),
        join(local, &["Claude", "bin", "claude.exe"]),
    ];
    list.extend(paths.search(Path::new(&join(local, &["Programs", "Claude"])), "claude.exe", 2));
    list.push(join(&paths.app_data, &["npm", "claude.cmd"]));
    list.push(join(&paths.app_data, &["npm", "claude.exe"]));
    list
}
