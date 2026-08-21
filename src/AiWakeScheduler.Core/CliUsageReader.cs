using System.Diagnostics;
using System.Net;
using System.Net.Http.Headers;
using System.Runtime.InteropServices;
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
    DateTimeOffset? ResetsAt,
    bool IsActiveCountdown = false)
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
/// account/rateLimits/read；Claude 使用 Claude Code 本身查詢 /api/oauth/usage
/// 時採用的本機 OAuth 憑證；絕不以排程時間推測帳戶額度。
/// </summary>
public sealed class CliUsageReader : ICliUsageReader
{
    private static readonly TimeSpan QueryTimeout = TimeSpan.FromSeconds(15);
    private const string ClaudeUsageEndpoint = "https://api.anthropic.com/api/oauth/usage";
    private const string ClaudeOAuthBeta = "oauth-2025-04-20";

    /// <summary>
    /// 判斷指定 CLI 的用量快照是否正處於有效的額度重置倒數中。
    /// 若額度未處於倒數狀態、倒數時間已過或無有效倒數時間戳記，傳回 false。
    /// </summary>
    public static bool IsCountingDown(CliUsageSnapshot? snapshot, DateTimeOffset now)
    {
        if (snapshot is null || snapshot.Availability != CliUsageAvailability.Available || snapshot.Windows.Count == 0)
        {
            return false;
        }

        // 對於 Claude，優先檢查 5 小時短週期額度視窗；其餘 CLI 取主要視窗
        var targetWindow = snapshot.Windows.FirstOrDefault(w => w.Duration is { } d && d <= TimeSpan.FromHours(6))
            ?? snapshot.Windows[0];

        if (targetWindow.IsActiveCountdown && targetWindow.ResetsAt is { } resetsAt)
        {
            return resetsAt > now;
        }

        return false;
    }

    public Task<CliUsageSnapshot> ReadAsync(
        CliKind kind,
        CliProfile profile,
        string workingDirectory,
        CancellationToken cancellationToken = default)
    {
        ArgumentNullException.ThrowIfNull(profile);

        return kind switch
        {
            CliKind.Codex => ReadCodexAsync(profile, workingDirectory, cancellationToken),
            CliKind.Antigravity or CliKind.AntigravityClaude => ReadAntigravityAsync(kind, cancellationToken),
            CliKind.Claude => ReadClaudeAsync(cancellationToken),
            _ => Task.FromResult(new CliUsageSnapshot(
                kind,
                CliUsageAvailability.Unsupported,
                [],
                "目前 CLI 未提供可機器解析的剩餘額度介面。",
                DateTimeOffset.Now))
        };
    }

    private static readonly SemaphoreSlim _agyQueryLock = new(1, 1);
    private static (string Json, DateTimeOffset CachedAt)? _cachedAgyResponse;
    private static readonly TimeSpan AgyCacheDuration = TimeSpan.FromSeconds(3);

    private static async Task<CliUsageSnapshot> ReadAntigravityAsync(
        CliKind kind,
        CancellationToken cancellationToken)
    {
        var observedAt = DateTimeOffset.Now;

        try
        {
            var lsProcesses = Process.GetProcessesByName("language_server");
            if (lsProcesses.Length == 0)
            {
                return Unavailable(kind, "Antigravity 尚未啟動，請先開啟 Antigravity 以讀取即時額度。", observedAt);
            }

            string responseJson;

            await _agyQueryLock.WaitAsync(cancellationToken).ConfigureAwait(false);
            try
            {
                if (_cachedAgyResponse.HasValue &&
                    DateTimeOffset.Now - _cachedAgyResponse.Value.CachedAt < AgyCacheDuration)
                {
                    responseJson = _cachedAgyResponse.Value.Json;
                }
                else
                {
                    var targetProc = lsProcesses[0];
                    var pid = targetProc.Id;

                    // 1. 解析 CSRF Token
                    var csrfToken = await TryGetCsrfTokenAsync(pid, cancellationToken).ConfigureAwait(false);
                    if (string.IsNullOrWhiteSpace(csrfToken))
                    {
                        return Unavailable(kind, "無法取得 Antigravity 認證權杖（CSRF Token）。", observedAt);
                    }

                    // 2. 尋找 HTTPS 監聽連接埠
                    var ports = GetAntigravityCandidatePorts(pid);
                    if (ports.Count == 0)
                    {
                        return Unavailable(kind, "無法偵測到 Antigravity Language Server 監聽連接埠。", observedAt);
                    }

                    // 3. 透過 HttpClient 呼叫 RPC（進行連續採樣驗證以確認倒數是否真實存在）
                    using var handler = new HttpClientHandler
                    {
                        ServerCertificateCustomValidationCallback = (_, _, _, _) => true
                    };
                    using var client = new HttpClient(handler)
                    {
                        Timeout = TimeSpan.FromSeconds(5)
                    };

                    var sample1 = await QueryAntigravityRpcAsync(client, ports, csrfToken, cancellationToken).ConfigureAwait(false);
                    var snapshot1 = ParseAntigravityModelConfigs(sample1, kind, observedAt);

                    // 若第一次採樣判定處於倒數中（UsedPercent > 0 且 resetsAt > now），間隔 5 秒進行第二次採樣比對是否為固定時間戳記
                    if (snapshot1.Availability == CliUsageAvailability.Available &&
                        snapshot1.Windows.Count > 0 &&
                        snapshot1.Windows[0].IsActiveCountdown &&
                        snapshot1.Windows[0].ResetsAt is { } reset1)
                    {
                        try
                        {
                            await Task.Delay(TimeSpan.FromSeconds(5), cancellationToken).ConfigureAwait(false);
                            var sample2 = await QueryAntigravityRpcAsync(client, ports, csrfToken, cancellationToken).ConfigureAwait(false);
                            var snapshot2 = ParseAntigravityModelConfigs(sample2, kind, DateTimeOffset.Now);
                            if (snapshot2.Availability == CliUsageAvailability.Available && snapshot2.Windows.Count > 0)
                            {
                                var w2 = snapshot2.Windows[0];
                                var isFixed = w2.ResetsAt is { } reset2 &&
                                    Math.Abs((reset2 - reset1).TotalSeconds) < 2.0;

                                var finalWindows = new List<CliUsageWindow>
                                {
                                    new(w2.Name, w2.UsedPercent, w2.Duration, isFixed ? w2.ResetsAt : null, isFixed && w2.UsedPercent > 0 && w2.ResetsAt > DateTimeOffset.Now)
                                };
                                responseJson = sample2;
                                _cachedAgyResponse = (responseJson, DateTimeOffset.Now);
                                return new CliUsageSnapshot(kind, CliUsageAvailability.Available, finalWindows, "讀取成功", DateTimeOffset.Now);
                            }
                        }
                        catch
                        {
                        }
                    }

                    responseJson = sample1;
                    _cachedAgyResponse = (responseJson, DateTimeOffset.Now);
                }
            }
            finally
            {
                _agyQueryLock.Release();
            }

            return ParseAntigravityModelConfigs(responseJson, kind, observedAt);
        }
        catch (OperationCanceledException) when (!cancellationToken.IsCancellationRequested)
        {
            return Unavailable(kind, "讀取 Antigravity 額度逾時。", observedAt);
        }
        catch (OperationCanceledException)
        {
            throw;
        }
        catch (Exception ex)
        {
            return Unavailable(kind, $"Antigravity 額度查詢失敗：{ex.Message}", observedAt);
        }
    }

    private static async Task<string> QueryAntigravityRpcAsync(
        HttpClient client,
        IReadOnlyList<int> ports,
        string csrfToken,
        CancellationToken cancellationToken)
    {
        using var requestMessage = new HttpRequestMessage(
            HttpMethod.Post,
            $"https://127.0.0.1:{ports[0]}/exa.language_server_pb.LanguageServerService/GetCascadeModelConfigData")
        {
            Content = new StringContent("{}", Encoding.UTF8, "application/json")
        };
        requestMessage.Headers.Add("x-codeium-csrf-token", csrfToken);

        try
        {
            using var response = await client.SendAsync(requestMessage, cancellationToken).ConfigureAwait(false);
            return await response.Content.ReadAsStringAsync(cancellationToken).ConfigureAwait(false);
        }
        catch (Exception) when (ports.Count > 1)
        {
            using var retryMessage = new HttpRequestMessage(
                HttpMethod.Post,
                $"https://127.0.0.1:{ports[1]}/exa.language_server_pb.LanguageServerService/GetCascadeModelConfigData")
            {
                Content = new StringContent("{}", Encoding.UTF8, "application/json")
            };
            retryMessage.Headers.Add("x-codeium-csrf-token", csrfToken);
            using var response = await client.SendAsync(retryMessage, cancellationToken).ConfigureAwait(false);
            return await response.Content.ReadAsStringAsync(cancellationToken).ConfigureAwait(false);
        }
    }

    private static async Task<string?> TryGetCsrfTokenAsync(int pid, CancellationToken cancellationToken)
    {
        // 1. 優先使用原生 Win32 PEB 讀取（微秒級完成、零子程序開銷）
        if (OperatingSystem.IsWindows())
        {
            try
            {
                var nativeCmd = NativeProcessHelper.GetCommandLine(pid);
                if (!string.IsNullOrWhiteSpace(nativeCmd))
                {
                    var match = System.Text.RegularExpressions.Regex.Match(nativeCmd, @"--csrf_token\s+([^\s]+)");
                    if (match.Success)
                    {
                        return match.Groups[1].Value;
                    }
                }
            }
            catch
            {
            }
        }

        // 2. 備用方案 A：透過 PowerShell 查詢 CIM 物件（精確語法）
        try
        {
            var startInfo = new ProcessStartInfo
            {
                FileName = "powershell.exe",
                Arguments = $"-NoProfile -ExecutionPolicy Bypass -Command \"(Get-CimInstance Win32_Process -Filter 'ProcessId = {pid}').CommandLine\"",
                UseShellExecute = false,
                CreateNoWindow = true,
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                StandardOutputEncoding = Encoding.UTF8
            };

            using var process = new Process { StartInfo = startInfo };
            if (process.Start())
            {
                using var cts = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
                cts.CancelAfter(TimeSpan.FromSeconds(5));

                var output = await process.StandardOutput.ReadToEndAsync(cts.Token).ConfigureAwait(false);
                await process.WaitForExitAsync(cts.Token).ConfigureAwait(false);

                if (!string.IsNullOrWhiteSpace(output))
                {
                    var match = System.Text.RegularExpressions.Regex.Match(output, @"--csrf_token\s+([^\s]+)");
                    if (match.Success)
                    {
                        return match.Groups[1].Value;
                    }
                }
            }
        }
        catch
        {
        }

        // 3. 備用方案 B：透過 PowerShell Get-Process 篩選
        try
        {
            var startInfo = new ProcessStartInfo
            {
                FileName = "powershell.exe",
                Arguments = $"-NoProfile -ExecutionPolicy Bypass -Command \"(Get-Process -Id {pid} -ErrorAction SilentlyContinue).CommandLine\"",
                UseShellExecute = false,
                CreateNoWindow = true,
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                StandardOutputEncoding = Encoding.UTF8
            };

            using var process = new Process { StartInfo = startInfo };
            if (process.Start())
            {
                using var cts = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
                cts.CancelAfter(TimeSpan.FromSeconds(5));

                var output = await process.StandardOutput.ReadToEndAsync(cts.Token).ConfigureAwait(false);
                await process.WaitForExitAsync(cts.Token).ConfigureAwait(false);

                if (!string.IsNullOrWhiteSpace(output))
                {
                    var match = System.Text.RegularExpressions.Regex.Match(output, @"--csrf_token\s+([^\s]+)");
                    if (match.Success)
                    {
                        return match.Groups[1].Value;
                    }
                }
            }
        }
        catch
        {
        }

        return null;
    }

    private static List<int> GetAntigravityCandidatePorts(int pid)
    {
        var ports = new List<int>();

        // 1. 從日誌中解析 HTTPS 連接埠（掃描前 200 行與後 200 行）
        var appData = Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData);
        var possibleLogPaths = new[]
        {
            Path.Combine(appData, "Antigravity", "logs", "language_server.log"),
            Path.Combine(appData, "Antigravity IDE", "logs", "language_server.log")
        };

        foreach (var logPath in possibleLogPaths)
        {
            if (!File.Exists(logPath)) continue;

            try
            {
                using var fs = new FileStream(logPath, FileMode.Open, FileAccess.Read, FileShare.ReadWrite);
                using var reader = new StreamReader(fs, Encoding.UTF8);

                string? line;
                var lineCount = 0;
                while ((line = reader.ReadLine()) is not null && lineCount < 200)
                {
                    lineCount++;
                    var m = System.Text.RegularExpressions.Regex.Match(line, @"listening on \w+ port at (\d+) for HTTPS", System.Text.RegularExpressions.RegexOptions.IgnoreCase);
                    if (m.Success && int.TryParse(m.Groups[1].Value, out var port))
                    {
                        if (!ports.Contains(port)) ports.Add(port);
                    }
                }
            }
            catch
            {
            }
        }

        // 2. 透過 PowerShell 取得該進程目前處於 Listen 狀態的 LocalPort
        if (ports.Count == 0 && pid > 0 && OperatingSystem.IsWindows())
        {
            try
            {
                var startInfo = new ProcessStartInfo
                {
                    FileName = "powershell.exe",
                    UseShellExecute = false,
                    CreateNoWindow = true,
                    RedirectStandardOutput = true,
                    RedirectStandardError = true,
                    StandardOutputEncoding = Encoding.UTF8
                };
                startInfo.ArgumentList.Add("-NoProfile");
                startInfo.ArgumentList.Add("-Command");
                startInfo.ArgumentList.Add($"(Get-NetTCPConnection -OwningProcess {pid} -State Listen -ErrorAction SilentlyContinue).LocalPort");

                using var process = new Process { StartInfo = startInfo };
                if (process.Start())
                {
                    var output = process.StandardOutput.ReadToEnd();
                    process.WaitForExit(3000);
                    foreach (var line in output.Split(['\r', '\n'], StringSplitOptions.RemoveEmptyEntries))
                    {
                        if (int.TryParse(line.Trim(), out var p) && !ports.Contains(p))
                        {
                            ports.Add(p);
                        }
                    }
                }
            }
            catch
            {
            }
        }

        return ports;
    }

    /// <summary>解析 Antigravity GetCascadeModelConfigData 的 JSON 回應。</summary>
    public static CliUsageSnapshot ParseAntigravityModelConfigs(string json, CliKind kind, DateTimeOffset observedAt)
    {
        try
        {
            using var document = JsonDocument.Parse(json);
            var root = document.RootElement;

            if (root.TryGetProperty("error", out var error))
            {
                var message = error.TryGetProperty("message", out var errorMessage)
                    ? errorMessage.GetString()
                    : error.GetRawText();
                return Unavailable(kind, $"Antigravity 額度查詢失敗：{message}", observedAt);
            }

            if (!root.TryGetProperty("clientModelConfigs", out var configs) || configs.ValueKind != JsonValueKind.Array)
            {
                return Unavailable(kind, "Antigravity 回應缺少 clientModelConfigs。", observedAt);
            }

            var isGeminiPool = kind == CliKind.Antigravity;
            var windowName = isGeminiPool ? "Antigravity (Gemini)" : "Antigravity (Claude / GPT)";

            foreach (var config in configs.EnumerateArray())
            {
                var label = config.TryGetProperty("label", out var labelProp) ? labelProp.GetString() ?? "" : "";
                var matchPool = isGeminiPool
                    ? label.Contains("Gemini", StringComparison.OrdinalIgnoreCase)
                    : (label.Contains("Claude", StringComparison.OrdinalIgnoreCase) || label.Contains("GPT", StringComparison.OrdinalIgnoreCase));

                if (!matchPool) continue;

                if (config.TryGetProperty("quotaInfo", out var quotaInfo) && quotaInfo.ValueKind == JsonValueKind.Object)
                {
                    var remainingFraction = 1.0;
                    if (quotaInfo.TryGetProperty("remainingFraction", out var remProp) && remProp.TryGetDouble(out var remVal))
                    {
                        remainingFraction = Math.Clamp(remVal, 0.0, 1.0);
                    }

                    var usedPercent = (int)Math.Round((1.0 - remainingFraction) * 100.0);

                    DateTimeOffset? resetsAt = null;
                    if (quotaInfo.TryGetProperty("resetTime", out var resetProp) && resetProp.ValueKind == JsonValueKind.String)
                    {
                        if (DateTimeOffset.TryParse(resetProp.GetString(), out var parsedReset))
                        {
                            resetsAt = parsedReset;
                        }
                    }

                    var clampedUsed = Math.Clamp(usedPercent, 0, 100);
                    var isActiveCountdown = clampedUsed > 0 && resetsAt.HasValue && resetsAt.Value > observedAt;
                    var window = new CliUsageWindow(windowName, clampedUsed, null, resetsAt, isActiveCountdown);
                    return new CliUsageSnapshot(kind, CliUsageAvailability.Available, [window], "讀取成功", observedAt);
                }
            }

            return Unavailable(kind, $"Antigravity 已回應，但未找到對應的 {windowName} 配額資料。", observedAt);
        }
        catch (JsonException ex)
        {
            return Unavailable(kind, $"Antigravity 回應 JSON 解析失敗：{ex.Message}", observedAt);
        }
    }

    private static async Task<CliUsageSnapshot> ReadClaudeAsync(CancellationToken cancellationToken)
    {
        var observedAt = DateTimeOffset.Now;

        try
        {
            var credentialsPath = GetClaudeCredentialsPath();
            if (credentialsPath is null || !File.Exists(credentialsPath))
            {
                return Unavailable(
                    CliKind.Claude,
                    "找不到 Claude Code 登入憑證，請先執行 claude auth login。",
                    observedAt);
            }

            var accessToken = await ReadClaudeAccessTokenAsync(credentialsPath, cancellationToken).ConfigureAwait(false);
            if (string.IsNullOrWhiteSpace(accessToken))
            {
                return Unavailable(
                    CliKind.Claude,
                    "Claude Code 尚未以 Claude 訂閱帳號登入，請先執行 claude auth login。",
                    observedAt);
            }

            using var client = new HttpClient { Timeout = QueryTimeout };
            for (var attempt = 0; attempt < 2; attempt++)
            {
                using var request = new HttpRequestMessage(HttpMethod.Get, ClaudeUsageEndpoint);
                request.Headers.Authorization = new AuthenticationHeaderValue("Bearer", accessToken);
                request.Headers.Accept.Add(new MediaTypeWithQualityHeaderValue("application/json"));
                request.Headers.TryAddWithoutValidation("anthropic-beta", ClaudeOAuthBeta);
                request.Headers.TryAddWithoutValidation("anthropic-version", "2023-06-01");
                request.Headers.UserAgent.ParseAdd("ai-wake-scheduler/1.4.0");

                using var response = await client.SendAsync(request, cancellationToken).ConfigureAwait(false);
                var responseJson = await response.Content.ReadAsStringAsync(cancellationToken).ConfigureAwait(false);
                if (response.IsSuccessStatusCode)
                {
                    return ParseClaudeUsage(responseJson, observedAt);
                }

                // Claude Code 可能剛好在另一個程序輪替短效 access token；重新讀檔後重試一次。
                if (response.StatusCode == HttpStatusCode.Unauthorized && attempt == 0)
                {
                    var refreshedToken = await ReadClaudeAccessTokenAsync(credentialsPath, cancellationToken).ConfigureAwait(false);
                    if (!string.IsNullOrWhiteSpace(refreshedToken) &&
                        !string.Equals(refreshedToken, accessToken, StringComparison.Ordinal))
                    {
                        accessToken = refreshedToken;
                        continue;
                    }

                    return Unavailable(
                        CliKind.Claude,
                        "Claude Code 登入憑證已過期；請先開啟 Claude Code 或重新執行 claude auth login。",
                        observedAt);
                }

                return Unavailable(
                    CliKind.Claude,
                    $"Claude 額度查詢失敗（HTTP {(int)response.StatusCode}）。",
                    observedAt);
            }

            return Unavailable(CliKind.Claude, "Claude 額度查詢失敗。", observedAt);
        }
        catch (OperationCanceledException) when (!cancellationToken.IsCancellationRequested)
        {
            return Unavailable(CliKind.Claude, "讀取 Claude 額度逾時。", observedAt);
        }
        catch (OperationCanceledException)
        {
            throw;
        }
        catch (JsonException ex)
        {
            return Unavailable(CliKind.Claude, $"Claude 額度 JSON 解析失敗：{ex.Message}", observedAt);
        }
        catch (HttpRequestException ex)
        {
            return Unavailable(CliKind.Claude, $"無法連線至 Claude 額度服務：{ex.Message}", observedAt);
        }
        catch (Exception ex)
        {
            return Unavailable(CliKind.Claude, ex.Message, observedAt);
        }
    }

    private static string? GetClaudeCredentialsPath()
    {
        var configuredDirectory = Environment.GetEnvironmentVariable("CLAUDE_CONFIG_DIR");
        if (!string.IsNullOrWhiteSpace(configuredDirectory))
        {
            return Path.Combine(Environment.ExpandEnvironmentVariables(configuredDirectory), ".credentials.json");
        }

        var userProfile = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
        return string.IsNullOrWhiteSpace(userProfile)
            ? null
            : Path.Combine(userProfile, ".claude", ".credentials.json");
    }

    private static async Task<string?> ReadClaudeAccessTokenAsync(
        string credentialsPath,
        CancellationToken cancellationToken)
    {
        await using var stream = new FileStream(
            credentialsPath,
            FileMode.Open,
            FileAccess.Read,
            FileShare.ReadWrite | FileShare.Delete,
            bufferSize: 4096,
            useAsync: true);
        using var document = await JsonDocument.ParseAsync(stream, cancellationToken: cancellationToken).ConfigureAwait(false);
        var root = document.RootElement;
        if (!root.TryGetProperty("claudeAiOauth", out var oauth) || oauth.ValueKind != JsonValueKind.Object ||
            !oauth.TryGetProperty("accessToken", out var token) || token.ValueKind != JsonValueKind.String)
        {
            return null;
        }

        return token.GetString();
    }

    /// <summary>解析 Claude Code /api/oauth/usage 的 JSON 回應，供測試與協定更新驗證。</summary>
    public static CliUsageSnapshot ParseClaudeUsage(string json, DateTimeOffset observedAt)
    {
        using var document = JsonDocument.Parse(json);
        var root = document.RootElement;

        if (root.TryGetProperty("error", out var error))
        {
            var message = error.ValueKind == JsonValueKind.Object &&
                          error.TryGetProperty("message", out var errorMessage)
                ? errorMessage.GetString()
                : error.GetRawText();
            return Unavailable(CliKind.Claude, $"Claude 額度查詢失敗：{message}", observedAt);
        }

        var windows = new List<CliUsageWindow>();
        AddClaudeWindow(windows, root, "five_hour", "Claude（5 小時）", TimeSpan.FromHours(5), observedAt);
        AddClaudeWindow(windows, root, "seven_day", "Claude（7 天）", TimeSpan.FromDays(7), observedAt);
        AddClaudeWindow(windows, root, "seven_day_opus", "Claude Opus（7 天）", TimeSpan.FromDays(7), observedAt);
        AddClaudeWindow(windows, root, "seven_day_sonnet", "Claude Sonnet（7 天）", TimeSpan.FromDays(7), observedAt);
        AddClaudeWindow(windows, root, "seven_day_oauth_apps", "Claude OAuth Apps（7 天）", TimeSpan.FromDays(7), observedAt);
        AddClaudeWindow(windows, root, "seven_day_cowork", "Claude Cowork（7 天）", TimeSpan.FromDays(7), observedAt);

        return windows.Count == 0
            ? Unavailable(CliKind.Claude, "Claude 已回應，但目前沒有可顯示的額度視窗。", observedAt)
            : new CliUsageSnapshot(CliKind.Claude, CliUsageAvailability.Available, windows, "讀取成功", observedAt);
    }

    private static void AddClaudeWindow(
        List<CliUsageWindow> destination,
        JsonElement root,
        string propertyName,
        string displayName,
        TimeSpan duration,
        DateTimeOffset observedAt)
    {
        if (!root.TryGetProperty(propertyName, out var window) || window.ValueKind != JsonValueKind.Object ||
            !window.TryGetProperty("utilization", out var utilizationElement) ||
            !utilizationElement.TryGetDouble(out var utilization))
        {
            return;
        }

        DateTimeOffset? resetsAt = null;
        if (window.TryGetProperty("resets_at", out var resetElement) &&
            resetElement.ValueKind == JsonValueKind.String &&
            DateTimeOffset.TryParse(resetElement.GetString(), out var parsedReset))
        {
            resetsAt = parsedReset;
        }

        var usedPercent = (int)Math.Round(utilization, MidpointRounding.AwayFromZero);
        var clampedUsed = Math.Clamp(usedPercent, 0, 100);
        var isActiveCountdown = clampedUsed > 0 && resetsAt.HasValue && resetsAt.Value > observedAt;
        destination.Add(new CliUsageWindow(displayName, clampedUsed, duration, resetsAt, isActiveCountdown));
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
                "{\"method\":\"initialize\",\"id\":0,\"params\":{\"clientInfo\":{\"name\":\"ai_wake_scheduler\",\"title\":\"AI Wake Scheduler\",\"version\":\"1.4.0\"}}}")
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
                AddBucketWindows(windows, bucket.Value, bucket.Name, observedAt);
            }
        }

        if (windows.Count == 0 && result.TryGetProperty("rateLimits", out var legacy) && legacy.ValueKind == JsonValueKind.Object)
        {
            AddBucketWindows(windows, legacy, "Codex", observedAt);
        }

        return windows.Count == 0
            ? Unavailable(CliKind.Codex, "Codex 已回應，但目前沒有可顯示的額度視窗。", observedAt)
            : new CliUsageSnapshot(CliKind.Codex, CliUsageAvailability.Available, windows, "讀取成功", observedAt);
    }

    private static void AddBucketWindows(List<CliUsageWindow> destination, JsonElement bucket, string fallbackName, DateTimeOffset observedAt)
    {
        var bucketName = GetString(bucket, "limitName") ?? GetString(bucket, "limitId") ?? fallbackName;
        AddWindow(destination, bucket, "primary", bucketName, observedAt);
        AddWindow(destination, bucket, "secondary", $"{bucketName}（次要）", observedAt);
    }

    private static void AddWindow(List<CliUsageWindow> destination, JsonElement bucket, string propertyName, string name, DateTimeOffset observedAt)
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

        var clampedUsed = Math.Clamp(usedPercent, 0, 100);
        var isActiveCountdown = clampedUsed > 0 && resetsAt.HasValue && resetsAt.Value > observedAt;
        destination.Add(new CliUsageWindow(name, clampedUsed, duration, resetsAt, isActiveCountdown));
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

internal static class NativeProcessHelper
{
    [DllImport("ntdll.dll")]
    private static extern int NtQueryInformationProcess(
        IntPtr processHandle,
        int processInformationClass,
        ref PROCESS_BASIC_INFORMATION processInformation,
        int processInformationLength,
        out int returnLength);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern IntPtr OpenProcess(int processAccess, bool bInheritHandle, int processId);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool ReadProcessMemory(
        IntPtr hProcess,
        IntPtr lpBaseAddress,
        [Out] byte[] lpBuffer,
        int dwSize,
        out IntPtr lpNumberOfBytesRead);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool CloseHandle(IntPtr hObject);

    [DllImport("advapi32.dll", SetLastError = true)]
    private static extern bool OpenProcessToken(IntPtr processHandle, int desiredAccess, out IntPtr tokenHandle);

    [DllImport("advapi32.dll", SetLastError = true, CharSet = CharSet.Auto)]
    private static extern bool LookupPrivilegeValue(string? lpSystemName, string lpName, out LUID luid);

    [DllImport("advapi32.dll", SetLastError = true)]
    private static extern bool AdjustTokenPrivileges(
        IntPtr tokenHandle,
        bool disableAllPrivileges,
        ref TOKEN_PRIVILEGES newState,
        int bufferLength,
        IntPtr previousState,
        IntPtr returnLength);

    [StructLayout(LayoutKind.Sequential)]
    private struct LUID
    {
        public uint LowPart;
        public int HighPart;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct TOKEN_PRIVILEGES
    {
        public int PrivilegeCount;
        public LUID Luid;
        public int Attributes;
    }

    private const int TOKEN_ADJUST_PRIVILEGES = 0x0020;
    private const int TOKEN_QUERY = 0x0008;
    private const int SE_PRIVILEGE_ENABLED = 0x0002;

    private static bool _debugPrivilegeAttempted;

    /// <summary>
    /// 嘗試啟用 SeDebugPrivilege：管理員權杖預設具備此權限但未啟動，
    /// 需顯式呼叫 AdjustTokenPrivileges 開啟後，OpenProcess 才能無視目標行程的
    /// DACL 限制（Antigravity language_server 已對 PROCESS_VM_READ 加上拒絕規則）。
    /// 非管理員權杖沒有這個特權，呼叫會失敗但不影響其餘備援方案。
    /// </summary>
    private static void EnsureDebugPrivilegeEnabled()
    {
        if (_debugPrivilegeAttempted) return;
        _debugPrivilegeAttempted = true;

        if (!OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, out var hToken))
        {
            return;
        }

        try
        {
            if (!LookupPrivilegeValue(null, "SeDebugPrivilege", out var luid))
            {
                return;
            }

            var privileges = new TOKEN_PRIVILEGES
            {
                PrivilegeCount = 1,
                Luid = luid,
                Attributes = SE_PRIVILEGE_ENABLED
            };

            AdjustTokenPrivileges(hToken, false, ref privileges, 0, IntPtr.Zero, IntPtr.Zero);
        }
        finally
        {
            CloseHandle(hToken);
        }
    }

    [DllImport("kernel32.dll")]
    private static extern IntPtr GetCurrentProcess();

    [StructLayout(LayoutKind.Sequential)]
    private struct PROCESS_BASIC_INFORMATION
    {
        public IntPtr ExitStatus;
        public IntPtr PebBaseAddress;
        public IntPtr AffinityMask;
        public IntPtr BasePriority;
        public IntPtr UniqueProcessId;
        public IntPtr InheritedFromUniqueProcessId;
    }

    private const int PROCESS_QUERY_INFORMATION = 0x0400;
    private const int PROCESS_QUERY_LIMITED_INFORMATION = 0x1000;
    private const int PROCESS_VM_READ = 0x0010;

    public static string? GetCommandLine(int pid)
    {
        if (!OperatingSystem.IsWindows()) return null;

        EnsureDebugPrivilegeEnabled();

        var hProcess = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, false, pid);
        if (hProcess == IntPtr.Zero)
        {
            hProcess = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid);
        }
        if (hProcess == IntPtr.Zero) return null;

        try
        {
            var pbi = new PROCESS_BASIC_INFORMATION();
            var status = NtQueryInformationProcess(hProcess, 0, ref pbi, Marshal.SizeOf(pbi), out _);
            if (status != 0 || pbi.PebBaseAddress == IntPtr.Zero) return null;

            if (IntPtr.Size == 8) // 64-bit
            {
                var ptrBuf = new byte[8];
                if (!ReadProcessMemory(hProcess, pbi.PebBaseAddress + 0x20, ptrBuf, 8, out _)) return null;
                var procParams = (IntPtr)BitConverter.ToInt64(ptrBuf, 0);
                if (procParams == IntPtr.Zero) return null;

                // 掃描 RTL_USER_PROCESS_PARAMETERS 中的可能 CommandLine 偏移量（0x60 ~ 0x80）
                var candidateOffsets = new[] { 0x70, 0x78, 0x68, 0x60, 0x80 };
                var cmdLineHeader = new byte[16];

                foreach (var offset in candidateOffsets)
                {
                    if (!ReadProcessMemory(hProcess, procParams + offset, cmdLineHeader, 16, out _)) continue;
                    var length = BitConverter.ToUInt16(cmdLineHeader, 0);
                    var bufferPtr = (IntPtr)BitConverter.ToInt64(cmdLineHeader, 8);
                    if (bufferPtr == IntPtr.Zero || length < 10 || length > 32768) continue;

                    var cmdBuf = new byte[length];
                    if (ReadProcessMemory(hProcess, bufferPtr, cmdBuf, length, out _))
                    {
                        var str = Encoding.Unicode.GetString(cmdBuf);
                        if (str.Contains("--csrf_token", StringComparison.OrdinalIgnoreCase) ||
                            str.Contains("language_server", StringComparison.OrdinalIgnoreCase))
                        {
                            return str;
                        }
                    }
                }
            }
            else // 32-bit
            {
                var ptrBuf = new byte[4];
                if (!ReadProcessMemory(hProcess, pbi.PebBaseAddress + 0x10, ptrBuf, 4, out _)) return null;
                var procParams = (IntPtr)BitConverter.ToInt32(ptrBuf, 0);
                if (procParams == IntPtr.Zero) return null;

                var candidateOffsets = new[] { 0x40, 0x44, 0x38, 0x48 };
                var cmdLineHeader = new byte[8];

                foreach (var offset in candidateOffsets)
                {
                    if (!ReadProcessMemory(hProcess, procParams + offset, cmdLineHeader, 8, out _)) continue;
                    var length = BitConverter.ToUInt16(cmdLineHeader, 0);
                    var bufferPtr = (IntPtr)BitConverter.ToInt32(cmdLineHeader, 4);
                    if (bufferPtr == IntPtr.Zero || length < 10 || length > 32768) continue;

                    var cmdBuf = new byte[length];
                    if (ReadProcessMemory(hProcess, bufferPtr, cmdBuf, length, out _))
                    {
                        var str = Encoding.Unicode.GetString(cmdBuf);
                        if (str.Contains("--csrf_token", StringComparison.OrdinalIgnoreCase) ||
                            str.Contains("language_server", StringComparison.OrdinalIgnoreCase))
                        {
                            return str;
                        }
                    }
                }
            }
        }
        catch
        {
            return null;
        }
        finally
        {
            CloseHandle(hProcess);
        }

        return null;
    }
}
