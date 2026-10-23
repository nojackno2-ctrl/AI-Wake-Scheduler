use std::sync::atomic::{AtomicUsize, Ordering};

use crate::models::CliKind;
use crate::paths::AppDataPaths;
use crate::process::ProcessExecution;
use crate::time::{self, Moment};

/// 日誌保留上限；超過的舊檔會在下一次寫入時清除。
const MAX_RETAINED_LOG_FILES: usize = 200;

/// 負責把 CLI 執行紀錄寫成檔案，並定期清掉舊日誌。
/// 與執行邏輯分離，讓 CLI 執行器只處理「執行什麼」。
pub struct CliLogWriter {
    paths: AppDataPaths,
    writes: AtomicUsize,
}

impl CliLogWriter {
    pub fn new(paths: AppDataPaths) -> Self {
        Self { paths, writes: AtomicUsize::new(0) }
    }

    pub fn write_run_log(
        &self,
        kind: CliKind,
        started_at: &Moment,
        executable: &str,
        arguments: &[String],
        execution: &ProcessExecution,
        error: &str,
    ) -> String {
        let body = format!(
            "CLI: {}\r\n開始時間: {}\r\n執行檔: {}\r\n參數: {}\r\n結束碼: {}\r\nShell 模式: {}\r\n錯誤摘要: {}\r\n--- stdout ---\r\n{}\r\n--- stderr ---\r\n{}\r\n",
            kind.display_name(),
            time::format_round_trip(started_at),
            executable,
            format_arguments(arguments),
            execution.exit_code,
            if execution.used_shell { "True" } else { "False" },
            error,
            execution.standard_output,
            execution.standard_error,
        );
        self.write(&format!("{}-{}.log", started_at.format("%Y%m%d-%H%M%S"), kind.name()), &body)
    }

    pub fn write_failure_log(&self, kind: CliKind, started_at: &Moment, executable: &str, error: &str) -> String {
        let body = format!(
            "CLI: {}\r\n開始時間: {}\r\n執行檔: {}\r\n{}",
            kind.display_name(),
            time::format_round_trip(started_at),
            executable,
            error,
        );
        self.write(&format!("{}-{}-error.log", started_at.format("%Y%m%d-%H%M%S"), kind.name()), &body)
    }

    /// 寫日誌失敗不應該讓喚醒本身失敗，失敗時回傳空字串。
    fn write(&self, file_name: &str, body: &str) -> String {
        if self.paths.ensure_created().is_err() {
            return String::new();
        }
        self.prune_periodically();
        let log_path = self.paths.logs_directory().join(file_name);
        match std::fs::write(&log_path, body) {
            Ok(()) => log_path.to_string_lossy().into_owned(),
            Err(_) => String::new(),
        }
    }

    /// 首次與每 32 次寫入清理，避免常駐期間無限累積或每次寫入都掃描目錄。
    fn prune_periodically(&self) {
        if !self.writes.fetch_add(1, Ordering::Relaxed).is_multiple_of(32) {
            return;
        }

        let Ok(entries) = std::fs::read_dir(self.paths.logs_directory()) else {
            return;
        };
        let mut files: Vec<_> = entries
            .flatten()
            // 除錯日誌有獨立的保留天數，不應被執行日誌清理捨棄。
            .filter(|entry| !entry.file_name().to_string_lossy().starts_with("debug-"))
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext.eq_ignore_ascii_case("log")))
            .filter_map(|entry| {
                let modified = entry.metadata().ok()?.modified().ok()?;
                Some((modified, entry.path()))
            })
            .collect();
        if files.len() < MAX_RETAINED_LOG_FILES {
            return;
        }

        files.sort_by_key(|entry| std::cmp::Reverse(entry.0));
        // 預留即將寫入的一筆；兩次清理間最多多出 31 筆。
        for (_, path) in files.into_iter().skip(MAX_RETAINED_LOG_FILES - 1) {
            // 個別檔案刪不掉（被佔用等）就跳過
            let _ = std::fs::remove_file(path);
        }
    }
}

fn format_arguments(arguments: &[String]) -> String {
    arguments
        .iter()
        .map(
            |argument| {
                if argument.is_empty() || argument.contains(' ') {
                    format!("\"{argument}\"")
                } else {
                    argument.clone()
                }
            },
        )
        .collect::<Vec<_>>()
        .join(" ")
}
