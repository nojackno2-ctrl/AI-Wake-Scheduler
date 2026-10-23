//! 純粹的子程序啟動機制，不含任何 CLI 或排程知識。
//! CLI 政策（要送什麼參數、怎麼記錄）與程序機制（怎麼啟動、怎麼收輸出、怎麼逾時）互不相依。

use std::io::Read;
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::error::{CoreError, CoreResult};
use crate::time::CancelToken;
use crate::win;

pub use crate::win::{SEM_FAILCRITICALERRORS, SEM_NOGPFAULTERRORBOX};

/// 單一串流最多保留的字元數。CLI 若話很多，超過的部分會被丟棄而不是一路長進記憶體，
/// 同時也讓日誌檔維持在可讀的大小。
pub const MAX_CAPTURED_CHARACTERS: usize = 32 * 1024;

pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 子程序的執行結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessExecution {
    pub exit_code: i32,
    pub standard_output: String,
    pub standard_error: String,
    pub timed_out: bool,
    pub used_shell: bool,
}

/// 禁止子孫程序彈出系統層級的錯誤對話框。
///
/// 錯誤模式是在建立子程序時繼承下去的，所以只要在啟動任何 CLI 之前設定一次，
/// 就同時涵蓋我們直接啟動的 CLI 以及它們自己再啟動的程序。
///
/// 實際遇到的情況：Codex 會自行啟動 `git.exe`，而防毒軟體注入自我防護 DLL
/// 偶爾會失敗，讓 git.exe 以 `0xc0000142`（STATUS_DLL_INIT_FAILED）啟動失敗，
/// Windows 就在使用者桌面上蓋一個 modal 對話框。一個常駐系統匣、按排程在背景
/// 呼叫 CLI 的工具不該讓子孫程序這樣打斷使用者。
///
/// 失敗本身不會被吞掉：結束碼與 stderr 照常記進 CLI 日誌。
/// 傳回設定之前的錯誤模式。
pub fn suppress_child_process_error_dialogs() -> u32 {
    win::set_error_mode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX)
}

/// 取得目前行程的錯誤模式；供測試驗證設定確實生效。
pub fn get_current_error_mode() -> u32 {
    win::get_error_mode()
}

/// 僅供登入／更新主控台使用；拒絕會改變 shell 語法的引號、展開與換行。
/// 一般 CLI（含批次檔）必須使用 `build_command`，交由標準函式庫處理參數。
pub fn build_command_shell_arguments<S: AsRef<str>>(executable: &str, arguments: &[S]) -> CoreResult<String> {
    let mut builder = String::from("/e:ON /v:OFF /d /s /c \"");
    builder.push_str(&quote_console_argument(executable)?);
    for argument in arguments {
        builder.push(' ');
        builder.push_str(&quote_console_argument(argument.as_ref())?);
    }
    builder.push('"');
    Ok(builder)
}

/// 所有值均加引號；百分比展開無法以此方式保護，因此明確拒絕。
fn quote_console_argument(value: &str) -> CoreResult<String> {
    if value.contains(['"', '%', '\r', '\n', '\0']) {
        return Err(CoreError::InvalidArgument(
            "登入／更新主控台的路徑與參數不可含雙引號、百分比、換行或 NUL 字元。".to_string(),
        ));
    }

    let mut builder = String::with_capacity(value.len() + 8);
    builder.push('"');
    let mut backslashes = 0usize;
    for character in value.chars() {
        if character == '\\' {
            backslashes += 1;
            continue;
        }

        builder.extend(std::iter::repeat_n('\\', backslashes));
        backslashes = 0;
        builder.push(character);
    }

    // 結尾的反斜線會和收尾引號黏在一起，必須加倍才不會把引號跳脫掉。
    builder.extend(std::iter::repeat_n('\\', backslashes * 2));
    builder.push('"');
    Ok(builder)
}

pub fn is_shell_script(executable: &str) -> bool {
    let lower = executable.to_ascii_lowercase();
    lower.ends_with(".cmd") || lower.ends_with(".bat")
}

/// 建立不開視窗的子程序命令。
///
/// .cmd / .bat 由 Rust 標準函式庫選用批次檔專用跳脫與 cmd.exe 設定。
pub fn build_command<S: AsRef<str>>(executable: &str, arguments: &[S], working_directory: &str) -> (Command, bool) {
    let shell_script = is_shell_script(executable);
    let mut command = Command::new(executable);
    command.args(arguments.iter().map(|argument| argument.as_ref()));

    if !working_directory.is_empty() {
        command.current_dir(working_directory);
    }
    command.creation_flags(CREATE_NO_WINDOW);
    // 沒有 ANSI 色碼，擷取到的輸出更小也更好讀。
    command.env("NO_COLOR", "1").env("TERM", "dumb");
    (command, shell_script)
}

/// 即時輸出回呼：第一個參數表示是否為 stderr，第二個是輸出片段。
pub type OutputCallback<'a> = &'a dyn Fn(bool, &str);

enum StreamEvent {
    Chunk(bool, String),
    Closed,
}

/// 逐段把位元組解成 UTF-8 文字；多位元組字元被讀取邊界切開時會保留到下一段。
pub(crate) struct Utf8Chunker {
    pending: Vec<u8>,
}

impl Utf8Chunker {
    pub(crate) fn new() -> Self {
        Self { pending: Vec::new() }
    }

    pub(crate) fn push(&mut self, bytes: &[u8]) -> String {
        self.pending.extend_from_slice(bytes);
        let mut text = String::new();
        loop {
            match std::str::from_utf8(&self.pending) {
                Ok(valid) => {
                    text.push_str(valid);
                    self.pending.clear();
                    return text;
                }
                Err(error) => {
                    let valid_up_to = error.valid_up_to();
                    text.push_str(std::str::from_utf8(&self.pending[..valid_up_to]).unwrap_or_default());
                    match error.error_len() {
                        Some(length) => {
                            text.push('\u{fffd}');
                            self.pending.drain(..valid_up_to + length);
                        }
                        None => {
                            self.pending.drain(..valid_up_to);
                            return text;
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn finish(&mut self) -> String {
        let text = String::from_utf8_lossy(&self.pending).into_owned();
        self.pending.clear();
        text
    }
}

fn spawn_reader(stream: impl Read + Send + 'static, is_error: bool, sender: mpsc::SyncSender<StreamEvent>) {
    std::thread::spawn(move || {
        let mut stream = stream;
        let mut buffer = [0u8; 4096];
        let mut chunker = Utf8Chunker::new();
        loop {
            match stream.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    let text = chunker.push(&buffer[..read]);
                    if !text.is_empty() && sender.send(StreamEvent::Chunk(is_error, text)).is_err() {
                        return;
                    }
                }
            }
        }
        let rest = chunker.finish();
        if !rest.is_empty() {
            let _ = sender.send(StreamEvent::Chunk(is_error, rest));
        }
        let _ = sender.send(StreamEvent::Closed);
    });
}

/// 有容量上限的輸出緩衝；超過的內容會被丟棄，不會累積在記憶體裡。
struct BoundedText {
    text: String,
    characters: usize,
    truncated: bool,
}

impl BoundedText {
    fn new() -> Self {
        Self { text: String::new(), characters: 0, truncated: false }
    }

    fn append(&mut self, chunk: &str) {
        let remaining = MAX_CAPTURED_CHARACTERS.saturating_sub(self.characters);
        if remaining == 0 {
            self.truncated = true;
            return;
        }
        let mut taken = 0usize;
        for character in chunk.chars() {
            if taken == remaining {
                self.truncated = true;
                break;
            }
            self.text.push(character);
            taken += 1;
        }
        self.characters += taken;
    }

    fn finish(mut self) -> String {
        if self.truncated {
            self.text.push_str("\r\n…（輸出過長，其餘內容已捨棄）");
        }
        self.text
    }
}

/// 執行子程序並收集輸出。
///
/// `on_output` 會在呼叫端的執行緒上被即時通知每一段輸出（第一個參數表示是否為 stderr），
/// 不受容量上限影響：上限只是為了讓存到磁碟的日誌維持可讀大小，畫面上仍應如實顯示。
pub fn execute<S: AsRef<str>>(
    executable: &str,
    arguments: &[S],
    working_directory: &str,
    timeout: Duration,
    cancel: &CancelToken,
    on_output: Option<OutputCallback<'_>>,
) -> CoreResult<ProcessExecution> {
    if cancel.is_canceled() {
        return Err(CoreError::Canceled);
    }
    if is_shell_script(executable) && arguments.iter().any(|argument| argument.as_ref().contains(['\r', '\n'])) {
        return Err(CoreError::InvalidArgument(
            "批次檔（.cmd／.bat）不支援含 CR/LF 的多行參數；請改用單行參數或原生 .exe。".to_string(),
        ));
    }
    let (mut command, shell_script) = build_command(executable, arguments, working_directory);
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = command.spawn().map_err(|error| CoreError::Io(format!("無法啟動 CLI 程序：{error}")))?;
    let process_id = child.id();

    // 必須持續讀到 EOF，否則子程序寫滿管線時會卡住。
    // 慢速回呼透過管線背壓限制記憶體，不丟失正常輸出。
    let (sender, receiver) = mpsc::sync_channel(64);
    if let Some(stdout) = child.stdout.take() {
        spawn_reader(stdout, false, sender.clone());
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_reader(stderr, true, sender.clone());
    }
    drop(sender);

    let deadline = Instant::now() + timeout;
    let mut output = BoundedText::new();
    let mut error = BoundedText::new();
    let mut open_streams = 2;
    let mut timed_out = false;
    let mut exit_status = None;
    // 程序結束後仍可能有孫程序握著管線；最多再等到這個時間點就不再等輸出。
    let mut drain_deadline: Option<Instant> = None;

    let mut handle = |event: StreamEvent, open_streams: &mut i32| match event {
        StreamEvent::Chunk(is_error, text) => {
            if let Some(callback) = on_output {
                callback(is_error, &text);
            }
            if is_error {
                error.append(&text);
            } else {
                output.append(&text);
            }
        }
        StreamEvent::Closed => *open_streams -= 1,
    };

    loop {
        if open_streams <= 0 && exit_status.is_none() {
            // 子程序先關閉管線後仍可能繼續運作，避免斷線佇列造成忙碌迴圈。
            if cancel.wait(Duration::from_millis(15)) {
                win::kill_process_tree(process_id);
                let _ = child.kill();
                let _ = child.wait();
                return Err(CoreError::Canceled);
            }
        }
        match receiver.recv_timeout(Duration::from_millis(15)) {
            Ok(event) => handle(event, &mut open_streams),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => open_streams = 0,
        }

        if exit_status.is_none() {
            if cancel.is_canceled() {
                win::kill_process_tree(process_id);
                let _ = child.kill();
                let _ = child.wait();
                return Err(CoreError::Canceled);
            }

            match child.try_wait() {
                Ok(Some(status)) => {
                    exit_status = Some(status);
                    drain_deadline = Some(Instant::now() + Duration::from_secs(2));
                }
                Ok(None) => {
                    if Instant::now() >= deadline {
                        timed_out = true;
                        win::kill_process_tree(process_id);
                        let _ = child.kill();
                        exit_status = child.wait().ok();
                        drain_deadline = Some(Instant::now() + Duration::from_secs(2));
                    }
                }
                Err(error) => {
                    win::kill_process_tree(process_id);
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(CoreError::Io(error.to_string()));
                }
            }
        }

        if exit_status.is_some() && (open_streams <= 0 || drain_deadline.is_some_and(|limit| Instant::now() >= limit)) {
            break;
        }
    }

    // 收尾：把已經在佇列中的片段取完。
    // 只收取當下最多一個佇列容量，避免孫程序持續輸出使收尾永不結束。
    for _ in 0..64 {
        match receiver.try_recv() {
            Ok(event) => handle(event, &mut open_streams),
            Err(_) => break,
        }
    }
    drop(handle);

    Ok(ProcessExecution {
        exit_code: exit_status.and_then(|status| status.code()).unwrap_or(-1),
        standard_output: output.finish(),
        standard_error: error.finish(),
        timed_out,
        used_shell: shell_script,
    })
}
