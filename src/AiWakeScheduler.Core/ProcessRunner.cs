using System.Diagnostics;
using System.Text;

namespace AiWakeScheduler.Core;

/// <summary>
/// 子程序的執行結果。
/// </summary>
public sealed record ProcessExecution(
    int ExitCode,
    string StandardOutput,
    string StandardError,
    bool TimedOut,
    bool UsedShell);

/// <summary>
/// 純粹的子程序啟動機制，不含任何 CLI 或排程知識。
/// 從 <see cref="CliRunner"/> 拆出來後，CLI 政策（要送什麼參數、怎麼記錄）
/// 與程序機制（怎麼啟動、怎麼收輸出、怎麼逾時）互不相依。
/// </summary>
public static class ProcessRunner
{
    /// <summary>
    /// 單一串流最多保留的字元數。CLI 若話很多，超過的部分會被丟棄而不是一路長進記憶體，
    /// 同時也讓日誌檔維持在可讀的大小。
    /// </summary>
    public const int MaxCapturedCharacters = 32 * 1024;

    public static async Task<ProcessExecution> ExecuteAsync(
        string executable,
        IReadOnlyList<string> arguments,
        string workingDirectory,
        TimeSpan timeout,
        CancellationToken cancellationToken)
    {
        // .cmd / .bat 沒有辦法在不開視窗的情況下重導向，只能交給 Shell 執行。
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

        for (var i = 0; i < arguments.Count; i++)
        {
            startInfo.ArgumentList.Add(arguments[i]);
        }

        if (!shellScript)
        {
            // 沒有 ANSI 色碼，擷取到的輸出更小也更好讀。
            startInfo.Environment["NO_COLOR"] = "1";
            startInfo.Environment["TERM"] = "dumb";
        }

        using var process = new Process { StartInfo = startInfo };
        if (!process.Start())
        {
            throw new InvalidOperationException("無法啟動 CLI 程序。");
        }

        // 必須持續讀到 EOF，否則子程序寫滿管線時會卡住；
        // 超過上限的內容會被丟棄，不會累積在記憶體裡。
        var outputTask = shellScript
            ? Task.FromResult(string.Empty)
            : ReadBoundedAsync(process.StandardOutput, cancellationToken);
        var errorTask = shellScript
            ? Task.FromResult(string.Empty)
            : ReadBoundedAsync(process.StandardError, cancellationToken);

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
            await SafeAwaitAsync(outputTask).ConfigureAwait(false),
            await SafeAwaitAsync(errorTask).ConfigureAwait(false),
            timedOut,
            shellScript);
    }

    private static async Task<string> SafeAwaitAsync(Task<string> task)
    {
        try
        {
            return await task.ConfigureAwait(false);
        }
        catch
        {
            return string.Empty;
        }
    }

    private static async Task<string> ReadBoundedAsync(StreamReader reader, CancellationToken cancellationToken)
    {
        var buffer = new char[4096];
        var builder = new StringBuilder();
        var truncated = false;

        int read;
        while ((read = await reader.ReadAsync(buffer.AsMemory(), cancellationToken).ConfigureAwait(false)) > 0)
        {
            var remaining = MaxCapturedCharacters - builder.Length;
            if (remaining <= 0)
            {
                truncated = true;
                continue;
            }

            var take = Math.Min(read, remaining);
            builder.Append(buffer, 0, take);
            if (take < read)
            {
                truncated = true;
            }
        }

        if (truncated)
        {
            builder.Append(Environment.NewLine).Append("…（輸出過長，其餘內容已捨棄）");
        }

        return builder.ToString();
    }

    private static void TryKill(Process process)
    {
        try
        {
            if (!process.HasExited)
            {
                process.Kill(entireProcessTree: true);
            }
        }
        catch
        {
            // 程序可能已自行結束或無權限終止，兩者都不影響結果判定
        }
    }
}
