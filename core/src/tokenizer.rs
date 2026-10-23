use crate::error::{CoreError, CoreResult};

/// 解析命令列參數字串，處理雙引號與斜線轉義。
/// 雙引號未正常閉合時回傳 `CoreError::Format`。
pub fn parse(command_line: Option<&str>) -> CoreResult<Vec<String>> {
    let Some(command_line) = command_line.filter(|text| !text.trim().is_empty()) else {
        return Ok(Vec::new());
    };

    let mut arguments = Vec::new();
    let mut current = String::with_capacity(command_line.len());
    let mut in_quotes = false;
    let mut backslashes = 0usize;
    let mut started = false;

    for character in command_line.chars() {
        if character == '\\' {
            started = true;
            backslashes += 1;
            continue;
        }

        if character == '"' {
            started = true;
            current.extend(std::iter::repeat_n('\\', backslashes / 2));
            if backslashes % 2 == 1 {
                current.push('"');
            } else {
                in_quotes = !in_quotes;
            }
            backslashes = 0;
            continue;
        }

        current.extend(std::iter::repeat_n('\\', backslashes));
        backslashes = 0;

        if character.is_whitespace() && !in_quotes {
            if started {
                arguments.push(std::mem::take(&mut current));
                started = false;
            }
        } else {
            started = true;
            current.push(character);
        }
    }

    current.extend(std::iter::repeat_n('\\', backslashes));
    if in_quotes {
        return Err(CoreError::Format("額外參數含有未結束的雙引號。".to_string()));
    }

    if started {
        arguments.push(current);
    }
    Ok(arguments)
}
