use std::collections::BTreeMap;
use std::fmt;

use serde_json::{json, Map, Value};

use crate::catalog;
use crate::time::{self, Moment, Span};

macro_rules! named_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum $name { $($variant),+ }

        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            /// 資料檔中使用的名稱。
            pub fn name(self) -> &'static str {
                match self { $($name::$variant => stringify!($variant)),+ }
            }

            pub fn parse(text: &str) -> Option<Self> {
                Self::ALL.iter().copied().find(|value| value.name().eq_ignore_ascii_case(text.trim()))
            }

            /// 由資料檔的值還原；同時接受名稱字串與舊式的整數值。
            pub fn from_json(value: &Value) -> Option<Self> {
                match value {
                    Value::String(text) => Self::parse(text),
                    Value::Number(number) => number
                        .as_u64()
                        .and_then(|index| usize::try_from(index).ok())
                        .and_then(|index| Self::ALL.get(index).copied()),
                    _ => None,
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.name())
            }
        }
    };
}

named_enum! {
    /// 支援的 AI CLI 工具種類。
    ///
    /// 新種類一律加在最後：舊版資料檔可能以整數序號保存目標。
    /// 畫面與預設目標的順序由 `catalog::all()` 決定，不依賴這裡的宣告順序。
    CliKind { Antigravity, AntigravityClaude, Codex, Claude, AntigravityArgon }
}

named_enum! {
    /// 排程狀態。
    ScheduleStatus { Pending, Running, Completed, Failed, Disabled }
}

named_enum! {
    /// 舊資料相容標記；排程一律依每日偵測時段運作。
    /// 舊值仍可讀取，儲存時統一使用 Interval 標記。
    ScheduleRecurrence { Once, Daily, Weekly, Interval, Countdown }
}

named_enum! {
    /// 思考程度 / 推理強度。
    ThinkingEffort { Default, Minimal, Low, Medium, High, XHigh, Max, Ultra }
}

impl CliKind {
    /// 設定與結果視窗使用的完整名稱。
    pub fn display_name(self) -> &'static str {
        catalog::get(self).display_name
    }

    /// 清單欄位使用的短名稱。
    pub fn short_name(self) -> &'static str {
        catalog::get(self).short_name
    }
}

impl ThinkingEffort {
    pub fn display_name(self) -> &'static str {
        match self {
            ThinkingEffort::Default => "預設",
            ThinkingEffort::Minimal => "最低 (Minimal)",
            ThinkingEffort::Low => "低 (Low)",
            ThinkingEffort::Medium => "中 (Medium)",
            ThinkingEffort::High => "高 (High)",
            ThinkingEffort::XHigh => "極高 (XHigh)",
            ThinkingEffort::Max => "最高 (Max)",
            ThinkingEffort::Ultra => "超高 (Ultra)",
        }
    }
}

/// 排程識別碼（GUID）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Guid(pub u128);

impl Guid {
    /// 產生新的識別碼（每次呼叫都不同，因此不提供 `Default`）。
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Guid(crate::win::new_guid())
    }

    pub fn parse(text: &str) -> Option<Self> {
        let hex: String = text.trim().chars().filter(|c| !matches!(c, '-' | '{' | '}' | '(' | ')')).collect();
        if hex.len() != 32 {
            return None;
        }
        u128::from_str_radix(&hex, 16).ok().map(Guid)
    }

    /// 不含連字號的 32 位十六進位表示。
    pub fn simple(&self) -> String {
        format!("{:032x}", self.0)
    }
}

impl fmt::Display for Guid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let hex = self.simple();
        write!(f, "{}-{}-{}-{}-{}", &hex[0..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..32])
    }
}

/// 可寫入 JSON 資料檔的模型。
pub trait JsonModel: Sized {
    fn to_json(&self) -> Value;
    fn from_json(value: &Value) -> Result<Self, String>;
}

/// 不分大小寫取得屬性（舊資料檔的屬性名稱大小寫可能不同）。
fn field<'a>(object: &'a Map<String, Value>, name: &str) -> Option<&'a Value> {
    object.get(name).or_else(|| object.iter().find(|(key, _)| key.eq_ignore_ascii_case(name)).map(|(_, value)| value))
}

fn expect_object<'a>(value: &'a Value, what: &str) -> Result<&'a Map<String, Value>, String> {
    value.as_object().ok_or_else(|| format!("{what} 必須是 JSON 物件。"))
}

/// 避免自動儲存時捨棄無法理解的欄位，或任選大小寫重複的欄位。
fn validate_fields(object: &Map<String, Value>, template: Value) -> Result<(), String> {
    let known = expect_object(&template, "欄位範本")?;
    let mut seen = std::collections::HashSet::new();
    for key in object.keys() {
        if !known.keys().any(|name| name.eq_ignore_ascii_case(key)) {
            return Err(format!("無法保留未知欄位：{key}。"));
        }
        if !seen.insert(key.to_ascii_lowercase()) {
            return Err(format!("欄位名稱重複：{key}。"));
        }
    }
    Ok(())
}

fn read_string(object: &Map<String, Value>, name: &str, target: &mut String) -> Result<(), String> {
    match field(object, name) {
        None => Ok(()),
        Some(Value::Null) => {
            target.clear();
            Ok(())
        }
        Some(Value::String(text)) => {
            *target = text.clone();
            Ok(())
        }
        Some(_) => Err(format!("{name} 必須是字串。")),
    }
}

fn read_bool(object: &Map<String, Value>, name: &str, target: &mut bool) -> Result<(), String> {
    match field(object, name) {
        None => Ok(()),
        Some(Value::Bool(flag)) => {
            *target = *flag;
            Ok(())
        }
        Some(_) => Err(format!("{name} 必須是布林值。")),
    }
}

fn read_i32(object: &Map<String, Value>, name: &str, target: &mut i32) -> Result<(), String> {
    match field(object, name) {
        None => Ok(()),
        Some(value) => {
            *target = value
                .as_i64()
                .and_then(|number| i32::try_from(number).ok())
                .ok_or_else(|| format!("{name} 必須是整數。"))?;
            Ok(())
        }
    }
}

fn read_moment(value: &Value, name: &str) -> Result<Moment, String> {
    value.as_str().and_then(time::parse_moment).ok_or_else(|| format!("{name} 不是有效的日期時間。"))
}

fn read_optional_moment(object: &Map<String, Value>, name: &str) -> Result<Option<Option<Moment>>, String> {
    match field(object, name) {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(value) => read_moment(value, name).map(|moment| Some(Some(moment))),
    }
}

macro_rules! read_enum {
    ($object:expr, $name:literal, $ty:ty, $target:expr) => {
        if let Some(value) = field($object, $name) {
            $target = <$ty>::from_json(value).ok_or_else(|| format!("{} 的值無效。", $name))?;
        }
    };
}

/// 個別 CLI 工具的設定檔。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CliProfile {
    pub executable: String,
    pub model: String,
    pub thinking_effort: ThinkingEffort,
    pub additional_arguments: String,
}

// 列舉由巨集產生，預設值在這裡明確指定。
#[allow(clippy::derivable_impls)]
impl Default for ThinkingEffort {
    fn default() -> Self {
        ThinkingEffort::Default
    }
}

impl JsonModel for CliProfile {
    fn to_json(&self) -> Value {
        json!({
            "Executable": self.executable,
            "Model": self.model,
            "ThinkingEffort": self.thinking_effort.name(),
            "AdditionalArguments": self.additional_arguments,
        })
    }

    fn from_json(value: &Value) -> Result<Self, String> {
        let object = expect_object(value, "CLI 設定")?;
        let mut profile = CliProfile::default();
        read_string(object, "Executable", &mut profile.executable)?;
        read_string(object, "Model", &mut profile.model)?;
        read_enum!(object, "ThinkingEffort", ThinkingEffort, profile.thinking_effort);
        read_string(object, "AdditionalArguments", &mut profile.additional_arguments)?;
        Ok(profile)
    }
}

/// 應用程式整體設定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppSettings {
    pub start_with_windows: bool,
    pub minimize_to_tray: bool,
    pub token_saver_mode: bool,
    pub execution_timeout_minutes: i32,
    pub quota_auto_refresh_minutes: i32,
    /// 是否允許背景無視窗執行 agy 內建 /usage；關閉時僅查詢已開啟的 IDE。
    pub auto_start_antigravity: bool,
    pub cli_profiles: BTreeMap<CliKind, CliProfile>,
    /// 已套用的預設模型更新版本；只在升級時遷移一次，之後使用者改回舊模型不會再被覆寫。
    pub default_models_revision: i32,
}

/// 目前的預設模型版本。1：Claude 預設由 Haiku 4.5 改為更便宜的 Haiku 5.5。
pub const DEFAULT_MODELS_REVISION: i32 = 1;

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            start_with_windows: false,
            minimize_to_tray: true,
            token_saver_mode: true,
            execution_timeout_minutes: 3,
            quota_auto_refresh_minutes: 10,
            auto_start_antigravity: true,
            cli_profiles: default_profiles(),
            default_models_revision: DEFAULT_MODELS_REVISION,
        }
    }
}

fn default_profile(descriptor: &catalog::CliDescriptor) -> CliProfile {
    CliProfile {
        executable: descriptor.default_command.to_string(),
        model: descriptor.default_model.unwrap_or_default().to_string(),
        ..CliProfile::default()
    }
}

fn default_profiles() -> BTreeMap<CliKind, CliProfile> {
    catalog::all().iter().map(|descriptor| (descriptor.kind, default_profile(descriptor))).collect()
}

impl AppSettings {
    pub fn create_default() -> Self {
        Self::default()
    }

    /// 補齊缺少的 CLI 設定並修正超出範圍的值。
    pub fn ensure_defaults(&mut self) {
        for descriptor in catalog::all() {
            let profile = self.cli_profiles.entry(descriptor.kind).or_insert_with(|| default_profile(descriptor));

            // 已從 CLI 清單移除的內建模型設定移到現行替代模型；自訂模型保留。
            let replacement = match (descriptor.kind, profile.model.trim()) {
                (CliKind::AntigravityClaude, "claude-sonnet-4-6") => Some("claude-sonnet-5-5-low"),
                (CliKind::AntigravityClaude, "claude-opus-4-6-thinking") => Some("claude-opus-5-5-high"),
                (CliKind::Codex, "gpt-5.4") => Some("gpt-6.1-sol"),
                (CliKind::Codex, "gpt-5.4-mini") => Some("gpt-6-luna"),
                // 2026-10-22 實測：ChatGPT 帳號的 Codex 已不支援 gpt-5.5（HTTP 400）。
                (CliKind::Codex, "gpt-5.5") => Some("gpt-6.1-sol"),
                _ => None,
            };
            if let Some(model) = replacement {
                profile.model = model.to_string();
            }

            // 舊版的預設模型改為目前最便宜的預設（只遷移一次）。
            let cheaper = match (descriptor.kind, profile.model.trim()) {
                (CliKind::Claude, "claude-haiku-4-5-20251001" | "claude-haiku-4-5") => Some("claude-haiku-5-5"),
                _ => None,
            };
            if let (true, Some(model)) = (self.default_models_revision < 1, cheaper) {
                profile.model = model.to_string();
            }
        }
        self.default_models_revision = self.default_models_revision.max(DEFAULT_MODELS_REVISION);

        self.execution_timeout_minutes = self.execution_timeout_minutes.clamp(1, 120);
        self.quota_auto_refresh_minutes = self.quota_auto_refresh_minutes.clamp(0, 1440);
    }

    /// 補齊預設值後的副本。
    pub fn normalized(&self) -> Self {
        let mut clone = self.clone();
        clone.ensure_defaults();
        clone
    }

    pub fn profile(&self, kind: CliKind) -> CliProfile {
        self.cli_profiles.get(&kind).cloned().unwrap_or_default()
    }
}

impl JsonModel for AppSettings {
    fn to_json(&self) -> Value {
        let mut profiles = Map::new();
        for (kind, profile) in &self.cli_profiles {
            profiles.insert(kind.name().to_string(), profile.to_json());
        }
        json!({
            "StartWithWindows": self.start_with_windows,
            "MinimizeToTray": self.minimize_to_tray,
            "TokenSaverMode": self.token_saver_mode,
            "ExecutionTimeoutMinutes": self.execution_timeout_minutes,
            "QuotaAutoRefreshMinutes": self.quota_auto_refresh_minutes,
            "AutoStartAntigravity": self.auto_start_antigravity,
            "CliProfiles": Value::Object(profiles),
            "DefaultModelsRevision": self.default_models_revision,
        })
    }

    fn from_json(value: &Value) -> Result<Self, String> {
        let object = expect_object(value, "設定檔")?;
        // 沒有此欄位的舊設定檔視為尚未套用任何預設模型更新。
        let mut settings = AppSettings { default_models_revision: 0, ..AppSettings::default() };
        read_i32(object, "DefaultModelsRevision", &mut settings.default_models_revision)?;
        read_bool(object, "StartWithWindows", &mut settings.start_with_windows)?;
        read_bool(object, "MinimizeToTray", &mut settings.minimize_to_tray)?;
        read_bool(object, "TokenSaverMode", &mut settings.token_saver_mode)?;
        read_i32(object, "ExecutionTimeoutMinutes", &mut settings.execution_timeout_minutes)?;
        read_i32(object, "QuotaAutoRefreshMinutes", &mut settings.quota_auto_refresh_minutes)?;
        read_bool(object, "AutoStartAntigravity", &mut settings.auto_start_antigravity)?;

        match field(object, "CliProfiles") {
            None | Some(Value::Null) => {}
            Some(Value::Object(entries)) => {
                let mut profiles = BTreeMap::new();
                for (key, entry) in entries {
                    let kind = CliKind::parse(key).ok_or_else(|| format!("未知的 CLI 種類：{key}"))?;
                    if !entry.is_null() {
                        profiles.insert(kind, CliProfile::from_json(entry)?);
                    }
                }
                settings.cli_profiles = profiles;
            }
            Some(_) => return Err("CliProfiles 必須是 JSON 物件。".to_string()),
        }
        Ok(settings)
    }
}

/// CLI 執行結果。
#[derive(Debug, Clone, PartialEq)]
pub struct CliRunResult {
    pub cli: CliKind,
    pub succeeded: bool,
    pub exit_code: Option<i32>,
    pub started_at: Moment,
    pub finished_at: Moment,
    pub executable_path: String,
    pub error: String,
    pub log_path: String,
}

impl CliRunResult {
    pub fn new(cli: CliKind) -> Self {
        Self {
            cli,
            succeeded: false,
            exit_code: None,
            started_at: Moment::default(),
            finished_at: Moment::default(),
            executable_path: String::new(),
            error: String::new(),
            log_path: String::new(),
        }
    }
}

impl JsonModel for CliRunResult {
    fn to_json(&self) -> Value {
        json!({
            "Cli": self.cli.name(),
            "Succeeded": self.succeeded,
            "ExitCode": self.exit_code,
            "StartedAt": time::format_json_moment(&self.started_at),
            "FinishedAt": time::format_json_moment(&self.finished_at),
            "ExecutablePath": self.executable_path,
            "Error": self.error,
            "LogPath": self.log_path,
        })
    }

    fn from_json(value: &Value) -> Result<Self, String> {
        let object = expect_object(value, "執行結果")?;
        let mut result = CliRunResult::new(CliKind::Antigravity);
        validate_fields(object, result.to_json())?;
        read_enum!(object, "Cli", CliKind, result.cli);
        read_bool(object, "Succeeded", &mut result.succeeded)?;
        match field(object, "ExitCode") {
            None => {}
            Some(Value::Null) => result.exit_code = None,
            Some(code) => {
                result.exit_code =
                    Some(code.as_i64().and_then(|number| i32::try_from(number).ok()).ok_or("ExitCode 必須是整數。")?)
            }
        }
        if let Some(value) = field(object, "StartedAt") {
            result.started_at = read_moment(value, "StartedAt")?;
        }
        if let Some(value) = field(object, "FinishedAt") {
            result.finished_at = read_moment(value, "FinishedAt")?;
        }
        read_string(object, "ExecutablePath", &mut result.executable_path)?;
        read_string(object, "Error", &mut result.error)?;
        read_string(object, "LogPath", &mut result.log_path)?;
        Ok(result)
    }
}

pub const DEFAULT_JOB_NAME: &str = "AI 倒數喚醒";
pub const DEFAULT_JOB_MESSAGE: &str = "早安";

/// 排程資料遷移版本；未標記的舊資料視為版本 0。
pub const SCHEDULE_REVISION: i32 = 2;

pub fn user_profile_directory() -> String {
    std::env::var("USERPROFILE").unwrap_or_default()
}

/// 排程工作模型。
#[derive(Debug, Clone, PartialEq)]
pub struct ScheduledJob {
    pub schedule_revision: i32,
    pub id: Guid,
    pub name: String,
    pub scheduled_at: Moment,
    pub message: String,
    pub working_directory: String,
    pub targets: Vec<CliKind>,
    pub recurrence: ScheduleRecurrence,
    pub initial_time_of_day: Option<Span>,
    pub end_time_of_day: Span,
    pub enabled: bool,
    pub status: ScheduleStatus,
    pub started_at: Option<Moment>,
    pub finished_at: Option<Moment>,
    pub last_results: Vec<CliRunResult>,
}

impl Default for ScheduledJob {
    fn default() -> Self {
        Self {
            schedule_revision: SCHEDULE_REVISION,
            id: Guid::new(),
            name: DEFAULT_JOB_NAME.to_string(),
            scheduled_at: time::now() + Span::minutes(5),
            message: DEFAULT_JOB_MESSAGE.to_string(),
            working_directory: user_profile_directory(),
            targets: catalog::kinds(),
            recurrence: ScheduleRecurrence::Interval,
            initial_time_of_day: None,
            end_time_of_day: Span::days(1),
            enabled: true,
            status: ScheduleStatus::Pending,
            started_at: None,
            finished_at: None,
            last_results: Vec::new(),
        }
    }
}

impl ScheduledJob {
    /// 建立副本；沒有任何目標時補回全部 CLI。
    pub fn snapshot(&self) -> Self {
        let mut clone = self.clone();
        if clone.targets.is_empty() {
            clone.targets = catalog::kinds();
        }
        clone
    }
}

impl JsonModel for ScheduledJob {
    fn to_json(&self) -> Value {
        json!({
            "ScheduleRevision": self.schedule_revision,
            "Id": self.id.to_string(),
            "Name": self.name,
            "ScheduledAt": time::format_json_moment(&self.scheduled_at),
            "Message": self.message,
            "WorkingDirectory": self.working_directory,
            "Targets": self.targets.iter().map(|kind| kind.name()).collect::<Vec<_>>(),
            "Recurrence": self.recurrence.name(),
            "InitialTimeOfDay": self.initial_time_of_day.map(time::format_span),
            "EndTimeOfDay": time::format_span(self.end_time_of_day),
            "Enabled": self.enabled,
            "Status": self.status.name(),
            "StartedAt": self.started_at.as_ref().map(time::format_json_moment),
            "FinishedAt": self.finished_at.as_ref().map(time::format_json_moment),
            "LastResults": self.last_results.iter().map(JsonModel::to_json).collect::<Vec<_>>(),
        })
    }

    fn from_json(value: &Value) -> Result<Self, String> {
        let object = expect_object(value, "排程")?;
        let mut job = ScheduledJob { schedule_revision: 0, ..ScheduledJob::default() };
        validate_fields(object, job.to_json())?;
        read_i32(object, "ScheduleRevision", &mut job.schedule_revision)?;
        if !(0..=SCHEDULE_REVISION).contains(&job.schedule_revision) {
            return Err("不支援此排程資料版本，請使用較新的程式。".to_string());
        }

        job.id = field(object, "Id").and_then(Value::as_str).and_then(Guid::parse).ok_or("Id 不是有效的 GUID。")?;
        read_string(object, "Name", &mut job.name)?;
        job.scheduled_at = read_moment(field(object, "ScheduledAt").ok_or("缺少 ScheduledAt。")?, "ScheduledAt")?;
        read_string(object, "Message", &mut job.message)?;
        read_string(object, "WorkingDirectory", &mut job.working_directory)?;

        match field(object, "Targets") {
            None => {}
            Some(Value::Null) => job.targets.clear(),
            Some(Value::Array(items)) => {
                job.targets = items
                    .iter()
                    .map(|item| CliKind::from_json(item).ok_or_else(|| "Targets 含有未知的 CLI 種類。".to_string()))
                    .collect::<Result<_, _>>()?;
            }
            Some(_) => return Err("Targets 必須是陣列。".to_string()),
        }

        read_enum!(object, "Recurrence", ScheduleRecurrence, job.recurrence);
        match field(object, "InitialTimeOfDay") {
            None => {}
            Some(Value::Null) => job.initial_time_of_day = None,
            Some(value) => {
                job.initial_time_of_day =
                    Some(value.as_str().and_then(time::parse_span).ok_or("InitialTimeOfDay 不是有效的時間。")?)
            }
        }
        if job.initial_time_of_day.is_some_and(|start| !time::is_valid_time_of_day(start)) {
            return Err("InitialTimeOfDay 必須介於 00:00 到 23:59。".to_string());
        }
        if let Some(value) = field(object, "EndTimeOfDay") {
            job.end_time_of_day = value.as_str().and_then(time::parse_span).ok_or("EndTimeOfDay 不是有效的時間。")?;
        }
        crate::calculator::validate_window(
            time::truncate_to_minutes(
                job.initial_time_of_day.unwrap_or_else(|| time::local_time_of_day(&job.scheduled_at)),
            ),
            job.end_time_of_day,
        )
        .map_err(|error| error.to_string())?;
        read_bool(object, "Enabled", &mut job.enabled)?;
        read_enum!(object, "Status", ScheduleStatus, job.status);
        if let Some(value) = read_optional_moment(object, "StartedAt")? {
            job.started_at = value;
        }
        if let Some(value) = read_optional_moment(object, "FinishedAt")? {
            job.finished_at = value;
        }

        match field(object, "LastResults") {
            None => {}
            Some(Value::Null) => job.last_results.clear(),
            Some(Value::Array(items)) => {
                job.last_results = items.iter().map(CliRunResult::from_json).collect::<Result<_, _>>()?;
            }
            Some(_) => return Err("LastResults 必須是陣列。".to_string()),
        }
        Ok(job)
    }
}

impl<T: JsonModel> JsonModel for Vec<T> {
    fn to_json(&self) -> Value {
        Value::Array(self.iter().map(JsonModel::to_json).collect())
    }

    fn from_json(value: &Value) -> Result<Self, String> {
        value.as_array().ok_or_else(|| "資料檔必須是 JSON 陣列。".to_string())?.iter().map(T::from_json).collect()
    }
}

/// 單次 CLI 呼叫的即時活動種類，供 UI 端的終端機面板顯示。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliActivityKind {
    Started,
    Output,
    ErrorOutput,
    Completed,
    Failed,
}

/// 單筆即時活動事件。`text` 的意義依 `kind` 而定：
/// Started/Completed/Failed 是一句摘要，Output/ErrorOutput 是子程序輸出的原始片段
/// （可能不是完整一行，UI 端自行拼接）。
#[derive(Debug, Clone, PartialEq)]
pub struct CliActivityEvent {
    pub cli: CliKind,
    pub job_name: String,
    pub kind: CliActivityKind,
    pub text: String,
    pub timestamp: Moment,
}
