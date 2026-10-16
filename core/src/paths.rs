use std::path::{Path, PathBuf};

/// 管理應用程式資料與工作區路徑。
#[derive(Debug, Clone)]
pub struct AppDataPaths {
    root_directory: PathBuf,
}

impl AppDataPaths {
    pub fn new(root_directory: Option<&str>) -> Self {
        let raw_root = match root_directory.filter(|value| !value.trim().is_empty()) {
            Some(value) => PathBuf::from(value),
            None => PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_default()).join("AI倒數喚醒"),
        };
        Self { root_directory: std::path::absolute(&raw_root).unwrap_or(raw_root) }
    }

    pub fn root_directory(&self) -> &Path {
        &self.root_directory
    }

    pub fn jobs_file(&self) -> PathBuf {
        self.root_directory.join("schedules.json")
    }

    pub fn settings_file(&self) -> PathBuf {
        self.root_directory.join("settings.json")
    }

    pub fn logs_directory(&self) -> PathBuf {
        self.root_directory.join("logs")
    }

    pub fn wakeup_workspace(&self) -> PathBuf {
        self.root_directory.join("workspace")
    }

    /// 確保所有必要的目錄結構皆已建立。
    pub fn ensure_created(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.root_directory)?;
        std::fs::create_dir_all(self.logs_directory())?;
        std::fs::create_dir_all(self.wakeup_workspace())
    }
}
