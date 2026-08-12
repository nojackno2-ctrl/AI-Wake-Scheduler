using System.Diagnostics;
using System.Text;
using System.Text.Json;

namespace AiWakeScheduler.Core;

public enum CliUsageAvailability
{
    Available,
    Unsupported,
    Unavailable
}

/// <summary>CLI 帳戶的一個額度視窗。</summary>
public sealed record CliUsageWindow(
    string Name,
    int UsedPercent,
    TimeSpan? Duration,
    DateTimeOffset? ResetsAt)
{
    public int RemainingPercent => Math.Clamp(100 - UsedPercent, 0, 100);
}

/// <summary>一次唯讀用量查詢的結果。</summary>
public sealed record CliUsageSnapshot(
    CliKind Cli,
    CliUsageAvailability Availability,
    IReadOnlyList<CliUsageWindow> Windows,
    string Message,
    DateTimeOffset ObservedAt);

/// <summary>
/// 讀取 CLI 帳戶額度。Codex 使用官方 app-server 的
/// account/rateLimits/read；其他 CLI 若沒有公開、可機器解析的介面，
/// 明確回傳 Unsupported，絕不以排程時間推測帳戶額度。
/// </summary>
public sealed class CliUsageReader
{
    private static readonly TimeSpan QueryTimeout = TimeSpan.FromSeconds(15);

    public Task<CliUsageSnapshot> ReadAsync(
        CliKind kind,
        CliProfile profile,
        string workingDirectory,
        CancellationToken cancellationToken = default)
    {
        ArgumentNullException.ThrowIfNull(profile);

        if (kind != CliKind.Codex)
        {
            return Task.FromResult(new CliUsageSnapshot(
                kind,
                CliUsageAvailability.Unsupported,
                [],
                "目前 CLI 未提供可機器解析的剩餘額度介面。",
                DateTimeOffset.Now));
        }

        return ReadCodexAsync(profile, workingDirectory, cancellationToken);
    }

    private static async Task<CliUsageSnapshot> ReadCodexAsync(
        CliProfile profile,
        string workingDirectory,
        CancellationToken cancellationToken)
    {
        var observedAt = DateTimeOffset.Now;
        Process? process = null;
        Task<string>? errorTask = null;

        try
        {
            if (!Directory.Exists(workingDirectory))
            {
                throw new DirectoryNotFoundException($"工作目錄不存在：{workingDirectory}");
            }

            var executable = ExecutableLocator.Resolve(CliKind.Codex, profile.Executable, workingDirectory);
            if (executable is null)
            {
                throw new FileNotFoundException("找不到 Codex CLI，請先在 CLI 設定指定正確路徑。");
            }

            var startInfo = new ProcessStartInfo
            {
                FileName = executable,
                WorkingDirectory = workingDirectory,
                UseShellExecute = false,
                CreateNoWindow = true,
                RedirectStandardInput = true,
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                // JSONL 第一個位元組不可有 UTF-8 BOM；否則 initialize 會被視為無效訊息，
                // 後續要求只會得到 Not initialized，嚴格握手則會一直等待 id=0。
                StandardInputEncoding = new UTF8Encoding(encoderShouldEmitUTF8Identifier: false),
                StandardOutputEncoding = Encoding.UTF8,
                StandardErrorEncoding = Encoding.UTF8
            };
            startInfo.ArgumentList.Add("app-server");
            startInfo.ArgumentList.Add("--listen");
            startInfo.ArgumentList.Add("stdio://");
            startInfo.Environment["NO_COLOR"] = "1";
            startInfo.Environment["TERM"] = "dumb";

            process = new Process { StartInfo = startInfo };
            if (!process.Start())
            {
                throw new InvalidOperationException("無法啟動 Codex app-server。");
            }

            errorTask = ReadBoundedAsync(process.StandardError, cancellationToken);
            using var timeoutSource = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
            timeoutSource.CancelAfter(QueryTimeout);
            var token = timeoutSource.Token;

            await process.StandardInput.WriteLineAsync(
                "{\"method\":\"initialize\",\"id\":0,\"params\":{\"clientInfo\":{\"name\":\"ai_wake_scheduler\",\"title\":\"AI Wake Scheduler\",\"version\":\"1.2.0\"}}}")
                .ConfigureAwait(false);
            await process.StandardInput.FlushAsync(token).ConfigureAwait(false);

            // app-server 明確要求先完成 initialize 握手。若三行一次送出，
            // 伺服器在負載下可能先處理額度要求並回覆 Not initialized。
            var initializeResponse = await ReadResponseAsync(process.StandardOutput, 0, token).ConfigureAwait(false);
            ThrowIfProtocolError(initializeResponse, "Codex app-server 初始化失敗");

            await process.StandardInput.WriteLineAsync("{\"method\":\"initialized\",\"params\":{}}")
                .ConfigureAwait(false);
            await process.StandardInput.WriteLineAsync("{\"method\":\"account/rateLimits/read\",\"id\":1}")
                .ConfigureAwait(false);
            await process.StandardInput.FlushAsync(token).ConfigureAwait(false);
            var rateLimitResponse = await ReadResponseAsync(process.StandardOutput, 1, token).ConfigureAwait(false);
            return ParseCodexRateLimits(rateLimitResponse, observedAt);
        }
        catch (OperationCanceledException) when (!cancellationToken.IsCancellationRequested)
        {
            return Unavailable(CliKind.Codex, "讀取 Codex 額度逾時。", observedAt);
        }
        catch (OperationCanceledException)
        {
            throw;
        }
        catch (Exception ex)
        {
            return Unavailable(CliKind.Codex, ex.Message, observedAt);
        }
        finally
        {
            TryKill(process);
            if (errorTask is not null)
            {
                try
                {
                    await errorTask.WaitAsync(TimeSpan.FromSeconds(2)).ConfigureAwait(false);
                }
                catch
                {
                }
            }
            process?.Dispose();
        }
    }

    /// <summary>解析 account/rateLimits/read 的單行 JSON 回應，供測試與協定更新驗證。</summary>
    public static CliUsageSnapshot ParseCodexRateLimits(string json, DateTimeOffset observedAt)
    {
        using var document = JsonDocument.Parse(json);
        var root = document.RootElement;

        if (root.TryGetProperty("error", out var error))
        {
            var message = error.TryGetProperty("message", out var errorMessage)
                ? errorMessage.GetString()
                : error.GetRawText();
            return Unavailable(CliKind.Codex, $"Codex 額度查詢失敗：{message}", observedAt);
        }

        if (!root.TryGetProperty("result", out var result))
        {
            return Unavailable(CliKind.Codex, "Codex 回應缺少 result。", observedAt);
        }

        var windows = new List<CliUsageWindow>();
        if (result.TryGetProperty("rateLimitsByLimitId", out var byId) && byId.ValueKind == JsonValueKind.Object)
        {
            foreach (var bucket in byId.EnumerateObject())
            {
                AddBucketWindows(windows, bucket.Value, bucket.Name);
            }
        }

        if (windows.Count == 0 && result.TryGetProperty("rateLimits", out var legacy) && legacy.ValueKind == JsonValueKind.Object)
        {
            AddBucketWindows(windows, legacy, "Codex");
        }

        return windows.Count == 0
            ? Unavailable(CliKind.Codex, "Codex 已回應，但目前沒有可顯示的額度視窗。", observedAt)
            : new CliUsageSnapshot(CliKind.Codex, CliUsageAvailability.Available, windows, "讀取成功", observedAt);
    }

    private static void AddBucketWindows(List<CliUsageWindow> destination, JsonElement bucket, string fallbackName)
    {
        var bucketName = GetString(bucket, "limitName") ?? GetString(bucket, "limitId") ?? fallbackName;
        AddWindow(destination, bucket, "primary", bucketName);
        AddWindow(destination, bucket, "secondary", $"{bucketName}（次要）");
    }

    private static void AddWindow(List<CliUsageWindow> destination, JsonElement bucket, string propertyName, string name)
    {
        if (!bucket.TryGetProperty(propertyName, out var window) || window.ValueKind != JsonValueKind.Object ||
            !window.TryGetProperty("usedPercent", out var usedElement) || !usedElement.TryGetInt32(out var usedPercent))
        {
            return;
        }

        TimeSpan? duration = null;
        if (window.TryGetProperty("windowDurationMins", out var durationElement) &&
            durationElement.ValueKind == JsonValueKind.Number && durationElement.TryGetInt64(out var minutes))
        {
            duration = TimeSpan.FromMinutes(minutes);
        }

        DateTimeOffset? resetsAt = null;
        if (window.TryGetProperty("resetsAt", out var resetElement) &&
            resetElement.ValueKind == JsonValueKind.Number && resetElement.TryGetInt64(out var unixSeconds))
        {
            try
            {
                resetsAt = DateTimeOffset.FromUnixTimeSeconds(unixSeconds);
            }
            catch (ArgumentOutOfRangeException)
            {
                // 協定若回傳無效時間，仍可顯示剩餘百分比。
            }
        }

        destination.Add(new CliUsageWindow(name, Math.Clamp(usedPercent, 0, 100), duration, resetsAt));
    }

    private static string? GetString(JsonElement element, string propertyName) =>
        element.TryGetProperty(propertyName, out var value) && value.ValueKind == JsonValueKind.String
            ? value.GetString()
            : null;

    private static bool TryGetResponseId(string json, out int id)
    {
        id = default;
        try
        {
            using var document = JsonDocument.Parse(json);
            return document.RootElement.TryGetProperty("id", out var value) && value.TryGetInt32(out id);
        }
        catch (JsonException)
        {
            return false;
        }
    }

    private static async Task<string> ReadResponseAsync(StreamReader reader, int responseId, CancellationToken cancellationToken)
    {
        while (true)
        {
            var line = await reader.ReadLineAsync(cancellationToken).ConfigureAwait(false);
            if (line is null)
            {
                throw new InvalidOperationException($"Codex app-server 在回傳 id={responseId} 前結束。");
            }
            if (TryGetResponseId(line, out var id) && id == responseId)
            {
                return line;
            }
        }
    }

    private static void ThrowIfProtocolError(string json, string prefix)
    {
        using var document = JsonDocument.Parse(json);
        if (!document.RootElement.TryGetProperty("error", out var error))
        {
            return;
        }

        var message = error.TryGetProperty("message", out var errorMessage)
            ? errorMessage.GetString()
            : error.GetRawText();
        throw new InvalidOperationException($"{prefix}：{message}");
    }

    private static CliUsageSnapshot Unavailable(CliKind kind, string message, DateTimeOffset observedAt) =>
        new(kind, CliUsageAvailability.Unavailable, [], message, observedAt);

    private static async Task<string> ReadBoundedAsync(StreamReader reader, CancellationToken cancellationToken)
    {
        var buffer = new char[2048];
        var builder = new StringBuilder();
        int read;
        while ((read = await reader.ReadAsync(buffer.AsMemory(), cancellationToken).ConfigureAwait(false)) > 0)
        {
            var remaining = ProcessRunner.MaxCapturedCharacters - builder.Length;
            if (remaining > 0)
            {
                builder.Append(buffer, 0, Math.Min(read, remaining));
            }
        }
        return builder.ToString();
    }

    private static void TryKill(Process? process)
    {
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
        }
    }
}
