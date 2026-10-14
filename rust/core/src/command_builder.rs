//! 依 `catalog` 的描述建構各 CLI 的執行參數。
//! 本身不含任何個別 CLI 的硬編碼路徑，支援模型選擇與思考程度（Reasoning / Thinking Effort）自訂。

use std::time::Duration;

use crate::catalog;
use crate::error::{CoreError, CoreResult};
use crate::models::{CliKind, CliProfile, ThinkingEffort};
use crate::tokenizer;

pub fn build_for_profile(
    kind: CliKind,
    message: &str,
    profile: Option<&CliProfile>,
    token_saver_mode: bool,
    timeout: Option<Duration>,
) -> CoreResult<Vec<String>> {
    build(
        kind,
        message,
        profile.map(|p| p.model.as_str()),
        profile.map(|p| p.thinking_effort).unwrap_or(ThinkingEffort::Default),
        profile.map(|p| p.additional_arguments.as_str()),
        token_saver_mode,
        timeout,
    )
}

pub fn build_with_arguments(
    kind: CliKind,
    message: &str,
    additional_arguments: Option<&str>,
    token_saver_mode: bool,
    timeout: Option<Duration>,
) -> CoreResult<Vec<String>> {
    build(kind, message, None, ThinkingEffort::Default, additional_arguments, token_saver_mode, timeout)
}

/// 建構參數清單。
/// 順序固定為：基本參數 → 模型設定 → 思考程度 → 節省 Token 參數 → 逾時參數 → 使用者額外參數 → 提示詞。
///
/// 提示詞一律排在最後：對 Codex 與 Claude CLI 是結尾的位置參數，
/// 對 Antigravity 則是 `--print` 這個字串旗標的值。
pub fn build(
    kind: CliKind,
    message: &str,
    model: Option<&str>,
    thinking_effort: ThinkingEffort,
    additional_arguments: Option<&str>,
    token_saver_mode: bool,
    timeout: Option<Duration>,
) -> CoreResult<Vec<String>> {
    if message.trim().is_empty() {
        return Err(CoreError::InvalidArgument("喚醒訊息不可為空白。".to_string()));
    }

    let descriptor = catalog::get(kind);
    let user_arguments = tokenizer::parse(additional_arguments)?;

    let mut arguments: Vec<String> = Vec::with_capacity(
        descriptor.base_arguments.len() + descriptor.token_saver_arguments.len() + user_arguments.len() + 8,
    );
    arguments.extend(descriptor.base_arguments.iter().map(|value| value.to_string()));

    // 1. 模型參數
    let model_specified = specifies_model(&user_arguments);
    let effective_model: Option<String> =
        get_specified_model(&user_arguments).or_else(|| match model.map(str::trim).filter(|value| !value.is_empty()) {
            Some(value) => Some(value.to_string()),
            None => descriptor.default_model.map(str::to_string),
        });
    if !model_specified {
        if let Some(value) = effective_model.as_deref().filter(|value| !value.trim().is_empty()) {
            arguments.push("--model".to_string());
            arguments.push(value.to_string());
        }
    }

    // 2. 思考程度 / 推理強度參數
    if !specifies_effort(&user_arguments) {
        let effective_effort = if thinking_effort != ThinkingEffort::Default {
            thinking_effort
        } else if token_saver_mode {
            ThinkingEffort::Low
        } else {
            ThinkingEffort::Default
        };
        append_effort_arguments(
            &mut arguments,
            kind,
            descriptor.normalize_effort(effective_model.as_deref(), effective_effort),
        );
    }

    // 3. 節省 Token 其他參數（如沙箱、停用外掛/MCP、停用技能展開等）
    if token_saver_mode {
        arguments.extend(descriptor.token_saver_arguments.iter().map(|value| value.to_string()));
    }

    // 4. 逾時參數
    if let (Some(limit), Some(timeout_arguments)) = (timeout, descriptor.timeout_arguments) {
        arguments.extend(timeout_arguments(limit));
    }

    // 5. 使用者自訂額外參數
    arguments.extend(user_arguments);

    // 6. 提示詞
    if let Some(prompt_flag) = descriptor.prompt_flag {
        arguments.push(prompt_flag.to_string());
    }
    arguments.push(if token_saver_mode { build_minimal_reply_prompt(message) } else { message.to_string() });
    Ok(arguments)
}

/// 節省 Token 模式的提示詞：保留使用者訊息，但明確要求最短回覆且不使用工具，
/// 把輸出 Token（單價最高的部分）壓到只剩幾個字。
pub fn build_minimal_reply_prompt(message: &str) -> String {
    format!("{message}\r\n只回「OK」，不要使用工具或說明。")
}

fn append_effort_arguments(arguments: &mut Vec<String>, kind: CliKind, effort: ThinkingEffort) {
    use ThinkingEffort as E;

    let mut push = |flag: &str, value: &str| {
        arguments.push(flag.to_string());
        arguments.push(value.to_string());
    };

    match kind {
        // 以 `agy --help` 實測核對：--effort 只接受 low|medium|high，
        // 沒有 minimal/xhigh/max，超出範圍一律夾到最接近的合法值。
        CliKind::Antigravity => match effort {
            E::Default => {}
            E::Minimal | E::Low => push("--effort", "low"),
            E::Medium => push("--effort", "medium"),
            E::High | E::XHigh | E::Max | E::Ultra => push("--effort", "high"),
        },

        // 此群組使用包含思考程度後綴的完整模型 ID（Claude 5.5 / GPT-OSS），
        // 由模型 ID 決定程度，不再附加可能衝突的 --effort。
        CliKind::AntigravityClaude => {}

        // 實際可用值由 Codex App Server model/list 核對；各模型不相容的較高等級
        // 已先由 normalize_effort 降到該模型可接受的最高值。
        CliKind::Codex => {
            let level = match effort {
                E::Default => return,
                E::Minimal | E::Low => "low",
                E::Medium => "medium",
                E::High => "high",
                E::XHigh => "xhigh",
                E::Max => "max",
                E::Ultra => "ultra",
            };
            push("-c", &format!("model_reasoning_effort=\"{level}\""));
        }

        // 以 `claude --help` 核對：--effort 合法值為 low|medium|high|xhigh|max。
        CliKind::Claude => match effort {
            E::Default => {}
            E::Minimal | E::Low => push("--effort", "low"),
            E::Medium => push("--effort", "medium"),
            E::High => push("--effort", "high"),
            E::XHigh => push("--effort", "xhigh"),
            E::Max | E::Ultra => push("--effort", "max"),
        },
    }
}

fn starts_with_ignore_case(text: &str, prefix: &str) -> bool {
    text.len() >= prefix.len()
        && text.is_char_boundary(prefix.len())
        && text[..prefix.len()].eq_ignore_ascii_case(prefix)
}

fn specifies_model(user_arguments: &[String]) -> bool {
    user_arguments.iter().any(|argument| {
        argument.eq_ignore_ascii_case("--model")
            || argument == "-m"
            || starts_with_ignore_case(argument, "--model=")
            || argument.starts_with("-m=")
    })
}

fn get_specified_model(user_arguments: &[String]) -> Option<String> {
    for (index, argument) in user_arguments.iter().enumerate() {
        if (argument.eq_ignore_ascii_case("--model") || argument == "-m") && index + 1 < user_arguments.len() {
            return Some(user_arguments[index + 1].clone());
        }
        if starts_with_ignore_case(argument, "--model=") {
            return Some(argument["--model=".len()..].to_string());
        }
        if let Some(value) = argument.strip_prefix("-m=") {
            return Some(value.to_string());
        }
    }
    None
}

fn specifies_effort(user_arguments: &[String]) -> bool {
    user_arguments.iter().any(|argument| {
        argument.eq_ignore_ascii_case("--effort")
            || argument == "-e"
            || starts_with_ignore_case(argument, "--effort=")
            || argument.starts_with("-e=")
            || argument.to_ascii_lowercase().contains("model_reasoning_effort")
    })
}
