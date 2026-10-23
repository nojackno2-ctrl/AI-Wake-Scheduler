//! 檢查各 CLI 是否有新版本。
//!
//! 三個 CLI 都有自己的 `update` 子命令，但都沒有「只檢查不安裝」的模式，
//! 所以這裡自行比對：本機版本由 `<cli> --version` 取得，最新版本向各 CLI 的發布來源查詢。
//! 本程式只負責提示與開啟更新主控台，實際安裝一律交給 CLI 自己的 `update`。

use std::cmp::Ordering;
use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;

use crate::error::CoreResult;
use crate::http::{self, HttpRequest};
use crate::login::{self, LoginStartInfo};
use crate::models::CliKind;
use crate::process;
use crate::time::CancelToken;

const VERSION_TIMEOUT: Duration = Duration::from_secs(30);
const QUERY_TIMEOUT: Duration = Duration::from_secs(15);

static VERSION_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d+\.\d+\.\d+").expect("版本號樣式應可編譯"));

/// 需要各自更新的 CLI 程式（三個 Antigravity 目標共用同一個 agy）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CliTool {
    Antigravity,
    Codex,
    Claude,
}

impl CliTool {
    pub const ALL: [CliTool; 3] = [CliTool::Antigravity, CliTool::Codex, CliTool::Claude];

    pub fn display_name(self) -> &'static str {
        match self {
            CliTool::Antigravity => "Antigravity CLI (agy)",
            CliTool::Codex => "Codex CLI",
            CliTool::Claude => "Claude Code",
        }
    }

    /// 用來取得可執行檔路徑設定的目標。
    pub fn kind(self) -> CliKind {
        match self {
            CliTool::Antigravity => CliKind::Antigravity,
            CliTool::Codex => CliKind::Codex,
            CliTool::Claude => CliKind::Claude,
        }
    }

    /// 最新版本的查詢來源。agy 的網址取自 agy 執行檔內建的自動更新服務。
    pub fn latest_version_url(self) -> &'static str {
        match self {
            CliTool::Antigravity => {
                "https://antigravity-cli-auto-updater-974169037036.us-central1.run.app/manifests/windows_amd64.json"
            }
            CliTool::Codex => "https://registry.npmjs.org/@openai/codex/latest",
            CliTool::Claude => "https://registry.npmjs.org/@anthropic-ai/claude-code/latest",
        }
    }

    /// 三個 CLI 都以 `update` 子命令自行更新（Claude Code 另有別名 `upgrade`）。
    pub fn update_arguments(self) -> &'static [&'static str] {
        &["update"]
    }
}

/// 單一 CLI 的版本檢查結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliUpdateStatus {
    pub tool: CliTool,
    pub executable: Option<String>,
    pub installed: Option<String>,
    pub latest: Option<String>,
    pub error: Option<String>,
}

impl CliUpdateStatus {
    pub fn update_available(&self) -> bool {
        match (&self.installed, &self.latest) {
            (Some(installed), Some(latest)) => compare_versions(latest, installed) == Ordering::Greater,
            _ => false,
        }
    }

    /// 清單與通知用的一行說明。
    pub fn describe(&self) -> String {
        let name = self.tool.display_name();
        match (&self.installed, &self.latest) {
            _ if self.executable.is_none() => format!("{name}：找不到 CLI"),
            (Some(installed), Some(latest)) if self.update_available() => {
                format!("{name}：{installed} → {latest}（有新版本）")
            }
            (Some(installed), Some(_)) => format!("{name}：{installed}（已是最新）"),
            (Some(installed), None) => {
                format!("{name}：{installed}（無法查詢最新版本：{}）", self.error.as_deref().unwrap_or("未知錯誤"))
            }
            (None, _) => format!("{name}：無法讀取版本（{}）", self.error.as_deref().unwrap_or("未知錯誤")),
        }
    }
}

/// 取出文字中的第一個 `x.y.z` 版本號（例如 `codex-cli 0.160.0`、`2.1.289 (Claude Code)`）。
pub fn parse_version(text: &str) -> Option<String> {
    VERSION_PATTERN.find(text).map(|found| found.as_str().to_string())
}

/// 逐段以數字比較版本號；缺少的段落視為 0。
pub fn compare_versions(left: &str, right: &str) -> Ordering {
    let parts = |text: &str| -> Vec<u64> { text.split('.').map(|part| part.trim().parse().unwrap_or(0)).collect() };
    let (left, right) = (parts(left), parts(right));
    for index in 0..left.len().max(right.len()) {
        let ordering = left.get(index).unwrap_or(&0).cmp(right.get(index).unwrap_or(&0));
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    Ordering::Equal
}

/// 由發布來源的 JSON（npm registry 的 latest 或 agy 的 manifest）取出版本號。
pub fn parse_latest_version(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    value.get("version")?.as_str().and_then(parse_version)
}

pub fn installed_version(executable: &str, working_directory: &str, cancel: &CancelToken) -> Result<String, String> {
    let execution = process::execute(executable, &["--version"], working_directory, VERSION_TIMEOUT, cancel, None)
        .map_err(|error| error.message().to_string())?;
    if execution.timed_out || execution.exit_code != 0 {
        return Err(format!("版本檢查失敗（結束碼 {}，逾時：{}）", execution.exit_code, execution.timed_out));
    }
    let output = format!("{}\n{}", execution.standard_output, execution.standard_error);
    parse_version(&output).ok_or_else(|| format!("無法辨識版本輸出（結束碼 {}）", execution.exit_code))
}

pub fn latest_version(tool: CliTool) -> Result<String, String> {
    let response = http::send(&HttpRequest {
        method: "GET",
        url: tool.latest_version_url(),
        headers: &[("Accept", "application/json")],
        body: None,
        connect_timeout: QUERY_TIMEOUT,
        timeout: QUERY_TIMEOUT,
        ignore_certificate_errors: false,
    })?;
    if !response.is_success() {
        return Err(format!("HTTP {}", response.status));
    }
    parse_latest_version(&response.body).ok_or_else(|| "回應中沒有版本號".to_string())
}

/// 檢查一個 CLI；`executable` 為 None 代表找不到 CLI。
pub fn check(
    tool: CliTool,
    executable: Option<String>,
    working_directory: &str,
    cancel: &CancelToken,
) -> CliUpdateStatus {
    let mut status = CliUpdateStatus { tool, executable, installed: None, latest: None, error: None };
    let Some(executable) = status.executable.clone() else {
        return status;
    };
    match installed_version(&executable, working_directory, cancel) {
        Ok(version) => status.installed = Some(version),
        Err(error) => {
            status.error = Some(error);
            return status;
        }
    }
    match latest_version(tool) {
        Ok(version) => status.latest = Some(version),
        Err(error) => status.error = Some(error),
    }
    status
}

/// 開一個看得見的主控台執行 CLI 自己的 `update`，失敗時暫停讓使用者看到錯誤。
pub fn create_update_start_info(
    tool: CliTool,
    executable: &str,
    working_directory: &str,
) -> CoreResult<LoginStartInfo> {
    login::create_console_start_info(executable, tool.update_arguments(), working_directory)
}
