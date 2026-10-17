//! CLI 重新登入。
//!
//! Claude、Codex、Antigravity 都是 OAuth 登入，一定要在瀏覽器按一次授權，
//! 所以這裡只負責判斷「是不是登入失效」與開一個看得見的主控台跑 CLI 自己的登入流程；
//! 本程式不讀寫任何憑證，也不代填帳號密碼。

use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;
use std::sync::LazyLock;

use regex::Regex;

use crate::error::{CoreError, CoreResult};
use crate::models::CliKind;
use crate::process;

static LOGIN_REQUIRED_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)not logged in|logged out|\blog ?in\b|sign ?in|unauthenticated|unauthori[sz]ed|authentication required|auth(?:entication)? token|token (?:has )?expired|refresh token|\b401\b|未登入|請先登入|重新登入|登入憑證",
    )
    .expect("登入判定樣式應可編譯")
});

/// 由錯誤訊息判斷是否為登入失效。
/// 網路被擋、逾時、429 之類的錯誤不能誤判，否則使用者登入完仍然讀不到。
pub fn looks_like_login_required(message: Option<&str>) -> bool {
    message.is_some_and(|text| !text.trim().is_empty() && LOGIN_REQUIRED_PATTERN.is_match(text))
}

/// 各 CLI 的登入子命令。agy 沒有 login 子命令，直接進互動模式就會走登入流程。
pub fn get_arguments(kind: CliKind) -> &'static [&'static str] {
    match kind {
        CliKind::Claude => &["auth", "login"],
        CliKind::Codex => &["login"],
        CliKind::Antigravity | CliKind::AntigravityArgon | CliKind::AntigravityClaude => &[],
    }
}

/// 登入用的啟動資訊。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginStartInfo {
    pub file_name: String,
    pub arguments: String,
    pub working_directory: Option<String>,
    /// 登入必須開啟看得見的主控台。
    pub create_no_window: bool,
}

impl LoginStartInfo {
    /// 啟動登入主控台；呼叫端可等待回傳的子程序結束後重新讀取額度。
    pub fn spawn(&self) -> CoreResult<std::process::Child> {
        let mut command = Command::new(&self.file_name);
        command.raw_arg(&self.arguments);
        if let Some(directory) = &self.working_directory {
            command.current_dir(directory);
        }
        // 一律給登入流程自己的主控台，即使本程式是從另一個主控台啟動的。
        const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
        command.creation_flags(if self.create_no_window { process::CREATE_NO_WINDOW } else { CREATE_NEW_CONSOLE });
        command.spawn().map_err(|error| CoreError::Io(error.to_string()))
    }
}

/// 建立登入用的啟動資訊：透過 `cmd.exe` 開一個看得見的主控台視窗，
/// 讓 CLI 能顯示授權網址、開瀏覽器並接收使用者輸入。
/// 登入失敗（結束碼非 0）時暫停，錯誤訊息才不會隨視窗一閃而逝。
pub fn create_start_info(kind: CliKind, executable: &str, working_directory: &str) -> CoreResult<LoginStartInfo> {
    if executable.trim().is_empty() {
        return Err(CoreError::InvalidArgument("可執行檔不可為空白。".to_string()));
    }

    let shell_arguments = process::build_command_shell_arguments(executable, get_arguments(kind));
    // build_command_shell_arguments 以一對外層引號收尾，把 || pause 放進引號內。
    let arguments = format!("{} || pause\"", &shell_arguments[..shell_arguments.len() - 1]);
    let working_directory = (!working_directory.trim().is_empty() && Path::new(working_directory).is_dir())
        .then(|| working_directory.to_string());

    Ok(LoginStartInfo { file_name: "cmd.exe".to_string(), arguments, working_directory, create_no_window: false })
}
