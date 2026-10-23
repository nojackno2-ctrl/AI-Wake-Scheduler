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
        let mut current = self.settings.lock().unwrap_or_else(|e| e.into_inner());
        self.settings_store.save(&normalized)?;
        *current = normalized;
        Ok(())
    }

    /// 套用開機啟動與設定；存檔失敗時補償啟動偏好，並保留兩個錯誤。
    /// 隔離模式由呼叫端傳入 None；測試可注入完全不碰登錄的替身。
    pub(crate) fn apply_settings_with_startup(
        &self,
        settings: AppSettings,
        startup: Option<&dyn Fn(bool) -> Result<(), String>>,
    ) -> Result<(), String> {
        let normalized = settings.normalized();
        let mut current = self.settings.lock().unwrap_or_else(|e| e.into_inner());
        let changed = normalized.start_with_windows != current.start_with_windows;
        if let Some(set_enabled) = startup.filter(|_| changed) {
            set_enabled(normalized.start_with_windows)?;
        }
        if let Err(error) = self.settings_store.save(&normalized) {
            let mut message = error.message().to_string();
            if let Some(set_enabled) = startup.filter(|_| changed) {
                if let Err(rollback) = set_enabled(current.start_with_windows) {
                    message.push_str(&format!("；還原開機啟動設定失敗：{rollback}"));
                }
            }
            return Err(message);
        }
        *current = normalized;
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regression_settings_failure_compensates_startup_and_reports_rollback_failure() {
        for rollback_fails in [false, true] {
            let root = std::env::temp_dir().join(format!("AiWakeScheduler-QA-{}", aiwake_core::Guid::new().simple()));
            let host = AppHost::create(Some(&root.to_string_lossy())).expect("隔離主機");
            let original = host.settings();
            let mut draft = original.clone();
            draft.start_with_windows = !original.start_with_windows;
            draft.execution_timeout_minutes = 17;
            std::fs::create_dir(host.paths.settings_file()).expect("建立隔離儲存障礙");
            let save_error = host.settings_store.save(&draft).unwrap_err().message().to_string();
            let calls = Mutex::new(Vec::new());
            let enabled = Mutex::new(original.start_with_windows);
            let fake_startup = |value| {
                let mut calls = calls.lock().unwrap();
                calls.push(value);
                if calls.len() == 2 && rollback_fails {
                    return Err("測試注入補償失敗".into());
                }
                *enabled.lock().unwrap() = value;
                Ok(())
            };
            let error = host.apply_settings_with_startup(draft.clone(), Some(&fake_startup)).unwrap_err();
            assert!(error.contains(&save_error), "必須保留原始存檔錯誤");
            assert_eq!(calls.lock().unwrap().as_slice(), &[draft.start_with_windows, original.start_with_windows]);
            assert_eq!(
                *enabled.lock().unwrap(),
                if rollback_fails { draft.start_with_windows } else { original.start_with_windows }
            );
            assert_eq!(error.contains("還原開機啟動設定失敗：測試注入補償失敗"), rollback_fails);
            assert_eq!(host.settings(), original);
            std::fs::remove_dir(host.paths.settings_file()).expect("移除隔離障礙");
            host.apply_settings_with_startup(draft.clone(), Some(&fake_startup)).expect("恢復後成功儲存");
            assert_eq!(host.settings(), draft);
            assert_eq!(host.settings_store.load().unwrap(), draft);
            let count = calls.lock().unwrap().len();
            host.apply_settings_with_startup(draft.clone(), Some(&fake_startup)).unwrap();
            assert_eq!(calls.lock().unwrap().len(), count, "啟動偏好未變更不呼叫替身");
            // None 與 UI 的隔離分支相同，僅寫入隔離 JSON。
            host.apply_settings_with_startup(original.clone(), None).unwrap();
            assert_eq!(calls.lock().unwrap().len(), count);
            assert_eq!(host.settings(), original);
            host.manager.shutdown();
            drop(host);
            let _ = std::fs::remove_dir_all(root);
        }
    }

    #[test]
    fn regression_startup_failure_does_not_publish_or_save_settings() {
        let root = std::env::temp_dir().join(format!("AiWakeScheduler-QA-{}", aiwake_core::Guid::new().simple()));
        let host = AppHost::create(Some(&root.to_string_lossy())).expect("隔離主機");
        let original = host.settings();
        let mut draft = original.clone();
        draft.start_with_windows = !original.start_with_windows;
        let error = host.apply_settings_with_startup(draft, Some(&|_| Err("測試注入啟動設定失敗".into()))).unwrap_err();
        assert_eq!(error, "測試注入啟動設定失敗");
        assert_eq!(host.settings(), original);
        assert!(!host.paths.settings_file().exists());
        host.manager.shutdown();
        drop(host);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn regression_failed_settings_save_preserves_current_settings() {
        let root = std::env::temp_dir().join(format!("AiWakeScheduler-QA-{}", aiwake_core::Guid::new().simple()));
        let host = AppHost::create(Some(&root.to_string_lossy())).expect("隔離主機");
        let original = host.settings();
        // 用同名資料夾使原子替換確定失敗，不依賴帳號權限或正式資料。
        std::fs::create_dir(host.paths.settings_file()).expect("建立儲存障礙");
        let mut draft = original.clone();
        draft.execution_timeout_minutes = 17;
        assert!(host.replace_settings(draft).is_err());
        assert_eq!(host.settings(), original);
        std::fs::remove_dir(host.paths.settings_file()).expect("移除隔離障礙");
        let mut draft = original;
        draft.execution_timeout_minutes = 19;
        host.replace_settings(draft.clone()).expect("恢復儲存");
        assert_eq!(host.settings(), draft);
        assert_eq!(host.settings_store.load().expect("讀回設定"), draft);
        host.manager.shutdown();
        drop(host);
        let _ = std::fs::remove_dir_all(root);
    }
}
