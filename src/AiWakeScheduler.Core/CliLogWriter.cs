using System.Text;

namespace AiWakeScheduler.Core;

/// <summary>
/// 負責把 CLI 執行紀錄寫成檔案，並定期清掉舊日誌。
/// 與執行邏輯分離，讓 <see cref="CliRunner"/> 只處理「執行什麼」。
/// </summary>
public sealed class CliLogWriter(AppDataPaths paths)
{
    private static readonly UTF8Encoding Utf8NoBom = new(encoderShouldEmitUTF8Identifier: false);

    /// <summary>日誌保留上限；超過的舊檔會在下一次寫入時清除。</summary>
    private const int MaxRetainedLogFiles = 200;

    private int _pruned;

    public async Task<string> WriteRunLogAsync(
        CliKind kind,
        DateTimeOffset startedAt,
        string executable,
        IReadOnlyList<string> arguments,
        ProcessExecution execution,
        string error)
    {
        var body = new StringBuilder(1024)
            .Append("CLI: ").AppendLine(CliDisplayNames.Get(kind))
            .Append("開始時間: ").AppendLine(startedAt.ToString("O"))
            .Append("執行檔: ").AppendLine(executable)
            .Append("參數: ").AppendLine(FormatArguments(arguments))
            .Append("結束碼: ").Append(execution.ExitCode).AppendLine()
            .Append("Shell 模式: ").Append(execution.UsedShell).AppendLine()
            .Append("錯誤摘要: ").AppendLine(error)
            .AppendLine("--- stdout ---")
            .AppendLine(execution.StandardOutput)
            .AppendLine("--- stderr ---")
            .AppendLine(execution.StandardError)
            .ToString();

        return await WriteAsync($"{startedAt:yyyyMMdd-HHmmss}-{kind}.log", body).ConfigureAwait(false);
    }

    public async Task<string> WriteFailureLogAsync(
        CliKind kind,
        DateTimeOffset startedAt,
        string executable,
        Exception exception)
    {
        var body = new StringBuilder(512)
            .Append("CLI: ").AppendLine(CliDisplayNames.Get(kind))
            .Append("開始時間: ").AppendLine(startedAt.ToString("O"))
            .Append("執行檔: ").AppendLine(executable)
            .Append(exception)
            .ToString();

        return await WriteAsync($"{startedAt:yyyyMMdd-HHmmss}-{kind}-error.log", body).ConfigureAwait(false);
    }

    private async Task<string> WriteAsync(string fileName, string body)
    {
        try
        {
            paths.EnsureCreated();
            PruneOnce();
            var logPath = Path.Combine(paths.LogsDirectory, fileName);
            await File.WriteAllTextAsync(logPath, body, Utf8NoBom).ConfigureAwait(false);
            return logPath;
        }
        catch
        {
            // 寫日誌失敗不應該讓喚醒本身失敗
            return string.Empty;
        }
    }

    /// <summary>每個程式生命週期只清一次，避免每次執行都掃整個日誌目錄。</summary>
    private void PruneOnce()
    {
        if (Interlocked.Exchange(ref _pruned, 1) == 1)
        {
            return;
        }

        try
        {
            var files = new DirectoryInfo(paths.LogsDirectory).GetFiles("*.log");
            if (files.Length <= MaxRetainedLogFiles)
            {
                return;
            }

            Array.Sort(files, (left, right) => right.LastWriteTimeUtc.CompareTo(left.LastWriteTimeUtc));
            for (var i = MaxRetainedLogFiles; i < files.Length; i++)
            {
                try
                {
                    files[i].Delete();
                }
                catch
                {
                    // 個別檔案刪不掉（被佔用等）就跳過
                }
            }
        }
        catch
        {
            // 清理失敗不影響主要流程
        }
    }

    private static string FormatArguments(IReadOnlyList<string> arguments)
    {
        var builder = new StringBuilder();
        for (var i = 0; i < arguments.Count; i++)
        {
            if (i > 0)
            {
                builder.Append(' ');
            }

            var argument = arguments[i];
            if (argument.Length == 0 || argument.Contains(' '))
            {
                builder.Append('"').Append(argument).Append('"');
            }
            else
            {
                builder.Append(argument);
            }
        }
        return builder.ToString();
    }
}
