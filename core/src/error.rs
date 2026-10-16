use std::fmt;

/// 核心層的錯誤種類。變體對應原本各自的例外型別，
/// 讓呼叫端（與測試）可以分辨「參數不合法」與「狀態不允許」等情況。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreError {
    InvalidArgument(String),
    OutOfRange(String),
    InvalidOperation(String),
    DirectoryNotFound(String),
    FileNotFound(String),
    InvalidData(String),
    Format(String),
    Io(String),
    Canceled,
}

impl CoreError {
    pub fn message(&self) -> &str {
        match self {
            CoreError::InvalidArgument(m)
            | CoreError::OutOfRange(m)
            | CoreError::InvalidOperation(m)
            | CoreError::DirectoryNotFound(m)
            | CoreError::FileNotFound(m)
            | CoreError::InvalidData(m)
            | CoreError::Format(m)
            | CoreError::Io(m) => m,
            CoreError::Canceled => "執行已取消。",
        }
    }

    /// 參數類錯誤（含超出範圍）。
    pub fn is_argument(&self) -> bool {
        matches!(self, CoreError::InvalidArgument(_) | CoreError::OutOfRange(_))
    }
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for CoreError {}

impl From<std::io::Error> for CoreError {
    fn from(error: std::io::Error) -> Self {
        CoreError::Io(error.to_string())
    }
}

pub type CoreResult<T> = Result<T, CoreError>;
