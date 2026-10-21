//! AI 倒數喚醒核心：排程計算、CLI 參數建構與執行、帳戶額度讀取。
//! 不含任何 UI；桌面程式與測試都只依賴這個 crate。

pub mod calculator;
pub mod catalog;
pub mod cli_update;
pub mod command_builder;
pub mod debug_log;
pub mod error;
pub mod http;
pub mod locator;
pub mod log_writer;
pub mod login;
pub mod manager;
pub mod models;
pub mod paths;
pub mod process;
pub mod runner;
pub mod store;
pub mod time;
pub mod tokenizer;
pub mod usage;
pub mod win;

pub use error::{CoreError, CoreResult};
pub use manager::ScheduleManager;
pub use models::{
    AppSettings, CliActivityEvent, CliActivityKind, CliKind, CliProfile, CliRunResult, Guid, JsonModel,
    ScheduleRecurrence, ScheduleStatus, ScheduledJob, ThinkingEffort,
};
pub use paths::AppDataPaths;
pub use runner::{CliProbeResult, CliRunner, ProcessCliRunner};
pub use store::{DataStore, JsonFileStore};
pub use time::{CancelToken, Clock, Moment, Span, SystemClock};
pub use usage::{CliUsageAvailability, CliUsageReader, CliUsageSnapshot, CliUsageWindow, UsageReader};
