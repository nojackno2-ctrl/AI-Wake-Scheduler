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
use aiwake_core::debug_log;
use aiwake_core::locator;
use aiwake_core::login;
use aiwake_core::process;
use aiwake_core::time::{self, local_wall_clock};
use aiwake_core::tokenizer;
use aiwake_core::usage::{self, is_counting_down};
use aiwake_core::*;
use chrono::Timelike;

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
        strings(&["--model", "gemini-4-argon", "--print", "早安"]),
        build_args(CliKind::AntigravityArgon, None, false, None)
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
    let codex_6_luna_ultra = build_profile(CliKind::Codex, "早安", "gpt-6-luna", E::Ultra, "", false);
    assert!(has(&codex_6_luna_ultra, "model_reasoning_effort=\"max\""), "GPT-6 Luna 的 Ultra 應降為支援的 Max。");

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
    for agy_kind in [CliKind::Antigravity, CliKind::AntigravityArgon, CliKind::AntigravityClaude] {
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

    let codex_model_override =
        build_profile(CliKind::Codex, "早安", "gpt-5.6-sol", E::Ultra, "--model gpt-6-luna", false);
    assert_eq!(
        codex_model_override.iter().filter(|a| *a == "--model").count(),
        1,
        "額外參數覆寫模型時不應重複附加 --model。"
    );
    assert!(
        has(&codex_model_override, "model_reasoning_effort=\"max\""),
        "Effort 應依額外參數實際指定的 GPT-6 Luna 正規化為 Max。"
    );

    let saver_agy = build_args(CliKind::Antigravity, None, true, Some(Duration::from_secs(180)));
    assert!(has(&saver_agy, "--effort") && has(&saver_agy, "low"), "Antigravity 節省模式應包含 low effort。");
    assert!(has(&saver_agy, "--disable-slash-commands"), "Antigravity 節省模式應停用斜線指令。");
    assert!(!has(&saver_agy, "--mode"), "Antigravity 節省模式停用斜線指令時不可帶 --mode plan 避免警告。");
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
    assert!(!has(&saver_agy_claude, "--mode"), "AntigravityClaude 節省模式停用技能展開時不可帶 --mode plan 避免警告。");
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
        has(&saver_claude, "claude-haiku-5-5") && has(&saver_claude, "--effort") && has(&saver_claude, "low"),
        "Claude 預設應使用最便宜的 Haiku 5.5 並以低推理執行。"
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
        ["", "gpt-6.1-sol", "gpt-6-astra", "gpt-6-sol", "gpt-6-luna", "gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna"]
    );
    assert!(
        !codex.preset_models.iter().any(|model| model.contains("gpt-5.2") || model.contains("gpt-5.1")),
        "Codex 推薦清單不應再包含 deprecated 的 GPT-5.2/5.1 Codex 模型。"
    );
    assert!(codex.get_supported_efforts(Some("gpt-5.6-sol")).contains(&E::Ultra), "GPT-5.6 Sol 應支援 Ultra。");
    assert!(!codex.get_supported_efforts(Some("gpt-5.6-luna")).contains(&E::Ultra), "GPT-5.6 Luna 不支援 Ultra。");
    assert_eq!(codex.normalize_effort(Some("gpt-6-luna"), E::Ultra), E::Max, "GPT-6 Luna Ultra 應正規化為 Max。");
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
    let mut removed_codex = AppSettings::create_default();
    removed_codex.cli_profiles.get_mut(&CliKind::Codex).unwrap().model = "gpt-5.5".to_string();
    removed_codex.ensure_defaults();
    assert_eq!(removed_codex.cli_profiles[&CliKind::Codex].model, "gpt-6.1-sol", "已下架的 gpt-5.5 應轉換到 6.1 Sol。");

    // 舊設定檔（沒有 DefaultModelsRevision）的 Haiku 4.5 只遷移一次到更便宜的 Haiku 5.5。
    let mut legacy_haiku = AppSettings::from_json(&serde_json::json!({
        "CliProfiles": { "Claude": { "Executable": "claude", "Model": "claude-haiku-4-5-20251001" } }
    }))
    .unwrap();
    legacy_haiku.ensure_defaults();
    assert_eq!(legacy_haiku.cli_profiles[&CliKind::Claude].model, "claude-haiku-5-5", "舊預設應遷移到 Haiku 5.5。");
    legacy_haiku.cli_profiles.get_mut(&CliKind::Claude).unwrap().model = "claude-haiku-4-5-20251001".to_string();
    let mut reloaded = AppSettings::from_json(&legacy_haiku.to_json()).unwrap();
    reloaded.ensure_defaults();
    assert_eq!(
        reloaded.cli_profiles[&CliKind::Claude].model,
        "claude-haiku-4-5-20251001",
        "遷移後使用者再選 Haiku 4.5 不應被覆寫。"
    );
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
    assert!(
        !agy.preset_models.iter().any(|model| model.contains("argon")),
        "Argon 已獨立成目標，Gemini 推薦清單不應再列出 Argon。"
    );
    let argon = catalog::get(CliKind::AntigravityArgon);
    assert_eq!(argon.default_model, Some("gemini-4-argon"), "Argon 目標預設應使用 gemini-4-argon。");
    assert_eq!(argon.default_command, "agy", "Argon 目標應呼叫 agy。");
    assert!(argon.get_supported_efforts(Some("gemini-4-argon")).contains(&E::High), "gemini-4-argon 應支援 High。");
    assert_eq!(
        argon.get_supported_efforts(Some("gemini-4-argon-high")),
        [E::Default],
        "gemini-4-argon-high 應抑制額外 --effort 避免衝突。"
    );
    let argon_low = build_profile(CliKind::AntigravityArgon, "OK", "gemini-4-argon", E::Low, "", true);
    assert!(
        has(&argon_low, "gemini-4-argon") && has(&argon_low, "--effort") && has(&argon_low, "low"),
        "Argon 基底模型應附加 --effort low。"
    );
    assert_eq!(catalog::kinds()[1], CliKind::AntigravityArgon, "Argon 應緊接在 Gemini 之後顯示。");
    assert_eq!(
        CliKind::ALL[..4],
        [CliKind::Antigravity, CliKind::AntigravityClaude, CliKind::Codex, CliKind::Claude],
        "舊種類的序號不可變動，否則舊版以整數保存的目標會錯位。"
    );
    assert!(
        AppSettings::create_default().cli_profiles.contains_key(&CliKind::AntigravityArgon),
        "預設設定應包含 Argon 設定。"
    );
    assert!(agy.get_supported_efforts(Some("gemini-3.8-flash")).contains(&E::High), "gemini-3.8-flash 應支援 High。");
    assert_eq!(
        agy.get_supported_efforts(Some("gemini-3.8-flash-high")),
        [E::Default],
        "gemini-3.8-flash-high 應抑制額外 --effort 避免衝突。"
    );

    for kind in [CliKind::Antigravity, CliKind::AntigravityArgon, CliKind::Codex, CliKind::Claude] {
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

    // 喚醒後 usedPercent 為 0，但重置時間固定（早於 現在 + 5 小時）仍屬倒數中；
    // 未開始的視窗重置時間約為 現在 + 5 小時，不可誤判。
    let started_reset = (observed_at + Span::minutes(285)).timestamp();
    let idle_reset = (observed_at + Span::minutes(300)).timestamp();
    let codex_zero = |reset: i64| {
        format!(
            r#"{{"id":1,"result":{{"rateLimitsByLimitId":{{"codex":{{"limitId":"codex","primary":{{"usedPercent":0,"windowDurationMins":300,"resetsAt":{reset}}}}}}}}}}}"#
        )
    };
    let started = usage::parse_codex_rate_limits(&codex_zero(started_reset), &observed_at).unwrap();
    assert!(started.windows[0].is_active_countdown, "已用 0% 但重置時間固定，應判定為倒數中。");
    assert!(usage::is_counting_down(Some(&started), &observed_at));
    let idle = usage::parse_codex_rate_limits(&codex_zero(idle_reset), &observed_at).unwrap();
    assert!(!idle.windows[0].is_active_countdown, "重置時間等於 現在 + 視窗長度，應判定為未倒數。");

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
            {{"name":"Five Hour Limit Remaining","window":"5h","remaining_fraction":1,"reset_time":"{}"}}]}},
          {{"name":"Gemini 4 Argon","buckets":[
            {{"name":"Gemini 4 Argon Weekly Limit","window":"weekly","remaining_fraction":0.89,"reset_time":"{}"}}]}}]}}"#,
        iso(observed_at + Span::hours(2)),
        iso(observed_at + Span::days(3)),
        iso(observed_at + Span::hours(5)),
        iso(observed_at + Span::days(6)),
    );
    let usage_gemini = usage::parse_antigravity_model_configs(&usage_data, CliKind::Antigravity, &observed_at);
    assert!(
        usage_gemini.windows.len() == 2
            && usage_gemini.windows[0].duration == Some(Span::hours(5))
            && usage_gemini.windows[1].duration == Some(Span::days(7))
            && usage_gemini.windows[1].used_percent == 20,
        "Gemini 額度池應只有 5h 與 Gemini weekly，不可混入 Argon。"
    );
    assert!(!usage_gemini.windows.iter().any(|window| window.name.contains("Argon")), "Gemini 不應含 Argon 視窗。");
    let usage_argon = usage::parse_antigravity_model_configs(&usage_data, CliKind::AntigravityArgon, &observed_at);
    assert!(
        usage_argon.availability == CliUsageAvailability::Available
            && usage_argon.windows.len() == 1
            && usage_argon.windows[0].duration == Some(Span::days(7))
            && usage_argon.windows[0].remaining_percent() == 89
            && usage_argon.windows[0].name.starts_with("Antigravity (Argon)"),
        "Argon 額度池應獨立解析出每週 89% 的視窗。"
    );
    assert!(is_counting_down(Some(&usage_argon), &observed_at), "Argon 已使用時應倒數至每週重置。");
    // 部分版本把 Argon bucket 放在 Gemini 群組內；仍需依 bucket 名稱分流。
    let nested_argon = format!(
        r#"{{"groups":[{{"name":"Gemini Models","buckets":[
            {{"name":"Weekly Limit Remaining","window":"weekly","remaining_fraction":0.5,"reset_time":"{}"}},
            {{"name":"Gemini 4 Argon Weekly Limit","window":"weekly","remaining_fraction":0.7,"reset_time":"{}"}}]}}]}}"#,
        iso(observed_at + Span::days(2)),
        iso(observed_at + Span::days(4)),
    );
    let nested_gemini = usage::parse_antigravity_model_configs(&nested_argon, CliKind::Antigravity, &observed_at);
    let nested_only_argon =
        usage::parse_antigravity_model_configs(&nested_argon, CliKind::AntigravityArgon, &observed_at);
    assert!(
        nested_gemini.windows.len() == 1 && nested_gemini.windows[0].remaining_percent() == 50,
        "Gemini 群組內的 Argon bucket 不可蓋掉 Gemini 每週額度。"
    );
    assert!(
        nested_only_argon.windows.len() == 1 && nested_only_argon.windows[0].remaining_percent() == 70,
        "Gemini 群組內的 Argon bucket 應歸給 Argon。"
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
    let start = hm(8, 30);
    let end = hm(18, 0);
    for (hour, minute, active) in [(8, 29, false), (8, 30, true), (17, 59, true), (18, 0, false), (23, 59, false)] {
        let now = local_wall_clock(2026, 10, 22, hour, minute, 0);
        assert_eq!(calculator::in_detection_window(start, end, &now), active);
    }
    for (hour, active) in [(0, true), (5, true), (6, false), (12, false), (21, false), (22, true), (23, true)] {
        assert_eq!(
            calculator::in_detection_window(hm(22, 0), hm(6, 0), &local_wall_clock(2026, 10, 22, hour, 0, 0)),
            active
        );
    }
    let evening = local_wall_clock(2026, 10, 22, 17, 55, 0);
    assert_eq!(
        calculator::next_detection(start, end, &evening, Span::minutes(10)).unwrap(),
        local_wall_clock(2026, 10, 23, 8, 30, 0)
    );
    let midnight = local_wall_clock(2026, 10, 23, 0, 0, 0);
    assert!(!calculator::in_detection_window(start, Span::days(1), &midnight));
    assert!(calculator::in_detection_window(Span::zero(), Span::days(1), &midnight));
    assert!(calculator::validate_window(start, start).is_err());
    assert!(calculator::validate_window(start, Span::hours(25)).is_err());
    assert!(calculator::validate_window(Span::hours(-1), end).is_err());
    assert!(calculator::validate_window(start, end + Span::seconds(1)).is_err());
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
    assert_eq!(settings.cli_profiles.len(), CliKind::ALL.len(), "ensure_defaults 應補齊缺少的 CLI 設定。");
    assert_eq!(
        settings.cli_profiles[&CliKind::AntigravityArgon].model,
        "gemini-4-argon",
        "舊設定檔升級後應補上 Argon 預設設定。"
    );
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

/// 喚醒背景掃描而不重設受測排程的探測時間。
fn wake_scheduler(manager: &ScheduleManager, directory: &TempDir) {
    manager.upsert(&ScheduledJob { enabled: false, ..job("喚醒掃描", directory, &[CliKind::Codex]) }).unwrap();
}

fn ready_usage(reader: &FakeCliUsageReader, kind: CliKind, now: Moment) {
    reader.set(available(kind, vec![CliUsageWindow::new("5h", 0, Some(Span::hours(5)), None, false)], now));
}

fn timed_job(directory: &TempDir, kind: CliKind, start: Span, end: Span, now: Moment) -> ScheduledJob {
    ScheduledJob {
        initial_time_of_day: Some(start),
        end_time_of_day: end,
        scheduled_at: now,
        ..job("每日時段", directory, &[kind])
    }
}

#[test]
fn schedule_manager_manual_parallel_run() {
    let _serial = serial();
    let directory = TempDir::new();
    let paths = AppDataPaths::new(Some(&directory.join("data").to_string_lossy()));
    let store = job_store(paths.jobs_file());
    let settings = shared_settings();
    for &kind in CliKind::ALL {
        settings
            .lock()
            .unwrap()
            .cli_profiles
            .insert(kind, fake_profile(&directory.join(&format!("{kind}.json")), 1000, 0));
    }
    let manager = manager_with(&store, Arc::new(ProcessCliRunner::new(paths)), &settings, None, None);
    manager.initialize().unwrap();
    let item = ScheduledJob { enabled: false, ..job("手動平行", &directory, CliKind::ALL) };
    manager.upsert(&item).unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = events.clone();
    manager.on_activity_logged(move |event| sink.lock().unwrap().push(event.clone()));
    let stopwatch = Instant::now();
    manager.run_now(item.id).unwrap();
    assert!(wait_until(Duration::from_secs(20), || manager
        .get_jobs()
        .unwrap()
        .iter()
        .any(|job| job.id == item.id && job.last_results.len() == CliKind::ALL.len())));
    assert!(stopwatch.elapsed() < Duration::from_millis(4500));
    let finished = manager.get_jobs().unwrap().into_iter().find(|job| job.id == item.id).unwrap();
    assert!(finished.last_results.iter().all(|result| result.succeeded));
    assert_eq!(finished.status, ScheduleStatus::Pending);
    assert!(finished.enabled);
    for &kind in CliKind::ALL {
        assert!(directory.join(&format!("{kind}.json")).is_file());
        let events = events.lock().unwrap();
        assert!(events.iter().any(|event| event.cli == kind && event.kind == CliActivityKind::Started));
        assert!(events.iter().any(|event| event.cli == kind && event.kind == CliActivityKind::Completed));
    }
    manager.delete(item.id).unwrap();
    manager.shutdown();
    assert!(manager.get_jobs().is_err());
}

#[test]
fn schedule_manager_obeys_daily_boundaries_and_resets() {
    let _serial = serial();
    let directory = TempDir::new();
    let store = job_store(directory.join("jobs.json"));
    let settings = shared_settings();
    let before = local_wall_clock(2026, 10, 22, 7, 59, 0);
    let clock = FakeClock::new(before);
    let reader = Arc::new(FakeCliUsageReader::default());
    ready_usage(&reader, CliKind::Codex, before);
    let runner = Arc::new(CountingCliRunner::default());
    store.save(&vec![timed_job(&directory, CliKind::Codex, hm(8, 0), hm(18, 0), before)]).unwrap();
    let manager = manager_with(&store, runner.clone(), &settings, Some(reader.clone()), Some(clock.clone()));
    manager.initialize().unwrap();
    sleep(150);
    assert_eq!(reader.read_count(CliKind::Codex), 0, "開始前不可偵測或呼叫。");
    let start = local_wall_clock(2026, 10, 22, 8, 0, 0);
    clock.set(start);
    wake_scheduler(&manager, &directory);
    assert!(wait_until(Duration::from_secs(3), || runner.call_count() == 1));
    assert!(wait_until(Duration::from_secs(3), || reader.read_count(CliKind::Codex) >= 2));
    reader.set(available(CliKind::Codex, vec![window("5h", 10, Span::hours(5), start + Span::hours(5), true)], start));
    clock.set(start + Span::minutes(10));
    wake_scheduler(&manager, &directory);
    assert!(wait_until(Duration::from_secs(3), || reader.read_count(CliKind::Codex) >= 3));
    assert_eq!(runner.call_count(), 1, "倒數中不呼叫。");
    let reset = start + Span::hours(5);
    ready_usage(&reader, CliKind::Codex, reset);
    clock.set(reset);
    wake_scheduler(&manager, &directory);
    assert!(wait_until(Duration::from_secs(3), || runner.call_count() == 2), "倒數結束立即重讀並呼叫。");
    sleep(100);
    clock.set(local_wall_clock(2026, 10, 22, 18, 0, 0));
    wake_scheduler(&manager, &directory);
    sleep(150);
    assert_eq!(runner.call_count(), 2, "結束時間不包含在時段內。");
    clock.set(local_wall_clock(2026, 10, 23, 8, 0, 0));
    wake_scheduler(&manager, &directory);
    assert!(wait_until(Duration::from_secs(3), || runner.call_count() == 3), "隔天開始時間重新偵測。");
}

#[test]
fn schedule_manager_countdown_reset_preempts_ten_minute_probe() {
    let _serial = serial();
    let directory = TempDir::new();
    let store = job_store(directory.join("early-reset.json"));
    let now = local_wall_clock(2026, 10, 22, 8, 0, 0);
    let clock = FakeClock::new(now);
    let reader = Arc::new(FakeCliUsageReader::default());
    reader.set(available(CliKind::Codex, vec![window("5h", 10, Span::hours(5), now + Span::minutes(3), true)], now));
    let runner = Arc::new(CountingCliRunner::default());
    store.save(&vec![timed_job(&directory, CliKind::Codex, hm(8, 0), hm(18, 0), now)]).unwrap();
    let manager = manager_with(&store, runner.clone(), &shared_settings(), Some(reader.clone()), Some(clock.clone()));
    manager.initialize().unwrap();
    assert!(wait_until(Duration::from_secs(3), || reader.read_count(CliKind::Codex) == 1));
    assert_eq!(runner.call_count(), 0);
    clock.set(now + Span::minutes(1));
    wake_scheduler(&manager, &directory);
    sleep(100);
    assert_eq!(reader.read_count(CliKind::Codex), 1, "未到重讀時刻時不可密集查詢。");
    clock.set(now + Span::minutes(3));
    wake_scheduler(&manager, &directory);
    assert!(wait_until(Duration::from_secs(3), || runner.call_count() == 1), "08:03倒數結束，無須等到08:10。");
    assert!(reader.read_count(CliKind::Codex) >= 2, "必須重讀確認，而非直接依舊快照呼叫。");
}

#[test]
fn schedule_manager_reset_survives_ui_snapshot_refresh() {
    let _serial = serial();
    let directory = TempDir::new();
    let store = job_store(directory.join("ui-reset.json"));
    let now = local_wall_clock(2026, 10, 22, 8, 0, 0);
    let reset = now + Span::minutes(3);
    let clock = FakeClock::new(now);
    let reader = Arc::new(FakeCliUsageReader::default());
    reader.set(available(CliKind::Codex, vec![window("5h", 10, Span::hours(5), reset, true)], now));
    let runner = Arc::new(CountingCliRunner::default());
    store.save(&vec![timed_job(&directory, CliKind::Codex, hm(8, 0), hm(18, 0), now)]).unwrap();
    let manager = manager_with(&store, runner.clone(), &shared_settings(), Some(reader.clone()), Some(clock.clone()));
    manager.initialize().unwrap();
    assert!(wait_until(Duration::from_secs(3), || reader.read_count(CliKind::Codex) == 1));
    let refreshed =
        available(CliKind::Codex, vec![window("5h", 0, Span::hours(5), reset + Span::hours(5), false)], reset);
    reader.set(refreshed.clone());
    reader.latest.lock().unwrap().insert(CliKind::Codex, refreshed);
    clock.set(reset);
    wake_scheduler(&manager, &directory);
    assert!(wait_until(Duration::from_secs(3), || runner.call_count() == 1), "UI先更新觀測時間，仍應在08:03立即重讀。");
    assert!(reader.read_count(CliKind::Codex) >= 2);
}

#[test]
fn schedule_manager_all_jobs_are_quota_gated_and_deduplicated() {
    let _serial = serial();
    let directory = TempDir::new();
    let store = job_store(directory.join("jobs.json"));
    let settings = shared_settings();
    let now = local_wall_clock(2026, 10, 22, 10, 0, 0);
    let reader = Arc::new(FakeCliUsageReader::default());
    let runner = Arc::new(CountingCliRunner::default());
    reader.set(available(CliKind::Antigravity, vec![window("7d", 100, Span::days(7), now + Span::days(1), true)], now));
    ready_usage(&reader, CliKind::Codex, now);
    ready_usage(&reader, CliKind::Claude, now);
    reader.set(available(CliKind::AntigravityArgon, Vec::new(), now));
    store
        .save(&vec![
            timed_job(&directory, CliKind::Antigravity, hm(8, 0), hm(18, 0), now),
            timed_job(&directory, CliKind::Codex, hm(8, 0), hm(18, 0), now),
            timed_job(&directory, CliKind::Codex, hm(8, 0), hm(18, 0), now),
            timed_job(&directory, CliKind::Claude, hm(8, 0), hm(18, 0), now),
            timed_job(&directory, CliKind::AntigravityArgon, hm(8, 0), hm(18, 0), now),
            timed_job(&directory, CliKind::AntigravityClaude, hm(8, 0), hm(18, 0), now),
        ])
        .unwrap();
    let manager = manager_with(&store, runner.clone(), &settings, Some(reader.clone()), Some(FakeClock::new(now)));
    manager.initialize().unwrap();
    assert!(wait_until(Duration::from_secs(3), || runner.call_count() == 2));
    sleep(150);
    let executed = runner.executed();
    assert_eq!(executed.iter().filter(|&&kind| kind == CliKind::Codex).count(), 1, "重疊排程不可重複呼叫同一CLI。");
    assert!(executed.contains(&CliKind::Claude), "非首筆排程也須偵測並呼叫。");
    assert!(!executed.contains(&CliKind::Antigravity), "每週額度用盡不呼叫。");
    assert!(!executed.contains(&CliKind::AntigravityArgon), "空額度視窗不呼叫。");
    assert!(!executed.contains(&CliKind::AntigravityClaude), "讀取不可用不呼叫。");
}

#[test]
fn schedule_manager_cross_midnight_and_read_past_end() {
    let _serial = serial();
    let directory = TempDir::new();
    let settings = shared_settings();
    let now = local_wall_clock(2026, 10, 23, 1, 0, 0);
    let reader = Arc::new(FakeCliUsageReader::default());
    ready_usage(&reader, CliKind::Codex, now);
    let runner = Arc::new(CountingCliRunner::default());
    let store = job_store(directory.join("overnight.json"));
    store.save(&vec![timed_job(&directory, CliKind::Codex, hm(22, 0), hm(6, 0), now)]).unwrap();
    let manager = manager_with(&store, runner.clone(), &settings, Some(reader), Some(FakeClock::new(now)));
    manager.initialize().unwrap();
    assert!(wait_until(Duration::from_secs(3), || runner.call_count() == 1));
    manager.shutdown();
    let end = local_wall_clock(2026, 10, 23, 6, 0, 0);
    let clock = FakeClock::new(end - Span::seconds(1));
    let reader = Arc::new(FakeCliUsageReader::default());
    ready_usage(&reader, CliKind::Codex, end);
    let move_clock = clock.clone();
    *reader.on_read.lock().unwrap() = Some(Box::new(move || move_clock.set(end)));
    let runner = Arc::new(CountingCliRunner::default());
    let store = job_store(directory.join("slow-read.json"));
    store.save(&vec![timed_job(&directory, CliKind::Codex, hm(22, 0), hm(6, 0), end - Span::seconds(1))]).unwrap();
    let manager = manager_with(&store, runner.clone(), &settings, Some(reader.clone()), Some(clock));
    manager.initialize().unwrap();
    assert!(wait_until(Duration::from_secs(3), || reader.read_count(CliKind::Codex) == 1));
    sleep(100);
    assert_eq!(runner.call_count(), 0, "讀取完成時已到結束時間，不可啟動CLI。");
}

#[test]
fn schedule_manager_without_usage_reader_never_auto_calls() {
    let _serial = serial();
    let directory = TempDir::new();
    let settings = shared_settings();
    let now = local_wall_clock(2026, 10, 22, 10, 0, 0);
    let clock = FakeClock::new(now);
    let runner = Arc::new(CountingCliRunner::default());
    let store = job_store(directory.join("jobs.json"));
    let item = timed_job(&directory, CliKind::Codex, hm(8, 0), hm(18, 0), now - Span::days(1));
    store.save(&vec![item.clone()]).unwrap();
    let manager = manager_with(&store, runner.clone(), &settings, None, Some(clock.clone()));
    manager.initialize().unwrap();
    sleep(100);
    clock.set(now + Span::hours(6));
    wake_scheduler(&manager, &directory);
    sleep(100);
    assert_eq!(runner.call_count(), 0, "缺少讀取器不可退回固定間隔盲呼叫。");
    manager.run_now(item.id).unwrap();
    assert!(wait_until(Duration::from_secs(3), || runner.call_count() == 1), "明確手動呼叫仍可執行。");
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
    let overlapping = base("重疊目標", "早安");
    manager.upsert(&overlapping).unwrap();
    assert!(
        matches!(manager.run_now(overlapping.id), Err(CoreError::InvalidOperation(_))),
        "另一排程不可重疊呼叫執行中的CLI。"
    );

    // 9. 驗證 AppSettings.quota_auto_refresh_minutes 預設值與 clone / ensure_defaults
    let test_settings = AppSettings { quota_auto_refresh_minutes: 30, ..AppSettings::default() };
    assert_eq!(test_settings.normalized().quota_auto_refresh_minutes, 30, "副本應複製 quota_auto_refresh_minutes。");
    let mut invalid_settings = AppSettings { quota_auto_refresh_minutes: -5, ..AppSettings::default() };
    invalid_settings.ensure_defaults();
    assert_eq!(invalid_settings.quota_auto_refresh_minutes, 0, "ensure_defaults 應將負值 clamp 為 0。");
}

#[test]
fn schedule_legacy_seconds_and_end_time_roundtrip() {
    let directory = TempDir::new();
    let store = job_store(directory.join("legacy-seconds.json"));
    let now = local_wall_clock(2026, 10, 22, 6, 0, 0);
    let original_time = local_wall_clock(2026, 10, 22, 8, 49, 37);
    let mut legacy = ScheduledJob { enabled: false, scheduled_at: original_time, ..ScheduledJob::default() }.to_json();
    legacy.as_object_mut().unwrap().remove("InitialTimeOfDay");
    legacy.as_object_mut().unwrap().remove("EndTimeOfDay");
    legacy.as_object_mut().unwrap().remove("ScheduleRevision");
    std::fs::write(store.path(), serde_json::json!([legacy]).to_string()).unwrap();
    let manager = manager_with(
        &store,
        Arc::new(CountingCliRunner::default()),
        &shared_settings(),
        None,
        Some(FakeClock::new(now)),
    );
    manager.initialize().unwrap();
    manager.shutdown();
    let loaded = store.load().unwrap();
    assert_eq!(loaded[0].scheduled_at, original_time, "保留歷史排程秒數。");
    assert_eq!(loaded[0].initial_time_of_day, Some(hm(8, 49)));
    assert_eq!(loaded[0].end_time_of_day, Span::days(1), "缺少結束時間預設24:00。");
    let mut custom = loaded[0].clone();
    custom.end_time_of_day = hm(6, 0);
    store.save(&vec![custom.clone()]).unwrap();
    assert_eq!(store.load().unwrap(), vec![custom], "跨日時段可往返保存。");
}

#[test]
fn schedule_manager_rechecks_window_before_worker_starts() {
    let _serial = serial();
    let directory = TempDir::new();
    let store = job_store(directory.join("worker.json"));
    let end = local_wall_clock(2026, 10, 22, 18, 0, 0);
    let clock = FakeClock::new(end - Span::seconds(1));
    let reader = Arc::new(FakeCliUsageReader::default());
    ready_usage(&reader, CliKind::Codex, end - Span::seconds(1));
    let runner = Arc::new(CountingCliRunner::default());
    store.save(&vec![timed_job(&directory, CliKind::Codex, hm(8, 0), hm(18, 0), end - Span::seconds(1))]).unwrap();
    let manager = manager_with(&store, runner.clone(), &shared_settings(), Some(reader.clone()), Some(clock.clone()));
    let move_clock = clock.clone();
    let read = reader.clone();
    manager.on_jobs_changed(move || {
        if read.read_count(CliKind::Codex) > 0 {
            move_clock.set(end);
        }
    });
    manager.initialize().unwrap();
    assert!(wait_until(Duration::from_secs(3), || reader.read_count(CliKind::Codex) > 0));
    sleep(150);
    assert_eq!(runner.call_count(), 0, "排入執行後、工作執行緒啟動前到達結束時間，不可呼叫。");
    assert_eq!(reader.read_count(CliKind::Codex), 1, "略過呼叫後不可在結束時間外重讀。");
}

#[test]
fn schedule_manager_rechecks_window_after_start_event() {
    let _serial = serial();
    let directory = TempDir::new();
    let store = job_store(directory.join("launch.json"));
    let end = local_wall_clock(2026, 10, 22, 18, 0, 0);
    let clock = FakeClock::new(end - Span::seconds(1));
    let reader = Arc::new(FakeCliUsageReader::default());
    ready_usage(&reader, CliKind::Codex, end - Span::seconds(1));
    let runner = Arc::new(CountingCliRunner::default());
    store.save(&vec![timed_job(&directory, CliKind::Codex, hm(8, 0), hm(18, 0), end - Span::seconds(1))]).unwrap();
    let manager = manager_with(&store, runner.clone(), &shared_settings(), Some(reader.clone()), Some(clock.clone()));
    manager.on_activity_logged(move |event| {
        if event.kind == CliActivityKind::Started {
            clock.set(end);
        }
    });
    manager.initialize().unwrap();
    assert!(wait_until(Duration::from_secs(3), || reader.read_count(CliKind::Codex) > 0));
    sleep(150);
    assert_eq!(runner.call_count(), 0, "開始事件處理期間超過結束時間，真正呼叫前仍須攔下。");
    assert_eq!(reader.read_count(CliKind::Codex), 1, "略過呼叫後不可在結束時間外重讀。");
}

#[test]
fn schedule_legacy_json_migration_is_stable() {
    let directory = TempDir::new();
    let store = job_store(directory.join("legacy.json"));
    let now = local_wall_clock(2026, 10, 22, 6, 0, 0);
    let clock = FakeClock::new(now);
    let settings = shared_settings();
    let runner = Arc::new(CountingCliRunner::default());
    let mut source = Vec::new();
    // 所有舊週期與狀態都同時驗證名稱、整數及大小寫相容。
    for (index, recurrence) in ScheduleRecurrence::ALL.iter().enumerate() {
        for (status_index, status) in ScheduleStatus::ALL.iter().enumerate() {
            for numeric in [false, true] {
                let mut value = ScheduledJob {
                    name: format!("自訂 {index} {status_index} {numeric}"),
                    message: "保留訊息".to_string(),
                    working_directory: directory.text(),
                    scheduled_at: now + Span::hours(2),
                    recurrence: *recurrence,
                    enabled: status_index % 2 == 0,
                    status: *status,
                    started_at: Some(now - Span::hours(1)),
                    finished_at: Some(now - Span::minutes(30)),
                    last_results: vec![CliRunResult::new(CliKind::Codex)],
                    targets: vec![CliKind::Codex, CliKind::Claude],
                    ..ScheduledJob::default()
                }
                .to_json();
                let object = value.as_object_mut().unwrap();
                object.remove("ScheduleRevision");
                object.remove("InitialTimeOfDay");
                object.remove("EndTimeOfDay");
                object.insert(
                    "recurrence".to_string(),
                    if numeric {
                        serde_json::json!(index)
                    } else {
                        serde_json::json!(recurrence.name().to_ascii_lowercase())
                    },
                );
                object.remove("Recurrence");
                if numeric {
                    object.insert("Status".to_string(), serde_json::json!(status_index));
                    object.insert("Targets".to_string(), serde_json::json!([2, 3]));
                    object["LastResults"][0]["Cli"] = serde_json::json!(2);
                }
                source.push(value);
            }
        }
    }
    let text = format!("\u{feff}{}", serde_json::to_string(&source).unwrap());
    std::fs::write(store.path(), text).unwrap();
    let original = store.load().unwrap();
    {
        let manager = manager_with(&store, runner.clone(), &settings, None, Some(clock.clone()));
        manager.initialize().unwrap();
        let migrated = store.load().unwrap();
        for (before, after) in original.iter().zip(&migrated) {
            let mut expected = before.clone();
            expected.schedule_revision = aiwake_core::models::SCHEDULE_REVISION;
            expected.recurrence = ScheduleRecurrence::Interval;
            expected.initial_time_of_day = Some(hm(8, 0));
            if expected.status == ScheduleStatus::Running {
                expected.status = if expected.enabled { ScheduleStatus::Pending } else { ScheduleStatus::Disabled };
                expected.started_at = None;
            }
            assert_eq!(*after, expected, "只允許遷移版本、模式、開始時間與中斷狀態改變。");
        }
        manager.shutdown();
    }
    let saved = std::fs::read(store.path()).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&saved).unwrap();
    assert!(json.as_array().unwrap().iter().all(|value| {
        value["ScheduleRevision"] == aiwake_core::models::SCHEDULE_REVISION
            && value["Recurrence"] == "Interval"
            && value["Targets"] == serde_json::json!(["Codex", "Claude"])
    }));
    // 非標準空白可證明再次初始化沒有重新序列化，而非碰巧寫出相同內容。
    let unchanged = format!("  {}\n", String::from_utf8(saved).unwrap());
    std::fs::write(store.path(), &unchanged).unwrap();
    {
        let manager = manager_with(&store, runner.clone(), &settings, None, Some(clock));
        manager.initialize().unwrap();
        manager.shutdown();
    }
    assert_eq!(std::fs::read_to_string(store.path()).unwrap(), unchanged);
    assert_eq!(runner.call_count(), 0, "停用、完成、失敗或尚未到期的排程不得被遷移觸發。");
}

#[test]
fn schedule_migration_rejects_damage_without_overwriting() {
    let directory = TempDir::new();
    let store = job_store(directory.join("damaged.json"));
    let settings = shared_settings();
    let runner = Arc::new(CountingCliRunner::default());
    let valid = ScheduledJob { enabled: false, ..ScheduledJob::default() }.to_json();
    let mut cases = vec!["null".to_string(), "[".to_string(), "{}".to_string(), "[{}]".to_string()];
    for (field, invalid) in [
        ("ScheduleRevision", serde_json::json!(aiwake_core::models::SCHEDULE_REVISION + 1)),
        ("ScheduleRevision", serde_json::json!(-1)),
        ("Recurrence", serde_json::json!(99)),
        ("Recurrence", serde_json::json!("Unknown")),
        ("Status", serde_json::json!("Unknown")),
        ("Targets", serde_json::json!([2, 99])),
        ("Id", serde_json::json!("無效")),
        ("ScheduledAt", serde_json::json!("無效")),
        ("InitialTimeOfDay", serde_json::json!("24:00:00")),
        ("EndTimeOfDay", serde_json::json!("1.01:00:00")),
        ("EndTimeOfDay", serde_json::json!("無效")),
        ("EndTimeOfDay", serde_json::json!(null)),
        ("CustomData", serde_json::json!("不可遺失")),
        ("name", serde_json::json!("重複欄位")),
    ] {
        let mut broken = valid.clone();
        broken[field] = invalid;
        cases.push(serde_json::json!([valid, broken]).to_string());
    }
    for field in ["Id", "ScheduledAt"] {
        let mut broken = valid.clone();
        broken.as_object_mut().unwrap().remove(field);
        cases.push(serde_json::json!([valid, broken]).to_string());
    }
    cases.push(serde_json::json!([valid, valid]).to_string());
    for text in cases {
        std::fs::write(store.path(), &text).unwrap();
        let manager = manager_with(&store, runner.clone(), &settings, None, None);
        assert!(matches!(manager.initialize(), Err(CoreError::InvalidData(_))), "應拒絕：{text}");
        assert!(manager.get_jobs().unwrap().is_empty(), "不得發布部分成功的資料。");
        manager.shutdown();
        assert_eq!(std::fs::read_to_string(store.path()).unwrap(), text, "錯誤不得覆寫原檔。");
    }
    assert_eq!(runner.call_count(), 0);
}

#[test]
fn schedule_latest_restart_preserves_probe_time_and_terminal_history() {
    let directory = TempDir::new();
    let store = job_store(directory.join("latest.json"));
    let now = local_wall_clock(2026, 10, 22, 10, 0, 0);
    let runner = Arc::new(CountingCliRunner::default());
    let settings = shared_settings();
    let active = ScheduledJob {
        initial_time_of_day: Some(hm(8, 0)),
        scheduled_at: now + Span::minutes(5),
        finished_at: Some(now - Span::minutes(5)),
        ..ScheduledJob::default()
    };
    let mut jobs = vec![active];
    for enabled in [false, true] {
        for status in [ScheduleStatus::Completed, ScheduleStatus::Failed, ScheduleStatus::Disabled] {
            jobs.push(ScheduledJob {
                enabled,
                status,
                scheduled_at: now - Span::days(10),
                initial_time_of_day: Some(hm(8, 0)),
                finished_at: Some(now - Span::days(9)),
                ..ScheduledJob::default()
            });
        }
    }
    store.save(&jobs).unwrap();
    let before = std::fs::read(store.path()).unwrap();
    let manager = manager_with(&store, runner.clone(), &settings, None, Some(FakeClock::new(now)));
    manager.initialize().unwrap();
    manager.shutdown();
    assert_eq!(store.load().unwrap(), jobs);
    assert_eq!(std::fs::read(store.path()).unwrap(), before);
    assert_eq!(runner.call_count(), 0);
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

    // 1. 測試解析本機已安裝的 CLI（每個目標均應成功解析）
    for (kind, name) in [
        (CliKind::Antigravity, "agy"),
        (CliKind::AntigravityArgon, "agy"),
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
        (CliKind::AntigravityArgon, "agy"),
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
        (CliKind::AntigravityArgon, "gemini-4-argon"),
        (CliKind::AntigravityClaude, "claude-sonnet-5-5-low"),
        (CliKind::Codex, "gpt-6-luna"),
        (CliKind::Claude, "claude-haiku-5-5"),
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

/// 只喚醒 Argon：走與排程相同的 ProcessCliRunner，再重讀額度確認每週倒數已開始。
#[test]
#[ignore = "會呼叫真實 agy 並消耗少量 Argon 每週額度"]
fn real_argon_wake_smoke() {
    let directory = TempDir::new();
    let runner = ProcessCliRunner::new(AppDataPaths::new(Some(&directory.join("data").to_string_lossy())));
    let profile = CliProfile { executable: "agy".to_string(), ..CliProfile::default() };
    let working_directory = directory.text();
    let cancel = CancelToken::none();

    let output = Mutex::new(String::new());
    let result = runner.run(
        CliKind::AntigravityArgon,
        &profile,
        "只回 OK",
        &working_directory,
        Duration::from_secs(180),
        true,
        &cancel,
        Some(&|is_error, chunk| {
            if !is_error {
                output.lock().unwrap().push_str(chunk);
            }
        }),
    );
    let output = output.into_inner().unwrap();
    println!("  Argon: Exit={:?}, Success={}, Output={}", result.exit_code, result.succeeded, output.trim());
    println!("  Log: {}", result.log_path);
    if let Ok(log) = std::fs::read_to_string(&result.log_path) {
        for line in log.lines().filter(|line| line.contains("agy") || line.contains("--model")).take(3) {
            println!("  {line}");
        }
    }
    assert!(result.succeeded, "Argon 實際喚醒失敗：{}", result.error);
    assert!(output.contains("OK"), "Argon 應回覆 OK。");

    // agy 回報用量可能略有延遲；最多等 60 秒。
    usage::clear_antigravity_connection_cache();
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        std::thread::sleep(Duration::from_secs(5));
        let snapshot =
            CliUsageReader::new().read(CliKind::AntigravityArgon, &profile, &working_directory, &cancel).unwrap();
        for window in &snapshot.windows {
            println!(
                "  -> {}: 剩餘 {}%, 倒數中={}, 重置時間: {:?}",
                window.name,
                window.remaining_percent(),
                window.is_active_countdown,
                window.resets_at
            );
        }
        if is_counting_down(Some(&snapshot), &time::now()) {
            break;
        }
        assert!(Instant::now() < deadline, "Argon 喚醒後 60 秒內額度仍未開始倒數：{}", snapshot.message);
    }
}

#[test]
fn debug_log_write_and_export() {
    let _serial = serial();
    let directory = TempDir::new();
    let paths = AppDataPaths::new(Some(&directory.join("data").to_string_lossy()));
    paths.ensure_created().unwrap();
    std::fs::write(paths.settings_file(), "{\"MinimizeToTray\": true}").unwrap();
    std::fs::write(paths.logs_directory().join("20260101-080000-Codex.log"), "CLI: Codex\r\nstdout").unwrap();
    // 過期的除錯日誌在初始化時清除。
    let stale = paths.logs_directory().join("debug-20000101.log");
    std::fs::write(&stale, "old").unwrap();

    debug_log::init(&paths);
    assert!(debug_log::is_enabled());
    assert!(!stale.exists(), "超過保留天數的除錯日誌應被清除。");
    debug_log::write("測試", "第一行\n第二行");

    let destination = directory.join("export").join("bundle.txt");
    let count = debug_log::export(&paths, "版本: 測試", &destination).unwrap();
    assert_eq!(count, 2, "應收錄 CLI 執行日誌與除錯日誌。");
    let bytes = std::fs::read(&destination).unwrap();
    assert_eq!(&bytes[..3], &[0xEF, 0xBB, 0xBF], "匯出檔應帶 UTF-8 BOM。");
    let text = String::from_utf8(bytes[3..].to_vec()).unwrap();
    for expected in
        ["版本: 測試", "MinimizeToTray", "20260101-080000-Codex.log", "[測試] 第一行\r\n    第二行", "（無法讀取"]
    {
        assert!(text.contains(expected), "匯出內容應包含「{expected}」：\n{text}");
    }
}

#[test]
fn cli_update_version_compare() {
    use aiwake_core::cli_update::{compare_versions, parse_latest_version, parse_version, CliTool, CliUpdateStatus};
    use std::cmp::Ordering;

    assert_eq!(parse_version("codex-cli 0.160.0").as_deref(), Some("0.160.0"));
    assert_eq!(parse_version("2.1.289 (Claude Code)").as_deref(), Some("2.1.289"));
    assert_eq!(parse_version("1.4.4\r\n").as_deref(), Some("1.4.4"));
    assert_eq!(parse_version("unknown"), None);

    assert_eq!(compare_versions("0.163.0", "0.160.0"), Ordering::Greater);
    assert_eq!(compare_versions("2.1.306", "2.1.289"), Ordering::Greater, "必須以數字比較，不能以字串比較。");
    assert_eq!(compare_versions("1.10.0", "1.9.9"), Ordering::Greater);
    assert_eq!(compare_versions("1.4.4", "1.4.4"), Ordering::Equal);
    assert_eq!(compare_versions("1.4", "1.4.0"), Ordering::Equal);

    assert_eq!(parse_latest_version(r#"{"name":"@openai/codex","version":"0.163.0"}"#).as_deref(), Some("0.163.0"));
    assert_eq!(parse_latest_version(r#"{"version":"1.4.4","url":"https://x/cli.exe"}"#).as_deref(), Some("1.4.4"));
    assert_eq!(parse_latest_version("<html>"), None);

    let status = |installed: Option<&str>, latest: Option<&str>| CliUpdateStatus {
        tool: CliTool::Codex,
        executable: Some("codex".to_string()),
        installed: installed.map(str::to_string),
        latest: latest.map(str::to_string),
        error: None,
    };
    assert!(status(Some("0.160.0"), Some("0.163.0")).update_available());
    assert!(!status(Some("0.163.0"), Some("0.163.0")).update_available());
    assert!(!status(Some("0.164.0"), Some("0.163.0")).update_available(), "本機較新（預覽版）不應提示。");
    assert!(!status(None, Some("0.163.0")).update_available(), "讀不到本機版本時不應提示。");
    assert_eq!(status(Some("0.160.0"), Some("0.163.0")).describe(), "Codex CLI：0.160.0 → 0.163.0（有新版本）");

    let update = aiwake_core::cli_update::create_update_start_info(CliTool::Claude, "claude", "").unwrap();
    assert!(update.arguments.contains("update") && update.arguments.contains("|| pause"), "更新失敗時應暫停主控台。");
    assert!(!update.create_no_window, "更新必須在看得見的主控台執行。");
}

#[test]
#[ignore = "需要本機已安裝的 CLI 與網路（唯讀，不耗 Token）"]
fn cli_update_check_real() {
    use aiwake_core::cli_update::{self, CliTool};
    let settings = AppSettings::create_default();
    for tool in CliTool::ALL {
        let profile = settings.profile(tool.kind());
        let executable = locator::resolve(tool.kind(), Some(&profile.executable), ".");
        let status = cli_update::check(tool, executable, ".", &CancelToken::new());
        println!("{}", status.describe());
        assert!(
            status.installed.is_some() && status.latest.is_some(),
            "{} 應可讀取本機與最新版本。",
            tool.display_name()
        );
    }
}
