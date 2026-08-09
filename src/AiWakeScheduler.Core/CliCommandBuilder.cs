namespace AiWakeScheduler.Core;

/// <summary>
/// 建構不同 CLI 的執行參數清單。
/// </summary>
public static class CliCommandBuilder
{
    /// <summary>
    /// 依據指定的 CLI 種類、訊息與設定建構參數。
    /// </summary>
    public static IReadOnlyList<string> Build(
        CliKind kind,
        string message,
        string? additionalArguments = null,
        bool tokenSaverMode = true)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(message);

        var arguments = new List<string>();
        switch (kind)
        {
            case CliKind.Antigravity:
                arguments.Add("--print");
                if (tokenSaverMode)
                {
                    arguments.Add("--effort");
                    arguments.Add("low");
                    arguments.Add("--disable-slash-commands");
                }
                break;
            case CliKind.Codex:
                arguments.Add("exec");
                arguments.Add("--skip-git-repo-check");
                if (tokenSaverMode)
                {
                    arguments.Add("--sandbox");
                    arguments.Add("read-only");
                    arguments.Add("-c");
                    arguments.Add("model_reasoning_effort=\"low\"");
                    arguments.Add("-c");
                    arguments.Add("model_verbosity=\"low\"");
                }
                break;
            case CliKind.Claude:
                arguments.Add("--print");
                if (tokenSaverMode)
                {
                    arguments.Add("--effort");
                    arguments.Add("low");
                    arguments.Add("--tools");
                    arguments.Add(string.Empty);
                    arguments.Add("--no-session-persistence");
                }
                break;
            default:
                throw new ArgumentOutOfRangeException(nameof(kind), kind, null);
        }

        arguments.AddRange(ArgumentTokenizer.Parse(additionalArguments));
        arguments.Add(tokenSaverMode ? BuildMinimalReplyPrompt(message) : message);
        return arguments;
    }

    /// <summary>
    /// 建立節省 Token 模式的簡短回覆提示。
    /// </summary>
    public static string BuildMinimalReplyPrompt(string message) =>
        $"{message}{Environment.NewLine}只回覆上面這句，不要使用工具。";
}

