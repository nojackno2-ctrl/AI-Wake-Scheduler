//! 讀取 CLI 帳戶額度。Codex 使用官方 app-server 的
//! account/rateLimits/read；Claude 使用 Claude Code 本身查詢 /api/oauth/usage
//! 時採用的本機 OAuth 憑證；絕不以排程時間推測帳戶額度。

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use regex::Regex;
use serde_json::Value;

use crate::catalog;
use crate::error::{CoreError, CoreResult};
use crate::http::{self, HttpResponse};
use crate::locator;
use crate::login;
use crate::models::{CliKind, CliProfile};
use crate::process;
use crate::time::{self, CancelToken, Moment, Span};
use crate::win;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliUsageAvailability {
    Available,
    Unsupported,
    Unavailable,
}

/// CLI 帳戶的一個額度視窗。
#[derive(Debug, Clone, PartialEq)]
pub struct CliUsageWindow {
    pub name: String,
    pub used_percent: i32,
    pub duration: Option<Span>,
    pub resets_at: Option<Moment>,
    pub is_active_countdown: bool,
}

impl CliUsageWindow {
    pub fn new(
        name: impl Into<String>,
        used_percent: i32,
        duration: Option<Span>,
        resets_at: Option<Moment>,
        is_active_countdown: bool,
    ) -> Self {
        Self { name: name.into(), used_percent, duration, resets_at, is_active_countdown }
    }

    pub fn remaining_percent(&self) -> i32 {
        (100 - self.used_percent).clamp(0, 100)
    }
}

/// 一次唯讀用量查詢的結果。
/// `requires_login` 表示失敗原因是 CLI 登入失效，UI 據此提供一鍵登入。
#[derive(Debug, Clone, PartialEq)]
pub struct CliUsageSnapshot {
    pub cli: CliKind,
    pub availability: CliUsageAvailability,
    pub windows: Vec<CliUsageWindow>,
    pub message: String,
    pub observed_at: Moment,
    pub requires_login: bool,
}

impl CliUsageSnapshot {
    pub fn new(
        cli: CliKind,
        availability: CliUsageAvailability,
        windows: Vec<CliUsageWindow>,
        message: impl Into<String>,
        observed_at: Moment,
    ) -> Self {
        Self { cli, availability, windows, message: message.into(), observed_at, requires_login: false }
    }

    /// 5 小時短週期額度視窗；沒有時取第一個視窗。
    pub fn short_window_or_first(&self) -> Option<&CliUsageWindow> {
        self.windows
            .iter()
            .find(|window| window.duration.is_some_and(|duration| duration <= Span::hours(6)))
            .or_else(|| self.windows.first())
    }
}

/// CLI 額度與倒數讀取的抽象。排程管理器可藉此判斷個別 CLI 是否仍在倒數中。
pub trait UsageReader: Send + Sync {
    fn read(
        &self,
        kind: CliKind,
        profile: &CliProfile,
        working_directory: &str,
        cancel: &CancelToken,
    ) -> CoreResult<CliUsageSnapshot>;

    fn latest_snapshot(&self, _kind: CliKind) -> Option<CliUsageSnapshot> {
        None
    }

    fn latest_snapshots(&self) -> HashMap<CliKind, CliUsageSnapshot> {
        HashMap::new()
    }
}

/// 判斷指定 CLI 的用量快照是否正處於有效的額度重置倒數中。
/// 若額度未處於倒數狀態、倒數時間已過或無有效倒數時間戳記，傳回 false。
pub fn is_counting_down(snapshot: Option<&CliUsageSnapshot>, now: &Moment) -> bool {
    let Some(snapshot) = snapshot else {
        return false;
    };
    if snapshot.availability != CliUsageAvailability::Available {
        return false;
    }

    // 對於 Claude，優先檢查 5 小時短週期額度視窗；其餘 CLI 取主要視窗
    match snapshot.short_window_or_first() {
        Some(window) if window.is_active_countdown => window.resets_at.is_some_and(|resets_at| resets_at > *now),
        _ => false,
    }
}

const QUERY_TIMEOUT: Duration = Duration::from_secs(15);
const CLAUDE_USAGE_ENDPOINT: &str = "https://api.anthropic.com/api/oauth/usage";
const CLAUDE_OAUTH_BETA: &str = "oauth-2025-04-20";

const AGY_CACHE_DURATION: Duration = Duration::from_secs(3);
/// 自行啟動 agy 取得的結果要快取久一點。
/// 三個 Antigravity 額度池（Gemini、Argon 與 Claude / GPT）是分開讀的，
/// 快取太短會讓後續讀取又啟動一次 agy。
const AGY_LAUNCHED_CACHE_DURATION: Duration = Duration::from_secs(20);
const AGY_QUOTA_RPC_METHODS: [&str; 4] = [
    // Antigravity 2.x：提供 Gemini／Claude+GPT 的 5 小時與每週 buckets。
    "RetrieveUserQuotaSummary",
    // 舊版／IDE fallback：提供模型層級的 session quota。
    "GetUserStatus",
    "GetCommandModelConfigs",
    "GetCascadeModelConfigData",
];

const CLAUDE_CACHE_SECONDS: i64 = 30;
const CLAUDE_DEFAULT_RATE_LIMIT_BACKOFF_MINUTES: i64 = 5;

struct CachedAgyResponse {
    json: String,
    cached_at: Instant,
    ttl: Duration,
    executable: String,
    working_directory: String,
    auto_start: bool,
}

/// 一組已驗證可用的 Antigravity Language Server 連線。
/// 連接埠、CSRF Token 與實際回應額度的 RPC 方法一起快取，
/// 下一次讀取就能直接命中，不必重新掃描日誌與列舉方法。
#[derive(Clone)]
struct AntigravityConnection {
    port: u16,
    csrf_token: String,
    rpc_method: &'static str,
}

/// 同一時間只允許一個 Antigravity 查詢；鎖內同時保存短效快取。
static AGY_QUERY: LazyLock<Mutex<Option<CachedAgyResponse>>> = LazyLock::new(|| Mutex::new(None));
static ACTIVE_AGY_CONNECTION: LazyLock<Mutex<Option<AntigravityConnection>>> = LazyLock::new(|| Mutex::new(None));

fn active_connection() -> std::sync::MutexGuard<'static, Option<AntigravityConnection>> {
    ACTIVE_AGY_CONNECTION.lock().unwrap_or_else(|e| e.into_inner())
}

/// 清除已快取的 Antigravity 連線（Antigravity 重啟後連接埠與 Token 都會改變）。
pub fn clear_antigravity_connection_cache() {
    *active_connection() = None;
}

#[derive(Default)]
struct ClaudeState {
    cached_snapshot: Option<CliUsageSnapshot>,
    cached_at: Option<Moment>,
    retry_after_at: Option<Moment>,
}

pub struct CliUsageReader {
    latest_snapshots: Mutex<HashMap<CliKind, CliUsageSnapshot>>,
    auto_start_antigravity: AtomicBool,
    claude: Mutex<ClaudeState>,
}

impl Default for CliUsageReader {
    fn default() -> Self {
        Self::new()
    }
}

impl CliUsageReader {
    pub fn new() -> Self {
        Self {
            latest_snapshots: Mutex::new(HashMap::new()),
            auto_start_antigravity: AtomicBool::new(true),
            claude: Mutex::new(ClaudeState::default()),
        }
    }

    /// 是否允許背景無視窗執行 agy 內建 /usage 指令讀取額度。
    /// 關閉後僅讀取已啟動的 IDE language server。
    pub fn auto_start_antigravity(&self) -> bool {
        self.auto_start_antigravity.load(Ordering::SeqCst)
    }

    pub fn set_auto_start_antigravity(&self, value: bool) {
        self.auto_start_antigravity.store(value, Ordering::SeqCst);
    }

    fn read_antigravity(
        &self,
        kind: CliKind,
        profile: &CliProfile,
        working_directory: &str,
        cancel: &CancelToken,
    ) -> CoreResult<CliUsageSnapshot> {
        let observed_at = time::now();
        let auto_start = self.auto_start_antigravity();

        let response_json = {
            let mut cache = AGY_QUERY.lock().unwrap_or_else(|e| e.into_inner());
            let reusable = cache.as_ref().filter(|cached| {
                cached.cached_at.elapsed() < cached.ttl
                    && cached.executable == profile.executable
                    && cached.working_directory == working_directory
                    && cached.auto_start == auto_start
            });
            match reusable {
                Some(cached) => cached.json.clone(),
                None => {
                    let fetched = self.fetch_antigravity_quota_json(profile, working_directory, auto_start, cancel);
                    match fetched {
                        Ok((json, launched)) => {
                            *cache = Some(CachedAgyResponse {
                                json: json.clone(),
                                cached_at: Instant::now(),
                                ttl: if launched { AGY_LAUNCHED_CACHE_DURATION } else { AGY_CACHE_DURATION },
                                executable: profile.executable.clone(),
                                working_directory: working_directory.to_string(),
                                auto_start,
                            });
                            json
                        }
                        Err(CoreError::Canceled) => return Err(CoreError::Canceled),
                        Err(error) => {
                            return Ok(unavailable(
                                kind,
                                format!("Antigravity 額度查詢失敗：{}", error.message()),
                                observed_at,
                            ))
                        }
                    }
                }
            }
        };

        let mut snapshot = parse_antigravity_model_configs(&response_json, kind, &observed_at);
        if snapshot.availability == CliUsageAvailability::Available {
            snapshot.message = describe_antigravity_coverage(&snapshot.windows).to_string();
        }
        Ok(snapshot)
    }

    /// 取得 Antigravity 額度 RPC 的原始 JSON。
    ///
    /// 開啟背景查詢時使用 agy 內建 /usage；關閉時依序嘗試
    /// 上一次成功的連線與既有 language server。
    /// 回傳的布林值表示這份資料來自本應用程式自己啟動的 CLI，
    /// 呼叫端要據此拉長快取時間，避免每個額度池都各啟動一次。
    fn fetch_antigravity_quota_json(
        &self,
        profile: &CliProfile,
        working_directory: &str,
        auto_start: bool,
        cancel: &CancelToken,
    ) -> CoreResult<(String, bool)> {
        if auto_start {
            let executable = locator::resolve(CliKind::Antigravity, Some(&profile.executable), working_directory)
                .ok_or_else(|| CoreError::FileNotFound("找不到 Antigravity CLI，請檢查 CLI 設定。".to_string()))?;
            let execution = process::execute(
                &executable,
                &["--output-format", "json", "--print", "/usage"],
                working_directory,
                QUERY_TIMEOUT,
                cancel,
                None,
            )?;
            if execution.timed_out {
                return Err(CoreError::InvalidOperation("Antigravity /usage 額度查詢逾時。".to_string()));
            }
            if execution.exit_code != 0 {
                return Err(CoreError::InvalidOperation(format!(
                    "Antigravity /usage 額度查詢失敗：{}",
                    trim_diagnostic(&execution.standard_error)
                )));
            }
            // 只接受內建 usage 指令回應；絕不將一般模型文字當成額度資料。
            let root: Value = serde_json::from_str(&execution.standard_output)
                .map_err(|error| CoreError::InvalidData(error.to_string()))?;
            let quota = object_property(&root, "command")
                .filter(|command| get_string(command, "name").as_deref() == Some("usage"))
                .and_then(|command| object_property(command, "data"))
                .filter(|quota| array_property(quota, "groups").is_some())
                .ok_or_else(|| {
                    CoreError::InvalidOperation(
                        "Antigravity CLI 未回傳內建 /usage 額度資料，請更新 agy CLI。".to_string(),
                    )
                })?;
            return Ok((quota.to_string(), true));
        }

        let cached_connection = active_connection().clone();
        if let Some(connection) = cached_connection {
            if let Some(body) = try_query_antigravity(&connection, cancel)? {
                return Ok((body, false));
            }
            // Antigravity 重啟後連接埠與 Token 都會換；捨棄快取重新探測。
            *active_connection() = None;
        }

        let process_id = win::process_ids_by_name("language_server").first().copied().unwrap_or(0);
        let (csrf_token, ports) = discover_antigravity_endpoints(process_id, cancel)?;

        // IDE 需要 CSRF；仍容許沒有 Token 的舊版 IDE 端點。
        for port in ports {
            for method in AGY_QUOTA_RPC_METHODS {
                let candidate = AntigravityConnection {
                    port,
                    csrf_token: csrf_token.clone().unwrap_or_default(),
                    rpc_method: method,
                };
                if let Some(body) = try_query_antigravity(&candidate, cancel)? {
                    *active_connection() = Some(candidate);
                    return Ok((body, false));
                }
            }
        }

        Err(CoreError::InvalidOperation(
            "Antigravity 尚未啟動，請先開啟 Antigravity 以讀取即時額度（自動啟動已關閉）。".to_string(),
        ))
    }

    fn read_claude(&self, cancel: &CancelToken) -> CoreResult<CliUsageSnapshot> {
        let mut state = self.claude.lock().unwrap_or_else(|e| e.into_inner());
        let observed_at = time::now();

        if let (Some(cached), Some(cached_at)) = (&state.cached_snapshot, &state.cached_at) {
            if observed_at - *cached_at < Span::seconds(CLAUDE_CACHE_SECONDS) {
                return Ok(refresh_claude_snapshot(
                    cached,
                    &observed_at,
                    "讀取成功（共用最近 30 秒內的 Claude 查詢結果）",
                ));
            }
        }

        if let Some(retry_after_at) = state.retry_after_at.filter(|retry| observed_at < *retry) {
            return Ok(create_claude_rate_limited_snapshot(&state, &observed_at, &retry_after_at));
        }

        let credentials_path = match claude_credentials_path().filter(|path| path.is_file()) {
            Some(path) => path,
            None => {
                return Ok(login_required(CliKind::Claude, "找不到 Claude Code 登入憑證，請重新登入。", observed_at))
            }
        };

        let read_token =
            |path: &Path| read_claude_access_token(path).map(|token| token.filter(|t| !t.trim().is_empty()));
        let mut access_token = match read_token(&credentials_path) {
            Ok(Some(token)) => token,
            Ok(None) => {
                return Ok(login_required(
                    CliKind::Claude,
                    "Claude Code 尚未以 Claude 訂閱帳號登入，請重新登入。",
                    observed_at,
                ))
            }
            Err(message) => return Ok(unavailable(CliKind::Claude, message, observed_at)),
        };

        for attempt in 0..2 {
            let token = access_token.clone();
            let response = send_cancellable(cancel, move || {
                let authorization = format!("Bearer {token}");
                http::send(&http::HttpRequest {
                    method: "GET",
                    url: CLAUDE_USAGE_ENDPOINT,
                    headers: &[
                        ("Authorization", &authorization),
                        ("Accept", "application/json"),
                        ("anthropic-beta", CLAUDE_OAUTH_BETA),
                        ("anthropic-version", "2023-06-01"),
                    ],
                    body: None,
                    connect_timeout: Duration::from_secs(10),
                    timeout: QUERY_TIMEOUT,
                    ignore_certificate_errors: false,
                })
            })?;
            let response = match response {
                Ok(response) => response,
                Err(message) => {
                    return Ok(unavailable(
                        CliKind::Claude,
                        format!("無法連線至 Claude 額度服務：{message}"),
                        observed_at,
                    ))
                }
            };

            if response.is_success() {
                let snapshot = match parse_claude_usage(&response.body, &observed_at) {
                    Ok(snapshot) => snapshot,
                    Err(message) => {
                        return Ok(unavailable(
                            CliKind::Claude,
                            format!("Claude 額度 JSON 解析失敗：{message}"),
                            observed_at,
                        ))
                    }
                };
                if snapshot.availability == CliUsageAvailability::Available {
                    state.cached_snapshot = Some(snapshot.clone());
                    state.cached_at = Some(observed_at);
                    state.retry_after_at = None;
                }
                return Ok(snapshot);
            }

            // Claude Code 可能剛好在另一個程序輪替短效 access token；重新讀檔後重試一次。
            if response.status == 401 && attempt == 0 {
                if let Ok(Some(refreshed)) = read_token(&credentials_path) {
                    if refreshed != access_token {
                        access_token = refreshed;
                        continue;
                    }
                }
                return Ok(login_required(CliKind::Claude, "Claude Code 登入憑證已過期，請重新登入。", observed_at));
            }

            if response.status == 429 {
                let retry_after_at = resolve_claude_retry_after(&response, &observed_at);
                state.retry_after_at = Some(retry_after_at);
                return Ok(create_claude_rate_limited_snapshot(&state, &observed_at, &retry_after_at));
            }

            return Ok(unavailable(
                CliKind::Claude,
                format!("Claude 額度查詢失敗（HTTP {}）。", response.status),
                observed_at,
            ));
        }

        Ok(unavailable(CliKind::Claude, "Claude 額度查詢失敗。", observed_at))
    }
}

impl UsageReader for CliUsageReader {
    fn read(
        &self,
        kind: CliKind,
        profile: &CliProfile,
        working_directory: &str,
        cancel: &CancelToken,
    ) -> CoreResult<CliUsageSnapshot> {
        let mut snapshot = match kind {
            CliKind::Codex => read_codex(profile, working_directory, cancel)?,
            CliKind::Antigravity | CliKind::AntigravityArgon | CliKind::AntigravityClaude => {
                self.read_antigravity(kind, profile, working_directory, cancel)?
            }
            CliKind::Claude => self.read_claude(cancel)?,
        };

        // Codex 與 Antigravity 的登入失效只會以錯誤文字呈現；Claude 已在讀取時明確標記。
        if snapshot.availability == CliUsageAvailability::Unavailable
            && !snapshot.requires_login
            && login::looks_like_login_required(Some(&snapshot.message))
        {
            snapshot.requires_login = true;
        }

        self.latest_snapshots.lock().unwrap_or_else(|e| e.into_inner()).insert(kind, snapshot.clone());
        Ok(snapshot)
    }

    fn latest_snapshot(&self, kind: CliKind) -> Option<CliUsageSnapshot> {
        self.latest_snapshots.lock().unwrap_or_else(|e| e.into_inner()).get(&kind).cloned()
    }

    fn latest_snapshots(&self) -> HashMap<CliKind, CliUsageSnapshot> {
        self.latest_snapshots.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

/// 在背景執行緒送出 HTTP 要求，等待期間可被取消。
fn send_cancellable(
    cancel: &CancelToken,
    send: impl FnOnce() -> Result<HttpResponse, String> + Send + 'static,
) -> CoreResult<Result<HttpResponse, String>> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(send());
    });
    loop {
        match receiver.recv_timeout(Duration::from_millis(50)) {
            Ok(result) => return Ok(result),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if cancel.is_canceled() {
                    return Err(CoreError::Canceled);
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(Err("HTTP 要求意外中止。".to_string())),
        }
    }
}

/// 說明這次讀到的額度涵蓋範圍。
/// Language Server 的 GetCascadeModelConfigData 只會回報短週期（5 小時）視窗，
/// 只有 RetrieveUserQuotaSummary 之類的新版 RPC 才會一併帶出每週額度；
/// 訊息必須誠實反映實際拿到什麼，才不會讓人誤以為週額度是滿的。
fn describe_antigravity_coverage(windows: &[CliUsageWindow]) -> &'static str {
    if windows.iter().any(|window| window.duration.is_some_and(|duration| duration >= Span::days(7))) {
        "讀取成功（含每週額度）"
    } else {
        "讀取成功（Language Server 僅提供短週期額度，不含每週額度）"
    }
}

/// 對單一連線嘗試一次額度 RPC；任何失敗都回傳 None，讓呼叫端換下一個候選。
fn try_query_antigravity(connection: &AntigravityConnection, cancel: &CancelToken) -> CoreResult<Option<String>> {
    let url = format!(
        "https://127.0.0.1:{}/exa.language_server_pb.LanguageServerService/{}",
        connection.port, connection.rpc_method
    );
    let csrf_token = connection.csrf_token.clone();
    let response = send_cancellable(cancel, move || {
        let mut headers = vec![("Content-Type", "application/json; charset=utf-8"), ("Connect-Protocol-Version", "1")];
        if !csrf_token.trim().is_empty() {
            headers.push(("x-codeium-csrf-token", &csrf_token));
        }
        http::send(&http::HttpRequest {
            method: "POST",
            url: &url,
            headers: &headers,
            body: Some(
                br#"{"metadata":{"ideName":"antigravity","extensionName":"antigravity","ideVersion":"unknown","locale":"en"}}"#,
            ),
            connect_timeout: Duration::from_secs(5),
            timeout: Duration::from_secs(5),
            ignore_certificate_errors: true,
        })
    })?;

    Ok(match response {
        Ok(response) if response.is_success() && !is_antigravity_error_payload(&response.body) => Some(response.body),
        _ => None,
    })
}

fn trim_diagnostic(text: &str) -> String {
    if text.trim().is_empty() {
        "空回應".to_string()
    } else {
        text.chars().take(200).collect()
    }
}

fn is_antigravity_error_payload(json: &str) -> bool {
    match serde_json::from_str::<Value>(json) {
        Ok(root) => {
            root.get("error").is_some()
                || object_property(&root, "response").is_some_and(|response| response.get("error").is_some())
        }
        Err(_) => false,
    }
}

static ANTIGRAVITY_PORT_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)listening on \w+ port at (\d+) for HTTPS").expect("連接埠樣式應可編譯"));

static ANTIGRAVITY_TOKEN_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)--(?:csrf_token|extension_server_csrf_token)(?:=|\s+)(?:"([^" ]+)"|'([^']+)'|([^\s]+))"#)
        .expect("Token 樣式應可編譯")
});

/// 找出 Antigravity Language Server 的 CSRF Token 與 HTTPS 連接埠。
///
/// 先掃 language_server.log 的尾端（純檔案讀取、不開子程序也不碰別的行程記憶體）；
/// 日誌不足以湊齊 Token 與連接埠時，才退回一次 CIM 查詢補齊。
fn discover_antigravity_endpoints(process_id: u32, cancel: &CancelToken) -> CoreResult<(Option<String>, Vec<u16>)> {
    let mut ports: Vec<u16> = Vec::new();
    let mut csrf_token: Option<String> = None;

    for log_path in antigravity_log_paths() {
        for line in read_tail_lines(&log_path, 300) {
            if let Some(port) = ANTIGRAVITY_PORT_PATTERN
                .captures(&line)
                .and_then(|captures| captures.get(1))
                .and_then(|value| value.as_str().parse::<u16>().ok())
            {
                if !ports.contains(&port) {
                    ports.push(port);
                }
            }
            if csrf_token.is_none() {
                csrf_token = extract_antigravity_token(&line);
            }
        }
    }

    // 較新的 language_server 不再把 Token 寫進日誌，連接埠也可能是舊的一輪；
    // 兩者任一缺漏就以單一次 CIM 查詢一併補齊命令列與 Listen 連接埠。
    if (csrf_token.is_none() || ports.is_empty()) && process_id > 0 {
        let (command_line, listening_ports) = query_process_details(process_id, cancel)?;
        if csrf_token.is_none() {
            csrf_token = command_line.as_deref().and_then(extract_antigravity_token);
        }
        for port in listening_ports {
            if !ports.contains(&port) {
                ports.push(port);
            }
        }
    }

    Ok((csrf_token, ports))
}

fn antigravity_log_paths() -> Vec<PathBuf> {
    let app_data = PathBuf::from(std::env::var("APPDATA").unwrap_or_default());
    let user_profile = PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default());
    vec![
        app_data.join("Antigravity").join("logs").join("language_server.log"),
        app_data.join("Antigravity IDE").join("logs").join("language_server.log"),
        user_profile.join(".gemini").join("antigravity").join("logs").join("language_server.log"),
    ]
}

/// 讀取檔案尾端的最後 `max_lines` 行。
/// 連接埠與 Token 都是最近一次啟動寫入的，所以必須從尾端讀而不是從頭讀；
/// 日誌可達數百 KB，這裡只載入尾端一小段以免每次讀取都掃整個檔案。
pub fn read_tail_lines(path: &Path, max_lines: usize) -> Vec<String> {
    const MAX_TAIL_BYTES: u64 = 128 * 1024;

    let read = || -> std::io::Result<Vec<String>> {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE：日誌可能正被寫入。
        let mut file = std::fs::OpenOptions::new().read(true).share_mode(0x7).open(path)?;
        let length = file.metadata()?.len();
        let tail_length = length.min(MAX_TAIL_BYTES);
        file.seek(SeekFrom::Start(length - tail_length))?;

        let mut buffer = Vec::with_capacity(tail_length as usize);
        file.take(tail_length).read_to_end(&mut buffer)?;
        let text = String::from_utf8_lossy(&buffer);

        let mut lines: Vec<&str> = text.split(['\r', '\n']).filter(|line| !line.is_empty()).collect();
        // 截斷處的第一行可能是半行，捨棄它。
        if tail_length < length && lines.len() > 1 {
            lines.remove(0);
        }

        let take = max_lines.min(lines.len());
        Ok(lines[lines.len() - take..].iter().map(|line| line.to_string()).collect())
    };
    read().unwrap_or_default()
}

/// 以單一次 PowerShell CIM 查詢取得指定行程的命令列與 Listen 連接埠。
fn query_process_details(process_id: u32, cancel: &CancelToken) -> CoreResult<(Option<String>, Vec<u16>)> {
    let script = format!(
        "$p = Get-CimInstance Win32_Process -Filter 'ProcessId = {process_id}' -ErrorAction SilentlyContinue; \
         $ports = @(Get-NetTCPConnection -OwningProcess {process_id} -State Listen -ErrorAction SilentlyContinue | \
         Select-Object -ExpandProperty LocalPort); \
         [pscustomobject]@{{ cmd = [string]$p.CommandLine; ports = ($ports -join ',') }} | ConvertTo-Json -Compress"
    );

    let execution = match process::execute(
        "powershell.exe",
        &["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &script],
        "",
        Duration::from_secs(5),
        cancel,
        None,
    ) {
        Ok(execution) => execution,
        Err(CoreError::Canceled) => return Err(CoreError::Canceled),
        Err(_) => return Ok((None, Vec::new())),
    };

    let mut ports = Vec::new();
    let Ok(root) = serde_json::from_str::<Value>(execution.standard_output.trim()) else {
        return Ok((None, ports));
    };
    let command_line = get_string(&root, "cmd");
    if let Some(raw_ports) = get_string(&root, "ports") {
        for raw in raw_ports.split(',').map(str::trim).filter(|raw| !raw.is_empty()) {
            if let Ok(port) = raw.parse::<u16>() {
                if !ports.contains(&port) {
                    ports.push(port);
                }
            }
        }
    }
    Ok((command_line, ports))
}

/// 從命令列或日誌行中取出 Antigravity 的 CSRF Token。
///
/// 新版 language_server 可能只暴露 extension server token，且旗標可用
/// --name=value 或 --name value；兩種格式都必須支援。
pub fn extract_antigravity_token(text: &str) -> Option<String> {
    let captures = ANTIGRAVITY_TOKEN_PATTERN.captures(text)?;
    captures.get(1).or_else(|| captures.get(2)).or_else(|| captures.get(3)).map(|value| value.as_str().to_string())
}

fn pool_name(kind: CliKind) -> &'static str {
    catalog::get(kind).display_name
}

/// 判斷額度群組／bucket 屬於哪個 Antigravity 額度池。
/// Argon 群組名稱同樣含有「Gemini」（例如 Gemini 4 Argon），必須先排除，
/// 否則它的每週額度會混進 Gemini Models；bucket 名稱也一併檢查，
/// 以涵蓋 Argon bucket 被放在 Gemini 群組內的回應格式。
fn matches_pool(kind: CliKind, group_label: &str, bucket_label: &str) -> bool {
    let group = group_label.to_ascii_lowercase();
    let is_argon = group.contains("argon") || bucket_label.to_ascii_lowercase().contains("argon");
    match kind {
        CliKind::AntigravityArgon => is_argon,
        CliKind::Antigravity => !is_argon && group.contains("gemini"),
        _ => !is_argon && (group.contains("claude") || group.contains("gpt")),
    }
}

fn used_percent_from_fraction(fraction: f64) -> i32 {
    (((1.0 - fraction.clamp(0.0, 1.0)) * 100.0).round() as i32).clamp(0, 100)
}

/// 解析 Antigravity 額度回應（內建 /usage、RetrieveUserQuotaSummary 或舊版模型設定）。
pub fn parse_antigravity_model_configs(json: &str, kind: CliKind, observed_at: &Moment) -> CliUsageSnapshot {
    let root: Value = match serde_json::from_str(json) {
        Ok(root) => root,
        Err(error) => {
            return unavailable(kind, format!("Antigravity 回應 JSON 解析失敗：{error}"), *observed_at);
        }
    };

    if let Some(error) = root.get("error") {
        return unavailable(kind, format!("Antigravity 額度查詢失敗：{}", error_message(error)), *observed_at);
    }

    // Antigravity 2.x 的 RetrieveUserQuotaSummary 會回傳
    // { response: { groups: [...] } }；部分版本則直接回傳 groups。
    if let Some(windows) = parse_antigravity_summary(&root, kind, observed_at) {
        return CliUsageSnapshot::new(kind, CliUsageAvailability::Available, windows, "讀取成功", *observed_at);
    }

    // 舊版 IDE / GetUserStatus / GetCommandModelConfigs 仍提供
    // clientModelConfigs；不要只取第一筆，否則前面的滿額模型會遮掉 5 小時倒數。
    if let Some(configs) = antigravity_configs(&root) {
        let windows = parse_antigravity_model_configs_array(configs, kind, observed_at);
        if !windows.is_empty() {
            return CliUsageSnapshot::new(kind, CliUsageAvailability::Available, windows, "讀取成功", *observed_at);
        }
    }

    unavailable(kind, "Antigravity 已回應，但未找到可解析的額度視窗。", *observed_at)
}

fn parse_antigravity_summary(root: &Value, kind: CliKind, observed_at: &Moment) -> Option<Vec<CliUsageWindow>> {
    let mut summary = root;
    if let Some(response) = object_property(summary, "response") {
        summary = response;
    }
    if let Some(data) = object_property(summary, "data") {
        summary = data;
    }
    let groups = array_property(summary, "groups")?;

    let mut windows = Vec::new();
    for group in groups {
        let group_name = get_string_any(group, &["displayName", "display_name", "name"]).unwrap_or_default();
        let Some(buckets) = array_property(group, "buckets") else {
            continue;
        };

        for bucket in buckets {
            // Protobuf JSON 省略缺省值；缺少 remainingFraction 代表未知，不是 0 或 1。
            let Some(fraction) = remaining_fraction(bucket) else {
                continue;
            };

            let bucket_label = get_string_any(
                bucket,
                &["displayName", "display_name", "name", "window", "bucketId", "bucket_id", "id"],
            )
            .unwrap_or_else(|| "額度".to_string());
            if !matches_pool(kind, &group_name, &bucket_label) {
                continue;
            }
            let duration = infer_antigravity_duration(bucket, &bucket_label);
            let resets_at = reset_time(bucket);
            windows.push(CliUsageWindow::new(
                format!("{}（{}）", pool_name(kind), bucket_label),
                used_percent_from_fraction(fraction),
                duration,
                resets_at,
                fraction < 1.0 && resets_at.is_some_and(|reset| reset > *observed_at),
            ));
        }
    }

    let windows = select_most_constrained_windows(windows);
    (!windows.is_empty()).then_some(windows)
}

fn parse_antigravity_model_configs_array(
    configs: &[Value],
    kind: CliKind,
    observed_at: &Moment,
) -> Vec<CliUsageWindow> {
    let mut candidates = Vec::new();
    for config in configs {
        let label = get_string_any(config, &["label", "displayName", "display_name"]).unwrap_or_default();
        let Some(quota_info) = object_property(config, "quotaInfo").filter(|_| matches_pool(kind, &label, "")) else {
            continue;
        };
        let Some(fraction) = remaining_fraction(quota_info) else {
            continue;
        };

        let duration = infer_antigravity_duration(config, &label).unwrap_or_else(|| Span::hours(5));
        let resets_at = reset_time(quota_info);
        candidates.push(CliUsageWindow::new(
            pool_name(kind),
            used_percent_from_fraction(fraction),
            Some(duration),
            resets_at,
            fraction < 1.0 && resets_at.is_some_and(|reset| reset > *observed_at),
        ));
    }
    select_most_constrained_windows(candidates)
}

fn is_argon_window(window: &CliUsageWindow) -> bool {
    window.name.to_ascii_lowercase().contains("argon")
}

/// 每種視窗長度只保留最吃緊的一筆（已用最多；相同時取最早重置者），並依長度由短到長排列（Argon 視窗獨立保留）。
fn select_most_constrained_windows(candidates: Vec<CliUsageWindow>) -> Vec<CliUsageWindow> {
    let mut selected: Vec<CliUsageWindow> = Vec::new();
    for candidate in candidates {
        let is_argon = is_argon_window(&candidate);
        match selected
            .iter_mut()
            .find(|window| is_argon_window(window) == is_argon && window.duration == candidate.duration)
        {
            Some(existing) => {
                let more_constrained = candidate.used_percent > existing.used_percent
                    || (candidate.used_percent == existing.used_percent
                        && match (candidate.resets_at, existing.resets_at) {
                            (Some(left), Some(right)) => left < right,
                            (Some(_), None) => true,
                            _ => false,
                        });
                if more_constrained {
                    *existing = candidate;
                }
            }
            None => selected.push(candidate),
        }
    }
    selected.sort_by(|a, b| {
        is_argon_window(a)
            .cmp(&is_argon_window(b))
            .then_with(|| a.duration.unwrap_or(Span::MAX).cmp(&b.duration.unwrap_or(Span::MAX)))
    });
    selected
}

fn antigravity_configs(root: &Value) -> Option<&Vec<Value>> {
    let mut candidates = vec![root];
    if let Some(response) = object_property(root, "response") {
        candidates.push(response);
        if let Some(user_status) = object_property(response, "userStatus") {
            candidates.push(user_status);
            if let Some(cascade) = object_property(user_status, "cascadeModelConfigData") {
                candidates.push(cascade);
            }
        }
    }
    if let Some(user_status) = object_property(root, "userStatus") {
        candidates.push(user_status);
        if let Some(cascade) = object_property(user_status, "cascadeModelConfigData") {
            candidates.push(cascade);
        }
    }
    candidates.into_iter().find_map(|candidate| array_property(candidate, "clientModelConfigs"))
}

fn object_property<'a>(element: &'a Value, name: &str) -> Option<&'a Value> {
    element.get(name).filter(|value| value.is_object())
}

fn array_property<'a>(element: &'a Value, name: &str) -> Option<&'a Vec<Value>> {
    element.get(name).and_then(Value::as_array)
}

fn get_string(element: &Value, name: &str) -> Option<String> {
    element.get(name).and_then(Value::as_str).map(str::to_string)
}

fn get_string_any(element: &Value, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| get_string(element, name))
}

fn number_any(element: &Value, names: &[&str]) -> Option<f64> {
    names.iter().find_map(|name| element.get(*name).and_then(Value::as_f64))
}

fn error_message(error: &Value) -> String {
    match error.get("message") {
        Some(Value::String(message)) => message.clone(),
        Some(other) => other.to_string(),
        None => error.to_string(),
    }
}

fn remaining_fraction(element: &Value) -> Option<f64> {
    const NAMES: [&str; 2] = ["remainingFraction", "remaining_fraction"];
    let in_range = |value: f64| (0.0..=1.0).contains(&value).then_some(value);
    if let Some(direct) = number_any(element, &NAMES) {
        return in_range(direct);
    }
    object_property(element, "remaining").and_then(|remaining| number_any(remaining, &NAMES)).and_then(in_range)
}

fn reset_time(element: &Value) -> Option<Moment> {
    for name in ["resetTime", "reset_time", "resetsAt", "resets_at"] {
        match element.get(name) {
            Some(Value::String(text)) => {
                if let Some(parsed) = time::parse_moment(text) {
                    return Some(parsed);
                }
            }
            Some(Value::Number(number)) => {
                if let Some(parsed) = number.as_i64().and_then(time::from_unix_seconds) {
                    return Some(parsed);
                }
            }
            _ => {}
        }
    }
    object_property(element, "remaining").and_then(reset_time)
}

fn infer_antigravity_duration(element: &Value, label: &str) -> Option<Span> {
    let window = get_string_any(element, &["window", "windowType", "window_type", "bucketId", "bucket_id"])
        .unwrap_or_else(|| label.to_string())
        .to_ascii_lowercase();
    if window.contains("5h") || window.contains("five") || window.contains("session") {
        return Some(Span::hours(5));
    }
    if window.contains("week") || window.contains("7d") {
        return Some(Span::days(7));
    }
    None
}

fn create_claude_rate_limited_snapshot(
    state: &ClaudeState,
    observed_at: &Moment,
    retry_after_at: &Moment,
) -> CliUsageSnapshot {
    let message = format!(
        "Claude 額度查詢暫時受限（HTTP 429），將於 {} 後再試。",
        time::to_local(retry_after_at).format("%H:%M:%S")
    );
    match &state.cached_snapshot {
        None => unavailable(CliKind::Claude, message, *observed_at),
        Some(cached) => refresh_claude_snapshot(cached, observed_at, &format!("{message}目前沿用最近一次成功資料。")),
    }
}

/// 以新的觀測時間更新快取的 Claude 快照；倒數已結束的視窗不再視為倒數中。
pub fn refresh_claude_snapshot(snapshot: &CliUsageSnapshot, observed_at: &Moment, message: &str) -> CliUsageSnapshot {
    let mut refreshed = snapshot.clone();
    for window in &mut refreshed.windows {
        window.is_active_countdown =
            window.is_active_countdown && window.resets_at.is_some_and(|resets_at| resets_at > *observed_at);
    }
    refreshed.message = message.to_string();
    refreshed.observed_at = *observed_at;
    refreshed
}

fn resolve_claude_retry_after(response: &HttpResponse, observed_at: &Moment) -> Moment {
    if let Some(value) = response.retry_after.as_deref().map(str::trim) {
        if let Ok(seconds) = value.parse::<i64>() {
            if seconds > 0 {
                return *observed_at + Span::seconds(seconds);
            }
        } else if let Ok(date) = chrono::DateTime::parse_from_rfc2822(value) {
            if date > *observed_at {
                return date;
            }
        }
    }
    *observed_at + Span::minutes(CLAUDE_DEFAULT_RATE_LIMIT_BACKOFF_MINUTES)
}

fn claude_credentials_path() -> Option<PathBuf> {
    if let Some(configured) = std::env::var("CLAUDE_CONFIG_DIR").ok().filter(|value| !value.trim().is_empty()) {
        return Some(PathBuf::from(win::expand_environment_variables(&configured)).join(".credentials.json"));
    }
    std::env::var("USERPROFILE")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(|profile| PathBuf::from(profile).join(".claude").join(".credentials.json"))
}

/// 只讀取 access token 欄位；本程式不持有也不寫入任何憑證。
fn read_claude_access_token(credentials_path: &Path) -> Result<Option<String>, String> {
    let text = std::fs::read_to_string(credentials_path).map_err(|error| error.to_string())?;
    let root: Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .map_err(|error| format!("Claude 額度 JSON 解析失敗：{error}"))?;
    Ok(object_property(&root, "claudeAiOauth").and_then(|oauth| get_string(oauth, "accessToken")))
}

/// 解析 Claude Code /api/oauth/usage 的 JSON 回應，供測試與協定更新驗證。
pub fn parse_claude_usage(json: &str, observed_at: &Moment) -> Result<CliUsageSnapshot, String> {
    let root: Value = serde_json::from_str(json).map_err(|error| error.to_string())?;

    if let Some(error) = root.get("error") {
        let message = if error.is_object() { error_message(error) } else { error.to_string() };
        return Ok(unavailable(CliKind::Claude, format!("Claude 額度查詢失敗：{message}"), *observed_at));
    }

    let mut windows = Vec::new();
    for (property, display_name, duration) in [
        ("five_hour", "Claude（5 小時）", Span::hours(5)),
        ("seven_day", "Claude（7 天）", Span::days(7)),
        ("seven_day_opus", "Claude Opus（7 天）", Span::days(7)),
        ("seven_day_sonnet", "Claude Sonnet（7 天）", Span::days(7)),
        ("seven_day_oauth_apps", "Claude OAuth Apps（7 天）", Span::days(7)),
        ("seven_day_cowork", "Claude Cowork（7 天）", Span::days(7)),
    ] {
        let Some(window) = object_property(&root, property) else {
            continue;
        };
        let Some(utilization) = window.get("utilization").and_then(Value::as_f64) else {
            continue;
        };
        let resets_at = window.get("resets_at").and_then(Value::as_str).and_then(time::parse_moment);
        let used_percent = (utilization.round() as i32).clamp(0, 100);
        let is_active_countdown = utilization > 0.0 && resets_at.is_some_and(|reset| reset > *observed_at);
        windows.push(CliUsageWindow::new(display_name, used_percent, Some(duration), resets_at, is_active_countdown));
    }

    Ok(if windows.is_empty() {
        unavailable(CliKind::Claude, "Claude 已回應，但目前沒有可顯示的額度視窗。", *observed_at)
    } else {
        CliUsageSnapshot::new(CliKind::Claude, CliUsageAvailability::Available, windows, "讀取成功", *observed_at)
    })
}

fn read_codex(profile: &CliProfile, working_directory: &str, cancel: &CancelToken) -> CoreResult<CliUsageSnapshot> {
    let observed_at = time::now();
    match query_codex_rate_limits(profile, working_directory, cancel) {
        Ok(response) => Ok(parse_codex_rate_limits(&response, &observed_at)
            .unwrap_or_else(|message| unavailable(CliKind::Codex, message, observed_at))),
        Err(CoreError::Canceled) => Err(CoreError::Canceled),
        Err(error) => Ok(unavailable(CliKind::Codex, error.message(), observed_at)),
    }
}

fn query_codex_rate_limits(profile: &CliProfile, working_directory: &str, cancel: &CancelToken) -> CoreResult<String> {
    if !Path::new(working_directory).is_dir() {
        return Err(CoreError::DirectoryNotFound(format!("工作目錄不存在：{working_directory}")));
    }
    let executable = locator::resolve(CliKind::Codex, Some(&profile.executable), working_directory)
        .ok_or_else(|| CoreError::FileNotFound("找不到 Codex CLI，請先在 CLI 設定指定正確路徑。".to_string()))?;

    let (mut command, _) =
        process::build_command(&executable, &["app-server", "--listen", "stdio://"], working_directory);
    command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child =
        command.spawn().map_err(|error| CoreError::InvalidOperation(format!("無法啟動 Codex app-server：{error}")))?;
    let process_id = child.id();

    // 持續排空 stderr，避免子程序寫滿管線時卡住。
    if let Some(mut stderr) = child.stderr.take() {
        std::thread::spawn(move || {
            let mut sink = [0u8; 2048];
            while matches!(stderr.read(&mut sink), Ok(read) if read > 0) {}
        });
    }

    let (sender, receiver) = mpsc::channel::<String>();
    if let Some(stdout) = child.stdout.take() {
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else {
                    break;
                };
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
    }

    let deadline = Instant::now() + QUERY_TIMEOUT;
    let read_response = |response_id: i64| -> CoreResult<String> {
        loop {
            if cancel.is_canceled() {
                return Err(CoreError::Canceled);
            }
            if Instant::now() >= deadline {
                return Err(CoreError::InvalidOperation("讀取 Codex 額度逾時。".to_string()));
            }
            match receiver.recv_timeout(Duration::from_millis(50)) {
                Ok(line) => {
                    let id = serde_json::from_str::<Value>(&line)
                        .ok()
                        .and_then(|value| value.get("id").and_then(Value::as_i64));
                    if id == Some(response_id) {
                        return Ok(line);
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(CoreError::InvalidOperation(format!(
                        "Codex app-server 在回傳 id={response_id} 前結束。"
                    )));
                }
            }
        }
    };

    let outcome = (|| -> CoreResult<String> {
        let mut stdin =
            child.stdin.take().ok_or_else(|| CoreError::InvalidOperation("無法啟動 Codex app-server。".to_string()))?;

        // JSONL 第一個位元組不可有 UTF-8 BOM；否則 initialize 會被視為無效訊息。
        stdin.write_all(
            b"{\"method\":\"initialize\",\"id\":0,\"params\":{\"clientInfo\":{\"name\":\"ai_wake_scheduler\",\"title\":\"AI Wake Scheduler\",\"version\":\"1.10.0\"}}}\n",
        )?;
        stdin.flush()?;

        // app-server 明確要求先完成 initialize 握手。若三行一次送出，
        // 伺服器在負載下可能先處理額度要求並回覆 Not initialized。
        let initialize_response = read_response(0)?;
        if let Some(error) =
            serde_json::from_str::<Value>(&initialize_response).ok().as_ref().and_then(|value| value.get("error"))
        {
            return Err(CoreError::InvalidOperation(format!("Codex app-server 初始化失敗：{}", error_message(error))));
        }

        stdin.write_all(b"{\"method\":\"initialized\",\"params\":{}}\n")?;
        stdin.write_all(b"{\"method\":\"account/rateLimits/read\",\"id\":1}\n")?;
        stdin.flush()?;
        read_response(1)
    })();

    win::kill_process_tree(process_id);
    let _ = child.wait();
    outcome
}

/// 解析 account/rateLimits/read 的單行 JSON 回應，供測試與協定更新驗證。
pub fn parse_codex_rate_limits(json: &str, observed_at: &Moment) -> Result<CliUsageSnapshot, String> {
    let root: Value = serde_json::from_str(json).map_err(|error| error.to_string())?;

    if let Some(error) = root.get("error") {
        return Ok(unavailable(CliKind::Codex, format!("Codex 額度查詢失敗：{}", error_message(error)), *observed_at));
    }

    let Some(result) = root.get("result") else {
        return Ok(unavailable(CliKind::Codex, "Codex 回應缺少 result。", *observed_at));
    };

    let mut windows = Vec::new();
    if let Some(by_id) = result.get("rateLimitsByLimitId").and_then(Value::as_object) {
        for (name, bucket) in by_id {
            add_bucket_windows(&mut windows, bucket, name, observed_at);
        }
    }

    if windows.is_empty() {
        if let Some(legacy) = object_property(result, "rateLimits") {
            add_bucket_windows(&mut windows, legacy, "Codex", observed_at);
        }
    }

    Ok(if windows.is_empty() {
        unavailable(CliKind::Codex, "Codex 已回應，但目前沒有可顯示的額度視窗。", *observed_at)
    } else {
        CliUsageSnapshot::new(CliKind::Codex, CliUsageAvailability::Available, windows, "讀取成功", *observed_at)
    })
}

fn add_bucket_windows(
    destination: &mut Vec<CliUsageWindow>,
    bucket: &Value,
    fallback_name: &str,
    observed_at: &Moment,
) {
    let bucket_name = get_string(bucket, "limitName")
        .or_else(|| get_string(bucket, "limitId"))
        .unwrap_or_else(|| fallback_name.to_string());
    add_window(destination, bucket, "primary", bucket_name.clone(), observed_at);
    add_window(destination, bucket, "secondary", format!("{bucket_name}（次要）"), observed_at);
}

fn add_window(
    destination: &mut Vec<CliUsageWindow>,
    bucket: &Value,
    property: &str,
    name: String,
    observed_at: &Moment,
) {
    let Some(window) = object_property(bucket, property) else {
        return;
    };
    let Some(used_percent) =
        window.get("usedPercent").and_then(Value::as_i64).and_then(|value| i32::try_from(value).ok())
    else {
        return;
    };

    let duration = window.get("windowDurationMins").and_then(Value::as_i64).map(Span::minutes);
    // 協定若回傳無效時間，仍可顯示剩餘百分比。
    let resets_at = window.get("resetsAt").and_then(Value::as_i64).and_then(time::from_unix_seconds);

    let clamped_used = used_percent.clamp(0, 100);
    let is_active_countdown = clamped_used > 0 && resets_at.is_some_and(|reset| reset > *observed_at);
    destination.push(CliUsageWindow::new(name, clamped_used, duration, resets_at, is_active_countdown));
}

fn unavailable(kind: CliKind, message: impl Into<String>, observed_at: Moment) -> CliUsageSnapshot {
    CliUsageSnapshot::new(kind, CliUsageAvailability::Unavailable, Vec::new(), message, observed_at)
}

fn login_required(kind: CliKind, message: impl Into<String>, observed_at: Moment) -> CliUsageSnapshot {
    CliUsageSnapshot { requires_login: true, ..unavailable(kind, message, observed_at) }
}
