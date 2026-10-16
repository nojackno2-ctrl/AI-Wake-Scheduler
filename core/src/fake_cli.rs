//! 測試用的假 CLI：依參數延遲、把收到的參數寫成 JSON，並以指定結束碼結束。

fn value_after<'a>(arguments: &'a [String], flag: &str) -> Option<&'a String> {
    arguments.iter().position(|argument| argument == flag).and_then(|index| arguments.get(index + 1))
}

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();

    if let Some(delay) = value_after(&arguments, "--fake-delay-ms").and_then(|value| value.parse::<u64>().ok()) {
        std::thread::sleep(std::time::Duration::from_millis(delay));
    }
    if let Some(output) = value_after(&arguments, "--fake-cli-output") {
        let json = serde_json::to_string(&arguments).unwrap_or_default();
        let _ = std::fs::write(output, json);
    }
    let exit_code =
        value_after(&arguments, "--fake-exit-code").and_then(|value| value.parse::<i32>().ok()).unwrap_or(0);
    std::process::exit(exit_code);
}
