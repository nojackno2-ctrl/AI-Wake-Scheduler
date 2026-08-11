namespace AiWakeScheduler.Core;

/// <summary>
/// 依 <see cref="CliCatalog"/> 的描述建構各 CLI 的執行參數。
/// 本身不含任何個別 CLI 的知識，新增 CLI 不需要改動這個類別。
/// </summary>
public static class CliCommandBuilder
{
    /// <summary>
    /// 建構參數清單。
    /// 順序固定為：基本參數 → 預設模型 → 節省 Token 參數 → 逾時參數 → 使用者額外參數 → 提示詞。
    ///
    /// 提示詞一律排在最後：對 Codex 與 Claude CLI 是結尾的位置參數，
    /// 對 Antigravity 則是 <c>--print</c> 這個字串旗標的值。
    /// </summary>
    /// <param name="kind">CLI 種類</param>
    /// <param name="message">要傳送的喚醒訊息</param>
    /// <param name="additionalArguments">使用者在設定中填寫的額外參數</param>
    /// <param name="tokenSaverMode">是否啟用節省 Token 模式</param>
    /// <param name="timeout">應用程式的單次執行上限，用於產生 CLI 自身的逾時參數</param>
    public static IReadOnlyList<string> Build(
        CliKind kind,
        string message,
        string? additionalArguments = null,
        bool tokenSaverMode = true,
        TimeSpan? timeout = null)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(message);

        var descriptor = CliCatalog.Get(kind);
        var userArguments = ArgumentTokenizer.Parse(additionalArguments);

        var arguments = new List<string>(
            descriptor.BaseArguments.Count + descriptor.TokenSaverArguments.Count + userArguments.Count + 6);

        arguments.AddRange(descriptor.BaseArguments);

        if (descriptor.DefaultModel is not null && !SpecifiesModel(userArguments))
        {
            arguments.Add("--model");
            arguments.Add(descriptor.DefaultModel);
        }

        if (tokenSaverMode)
        {
            arguments.AddRange(descriptor.TokenSaverArguments);
        }

        if (timeout is { } limit && descriptor.TimeoutArguments is { } timeoutArguments)
        {
            arguments.AddRange(timeoutArguments(limit));
        }

        arguments.AddRange(userArguments);

        if (descriptor.PromptFlag is { } promptFlag)
        {
            arguments.Add(promptFlag);
        }
        arguments.Add(tokenSaverMode ? BuildMinimalReplyPrompt(message) : message);
        return arguments;
    }

    /// <summary>
    /// 節省 Token 模式的提示詞：保留使用者訊息，但明確要求最短回覆且不使用工具，
    /// 把輸出 Token（單價最高的部分）壓到只剩幾個字。
    /// </summary>
    public static string BuildMinimalReplyPrompt(string message) =>
        $"{message}{Environment.NewLine}只回「OK」，不要使用工具或說明。";

    private static bool SpecifiesModel(IReadOnlyList<string> userArguments)
    {
        for (var i = 0; i < userArguments.Count; i++)
        {
            var argument = userArguments[i];
            if (string.Equals(argument, "--model", StringComparison.OrdinalIgnoreCase) ||
                string.Equals(argument, "-m", StringComparison.Ordinal) ||
                argument.StartsWith("--model=", StringComparison.OrdinalIgnoreCase))
            {
                return true;
            }
        }
        return false;
    }
}
