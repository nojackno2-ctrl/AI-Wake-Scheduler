using System.Diagnostics;
using System.Text;

namespace AiWakeScheduler.Core;

public sealed class CliProbeResult
{
    public required CliKind Cli { get; init; }
    public required bool Succeeded { get; init; }
    public required string Summary { get; init; }
    public string ExecutablePath { get; init; } = string.Empty;
}

public sealed class CliRunner(AppDataPaths paths)
{
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

            var arguments = CliCommandBuilder.Build(kind, message, profile.AdditionalArguments, tokenSaverMode);
            var execution = await ExecuteProcessAsync(
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
            result.LogPath = await WriteLogAsync(kind, startedAt, executable, arguments, execution, result.Error)
                .ConfigureAwait(false);
        }
        catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
        {
            result.Error = "執行已取消。";
        }
        catch (Exception ex)
        {
            result.Error = ex.Message;
            result.LogPath = await WriteFailureLogAsync(kind, startedAt, result.ExecutablePath, ex).ConfigureAwait(false);
        }

        result.FinishedAt = DateTimeOffset.Now;
        return result;
    }

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
            var execution = await ExecuteProcessAsync(
                executable,
                ["--version"],
                workingDirectory,
                TimeSpan.FromSeconds(15),
                cancellationToken).ConfigureAwait(false);
            var firstLine = execution.StandardOutput
                .Split(['\r', '\n'], StringSplitOptions.RemoveEmptyEntries)
                .FirstOrDefault();

            return new CliProbeResult
            {
                Cli = kind,
                Succeeded = execution.ExitCode == 0 && !execution.TimedOut,
                ExecutablePath = executable,
                Summary = execution.TimedOut
                    ? "檢查逾時"
                    : execution.ExitCode == 0
                        ? firstLine ?? "可執行"
                        : string.IsNullOrWhiteSpace(execution.StandardError)
                            ? $"結束碼 {execution.ExitCode}"
                            : execution.StandardError.Trim()
            };
        }
        catch (Exception ex)
        {
            return new CliProbeResult { Cli = kind, Succeeded = false, Summary = ex.Message };
        }
    }

    private static async Task<ProcessExecution> ExecuteProcessAsync(
        string executable,
        IReadOnlyList<string> arguments,
        string workingDirectory,
        TimeSpan timeout,
        CancellationToken cancellationToken)
    {
        var shellScript = OperatingSystem.IsWindows() &&
                          (executable.EndsWith(".cmd", StringComparison.OrdinalIgnoreCase) ||
                           executable.EndsWith(".bat", StringComparison.OrdinalIgnoreCase));
        var startInfo = new ProcessStartInfo
        {
            FileName = executable,
            WorkingDirectory = workingDirectory,
            UseShellExecute = shellScript,
            CreateNoWindow = !shellScript,
            RedirectStandardOutput = !shellScript,
            RedirectStandardError = !shellScript,
            StandardOutputEncoding = shellScript ? null : Encoding.UTF8,
            StandardErrorEncoding = shellScript ? null : Encoding.UTF8
        };
        foreach (var argument in arguments)
        {
            startInfo.ArgumentList.Add(argument);
        }
        if (!shellScript)
        {
            startInfo.Environment["NO_COLOR"] = "1";
        }

        using var process = new Process { StartInfo = startInfo };
        if (!process.Start())
        {
            throw new InvalidOperationException("無法啟動 CLI 程序。");
        }

        var outputTask = shellScript ? Task.FromResult(string.Empty) : process.StandardOutput.ReadToEndAsync(cancellationToken);
        var errorTask = shellScript ? Task.FromResult(string.Empty) : process.StandardError.ReadToEndAsync(cancellationToken);
        using var timeoutSource = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        timeoutSource.CancelAfter(timeout);
        var timedOut = false;

        try
        {
            await process.WaitForExitAsync(timeoutSource.Token).ConfigureAwait(false);
        }
        catch (OperationCanceledException) when (!cancellationToken.IsCancellationRequested)
        {
            timedOut = true;
            TryKill(process);
            await process.WaitForExitAsync(CancellationToken.None).ConfigureAwait(false);
        }
        catch (OperationCanceledException)
        {
            TryKill(process);
            throw;
        }

        return new ProcessExecution(
            process.ExitCode,
            await outputTask.ConfigureAwait(false),
            await errorTask.ConfigureAwait(false),
            timedOut,
            shellScript);
    }

    private async Task<string> WriteLogAsync(
        CliKind kind,
        DateTimeOffset startedAt,
        string executable,
        IReadOnlyList<string> arguments,
        ProcessExecution execution,
        string error)
    {
        paths.EnsureCreated();
        var logPath = Path.Combine(paths.LogsDirectory, $"{startedAt:yyyyMMdd-HHmmss}-{kind}.log");
        var body = new StringBuilder()
            .AppendLine($"CLI: {CliDisplayNames.Get(kind)}")
            .AppendLine($"開始時間: {startedAt:O}")
            .AppendLine($"執行檔: {executable}")
            .AppendLine($"參數: {string.Join(' ', arguments.Select(MaskForLog))}")
            .AppendLine($"結束碼: {execution.ExitCode}")
            .AppendLine($"Shell 模式: {execution.UsedShell}")
            .AppendLine($"錯誤摘要: {error}")
            .AppendLine("--- stdout ---")
            .AppendLine(execution.StandardOutput)
            .AppendLine("--- stderr ---")
            .AppendLine(execution.StandardError)
            .ToString();
        await File.WriteAllTextAsync(logPath, body, new UTF8Encoding(false)).ConfigureAwait(false);
        return logPath;
    }

    private async Task<string> WriteFailureLogAsync(CliKind kind, DateTimeOffset startedAt, string executable, Exception exception)
    {
        paths.EnsureCreated();
        var logPath = Path.Combine(paths.LogsDirectory, $"{startedAt:yyyyMMdd-HHmmss}-{kind}-error.log");
        await File.WriteAllTextAsync(
            logPath,
            $"CLI: {CliDisplayNames.Get(kind)}{Environment.NewLine}開始時間: {startedAt:O}{Environment.NewLine}執行檔: {executable}{Environment.NewLine}{exception}",
            new UTF8Encoding(false)).ConfigureAwait(false);
        return logPath;
    }

    private static string MaskForLog(string argument) => argument.Contains(' ') ? $"\"{argument}\"" : argument;

    private static void ValidateWorkingDirectory(string workingDirectory)
    {
        if (string.IsNullOrWhiteSpace(workingDirectory) || !Directory.Exists(workingDirectory))
        {
            throw new DirectoryNotFoundException($"工作目錄不存在：{workingDirectory}");
        }
    }

    private static void TryKill(Process process)
    {
        try
        {
            if (!process.HasExited)
            {
                process.Kill(true);
            }
        }
        catch (InvalidOperationException)
        {
        }
    }

    private sealed record ProcessExecution(
        int ExitCode,
        string StandardOutput,
        string StandardError,
        bool TimedOut,
        bool UsedShell);
}
