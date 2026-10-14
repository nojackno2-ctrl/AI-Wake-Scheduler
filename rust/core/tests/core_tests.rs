//! 可重現的 deterministic 測試（不需要本機 CLI 或登入狀態）。
//! 標示 `#[ignore]` 的測試需要本機已安裝並登入的 CLI，以 `cargo test -- --ignored` 明確執行。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use aiwake_core::calculator;
use aiwake_core::catalog;
use aiwake_core::command_builder;
use aiwake_core::locator;
use aiwake_core::login;
use aiwake_core::process;
use aiwake_core::time::{self, local_wall_clock};
use aiwake_core::tokenizer;
use aiwake_core::usage::{self, is_counting_down};
use aiwake_core::*;
use chrono::{Datelike, NaiveDate, NaiveDateTime, Timelike};

/// 多個測試依賴真實經過時間與行程層級的狀態，逐一執行才不會互相干擾。
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join("AiWakeSchedulerRustTests").join(Guid::new().simple());
        std::fs::create_dir_all(&path).unwrap();
        TempDir(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn text(&self) -> String {
        self.0.to_string_lossy().into_owned()
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fake_profile(output_path: &Path, delay_milliseconds: u64, exit_code: i32) -> CliProfile {
    CliProfile {
        executable: env!("CARGO_BIN_EXE_aiwake-fake-cli").to_string(),
        additional_arguments: format!(
            "--fake-cli --fake-delay-ms {delay_milliseconds} --fake-exit-code {exit_code} --fake-cli-output \"{}\"",
            output_path.display()
        ),
        ..CliProfile::default()
    }
}

fn naive(year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(year, month, day).unwrap().and_hms_opt(hour, minute, second).unwrap()
}

fn hm(hours: i64, minutes: i64) -> Span {
    Span::hours(hours) + Span::minutes(minutes)
}

fn wait_until(timeout: Duration, condition: impl Fn() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if condition() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    condition()
}

fn sleep(milliseconds: u64) {
    std::thread::sleep(Duration::from_millis(milliseconds));
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

fn job_store(path: PathBuf) -> Arc<JsonFileStore<Vec<ScheduledJob>>> {
    Arc::new(JsonFileStore::new(path, Vec::new))
}

type SharedSettings = Arc<Mutex<AppSettings>>;

fn shared_settings() -> SharedSettings {
    Arc::new(Mutex::new(AppSettings::create_default()))
}

fn manager_with(
    store: &Arc<JsonFileStore<Vec<ScheduledJob>>>,
    runner: Arc<dyn CliRunner>,
    settings: &SharedSettings,
    usage_reader: Option<Arc<dyn UsageReader>>,
    clock: Option<Arc<dyn Clock>>,
) -> ScheduleManager {
    let settings = Arc::clone(settings);
    ScheduleManager::new(store.clone(), runner, move || settings.lock().unwrap().clone(), usage_reader, clock)
}

/// 假的 CLI 執行器。ScheduleManager 只依賴 CliRunner，
/// 因此排程邏輯可以完全不啟動任何子程序就驗證。
#[derive(Default)]
struct CountingCliRunner {
    call_count: AtomicUsize,
    executed_clis: Mutex<Vec<CliKind>>,
    last_token_saver_mode: AtomicBool,
}

impl CountingCliRunner {
    fn call_count(&self) -> usize {
        self.call_count.load(Ordering::SeqCst)
    }

    fn executed(&self) -> Vec<CliKind> {
        self.executed_clis.lock().unwrap().clone()
    }
}

impl CliRunner for CountingCliRunner {
    fn run(
        &self,
        kind: CliKind,
        _profile: &CliProfile,
        _message: &str,
        _working_directory: &str,
        _timeout: Duration,
        token_saver_mode: bool,
        _cancel: &CancelToken,
        _on_output: Option<&dyn Fn(bool, &str)>,
    ) -> CliRunResult {
        self.executed_clis.lock().unwrap().push(kind);
        self.call_count.fetch_add(1, Ordering::SeqCst);
        self.last_token_saver_mode.store(token_saver_mode, Ordering::SeqCst);
        let mut result = CliRunResult::new(kind);
        result.succeeded = true;
        result.exit_code = Some(0);
        result.started_at = time::now();
        result.finished_at = time::now();
        result
    }

    fn probe(&self, kind: CliKind, _: &CliProfile, _: &str, _: &CancelToken) -> CliProbeResult {
        CliProbeResult { cli: kind, succeeded: true, summary: "fake".to_string(), executable_path: String::new() }
    }
}

#[derive(Default)]
struct FakeCliUsageReader {
    on_read: Mutex<Option<Box<dyn Fn() + Send + Sync>>>,
    snapshots: Mutex<HashMap<CliKind, CliUsageSnapshot>>,
    latest: Mutex<HashMap<CliKind, CliUsageSnapshot>>,
    read_counts: Mutex<HashMap<CliKind, usize>>,
}

impl FakeCliUsageReader {
    fn set(&self, snapshot: CliUsageSnapshot) {
        self.snapshots.lock().unwrap().insert(snapshot.cli, snapshot);
    }

    fn read_count(&self, kind: CliKind) -> usize {
        self.read_counts.lock().unwrap().get(&kind).copied().unwrap_or(0)
    }
}

impl UsageReader for FakeCliUsageReader {
    fn read(&self, kind: CliKind, _: &CliProfile, _: &str, _: &CancelToken) -> CoreResult<CliUsageSnapshot> {
        if let Some(callback) = self.on_read.lock().unwrap().as_ref() {
            callback();
        }
        *self.read_counts.lock().unwrap().entry(kind).or_insert(0) += 1;

        let result = self.snapshots.lock().unwrap().get(&kind).cloned().unwrap_or_else(|| {
            CliUsageSnapshot::new(kind, CliUsageAvailability::Unavailable, Vec::new(), "No fake data", time::now())
        });
        self.latest.lock().unwrap().insert(kind, result.clone());
        Ok(result)
    }

    fn latest_snapshot(&self, kind: CliKind) -> Option<CliUsageSnapshot> {
        self.latest.lock().unwrap().get(&kind).cloned()
    }

    fn latest_snapshots(&self) -> HashMap<CliKind, CliUsageSnapshot> {
        self.latest.lock().unwrap().clone()
    }
}

struct FakeClock(Mutex<Moment>);

impl FakeClock {
    fn new(initial: Moment) -> Arc<Self> {
        Arc::new(FakeClock(Mutex::new(initial)))
    }

    fn set(&self, now: Moment) {
        *self.0.lock().unwrap() = now;
    }

    fn get(&self) -> Moment {
        *self.0.lock().unwrap()
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Moment {
        time::to_local(&self.get())
    }
}

fn window(name: &str, used: i32, duration: Span, resets_at: Moment, active: bool) -> CliUsageWindow {
    CliUsageWindow::new(name, used, Some(duration), Some(resets_at), active)
}

fn available(kind: CliKind, windows: Vec<CliUsageWindow>, observed_at: Moment) -> CliUsageSnapshot {
    CliUsageSnapshot::new(kind, CliUsageAvailability::Available, windows, "讀取成功", observed_at)
}

fn job(name: &str, directory: &TempDir, targets: &[CliKind]) -> ScheduledJob {
    ScheduledJob {
        name: name.to_string(),
        message: "早安".to_string(),
        working_directory: directory.text(),
        targets: targets.to_vec(),
        ..ScheduledJob::default()
    }
}

#[test]
fn argument_tokenizer() {
    assert_eq!(
        strings(&["--model", "fast model", "--flag"]),
        tokenizer::parse(Some("--model \"fast model\" --flag")).unwrap()
    );
    assert_eq!(
        strings(&["a\\b", "quoted \"value\""]),
        tokenizer::parse(Some("a\\b \"quoted \\\"value\\\"\"")).unwrap()
    );
    assert!(tokenizer::parse(Some("")).unwrap().is_empty());
    assert!(tokenizer::parse(Some("   \t ")).unwrap().is_empty());
    assert!(tokenizer::parse(None).unwrap().is_empty());
    assert_eq!(strings(&["a", "b", "c"]), tokenizer::parse(Some("a   b \t c")).unwrap());
    assert!(matches!(tokenizer::parse(Some("\"unfinished")), Err(CoreError::Format(_))));
}

fn build_args(kind: CliKind, arguments: Option<&str>, saver: bool, timeout: Option<Duration>) -> Vec<String> {
    command_builder::build_with_arguments(kind, "早安", arguments, saver, timeout).unwrap()
}

fn build_profile(
    kind: CliKind,
    message: &str,
    model: &str,
    effort: ThinkingEffort,
    extra: &str,
    saver: bool,
) -> Vec<String> {
    let profile = CliProfile {
        model: model.to_string(),
        thinking_effort: effort,
        additional_arguments: extra.to_string(),
        ..CliProfile::default()
    };
    command_builder::build_for_profile(kind, message, Some(&profile), saver, None).unwrap()
}

fn has(arguments: &[String], value: &str) -> bool {
    arguments.iter().any(|argument| argument == value)
}

#[test]
fn cli_command_builder() {
    use ThinkingEffort as E;

    assert_eq!(
        strings(&["--model", "gemini-3.8-flash", "--print", "早安"]),
        build_args(CliKind::Antigravity, None, false, None)
    );
    assert_eq!(
        strings(&["--model", "claude-sonnet-5-5-low", "--print", "早安"]),
        build_args(CliKind::AntigravityClaude, None, false, None)
    );
    assert_eq!(
        strings(&["--model", "custom-model", "--print", "早安"]),
        build_args(CliKind::AntigravityClaude, Some("--model custom-model"), false, None)
    );
    assert_eq!(
        strings(&["exec", "--skip-git-repo-check", "--ephemeral", "--color", "never", "--model", "gpt-6-luna", "早安"]),
        build_args(CliKind::Codex, None, false, None)
    );
    assert_eq!(
        strings(&["--print", "--model", "sonnet", "早安"]),
        build_args(CliKind::Claude, Some("--model sonnet"), false, None)
    );

    // 自訂 Model 與 ThinkingEffort 測試（模型 ID 與 effort 合法值均以實際 CLI 行為核對）
    assert_eq!(
        strings(&["--model", "gemini-3.8-flash", "--effort", "medium", "--print", "早安"]),
        build_profile(CliKind::Antigravity, "早安", "gemini-3.8-flash", E::Medium, "", false)
    );
    assert_eq!(
        strings(&["--model", "gemini-3.8-flash-high", "--print", "早安"]),
        build_profile(CliKind::Antigravity, "早安", "gemini-3.8-flash-high", E::Low, "", false)
    );
    assert_eq!(
        strings(&["--model", "gemini-3.7-flash", "--effort", "high", "--print", "早安"]),
        build_profile(CliKind::Antigravity, "早安", "gemini-3.7-flash", E::High, "", false)
    );
    assert_eq!(
        strings(&[
            "exec",
            "--skip-git-repo-check",
            "--ephemeral",
            "--color",
            "never",
            "--model",
            "gpt-5.6-sol",
            "-c",
            "model_reasoning_effort=\"ultra\"",
            "早安"
        ]),
        build_profile(CliKind::Codex, "早安", "gpt-5.6-sol", E::Ultra, "", false)
    );
    assert_eq!(
        strings(&[
            "exec",
            "--skip-git-repo-check",
            "--ephemeral",
            "--color",
            "never",
            "--model",
            "gpt-6-luna",
            "-c",
            "model_reasoning_effort=\"low\"",
            "早安"
        ]),
        build_profile(CliKind::Codex, "早安", "", E::Minimal, "", false)
    );

    let codex_luna_ultra = build_profile(CliKind::Codex, "早安", "gpt-5.6-luna", E::Ultra, "", false);
    assert!(has(&codex_luna_ultra, "model_reasoning_effort=\"max\""), "GPT-5.6 Luna 的 Ultra 應降為支援的 Max。");
    let codex55_max = build_profile(CliKind::Codex, "早安", "gpt-5.5", E::Max, "", false);
    assert!(has(&codex55_max, "model_reasoning_effort=\"xhigh\""), "GPT-5.5 的 Max 應降為支援的 XHigh。");

    assert_eq!(
        strings(&["--print", "--model", "opus", "--effort", "medium", "早安"]),
        build_profile(CliKind::Claude, "早安", "opus", E::Medium, "", false)
    );

    // AntigravityClaude 使用含程度後綴的完整 ID，不另外加 --effort。
    let agy_claude_any_model =
        build_profile(CliKind::AntigravityClaude, "早安", "gpt-oss-120b-medium", E::High, "", false);
    assert!(!has(&agy_claude_any_model, "--effort"), "AntigravityClaude 的任何模型都不可帶 --effort。");

    // 迴歸測試：Antigravity 的旗標絕不能出現在 --print 之後，
    // 否則會被當成提示詞送出，其餘旗標則全部失效。
    for agy_kind in [CliKind::Antigravity, CliKind::AntigravityClaude] {
        let built = build_args(agy_kind, Some("--add-dir C:\\tmp"), true, Some(Duration::from_secs(120)));
        let print_index = built.iter().position(|argument| argument == "--print").unwrap();
        assert_eq!(print_index, built.len() - 2, "{agy_kind}：--print 必須是倒數第二個參數。");
        assert!(built.last().unwrap().starts_with("早安"), "{agy_kind}：提示詞必須是 --print 的值。");
        assert!(
            built[..print_index].iter().all(|argument| argument != "早安"),
            "{agy_kind}：提示詞不應出現在旗標之間。"
        );
    }

    // 使用者以 -m 指定模型時，也不應再附加預設模型
    let short_model_override = build_args(CliKind::AntigravityClaude, Some("-m custom"), false, None);
    assert!(!has(&short_model_override, "claude-sonnet-5-5-low"), "使用者以 -m 指定模型時不應覆寫。");

    // 使用者以額外參數指定思考程度時，不應重複附加
    let codex_effort_override =
        build_profile(CliKind::Codex, "早安", "", E::Low, "-c model_reasoning_effort=\"high\"", false);
    let occurrences =
        codex_effort_override.iter().filter(|a| a.to_lowercase().contains("model_reasoning_effort")).count();
    assert_eq!(occurrences, 1, "使用者在額外參數自訂 model_reasoning_effort 時不應重複附加。");

    let codex_model_override = build_profile(CliKind::Codex, "早安", "gpt-5.6-sol", E::Ultra, "--model gpt-5.5", false);
    assert_eq!(
        codex_model_override.iter().filter(|a| *a == "--model").count(),
        1,
        "額外參數覆寫模型時不應重複附加 --model。"
    );
    assert!(
        has(&codex_model_override, "model_reasoning_effort=\"xhigh\""),
        "Effort 應依額外參數實際指定的 GPT-5.5 正規化為 XHigh。"
    );

    let saver_agy = build_args(CliKind::Antigravity, None, true, Some(Duration::from_secs(180)));
    assert!(has(&saver_agy, "--effort") && has(&saver_agy, "low"), "Antigravity 節省模式應包含 low effort。");
    assert!(has(&saver_agy, "--disable-slash-commands"), "Antigravity 節省模式應停用斜線指令。");
    assert!(has(&saver_agy, "--mode") && has(&saver_agy, "plan"), "Antigravity 節省模式不可修改工作區。");
    assert!(
        has(&saver_agy, "--print-timeout") && has(&saver_agy, "180s"),
        "應把應用程式逾時轉為 CLI 自身的 --print-timeout。"
    );

    let saver_agy_claude = build_args(CliKind::AntigravityClaude, None, true, None);
    assert!(
        has(&saver_agy_claude, "--model") && has(&saver_agy_claude, "claude-sonnet-5-5-low"),
        "AntigravityClaude 預設應使用 Claude Sonnet 模型 ID。"
    );
    assert!(has(&saver_agy_claude, "--disable-slash-commands"), "AntigravityClaude 節省模式應停用技能展開。");
    assert!(
        has(&saver_agy_claude, "--mode") && has(&saver_agy_claude, "plan"),
        "AntigravityClaude 節省模式不可修改工作區。"
    );
    // Claude 系列模型不接受 --effort，帶上去 agy 會直接拒絕整個呼叫
    assert!(!has(&saver_agy_claude, "--effort"), "AntigravityClaude 不可帶 --effort，Claude 模型不支援。");
    assert!(!has(&saver_agy_claude, "--print-timeout"), "未指定逾時時不應產生 --print-timeout。");

    let saver_codex = build_args(CliKind::Codex, None, true, None);
    assert!(has(&saver_codex, "read-only"), "節省 Token 模式應使用 Codex 唯讀沙箱。");
    assert!(has(&saver_codex, "model_reasoning_effort=\"low\""), "節省 Token 模式應降低 Codex 推理量。");
    assert!(
        has(&saver_codex, "--ignore-user-config"),
        "節省 Token 模式應略過 config.toml，避免載入 MCP 伺服器與自訂指示。"
    );
    assert!(saver_codex.last().unwrap().contains("只回「OK」"), "節省模式應要求最短回覆。");

    let saver_claude = build_args(CliKind::Claude, None, true, None);
    assert!(
        has(&saver_claude, "claude-haiku-4-5-20251001") && !has(&saver_claude, "--effort"),
        "Claude 預設應使用低價 Haiku 並省略不支援的 effort。"
    );
    assert!(has(&saver_claude, "--tools") && has(&saver_claude, ""), "節省模式應停用 Claude 內建工具。");
    assert!(has(&saver_claude, "--safe-mode"), "節省模式應停用 CLAUDE.md、技能、外掛與 MCP 伺服器。");
    assert!(has(&saver_claude, "--strict-mcp-config"), "節省模式應確保不載入任何 MCP 工具結構描述。");
    assert!(has(&saver_claude, "--no-session-persistence"), "節省模式不應保存一次性喚醒 session。");
    let suggestions_index = saver_claude.iter().position(|a| a == "--prompt-suggestions").unwrap();
    assert_eq!(saver_claude[suggestions_index + 1], "false", "節省模式應停用額外的提示建議生成。");

    // 節省模式下若使用者明確指定了 ThinkingEffort.High，應尊重使用者設定而非強行覆蓋為 low
    let saver_codex_high = build_profile(CliKind::Codex, "早安", "", E::High, "", true);
    assert!(has(&saver_codex_high, "model_reasoning_effort=\"high\""), "使用者指定 High 時應使用 high。");
    assert!(!has(&saver_codex_high, "model_reasoning_effort=\"low\""), "使用者指定 High 時不應包含 low。");

    // --tools 是可變長度參數，後面必須緊接旗標，否則會吃掉提示詞
    let tools_index = saver_claude.iter().position(|a| a == "--tools").unwrap();
    assert!(saver_claude[tools_index + 1].is_empty(), "--tools 之後應緊接空字串。");
    assert!(saver_claude[tools_index + 2].starts_with("--"), "--tools 的值之後必須是旗標，避免吞掉位置參數。");
}

#[test]
fn cli_catalog() {
    use ThinkingEffort as E;

    for &kind in CliKind::ALL {
        let descriptor = catalog::get(kind);
        assert_eq!(descriptor.kind, kind, "描述應對應到正確的 CLI 種類。");
        assert!(!descriptor.display_name.trim().is_empty(), "每個 CLI 都應有顯示名稱。");
        assert!(!descriptor.short_name.trim().is_empty(), "每個 CLI 都應有短名稱。");
        assert!(!descriptor.default_command.trim().is_empty(), "每個 CLI 都應有預設命令。");
        assert_eq!(kind.display_name(), descriptor.display_name, "顯示名稱應由目錄提供。");
        assert_eq!(kind.short_name(), descriptor.short_name, "短名稱應由目錄提供。");
        assert!(descriptor.preset_models.len() >= 3, "每個 CLI 應至少提供最新 3 款推薦模型選項。");
        assert!(descriptor.supported_efforts.contains(&E::Default), "每個 CLI 都應允許使用 CLI 自身預設思考程度。");
    }

    // AntigravityClaude 的程度已在完整模型 ID 中，此處僅提供預設。
    assert_eq!(
        catalog::get(CliKind::AntigravityClaude).supported_efforts.len(),
        1,
        "AntigravityClaude 不應提供無法生效的思考程度選項。"
    );

    let codex = catalog::get(CliKind::Codex);
    assert_eq!(
        codex.preset_models,
        [
            "",
            "gpt-6.1-sol",
            "gpt-6-astra",
            "gpt-6-sol",
            "gpt-6-luna",
            "gpt-5.6-sol",
            "gpt-5.6-terra",
            "gpt-5.6-luna",
            "gpt-5.5"
        ]
    );
    assert!(
        !codex.preset_models.iter().any(|model| model.contains("gpt-5.2") || model.contains("gpt-5.1")),
        "Codex 推薦清單不應再包含 deprecated 的 GPT-5.2/5.1 Codex 模型。"
    );
    assert!(codex.get_supported_efforts(Some("gpt-5.6-sol")).contains(&E::Ultra), "GPT-5.6 Sol 應支援 Ultra。");
    assert!(!codex.get_supported_efforts(Some("gpt-5.6-luna")).contains(&E::Ultra), "GPT-5.6 Luna 不支援 Ultra。");
    assert_eq!(codex.normalize_effort(Some("gpt-5.5"), E::Max), E::XHigh, "GPT-5.5 Max 應正規化為 XHigh。");
    for model in ["gpt-6.1-sol", "gpt-6-astra", "gpt-6-sol"] {
        let built = build_profile(CliKind::Codex, "OK", model, E::Ultra, "", false);
        assert!(has(&built, model) && has(&built, "model_reasoning_effort=\"ultra\""), "新 Sol/Astra 應可使用 Ultra。");
    }
    let luna = build_profile(CliKind::Codex, "OK", "gpt-6-luna", E::Ultra, "", false);
    assert!(has(&luna, "model_reasoning_effort=\"max\""), "GPT-6 Luna 的 Ultra 應降為 Max。");

    let agy_claude = catalog::get(CliKind::AntigravityClaude);
    assert_eq!(agy_claude.default_model, Some("claude-sonnet-5-5-low"), "AGY Claude 預設應使用目前的 Sonnet 5.5 Low。");
    let mut legacy_settings = AppSettings::create_default();
    legacy_settings.cli_profiles.get_mut(&CliKind::AntigravityClaude).unwrap().model =
        "claude-opus-4-6-thinking".to_string();
    legacy_settings.cli_profiles.get_mut(&CliKind::Codex).unwrap().model = "gpt-5.4-mini".to_string();
    legacy_settings.cli_profiles.get_mut(&CliKind::Claude).unwrap().model = "custom-model".to_string();
    legacy_settings.ensure_defaults();
    assert_eq!(
        legacy_settings.cli_profiles[&CliKind::AntigravityClaude].model,
        "claude-opus-5-5-high",
        "已移除 Opus 設定應轉換到新完整 ID。"
    );
    assert_eq!(legacy_settings.cli_profiles[&CliKind::Codex].model, "gpt-6-luna", "已移除 mini 設定應轉換到 Luna。");
    assert_eq!(legacy_settings.cli_profiles[&CliKind::Claude].model, "custom-model", "自訂模型不可被轉換。");
    for model in agy_claude.preset_models {
        let built = build_profile(CliKind::AntigravityClaude, "OK", model, E::High, "", true);
        assert!(has(&built, model) && !has(&built, "--effort"), "AGY 完整 ID 不應再加 --effort。");
    }
    for model in ["claude-sonnet-5-5", "claude-opus-5-5", "claude-fable-5-1"] {
        assert!(catalog::get(CliKind::Claude).preset_models.contains(&model), "Claude 應提供目前的固定版本模型。");
    }

    let agy = catalog::get(CliKind::Antigravity);
    assert!(agy.preset_models.contains(&"gemini-3.8-flash"), "Antigravity 推薦清單應包含 gemini-3.8-flash。");
    assert!(agy.get_supported_efforts(Some("gemini-3.8-flash")).contains(&E::High), "gemini-3.8-flash 應支援 High。");
    assert_eq!(
        agy.get_supported_efforts(Some("gemini-3.8-flash-high")),
        [E::Default],
        "gemini-3.8-flash-high 應抑制額外 --effort 避免衝突。"
    );

    for kind in [CliKind::Antigravity, CliKind::Codex, CliKind::Claude] {
        assert!(catalog::get(kind).supported_efforts.len() >= 3, "{kind} 應至少支援 3 種思考程度選項。");
    }
    assert_eq!(catalog::all().len(), CliKind::ALL.len(), "目錄應涵蓋所有 CliKind。");
}

#[test]
fn cli_usage_reader() {
    let observed_at = chrono::DateTime::parse_from_rfc3339("2026-08-12T08:00:00+08:00").unwrap();
    let primary_reset = (observed_at + Span::hours(2)).timestamp();
    let secondary_reset = (observed_at + Span::days(6)).timestamp();
    let response = format!(
        r#"{{
          "id": 1,
          "result": {{
            "rateLimits": {{}},
            "rateLimitsByLimitId": {{
              "codex": {{
                "limitId": "codex",
                "primary": {{ "usedPercent": 7, "windowDurationMins": 300, "resetsAt": {primary_reset} }},
                "secondary": {{ "usedPercent": 35, "windowDurationMins": 10080, "resetsAt": {secondary_reset} }}
              }}
            }}
          }}
        }}"#
    );

    let snapshot = usage::parse_codex_rate_limits(&response, &observed_at).unwrap();
    assert_eq!(snapshot.availability, CliUsageAvailability::Available, "Codex rate-limit 回應應可解析。");
    assert_eq!(snapshot.windows.len(), 2, "主要與次要額度視窗都應保留。");
    assert_eq!(snapshot.windows[0].remaining_percent(), 93, "剩餘百分比應為 100 減 usedPercent。");
    assert_eq!(snapshot.windows[0].resets_at.unwrap().timestamp(), primary_reset, "應解析 Unix 重置時間。");
    assert_eq!(snapshot.windows[1].duration, Some(Span::days(7)), "應解析額度視窗長度。");

    let error =
        usage::parse_codex_rate_limits("{\"id\":1,\"error\":{\"message\":\"not authenticated\"}}", &observed_at)
            .unwrap();
    assert_eq!(error.availability, CliUsageAvailability::Unavailable, "伺服器錯誤不可標示為可用額度。");

    let claude_response = r#"{
          "five_hour": { "utilization": 7.4, "resets_at": "2026-08-12T10:30:00+08:00" },
          "seven_day": { "utilization": 26.0, "resets_at": "2026-08-18T05:59:59+08:00" },
          "seven_day_opus": null,
          "extra_usage": { "is_enabled": false }
        }"#;
    let claude_snapshot = usage::parse_claude_usage(claude_response, &observed_at).unwrap();
    assert_eq!(claude_snapshot.availability, CliUsageAvailability::Available, "Claude OAuth usage 回應應可解析。");
    assert_eq!(claude_snapshot.windows.len(), 2, "Claude 五小時與七天額度視窗都應保留。");
    assert_eq!(claude_snapshot.windows[0].used_percent, 7, "Claude utilization 應四捨五入為已使用百分比。");
    assert_eq!(claude_snapshot.windows[0].remaining_percent(), 93, "Claude 剩餘百分比應為 100 減 utilization。");
    assert_eq!(claude_snapshot.windows[0].duration, Some(Span::hours(5)), "Claude 主要額度應標示五小時視窗。");
    assert_eq!(
        claude_snapshot.windows[0].resets_at.unwrap().offset().local_minus_utc(),
        8 * 3600,
        "Claude 重置時間應保留原始時區。"
    );
    assert_eq!(claude_snapshot.windows[1].duration, Some(Span::days(7)), "Claude 次要額度應標示七天視窗。");

    let claude_error =
        usage::parse_claude_usage("{\"error\":{\"message\":\"invalid bearer token\"}}", &observed_at).unwrap();
    assert_eq!(claude_error.availability, CliUsageAvailability::Unavailable, "Claude 錯誤回應不可標示為可用額度。");

    // Antigravity (Gemini 與 Claude/GPT 雙額度池解析測試)
    let agy_response = r#"{
          "clientModelConfigs": [
            { "label": "Gemini 3.7 Flash (High)",
              "quotaInfo": { "remainingFraction": 0.9006, "resetTime": "2026-08-18T05:25:50Z" } },
            { "label": "Claude Sonnet 4.6 (Thinking)",
              "quotaInfo": { "remainingFraction": 1.0, "resetTime": "2026-08-18T09:09:25Z" } }
          ]
        }"#;

    let agy_gemini = usage::parse_antigravity_model_configs(agy_response, CliKind::Antigravity, &observed_at);
    assert_eq!(agy_gemini.availability, CliUsageAvailability::Available, "Antigravity (Gemini) 配額回應應可解析。");
    assert_eq!(agy_gemini.windows.len(), 1, "Gemini 應有一個額度視窗。");
    assert_eq!(agy_gemini.windows[0].remaining_percent(), 90, "剩餘百分比應為 90。");
    assert_eq!(agy_gemini.windows[0].used_percent, 10, "已使用百分比應為 10。");
    assert_eq!(agy_gemini.windows[0].resets_at.unwrap().to_utc().hour(), 5, "應精確解析 UTC 重置時分。");

    let agy_claude = usage::parse_antigravity_model_configs(agy_response, CliKind::AntigravityClaude, &observed_at);
    assert_eq!(
        agy_claude.availability,
        CliUsageAvailability::Available,
        "Antigravity (Claude / GPT) 配額回應應可解析。"
    );
    assert_eq!(agy_claude.windows.len(), 1, "Claude/GPT 應有一個額度視窗。");
    assert_eq!(agy_claude.windows[0].remaining_percent(), 100, "剩餘百分比應為 100。");
    assert_eq!(agy_claude.windows[0].used_percent, 0, "已使用百分比應為 0。");
    assert_eq!(agy_claude.windows[0].resets_at.unwrap().to_utc().hour(), 9, "應精確解析 UTC 重置時分。");

    // 新版內建 /usage 的 command.data 結構，含低於 UI 四捨五入精度的用量。
    let iso = |moment: Moment| time::format_round_trip(&moment);
    let usage_data = format!(
        r#"{{"groups":[
          {{"name":"Gemini Models","buckets":[
            {{"name":"Five Hour Limit Remaining","window":"5h","remaining_fraction":0.999,"reset_time":"{}"}},
            {{"name":"Weekly Limit Remaining","window":"weekly","remaining_fraction":0.8,"reset_time":"{}"}}]}},
          {{"name":"Claude and GPT models","buckets":[
            {{"name":"Five Hour Limit Remaining","window":"5h","remaining_fraction":1,"reset_time":"{}"}}]}}]}}"#,
        iso(observed_at + Span::hours(2)),
        iso(observed_at + Span::days(3)),
        iso(observed_at + Span::hours(5)),
    );
    let usage_gemini = usage::parse_antigravity_model_configs(&usage_data, CliKind::Antigravity, &observed_at);
    assert!(
        usage_gemini.windows.len() == 2 && usage_gemini.windows[1].duration == Some(Span::days(7)),
        "新版 usage 應解析 5h 與 weekly。"
    );
    assert!(
        usage_gemini.windows[0].used_percent == 0 && is_counting_down(Some(&usage_gemini), &observed_at),
        "微量使用即使顯示為 0% 仍應倒數。"
    );
    let usage_claude = usage::parse_antigravity_model_configs(&usage_data, CliKind::AntigravityClaude, &observed_at);
    assert!(
        usage_claude.windows.len() == 1 && !is_counting_down(Some(&usage_claude), &observed_at),
        "滿額滑動時間不可當成真正倒數。"
    );
    let tiny_claude = usage::parse_claude_usage(
        &format!(r#"{{"five_hour":{{"utilization":0.1,"resets_at":"{}"}}}}"#, iso(observed_at + Span::hours(2))),
        &observed_at,
    )
    .unwrap();
    assert!(is_counting_down(Some(&tiny_claude), &observed_at), "Claude 微量使用亦應立即啟動倒數。");
    let later = observed_at + Span::seconds(10);
    let cached_tiny_claude = usage::refresh_claude_snapshot(&tiny_claude, &later, "cached");
    assert!(is_counting_down(Some(&cached_tiny_claude), &later), "Claude 微量使用的快取更新亦應保留有效倒數。");

    let agy_error = usage::parse_antigravity_model_configs(
        "{\"error\":{\"message\":\"invalid CSRF token\"}}",
        CliKind::Antigravity,
        &observed_at,
    );
    assert_eq!(agy_error.availability, CliUsageAvailability::Unavailable, "Antigravity 錯誤回應不可標示為可用。");
    let agy_missing_pool =
        usage::parse_antigravity_model_configs("{\"clientModelConfigs\":[]}", CliKind::Antigravity, &observed_at);
    assert_eq!(agy_missing_pool.availability, CliUsageAvailability::Unavailable, "缺少模型配額時應回傳 Unavailable。");

    // 測試 is_counting_down 判斷邏輯
    let future_reset = observed_at + Span::hours(3);
    let past_reset = observed_at - Span::hours(1);

    let counting_down = available(
        CliKind::Antigravity,
        vec![window("Antigravity (Gemini)", 10, Span::hours(5), future_reset, true)],
        observed_at,
    );
    assert!(is_counting_down(Some(&counting_down), &observed_at), "真實倒數且重置時間在未來時應判定為倒數中。");

    let sliding = available(
        CliKind::AntigravityClaude,
        vec![window("Antigravity (Claude / GPT)", 0, Span::hours(5), future_reset, false)],
        observed_at,
    );
    assert!(!is_counting_down(Some(&sliding), &observed_at), "滑動時間戳記（未消耗額度）應判定為未倒數。");

    let expired = available(
        CliKind::Antigravity,
        vec![window("Antigravity (Gemini)", 0, Span::hours(5), past_reset, false)],
        observed_at,
    );
    assert!(!is_counting_down(Some(&expired), &observed_at), "重置時間在過去時應判定為未倒數。");

    let claude_5h_past_7d_future = available(
        CliKind::Claude,
        vec![
            window("Claude（5 小時）", 0, Span::hours(5), past_reset, false),
            window("Claude（7 天）", 20, Span::days(7), future_reset + Span::days(4), true),
        ],
        observed_at,
    );
    assert!(
        !is_counting_down(Some(&claude_5h_past_7d_future), &observed_at),
        "Claude 5小時已重置時即使7天仍在倒數也應判定為未倒數。"
    );
    assert!(!is_counting_down(None, &observed_at), "None snapshot 應判定為未倒數。");
    assert!(!is_counting_down(Some(&claude_error), &observed_at), "Unavailable snapshot 應判定為未倒數。");

    // Antigravity CSRF Token 擷取：--name=value、--name value、加引號與
    // extension server 變體都要能取出，不相關的日誌行則不可誤判。
    let token = |text: &str| usage::extract_antigravity_token(text);
    assert_eq!(token("language_server.exe --csrf_token=abc123 --port 6370").as_deref(), Some("abc123"));
    assert_eq!(token("language_server.exe --csrf_token def456 --port 6370").as_deref(), Some("def456"));
    assert_eq!(
        token("language_server.exe --csrf_token \"ghi789\"").as_deref(),
        Some("ghi789"),
        "加雙引號的 Token 應去掉引號。"
    );
    assert_eq!(token("--extension_server_csrf_token=jkl012").as_deref(), Some("jkl012"));
    assert_eq!(token("listening on random port at 6370 for HTTPS"), None, "不含 Token 的日誌行不應誤判。");

    // 連接埠與 Token 都是最近一次啟動寫入的，必須從日誌尾端讀；
    // 從檔頭讀會拿到 Antigravity 重啟前的失效連接埠。
    let directory = TempDir::new();
    let agy_log_path = directory.join("agy.log");
    let mut log_lines = vec!["listening on random port at 1111 for HTTPS".to_string()];
    log_lines.extend((0..500).map(|i| format!("noise line {i}")));
    log_lines.push("listening on random port at 6370 for HTTPS".to_string());
    std::fs::write(&agy_log_path, log_lines.join("\r\n") + "\r\n").unwrap();

    let tail = usage::read_tail_lines(&agy_log_path, 300);
    assert_eq!(tail.len(), 300, "應只讀取尾端指定行數。");
    assert!(tail.last().unwrap().contains("6370"), "尾端最後一行應是最新一次啟動寫入的連接埠。");
    assert!(!tail.iter().any(|line| line.contains("1111")), "Antigravity 重啟前的失效連接埠不應出現在尾端視窗中。");
    assert!(
        usage::read_tail_lines(&directory.join("missing.log"), 300).is_empty(),
        "日誌檔不存在時應回傳空清單而非回報錯誤。"
    );

    // 登入失效判定：只認登入相關錯誤，網路被擋或 429 不可誤判成要重新登入。
    let login_required = |text: &str| login::looks_like_login_required(Some(text));
    assert!(login_required("Codex 額度查詢失敗：Not logged in"), "Codex 未登入應判定為需要登入。");
    assert!(login_required("HTTP 401 Unauthorized"), "401 應判定為需要登入。");
    assert!(login_required("Antigravity 額度查詢失敗：user is unauthenticated"), "unauthenticated 應判定為需要登入。");
    assert!(
        !login_required("Codex 額度查詢失敗：failed to fetch codex rate limits: error sending request for url (https://chatgpt.com/backend-api/wham/usage)"),
        "網路錯誤不可判定為需要登入。"
    );
    assert!(
        !login_required("無法連線至 Claude 額度服務：嘗試存取通訊端被拒絕 (api.anthropic.com:443)"),
        "連線被拒不可判定為需要登入。"
    );
    assert!(!login_required("Claude 額度查詢暫時受限（HTTP 429），將於 10:00:00 後再試。"), "429 不可判定為需要登入。");
    assert!(!login_required("listening on port 54012"), "數字中間出現 401 不可誤判。");
    assert!(!login::looks_like_login_required(None));

    let claude_login = login::create_start_info(CliKind::Claude, "C:\\my tools\\claude.exe", "").unwrap();
    assert!(claude_login.file_name == "cmd.exe" && !claude_login.create_no_window, "登入必須開啟看得見的主控台。");
    assert_eq!(
        claude_login.arguments, "/d /s /c \"\"C:\\my tools\\claude.exe\" auth login || pause\"",
        "Claude 登入指令應為 auth login，失敗時暫停。"
    );
    assert_eq!(
        login::create_start_info(CliKind::Codex, "codex.exe", "").unwrap().arguments,
        "/d /s /c \"codex.exe login || pause\"",
        "Codex 登入指令應為 login。"
    );
    assert_eq!(
        login::create_start_info(CliKind::AntigravityClaude, "agy.exe", "").unwrap().arguments,
        "/d /s /c \"agy.exe || pause\"",
        "agy 沒有 login 子命令，應直接進互動模式。"
    );
}

#[test]
fn antigravity_usage_source_cache() {
    let _serial = serial();
    let directory = TempDir::new();

    let create_usage_cli = |folder: &Path, remaining: f64| -> String {
        std::fs::create_dir_all(folder).unwrap();
        let path = folder.join("usage.cmd");
        let json = serde_json::json!({
            "command": { "name": "usage", "data": { "groups": [
                { "name": "Gemini models", "buckets": [
                    { "name": "5h", "window": "5h", "remaining_fraction": remaining,
                      "reset_time": time::format_round_trip(&(time::now() + Span::hours(2))) }
                ] }
            ] } }
        });
        std::fs::write(
            &path,
            format!("@echo off\r\necho call>>\"{}\"\r\necho {}\r\n", folder.join("calls.txt").display(), json),
        )
        .unwrap();
        path.to_string_lossy().into_owned()
    };

    let first_folder = directory.join("first");
    let second_folder = directory.join("second");
    let first_path = create_usage_cli(&first_folder, 0.7);
    let second_path = create_usage_cli(&second_folder, 0.2);
    let reader = CliUsageReader::new();
    let cancel = CancelToken::none();
    let profile = |path: &str| CliProfile { executable: path.to_string(), ..CliProfile::default() };

    let first =
        reader.read(CliKind::Antigravity, &profile(&first_path), &first_folder.to_string_lossy(), &cancel).unwrap();
    assert!(
        first.availability == CliUsageAvailability::Available
            && first.windows.len() == 1
            && first.windows[0].remaining_percent() == 70,
        "Initial CLI quota must be read: {}",
        first.message
    );
    reader.read(CliKind::Antigravity, &profile(&first_path), &first_folder.to_string_lossy(), &cancel).unwrap();
    assert_eq!(
        std::fs::read_to_string(first_folder.join("calls.txt")).unwrap().lines().count(),
        1,
        "Repeated source must reuse its short-lived quota cache."
    );
    let second =
        reader.read(CliKind::Antigravity, &profile(&second_path), &second_folder.to_string_lossy(), &cancel).unwrap();
    assert!(
        second.availability == CliUsageAvailability::Available
            && second.windows.len() == 1
            && second.windows[0].remaining_percent() == 20,
        "Changed CLI source must not reuse the old quota: {}",
        second.message
    );
}

#[test]
fn schedule_calculator() {
    let now = local_wall_clock(2026, 8, 10, 12, 0, 0);
    let local = |moment: &Moment| time::local_naive(moment);

    let morning = calculator::get_next_daily_occurrence(hm(8, 30) + Span::seconds(45), &now).unwrap();
    assert_eq!(local(&morning), naive(2026, 8, 11, 8, 30, 0), "已過的每日時分應排到明天並移除秒數。");
    let evening = calculator::get_next_daily_occurrence(hm(18, 5), &now).unwrap();
    assert_eq!(local(&evening), naive(2026, 8, 10, 18, 5, 0), "尚未到的每日時分應排在今天。");

    // 跨日邊界測試：午夜 23:59:59 評估 00:00:00 排程
    let midnight_now = local_wall_clock(2026, 12, 31, 23, 59, 59);
    let next_day_midnight = calculator::get_next_daily_occurrence(Span::zero(), &midnight_now).unwrap();
    assert_eq!(local(&next_day_midnight), naive(2027, 1, 1, 0, 0, 0), "跨日午夜排程應排到下一年的 1 月 1 日 00:00。");

    // 時分剛好等於目前時刻邊界 (應排到明天)
    let exact_now = local_wall_clock(2026, 8, 10, 8, 30, 0);
    let exact_next = calculator::get_next_daily_occurrence(hm(8, 30), &exact_now).unwrap();
    assert_eq!(local(&exact_next), naive(2026, 8, 11, 8, 30, 0), "當前時刻與排程時間相同時應排到明天。");

    // 追溯過期多日 (系統休眠/時鐘回撥/錯過執行)
    let old = local_wall_clock(2026, 8, 1, 8, 30, 0);
    let daily = calculator::get_next_occurrence(&old, ScheduleRecurrence::Daily, &now).unwrap();
    assert_eq!(local(&daily), naive(2026, 8, 11, 8, 30, 0), "過期多日的每日排程應跳到目前時間之後的下一個時點。");

    let old_weekly = local_wall_clock(2026, 7, 1, 8, 30, 0);
    let weekly = calculator::get_next_occurrence(&old_weekly, ScheduleRecurrence::Weekly, &now).unwrap();
    assert!(
        local(&weekly) > local(&now) && local(&weekly).weekday() == local(&old_weekly).weekday(),
        "過期多週的每週排程應保持相同星期數並跳至未來。"
    );

    // 夏令時間 (DST) 與時區時分保留測試
    let dst_offset_now = chrono::DateTime::parse_from_rfc3339("2026-03-29T23:30:00-05:00").unwrap();
    let dst_next = calculator::get_next_daily_occurrence(hm(8, 30), &dst_offset_now).unwrap();
    assert!(
        local(&dst_next).hour() == 8 && local(&dst_next).minute() == 30,
        "跨時區/DST 時應保持本地牆上時間 (Wall-clock time) 的時與分。"
    );

    // 參數邊界與例外測試
    assert!(matches!(calculator::get_next_daily_occurrence(Span::hours(-1), &now), Err(CoreError::OutOfRange(_))));
    assert!(matches!(calculator::get_next_daily_occurrence(Span::hours(24), &now), Err(CoreError::OutOfRange(_))));
    assert!(calculator::get_next_occurrence(&old, ScheduleRecurrence::Once, &now).is_err_and(|e| e.is_argument()));

    // 邊界合法值 (00:00:00 與 23:59:59)
    let min_time = calculator::get_next_daily_occurrence(Span::zero(), &now).unwrap();
    assert_eq!(time::local_time_of_day(&min_time), Span::zero(), "00:00 應為合法每日時間。");
    let max_time = calculator::get_next_daily_occurrence(hm(23, 59) + Span::seconds(59), &now).unwrap();
    assert_eq!(time::local_time_of_day(&max_time), hm(23, 59), "23:59 應為合法每日時間。");

    // 自動模式（每 5 小時 1 分鐘 = 301 分鐘）測試
    assert_eq!(calculator::auto_interval(), Span::minutes(301), "AutoInterval 應精確為 301 分鐘（5 小時 1 分鐘）。");
    let initial_wakeup = hm(5, 30);
    let next_auto =
        |finished: &Moment| calculator::get_next_auto_interval_occurrence(initial_wakeup, finished).unwrap();

    // 1. 同日日間正常推進（08:00 完成 -> 13:01）
    let finished_morning = local_wall_clock(2026, 8, 10, 8, 0, 0);
    let next_morning = next_auto(&finished_morning);
    assert_eq!(next_morning, finished_morning + calculator::auto_interval(), "當日日間 08:00 執行完成應推進至 13:01。");
    assert_eq!(local(&next_morning), naive(2026, 8, 10, 13, 1, 0), "13:01 時間應正確。");

    // 2. 同日傍晚正常推進（18:02 完成 -> 23:03）
    let finished_evening = local_wall_clock(2026, 8, 10, 18, 2, 0);
    let next_evening = next_auto(&finished_evening);
    assert_eq!(next_evening, finished_evening + calculator::auto_interval(), "當日 18:02 執行完成應推進至 23:03。");
    assert_eq!(local(&next_evening), naive(2026, 8, 10, 23, 3, 0), "23:03 時間應正確。");

    // 3. 跨日不排半夜：23:03 完成後加 5h1m 為隔日 04:04（超過 24:00），應自動重置為隔日 05:30
    let next_cross_day = next_auto(&local_wall_clock(2026, 8, 10, 23, 3, 0));
    assert_eq!(
        local(&next_cross_day),
        naive(2026, 8, 11, 5, 30, 0),
        "23:03 跨日後不應排在半夜 04:04，應排在隔天 05:30。"
    );

    // 4. 20:00 執行跨日（加 5h1m 為 01:01），重置為隔日 05:30
    let next_after_8pm = next_auto(&local_wall_clock(2026, 8, 10, 20, 0, 0));
    assert_eq!(local(&next_after_8pm), naive(2026, 8, 11, 5, 30, 0), "20:00 跨日後應排在隔天 05:30。");

    // 5. 跨日計算異常邊界
    assert!(matches!(
        calculator::get_next_auto_interval_occurrence(Span::hours(-1), &finished_morning),
        Err(CoreError::OutOfRange(_))
    ));
    assert!(matches!(
        calculator::get_next_auto_interval_occurrence(Span::hours(24), &finished_morning),
        Err(CoreError::OutOfRange(_))
    ));
}

#[test]
fn json_file_store() {
    let directory = TempDir::new();
    let store = job_store(directory.join("jobs.json"));
    let expected = ScheduledJob {
        name: "測試排程".to_string(),
        message: "早安".to_string(),
        scheduled_at: time::now() + Span::hours(1),
        targets: vec![CliKind::Antigravity, CliKind::Codex, CliKind::Claude],
        initial_time_of_day: Some(hm(8, 49)),
        ..ScheduledJob::default()
    };
    store.save(&vec![expected.clone()]).unwrap();
    let actual = store.load().unwrap();
    assert_eq!(actual.len(), 1, "應載入一筆排程。");
    assert_eq!(actual[0].name, expected.name, "排程名稱應往返保存。");
    assert_eq!(actual[0].targets, expected.targets);
    assert_eq!(actual[0].id, expected.id, "排程識別碼應往返保存。");
    assert_eq!(actual[0].initial_time_of_day, expected.initial_time_of_day, "開始時間應往返保存。");
    assert!(
        (actual[0].scheduled_at - expected.scheduled_at).abs() < Span::microseconds(1),
        "排程時間應往返保存（資料檔精度為 100 奈秒）。"
    );

    // 0. 舊版程式寫出的資料檔（PascalCase 屬性、字串列舉、hh:mm:ss 時間）必須可直接讀取
    let legacy_path = directory.join("legacy.json");
    std::fs::write(
        &legacy_path,
        r#"[{"Id":"3f2504e0-4f89-11d3-9a0c-0305e82c3301","Name":"舊版排程","ScheduledAt":"2026-10-07T08:49:00+08:00",
            "Message":"早安","WorkingDirectory":"C:\\Users","Targets":["Antigravity","AntigravityClaude","Codex","Claude"],
            "Recurrence":"Interval","InitialTimeOfDay":"08:49:00","Enabled":false,"Status":"Disabled","StartedAt":null,
            "FinishedAt":"2026-10-06T23:03:00.1234567+08:00",
            "LastResults":[{"Cli":"Codex","Succeeded":true,"ExitCode":0,"StartedAt":"2026-10-06T23:02:40+08:00",
              "FinishedAt":"2026-10-06T23:03:00+08:00","ExecutablePath":"codex.exe","Error":"","LogPath":""}]}]"#,
    )
    .unwrap();
    let legacy = job_store(legacy_path).load().unwrap();
    assert_eq!(legacy[0].id.to_string(), "3f2504e0-4f89-11d3-9a0c-0305e82c3301");
    assert_eq!(legacy[0].recurrence, ScheduleRecurrence::Interval);
    assert_eq!(legacy[0].initial_time_of_day, Some(hm(8, 49)));
    assert!(!legacy[0].enabled && legacy[0].status == ScheduleStatus::Disabled && legacy[0].started_at.is_none());
    assert_eq!(legacy[0].finished_at.unwrap().timestamp_subsec_nanos(), 123_456_700);
    assert_eq!(legacy[0].last_results[0].exit_code, Some(0));
    assert_eq!(time::format_json_moment(&legacy[0].finished_at.unwrap()), "2026-10-06T23:03:00.1234567+08:00");

    let settings_path = directory.join("settings.json");
    std::fs::write(
        &settings_path,
        r#"{"StartWithWindows":true,"MinimizeToTray":false,"ExecutionTimeoutMinutes":7,
            "CliProfiles":{"Codex":{"Executable":"codex","Model":"gpt-6-luna","ThinkingEffort":"Max","AdditionalArguments":""}}}"#,
    )
    .unwrap();
    let settings_store: JsonFileStore<AppSettings> = JsonFileStore::new(settings_path, AppSettings::create_default);
    let mut settings = settings_store.load().unwrap();
    assert!(settings.start_with_windows && !settings.minimize_to_tray && settings.token_saver_mode);
    assert_eq!(settings.execution_timeout_minutes, 7);
    assert_eq!(settings.cli_profiles.len(), 1, "資料檔中的 CLI 設定應取代預設值。");
    assert_eq!(settings.cli_profiles[&CliKind::Codex].thinking_effort, ThinkingEffort::Max);
    settings.ensure_defaults();
    assert_eq!(settings.cli_profiles.len(), 4, "ensure_defaults 應補齊缺少的 CLI 設定。");
    settings_store.save(&settings).unwrap();
    assert_eq!(settings_store.load().unwrap(), settings, "設定應完整往返保存。");

    // 1. 測試讀取不存在的檔案 (應傳回 default_factory)
    assert!(
        job_store(directory.join("non_existent.json")).load().unwrap().is_empty(),
        "不存在的檔案應傳回預設工廠建立的物件。"
    );

    // 2. 測試破損 JSON 內容 (損毀語法)
    let corrupted_path = directory.join("corrupted.json");
    std::fs::write(&corrupted_path, "INVALID JSON {{ [").unwrap();
    assert!(matches!(job_store(corrupted_path).load(), Err(CoreError::InvalidData(_))));
    let backups = std::fs::read_dir(directory.path())
        .unwrap()
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("corrupted.json.corrupted."))
        .count();
    assert_eq!(backups, 1, "損毀的資料檔應先備份。");

    // 3. 測試截斷/未完成的 JSON 內容 (模擬檔案寫入遺失/部分寫入)
    let truncated_path = directory.join("truncated.json");
    std::fs::write(&truncated_path, "[{\"Name\":\"截斷內容").unwrap();
    assert!(matches!(job_store(truncated_path).load(), Err(CoreError::InvalidData(_))));

    // 4. 測試原子寫入與臨時檔 (.tmp) 清理
    let atomic_path = directory.join("atomic.json");
    let atomic_store = job_store(atomic_path.clone());
    atomic_store.save(&vec![expected.clone()]).unwrap();
    assert!(atomic_path.exists(), "目標檔案應原子替換寫入。");
    let leftovers = std::fs::read_dir(directory.path())
        .unwrap()
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
        .count();
    assert_eq!(leftovers, 0, "寫入完成後臨時檔 .tmp 應被正確移除。");

    // 5. 測試多執行緒併發 save
    let handles: Vec<_> = (0..10)
        .map(|i| {
            let store = atomic_store.clone();
            std::thread::spawn(move || {
                store.save(&vec![ScheduledJob {
                    name: format!("併發作業-{i}"),
                    message: "測試".to_string(),
                    targets: vec![CliKind::Antigravity],
                    ..ScheduledJob::default()
                }])
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap().unwrap();
    }
    assert_eq!(atomic_store.load().unwrap().len(), 1, "併發寫入經由門控保護，資料檔應維持完整可讀。");
}

#[test]
fn cli_runner_safe_arguments() {
    let _serial = serial();
    let directory = TempDir::new();
    let output_path = directory.join("arguments.json");
    let paths = AppDataPaths::new(Some(&directory.join("data").to_string_lossy()));
    let runner = ProcessCliRunner::new(paths);
    let cancel = CancelToken::none();
    let profile = fake_profile(&output_path, 0, 0);
    let message = "早安 & whoami";
    let run = |profile: &CliProfile, message: &str, working_directory: &str, timeout: Duration| {
        runner.run(CliKind::Antigravity, profile, message, working_directory, timeout, true, &cancel, None)
    };

    let result = run(&profile, message, &directory.text(), Duration::from_secs(15));
    assert!(result.succeeded, "假 CLI 應成功：{}", result.error);
    assert!(Path::new(&result.log_path).is_file(), "應建立 CLI 日誌。");
    let received: Vec<String> = serde_json::from_str(&std::fs::read_to_string(&output_path).unwrap()).unwrap();
    assert!(
        received.iter().any(|argument| argument.starts_with(message)),
        "含 Shell 符號的訊息應完整留在單一提示參數中。"
    );
    assert!(has(&received, "--print"), "Antigravity 應使用 --print。");

    // 測試程序執行逾時
    let timeout_result = run(
        &fake_profile(&directory.join("timeout.json"), 3000, 0),
        "逾時測試",
        &directory.text(),
        Duration::from_millis(200),
    );
    assert!(!timeout_result.succeeded, "逾時的 CLI 應回傳失敗。");
    assert!(timeout_result.error.contains("超過"), "逾時錯誤訊息應包含超過提示。");

    // 測試程序結束碼非 0 失敗
    let error_result = run(
        &fake_profile(&directory.join("error.json"), 0, 42),
        "錯誤碼測試",
        &directory.text(),
        Duration::from_secs(15),
    );
    assert!(!error_result.succeeded, "結束碼非 0 應標示為失敗。");
    assert_eq!(error_result.exit_code, Some(42), "應正確記錄結束碼 42。");

    // 測試無效工作目錄
    let invalid_dir_result =
        run(&profile, "無效目錄", &directory.join("nonexistent").to_string_lossy(), Duration::from_secs(5));
    assert!(!invalid_dir_result.succeeded, "無效工作目錄應執行失敗。");
    assert!(invalid_dir_result.error.contains("工作目錄不存在"), "應回傳工作目錄不存在錯誤。");

    // 測試損毀/非法的可執行檔 (觸發程序啟動失敗與錯誤日誌建立)
    let invalid_exe_path = directory.join("invalid_app.exe");
    std::fs::write(&invalid_exe_path, "NOT A VALID EXE FILE").unwrap();
    let invalid_exe_profile =
        CliProfile { executable: invalid_exe_path.to_string_lossy().into_owned(), ..CliProfile::default() };
    let invalid_exe_result = run(&invalid_exe_profile, "無效執行檔測試", &directory.text(), Duration::from_secs(5));
    assert!(!invalid_exe_result.succeeded, "非法的可執行檔應執行失敗。");
    assert!(invalid_exe_result.log_path.to_lowercase().ends_with("-error.log"), "執行失敗應建立 -error.log 日誌檔案。");
    assert!(Path::new(&invalid_exe_result.log_path).is_file(), "錯誤日誌檔案應存在於磁碟。");

    // 即時輸出回呼：stdout 與 stderr 都要如實轉送。
    let echo_path = directory.join("echo-streams.cmd");
    std::fs::write(&echo_path, "@echo off\r\necho to-stdout\r\necho to-stderr 1>&2\r\n").unwrap();
    let chunks: Mutex<Vec<(bool, String)>> = Mutex::new(Vec::new());
    let streams = process::execute(
        &echo_path.to_string_lossy(),
        &[] as &[&str],
        &directory.text(),
        Duration::from_secs(20),
        &cancel,
        Some(&|is_error, text| chunks.lock().unwrap().push((is_error, text.to_string()))),
    )
    .unwrap();
    assert!(
        streams.used_shell
            && streams.standard_output.contains("to-stdout")
            && streams.standard_error.contains("to-stderr")
    );
    let chunks = chunks.into_inner().unwrap();
    assert!(chunks.iter().any(|(is_error, text)| !is_error && text.contains("to-stdout")), "stdout 片段應即時通知。");
    assert!(chunks.iter().any(|(is_error, text)| *is_error && text.contains("to-stderr")), "stderr 片段應即時通知。");

    // .cmd / .bat 必須經由 cmd.exe 執行，而且不能開視窗、輸出要抓得到。
    let shim_path = directory.join("echo-args.cmd");
    // %2 保留原本的引號：批次檔內若把 & 展開成裸字元，
    // 會被 cmd 當成命令分隔符，那是批次檔自身的語法問題而非參數傳遞問題。
    std::fs::write(&shim_path, "@echo off\r\necho ARG1=%~1\r\necho ARG2=%2\r\n").unwrap();
    let shim_result = process::execute(
        &shim_path.to_string_lossy(),
        &["hello world", "a&b"],
        &directory.text(),
        Duration::from_secs(20),
        &cancel,
        None,
    )
    .unwrap();
    assert_eq!(shim_result.exit_code, 0, ".cmd 應正常結束：{}", shim_result.standard_error);
    assert!(
        shim_result.standard_output.contains("ARG1=hello world"),
        ".cmd 執行的輸出必須擷取得到，且含空白的參數要完整傳入。"
    );
    assert!(
        shim_result.standard_output.contains("ARG2=\"a&b\""),
        "含 cmd 保留字元的參數必須以單一參數送達，不可在外層命令列被切開。"
    );

    // 取消：執行中的子程序必須被終止並回報取消。
    let canceling = CancelToken::new();
    let trigger = canceling.clone();
    let canceler = std::thread::spawn(move || {
        sleep(200);
        trigger.cancel();
    });
    let started = Instant::now();
    let canceled = runner.run(
        CliKind::Antigravity,
        &fake_profile(&directory.join("canceled.json"), 5000, 0),
        "取消測試",
        &directory.text(),
        Duration::from_secs(15),
        true,
        &canceling,
        None,
    );
    canceler.join().unwrap();
    assert!(!canceled.succeeded && canceled.error == "執行已取消。", "取消應回報為已取消：{}", canceled.error);
    assert!(started.elapsed() < Duration::from_secs(3), "取消後不應等到子程序自行結束。");

    // 子孫程序的系統錯誤對話框必須被抑制：排程工具在背景執行時，
    // Codex 自行啟動的 git.exe 若被防毒攔到而以 0xc0000142 啟動失敗，
    // 不能在使用者桌面上蓋一個 modal 對話框。
    process::suppress_child_process_error_dialogs();
    let error_mode = process::get_current_error_mode();
    assert!(
        error_mode & process::SEM_FAILCRITICALERRORS != 0,
        "應設定 SEM_FAILCRITICALERRORS，子程序啟動失敗才不會彈出對話框。"
    );
    assert!(
        error_mode & process::SEM_NOGPFAULTERRORBOX != 0,
        "應設定 SEM_NOGPFAULTERRORBOX，子程序當掉才不會彈出錯誤報告對話框。"
    );

    // cmd.exe 的引號規則：空白與引號要包起來，結尾反斜線要加倍。
    let shell = |executable: &str, arguments: &[&str]| process::build_command_shell_arguments(executable, arguments);
    assert_eq!(
        shell("C:\\tools\\run.cmd", &[]),
        "/d /s /c \"C:\\tools\\run.cmd\"",
        "沒有特殊字元的路徑不應被多加引號。"
    );
    assert_eq!(
        shell("C:\\my tools\\run.cmd", &["早安 & whoami"]),
        "/d /s /c \"\"C:\\my tools\\run.cmd\" \"早安 & whoami\"\"",
        "含空白的路徑與參數都必須各自加引號。"
    );
    assert_eq!(
        shell("run.cmd", &["say \"hi\""]),
        "/d /s /c \"run.cmd \"say \\\"hi\\\"\"\"",
        "參數內的雙引號必須以反斜線跳脫。"
    );
    assert_eq!(
        shell("run.cmd", &["C:\\path with space\\"]),
        "/d /s /c \"run.cmd \"C:\\path with space\\\\\"\"",
        "結尾反斜線必須加倍，否則會把收尾引號跳脫掉。"
    );
    assert_eq!(shell("run.cmd", &[""]), "/d /s /c \"run.cmd \"\"\"", "空字串參數必須保留為一對引號。");
    // 迴歸測試：a&b 沒有空白也沒有引號，只依 argv 規則不會加引號，
    // 但 cmd 會把它切成兩個命令並嘗試執行 b。
    assert_eq!(
        shell("run.cmd", &["a&b"]),
        "/d /s /c \"run.cmd \"a&b\"\"",
        "含 cmd 運算子的參數即使沒有空白也必須加引號。"
    );
    assert_eq!(
        shell("run.cmd", &["a|b", "c>d", "e^f"]),
        "/d /s /c \"run.cmd \"a|b\" \"c>d\" \"e^f\"\"",
        "其餘 cmd 運算子同樣必須加引號。"
    );
}

#[test]
fn schedule_manager_due_job() {
    let _serial = serial();
    let directory = TempDir::new();
    let output_paths: HashMap<CliKind, PathBuf> =
        CliKind::ALL.iter().map(|&kind| (kind, directory.join(&format!("scheduled-{kind}-arguments.json")))).collect();
    let paths = AppDataPaths::new(Some(&directory.join("data").to_string_lossy()));
    let jobs = job_store(paths.jobs_file());
    let settings = shared_settings();
    for &kind in CliKind::ALL {
        settings.lock().unwrap().cli_profiles.insert(kind, fake_profile(&output_paths[&kind], 1000, 0));
    }
    let runner = Arc::new(ProcessCliRunner::new(paths));
    jobs.save(&vec![ScheduledJob {
        scheduled_at: time::now() - Span::seconds(1),
        ..job("到期測試", &directory, CliKind::ALL)
    }])
    .unwrap();

    let manager = manager_with(&jobs, runner, &settings, None, None);
    let activities: Arc<Mutex<Vec<CliActivityEvent>>> = Arc::default();
    let sink = activities.clone();
    manager.on_activity_logged(move |event| sink.lock().unwrap().push(event.clone()));

    let concurrent_stopwatch = Instant::now();
    manager.initialize().unwrap();
    let done = wait_until(Duration::from_secs(20), || {
        let finished = &manager.get_jobs().unwrap()[0];
        finished.status == ScheduleStatus::Pending && finished.last_results.len() == 4
    });
    let elapsed = concurrent_stopwatch.elapsed();
    let finished = manager.get_jobs().unwrap().remove(0);
    assert!(done && finished.enabled, "舊排程應轉為每日排程並繼續等待。");
    assert_eq!(finished.recurrence, ScheduleRecurrence::Daily, "舊排程載入後應轉為每日排程。");
    assert!(finished.scheduled_at > time::now(), "每日排程執行後應排到下一個未來時分。");
    assert_eq!(finished.last_results.len(), 4, "排程器應記錄四個 CLI 的結果。");
    assert!(finished.last_results.iter().all(|result| result.succeeded), "四個假 CLI 都應成功。");
    for output_path in output_paths.values() {
        assert!(output_path.is_file(), "排程器應真的啟動四個假 CLI。");
    }
    println!("  concurrent CLI elapsed: {} ms", elapsed.as_millis());
    assert!(
        elapsed < Duration::from_millis(2500),
        "四個各延遲 1 秒的 CLI 應平行完成，不應循序等待；實際 {} ms。",
        elapsed.as_millis()
    );
    {
        let activities = activities.lock().unwrap();
        for &kind in CliKind::ALL {
            let kinds: Vec<CliActivityKind> =
                activities.iter().filter(|event| event.cli == kind).map(|event| event.kind).collect();
            assert_eq!(kinds.first(), Some(&CliActivityKind::Started), "{kind} 應先廣播開始事件。");
            assert_eq!(kinds.last(), Some(&CliActivityKind::Completed), "{kind} 應以完成事件結束。");
        }
    }

    let recurring_id = Guid::new();
    let selected_time = time::now() - Span::hours(2);
    manager
        .upsert(&ScheduledJob {
            id: recurring_id,
            scheduled_at: selected_time,
            recurrence: ScheduleRecurrence::Once,
            ..job("每日測試", &directory, &[CliKind::Antigravity])
        })
        .unwrap();
    let find = |id: Guid| manager.get_jobs().unwrap().into_iter().find(|job| job.id == id).unwrap();
    let recurring = find(recurring_id);
    assert!(recurring.status == ScheduleStatus::Pending && recurring.enabled, "儲存後應成為等待中的每日排程。");
    assert_eq!(recurring.recurrence, ScheduleRecurrence::Daily, "核心層應強制使用每日排程。");
    assert!(recurring.scheduled_at > time::now(), "已過的時分應自動排到明天，而不是立即執行。");
    let scheduled_local = time::local_naive(&recurring.scheduled_at);
    let selected_local = time::local_naive(&selected_time);
    assert!(
        scheduled_local.hour() == selected_local.hour() && scheduled_local.minute() == selected_local.minute(),
        "每日排程應保留使用者選擇的時與分。"
    );
    assert_eq!(scheduled_local.second(), 0, "每日排程不應保存秒數。");

    let next_daily_occurrence = recurring.scheduled_at;
    manager.run_now(recurring_id).unwrap();
    wait_until(Duration::from_secs(20), || {
        let manual_run = find(recurring_id);
        !manual_run.last_results.is_empty() && manual_run.status == ScheduleStatus::Pending
    });
    let manual_run = find(recurring_id);
    assert!(manual_run.status == ScheduleStatus::Pending && manual_run.enabled, "立即執行後，每日排程應回到等待狀態。");
    assert_eq!(manual_run.scheduled_at, next_daily_occurrence, "立即執行不應改變原本的每天時分或跳過下一次排程。");

    // 刪除與關閉後的狀態
    manager.delete(recurring_id).unwrap();
    assert!(manager.get_jobs().unwrap().iter().all(|job| job.id != recurring_id), "刪除後不應再列出排程。");
    manager.shutdown();
    assert!(manager.get_jobs().is_err(), "關閉後不應再接受操作。");
}

#[test]
fn schedule_manager_adaptive_wait() {
    let _serial = serial();
    let directory = TempDir::new();
    let store = job_store(directory.join("data").join("schedules.json"));
    let settings = shared_settings();
    let runner = Arc::new(CountingCliRunner::default());

    let id = Guid::new();
    store
        .save(&vec![ScheduledJob {
            id,
            scheduled_at: time::now() - Span::seconds(1),
            ..job("到期喚醒", &directory, &[CliKind::Antigravity, CliKind::Claude])
        }])
        .unwrap();

    let manager = manager_with(&store, runner.clone(), &settings, None, None);

    // 排程器採自適應等待，第一輪掃描必須在啟動時立即發生，
    // 而不是等待任何固定間隔。
    let stopwatch = Instant::now();
    manager.initialize().unwrap();
    wait_until(Duration::from_secs(15), || runner.call_count() >= 2);
    let elapsed = stopwatch.elapsed();

    assert_eq!(runner.call_count(), 2, "應對兩個 CLI 各呼叫一次。");
    assert!(runner.last_token_saver_mode.load(Ordering::SeqCst), "預設應以節省 Token 模式執行。");
    assert!(
        elapsed < Duration::from_secs(2),
        "到期排程應立即啟動，不應等待輪詢間隔；實際 {} ms。",
        elapsed.as_millis()
    );

    wait_until(Duration::from_secs(5), || manager.get_jobs().unwrap()[0].status == ScheduleStatus::Pending);
    let finished = manager.get_jobs().unwrap().remove(0);
    assert_eq!(finished.status, ScheduleStatus::Pending, "執行完成後應回到等待狀態。");
    assert!(finished.scheduled_at > time::now(), "應排到下一個未來時分。");

    // 手動觸發應透過喚醒訊號立即執行，而非等到下一個間隔
    let manual_stopwatch = Instant::now();
    manager.run_now(id).unwrap();
    wait_until(Duration::from_secs(15), || runner.call_count() >= 4);
    let manual_elapsed = manual_stopwatch.elapsed();
    assert_eq!(runner.call_count(), 4, "立即執行應再呼叫兩次。");
    assert!(manual_elapsed < Duration::from_secs(2), "立即執行應馬上啟動；實際 {} ms。", manual_elapsed.as_millis());
}

#[test]
fn schedule_manager_restart_does_not_refire() {
    let _serial = serial();
    let directory = TempDir::new();
    let store = job_store(directory.join("data").join("schedules.json"));
    let settings = shared_settings();
    let runner = Arc::new(CountingCliRunner::default());

    // 今天的時分已經過了，而且今天已經跑過一次（finished_at 在該時點之後）
    let already_ran_at = time::now() - Span::minutes(30);
    let id = Guid::new();
    store
        .save(&vec![ScheduledJob {
            id,
            scheduled_at: already_ran_at,
            finished_at: Some(already_ran_at + Span::seconds(20)),
            ..job("今天已執行", &directory, &[CliKind::Antigravity])
        }])
        .unwrap();

    {
        let manager = manager_with(&store, runner.clone(), &settings, None, None);
        manager.initialize().unwrap();
        sleep(1500);

        let stored = manager.get_jobs().unwrap().into_iter().find(|item| item.id == id).unwrap();
        assert_eq!(runner.call_count(), 0, "重開程式不應重跑今天已完成的喚醒。");
        assert!(stored.scheduled_at > time::now(), "今天已執行過的排程應排到明天。");
        assert_eq!(stored.status, ScheduleStatus::Pending, "排程應維持等待中。");
    }

    // 相對地，程式關閉期間錯過的喚醒仍應補做一次
    let missed_runner = Arc::new(CountingCliRunner::default());
    store
        .save(&vec![ScheduledJob {
            scheduled_at: time::now() - Span::minutes(30),
            finished_at: None,
            ..job("錯過未執行", &directory, &[CliKind::Antigravity])
        }])
        .unwrap();

    let manager = manager_with(&store, missed_runner.clone(), &settings, None, None);
    manager.initialize().unwrap();
    wait_until(Duration::from_secs(10), || missed_runner.call_count() > 0);
    assert_eq!(missed_runner.call_count(), 1, "程式關閉期間錯過的喚醒仍應補做一次。");
}

#[test]
fn schedule_manager_coalesces_overdue_jobs() {
    let _serial = serial();
    let directory = TempDir::new();
    let store = job_store(directory.join("data").join("schedules.json"));
    let settings = shared_settings();
    let runner = Arc::new(CountingCliRunner::default());

    // 程式關閉了一段時間，三個排程都錯過了各自的時分。
    let (oldest_id, middle_id, newest_id) = (Guid::new(), Guid::new(), Guid::new());
    let missed = |id: Guid, name: &str, ago: Span| ScheduledJob {
        id,
        scheduled_at: time::now() - ago,
        finished_at: None,
        ..job(name, &directory, &[CliKind::Antigravity])
    };
    store
        .save(&vec![
            missed(oldest_id, "最早錯過", Span::hours(3)),
            missed(middle_id, "中間錯過", Span::hours(2)),
            missed(newest_id, "最近錯過", Span::minutes(30)),
        ])
        .unwrap();

    let manager = manager_with(&store, runner.clone(), &settings, None, None);
    manager.initialize().unwrap();
    wait_until(Duration::from_secs(10), || runner.call_count() > 0);
    // 給補跑的那個排程一點時間完成，並讓其餘排程有機會（錯誤地）被觸發。
    sleep(500);

    assert_eq!(runner.call_count(), 1, "多個排程同時逾時時只應補做最近的一個。");

    let jobs = manager.get_jobs().unwrap();
    let find = |id: Guid| jobs.iter().find(|item| item.id == id).unwrap();
    assert_eq!(find(newest_id).status, ScheduleStatus::Pending, "被補做的排程應回到等待中。");
    assert!(find(newest_id).scheduled_at > time::now(), "被補做的排程應排到下一個每日時點。");
    for id in [middle_id, oldest_id] {
        assert_eq!(find(id).status, ScheduleStatus::Pending, "被跳過的排程不應卡在執行中。");
        assert!(find(id).scheduled_at > time::now(), "被跳過的排程應直接推到下一個每日時點，而不是重複呼叫。");
    }
}

#[test]
fn schedule_manager_boundaries_and_state() {
    let _serial = serial();
    let directory = TempDir::new();
    let paths = AppDataPaths::new(Some(&directory.join("data").to_string_lossy()));
    let jobs = job_store(paths.jobs_file());
    let settings = shared_settings();
    let runner = Arc::new(ProcessCliRunner::new(paths));

    let manager = manager_with(&jobs, runner, &settings, None, None);
    manager.initialize().unwrap();
    let base = |name: &str, message: &str| ScheduledJob {
        message: message.to_string(),
        ..job(name, &directory, &[CliKind::Antigravity])
    };
    let is_argument_error = |result: CoreResult<()>| result.is_err_and(|error| error.is_argument());

    // 1. 排程名稱空白驗證
    assert!(is_argument_error(manager.upsert(&base("   ", "測試"))));
    // 2. 訊息空白驗證
    assert!(is_argument_error(manager.upsert(&base("測試", ""))));
    // 3. 訊息長度超過 50 字元驗證
    assert!(is_argument_error(manager.upsert(&base("測試", &"A".repeat(51)))));

    // 4. 訊息長度剛好 50 字元 (邊界值成功)
    let exact_50 = base("50字測試", &"A".repeat(50));
    manager.upsert(&exact_50).unwrap();
    let saved_50 = manager.get_jobs().unwrap().into_iter().find(|j| j.id == exact_50.id).unwrap();
    assert_eq!(saved_50.message.len(), 50, "長度 50 字元的訊息應可成功儲存。");

    // 5. CLI 目標列表為空驗證
    assert!(is_argument_error(manager.upsert(&ScheduledJob { targets: Vec::new(), ..base("測試", "早安") })));

    // 6. 工作目錄不存在驗證
    assert!(matches!(
        manager.upsert(&ScheduledJob {
            working_directory: directory.join("invalid_dir").to_string_lossy().into_owned(),
            ..base("測試", "早安")
        }),
        Err(CoreError::DirectoryNotFound(_))
    ));
    // 開始時間超出一天
    assert!(is_argument_error(
        manager.upsert(&ScheduledJob { initial_time_of_day: Some(Span::hours(24)), ..base("測試", "早安") })
    ));

    // 7. 找不到指定排程時 run_now
    assert!(matches!(manager.run_now(Guid::new()), Err(CoreError::InvalidOperation(_))));

    // 8. 排程執行中 (Running) 時禁止修改、刪除或重複立即執行
    settings
        .lock()
        .unwrap()
        .cli_profiles
        .insert(CliKind::Antigravity, fake_profile(&directory.join("slow-cli.json"), 3000, 0));
    let running_job =
        ScheduledJob { scheduled_at: time::now() + Span::hours(1), ..base("執行中保護測試", "早安") };
    manager.upsert(&running_job).unwrap();
    manager.run_now(running_job.id).unwrap();

    let find_running = || manager.get_jobs().unwrap().into_iter().find(|j| j.id == running_job.id);
    wait_until(Duration::from_secs(5), || find_running().is_some_and(|j| j.status == ScheduleStatus::Running));
    let running_state = find_running().unwrap();
    assert_eq!(running_state.status, ScheduleStatus::Running, "排程應處於 Running 狀態。");

    assert!(matches!(manager.run_now(running_job.id), Err(CoreError::InvalidOperation(_))));
    assert!(matches!(manager.upsert(&running_state), Err(CoreError::InvalidOperation(_))));
    assert!(matches!(manager.delete(running_job.id), Err(CoreError::InvalidOperation(_))));

    // 9. 驗證 AppSettings.quota_auto_refresh_minutes 預設值與 clone / ensure_defaults
    let test_settings = AppSettings { quota_auto_refresh_minutes: 30, ..AppSettings::default() };
    assert_eq!(test_settings.normalized().quota_auto_refresh_minutes, 30, "副本應複製 quota_auto_refresh_minutes。");
    let mut invalid_settings = AppSettings { quota_auto_refresh_minutes: -5, ..AppSettings::default() };
    invalid_settings.ensure_defaults();
    assert_eq!(invalid_settings.quota_auto_refresh_minutes, 0, "ensure_defaults 應將負值 clamp 為 0。");
}

#[test]
fn schedule_manager_auto_interval() {
    let _serial = serial();
    let directory = TempDir::new();
    let store = job_store(directory.join("data").join("schedules.json"));
    let settings = shared_settings();
    let runner = Arc::new(CountingCliRunner::default());
    let initial_wakeup = hm(5, 30);
    let interval_job = |name: &str| ScheduledJob {
        initial_time_of_day: Some(initial_wakeup),
        recurrence: ScheduleRecurrence::Interval,
        ..job(name, &directory, &[CliKind::Antigravity])
    };

    let auto_job = ScheduledJob { scheduled_at: time::now() - Span::seconds(1), ..interval_job("自動模式測試") };
    store.save(&vec![auto_job.clone()]).unwrap();

    // 1. 測試排程器初始化並觸發首次喚醒
    {
        let manager = manager_with(&store, runner.clone(), &settings, None, None);
        manager.initialize().unwrap();
        wait_until(Duration::from_secs(10), || runner.call_count() > 0);
        assert_eq!(runner.call_count(), 1, "首次到期應觸發一次呼叫。");

        let current = || manager.get_jobs().unwrap().into_iter().find(|j| j.id == auto_job.id).unwrap();
        wait_until(Duration::from_secs(5), || {
            current().status == ScheduleStatus::Pending && current().finished_at.is_some()
        });
        let stored = current();
        assert_eq!(stored.recurrence, ScheduleRecurrence::Interval, "週期應維持 Interval。");
        assert_eq!(stored.status, ScheduleStatus::Pending, "執行後應回到 Pending 狀態。");
        let finished_at = stored.finished_at.expect("應記錄 finished_at。");
        let expected_next =
            calculator::get_next_auto_interval_occurrence(stored.initial_time_of_day.unwrap(), &finished_at).unwrap();
        assert!(
            (stored.scheduled_at - expected_next).abs() < Span::seconds(2),
            "執行完成後 scheduled_at 應正確推算，預期：{expected_next}，實際：{}",
            stored.scheduled_at
        );

        // 2. 測試手動「立即執行」對自動模式之推進
        manager.run_now(auto_job.id).unwrap();
        wait_until(Duration::from_secs(10), || runner.call_count() > 1);
        assert_eq!(runner.call_count(), 2, "立即執行應再觸發一次呼叫。");

        wait_until(Duration::from_secs(5), || {
            current().status == ScheduleStatus::Pending && current().finished_at.is_some()
        });
        let stored = current();
        let expected_next = calculator::get_next_auto_interval_occurrence(
            stored.initial_time_of_day.unwrap(),
            &stored.finished_at.unwrap(),
        )
        .unwrap();
        assert!(
            (stored.scheduled_at - expected_next).abs() < Span::seconds(2),
            "立即執行後 scheduled_at 應重新推進，預期：{expected_next}，實際：{}",
            stored.scheduled_at
        );
    }

    // 3. 測試重啟防護：1 小時前剛跑過（未滿 5 小時 1 分鐘），重開不應重新觸發
    let recent_runner = Arc::new(CountingCliRunner::default());
    let now = time::now();
    let recent_clock = FakeClock::new(local_wall_clock(now.year(), now.month(), now.day(), 12, 0, 0));
    let recent_ran_at = recent_clock.now() - Span::hours(1);
    let recent_job = ScheduledJob {
        scheduled_at: calculator::get_next_auto_interval_occurrence(initial_wakeup, &recent_ran_at).unwrap(),
        finished_at: Some(recent_ran_at),
        ..interval_job("近期已執行")
    };
    store.save(&vec![recent_job.clone()]).unwrap();
    {
        let manager = manager_with(&store, recent_runner.clone(), &settings, None, Some(recent_clock.clone()));
        manager.initialize().unwrap();
        sleep(500);

        assert_eq!(recent_runner.call_count(), 0, "5 小時 1 分鐘未到期前重開程式不應重跑。");
        let stored = manager.get_jobs().unwrap().remove(0);
        assert!(stored.scheduled_at > recent_clock.now(), "排程時間應在未來的時點。");
    }

    // 4. 測試隔天逾時開機補做：昨天 23:03 跑過，今天 08:00 開機（超過 05:30），應立即補做一次
    let late_boot_clock = FakeClock::new(local_wall_clock(2026, 8, 19, 8, 0, 0));
    let late_boot_runner = Arc::new(CountingCliRunner::default());
    let yesterday_finished = local_wall_clock(2026, 8, 18, 23, 3, 0);
    let boot_job = |name: &str| ScheduledJob {
        scheduled_at: local_wall_clock(2026, 8, 19, 5, 30, 0),
        finished_at: Some(yesterday_finished),
        ..interval_job(name)
    };
    store.save(&vec![boot_job("隔天逾時開機補做")]).unwrap();
    {
        let manager = manager_with(&store, late_boot_runner.clone(), &settings, None, Some(late_boot_clock));
        manager.initialize().unwrap();
        wait_until(Duration::from_secs(10), || late_boot_runner.call_count() > 0);
        assert_eq!(late_boot_runner.call_count(), 1, "隔日 08:00 開機（已過 05:30）應立即補做今日第一輪。");
        wait_until(Duration::from_secs(5), || manager.get_jobs().unwrap()[0].status == ScheduleStatus::Pending);
        let late_job = manager.get_jobs().unwrap().remove(0);
        assert_eq!(
            time::local_naive(&late_job.scheduled_at),
            naive(2026, 8, 19, 13, 1, 0),
            "補做完成後應推進至當日 13:01。"
        );
    }

    // 5. 測試隔天提早開機等待：昨天 23:03 跑過，今天 04:30 開機（未到 05:30），不應提前執行
    let early_boot_clock = FakeClock::new(local_wall_clock(2026, 8, 19, 4, 30, 0));
    let early_boot_runner = Arc::new(CountingCliRunner::default());
    store.save(&vec![boot_job("隔天提早開機等待")]).unwrap();
    {
        let manager = manager_with(&store, early_boot_runner.clone(), &settings, None, Some(early_boot_clock));
        manager.initialize().unwrap();
        sleep(500);

        assert_eq!(early_boot_runner.call_count(), 0, "隔日 04:30 提早開機（未到 05:30）不應提前執行。");
        let early_job = manager.get_jobs().unwrap().remove(0);
        assert_eq!(
            time::local_naive(&early_job.scheduled_at),
            naive(2026, 8, 19, 5, 30, 0),
            "排程時間應排定在今日 05:30。"
        );
    }
}

#[test]
fn schedule_manager_quota_aware_interval() {
    let _serial = serial();
    let directory = TempDir::new();
    let store = job_store(directory.join("data").join("schedules.json"));
    let settings = shared_settings();
    let initial_wakeup = hm(8, 0);
    let interval_job = |name: &str, targets: &[CliKind]| ScheduledJob {
        initial_time_of_day: Some(initial_wakeup),
        recurrence: ScheduleRecurrence::Interval,
        ..job(name, &directory, targets)
    };
    let h5 = Span::hours(5);

    // 1. 當日首次時間之前（07:30 < 08:00）：即便未倒數也不提前喚醒
    let early_now = local_wall_clock(2026, 8, 21, 7, 30, 0);
    let early_runner = Arc::new(CountingCliRunner::default());
    let fake_usage_reader = Arc::new(FakeCliUsageReader::default());

    // 模擬三個 CLI：全部未在倒數中（例如 reset 在過去或 100% 額度）
    let past = early_now - Span::hours(1);
    fake_usage_reader.set(available(
        CliKind::Antigravity,
        vec![window("Antigravity (Gemini)", 0, h5, past, false)],
        early_now,
    ));
    fake_usage_reader.set(available(
        CliKind::AntigravityClaude,
        vec![window("Antigravity (Claude / GPT)", 0, h5, past, false)],
        early_now,
    ));
    fake_usage_reader.set(available(
        CliKind::Claude,
        vec![
            window("Claude（5 小時）", 0, h5, past, false),
            window("Claude（7 天）", 20, Span::days(7), early_now + Span::days(4), false),
        ],
        early_now,
    ));

    store
        .save(&vec![ScheduledJob {
            scheduled_at: local_wall_clock(2026, 8, 21, 13, 1, 0),
            finished_at: Some(local_wall_clock(2026, 8, 20, 23, 0, 0)),
            ..interval_job("流量未倒數測試", &[CliKind::Antigravity, CliKind::AntigravityClaude, CliKind::Claude])
        }])
        .unwrap();

    {
        let manager = manager_with(
            &store,
            early_runner.clone(),
            &settings,
            Some(fake_usage_reader.clone()),
            Some(FakeClock::new(early_now)),
        );
        manager.initialize().unwrap();
        sleep(400);
        assert_eq!(early_runner.call_count(), 0, "當日首次喚醒時間（08:00）之前，07:30 不應觸發未倒數立即執行。");
    }

    // 2. 當日首次時間之後（08:30 > 08:00）：
    // 設定 Antigravity（Gemini）倒數中（resets at 13:30），
    // AntigravityClaude 與 Claude 未倒數（resets at 07:30，已過期）。
    // 預期：只喚醒 AntigravityClaude 與 Claude，不呼叫 Antigravity！
    let late_now = local_wall_clock(2026, 8, 21, 8, 30, 0);
    let late_runner = Arc::new(CountingCliRunner::default());
    fake_usage_reader.set(available(
        CliKind::Antigravity,
        vec![window("Antigravity (Gemini)", 10, h5, late_now + h5, true)],
        late_now,
    ));
    fake_usage_reader.set(available(
        CliKind::AntigravityClaude,
        vec![window("Antigravity (Claude / GPT)", 0, h5, late_now - Span::hours(1), false)],
        late_now,
    ));
    fake_usage_reader.set(available(
        CliKind::Claude,
        vec![
            window("Claude（5 小時）", 0, h5, late_now - Span::minutes(30), false),
            window("Claude（7 天）", 30, Span::days(7), late_now + Span::days(3), true),
        ],
        late_now,
    ));

    {
        let manager = manager_with(
            &store,
            late_runner.clone(),
            &settings,
            Some(fake_usage_reader.clone()),
            Some(FakeClock::new(late_now)),
        );
        manager.initialize().unwrap();
        wait_until(Duration::from_secs(10), || late_runner.call_count() >= 2);
        sleep(200);

        assert_eq!(late_runner.call_count(), 2, "當日首次時間後，應只呼叫未倒數的 2 個 CLI。");
        let executed = late_runner.executed();
        assert!(!executed.contains(&CliKind::Antigravity), "處於倒數中的 Antigravity 不應被呼叫。");
        assert!(executed.contains(&CliKind::AntigravityClaude), "未倒數的 AntigravityClaude 應被呼叫。");
        assert!(executed.contains(&CliKind::Claude), "未倒數的 Claude 應被呼叫。");
    }

    // 3. 背景排程迴圈即使被頻繁喚醒，未倒數時也只能依使用者設定的分鐘間隔查一次額度。
    // 這是 Claude /api/oauth/usage 不再因每 30 秒重查而觸發 HTTP 429 的迴歸測試。
    settings.lock().unwrap().quota_auto_refresh_minutes = 10;
    let throttled_now = local_wall_clock(2026, 8, 21, 9, 0, 0);
    let throttled_clock = FakeClock::new(throttled_now);
    let throttled_usage_reader = Arc::new(FakeCliUsageReader::default());
    // 模擬未抓到倒數（例如 100% 額度或未在倒數中）
    throttled_usage_reader.set(available(
        CliKind::Claude,
        vec![window("Claude（5 小時）", 0, h5, throttled_now - Span::hours(1), false)],
        throttled_now,
    ));
    store
        .save(&vec![ScheduledJob {
            scheduled_at: throttled_now + Span::hours(4),
            finished_at: Some(throttled_now - Span::hours(1)),
            ..interval_job("額度探測節流測試", &[CliKind::Claude])
        }])
        .unwrap();

    {
        let manager = manager_with(
            &store,
            Arc::new(CountingCliRunner::default()),
            &settings,
            Some(throttled_usage_reader.clone()),
            Some(throttled_clock.clone()),
        );
        manager.initialize().unwrap();
        wait_until(Duration::from_secs(3), || throttled_usage_reader.read_count(CliKind::Claude) >= 2);
        assert_eq!(
            throttled_usage_reader.read_count(CliKind::Claude),
            2,
            "啟動探測與執行後捕捉新倒數應各讀取一次 Claude 額度。"
        );

        wait_until(Duration::from_secs(3), || manager.get_jobs().unwrap()[0].status == ScheduleStatus::Pending);
        let throttled_job = manager.get_jobs().unwrap().remove(0);
        for _ in 0..3 {
            manager.upsert(&throttled_job).unwrap();
            sleep(100);
        }
        assert_eq!(
            throttled_usage_reader.read_count(CliKind::Claude),
            2,
            "10 分鐘探測間隔內即使排程器被喚醒多次，也不應重查 Claude 額度。"
        );

        // 推進 11 分鐘，因為仍處於未倒數狀態，應觸發定期探測
        throttled_clock.set(throttled_now + Span::minutes(11));
        manager.upsert(&throttled_job).unwrap();
        wait_until(Duration::from_secs(3), || throttled_usage_reader.read_count(CliKind::Claude) >= 3);
        assert!(
            throttled_usage_reader.read_count(CliKind::Claude) >= 3,
            "超過使用者設定的 10 分鐘後，應允許下一次 Claude 額度探測。"
        );
    }

    // 4. 倒數結束時（Countdown Ended）自動喚醒並探測
    let countdown_end_now = local_wall_clock(2026, 8, 21, 9, 0, 0);
    let countdown_end_clock = FakeClock::new(countdown_end_now);
    let countdown_end_reader = Arc::new(FakeCliUsageReader::default());
    let countdown_end_runner = Arc::new(CountingCliRunner::default());

    // 模擬 Claude 倒數即將在 15 分鐘後（09:15）結束
    let reset_point = countdown_end_now + Span::minutes(15);
    countdown_end_reader.set(available(
        CliKind::Claude,
        vec![window("Claude（5 小時）", 10, h5, reset_point, true)],
        countdown_end_now,
    ));
    store
        .save(&vec![ScheduledJob {
            scheduled_at: reset_point,
            finished_at: Some(countdown_end_now - Span::hours(1)),
            ..interval_job("倒數結束自動喚醒測試", &[CliKind::Claude])
        }])
        .unwrap();

    {
        let manager = manager_with(
            &store,
            countdown_end_runner.clone(),
            &settings,
            Some(countdown_end_reader.clone()),
            Some(countdown_end_clock.clone()),
        );
        manager.initialize().unwrap();
        wait_until(Duration::from_secs(3), || countdown_end_reader.read_count(CliKind::Claude) >= 1);
        sleep(150);
        assert_eq!(countdown_end_reader.read_count(CliKind::Claude), 1, "啟動初始化探測。");
        assert_eq!(countdown_end_runner.call_count(), 0, "倒數中不應喚醒。");

        // 時間推進至倒數剛結束（09:16），且伺服器已更新為未倒數（額度已重置）
        countdown_end_reader.set(available(
            CliKind::Claude,
            vec![window("Claude（5 小時）", 0, h5, reset_point, false)],
            countdown_end_now + Span::minutes(16),
        ));
        countdown_end_clock.set(countdown_end_now + Span::minutes(16));
        // 喚醒排程器評估
        let countdown_job = manager.get_jobs().unwrap().remove(0);
        manager.upsert(&countdown_job).unwrap();

        wait_until(Duration::from_secs(5), || countdown_end_runner.call_count() >= 1);
        assert_eq!(countdown_end_runner.call_count(), 1, "倒數結束後，排程器應自動探測並喚醒已重置的 Claude。");
        assert!(countdown_end_reader.read_count(CliKind::Claude) >= 2, "倒數結束與執行前/後應觸發額度讀取。");
    }
}

#[test]
fn schedule_manager_countdown() {
    let _serial = serial();
    let directory = TempDir::new();
    let store = job_store(directory.join("jobs.json"));
    let clock = FakeClock::new(local_wall_clock(2026, 10, 7, 7, 59, 0));
    let runner = Arc::new(CountingCliRunner::default());
    let usage_reader = Arc::new(FakeCliUsageReader::default());
    let settings = shared_settings();
    settings.lock().unwrap().quota_auto_refresh_minutes = 0;
    store
        .save(&vec![ScheduledJob {
            recurrence: ScheduleRecurrence::Countdown,
            initial_time_of_day: Some(Span::hours(8)),
            scheduled_at: local_wall_clock(2026, 10, 6, 8, 0, 0),
            working_directory: directory.text(),
            targets: vec![CliKind::Codex, CliKind::Claude],
            ..ScheduledJob::default()
        }])
        .unwrap();

    let manager = manager_with(&store, runner.clone(), &settings, Some(usage_reader.clone()), Some(clock.clone()));
    manager.initialize().unwrap();
    let scan = |day: u32, hour: u32, minute: u32| {
        clock.set(local_wall_clock(2026, 10, day, hour, minute, 0));
        manager.scan_countdown_jobs().unwrap();
        sleep(120);
    };
    let snapshot = |kind: CliKind, active: bool, reset_day: u32, reset_hour: u32| {
        usage_reader.set(CliUsageSnapshot::new(
            kind,
            CliUsageAvailability::Available,
            vec![window(
                "5h",
                if active { 10 } else { 0 },
                Span::hours(5),
                local_wall_clock(2026, 10, reset_day, reset_hour, 0, 0),
                active,
            )],
            "test",
            clock.get(),
        ));
    };

    snapshot(CliKind::Codex, false, 7, 8);
    snapshot(CliKind::Claude, true, 7, 13);
    scan(7, 7, 59);
    assert!(runner.call_count() == 0 && usage_reader.read_count(CliKind::Codex) == 0, "開始前不能偵測或補做昨日呼叫。");
    scan(7, 8, 0);
    assert!(
        runner.call_count() == 1 && runner.executed() == [CliKind::Codex],
        "開始時立刻只喚醒未倒數者，即使全域刷新關閉。"
    );
    scan(7, 8, 1);
    assert_eq!(runner.call_count(), 1, "倒數尚未反映時不能連發。");
    snapshot(CliKind::Codex, true, 7, 13);
    scan(7, 8, 3);
    assert_eq!(runner.call_count(), 1, "倒數中必須等待。");
    scan(7, 13, 0);
    assert_eq!(runner.call_count(), 3, "倒數結束必須再次喚醒兩個目標。");
    snapshot(CliKind::Codex, true, 7, 18);
    snapshot(CliKind::Claude, true, 7, 18);
    scan(7, 13, 3);
    assert_eq!(runner.call_count(), 3, "新一輪倒數中不能喚醒。");
    scan(7, 18, 0);
    assert_eq!(runner.call_count(), 5, "當日持續循環，不能僅呼叫一次。");
    snapshot(CliKind::Codex, true, 8, 0);
    snapshot(CliKind::Claude, true, 8, 0);
    scan(7, 23, 59);
    assert_eq!(runner.call_count(), 5, "接近午夜仍應等待有效倒數。");
    scan(8, 0, 0);
    scan(8, 7, 59);
    assert_eq!(runner.call_count(), 5, "午夜及翌日開始前不能呼叫。");
    scan(8, 8, 0);
    assert_eq!(runner.call_count(), 7, "翌日指定時間應重新開始。");
    usage_reader.set(CliUsageSnapshot::new(
        CliKind::Codex,
        CliUsageAvailability::Unavailable,
        Vec::new(),
        "offline",
        clock.get(),
    ));
    snapshot(CliKind::Claude, true, 8, 13);
    scan(8, 8, 3);
    assert_eq!(runner.call_count(), 7, "讀取失敗不能誤認沒有倒數。");

    let persisted = store.load().unwrap().remove(0);
    assert!(
        persisted.recurrence == ScheduleRecurrence::Countdown && persisted.initial_time_of_day == Some(Span::hours(8)),
        "持久化必須保留新模式與開始時間。"
    );
    manager.shutdown();

    let restart_runner = Arc::new(CountingCliRunner::default());
    snapshot(CliKind::Codex, true, 8, 13);
    {
        let restarted =
            manager_with(&store, restart_runner.clone(), &settings, Some(usage_reader.clone()), Some(clock.clone()));
        restarted.initialize().unwrap();
        sleep(150);
        assert_eq!(restart_runner.call_count(), 0, "當日重啟仍在倒數時不可強制首次呼叫。");
    }

    // 額度查詢在 23:59 開始、隔日才回來，不得發出昨日的喚醒。
    clock.set(local_wall_clock(2026, 10, 8, 23, 59, 0));
    snapshot(CliKind::Codex, false, 8, 23);
    snapshot(CliKind::Claude, false, 8, 23);
    let midnight_clock = clock.clone();
    *usage_reader.on_read.lock().unwrap() =
        Some(Box::new(move || midnight_clock.set(local_wall_clock(2026, 10, 9, 0, 0, 0))));
    {
        let midnight =
            manager_with(&store, restart_runner.clone(), &settings, Some(usage_reader.clone()), Some(clock.clone()));
        midnight.initialize().unwrap();
        sleep(150);
        assert_eq!(restart_runner.call_count(), 0, "查詢跨午夜必須在呼叫前再次檢查日期。");
    }
}

#[test]
fn installer_contract() {
    let mut directory = Some(Path::new(env!("CARGO_MANIFEST_DIR")));
    while let Some(candidate) = directory {
        if candidate.join("build-installer.ps1").is_file() {
            break;
        }
        directory = candidate.parent();
    }
    let root = directory.expect("找不到 AI Wake Scheduler repository root。");

    let scripts: Vec<PathBuf> = std::fs::read_dir(root.join("installer"))
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "iss"))
        .collect();
    assert_eq!(scripts.len(), 1, "installer 目錄應只有一個 .iss。");
    let script = std::fs::read_to_string(&scripts[0]).unwrap();
    let script = script.trim_start_matches('\u{feff}');
    let lines: Vec<&str> = script.lines().filter(|line| !line.is_empty()).collect();
    let single = |predicate: &dyn Fn(&str) -> bool| -> &str {
        let matches: Vec<&&str> = lines.iter().filter(|line| predicate(line)).collect();
        assert_eq!(matches.len(), 1, "Installer 設定應恰好出現一次。");
        matches[0]
    };

    for directive in [
        "AllowNoIcons=no",
        "DisableProgramGroupPage=yes",
        "UsePreviousAppDir=yes",
        "UsePreviousGroup=yes",
        "UsePreviousTasks=yes",
        "CreateUninstallRegKey=yes",
        "SetupLogging=yes",
        "UninstallLogging=yes",
        "RestartApplications=no",
    ] {
        assert!(lines.contains(&directive), "Installer 缺少必要設定：{directive}");
    }

    let start_menu_shortcut = single(&|line| line.starts_with("Name: \"{group}\\{#MyAppName}\";"));
    assert!(!start_menu_shortcut.contains("Tasks:"), "開始功能表捷徑必須固定建立。");
    assert!(start_menu_shortcut.contains("WorkingDir: \"{app}\""), "開始功能表捷徑缺少工作目錄。");
    assert!(start_menu_shortcut.contains("AppUserModelID:"), "開始功能表捷徑缺少 AppUserModelID。");

    let uninstall_shortcut = single(&|line| line.contains("{cm:UninstallProgram,{#MyAppName}}"));
    assert!(!uninstall_shortcut.contains("Tasks:"), "解除安裝捷徑必須固定建立。");

    let desktop_task = single(&|line| line.starts_with("Name: \"desktopicon\";"));
    let startup_task = single(&|line| line.starts_with("Name: \"startupicon\";"));
    assert!(desktop_task.contains("Flags: unchecked"), "桌面捷徑必須預設不勾選。");
    assert!(startup_task.contains("Flags: unchecked"), "開機啟動必須預設不勾選。");
}

/// opt-in：需要本機已安裝並登入四個 CLI。
#[test]
#[ignore = "需要本機已安裝並登入的 CLI"]
fn executable_locator_resolution() {
    let working_dir = std::env::var("USERPROFILE").unwrap();
    let cancel = CancelToken::none();
    let named = |name: &str| CliProfile { executable: name.to_string(), ..CliProfile::default() };

    // 1. 測試解析本機已安裝的 CLI（四者均應成功解析）
    for (kind, name) in [
        (CliKind::Antigravity, "agy"),
        (CliKind::AntigravityClaude, "agy"),
        (CliKind::Codex, "codex"),
        (CliKind::Claude, "claude"),
    ] {
        let resolved = locator::resolve(kind, Some(name), &working_dir);
        assert!(
            resolved.as_deref().is_some_and(|path| Path::new(path).is_file()),
            "{kind} 應成功解析到可執行檔：{resolved:?}"
        );
    }

    // 2. 測試明確絕對路徑
    let codex = locator::resolve(CliKind::Codex, Some("codex"), &working_dir).unwrap();
    assert_eq!(locator::resolve(CliKind::Codex, Some(&codex), &working_dir).as_deref(), Some(codex.as_str()));

    // 3. 測試相對路徑
    let temp_dir = TempDir::new();
    let dummy_exe = temp_dir.join("custom_app.exe");
    std::fs::write(&dummy_exe, "dummy").unwrap();
    let relative = locator::resolve(CliKind::Codex, Some("custom_app.exe"), &temp_dir.text()).unwrap();
    assert_eq!(Path::new(&relative), dummy_exe, "傳入工作目錄中的相對路徑應成功解析。");

    // 4. 測試真實 CLI probe 與即時額度讀取（唯讀查詢，不會建立模型回合或消耗 Token）
    let runner = ProcessCliRunner::new(AppDataPaths::new(Some(&temp_dir.join("data").to_string_lossy())));
    for (kind, name) in [
        (CliKind::Antigravity, "agy"),
        (CliKind::AntigravityClaude, "agy"),
        (CliKind::Codex, "codex"),
        (CliKind::Claude, "claude"),
    ] {
        let probe = runner.probe(kind, &named(name), &working_dir, &cancel);
        assert!(probe.succeeded, "{kind} Probe 應成功：{}", probe.summary);

        let snapshot = CliUsageReader::new().read(kind, &named(name), &working_dir, &cancel).unwrap();
        println!(
            "  {kind} Usage: {:?}, Windows={}, Message={}",
            snapshot.availability,
            snapshot.windows.len(),
            snapshot.message
        );
        for window in &snapshot.windows {
            println!("  -> {}: 剩餘 {}%, 重置時間: {:?}", window.name, window.remaining_percent(), window.resets_at);
        }
        assert_eq!(
            snapshot.availability,
            CliUsageAvailability::Available,
            "{kind} 即時額度應可讀取：{}",
            snapshot.message
        );
        assert!(!snapshot.windows.is_empty(), "{kind} 額度回應應至少有一個視窗。");
        assert!(snapshot.windows.iter().all(|window| (0..=100).contains(&window.remaining_percent())));
    }
}

/// 明確 opt-in：會真的建立四個最小模型回合，使用產品 Runner／參數／日誌路徑（會消耗少量 Token）。
#[test]
#[ignore = "會呼叫真實 CLI 並消耗少量 Token"]
fn real_cli_wake_smoke() {
    let directory = TempDir::new();
    let runner = Arc::new(ProcessCliRunner::new(AppDataPaths::new(Some(&directory.join("data").to_string_lossy()))));
    let cases = [
        (CliKind::Antigravity, "gemini-3.8-flash"),
        (CliKind::AntigravityClaude, "claude-sonnet-5-5-low"),
        (CliKind::Codex, "gpt-6-luna"),
        (CliKind::Claude, "claude-haiku-4-5-20251001"),
    ];
    let handles: Vec<_> = cases
        .into_iter()
        .map(|(kind, model)| {
            assert_eq!(catalog::get(kind).default_model, Some(model), "{kind} 應測試真正的預設模型。");
            let runner = runner.clone();
            let working_directory = directory.text();
            std::thread::spawn(move || {
                let output = Mutex::new(String::new());
                let result = runner.run(
                    kind,
                    &CliProfile::default(),
                    "只回 OK",
                    &working_directory,
                    Duration::from_secs(120),
                    true,
                    &CancelToken::none(),
                    Some(&|is_error, chunk| {
                        if !is_error {
                            output.lock().unwrap().push_str(chunk);
                        }
                    }),
                );
                println!("  {kind} / {model}: Exit={:?}, Success={}", result.exit_code, result.succeeded);
                (kind, result, output.into_inner().unwrap())
            })
        })
        .collect();
    for handle in handles {
        let (kind, result, output) = handle.join().unwrap();
        assert!(result.succeeded, "{kind} 實際喚醒失敗：{}", result.error);
        assert!(output.contains("OK"), "{kind} 應回覆 OK。");
    }
}
