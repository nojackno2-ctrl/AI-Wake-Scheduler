namespace AiWakeScheduler.Core;

/// <summary>
/// CLI 探測結果模型。
/// </summary>
public sealed class CliProbeResult
{
    public required CliKind Cli { get; init; }
    public required bool Succeeded { get; init; }
    public required string Summary { get; init; }
    public string ExecutablePath { get; init; } = string.Empty;
}

/// <summary>
/// 決定「要對哪個 CLI 執行什麼」的政策層。
/// 實際的程序啟動交給 <see cref="ProcessRunner"/>，日誌交給 <see cref="CliLogWriter"/>。
/// </summary>
public sealed class CliRunner(AppDataPaths paths) : ICliRunner
{
    private static readonly TimeSpan ProbeTimeout = TimeSpan.FromSeconds(15);
    private static readonly string[] ProbeArguments = ["--version"];

    private readonly CliLogWriter _log = new(paths);

    /// <summary>
    /// 執行指定的 CLI 命令，並記錄執行日誌。
    /// </summary>
    public async Task<CliRunResult> RunAsync(
        CliKind kind,
        CliProfile profile,
        string message,
        string workingDirectory,
        TimeSpan timeout,
        bool tokenSaverMode = true,
        CancellationToken cancellationToken = default)
    {
        var startedAt = DateTimeOffset.Now;
        var result = new CliRunResult { Cli = kind, StartedAt = startedAt };

        try
        {
            ValidateWorkingDirectory(workingDirectory);
            var executable = ExecutableLocator.Resolve(kind, profile.Executable, workingDirectory)
                ?? throw new FileNotFoundException($"找不到 {CliDisplayNames.Get(kind)}。請在設定中指定可執行檔。");
            result.ExecutablePath = executable;

            var arguments = CliCommandBuilder.Build(
                kind,
                message,
                profile.AdditionalArguments,
                tokenSaverMode,
                timeout);

            var execution = await ProcessRunner.ExecuteAsync(
                executable,
                arguments,
                workingDirectory,
                timeout,
                cancellationToken).ConfigureAwait(false);

            result.ExitCode = execution.ExitCode;
            result.Succeeded = execution.ExitCode == 0 && !execution.TimedOut;
            result.Error = execution.TimedOut
                ? $"超過 {timeout.TotalMinutes:0} 分鐘，程序已終止。"
                : execution.ExitCode == 0 ? string.Empty : $"CLI 結束碼為 {execution.ExitCode}。";
            result.LogPath = await _log
                .WriteRunLogAsync(kind, startedAt, executable, arguments, execution, result.Error)
                .ConfigureAwait(false);
        }
        catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
        {
            result.Error = "執行已取消。";
        }
        catch (Exception ex)
        {
            result.Error = ex.Message;
            result.LogPath = await _log
                .WriteFailureLogAsync(kind, startedAt, result.ExecutablePath, ex)
                .ConfigureAwait(false);
        }

        result.FinishedAt = DateTimeOffset.Now;
        return result;
    }

    /// <summary>
    /// 探測指定 CLI 是否可正常執行（只跑 --version，不會消耗任何 Token）。
    /// </summary>
    public async Task<CliProbeResult> ProbeAsync(
        CliKind kind,
        CliProfile profile,
        string workingDirectory,
        CancellationToken cancellationToken = default)
    {
        try
        {
            ValidateWorkingDirectory(workingDirectory);
            var executable = ExecutableLocator.Resolve(kind, profile.Executable, workingDirectory)
                ?? throw new FileNotFoundException($"找不到 {CliDisplayNames.Get(kind)}。");

            var execution = await ProcessRunner.ExecuteAsync(
                executable,
                ProbeArguments,
                workingDirectory,
                ProbeTimeout,
                cancellationToken).ConfigureAwait(false);

            return new CliProbeResult
            {
                Cli = kind,
                Succeeded = execution.ExitCode == 0 && !execution.TimedOut,
                ExecutablePath = executable,
                Summary = SummarizeProbe(execution)
            };
        }
        catch (Exception ex)
        {
            return new CliProbeResult { Cli = kind, Succeeded = false, Summary = ex.Message };
        }
    }

    private static string SummarizeProbe(ProcessExecution execution)
    {
        if (execution.TimedOut)
        {
            return "檢查逾時";
        }

        if (execution.ExitCode != 0)
        {
            return string.IsNullOrWhiteSpace(execution.StandardError)
                ? $"結束碼 {execution.ExitCode}"
                : execution.StandardError.Trim();
        }

        var output = string.IsNullOrWhiteSpace(execution.StandardOutput)
            ? execution.StandardError
            : execution.StandardOutput;

        return FirstNonEmptyLine(output) ?? "可執行";
    }

    private static string? FirstNonEmptyLine(string text)
    {
        var start = 0;
        while (start < text.Length)
        {
            var end = text.IndexOfAny(['\r', '\n'], start);
            if (end < 0)
            {
                end = text.Length;
            }

            if (end > start)
            {
                return text[start..end];
            }

            start = end + 1;
        }

        return null;
    }

    private static void ValidateWorkingDirectory(string workingDirectory)
    {
        if (string.IsNullOrWhiteSpace(workingDirectory) || !Directory.Exists(workingDirectory))
        {
            throw new DirectoryNotFoundException($"工作目錄不存在：{workingDirectory}");
        }
    }
}
