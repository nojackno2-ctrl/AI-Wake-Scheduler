//! 搜尋與定位 CLI 可執行檔路徑。
//!
//! 候選清單來自 `catalog`；解析結果會快取，
//! 避免每次執行或探測都重新遞迴掃描 LocalAppData / Program Files。

use std::collections::HashMap;
use std::path::Path;
use std::sync::{LazyLock, Mutex};

use crate::catalog::{self, ExecutableSearchPaths};
use crate::models::CliKind;
use crate::win;

type CacheKey = (CliKind, String, String);

static CACHE: LazyLock<Mutex<HashMap<CacheKey, String>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn cache() -> std::sync::MutexGuard<'static, HashMap<CacheKey, String>> {
    CACHE.lock().unwrap_or_else(|e| e.into_inner())
}

fn full_path(path: &Path) -> String {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf()).to_string_lossy().into_owned()
}

/// 解析指定 CLI 種類的可執行檔路徑，找不到時傳回 None。
pub fn resolve(kind: CliKind, configured_value: Option<&str>, working_directory: &str) -> Option<String> {
    let configured = configured_value.unwrap_or_default().trim().to_string();
    let key = (kind, configured.clone(), working_directory.to_string());

    // 快取命中後仍確認檔案存在：CLI 自動更新換到新的雜湊目錄時會自然失效。
    if let Some(cached) = cache().get(&key).cloned() {
        if Path::new(&cached).is_file() {
            return Some(cached);
        }
    }

    let resolved = resolve_core(kind, &configured, working_directory);
    match &resolved {
        Some(path) => {
            cache().insert(key, path.clone());
        }
        None => {
            cache().remove(&key);
        }
    }
    resolved
}

/// 清除路徑快取（使用者變更設定或手動重新探測時使用）。
pub fn clear_cache() {
    cache().clear();
}

fn has_separator(value: &str) -> bool {
    value.contains('\\') || value.contains('/')
}

fn resolve_core(kind: CliKind, configured: &str, working_directory: &str) -> Option<String> {
    let descriptor = catalog::get(kind);
    let configured_is_default_command = is_default_command(configured, descriptor.default_command);

    if !configured.trim().is_empty() {
        let expanded = win::expand_environment_variables(configured);

        if let Some(explicit_path) = resolve_explicit_path(&expanded, working_directory) {
            return Some(explicit_path);
        }

        if !has_separator(&expanded) && !configured_is_default_command {
            if let Some(from_path) = find_on_path(&expanded) {
                return Some(from_path);
            }
        }
    }

    for candidate in (descriptor.executable_candidates)(ExecutableSearchPaths::current()) {
        if Path::new(&candidate).is_file() {
            return Some(full_path(Path::new(&candidate)));
        }
    }

    // WindowsApps 中的 Codex app execution alias 可能看似存在但無法由一般程序啟動。
    // 使用預設命令名稱時，必須先檢查 Desktop-managed 安裝位置；只有找不到才回退 PATH。
    if !configured.trim().is_empty() && !has_separator(configured) {
        if let Some(configured_from_path) = find_on_path(configured) {
            return Some(configured_from_path);
        }
    }

    find_on_path(descriptor.default_command)
}

fn is_default_command(configured: &str, default_command: &str) -> bool {
    if configured.trim().is_empty() {
        return false;
    }
    Path::new(configured).file_stem().is_some_and(|stem| stem.to_string_lossy().eq_ignore_ascii_case(default_command))
}

fn is_path_rooted(value: &str) -> bool {
    let bytes = value.as_bytes();
    matches!(bytes.first(), Some(b'\\') | Some(b'/'))
        || (bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic())
}

fn resolve_explicit_path(value: &str, working_directory: &str) -> Option<String> {
    if is_path_rooted(value) {
        return Path::new(value).is_file().then(|| full_path(Path::new(value)));
    }

    if !working_directory.trim().is_empty() {
        let combined = Path::new(working_directory).join(value);
        if combined.is_file() {
            return Some(full_path(&combined));
        }
    }
    None
}

fn find_on_path(command: &str) -> Option<String> {
    let path_env = std::env::var("PATH").ok().filter(|value| !value.trim().is_empty())?;

    let extensions = if Path::new(command).extension().is_some() { vec![String::new()] } else { path_extensions() };
    for raw_directory in path_env.split(';') {
        let directory = raw_directory.trim().trim_matches('"');
        if directory.is_empty() {
            continue;
        }

        for extension in &extensions {
            let candidate = Path::new(directory).join(format!("{command}{extension}"));
            if candidate.is_file() {
                return Some(full_path(&candidate));
            }
        }
    }
    None
}

fn path_extensions() -> Vec<String> {
    match std::env::var("PATHEXT") {
        Ok(configured) if !configured.trim().is_empty() => {
            configured.split(';').map(str::trim).filter(|extension| !extension.is_empty()).map(str::to_string).collect()
        }
        _ => vec![".exe".to_string(), ".cmd".to_string(), ".bat".to_string()],
    }
}
