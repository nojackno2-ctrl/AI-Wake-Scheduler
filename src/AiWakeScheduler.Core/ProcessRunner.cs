using System.Buffers;
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

    /// <summary>
    /// 組出 <c>cmd.exe /d /s /c "..."</c> 的參數字串。
    ///
    /// <c>/d</c> 跳過 AutoRun 登錄項，<c>/s</c> 讓 cmd 只剝掉最外層的一對引號、
    /// 中間的內容原封不動交給批次檔，這樣每個參數只要各自照 argv 規則加引號即可。
    /// 供測試驗證引號規則。
    /// </summary>
    public static string BuildCommandShellArguments(string executable, IReadOnlyList<string> arguments)
    {
        var builder = new StringBuilder("/d /s /c \"");
        builder.Append(QuoteArgument(executable));
        for (var i = 0; i < arguments.Count; i++)
        {
            builder.Append(' ').Append(QuoteArgument(arguments[i]));
        }
        return builder.Append('"').ToString();
    }

    /// <summary>
    /// cmd.exe 會當成運算子的字元。
    ///
    /// argv 的引號規則只在意空白與雙引號，但這串參數要先經過 cmd 的剖析，
    /// 沒有空白的 <c>a&amp;b</c> 一樣會被切成兩個命令，所以必須一併納入判斷。
    /// 注意 <c>%</c> 的展開發生在引號處理之前，加引號救不了，這是 cmd 的固有限制。
    /// </summary>
    private const string CommandShellMetaCharacters = "&|<>^()!,;=";

    /// <summary>
    /// 依 Windows argv 規則替單一參數加引號。
    /// 反斜線只有在緊接著引號時才需要加倍，其餘情況維持原樣。
    /// </summary>
    private static string QuoteArgument(string value)
    {
        if (value.Length == 0)
        {
            return "\"\"";
        }

        var needsQuotes = false;
        for (var i = 0; i < value.Length; i++)
        {
            var character = value[i];
            if (char.IsWhiteSpace(character) ||
                character == '"' ||
                CommandShellMetaCharacters.Contains(character, StringComparison.Ordinal))
            {
                needsQuotes = true;
                break;
            }
        }

        if (!needsQuotes)
        {
            return value;
        }

        var builder = new StringBuilder(value.Length + 8).Append('"');
        var backslashes = 0;
        foreach (var character in value)
        {
            if (character == '\\')
            {
                backslashes++;
                continue;
            }

            if (character == '"')
            {
                builder.Append('\\', backslashes * 2 + 1).Append('"');
                backslashes = 0;
                continue;
            }

            builder.Append('\\', backslashes);
            backslashes = 0;
            builder.Append(character);
        }

        // 結尾的反斜線會和收尾引號黏在一起，必須加倍才不會把引號跳脫掉。
        return builder.Append('\\', backslashes * 2).Append('"').ToString();
    }

    public static async Task<ProcessExecution> ExecuteAsync(
        string executable,
        IReadOnlyList<string> arguments,
        string workingDirectory,
        TimeSpan timeout,
        CancellationToken cancellationToken,
        Action<bool, string>? onOutput = null)
    {
        // .cmd / .bat 不能直接當成可執行檔啟動，必須經由 cmd.exe。
        // 交給 ShellExecute 會跳出主控台視窗而且完全擷取不到輸出，
        // 因此改為自己呼叫 `cmd.exe /d /s /c`：一樣能執行批次檔，
        // 但可以維持 CreateNoWindow 與標準串流重導。
        var shellScript = OperatingSystem.IsWindows() &&
                          (executable.EndsWith(".cmd", StringComparison.OrdinalIgnoreCase) ||
                           executable.EndsWith(".bat", StringComparison.OrdinalIgnoreCase));

        var startInfo = new ProcessStartInfo
        {
            WorkingDirectory = workingDirectory,
            UseShellExecute = false,
            CreateNoWindow = true,
            WindowStyle = ProcessWindowStyle.Hidden,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            StandardOutputEncoding = Encoding.UTF8,
            StandardErrorEncoding = Encoding.UTF8
        };

        if (shellScript)
        {
            startInfo.FileName = Environment.GetEnvironmentVariable("ComSpec") ?? "cmd.exe";
            // cmd.exe 的引號規則和 argv 不同，ArgumentList 的跳脫方式會被它誤解，
            // 只能自己組整串命令列。/s 讓 cmd 只剝掉最外層引號，其餘原樣傳給批次檔。
            startInfo.Arguments = BuildCommandShellArguments(executable, arguments);
        }
        else
        {
            startInfo.FileName = executable;
            for (var i = 0; i < arguments.Count; i++)
            {
                startInfo.ArgumentList.Add(arguments[i]);
            }
        }

        // 沒有 ANSI 色碼，擷取到的輸出更小也更好讀。
        startInfo.Environment["NO_COLOR"] = "1";
        startInfo.Environment["TERM"] = "dumb";

        using var process = new Process { StartInfo = startInfo };
        if (!process.Start())
        {
            throw new InvalidOperationException("無法啟動 CLI 程序。");
        }

        using var timeoutSource = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        timeoutSource.CancelAfter(timeout);

        // 必須持續讀到 EOF，否則子程序寫滿管線時會卡住；
        // 超過上限的內容會被丟棄，不會累積在記憶體裡。
        var outputTask = ReadBoundedAsync(
            process.StandardOutput,
            timeoutSource.Token,
            chunk => onOutput?.Invoke(false, chunk));
        var errorTask = ReadBoundedAsync(
            process.StandardError,
            timeoutSource.Token,
            chunk => onOutput?.Invoke(true, chunk));
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

    private static async Task<string> ReadBoundedAsync(StreamReader reader, CancellationToken cancellationToken, Action<string>? onChunk = null)
    {
        var buffer = ArrayPool<char>.Shared.Rent(4096);
        try
        {
            var builder = new StringBuilder();
            var truncated = false;

            int read;
            while ((read = await reader.ReadAsync(buffer.AsMemory(0, 4096), cancellationToken).ConfigureAwait(false)) > 0)
            {
                // 即時通知用完整片段，不受下方的容量上限影響：
                // 容量上限只是為了讓存到磁碟的日誌維持可讀大小，畫面上仍應如實顯示。
                onChunk?.Invoke(new string(buffer, 0, read));

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
        finally
        {
            ArrayPool<char>.Shared.Return(buffer);
        }
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
