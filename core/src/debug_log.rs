//! 除錯日誌：把排程判斷、額度讀取與 CLI 執行的經過逐行寫入 `logs\debug-YYYYMMDD.log`，
//! 並可將設定、排程與所有日誌合併匯出成單一文字檔，方便回報問題。
//!
//! 只有呼叫 `init` 之後才會寫檔；測試與未初始化的程式呼叫 `write` 不會有任何副作用。

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::paths::AppDataPaths;
use crate::time;

/// 除錯日誌保留天數；超過的檔案在初始化時清除。
const RETAINED_DAYS: i64 = 14;

/// 匯出時單一檔案最多收錄的位元組數（只取尾端），避免匯出檔過大。
const MAX_EXPORTED_BYTES_PER_FILE: u64 = 2 * 1024 * 1024;

const FILE_PREFIX: &str = "debug-";

fn directory() -> &'static Mutex<Option<PathBuf>> {
    static DIRECTORY: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
    DIRECTORY.get_or_init(|| Mutex::new(None))
}

/// 開始把除錯日誌寫到指定資料目錄的 logs 底下，並清除過期的除錯日誌。
pub fn init(paths: &AppDataPaths) {
    let logs = paths.logs_directory();
    let _ = std::fs::create_dir_all(&logs);
    prune(&logs);
    *directory().lock().unwrap_or_else(|e| e.into_inner()) = Some(logs);
}

/// 目前是否會寫入除錯日誌。
pub fn is_enabled() -> bool {
    directory().lock().unwrap_or_else(|e| e.into_inner()).is_some()
}

/// 寫入一行除錯日誌；未初始化或寫檔失敗時直接忽略，不影響主要流程。
pub fn write(category: &str, message: impl AsRef<str>) {
    let guard = directory().lock().unwrap_or_else(|e| e.into_inner());
    let Some(logs) = guard.as_ref() else {
        return;
    };
    let now = time::now();
    let local = time::local_naive(&now);
    let path = logs.join(format!("{FILE_PREFIX}{}.log", local.format("%Y%m%d")));
    // 多行訊息縮排接續，讓每筆紀錄仍以時間開頭。
    let message = message.as_ref().trim_end().replace("\r\n", "\n").replace('\n', "\r\n    ");
    let line = format!("{} [{category}] {message}\r\n", local.format("%Y-%m-%d %H:%M:%S%.3f"));
    // 持有鎖寫入，避免多個執行緒的紀錄交錯。
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(line.as_bytes());
    }
}

fn prune(logs: &Path) {
    let Ok(entries) = std::fs::read_dir(logs) else {
        return;
    };
    let cutoff = time::local_naive(&time::now()).date() - chrono::Duration::days(RETAINED_DAYS);
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(date) = name
            .strip_prefix(FILE_PREFIX)
            .and_then(|rest| rest.strip_suffix(".log"))
            .and_then(|date| chrono::NaiveDate::parse_from_str(date, "%Y%m%d").ok())
        else {
            continue;
        };
        if date < cutoff {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// 將摘要、設定檔、排程檔與 logs 底下所有日誌合併寫成一個 UTF-8 文字檔。
/// `summary` 由呼叫端提供（版本、目前額度等），回傳收錄的日誌檔數。
pub fn export(paths: &AppDataPaths, summary: &str, destination: &Path) -> std::io::Result<usize> {
    let mut output = String::new();
    output.push_str("===== AI 倒數喚醒 除錯日誌匯出 =====\r\n");
    output.push_str(&format!(
        "匯出時間: {}\r\n資料目錄: {}\r\n\r\n",
        time::local_naive(&time::now()).format("%Y-%m-%d %H:%M:%S"),
        paths.root_directory().display()
    ));
    if !summary.trim().is_empty() {
        output.push_str("===== 目前狀態 =====\r\n");
        output.push_str(summary.trim_end());
        output.push_str("\r\n\r\n");
    }

    for (title, path) in [("設定檔", paths.settings_file()), ("排程檔", paths.jobs_file())] {
        output.push_str(&format!("===== {title}：{} =====\r\n", path.display()));
        match std::fs::read(&path) {
            Ok(bytes) => output.push_str(String::from_utf8_lossy(&bytes).trim_end()),
            Err(error) => output.push_str(&format!("（無法讀取：{error}）")),
        }
        output.push_str("\r\n\r\n");
    }

    // 依修改時間由舊到新排列，與事件發生順序一致。
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(paths.logs_directory())
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| entry.path().extension().is_some_and(|ext| ext.eq_ignore_ascii_case("log")))
                .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
                .collect()
        })
        .unwrap_or_default();
    files.sort();

    output.push_str(&format!("===== 日誌檔（共 {} 個）=====\r\n\r\n", files.len()));
    for (_, path) in &files {
        output.push_str(&format!("----- {} -----\r\n", path.file_name().unwrap_or_default().to_string_lossy()));
        output.push_str(read_tail(path).trim_end());
        output.push_str("\r\n\r\n");
    }

    if let Some(parent) = destination.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    // 加上 BOM，讓記事本以外的舊工具也能正確辨識 UTF-8。
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(output.as_bytes());
    std::fs::write(destination, bytes)?;
    Ok(files.len())
}

fn read_tail(path: &Path) -> String {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut file) = std::fs::File::open(path) else {
        return "（無法讀取）".to_string();
    };
    let length = file.metadata().map(|metadata| metadata.len()).unwrap_or(0);
    let mut prefix = String::new();
    if length > MAX_EXPORTED_BYTES_PER_FILE {
        let _ = file.seek(SeekFrom::Start(length - MAX_EXPORTED_BYTES_PER_FILE));
        prefix = format!("（檔案共 {length} 位元組，只收錄最後 {MAX_EXPORTED_BYTES_PER_FILE} 位元組）\r\n");
    }
    let mut bytes = Vec::new();
    let _ = file.read_to_end(&mut bytes);
    prefix + &String::from_utf8_lossy(&bytes)
}
