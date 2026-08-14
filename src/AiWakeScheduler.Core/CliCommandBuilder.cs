namespace AiWakeScheduler.Core;

/// <summary>
/// 依 <see cref="CliCatalog"/> 的描述建構各 CLI 的執行參數。
/// 本身不含任何個別 CLI 的硬編碼路徑，支援模型選擇與思考程度（Reasoning / Thinking Effort）自訂。
/// </summary>
public static class CliCommandBuilder
{
    public static IReadOnlyList<string> Build(
        CliKind kind,
        string message,
        CliProfile? profile,
        bool tokenSaverMode = true,
        TimeSpan? timeout = null)
    {
        return Build(
            kind,
            message,
            profile?.Model,
            profile?.ThinkingEffort ?? ThinkingEffort.Default,
            profile?.AdditionalArguments,
            tokenSaverMode,
            timeout);
    }

    public static IReadOnlyList<string> Build(
        CliKind kind,
        string message,
        string? additionalArguments = null,
        bool tokenSaverMode = true,
        TimeSpan? timeout = null)
    {
        return Build(
            kind,
            message,
            model: null,
            thinkingEffort: ThinkingEffort.Default,
            additionalArguments,
            tokenSaverMode,
            timeout);
    }

    /// <summary>
    /// 建構參數清單。
    /// 順序固定為：基本參數 → 模型設定 → 思考程度 → 節省 Token 參數 → 逾時參數 → 使用者額外參數 → 提示詞。
    ///
    /// 提示詞一律排在最後：對 Codex 與 Claude CLI 是結尾的位置參數，
    /// 對 Antigravity 則是 <c>--print</c> 這個字串旗標的值。
    /// </summary>
    /// <param name="kind">CLI 種類</param>
    /// <param name="message">要傳送的喚醒訊息</param>
    /// <param name="model">自訂模型名稱（null 或空表示交給 CLI 預設或 Descriptor 預設）</param>
    /// <param name="thinkingEffort">思考程度 / 推理強度</param>
    /// <param name="additionalArguments">使用者在設定中填寫的額外參數</param>
    /// <param name="tokenSaverMode">是否啟用節省 Token 模式</param>
    /// <param name="timeout">應用程式的單次執行上限，用於產生 CLI 自身的逾時參數</param>
    public static IReadOnlyList<string> Build(
        CliKind kind,
        string message,
        string? model,
        ThinkingEffort thinkingEffort,
        string? additionalArguments = null,
        bool tokenSaverMode = true,
        TimeSpan? timeout = null)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(message);

        var descriptor = CliCatalog.Get(kind);
        var userArguments = ArgumentTokenizer.Parse(additionalArguments);

        var arguments = new List<string>(
            descriptor.BaseArguments.Count + descriptor.TokenSaverArguments.Count + userArguments.Count + 8);

        arguments.AddRange(descriptor.BaseArguments);

        // 1. 模型參數
        var modelSpecified = SpecifiesModel(userArguments);
        var effectiveModel = GetSpecifiedModel(userArguments) ??
            (!string.IsNullOrWhiteSpace(model) ? model.Trim() : descriptor.DefaultModel);
        if (!modelSpecified)
        {
            if (!string.IsNullOrWhiteSpace(effectiveModel))
            {
                arguments.Add("--model");
                arguments.Add(effectiveModel);
            }
        }

        // 2. 思考程度 / 推理強度參數
        if (!SpecifiesEffort(userArguments))
        {
            var effectiveEffort = thinkingEffort != ThinkingEffort.Default
                ? thinkingEffort
                : (tokenSaverMode ? ThinkingEffort.Low : ThinkingEffort.Default);

            AppendEffortArguments(arguments, kind, descriptor.NormalizeEffort(effectiveModel, effectiveEffort));
        }

        // 3. 節省 Token 其他參數（如沙箱、停用外掛/MCP、停用技能展開等）
        if (tokenSaverMode)
        {
            arguments.AddRange(descriptor.TokenSaverArguments);
        }

        // 4. 逾時參數
        if (timeout is { } limit && descriptor.TimeoutArguments is { } timeoutArguments)
        {
            arguments.AddRange(timeoutArguments(limit));
        }

        // 5. 使用者自訂額外參數
        arguments.AddRange(userArguments);

        // 6. 提示詞
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

    private static void AppendEffortArguments(
        List<string> arguments,
        CliKind kind,
        ThinkingEffort effort)
    {
        if (effort == ThinkingEffort.Default)
        {
            return;
        }

        switch (kind)
        {
            // 以 `agy --help` 實測核對：--effort 只接受 low|medium|high，
            // 沒有 minimal/xhigh/max，超出範圍一律夾到最接近的合法值。
            case CliKind.Antigravity:
                switch (effort)
                {
                    case ThinkingEffort.Minimal:
                    case ThinkingEffort.Low:
                        arguments.Add("--effort");
                        arguments.Add("low");
                        break;
                    case ThinkingEffort.Medium:
                        arguments.Add("--effort");
                        arguments.Add("medium");
                        break;
                    case ThinkingEffort.High:
                    case ThinkingEffort.XHigh:
                    case ThinkingEffort.Max:
                    case ThinkingEffort.Ultra:
                        arguments.Add("--effort");
                        arguments.Add("high");
                        break;
                }
                break;

            case CliKind.AntigravityClaude:
                // 這個設定檔僅提供 claude-sonnet-4-6 / claude-opus-4-6-thinking / gpt-oss-120b-medium，
                // 三者皆已內建固定思考程度，agy 對其一律拒絕 --effort（實測：
                // "--effort is not supported for model ..."），故永遠略過。
                return;

            // 實際可用值由 Codex App Server model/list 核對；各模型不相容的較高等級
            // 已先由 CliDescriptor.NormalizeEffort 降到該模型可接受的最高值。
            case CliKind.Codex:
                switch (effort)
                {
                    case ThinkingEffort.Minimal:
                    case ThinkingEffort.Low:
                        arguments.Add("-c");
                        arguments.Add("model_reasoning_effort=\"low\"");
                        break;
                    case ThinkingEffort.Medium:
                        arguments.Add("-c");
                        arguments.Add("model_reasoning_effort=\"medium\"");
                        break;
                    case ThinkingEffort.High:
                        arguments.Add("-c");
                        arguments.Add("model_reasoning_effort=\"high\"");
                        break;
                    case ThinkingEffort.XHigh:
                        arguments.Add("-c");
                        arguments.Add("model_reasoning_effort=\"xhigh\"");
                        break;
                    case ThinkingEffort.Max:
                        arguments.Add("-c");
                        arguments.Add("model_reasoning_effort=\"max\"");
                        break;
                    case ThinkingEffort.Ultra:
                        arguments.Add("-c");
                        arguments.Add("model_reasoning_effort=\"ultra\"");
                        break;
                }
                break;

            // 以 `claude --help` 核對：--effort 合法值為 low|medium|high|xhigh|max。
            case CliKind.Claude:
                switch (effort)
                {
                    case ThinkingEffort.Minimal:
                    case ThinkingEffort.Low:
                        arguments.Add("--effort");
                        arguments.Add("low");
                        break;
                    case ThinkingEffort.Medium:
                        arguments.Add("--effort");
                        arguments.Add("medium");
                        break;
                    case ThinkingEffort.High:
                        arguments.Add("--effort");
                        arguments.Add("high");
                        break;
                    case ThinkingEffort.XHigh:
                        arguments.Add("--effort");
                        arguments.Add("xhigh");
                        break;
                    case ThinkingEffort.Max:
                    case ThinkingEffort.Ultra:
                        arguments.Add("--effort");
                        arguments.Add("max");
                        break;
                }
                break;
        }
    }

    private static bool SpecifiesModel(IReadOnlyList<string> userArguments)
    {
        for (var i = 0; i < userArguments.Count; i++)
        {
            var argument = userArguments[i];
            if (string.Equals(argument, "--model", StringComparison.OrdinalIgnoreCase) ||
                string.Equals(argument, "-m", StringComparison.Ordinal) ||
                argument.StartsWith("--model=", StringComparison.OrdinalIgnoreCase) ||
                argument.StartsWith("-m=", StringComparison.Ordinal))
            {
                return true;
            }
        }
        return false;
    }

    private static string? GetSpecifiedModel(IReadOnlyList<string> userArguments)
    {
        for (var i = 0; i < userArguments.Count; i++)
        {
            var argument = userArguments[i];
            if ((string.Equals(argument, "--model", StringComparison.OrdinalIgnoreCase) ||
                 string.Equals(argument, "-m", StringComparison.Ordinal)) &&
                i + 1 < userArguments.Count)
            {
                return userArguments[i + 1];
            }

            const string longPrefix = "--model=";
            if (argument.StartsWith(longPrefix, StringComparison.OrdinalIgnoreCase))
            {
                return argument[longPrefix.Length..];
            }

            const string shortPrefix = "-m=";
            if (argument.StartsWith(shortPrefix, StringComparison.Ordinal))
            {
                return argument[shortPrefix.Length..];
            }
        }

        return null;
    }

    private static bool SpecifiesEffort(IReadOnlyList<string> userArguments)
    {
        for (var i = 0; i < userArguments.Count; i++)
        {
            var argument = userArguments[i];
            if (string.Equals(argument, "--effort", StringComparison.OrdinalIgnoreCase) ||
                string.Equals(argument, "-e", StringComparison.Ordinal) ||
                argument.StartsWith("--effort=", StringComparison.OrdinalIgnoreCase) ||
                argument.StartsWith("-e=", StringComparison.Ordinal) ||
                argument.Contains("model_reasoning_effort", StringComparison.OrdinalIgnoreCase))
            {
                return true;
            }
        }
        return false;
    }
}
