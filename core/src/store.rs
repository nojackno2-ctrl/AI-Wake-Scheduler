use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::error::{CoreError, CoreResult};
use crate::models::{Guid, JsonModel};

/// 資料持久化的抽象。排程管理器只依賴這個介面，
/// 不需要知道資料實際上是 JSON 檔、記憶體還是資料庫。
pub trait DataStore<T>: Send + Sync {
    fn load(&self) -> CoreResult<T>;
    fn save(&self, value: &T) -> CoreResult<()>;
}

/// 提供 JSON 檔案的安全讀寫與原子寫入機制。
pub struct JsonFileStore<T> {
    path: PathBuf,
    default_factory: Box<dyn Fn() -> T + Send + Sync>,
    gate: Mutex<()>,
}

impl<T: JsonModel> JsonFileStore<T> {
    pub fn new(path: impl Into<PathBuf>, default_factory: impl Fn() -> T + Send + Sync + 'static) -> Self {
        Self { path: path.into(), default_factory: Box::new(default_factory), gate: Mutex::new(()) }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn corrupted(&self) -> CoreError {
        backup_corrupted_file(&self.path);
        CoreError::InvalidData(format!("無法讀取資料檔：{}", self.path.display()))
    }
}

impl<T: JsonModel + Send> DataStore<T> for JsonFileStore<T> {
    /// 載入 JSON 資料檔。若檔案不存在，傳回預設值；若損毀，會備份損毀檔並回報錯誤。
    fn load(&self) -> CoreResult<T> {
        let _guard = self.gate.lock().unwrap_or_else(|e| e.into_inner());
        if !self.path.exists() {
            return Ok((self.default_factory)());
        }

        let bytes = std::fs::read(&self.path)?;
        let text = match std::str::from_utf8(&bytes) {
            Ok(text) => text.trim_start_matches('\u{feff}'),
            Err(_) => return Err(self.corrupted()),
        };
        let value: serde_json::Value = match serde_json::from_str(text) {
            Ok(value) => value,
            Err(_) => return Err(self.corrupted()),
        };
        T::from_json(&value).map_err(|_| self.corrupted())
    }

    /// 以原子寫入方式（先寫入暫存檔再移動覆蓋）儲存資料。
    fn save(&self, value: &T) -> CoreResult<()> {
        let _guard = self.gate.lock().unwrap_or_else(|e| e.into_inner());
        let directory =
            self.path.parent().ok_or_else(|| CoreError::InvalidOperation("資料檔路徑沒有父目錄。".to_string()))?;
        std::fs::create_dir_all(directory)?;

        let mut temporary = self.path.clone().into_os_string();
        temporary.push(format!(".{}.tmp", Guid::new().simple()));
        let temporary = PathBuf::from(temporary);

        let text = serde_json::to_string_pretty(&value.to_json()).map_err(|e| CoreError::Io(e.to_string()))?;
        let result = std::fs::write(&temporary, text).and_then(|_| std::fs::rename(&temporary, &self.path));
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result.map_err(CoreError::from)
    }
}

fn backup_corrupted_file(path: &Path) {
    if path.exists() {
        let mut backup = path.as_os_str().to_os_string();
        backup.push(format!(".corrupted.{}", chrono::Local::now().format("%Y%m%d-%H%M%S")));
        // 備份失敗不影響錯誤回報
        let _ = std::fs::copy(path, backup);
    }
}
