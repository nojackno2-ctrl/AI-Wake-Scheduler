using System.Diagnostics;
using System.Text;
using System.Text.RegularExpressions;

namespace AiWakeScheduler.Core;

/// <summary>
/// 由本應用程式代管的一個 Antigravity language server。
///
/// Antigravity 沒開時額度就讀不到，但 <c>language_server.exe</c> 無法自行啟動：
/// 預設模式要求 IDE 從 stdin 餵入私有的 metadata（否則 <c>Failed to read initial
/// metadata from stdin</c> 直接結束），而 <c>-standalone=true</c> 會進入互動式
/// OAuth 流程等人貼網址。因此改用 agy CLI —— 它的 language server 是行程內啟動的，
/// 認證沿用使用者既有的登入狀態，且不需要任何互動。
/// </summary>
public sealed class AntigravityHost : IDisposable
{
    private Process? _process;

    internal AntigravityHost(int port, Process process)
    {
        Port = port;
        _process = process;
    }

    /// <summary>agy 行程內 language server 的 HTTPS 連接埠。</summary>
    public int Port { get; }

    /// <summary>代管的 agy 程序是否仍在執行。</summary>
    public bool IsAlive
    {
        get
        {
            try
            {
                return _process is { HasExited: false };
            }
            catch
            {
                return false;
            }
        }
    }

    public void Dispose()
    {
        var process = Interlocked.Exchange(ref _process, null);
        if (process is null)
        {
            return;
        }

        try
        {
            if (!process.HasExited)
            {
                process.Kill(entireProcessTree: true);
            }
        }
        catch
        {
            // 程序可能已自行結束，忽略。
        }
        finally
        {
            process.Dispose();
        }
    }
}

/// <summary>
/// 在沒有任何 Antigravity language server 可用時，於背景無視窗地啟動一個。
/// </summary>
public static class AntigravityLauncher
{
    /// <summary>
    /// 用來把 language server 拉起來的 agy 子命令。
    ///
    /// <c>models</c> 需要向後端查詢可用模型，因此一定會啟動行程內的 language server，
    /// 而它是子命令而不是提示詞，不會建立模型回合、不消耗任何 Token。
    /// </summary>
    private static readonly string[] StartupArguments = ["models"];

    /// <summary>agy 啟動到 language server 開始監聽大約需要 1.5 秒，這裡留足餘裕。</summary>
    private static readonly TimeSpan StartupTimeout = TimeSpan.FromSeconds(20);

    private static readonly TimeSpan PollInterval = TimeSpan.FromMilliseconds(25);

    private static readonly Regex ListeningPortPattern = new(
        @"listening on \w+ port at (\d+) for HTTPS",
        RegexOptions.IgnoreCase | RegexOptions.Compiled);

    /// <summary>
    /// 啟動一個隱藏的 agy 程序並等待它的 language server 開始監聽。
    ///
    /// 回傳 null 代表沒能在時限內取得連接埠；呼叫端可以直接重試。
    /// 取得的 <see cref="AntigravityHost"/> 必須釋放，否則 agy 程序會留著。
    /// </summary>
    public static async Task<AntigravityHost?> StartAsync(
        string agyExecutable,
        string workingDirectory,
        CancellationToken cancellationToken)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(agyExecutable);

        // 先記下現有的工作階段日誌，之後才分得出哪一個是這次啟動寫的。
        var logDirectory = GetSessionLogDirectory();
        var existingLogs = SnapshotSessionLogs(logDirectory);

        var startInfo = new ProcessStartInfo
        {
            FileName = agyExecutable,
            UseShellExecute = false,
            CreateNoWindow = true,
            WindowStyle = ProcessWindowStyle.Hidden,
            // 三個標準串流都必須重導，否則在沒有主控台的情況下 agy 會拿到無效的控制代碼。
            RedirectStandardInput = true,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            StandardOutputEncoding = Encoding.UTF8,
            StandardErrorEncoding = Encoding.UTF8
        };

        if (Directory.Exists(workingDirectory))
        {
            startInfo.WorkingDirectory = workingDirectory;
        }

        for (var i = 0; i < StartupArguments.Length; i++)
        {
            startInfo.ArgumentList.Add(StartupArguments[i]);
        }

        startInfo.Environment["NO_COLOR"] = "1";
        startInfo.Environment["TERM"] = "dumb";

        Process? process = null;
        try
        {
            process = new Process { StartInfo = startInfo };
            if (!process.Start())
            {
                process.Dispose();
                return null;
            }

            // 刻意不讀取 stdout：`agy models` 的輸出很小，不會塞滿管線，
            // 但也沒有必要浪費一條讀取任務。
            var port = await WaitForListeningPortAsync(
                logDirectory,
                existingLogs,
                process,
                cancellationToken).ConfigureAwait(false);

            if (port is null)
            {
                var orphan = process;
                process = null;
                using var host = new AntigravityHost(0, orphan);
                return null;
            }

            var started = process;
            process = null;
            return new AntigravityHost(port.Value, started);
        }
        catch (OperationCanceledException)
        {
            throw;
        }
        catch
        {
            return null;
        }
        finally
        {
            if (process is not null)
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
                }

                process.Dispose();
            }
        }
    }

    /// <summary>
    /// 輪詢 agy 的工作階段日誌，等待這次啟動寫出監聽連接埠。
    ///
    /// agy 只會活幾秒鐘，所以輪詢間隔必須夠短；程序提前結束就沒有再等的意義。
    /// </summary>
    private static async Task<int?> WaitForListeningPortAsync(
        string logDirectory,
        HashSet<string> existingLogs,
        Process process,
        CancellationToken cancellationToken)
    {
        var deadline = DateTimeOffset.UtcNow + StartupTimeout;
        var processExited = false;

        while (DateTimeOffset.UtcNow < deadline)
        {
            foreach (var logPath in EnumerateSessionLogs(logDirectory))
            {
                if (existingLogs.Contains(logPath))
                {
                    continue;
                }

                foreach (var line in CliUsageReader.ReadTailLines(logPath, 200))
                {
                    var match = ListeningPortPattern.Match(line);
                    if (match.Success && int.TryParse(match.Groups[1].Value, out var port) && port > 0)
                    {
                        return port;
                    }
                }
            }

            // 程序結束後再掃一輪，日誌可能在結束的同時才刷入磁碟。
            if (processExited)
            {
                return null;
            }

            try
            {
                processExited = process.HasExited;
            }
            catch
            {
                processExited = true;
            }

            await Task.Delay(PollInterval, cancellationToken).ConfigureAwait(false);
        }

        return null;
    }

    private static string GetSessionLogDirectory() => Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.UserProfile),
        ".gemini",
        "antigravity-cli",
        "log");

    private static HashSet<string> SnapshotSessionLogs(string logDirectory) =>
        new(EnumerateSessionLogs(logDirectory), StringComparer.OrdinalIgnoreCase);

    private static IEnumerable<string> EnumerateSessionLogs(string logDirectory)
    {
        try
        {
            return Directory.Exists(logDirectory)
                ? Directory.EnumerateFiles(logDirectory, "cli-*.log", SearchOption.TopDirectoryOnly).ToArray()
                : [];
        }
        catch
        {
            return [];
        }
    }
}
