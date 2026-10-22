use std::path::Path;
use std::time::Duration;

use crate::cli_update::{self, CliTool, CliUpdateStatus};
use crate::command_builder;
use crate::error::{CoreError, CoreResult};
use crate::locator;
use crate::log_writer::CliLogWriter;
use crate::models::{CliKind, CliProfile, CliRunResult};
use crate::paths::AppDataPaths;
use crate::process::{self, OutputCallback, ProcessExecution};
use crate::time::{self, CancelToken};

/// CLI 探測結果模型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliProbeResult {
    pub cli: CliKind,
    pub succeeded: bool,
    pub summary: String,
    pub executable_path: String,
}

/// CLI 執行的抽象。排程管理器只依賴這個介面，
/// 測試可用假實作取代，不必真的啟動子程序。
pub trait CliRunner: Send + Sync {
    #[allow(clippy::too_many_arguments)]
    fn run(
        &self,
        kind: CliKind,
        profile: &CliProfile,
        message: &str,
        working_directory: &str,
        timeout: Duration,
        token_saver_mode: bool,
        cancel: &CancelToken,
        on_output: Option<OutputCallback<'_>>,
    ) -> CliRunResult;

    fn probe(
        &self,
        kind: CliKind,
        profile: &CliProfile,
        working_directory: &str,
        cancel: &CancelToken,
    ) -> CliProbeResult;
}

const PROBE_TIMEOUT: Duration = Duration::from_secs(15);

/// 決定「要對哪個 CLI 執行什麼」的政策層。
/// 實際的程序啟動交給 `process`，日誌交給 `CliLogWriter`。
pub struct ProcessCliRunner {
    log: CliLogWriter,
    update_checker: Box<UpdateChecker>,
}

type UpdateChecker = dyn Fn(CliTool, Option<String>, &str, &CancelToken) -> CliUpdateStatus + Send + Sync;

impl ProcessCliRunner {
    pub fn new(paths: AppDataPaths) -> Self {
        Self::with_update_checker(paths, cli_update::check)
    }

    /// 注入版本來源供隔離測試使用；放行規則仍由執行器統一套用。
    pub fn with_update_checker(
        paths: AppDataPaths,
        checker: impl Fn(CliTool, Option<String>, &str, &CancelToken) -> CliUpdateStatus + Send + Sync + 'static,
    ) -> Self {
        Self { log: CliLogWriter::new(paths), update_checker: Box::new(checker) }
    }
}

impl CliRunner for ProcessCliRunner {
    /// 執行指定的 CLI 命令，並記錄執行日誌。
    fn run(
        &self,
        kind: CliKind,
        profile: &CliProfile,
        message: &str,
        working_directory: &str,
        timeout: Duration,
        token_saver_mode: bool,
        cancel: &CancelToken,
        on_output: Option<OutputCallback<'_>>,
    ) -> CliRunResult {
        let started_at = time::now();
        let mut result = CliRunResult::new(kind);
        result.started_at = started_at;

        let outcome = (|| -> CoreResult<()> {
            validate_working_directory(working_directory)?;
            let executable = locator::resolve(kind, Some(&profile.executable), working_directory).ok_or_else(|| {
                CoreError::FileNotFound(format!("找不到 {}。請在設定中指定可執行檔。", kind.display_name()))
            })?;
            result.executable_path = executable.clone();

            if cancel.is_canceled() {
                return Err(CoreError::Canceled);
            }
            let status =
                (self.update_checker)(CliTool::from_kind(kind), Some(executable.clone()), working_directory, cancel);
            if cancel.is_canceled() {
                return Err(CoreError::Canceled);
            }
            crate::debug_log::write("更新", format!("呼叫前強制檢查：{}", status.describe()));
            status.require_current()?;

            let arguments =
                command_builder::build_for_profile(kind, message, Some(profile), token_saver_mode, Some(timeout))?;
            let execution = process::execute(&executable, &arguments, working_directory, timeout, cancel, on_output)?;

            result.exit_code = Some(execution.exit_code);
            result.succeeded = execution.exit_code == 0 && !execution.timed_out;
            result.error = if execution.timed_out {
                format!("超過 {:.0} 分鐘，程序已終止。", timeout.as_secs_f64() / 60.0)
            } else if execution.exit_code == 0 {
                String::new()
            } else {
                format!("CLI 結束碼為 {}。", execution.exit_code)
            };
            result.log_path =
                self.log.write_run_log(kind, &started_at, &executable, &arguments, &execution, &result.error);
            Ok(())
        })();

        match outcome {
            Ok(()) => {}
            Err(CoreError::Canceled) => result.error = "執行已取消。".to_string(),
            Err(error) => {
                result.error = error.message().to_string();
                result.log_path =
                    self.log.write_failure_log(kind, &started_at, &result.executable_path, &format!("{error:?}"));
            }
        }

        result.finished_at = time::now();
        result
    }

    /// 探測指定 CLI 是否可正常執行（只跑 --version，不會消耗任何 Token）。
    fn probe(
        &self,
        kind: CliKind,
        profile: &CliProfile,
        working_directory: &str,
        cancel: &CancelToken,
    ) -> CliProbeResult {
        let outcome = (|| -> CoreResult<CliProbeResult> {
            validate_working_directory(working_directory)?;
            let executable = locator::resolve(kind, Some(&profile.executable), working_directory)
                .ok_or_else(|| CoreError::FileNotFound(format!("找不到 {}。", kind.display_name())))?;
            let execution =
                process::execute(&executable, &["--version"], working_directory, PROBE_TIMEOUT, cancel, None)?;
            Ok(CliProbeResult {
                cli: kind,
                succeeded: execution.exit_code == 0 && !execution.timed_out,
                summary: summarize_probe(&execution),
                executable_path: executable,
            })
        })();

        outcome.unwrap_or_else(|error| CliProbeResult {
            cli: kind,
            succeeded: false,
            summary: error.message().to_string(),
            executable_path: String::new(),
        })
    }
}

fn summarize_probe(execution: &ProcessExecution) -> String {
    if execution.timed_out {
        return "檢查逾時".to_string();
    }

    if execution.exit_code != 0 {
        return if execution.standard_error.trim().is_empty() {
            format!("結束碼 {}", execution.exit_code)
        } else {
            execution.standard_error.trim().to_string()
        };
    }

    let output = if execution.standard_output.trim().is_empty() {
        &execution.standard_error
    } else {
        &execution.standard_output
    };
    output.split(['\r', '\n']).find(|line| !line.is_empty()).map(str::to_string).unwrap_or_else(|| "可執行".to_string())
}

fn validate_working_directory(working_directory: &str) -> CoreResult<()> {
    if working_directory.trim().is_empty() || !Path::new(working_directory).is_dir() {
        return Err(CoreError::DirectoryNotFound(format!("工作目錄不存在：{working_directory}")));
    }
    Ok(())
}
