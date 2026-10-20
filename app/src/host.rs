use std::sync::{Arc, Mutex};

use aiwake_core::{
    catalog, debug_log, AppDataPaths, AppSettings, CliUsageReader, CoreResult, DataStore, JsonFileStore,
    ProcessCliRunner, ScheduleManager, ScheduledJob, UsageReader,
};

use crate::startup;

/// 組合根：唯一知道所有元件如何組裝起來的地方。
/// 視窗只拿到它需要的服務，main 只負責啟動與關閉。
pub struct AppHost {
    pub paths: AppDataPaths,
    pub settings_store: JsonFileStore<AppSettings>,
    pub settings: Arc<Mutex<AppSettings>>,
    pub runner: Arc<ProcessCliRunner>,
    pub usage_reader: Arc<CliUsageReader>,
    pub manager: ScheduleManager,
    /// 是否使用隔離的資料目錄（功能驗證模式）；隔離時不碰正式的開機啟動設定。
    pub is_isolated: bool,
}

impl AppHost {
    pub fn create(root_directory: Option<&str>) -> CoreResult<Self> {
        let paths = AppDataPaths::new(root_directory);
        paths.ensure_created()?;
        debug_log::init(&paths);
        debug_log::write(
            "程式",
            format!(
                "啟動 v{}，資料目錄 {}，執行檔 {}",
                env!("CARGO_PKG_VERSION"),
                paths.root_directory().display(),
                std::env::current_exe().map(|path| path.display().to_string()).unwrap_or_default()
            ),
        );

        let settings_store = JsonFileStore::new(paths.settings_file(), AppSettings::create_default);
        let mut settings = settings_store.load()?;
        settings.ensure_defaults();
        if root_directory.is_none() {
            settings.start_with_windows = startup::is_enabled();
        }

        let job_store: Arc<JsonFileStore<Vec<ScheduledJob>>> =
            Arc::new(JsonFileStore::new(paths.jobs_file(), Vec::new));
        let runner = Arc::new(ProcessCliRunner::new(paths.clone()));
        let usage_reader = Arc::new(CliUsageReader::new());
        usage_reader.set_auto_start_antigravity(settings.auto_start_antigravity);

        let settings = Arc::new(Mutex::new(settings));
        let provider = Arc::clone(&settings);
        let manager = ScheduleManager::new(
            job_store,
            runner.clone(),
            move || provider.lock().unwrap_or_else(|e| e.into_inner()).clone(),
            Some(usage_reader.clone() as Arc<dyn UsageReader>),
            None,
        );
        manager.initialize()?;

        Ok(Self {
            paths,
            settings_store,
            settings,
            runner,
            usage_reader,
            manager,
            is_isolated: root_directory.is_some(),
        })
    }

    /// 目前設定的副本。
    pub fn settings(&self) -> AppSettings {
        self.settings.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// 套用並持久化新的設定。
    pub fn replace_settings(&self, settings: AppSettings) -> CoreResult<()> {
        let normalized = settings.normalized();
        *self.settings.lock().unwrap_or_else(|e| e.into_inner()) = normalized.clone();
        self.settings_store.save(&normalized)
    }

    /// 將目前額度、設定、排程與所有日誌匯出成單一文字檔，回傳收錄的日誌檔數。
    pub fn export_debug_logs(&self, destination: &std::path::Path) -> std::io::Result<usize> {
        let mut summary = format!(
            "版本: v{}\r\n執行檔: {}\r\n",
            env!("CARGO_PKG_VERSION"),
            std::env::current_exe().map(|path| path.display().to_string()).unwrap_or_default()
        );
        let snapshots = self.usage_reader.latest_snapshots();
        for descriptor in catalog::all() {
            let state = snapshots
                .get(&descriptor.kind)
                .map(|snapshot| snapshot.describe())
                .unwrap_or_else(|| "尚未讀取".to_string());
            summary.push_str(&format!("額度 {}: {state}\r\n", descriptor.display_name));
        }
        debug_log::write("程式", format!("匯出除錯日誌至 {}", destination.display()));
        debug_log::export(&self.paths, &summary, destination)
    }

    pub fn wakeup_workspace(&self) -> String {
        self.paths.wakeup_workspace().to_string_lossy().into_owned()
    }
}
