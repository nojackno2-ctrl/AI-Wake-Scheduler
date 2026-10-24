//! 把 CLI 的原始輸出翻成終端機面板上一般人看得懂的句子。
//! 原始內容仍完整寫進執行日誌；這裡只決定畫面上要顯示什麼。

use crate::models::{CliActivityKind, CliKind};

/// 逐行整理單次執行的輸出。輸出片段不一定是完整一行，因此每個串流各自暫存未完成的部分。
pub struct ActivityTranslator {
    cli: CliKind,
    message_lines: Vec<String>,
    partial: [String; 2],
    codex_section: CodexSection,
    reply_started: bool,
    reason: Option<String>,
}

/// Codex 會把橫幅、提示詞與回覆都印到 stderr，依段落判斷哪些是雜訊。
#[derive(Clone, Copy, PartialEq, Eq)]
enum CodexSection {
    Header,
    Prompt,
    Reply,
    Tokens,
}

impl ActivityTranslator {
    pub fn new(cli: CliKind, message: &str) -> Self {
        Self {
            cli,
            message_lines: message.lines().map(|line| line.trim().to_string()).filter(|l| !l.is_empty()).collect(),
            partial: [String::new(), String::new()],
            codex_section: CodexSection::Header,
            reply_started: false,
            reason: None,
        }
    }

    /// 收下一段輸出，回傳其中已完整、且值得顯示的行。
    pub fn push(&mut self, is_error: bool, chunk: &str) -> Vec<(CliActivityKind, String)> {
        let index = usize::from(is_error);
        self.partial[index].push_str(chunk);
        let mut lines = Vec::new();
        while let Some(position) = self.partial[index].find('\n') {
            let line: String = self.partial[index].drain(..=position).collect();
            lines.push(line);
        }
        lines.into_iter().filter_map(|line| self.translate(is_error, &line)).collect()
    }

    /// 程序結束後取出最後一段沒有換行的輸出。
    pub fn finish(&mut self) -> Vec<(CliActivityKind, String)> {
        let rest = [std::mem::take(&mut self.partial[0]), std::mem::take(&mut self.partial[1])];
        [(false, &rest[0]), (true, &rest[1])]
            .into_iter()
            .filter_map(|(is_error, line)| self.translate(is_error, line))
            .collect()
    }

    /// 輸出中看到的失敗原因（已翻成白話），供「喚醒失敗」那一行使用。
    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }

    fn translate(&mut self, is_error: bool, raw: &str) -> Option<(CliActivityKind, String)> {
        let line = strip_ansi(raw.trim());
        if line.is_empty() {
            return None;
        }
        if let Some(explained) = explain_error(self.cli, &line) {
            self.reason = Some(explained.clone());
            return Some((CliActivityKind::ErrorOutput, explained));
        }
        if !is_error {
            let text = if self.reply_started { line } else { format!("AI 回覆：{line}") };
            self.reply_started = true;
            return Some((CliActivityKind::Output, text));
        }
        if self.cli == CliKind::Codex {
            return self.translate_codex(&line);
        }
        if is_noise(&line) || self.is_prompt_echo(&line) {
            return None;
        }
        if looks_like_error(&line) {
            let text = format!("錯誤：{line}");
            self.reason = Some(text.clone());
            return Some((CliActivityKind::ErrorOutput, text));
        }
        // 其他 stderr 多半是進度或提示，不是錯誤，以一般訊息顯示。
        Some((CliActivityKind::Output, line))
    }

    fn translate_codex(&mut self, line: &str) -> Option<(CliActivityKind, String)> {
        match line {
            "user" => self.codex_section = CodexSection::Prompt,
            "codex" => self.codex_section = CodexSection::Reply,
            "thinking" => {}
            "tokens used" => self.codex_section = CodexSection::Tokens,
            _ if self.codex_section == CodexSection::Tokens && is_number(line) => {
                self.codex_section = CodexSection::Header;
                return Some((CliActivityKind::Output, format!("本次用掉 {line} 個 token")));
            }
            // 回覆已從 stdout 顯示過；提示詞、橫幅與設定資訊不需要再顯示。
            _ if matches!(self.codex_section, CodexSection::Prompt | CodexSection::Reply) => {}
            _ if is_noise(line) => {}
            _ if looks_like_error(line) => {
                let text = format!("錯誤：{line}");
                self.reason = Some(text.clone());
                return Some((CliActivityKind::ErrorOutput, text));
            }
            _ => return Some((CliActivityKind::Output, line.to_string())),
        }
        None
    }

    fn is_prompt_echo(&self, line: &str) -> bool {
        self.message_lines.iter().any(|message| message == line)
    }
}

/// 喚醒開始時顯示的句子。
pub fn started(message: &str) -> String {
    format!("開始喚醒，送出訊息：「{}」", message.trim())
}

/// 喚醒成功時顯示的句子。
pub fn completed() -> String {
    "喚醒成功，AI 已回應。".to_string()
}

/// 喚醒失敗時顯示的句子；優先使用輸出中看到的原因，而不是只有結束碼。
pub fn failed(reason: Option<&str>, error: &str) -> String {
    match reason {
        Some(reason) => format!("喚醒失敗。{reason}"),
        None if error.is_empty() => "喚醒失敗，原因不明；詳細內容請看執行日誌。".to_string(),
        None => format!("喚醒失敗：{error} 詳細內容請看執行日誌。"),
    }
}

/// 認得的錯誤直接翻成白話，並說明接下來會怎樣或該怎麼做。
fn explain_error(cli: CliKind, line: &str) -> Option<String> {
    let lower = line.to_lowercase();
    let name = cli.display_name();
    if let Some(json) = line.strip_prefix("AGY_ERROR:") {
        return Some(explain_agy_error(name, json.trim()));
    }
    if lower.contains("credits balance is too low") || lower.contains("insufficient credits") {
        return Some(format!("{name} 的 AI 點數不足，等額度恢復後才能喚醒。"));
    }
    if lower.contains("usage limit") || lower.contains("rate limit") || lower.contains("quota exceeded") {
        return Some(format!("{name} 的額度已用完或請求太頻繁，稍後會再試。"));
    }
    if lower.contains("/login")
        || lower.contains("not logged in")
        || lower.contains("token has expired")
        || lower.contains("invalid api key")
        || lower.contains("unauthorized")
    {
        return Some(format!("{name} 的登入已失效，請在設定中重新登入。"));
    }
    None
}

fn explain_agy_error(name: &str, json: &str) -> String {
    let value = serde_json::from_str::<serde_json::Value>(json).ok();
    let code = value.as_ref().and_then(|v| v.get("error_code")).and_then(serde_json::Value::as_i64);
    let detail = value.as_ref().and_then(|v| v.get("short_error")).and_then(serde_json::Value::as_str);
    match code {
        Some(429) => format!("{name} 伺服器回應 429：額度已用完或請求太頻繁，稍後會再試。"),
        Some(401 | 403) => format!("{name} 伺服器拒絕存取（{}）：請在設定中重新登入。", code.unwrap_or_default()),
        Some(code) if code >= 500 => format!("{name} 伺服器暫時故障（代碼 {code}），稍後會再試。"),
        Some(code) => format!("{name} 回報錯誤（代碼 {code}）：{}", detail.unwrap_or("未提供說明")),
        None => format!("{name} 回報錯誤：{json}"),
    }
}

/// CLI 的橫幅、設定資訊與分隔線，對使用者沒有意義。
fn is_noise(line: &str) -> bool {
    const PREFIXES: [&str; 12] = [
        "Reading additional input from stdin",
        "OpenAI Codex v",
        "workdir:",
        "model:",
        "provider:",
        "approval:",
        "sandbox:",
        "reasoning effort:",
        "reasoning summaries:",
        "session id:",
        "mcp startup:",
        "warning: proceeding, even though",
    ];
    line.chars().all(|c| c == '-' || c == '=')
        || PREFIXES.iter().any(|prefix| line.to_lowercase().starts_with(&prefix.to_lowercase()))
}

fn looks_like_error(line: &str) -> bool {
    let lower = line.to_lowercase();
    ["error", "failed", "denied", "exception", "panic", "fatal"].iter().any(|word| lower.contains(word))
}

fn is_number(line: &str) -> bool {
    !line.is_empty() && line.chars().all(|c| c.is_ascii_digit() || c == ',')
}

/// 移除 ANSI 色彩控制碼，避免面板上出現亂碼。
fn strip_ansi(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for next in chars.by_ref() {
                    if next.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        result.push(c);
    }
    result
}
