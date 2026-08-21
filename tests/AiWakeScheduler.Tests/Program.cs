using System.Reflection;
using System.Text.Json;
using AiWakeScheduler.Core;

if (args.Contains("--fake-cli", StringComparer.Ordinal))
{
    var delayIndex = Array.IndexOf(args, "--fake-delay-ms");
    if (delayIndex >= 0 && delayIndex + 1 < args.Length && int.TryParse(args[delayIndex + 1], out var delayMilliseconds))
    {
        await Task.Delay(delayMilliseconds);
    }
    var outputIndex = Array.IndexOf(args, "--fake-cli-output");
    if (outputIndex >= 0 && outputIndex + 1 < args.Length)
    {
        await File.WriteAllTextAsync(args[outputIndex + 1], JsonSerializer.Serialize(args));
    }
    var exitCodeIndex = Array.IndexOf(args, "--fake-exit-code");
    if (exitCodeIndex >= 0 && exitCodeIndex + 1 < args.Length && int.TryParse(args[exitCodeIndex + 1], out var exitCode))
    {
        return exitCode;
    }
    return 0;
}

var deterministicTests = new (string Name, Func<Task> Run)[]
{
    ("ArgumentTokenizer", TestArgumentTokenizerAsync),
    ("CliCatalog", TestCliCatalogAsync),
    ("CliCommandBuilder", TestCliCommandBuilderAsync),
    ("CliUsageReader", TestCliUsageReaderAsync),
    ("ScheduleCalculator", TestScheduleCalculatorAsync),
    ("JsonFileStore", TestJsonFileStoreAsync),
    ("CliRunnerSafeArguments", TestCliRunnerAsync),
    ("ScheduleManagerDueJob", TestScheduleManagerAsync),
    ("ScheduleManagerAdaptiveWait", TestScheduleManagerAdaptiveWaitAsync),
    ("ScheduleManagerRestartDoesNotRefire", TestScheduleManagerRestartDoesNotRefireAsync),
    ("ScheduleManagerCoalescesOverdueJobs", TestScheduleManagerCoalescesOverdueJobsAsync),
    ("ScheduleManagerAutoInterval", TestScheduleManagerAutoIntervalAsync),
    ("ScheduleManagerQuotaAwareInterval", TestScheduleManagerQuotaAwareIntervalAsync),
    ("ScheduleManagerBoundariesAndState", TestScheduleManagerBoundariesAndStateAsync),
    ("InstallerContract", TestInstallerContractAsync)
};

var integrationTests = new (string Name, Func<Task> Run)[]
{
    ("ExecutableLocatorResolution", TestExecutableLocatorAsync)
};

var runIntegration = args.Contains("--integration", StringComparer.Ordinal);
var tests = runIntegration ? integrationTests : deterministicTests;
Console.WriteLine(runIntegration
    ? "執行 opt-in 本機 CLI／登入整合測試。"
    : "執行可重現的 deterministic 測試（不需要本機 CLI 或登入狀態）。");

var failures = new List<string>();
foreach (var test in tests)
{
    try
    {
        await test.Run();
        Console.WriteLine($"PASS {test.Name}");
    }
    catch (Exception ex)
    {
        failures.Add($"{test.Name}: {ex.Message}");
        Console.WriteLine($"FAIL {test.Name}: {ex}");
    }
}

Console.WriteLine($"{tests.Length - failures.Count}/{tests.Length} tests passed");
return failures.Count == 0 ? 0 : 1;

static Task TestArgumentTokenizerAsync()
{
    Equal(["--model", "fast model", "--flag"], ArgumentTokenizer.Parse("--model \"fast model\" --flag"));
    Equal(["a\\b", "quoted \"value\""], ArgumentTokenizer.Parse("a\\b \"quoted \\\"value\\\"\""));
    Equal(Array.Empty<string>(), ArgumentTokenizer.Parse(""));
    Equal(Array.Empty<string>(), ArgumentTokenizer.Parse("   \t "));
    Equal(["a", "b", "c"], ArgumentTokenizer.Parse("a   b \t c"));
    Throws<FormatException>(() => ArgumentTokenizer.Parse("\"unfinished"));
    return Task.CompletedTask;
}

static Task TestCliCommandBuilderAsync()
{
    Equal(["--print", "早安"], CliCommandBuilder.Build(CliKind.Antigravity, "早安", tokenSaverMode: false));
    Equal(["--model", "claude-sonnet-4-6", "--print", "早安"], CliCommandBuilder.Build(CliKind.AntigravityClaude, "早安", tokenSaverMode: false));
    Equal(["--model", "custom-model", "--print", "早安"], CliCommandBuilder.Build(CliKind.AntigravityClaude, "早安", "--model custom-model", tokenSaverMode: false));
    Equal(["exec", "--skip-git-repo-check", "--ephemeral", "--color", "never", "早安"], CliCommandBuilder.Build(CliKind.Codex, "早安", tokenSaverMode: false));
    Equal(["--print", "--model", "sonnet", "早安"], CliCommandBuilder.Build(CliKind.Claude, "早安", "--model sonnet", tokenSaverMode: false));

    // 自訂 Model 與 ThinkingEffort 測試（模型 ID 與 effort 合法值均以實際 CLI 行為核對）
    var agyCustom = CliCommandBuilder.Build(
        CliKind.Antigravity,
        "早安",
        new CliProfile { Model = "gemini-3.7-flash", ThinkingEffort = ThinkingEffort.High },
        tokenSaverMode: false);
    Equal(["--model", "gemini-3.7-flash", "--effort", "high", "--print", "早安"], agyCustom);

    var codexCustom = CliCommandBuilder.Build(
        CliKind.Codex,
        "早安",
        new CliProfile { Model = "gpt-5.6-sol", ThinkingEffort = ThinkingEffort.Ultra },
        tokenSaverMode: false);
    Equal(["exec", "--skip-git-repo-check", "--ephemeral", "--color", "never", "--model", "gpt-5.6-sol", "-c", "model_reasoning_effort=\"ultra\"", "早安"], codexCustom);

    var codexMinimal = CliCommandBuilder.Build(
        CliKind.Codex,
        "早安",
        new CliProfile { ThinkingEffort = ThinkingEffort.Minimal },
        tokenSaverMode: false);
    Equal(["exec", "--skip-git-repo-check", "--ephemeral", "--color", "never", "-c", "model_reasoning_effort=\"low\"", "早安"], codexMinimal);

    var codexLunaUltra = CliCommandBuilder.Build(
        CliKind.Codex,
        "早安",
        new CliProfile { Model = "gpt-5.6-luna", ThinkingEffort = ThinkingEffort.Ultra },
        tokenSaverMode: false);
    Assert(codexLunaUltra.Contains("model_reasoning_effort=\"max\""), "GPT-5.6 Luna 的 Ultra 應降為支援的 Max。");

    var codex55Max = CliCommandBuilder.Build(
        CliKind.Codex,
        "早安",
        new CliProfile { Model = "gpt-5.5", ThinkingEffort = ThinkingEffort.Max },
        tokenSaverMode: false);
    Assert(codex55Max.Contains("model_reasoning_effort=\"xhigh\""), "GPT-5.5 的 Max 應降為支援的 XHigh。");

    var claudeCustom = CliCommandBuilder.Build(
        CliKind.Claude,
        "早安",
        new CliProfile { Model = "opus", ThinkingEffort = ThinkingEffort.Medium },
        tokenSaverMode: false);
    Equal(["--print", "--model", "opus", "--effort", "medium", "早安"], claudeCustom);

    // AntigravityClaude 的三個模型（claude-sonnet-4-6、claude-opus-4-6-thinking、
    // gpt-oss-120b-medium）在 agy 中皆已內建固定思考程度，一律不能帶 --effort。
    var agyClaudeAnyModel = CliCommandBuilder.Build(
        CliKind.AntigravityClaude,
        "早安",
        new CliProfile { Model = "gpt-oss-120b-medium", ThinkingEffort = ThinkingEffort.High },
        tokenSaverMode: false);
    Assert(!agyClaudeAnyModel.Contains("--effort"), "AntigravityClaude 的任何模型都不可帶 --effort。");

    // 迴歸測試：Antigravity 的旗標絕不能出現在 --print 之後，
    // 否則會被當成提示詞送出，其餘旗標則全部失效。
    foreach (var agyKind in new[] { CliKind.Antigravity, CliKind.AntigravityClaude })
    {
        var built = CliCommandBuilder.Build(agyKind, "早安", "--add-dir C:\\tmp", timeout: TimeSpan.FromMinutes(2));
        var printIndex = built.ToList().IndexOf("--print");
        Assert(printIndex == built.Count - 2, $"{agyKind}：--print 必須是倒數第二個參數。");
        Assert(built[^1].StartsWith("早安", StringComparison.Ordinal), $"{agyKind}：提示詞必須是 --print 的值。");
        for (var i = 0; i < printIndex; i++)
        {
            Assert(built[i] != "早安", $"{agyKind}：提示詞不應出現在旗標之間。");
        }
    }

    // 使用者以 -m 指定模型時，也不應再附加預設模型
    var shortModelOverride = CliCommandBuilder.Build(CliKind.AntigravityClaude, "早安", "-m custom", tokenSaverMode: false);
    Assert(!shortModelOverride.Contains("claude-sonnet-4-6"), "使用者以 -m 指定模型時不應覆寫。");

    // 使用者以額外參數指定思考程度時，不應重複附加
    var codexEffortOverride = CliCommandBuilder.Build(
        CliKind.Codex,
        "早安",
        new CliProfile { ThinkingEffort = ThinkingEffort.Low, AdditionalArguments = "-c model_reasoning_effort=\"high\"" },
        tokenSaverMode: false);
    var occurrences = codexEffortOverride.Count(arg => arg.Contains("model_reasoning_effort", StringComparison.OrdinalIgnoreCase));
    Assert(occurrences == 1, "使用者在額外參數自訂 model_reasoning_effort 時不應重複附加。");

    var codexModelOverride = CliCommandBuilder.Build(
        CliKind.Codex,
        "早安",
        new CliProfile { Model = "gpt-5.6-sol", ThinkingEffort = ThinkingEffort.Ultra, AdditionalArguments = "--model gpt-5.5" },
        tokenSaverMode: false);
    Assert(codexModelOverride.Count(argument => argument == "--model") == 1, "額外參數覆寫模型時不應重複附加 --model。");
    Assert(codexModelOverride.Contains("model_reasoning_effort=\"xhigh\""), "Effort 應依額外參數實際指定的 GPT-5.5 正規化為 XHigh。");

    var saverAgy = CliCommandBuilder.Build(CliKind.Antigravity, "早安", timeout: TimeSpan.FromMinutes(3));
    Assert(saverAgy.Contains("--effort") && saverAgy.Contains("low"), "Antigravity 節省模式應包含 low effort。");
    Assert(saverAgy.Contains("--disable-slash-commands"), "Antigravity 節省模式應停用斜線指令。");
    Assert(saverAgy.Contains("--mode") && saverAgy.Contains("plan"), "Antigravity 節省模式不可修改工作區。");
    Assert(saverAgy.Contains("--print-timeout") && saverAgy.Contains("180s"), "應把應用程式逾時轉為 CLI 自身的 --print-timeout。");

    var saverAgyClaude = CliCommandBuilder.Build(CliKind.AntigravityClaude, "早安");
    Assert(saverAgyClaude.Contains("--model") && saverAgyClaude.Contains("claude-sonnet-4-6"), "AntigravityClaude 預設應使用 Claude Sonnet 模型 ID。");
    Assert(saverAgyClaude.Contains("--disable-slash-commands"), "AntigravityClaude 節省模式應停用技能展開。");
    Assert(saverAgyClaude.Contains("--mode") && saverAgyClaude.Contains("plan"), "AntigravityClaude 節省模式不可修改工作區。");
    // Claude 系列模型不接受 --effort，帶上去 agy 會直接拒絕整個呼叫
    Assert(!saverAgyClaude.Contains("--effort"), "AntigravityClaude 不可帶 --effort，Claude 模型不支援。");
    Assert(!saverAgyClaude.Contains("--print-timeout"), "未指定逾時時不應產生 --print-timeout。");

    var saverCodex = CliCommandBuilder.Build(CliKind.Codex, "早安");
    Assert(saverCodex.Contains("read-only"), "節省 Token 模式應使用 Codex 唯讀沙箱。");
    Assert(saverCodex.Contains("model_reasoning_effort=\"low\""), "節省 Token 模式應降低 Codex 推理量。");
    Assert(saverCodex.Contains("--ignore-user-config"), "節省 Token 模式應略過 config.toml，避免載入 MCP 伺服器與自訂指示。");
    Assert(saverCodex.Last().Contains("只回「OK」", StringComparison.Ordinal), "節省模式應要求最短回覆。");

    var saverClaude = CliCommandBuilder.Build(CliKind.Claude, "早安");
    Assert(saverClaude.Contains("--effort") && saverClaude.Contains("low"), "Claude 節省模式應包含 low effort。");
    Assert(saverClaude.Contains("--tools") && saverClaude.Contains(string.Empty), "節省模式應停用 Claude 內建工具。");
    Assert(saverClaude.Contains("--safe-mode"), "節省模式應停用 CLAUDE.md、技能、外掛與 MCP 伺服器。");
    Assert(saverClaude.Contains("--strict-mcp-config"), "節省模式應確保不載入任何 MCP 工具結構描述。");
    Assert(saverClaude.Contains("--no-session-persistence"), "節省模式不應保存一次性喚醒 session。");
    var suggestionsIndex = saverClaude.ToList().IndexOf("--prompt-suggestions");
    Assert(suggestionsIndex >= 0 && saverClaude[suggestionsIndex + 1] == "false", "節省模式應停用額外的提示建議生成。");

    // 節省模式下若使用者明確指定了 ThinkingEffort.High，應尊重使用者設定而非強行覆蓋為 low
    var saverCodexHigh = CliCommandBuilder.Build(
        CliKind.Codex,
        "早安",
        new CliProfile { ThinkingEffort = ThinkingEffort.High },
        tokenSaverMode: true);
    Assert(saverCodexHigh.Contains("model_reasoning_effort=\"high\""), "使用者指定 High 時應使用 high。");
    Assert(!saverCodexHigh.Contains("model_reasoning_effort=\"low\""), "使用者指定 High 時不應包含 low。");

    // --tools 是可變長度參數，後面必須緊接旗標，否則會吃掉提示詞
    var toolsIndex = saverClaude.ToList().IndexOf("--tools");
    Assert(toolsIndex >= 0 && saverClaude[toolsIndex + 1].Length == 0, "--tools 之後應緊接空字串。");
    Assert(saverClaude[toolsIndex + 2].StartsWith("--", StringComparison.Ordinal), "--tools 的值之後必須是旗標，避免吞掉位置參數。");

    return Task.CompletedTask;
}

static Task TestInstallerContractAsync()
{
    var directory = new DirectoryInfo(AppContext.BaseDirectory);
    while (directory is not null && !File.Exists(Path.Combine(directory.FullName, "build-installer.ps1")))
    {
        directory = directory.Parent;
    }

    Assert(directory is not null, "找不到 AI Wake Scheduler repository root。");
    var installerDirectory = Path.Combine(directory!.FullName, "installer");
    var installerPath = Directory.GetFiles(installerDirectory, "*.iss").Single();
    var script = File.ReadAllText(installerPath);
    var lines = script.Split(["\r\n", "\n"], StringSplitOptions.RemoveEmptyEntries);

    foreach (var directive in new[]
    {
        "AllowNoIcons=no",
        "DisableProgramGroupPage=yes",
        "UsePreviousAppDir=yes",
        "UsePreviousGroup=yes",
        "UsePreviousTasks=yes",
        "CreateUninstallRegKey=yes",
        "SetupLogging=yes",
        "UninstallLogging=yes",
        "RestartApplications=no"
    })
    {
        Assert(lines.Contains(directive, StringComparer.Ordinal), $"Installer 缺少必要設定：{directive}");
    }

    var startMenuShortcut = lines.Single(line =>
        line.StartsWith("Name: \"{group}\\{#MyAppName}\";", StringComparison.Ordinal));
    Assert(!startMenuShortcut.Contains("Tasks:", StringComparison.Ordinal), "開始功能表捷徑必須固定建立。");
    Assert(startMenuShortcut.Contains("WorkingDir: \"{app}\"", StringComparison.Ordinal), "開始功能表捷徑缺少工作目錄。");
    Assert(startMenuShortcut.Contains("AppUserModelID:", StringComparison.Ordinal), "開始功能表捷徑缺少 AppUserModelID。");

    var uninstallShortcut = lines.Single(line =>
        line.Contains("{cm:UninstallProgram,{#MyAppName}}", StringComparison.Ordinal));
    Assert(!uninstallShortcut.Contains("Tasks:", StringComparison.Ordinal), "解除安裝捷徑必須固定建立。");

    var desktopTask = lines.Single(line => line.StartsWith("Name: \"desktopicon\";", StringComparison.Ordinal));
    var startupTask = lines.Single(line => line.StartsWith("Name: \"startupicon\";", StringComparison.Ordinal));
    Assert(desktopTask.Contains("Flags: unchecked", StringComparison.Ordinal), "桌面捷徑必須預設不勾選。");
    Assert(startupTask.Contains("Flags: unchecked", StringComparison.Ordinal), "開機啟動必須預設不勾選。");

    return Task.CompletedTask;
}

static Task TestCliUsageReaderAsync()
{
    var observedAt = new DateTimeOffset(2026, 8, 12, 8, 0, 0, TimeSpan.FromHours(8));
    var primaryReset = observedAt.AddHours(2).ToUnixTimeSeconds();
    var secondaryReset = observedAt.AddDays(6).ToUnixTimeSeconds();
    var response = $$"""
        {
          "id": 1,
          "result": {
            "rateLimits": {},
            "rateLimitsByLimitId": {
              "codex": {
                "limitId": "codex",
                "primary": { "usedPercent": 7, "windowDurationMins": 300, "resetsAt": {{primaryReset}} },
                "secondary": { "usedPercent": 35, "windowDurationMins": 10080, "resetsAt": {{secondaryReset}} }
              }
            }
          }
        }
        """;

    var snapshot = CliUsageReader.ParseCodexRateLimits(response, observedAt);
    Assert(snapshot.Availability == CliUsageAvailability.Available, "Codex rate-limit 回應應可解析。");
    Assert(snapshot.Windows.Count == 2, "主要與次要額度視窗都應保留。");
    Assert(snapshot.Windows[0].RemainingPercent == 93, "剩餘百分比應為 100 減 usedPercent。");
    Assert(snapshot.Windows[0].ResetsAt?.ToUnixTimeSeconds() == primaryReset, "應解析 Unix 重置時間。");
    Assert(snapshot.Windows[1].Duration == TimeSpan.FromDays(7), "應解析額度視窗長度。");

    var error = CliUsageReader.ParseCodexRateLimits(
        "{\"id\":1,\"error\":{\"message\":\"not authenticated\"}}",
        observedAt);
    Assert(error.Availability == CliUsageAvailability.Unavailable, "伺服器錯誤不可標示為可用額度。");

    var claudeResponse = """
        {
          "five_hour": {
            "utilization": 7.4,
            "resets_at": "2026-08-12T10:30:00+08:00"
          },
          "seven_day": {
            "utilization": 26.0,
            "resets_at": "2026-08-18T05:59:59+08:00"
          },
          "seven_day_opus": null,
          "extra_usage": { "is_enabled": false }
        }
        """;
    var claudeSnapshot = CliUsageReader.ParseClaudeUsage(claudeResponse, observedAt);
    Assert(claudeSnapshot.Availability == CliUsageAvailability.Available, "Claude OAuth usage 回應應可解析。");
    Assert(claudeSnapshot.Windows.Count == 2, "Claude 五小時與七天額度視窗都應保留。");
    Assert(claudeSnapshot.Windows[0].UsedPercent == 7, "Claude utilization 應四捨五入為已使用百分比。");
    Assert(claudeSnapshot.Windows[0].RemainingPercent == 93, "Claude 剩餘百分比應為 100 減 utilization。");
    Assert(claudeSnapshot.Windows[0].Duration == TimeSpan.FromHours(5), "Claude 主要額度應標示五小時視窗。");
    Assert(claudeSnapshot.Windows[0].ResetsAt?.Offset == TimeSpan.FromHours(8), "Claude 重置時間應保留原始時區。");
    Assert(claudeSnapshot.Windows[1].Duration == TimeSpan.FromDays(7), "Claude 次要額度應標示七天視窗。");

    var claudeError = CliUsageReader.ParseClaudeUsage(
        "{\"error\":{\"message\":\"invalid bearer token\"}}",
        observedAt);
    Assert(claudeError.Availability == CliUsageAvailability.Unavailable, "Claude 錯誤回應不可標示為可用額度。");

    // Antigravity (Gemini 與 Claude/GPT 雙額度池解析測試)
    var agyResponse = """
        {
          "clientModelConfigs": [
            {
              "label": "Gemini 3.7 Flash (High)",
              "quotaInfo": {
                "remainingFraction": 0.9006,
                "resetTime": "2026-08-18T05:25:50Z"
              }
            },
            {
              "label": "Claude Sonnet 4.6 (Thinking)",
              "quotaInfo": {
                "remainingFraction": 1.0,
                "resetTime": "2026-08-18T09:09:25Z"
              }
            }
          ]
        }
        """;

    var agyGeminiSnapshot = CliUsageReader.ParseAntigravityModelConfigs(agyResponse, CliKind.Antigravity, observedAt);
    Assert(agyGeminiSnapshot.Availability == CliUsageAvailability.Available, "Antigravity (Gemini) 配額回應應可解析。");
    Assert(agyGeminiSnapshot.Windows.Count == 1, "Gemini 應有一個額度視窗。");
    Assert(agyGeminiSnapshot.Windows[0].RemainingPercent == 90, "剩餘百分比應為 90。");
    Assert(agyGeminiSnapshot.Windows[0].UsedPercent == 10, "已使用百分比應為 10。");
    Assert(agyGeminiSnapshot.Windows[0].ResetsAt?.UtcDateTime.Hour == 5, "應精確解析 UTC 重置時分。");

    var agyClaudeSnapshot = CliUsageReader.ParseAntigravityModelConfigs(agyResponse, CliKind.AntigravityClaude, observedAt);
    Assert(agyClaudeSnapshot.Availability == CliUsageAvailability.Available, "Antigravity (Claude / GPT) 配額回應應可解析。");
    Assert(agyClaudeSnapshot.Windows.Count == 1, "Claude/GPT 應有一個額度視窗。");
    Assert(agyClaudeSnapshot.Windows[0].RemainingPercent == 100, "剩餘百分比應為 100。");
    Assert(agyClaudeSnapshot.Windows[0].UsedPercent == 0, "已使用百分比應為 0。");
    Assert(agyClaudeSnapshot.Windows[0].ResetsAt?.UtcDateTime.Hour == 9, "應精確解析 UTC 重置時分。");

    var agyError = CliUsageReader.ParseAntigravityModelConfigs("{\"error\":{\"message\":\"invalid CSRF token\"}}", CliKind.Antigravity, observedAt);
    Assert(agyError.Availability == CliUsageAvailability.Unavailable, "Antigravity 錯誤回應不可標示為可用。");

    var agyMissingPool = CliUsageReader.ParseAntigravityModelConfigs("{\"clientModelConfigs\":[]}", CliKind.Antigravity, observedAt);
    Assert(agyMissingPool.Availability == CliUsageAvailability.Unavailable, "缺少模型配額時應回傳 Unavailable。");

    // 測試 IsCountingDown 判斷邏輯
    var futureReset = observedAt.AddHours(3);
    var pastReset = observedAt.AddHours(-1);

    var countingDownSnapshot = new CliUsageSnapshot(
        CliKind.Antigravity,
        CliUsageAvailability.Available,
        [new CliUsageWindow("Antigravity (Gemini)", 10, TimeSpan.FromHours(5), futureReset, IsActiveCountdown: true)],
        "讀取成功",
        observedAt);
    Assert(CliUsageReader.IsCountingDown(countingDownSnapshot, observedAt), "真實倒數且重置時間在未來時應判定為倒數中。");

    var slidingSnapshot = new CliUsageSnapshot(
        CliKind.AntigravityClaude,
        CliUsageAvailability.Available,
        [new CliUsageWindow("Antigravity (Claude / GPT)", 0, TimeSpan.FromHours(5), futureReset, IsActiveCountdown: false)],
        "讀取成功",
        observedAt);
    Assert(!CliUsageReader.IsCountingDown(slidingSnapshot, observedAt), "滑動時間戳記（未消耗額度）應判定為未倒數。");

    var expiredSnapshot = new CliUsageSnapshot(
        CliKind.Antigravity,
        CliUsageAvailability.Available,
        [new CliUsageWindow("Antigravity (Gemini)", 0, TimeSpan.FromHours(5), pastReset, IsActiveCountdown: false)],
        "讀取成功",
        observedAt);
    Assert(!CliUsageReader.IsCountingDown(expiredSnapshot, observedAt), "重置時間在過去時應判定為未倒數。");

    var claude5hPast7dFuture = new CliUsageSnapshot(
        CliKind.Claude,
        CliUsageAvailability.Available,
        [
            new CliUsageWindow("Claude（5 小時）", 0, TimeSpan.FromHours(5), pastReset, IsActiveCountdown: false),
            new CliUsageWindow("Claude（7 天）", 20, TimeSpan.FromDays(7), futureReset.AddDays(4), IsActiveCountdown: true)
        ],
        "讀取成功",
        observedAt);
    Assert(!CliUsageReader.IsCountingDown(claude5hPast7dFuture, observedAt), "Claude 5小時已重置時即使7天仍在倒數也應判定為未倒數。");

    Assert(!CliUsageReader.IsCountingDown(null, observedAt), "null snapshot 應判定為未倒數。");
    Assert(!CliUsageReader.IsCountingDown(claudeError, observedAt), "Unavailable snapshot 應判定為未倒數。");

    return Task.CompletedTask;
}

static async Task TestScheduleManagerRestartDoesNotRefireAsync()
{
    var directory = CreateTemporaryDirectory();
    try
    {
        var paths = new AppDataPaths(Path.Combine(directory, "data"));
        var store = new JsonFileStore<List<ScheduledJob>>(paths.JobsFile, () => []);
        var settings = AppSettings.CreateDefault();
        var runner = new CountingCliRunner();

        // 今天的時分已經過了，而且今天已經跑過一次（FinishedAt 在該時點之後）
        var alreadyRanAt = DateTimeOffset.Now.AddMinutes(-30);
        var id = Guid.NewGuid();
        await store.SaveAsync(
        [
            new ScheduledJob
            {
                Id = id,
                Name = "今天已執行",
                ScheduledAt = alreadyRanAt,
                FinishedAt = alreadyRanAt.AddSeconds(20),
                Message = "早安",
                WorkingDirectory = directory,
                Targets = [CliKind.Antigravity]
            }
        ]);

        await using (var manager = new ScheduleManager(store, runner, () => settings))
        {
            await manager.InitializeAsync();
            await Task.Delay(1500);

            var job = (await manager.GetJobsAsync()).Single(item => item.Id == id);
            Assert(runner.CallCount == 0, $"重開程式不應重跑今天已完成的喚醒，實際呼叫 {runner.CallCount} 次。");
            Assert(job.ScheduledAt > DateTimeOffset.Now, "今天已執行過的排程應排到明天。");
            Assert(job.Status == ScheduleStatus.Pending, "排程應維持等待中。");
        }

        // 相對地，程式關閉期間錯過的喚醒仍應補做一次
        var missedRunner = new CountingCliRunner();
        var missedId = Guid.NewGuid();
        await store.SaveAsync(
        [
            new ScheduledJob
            {
                Id = missedId,
                Name = "錯過未執行",
                ScheduledAt = DateTimeOffset.Now.AddMinutes(-30),
                FinishedAt = null,
                Message = "早安",
                WorkingDirectory = directory,
                Targets = [CliKind.Antigravity]
            }
        ]);

        await using (var manager = new ScheduleManager(store, missedRunner, () => settings))
        {
            await manager.InitializeAsync();
            var deadline = DateTime.UtcNow.AddSeconds(10);
            while (DateTime.UtcNow < deadline && missedRunner.CallCount == 0)
            {
                await Task.Delay(25);
            }
            Assert(missedRunner.CallCount == 1, "程式關閉期間錯過的喚醒仍應補做一次。");
        }
    }
    finally
    {
        Directory.Delete(directory, true);
    }
}

static async Task TestScheduleManagerCoalescesOverdueJobsAsync()
{
    var directory = CreateTemporaryDirectory();
    try
    {
        var paths = new AppDataPaths(Path.Combine(directory, "data"));
        var store = new JsonFileStore<List<ScheduledJob>>(paths.JobsFile, () => []);
        var settings = AppSettings.CreateDefault();
        var runner = new CountingCliRunner();

        // 程式關閉了一段時間，三個排程都錯過了各自的時分。
        var oldestId = Guid.NewGuid();
        var middleId = Guid.NewGuid();
        var newestId = Guid.NewGuid();
        await store.SaveAsync(
        [
            new ScheduledJob
            {
                Id = oldestId,
                Name = "最早錯過",
                ScheduledAt = DateTimeOffset.Now.AddHours(-3),
                FinishedAt = null,
                Message = "早安",
                WorkingDirectory = directory,
                Targets = [CliKind.Antigravity]
            },
            new ScheduledJob
            {
                Id = middleId,
                Name = "中間錯過",
                ScheduledAt = DateTimeOffset.Now.AddHours(-2),
                FinishedAt = null,
                Message = "早安",
                WorkingDirectory = directory,
                Targets = [CliKind.Antigravity]
            },
            new ScheduledJob
            {
                Id = newestId,
                Name = "最近錯過",
                ScheduledAt = DateTimeOffset.Now.AddMinutes(-30),
                FinishedAt = null,
                Message = "早安",
                WorkingDirectory = directory,
                Targets = [CliKind.Antigravity]
            }
        ]);

        await using var manager = new ScheduleManager(store, runner, () => settings);
        await manager.InitializeAsync();

        var deadline = DateTime.UtcNow.AddSeconds(10);
        while (DateTime.UtcNow < deadline && runner.CallCount == 0)
        {
            await Task.Delay(25);
        }
        // 給補跑的那個排程一點時間完成，並讓其餘排程有機會（錯誤地）被觸發。
        await Task.Delay(500);

        Assert(runner.CallCount == 1, $"多個排程同時逾時時只應補做最近的一個，實際呼叫 {runner.CallCount} 次。");

        var jobs = await manager.GetJobsAsync();
        var newest = jobs.Single(item => item.Id == newestId);
        var middle = jobs.Single(item => item.Id == middleId);
        var oldest = jobs.Single(item => item.Id == oldestId);

        Assert(newest.Status == ScheduleStatus.Pending, "被補做的排程應回到等待中。");
        Assert(newest.ScheduledAt > DateTimeOffset.Now, "被補做的排程應排到下一個每日時點。");

        Assert(middle.Status == ScheduleStatus.Pending, "被跳過的排程不應卡在執行中。");
        Assert(middle.ScheduledAt > DateTimeOffset.Now, "被跳過的排程應直接推到下一個每日時點，而不是重複呼叫。");
        Assert(oldest.Status == ScheduleStatus.Pending, "被跳過的排程不應卡在執行中。");
        Assert(oldest.ScheduledAt > DateTimeOffset.Now, "被跳過的排程應直接推到下一個每日時點，而不是重複呼叫。");
    }
    finally
    {
        Directory.Delete(directory, true);
    }
}

static Task TestCliCatalogAsync()
{
    foreach (var kind in Enum.GetValues<CliKind>())
    {
        var descriptor = CliCatalog.Get(kind);
        Assert(descriptor.Kind == kind, "描述應對應到正確的 CLI 種類。");
        Assert(!string.IsNullOrWhiteSpace(descriptor.DisplayName), "每個 CLI 都應有顯示名稱。");
        Assert(!string.IsNullOrWhiteSpace(descriptor.ShortName), "每個 CLI 都應有短名稱。");
        Assert(!string.IsNullOrWhiteSpace(descriptor.DefaultCommand), "每個 CLI 都應有預設命令。");
        Assert(CliDisplayNames.Get(kind) == descriptor.DisplayName, "顯示名稱應由目錄提供。");
        Assert(CliDisplayNames.GetShort(kind) == descriptor.ShortName, "短名稱應由目錄提供。");
        Assert(descriptor.PresetModels.Count >= 3, "每個 CLI 應至少提供最新 3 款推薦模型選項。");
        Assert(descriptor.SupportedEfforts.Contains(ThinkingEffort.Default), "每個 CLI 都應允許使用 CLI 自身預設思考程度。");
    }

    // AntigravityClaude 的三個模型都已內建固定思考程度，agy 會直接拒絕 --effort，
    // 因此這個設定檔刻意只提供「預設」一個選項（詳見 CliCommandBuilder 的實測註解）。
    Assert(CliCatalog.Get(CliKind.AntigravityClaude).SupportedEfforts.Count == 1, "AntigravityClaude 不應提供無法生效的思考程度選項。");

    var codex = CliCatalog.Get(CliKind.Codex);
    Equal(
        ["", "gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna", "gpt-5.5", "gpt-5.4", "gpt-5.4-mini"],
        codex.PresetModels);
    Assert(!codex.PresetModels.Any(model => model.Contains("gpt-5.2", StringComparison.OrdinalIgnoreCase) || model.Contains("gpt-5.1", StringComparison.OrdinalIgnoreCase)),
        "Codex 推薦清單不應再包含 deprecated 的 GPT-5.2/5.1 Codex 模型。");
    Assert(codex.GetSupportedEfforts("gpt-5.6-sol").Contains(ThinkingEffort.Ultra), "GPT-5.6 Sol 應支援 Ultra。");
    Assert(!codex.GetSupportedEfforts("gpt-5.6-luna").Contains(ThinkingEffort.Ultra), "GPT-5.6 Luna 不支援 Ultra。");
    Assert(codex.NormalizeEffort("gpt-5.5", ThinkingEffort.Max) == ThinkingEffort.XHigh, "GPT-5.5 Max 應正規化為 XHigh。");

    foreach (var kind in new[] { CliKind.Antigravity, CliKind.Codex, CliKind.Claude })
    {
        Assert(CliCatalog.Get(kind).SupportedEfforts.Count >= 3, $"{kind} 應至少支援 3 種思考程度選項。");
    }

    Assert(CliCatalog.All.Count == Enum.GetValues<CliKind>().Length, "目錄應涵蓋所有 CliKind。");
    return Task.CompletedTask;
}

static async Task TestScheduleManagerAdaptiveWaitAsync()
{
    var directory = CreateTemporaryDirectory();
    try
    {
        var paths = new AppDataPaths(Path.Combine(directory, "data"));
        var store = new JsonFileStore<List<ScheduledJob>>(paths.JobsFile, () => []);
        var settings = AppSettings.CreateDefault();
        var runner = new CountingCliRunner();

        var id = Guid.NewGuid();
        await store.SaveAsync(
        [
            new ScheduledJob
            {
                Id = id,
                Name = "到期喚醒",
                ScheduledAt = DateTimeOffset.Now.AddSeconds(-1),
                Message = "早安",
                WorkingDirectory = directory,
                Targets = [CliKind.Antigravity, CliKind.Claude]
            }
        ]);

        await using var manager = new ScheduleManager(store, runner, () => settings);

        // 排程器改為自適應等待後，第一輪掃描必須在啟動時立即發生，
        // 而不是等待任何固定間隔。
        var stopwatch = System.Diagnostics.Stopwatch.StartNew();
        await manager.InitializeAsync();

        var deadline = DateTime.UtcNow.AddSeconds(15);
        while (DateTime.UtcNow < deadline && runner.CallCount < 2)
        {
            await Task.Delay(25);
        }
        stopwatch.Stop();

        Assert(runner.CallCount == 2, $"應對兩個 CLI 各呼叫一次，實際 {runner.CallCount} 次。");
        Assert(runner.LastTokenSaverMode, "預設應以節省 Token 模式執行。");
        Assert(
            stopwatch.Elapsed < TimeSpan.FromSeconds(2),
            $"到期排程應立即啟動，不應等待輪詢間隔；實際 {stopwatch.Elapsed.TotalMilliseconds:0} ms。");

        var job = (await manager.GetJobsAsync()).Single(item => item.Id == id);
        Assert(job.Status == ScheduleStatus.Pending, "執行完成後應回到等待狀態。");
        Assert(job.ScheduledAt > DateTimeOffset.Now, "應排到下一個未來時分。");

        // 手動觸發應透過喚醒訊號立即執行，而非等到下一個間隔
        var manualStopwatch = System.Diagnostics.Stopwatch.StartNew();
        await manager.RunNowAsync(id);
        deadline = DateTime.UtcNow.AddSeconds(15);
        while (DateTime.UtcNow < deadline && runner.CallCount < 4)
        {
            await Task.Delay(25);
        }
        manualStopwatch.Stop();
        Assert(runner.CallCount == 4, $"立即執行應再呼叫兩次，實際共 {runner.CallCount} 次。");
        Assert(
            manualStopwatch.Elapsed < TimeSpan.FromSeconds(2),
            $"立即執行應馬上啟動；實際 {manualStopwatch.Elapsed.TotalMilliseconds:0} ms。");
    }
    finally
    {
        Directory.Delete(directory, true);
    }
}

static Task TestScheduleCalculatorAsync()
{
    var now = new DateTimeOffset(2026, 8, 10, 12, 0, 0, TimeSpan.FromHours(8));
    var morning = ScheduleCalculator.GetNextDailyOccurrence(new TimeSpan(8, 30, 45), now);
    Assert(morning.LocalDateTime == new DateTime(2026, 8, 11, 8, 30, 0), "已過的每日時分應排到明天並移除秒數。");
    var evening = ScheduleCalculator.GetNextDailyOccurrence(new TimeSpan(18, 5, 0), now);
    Assert(evening.LocalDateTime == new DateTime(2026, 8, 10, 18, 5, 0), "尚未到的每日時分應排在今天。");

    // 跨日邊界測試：午夜 23:59:59 評估 00:00:00 排程
    var midnightNow = new DateTimeOffset(2026, 12, 31, 23, 59, 59, TimeSpan.FromHours(8));
    var nextDayMidnight = ScheduleCalculator.GetNextDailyOccurrence(TimeSpan.Zero, midnightNow);
    Assert(nextDayMidnight.LocalDateTime == new DateTime(2027, 1, 1, 0, 0, 0), "跨日午夜排程應排到下一年的 1 月 1 日 00:00。");

    // 時分剛好等於目前時刻邊界 (應排到明天)
    var exactNow = new DateTimeOffset(2026, 8, 10, 8, 30, 0, TimeSpan.FromHours(8));
    var exactNext = ScheduleCalculator.GetNextDailyOccurrence(new TimeSpan(8, 30, 0), exactNow);
    Assert(exactNext.LocalDateTime == new DateTime(2026, 8, 11, 8, 30, 0), "當前時刻與排程時間相同時應排到明天。");

    // 追溯過期多日 (系統休眠/時鐘回撥/錯過執行)
    var old = new DateTimeOffset(2026, 8, 1, 8, 30, 0, TimeSpan.FromHours(8));
    var daily = ScheduleCalculator.GetNextOccurrence(old, ScheduleRecurrence.Daily, now);
    Assert(daily.LocalDateTime == new DateTime(2026, 8, 11, 8, 30, 0), "過期多日的每日排程應跳到目前時間之後的下一個時點。");

    var oldWeekly = new DateTimeOffset(2026, 7, 1, 8, 30, 0, TimeSpan.FromHours(8));
    var weekly = ScheduleCalculator.GetNextOccurrence(oldWeekly, ScheduleRecurrence.Weekly, now);
    Assert(weekly.LocalDateTime > now.LocalDateTime && weekly.LocalDateTime.DayOfWeek == oldWeekly.DayOfWeek, "過期多週的每週排程應保持相同星期數並跳至未來。");

    // 夏令時間 (DST) 與時區時分保留測試
    var dstOffsetNow = new DateTimeOffset(2026, 3, 29, 23, 30, 0, TimeSpan.FromHours(-5)); // 美東/歐美 DST 變更日
    var dstNext = ScheduleCalculator.GetNextDailyOccurrence(new TimeSpan(8, 30, 0), dstOffsetNow);
    Assert(dstNext.LocalDateTime.Hour == 8 && dstNext.LocalDateTime.Minute == 30, "跨時區/DST 時應保持本地牆上時間 (Wall-clock time) 的時與分。");

    // 參數邊界與例外測試
    Throws<ArgumentOutOfRangeException>(() => ScheduleCalculator.GetNextDailyOccurrence(TimeSpan.FromHours(-1), now));
    Throws<ArgumentOutOfRangeException>(() => ScheduleCalculator.GetNextDailyOccurrence(TimeSpan.FromHours(24), now));
    Throws<ArgumentException>(() => ScheduleCalculator.GetNextOccurrence(old, ScheduleRecurrence.Once, now));

    // 邊界合法值 (00:00:00 與 23:59:59)
    var minTime = ScheduleCalculator.GetNextDailyOccurrence(TimeSpan.Zero, now);
    Assert(minTime.LocalDateTime.TimeOfDay == TimeSpan.Zero, "00:00 應為合法每日時間。");
    var maxTime = ScheduleCalculator.GetNextDailyOccurrence(new TimeSpan(23, 59, 59), now);
    Assert(maxTime.LocalDateTime.TimeOfDay == new TimeSpan(23, 59, 0), "23:59 應為合法每日時間。");

    // 自動模式（每 5 小時 1 分鐘 = 301 分鐘）測試
    Assert(ScheduleCalculator.AutoInterval == TimeSpan.FromMinutes(301), "AutoInterval 應精確為 301 分鐘（5 小時 1 分鐘）。");
    var initialWakeup = new TimeSpan(5, 30, 0);

    // 1. 同日日間正常推進（08:00 完成 -> 13:01）
    var finishedMorning = new DateTimeOffset(2026, 8, 10, 8, 0, 0, TimeSpan.FromHours(8));
    var nextMorning = ScheduleCalculator.GetNextAutoIntervalOccurrence(initialWakeup, finishedMorning);
    Assert(nextMorning == finishedMorning.Add(ScheduleCalculator.AutoInterval), "當日日間 08:00 執行完成應推進至 13:01。");
    Assert(nextMorning.LocalDateTime == new DateTime(2026, 8, 10, 13, 1, 0), "13:01 時間應正確。");

    // 2. 同日傍晚正常推進（18:02 完成 -> 23:03）
    var finishedEvening = new DateTimeOffset(2026, 8, 10, 18, 2, 0, TimeSpan.FromHours(8));
    var nextEvening = ScheduleCalculator.GetNextAutoIntervalOccurrence(initialWakeup, finishedEvening);
    Assert(nextEvening == finishedEvening.Add(ScheduleCalculator.AutoInterval), "當日 18:02 執行完成應推進至 23:03。");
    Assert(nextEvening.LocalDateTime == new DateTime(2026, 8, 10, 23, 3, 0), "23:03 時間應正確。");

    // 3. 跨日不排半夜：23:03 完成後加 5h1m 為隔日 04:04（超過 24:00），應自動重置為隔日 05:30
    var finishedLateNight = new DateTimeOffset(2026, 8, 10, 23, 3, 0, TimeSpan.FromHours(8));
    var nextCrossDay = ScheduleCalculator.GetNextAutoIntervalOccurrence(initialWakeup, finishedLateNight);
    Assert(nextCrossDay.LocalDateTime == new DateTime(2026, 8, 11, 5, 30, 0), "23:03 跨日後不應排在半夜 04:04，應排在隔天 05:30。");

    // 4. 20:00 執行跨日（加 5h1m 為 01:01），重置為隔日 05:30
    var finished8pm = new DateTimeOffset(2026, 8, 10, 20, 0, 0, TimeSpan.FromHours(8));
    var nextAfter8pm = ScheduleCalculator.GetNextAutoIntervalOccurrence(initialWakeup, finished8pm);
    Assert(nextAfter8pm.LocalDateTime == new DateTime(2026, 8, 11, 5, 30, 0), "20:00 跨日後應排在隔天 05:30。");

    // 5. 跨日計算異常邊界
    Throws<ArgumentOutOfRangeException>(() => ScheduleCalculator.GetNextAutoIntervalOccurrence(TimeSpan.FromHours(-1), finishedMorning));
    Throws<ArgumentOutOfRangeException>(() => ScheduleCalculator.GetNextAutoIntervalOccurrence(TimeSpan.FromHours(24), finishedMorning));

    return Task.CompletedTask;
}

static async Task TestJsonFileStoreAsync()
{
    var directory = CreateTemporaryDirectory();
    try
    {
        var path = Path.Combine(directory, "jobs.json");
        var store = new JsonFileStore<List<ScheduledJob>>(path, () => []);
        var expected = new ScheduledJob
        {
            Name = "測試排程",
            Message = "早安",
            ScheduledAt = DateTimeOffset.Now.AddHours(1),
            Targets = [CliKind.Antigravity, CliKind.Codex, CliKind.Claude]
        };
        await store.SaveAsync([expected]);
        var actual = await store.LoadAsync();
        Assert(actual.Count == 1, "應載入一筆排程。");
        Assert(actual[0].Name == expected.Name, "排程名稱應往返保存。");
        Equal(expected.Targets, actual[0].Targets);

        // 1. 測試讀取不存在的檔案 (應傳回 defaultFactory)
        var nonExistentPath = Path.Combine(directory, "non_existent.json");
        var nonExistentStore = new JsonFileStore<List<ScheduledJob>>(nonExistentPath, () => []);
        var loadedDefault = await nonExistentStore.LoadAsync();
        Assert(loadedDefault.Count == 0, "不存在的檔案應傳回預設工廠建立的物件。");

        // 2. 測試破損 JSON 內容 (損毀語法)
        var corruptedPath = Path.Combine(directory, "corrupted.json");
        var corruptedStore = new JsonFileStore<List<ScheduledJob>>(corruptedPath, () => []);
        await File.WriteAllTextAsync(corruptedPath, "INVALID JSON {{ [");
        await ThrowsAsync<InvalidDataException>(async () => await corruptedStore.LoadAsync());

        // 3. 測試截斷/未完成的 JSON 內容 (模擬檔案寫入遺失/部分寫入)
        var truncatedPath = Path.Combine(directory, "truncated.json");
        var truncatedStore = new JsonFileStore<List<ScheduledJob>>(truncatedPath, () => []);
        await File.WriteAllTextAsync(truncatedPath, "[{\"Name\":\"截斷內容");
        await ThrowsAsync<InvalidDataException>(async () => await truncatedStore.LoadAsync());

        // 4. 測試原子寫入與臨時檔 (.tmp) 清理
        var atomicPath = Path.Combine(directory, "atomic.json");
        var atomicStore = new JsonFileStore<List<ScheduledJob>>(atomicPath, () => []);
        await atomicStore.SaveAsync([expected]);
        Assert(File.Exists(atomicPath), "目標檔案應原子替換寫入。");
        Assert(!File.Exists(atomicPath + ".tmp"), "寫入完成後臨時檔 .tmp 應被正確移除。");

        // 5. 測試多任務併發 SaveAsync
        var tasks = Enumerable.Range(0, 10).Select(i => atomicStore.SaveAsync([new ScheduledJob
        {
            Name = $"併發作業-{i}",
            Message = "測試",
            ScheduledAt = DateTimeOffset.Now,
            Targets = [CliKind.Antigravity]
        }])).ToArray();
        await Task.WhenAll(tasks);
        var finalLoaded = await atomicStore.LoadAsync();
        Assert(finalLoaded.Count == 1, "併發寫入經由 Semaphore 門控保護，資料檔應維持完整可讀。");
    }
    finally
    {
        Directory.Delete(directory, true);
    }
}

static async Task TestCliRunnerAsync()
{
    var directory = CreateTemporaryDirectory();
    try
    {
        var outputPath = Path.Combine(directory, "arguments.json");
        var paths = new AppDataPaths(Path.Combine(directory, "data"));
        var runner = new CliRunner(paths);
        var profile = FakeProfile(outputPath);
        var message = "早安 & whoami";
        var result = await runner.RunAsync(
            CliKind.Antigravity,
            profile,
            message,
            directory,
            TimeSpan.FromSeconds(15));
        Assert(result.Succeeded, $"假 CLI 應成功：{result.Error}");
        Assert(File.Exists(result.LogPath), "應建立 CLI 日誌。");
        var received = JsonSerializer.Deserialize<string[]>(await File.ReadAllTextAsync(outputPath)) ?? [];
        Assert(received.Any(argument => argument.StartsWith(message, StringComparison.Ordinal)), "含 Shell 符號的訊息應完整留在單一提示參數中。");
        Assert(received.Contains("--print"), "Antigravity 應使用 --print。");

        // 測試程序執行逾時
        var timeoutProfile = FakeProfile(Path.Combine(directory, "timeout.json"), delayMilliseconds: 3000);
        var timeoutResult = await runner.RunAsync(
            CliKind.Antigravity,
            timeoutProfile,
            "逾時測試",
            directory,
            TimeSpan.FromMilliseconds(200));
        Assert(!timeoutResult.Succeeded, "逾時的 CLI 應回傳失敗。");
        Assert(timeoutResult.Error.Contains("超過"), "逾時錯誤訊息應包含超過提示。");

        // 測試程序結束碼非 0 失敗
        var errorProfile = FakeProfile(Path.Combine(directory, "error.json"), exitCode: 42);
        var errorResult = await runner.RunAsync(
            CliKind.Antigravity,
            errorProfile,
            "錯誤碼測試",
            directory,
            TimeSpan.FromSeconds(15));
        Assert(!errorResult.Succeeded, "結束碼非 0 應標示為失敗。");
        Assert(errorResult.ExitCode == 42, "應正確記錄結束碼 42。");

        // 測試無效工作目錄
        var invalidDirResult = await runner.RunAsync(
            CliKind.Antigravity,
            profile,
            "無效目錄",
            Path.Combine(directory, "nonexistent"),
            TimeSpan.FromSeconds(5));
        Assert(!invalidDirResult.Succeeded, "無效工作目錄應執行失敗。");
        Assert(invalidDirResult.Error.Contains("工作目錄不存在"), "應回傳工作目錄不存在錯誤。");

        // 測試損毀/非法的可執行檔 (觸發程序啟動失敗與錯誤日誌建立)
        var invalidExePath = Path.Combine(directory, "invalid_app.exe");
        await File.WriteAllTextAsync(invalidExePath, "NOT A VALID EXE FILE");
        var invalidExeProfile = new CliProfile { Executable = invalidExePath };
        var invalidExeResult = await runner.RunAsync(
            CliKind.Antigravity,
            invalidExeProfile,
            "無效執行檔測試",
            directory,
            TimeSpan.FromSeconds(5));
        Assert(!invalidExeResult.Succeeded, "非法的可執行檔應執行失敗。");
        Assert(invalidExeResult.LogPath.EndsWith("-error.log", StringComparison.OrdinalIgnoreCase), "執行失敗應建立 -error.log 日誌檔案。");
        Assert(File.Exists(invalidExeResult.LogPath), "錯誤日誌檔案應存在於磁碟。");
    }
    finally
    {
        Directory.Delete(directory, true);
    }
}

static async Task TestScheduleManagerAsync()
{
    var directory = CreateTemporaryDirectory();
    try
    {
        var outputPaths = Enum.GetValues<CliKind>().ToDictionary(
            kind => kind,
            kind => Path.Combine(directory, $"scheduled-{kind}-arguments.json"));
        var paths = new AppDataPaths(Path.Combine(directory, "data"));
        var jobs = new JsonFileStore<List<ScheduledJob>>(paths.JobsFile, () => []);
        var settings = AppSettings.CreateDefault();
        foreach (var kind in Enum.GetValues<CliKind>())
        {
            settings.CliProfiles[kind] = FakeProfile(outputPaths[kind], delayMilliseconds: 1000);
        }
        var runner = new CliRunner(paths);
        await jobs.SaveAsync(
        [
            new ScheduledJob
            {
                Name = "到期測試",
                ScheduledAt = DateTimeOffset.Now.AddSeconds(-1),
                Message = "早安",
                WorkingDirectory = directory,
                Targets = [CliKind.Antigravity, CliKind.AntigravityClaude, CliKind.Codex, CliKind.Claude]
            }
        ]);

        await using var manager = new ScheduleManager(jobs, runner, () => settings);
        var concurrentStopwatch = System.Diagnostics.Stopwatch.StartNew();
        await manager.InitializeAsync();
        ScheduledJob? finished = null;
        var deadline = DateTime.UtcNow.AddSeconds(20);
        while (DateTime.UtcNow < deadline)
        {
            finished = (await manager.GetJobsAsync()).Single();
            if (finished.Status == ScheduleStatus.Pending && finished.LastResults.Count == 4)
            {
                break;
            }
            await Task.Delay(100);
        }
        Assert(finished is { Status: ScheduleStatus.Pending, Enabled: true }, "舊排程應轉為每日排程並繼續等待。");
        Assert(finished?.Recurrence == ScheduleRecurrence.Daily, "舊排程載入後應轉為每日排程。");
        Assert(finished?.ScheduledAt > DateTimeOffset.Now, "每日排程執行後應排到下一個未來時分。");
        Assert(finished?.LastResults.Count == 4, "排程器應記錄四個 CLI 的結果。");
        foreach (var outputPath in outputPaths.Values)
        {
            Assert(File.Exists(outputPath), "排程器應真的啟動四個假 CLI。");
        }
        concurrentStopwatch.Stop();
        Console.WriteLine($"  concurrent CLI elapsed: {concurrentStopwatch.Elapsed.TotalMilliseconds:0} ms");
        Assert(
            concurrentStopwatch.Elapsed < TimeSpan.FromMilliseconds(2500),
            $"四個各延遲 1 秒的 CLI 應平行完成，不應循序等待；實際 {concurrentStopwatch.Elapsed.TotalMilliseconds:0} ms。");

        var recurringId = Guid.NewGuid();
        var selectedTime = DateTimeOffset.Now.AddHours(-2);
        await manager.UpsertAsync(new ScheduledJob
        {
            Id = recurringId,
            Name = "每日測試",
            ScheduledAt = selectedTime,
            Message = "早安",
            WorkingDirectory = directory,
            Targets = [CliKind.Antigravity],
            Recurrence = ScheduleRecurrence.Once
        });
        var recurring = (await manager.GetJobsAsync()).Single(job => job.Id == recurringId);
        Assert(recurring is { Status: ScheduleStatus.Pending, Enabled: true }, "儲存後應成為等待中的每日排程。");
        Assert(recurring.Recurrence == ScheduleRecurrence.Daily, "核心層應強制使用每日排程。");
        Assert(recurring.ScheduledAt > DateTimeOffset.Now, "已過的時分應自動排到明天，而不是立即執行。");
        Assert(recurring.ScheduledAt.Hour == selectedTime.LocalDateTime.Hour && recurring.ScheduledAt.Minute == selectedTime.LocalDateTime.Minute,
            "每日排程應保留使用者選擇的時與分。");
        Assert(recurring.ScheduledAt.Second == 0, "每日排程不應保存秒數。");

        var nextDailyOccurrence = recurring.ScheduledAt;
        await manager.RunNowAsync(recurringId);
        ScheduledJob? manualRun = null;
        deadline = DateTime.UtcNow.AddSeconds(20);
        while (DateTime.UtcNow < deadline)
        {
            manualRun = (await manager.GetJobsAsync()).Single(job => job.Id == recurringId);
            if (manualRun.LastResults.Count > 0 && manualRun.Status == ScheduleStatus.Pending)
            {
                break;
            }
            await Task.Delay(100);
        }
        Assert(manualRun is { Status: ScheduleStatus.Pending, Enabled: true }, "立即執行後，每日排程應回到等待狀態。");
        Assert(manualRun?.ScheduledAt == nextDailyOccurrence, "立即執行不應改變原本的每天時分或跳過下一次排程。");
    }
    finally
    {
        Directory.Delete(directory, true);
    }
}

static async Task TestScheduleManagerBoundariesAndStateAsync()
{
    var directory = CreateTemporaryDirectory();
    try
    {
        var paths = new AppDataPaths(Path.Combine(directory, "data"));
        var jobs = new JsonFileStore<List<ScheduledJob>>(paths.JobsFile, () => []);
        var settings = AppSettings.CreateDefault();
        var runner = new CliRunner(paths);

        await using var manager = new ScheduleManager(jobs, runner, () => settings);
        await manager.InitializeAsync();

        // 1. 排程名稱空白驗證
        await ThrowsAsync<ArgumentException>(() => manager.UpsertAsync(new ScheduledJob
        {
            Name = "   ",
            Message = "測試",
            WorkingDirectory = directory,
            Targets = [CliKind.Antigravity]
        }));

        // 2. 訊息空白驗證
        await ThrowsAsync<ArgumentException>(() => manager.UpsertAsync(new ScheduledJob
        {
            Name = "測試",
            Message = "",
            WorkingDirectory = directory,
            Targets = [CliKind.Antigravity]
        }));

        // 3. 訊息長度超過 50 字元驗證
        await ThrowsAsync<ArgumentException>(() => manager.UpsertAsync(new ScheduledJob
        {
            Name = "測試",
            Message = new string('A', 51),
            WorkingDirectory = directory,
            Targets = [CliKind.Antigravity]
        }));

        // 4. 訊息長度剛好 50 字元 (邊界值成功)
        var exact50JobId = Guid.NewGuid();
        await manager.UpsertAsync(new ScheduledJob
        {
            Id = exact50JobId,
            Name = "50字測試",
            Message = new string('A', 50),
            WorkingDirectory = directory,
            Targets = [CliKind.Antigravity]
        });
        var saved50 = (await manager.GetJobsAsync()).Single(j => j.Id == exact50JobId);
        Assert(saved50.Message.Length == 50, "長度 50 字元的訊息應可成功儲存。");

        // 5. CLI 目標列表為空驗證
        await ThrowsAsync<ArgumentException>(() => manager.UpsertAsync(new ScheduledJob
        {
            Name = "測試",
            Message = "早安",
            WorkingDirectory = directory,
            Targets = []
        }));

        // 6. 工作目錄不存在驗證
        await ThrowsAsync<DirectoryNotFoundException>(() => manager.UpsertAsync(new ScheduledJob
        {
            Name = "測試",
            Message = "早安",
            WorkingDirectory = Path.Combine(directory, "invalid_dir"),
            Targets = [CliKind.Antigravity]
        }));

        // 7. 找不到指定排程時 RunNowAsync
        await ThrowsAsync<InvalidOperationException>(() => manager.RunNowAsync(Guid.NewGuid()));

        // 8. 排程執行中 (Running) 時禁止修改、刪除或重複體立即執行
        var slowOutputPath = Path.Combine(directory, "slow-cli.json");
        settings.CliProfiles[CliKind.Antigravity] = FakeProfile(slowOutputPath, delayMilliseconds: 3000);
        var runningJobId = Guid.NewGuid();
        await manager.UpsertAsync(new ScheduledJob
        {
            Id = runningJobId,
            Name = "執行中保護測試",
            ScheduledAt = DateTimeOffset.Now.AddHours(1),
            Message = "早安",
            WorkingDirectory = directory,
            Targets = [CliKind.Antigravity]
        });

        // 觸發執行
        await manager.RunNowAsync(runningJobId);

        // 等待排程狀態變為 Running
        ScheduledJob? runningState = null;
        var deadline = DateTime.UtcNow.AddSeconds(5);
        while (DateTime.UtcNow < deadline)
        {
            runningState = (await manager.GetJobsAsync()).SingleOrDefault(j => j.Id == runningJobId);
            if (runningState?.Status == ScheduleStatus.Running)
            {
                break;
            }
            await Task.Delay(50);
        }
        Assert(runningState?.Status == ScheduleStatus.Running, "排程應處於 Running 狀態。");

        // 驗證 Running 狀態下的操作保護
        await ThrowsAsync<InvalidOperationException>(() => manager.RunNowAsync(runningJobId));
        await ThrowsAsync<InvalidOperationException>(() => manager.UpsertAsync(runningState!));
        await ThrowsAsync<InvalidOperationException>(() => manager.DeleteAsync(runningJobId));

        // 9. 驗證 AppSettings.QuotaAutoRefreshMinutes 預設值與 CopyTo / EnsureDefaults
        var testSettings = new AppSettings { QuotaAutoRefreshMinutes = 30 };
        var copiedSettings = testSettings.Clone();
        Assert(copiedSettings.QuotaAutoRefreshMinutes == 30, "Clone / CopyTo 應複製 QuotaAutoRefreshMinutes。");

        var invalidSettings = new AppSettings { QuotaAutoRefreshMinutes = -5 };
        invalidSettings.EnsureDefaults();
        Assert(invalidSettings.QuotaAutoRefreshMinutes == 0, "EnsureDefaults 應將負值 clamp 為 0。");
    }
    finally
    {
        Directory.Delete(directory, true);
    }
}

static async Task TestScheduleManagerAutoIntervalAsync()
{
    var directory = CreateTemporaryDirectory();
    try
    {
        var paths = new AppDataPaths(Path.Combine(directory, "data"));
        var store = new JsonFileStore<List<ScheduledJob>>(paths.JobsFile, () => []);
        var settings = AppSettings.CreateDefault();
        var runner = new CountingCliRunner();

        var autoJobId = Guid.NewGuid();
        var initialTime = DateTimeOffset.Now.AddSeconds(-1);
        await store.SaveAsync(
        [
            new ScheduledJob
            {
                Id = autoJobId,
                Name = "自動模式測試",
                ScheduledAt = initialTime,
                InitialTimeOfDay = new TimeSpan(5, 30, 0),
                Message = "早安",
                WorkingDirectory = directory,
                Targets = [CliKind.Antigravity],
                Recurrence = ScheduleRecurrence.Interval
            }
        ]);

        // 1. 測試排程器初始化並觸發首次喚醒
        await using (var manager = new ScheduleManager(store, runner, () => settings))
        {
            await manager.InitializeAsync();
            var deadline = DateTime.UtcNow.AddSeconds(10);
            while (DateTime.UtcNow < deadline && runner.CallCount == 0)
            {
                await Task.Delay(25);
            }
            Assert(runner.CallCount == 1, "首次到期應觸發一次呼叫。");

            var job = (await manager.GetJobsAsync()).Single(j => j.Id == autoJobId);
            Assert(job.Recurrence == ScheduleRecurrence.Interval, "週期應維持 Interval。");
            Assert(job.Status == ScheduleStatus.Pending, "執行後應回到 Pending 狀態。");
            Assert(job.FinishedAt.HasValue, "應記錄 FinishedAt。");
            var expectedNext = ScheduleCalculator.GetNextAutoIntervalOccurrence(job.InitialTimeOfDay!.Value, job.FinishedAt!.Value);
            var diff = (job.ScheduledAt - expectedNext).Duration();
            Assert(diff < TimeSpan.FromSeconds(2), $"執行完成後 ScheduledAt 應正確推算，預期：{expectedNext}，實際：{job.ScheduledAt}");

            // 2. 測試手動「立即執行」對自動模式之推進
            await manager.RunNowAsync(autoJobId);
            deadline = DateTime.UtcNow.AddSeconds(10);
            while (DateTime.UtcNow < deadline && runner.CallCount == 1)
            {
                await Task.Delay(25);
            }
            Assert(runner.CallCount == 2, "立即執行應再觸發一次呼叫。");

            job = (await manager.GetJobsAsync()).Single(j => j.Id == autoJobId);
            expectedNext = ScheduleCalculator.GetNextAutoIntervalOccurrence(job.InitialTimeOfDay!.Value, job.FinishedAt!.Value);
            diff = (job.ScheduledAt - expectedNext).Duration();
            Assert(diff < TimeSpan.FromSeconds(2), $"立即執行後 ScheduledAt 應重新推進，預期：{expectedNext}，實際：{job.ScheduledAt}");
        }

        // 3. 測試重啟防護：1 小時前剛跑過（未滿 5 小時 1 分鐘），重開不應重新觸發
        var recentRunner = new CountingCliRunner();
        var recentRanAt = DateTimeOffset.Now.AddHours(-1);
        var recentJobId = Guid.NewGuid();
        var initialWakeup = new TimeSpan(5, 30, 0);
        await store.SaveAsync(
        [
            new ScheduledJob
            {
                Id = recentJobId,
                Name = "近期已執行",
                ScheduledAt = ScheduleCalculator.GetNextAutoIntervalOccurrence(initialWakeup, recentRanAt),
                InitialTimeOfDay = initialWakeup,
                FinishedAt = recentRanAt,
                Message = "早安",
                WorkingDirectory = directory,
                Targets = [CliKind.Antigravity],
                Recurrence = ScheduleRecurrence.Interval
            }
        ]);

        await using (var recentManager = new ScheduleManager(store, recentRunner, () => settings))
        {
            await recentManager.InitializeAsync();
            await Task.Delay(500);

            Assert(recentRunner.CallCount == 0, "5 小時 1 分鐘未到期前重開程式不應重跑。");
            var recentJob = (await recentManager.GetJobsAsync()).Single(j => j.Id == recentJobId);
            Assert(recentJob.ScheduledAt > DateTimeOffset.Now, "排程時間應在未來的時點。");
        }

        // 4. 測試隔天逾時開機補做：昨天 23:03 跑過，今天 08:00 開機（超過 05:30），應立即補做一次
        var lateBootTime = new DateTimeOffset(2026, 8, 19, 8, 0, 0, TimeSpan.FromHours(8));
        var lateBootTimeProvider = new FakeTimeProvider(lateBootTime);
        var lateBootRunner = new CountingCliRunner();
        var yesterdayFinished = new DateTimeOffset(2026, 8, 18, 23, 3, 0, TimeSpan.FromHours(8));
        var lateBootJobId = Guid.NewGuid();
        await store.SaveAsync(
        [
            new ScheduledJob
            {
                Id = lateBootJobId,
                Name = "隔天逾時開機補做",
                ScheduledAt = new DateTimeOffset(2026, 8, 19, 5, 30, 0, TimeSpan.FromHours(8)),
                InitialTimeOfDay = initialWakeup,
                FinishedAt = yesterdayFinished,
                Message = "早安",
                WorkingDirectory = directory,
                Targets = [CliKind.Antigravity],
                Recurrence = ScheduleRecurrence.Interval
            }
        ]);

        await using (var lateBootManager = new ScheduleManager(store, lateBootRunner, () => settings, timeProvider: lateBootTimeProvider))
        {
            await lateBootManager.InitializeAsync();
            var deadline = DateTime.UtcNow.AddSeconds(10);
            while (DateTime.UtcNow < deadline && lateBootRunner.CallCount == 0)
            {
                await Task.Delay(25);
            }
            Assert(lateBootRunner.CallCount == 1, "隔日 08:00 開機（已過 05:30）應立即補做今日第一輪。");
            var lateJob = (await lateBootManager.GetJobsAsync()).Single(j => j.Id == lateBootJobId);
            Assert(lateJob.ScheduledAt.LocalDateTime == new DateTime(2026, 8, 19, 13, 1, 0), "補做完成後應推進至當日 13:01。");
        }

        // 5. 測試隔天提早開機等待：昨天 23:03 跑過，今天 04:30 開機（未到 05:30），不應提前執行
        var earlyBootTime = new DateTimeOffset(2026, 8, 19, 4, 30, 0, TimeSpan.FromHours(8));
        var earlyBootTimeProvider = new FakeTimeProvider(earlyBootTime);
        var earlyBootRunner = new CountingCliRunner();
        var earlyBootJobId = Guid.NewGuid();
        await store.SaveAsync(
        [
            new ScheduledJob
            {
                Id = earlyBootJobId,
                Name = "隔天提早開機等待",
                ScheduledAt = new DateTimeOffset(2026, 8, 19, 5, 30, 0, TimeSpan.FromHours(8)),
                InitialTimeOfDay = initialWakeup,
                FinishedAt = yesterdayFinished,
                Message = "早安",
                WorkingDirectory = directory,
                Targets = [CliKind.Antigravity],
                Recurrence = ScheduleRecurrence.Interval
            }
        ]);

        await using (var earlyBootManager = new ScheduleManager(store, earlyBootRunner, () => settings, timeProvider: earlyBootTimeProvider))
        {
            await earlyBootManager.InitializeAsync();
            await Task.Delay(500);

            Assert(earlyBootRunner.CallCount == 0, "隔日 04:30 提早開機（未到 05:30）不應提前執行。");
            var earlyJob = (await earlyBootManager.GetJobsAsync()).Single(j => j.Id == earlyBootJobId);
            Assert(earlyJob.ScheduledAt.LocalDateTime == new DateTime(2026, 8, 19, 5, 30, 0), "排程時間應排定在今日 05:30。");
        }
    }
    finally
    {
        Directory.Delete(directory, true);
    }
}

static async Task TestScheduleManagerQuotaAwareIntervalAsync()
{
    var directory = CreateTemporaryDirectory();
    try
    {
        var paths = new AppDataPaths(Path.Combine(directory, "data"));
        var store = new JsonFileStore<List<ScheduledJob>>(paths.JobsFile, () => []);
        var settings = AppSettings.CreateDefault();
        var initialWakeup = new TimeSpan(8, 0, 0);

        // 1. 當日首次時間之前（07:30 < 08:00）：即便未倒數也不提前喚醒
        var earlyNow = new DateTimeOffset(2026, 8, 21, 7, 30, 0, TimeSpan.FromHours(8));
        var earlyTimeProvider = new FakeTimeProvider(earlyNow);
        var earlyRunner = new CountingCliRunner();
        var fakeUsageReader = new FakeCliUsageReader();

        // 模擬三個 CLI：全部未在倒數中（例如 reset 在過去或 100% 額度）
        fakeUsageReader.Snapshots[CliKind.Antigravity] = new CliUsageSnapshot(
            CliKind.Antigravity,
            CliUsageAvailability.Available,
            [new CliUsageWindow("Antigravity (Gemini)", 0, TimeSpan.FromHours(5), earlyNow.AddHours(-1))],
            "讀取成功",
            earlyNow);
        fakeUsageReader.Snapshots[CliKind.AntigravityClaude] = new CliUsageSnapshot(
            CliKind.AntigravityClaude,
            CliUsageAvailability.Available,
            [new CliUsageWindow("Antigravity (Claude / GPT)", 0, TimeSpan.FromHours(5), earlyNow.AddHours(-1))],
            "讀取成功",
            earlyNow);
        fakeUsageReader.Snapshots[CliKind.Claude] = new CliUsageSnapshot(
            CliKind.Claude,
            CliUsageAvailability.Available,
            [
                new CliUsageWindow("Claude（5 小時）", 0, TimeSpan.FromHours(5), earlyNow.AddHours(-1)),
                new CliUsageWindow("Claude（7 天）", 20, TimeSpan.FromDays(7), earlyNow.AddDays(4))
            ],
            "讀取成功",
            earlyNow);

        var jobId = Guid.NewGuid();
        await store.SaveAsync(
        [
            new ScheduledJob
            {
                Id = jobId,
                Name = "流量未倒數測試",
                ScheduledAt = new DateTimeOffset(2026, 8, 21, 13, 1, 0, TimeSpan.FromHours(8)),
                InitialTimeOfDay = initialWakeup,
                FinishedAt = new DateTimeOffset(2026, 8, 20, 23, 0, 0, TimeSpan.FromHours(8)),
                Message = "早安",
                WorkingDirectory = directory,
                Targets = [CliKind.Antigravity, CliKind.AntigravityClaude, CliKind.Claude],
                Recurrence = ScheduleRecurrence.Interval
            }
        ]);

        await using (var earlyManager = new ScheduleManager(store, earlyRunner, () => settings, fakeUsageReader, earlyTimeProvider))
        {
            await earlyManager.InitializeAsync();
            await Task.Delay(400);
            Assert(earlyRunner.CallCount == 0, "當日首次喚醒時間（08:00）之前，07:30 不應觸發未倒數立即執行。");
        }

        // 2. 當日首次時間之後（08:30 > 08:00）：
        // 設定 Antigravity（Gemini）倒數中（resets at 13:30），
        // AntigravityClaude 與 Claude 未倒數（resets at 07:30，已過期）。
        // 預期：只喚醒 AntigravityClaude 與 Claude，不呼叫 Antigravity！
        var lateNow = new DateTimeOffset(2026, 8, 21, 8, 30, 0, TimeSpan.FromHours(8));
        var lateTimeProvider = new FakeTimeProvider(lateNow);
        var lateRunner = new CountingCliRunner();

        fakeUsageReader.Snapshots[CliKind.Antigravity] = new CliUsageSnapshot(
            CliKind.Antigravity,
            CliUsageAvailability.Available,
            [new CliUsageWindow("Antigravity (Gemini)", 10, TimeSpan.FromHours(5), lateNow.AddHours(5), IsActiveCountdown: true)],
            "讀取成功",
            lateNow); // 倒數中！
        fakeUsageReader.Snapshots[CliKind.AntigravityClaude] = new CliUsageSnapshot(
            CliKind.AntigravityClaude,
            CliUsageAvailability.Available,
            [new CliUsageWindow("Antigravity (Claude / GPT)", 0, TimeSpan.FromHours(5), lateNow.AddHours(-1), IsActiveCountdown: false)],
            "讀取成功",
            lateNow); // 未倒數！
        fakeUsageReader.Snapshots[CliKind.Claude] = new CliUsageSnapshot(
            CliKind.Claude,
            CliUsageAvailability.Available,
            [
                new CliUsageWindow("Claude（5 小時）", 0, TimeSpan.FromHours(5), lateNow.AddMinutes(-30), IsActiveCountdown: false), // 未倒數！
                new CliUsageWindow("Claude（7 天）", 30, TimeSpan.FromDays(7), lateNow.AddDays(3), IsActiveCountdown: true)
            ],
            "讀取成功",
            lateNow);

        await using (var lateManager = new ScheduleManager(store, lateRunner, () => settings, fakeUsageReader, lateTimeProvider))
        {
            await lateManager.InitializeAsync();
            var deadline = DateTime.UtcNow.AddSeconds(10);
            while (DateTime.UtcNow < deadline && lateRunner.CallCount < 2)
            {
                await Task.Delay(25);
            }

            Assert(lateRunner.CallCount == 2, $"當日首次時間後，應只呼叫未倒數的 2 個 CLI，實際呼叫 {lateRunner.CallCount} 次。");
            lock (lateRunner.ExecutedClis)
            {
                Assert(!lateRunner.ExecutedClis.Contains(CliKind.Antigravity), "處於倒數中的 Antigravity 不應被呼叫。");
                Assert(lateRunner.ExecutedClis.Contains(CliKind.AntigravityClaude), "未倒數的 AntigravityClaude 應被呼叫。");
                Assert(lateRunner.ExecutedClis.Contains(CliKind.Claude), "未倒數的 Claude 應被呼叫。");
            }
        }

        // 3. 背景排程迴圈即使被頻繁喚醒，未倒數時也只能依使用者設定的分鐘間隔查一次額度。
        // 這是 Claude /api/oauth/usage 不再因每 30 秒重查而觸發 HTTP 429 的迴歸測試。
        settings.QuotaAutoRefreshMinutes = 10;
        var throttledNow = new DateTimeOffset(2026, 8, 21, 9, 0, 0, TimeSpan.FromHours(8));
        var throttledTimeProvider = new FakeTimeProvider(throttledNow);
        var throttledUsageReader = new FakeCliUsageReader();
        // 模擬未抓到倒數（例如 100% 額度或未在倒數中）
        throttledUsageReader.Snapshots[CliKind.Claude] = new CliUsageSnapshot(
            CliKind.Claude,
            CliUsageAvailability.Available,
            [new CliUsageWindow("Claude（5 小時）", 0, TimeSpan.FromHours(5), throttledNow.AddHours(-1), IsActiveCountdown: false)],
            "讀取成功",
            throttledNow);

        var throttledJobId = Guid.NewGuid();
        await store.SaveAsync(
        [
            new ScheduledJob
            {
                Id = throttledJobId,
                Name = "額度探測節流測試",
                ScheduledAt = throttledNow.AddHours(4),
                InitialTimeOfDay = initialWakeup,
                FinishedAt = throttledNow.AddHours(-1),
                Message = "早安",
                WorkingDirectory = directory,
                Targets = [CliKind.Claude],
                Recurrence = ScheduleRecurrence.Interval
            }
        ]);

        await using (var throttledManager = new ScheduleManager(
            store,
            new CountingCliRunner(),
            () => settings,
            throttledUsageReader,
            throttledTimeProvider))
        {
            await throttledManager.InitializeAsync();
            var firstProbeDeadline = DateTime.UtcNow.AddSeconds(3);
            while (DateTime.UtcNow < firstProbeDeadline && throttledUsageReader.GetReadCount(CliKind.Claude) < 2)
            {
                await Task.Delay(25);
            }
            Assert(throttledUsageReader.GetReadCount(CliKind.Claude) == 2, "啟動探測與執行後捕捉新倒數應各讀取一次 Claude 額度。");

            var throttledJob = (await throttledManager.GetJobsAsync()).Single();
            for (var i = 0; i < 3; i++)
            {
                await throttledManager.UpsertAsync(throttledJob);
                await Task.Delay(100);
            }
            Assert(throttledUsageReader.GetReadCount(CliKind.Claude) == 2,
                "10 分鐘探測間隔內即使排程器被喚醒多次，也不應重查 Claude 額度。");

            // 推進 11 分鐘，因為仍處於未倒數狀態，應觸發定期探測
            throttledTimeProvider.SetUtcNow(throttledNow.AddMinutes(11));
            await throttledManager.UpsertAsync(throttledJob);
            var secondProbeDeadline = DateTime.UtcNow.AddSeconds(3);
            while (DateTime.UtcNow < secondProbeDeadline && throttledUsageReader.GetReadCount(CliKind.Claude) < 3)
            {
                await Task.Delay(25);
            }
            Assert(throttledUsageReader.GetReadCount(CliKind.Claude) >= 3,
                "超過使用者設定的 10 分鐘後，應允許下一次 Claude 額度探測。");
        }

        // 4. 倒數結束時（Countdown Ended）自動喚醒並探測
        var countdownEndNow = new DateTimeOffset(2026, 8, 21, 9, 0, 0, TimeSpan.FromHours(8));
        var countdownEndTimeProvider = new FakeTimeProvider(countdownEndNow);
        var countdownEndReader = new FakeCliUsageReader();
        var countdownEndRunner = new CountingCliRunner();

        // 模擬 Claude 倒數即將在 15 分鐘後（09:15）結束
        var resetPoint = countdownEndNow.AddMinutes(15);
        countdownEndReader.Snapshots[CliKind.Claude] = new CliUsageSnapshot(
            CliKind.Claude,
            CliUsageAvailability.Available,
            [new CliUsageWindow("Claude（5 小時）", 10, TimeSpan.FromHours(5), resetPoint, IsActiveCountdown: true)],
            "讀取成功",
            countdownEndNow);

        var countdownJobId = Guid.NewGuid();
        await store.SaveAsync(
        [
            new ScheduledJob
            {
                Id = countdownJobId,
                Name = "倒數結束自動喚醒測試",
                ScheduledAt = resetPoint,
                InitialTimeOfDay = initialWakeup,
                FinishedAt = countdownEndNow.AddHours(-1),
                Message = "早安",
                WorkingDirectory = directory,
                Targets = [CliKind.Claude],
                Recurrence = ScheduleRecurrence.Interval
            }
        ]);

        await using (var countdownManager = new ScheduleManager(
            store,
            countdownEndRunner,
            () => settings,
            countdownEndReader,
            countdownEndTimeProvider))
        {
            await countdownManager.InitializeAsync();
            var initDeadline = DateTime.UtcNow.AddSeconds(3);
            while (DateTime.UtcNow < initDeadline && countdownEndReader.GetReadCount(CliKind.Claude) < 1)
            {
                await Task.Delay(25);
            }
            Assert(countdownEndReader.GetReadCount(CliKind.Claude) == 1, "啟動初始化探測。");

            // 時間推進至倒數剛結束（09:16），且伺服器已更新為未倒數（額度已重置）
            countdownEndReader.Snapshots[CliKind.Claude] = new CliUsageSnapshot(
                CliKind.Claude,
                CliUsageAvailability.Available,
                [new CliUsageWindow("Claude（5 小時）", 0, TimeSpan.FromHours(5), resetPoint, IsActiveCountdown: false)],
                "讀取成功",
                countdownEndNow.AddMinutes(16));

            countdownEndTimeProvider.SetUtcNow(countdownEndNow.AddMinutes(16));
            // 喚醒排程器評估
            var countdownJob = (await countdownManager.GetJobsAsync()).Single();
            await countdownManager.UpsertAsync(countdownJob);

            var wakeDeadline = DateTime.UtcNow.AddSeconds(5);
            while (DateTime.UtcNow < wakeDeadline && countdownEndRunner.CallCount < 1)
            {
                await Task.Delay(25);
            }
            Assert(countdownEndRunner.CallCount == 1, "倒數結束後，排程器應自動探測並喚醒已重置的 Claude。");
            Assert(countdownEndReader.GetReadCount(CliKind.Claude) >= 2, "倒數結束與執行前/後應觸發額度讀取。");
        }
    }
    finally
    {
        Directory.Delete(directory, true);
    }
}

static async Task TestExecutableLocatorAsync()
{
    var workingDir = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);

    // 1. 測試解析本機已安裝的 CLI（四者均應成功解析）
    var agy = ExecutableLocator.Resolve(CliKind.Antigravity, "agy", workingDir);
    Assert(agy is not null && File.Exists(agy), $"Antigravity CLI 應成功解析到可執行檔：{agy}");

    var agyClaude = ExecutableLocator.Resolve(CliKind.AntigravityClaude, "agy", workingDir);
    Assert(agyClaude is not null && File.Exists(agyClaude), $"Antigravity Claude/GPT CLI 應成功解析到可執行檔：{agyClaude}");

    var codex = ExecutableLocator.Resolve(CliKind.Codex, "codex", workingDir);
    Assert(codex is not null && File.Exists(codex), $"Codex CLI 應成功解析到子目錄或已知路徑中的可執行檔：{codex}");

    var claude = ExecutableLocator.Resolve(CliKind.Claude, "claude", workingDir);
    Assert(claude is not null && File.Exists(claude), $"Claude CLI 應成功解析到可執行檔：{claude}");

    // 2. 測試明確絕對路徑
    var explicitFound = ExecutableLocator.Resolve(CliKind.Codex, codex, workingDir);
    Assert(explicitFound == codex, "傳入明確存在的絕對路徑應直接解析。");

    var tempDir = CreateTemporaryDirectory();
    try
    {
        // 3. 測試相對路徑
        var dummyExe = Path.Combine(tempDir, "custom_app.exe");
        await File.WriteAllTextAsync(dummyExe, "dummy");
        var relative = ExecutableLocator.Resolve(CliKind.Codex, "custom_app.exe", tempDir);
        Assert(relative is not null && Path.GetFullPath(relative) == dummyExe, "傳入工作目錄中的相對路徑應成功解析。");

        // 4. 測試真實 CLI ProbeAsync（四者均應回傳 Succeeded == true）
        var paths = new AppDataPaths(Path.Combine(tempDir, "data"));
        var runner = new CliRunner(paths);

        var probeAgy = await runner.ProbeAsync(CliKind.Antigravity, new CliProfile { Executable = "agy" }, workingDir);
        Assert(probeAgy.Succeeded, $"Antigravity Probe 應成功：{probeAgy.Summary}");

        var probeAgyClaude = await runner.ProbeAsync(CliKind.AntigravityClaude, new CliProfile { Executable = "agy" }, workingDir);
        Assert(probeAgyClaude.Succeeded, $"Antigravity Claude Probe 應成功：{probeAgyClaude.Summary}");

        var probeCodex = await runner.ProbeAsync(CliKind.Codex, new CliProfile { Executable = "codex" }, workingDir);
        Assert(probeCodex.Succeeded, $"Codex Probe 應成功：{probeCodex.Summary}");

        // 直接走產品用的 app-server 整合，確認目前登入帳戶真的能回傳額度視窗。
        // account/rateLimits/read 是唯讀查詢，不會建立模型回合或消耗 Token。
        var codexUsage = await new CliUsageReader().ReadAsync(
            CliKind.Codex,
            new CliProfile { Executable = "codex" },
            workingDir);
        Assert(codexUsage.Availability == CliUsageAvailability.Available,
            $"Codex 額度應可由 app-server 讀取：{codexUsage.Message}");
        Assert(codexUsage.Windows.Count > 0, "Codex 額度回應應至少有一個視窗。");
        Assert(codexUsage.Windows.All(window => window.RemainingPercent is >= 0 and <= 100),
            "Codex 剩餘百分比應落在 0 到 100。");

        // 測試 Antigravity (Gemini 與 Claude/GPT) 即時額度讀取
        var agyUsage = await new CliUsageReader().ReadAsync(
            CliKind.Antigravity,
            new CliProfile { Executable = "agy" },
            workingDir);
        Console.WriteLine($"  Antigravity (Gemini) Usage: Availability={agyUsage.Availability}, Windows={agyUsage.Windows.Count}, Message={agyUsage.Message}");
        if (agyUsage.Availability == CliUsageAvailability.Available)
        {
            Assert(agyUsage.Windows.Count == 1, "Antigravity (Gemini) 應回傳 1 個額度視窗。");
            Assert(agyUsage.Windows[0].RemainingPercent is >= 0 and <= 100, "Gemini 剩餘百分比應落在 0 到 100。");
            Console.WriteLine($"  -> Gemini: 剩餘 {agyUsage.Windows[0].RemainingPercent}%, 重置時間: {agyUsage.Windows[0].ResetsAt?.LocalDateTime}");
        }

        var agyClaudeUsage = await new CliUsageReader().ReadAsync(
            CliKind.AntigravityClaude,
            new CliProfile { Executable = "agy" },
            workingDir);
        Console.WriteLine($"  Antigravity (Claude / GPT) Usage: Availability={agyClaudeUsage.Availability}, Windows={agyClaudeUsage.Windows.Count}, Message={agyClaudeUsage.Message}");
        if (agyClaudeUsage.Availability == CliUsageAvailability.Available)
        {
            Assert(agyClaudeUsage.Windows.Count == 1, "Antigravity (Claude / GPT) 應回傳 1 個額度視窗。");
            Assert(agyClaudeUsage.Windows[0].RemainingPercent is >= 0 and <= 100, "Claude/GPT 剩餘百分比應落在 0 到 100。");
            Console.WriteLine($"  -> Claude/GPT: 剩餘 {agyClaudeUsage.Windows[0].RemainingPercent}%, 重置時間: {agyClaudeUsage.Windows[0].ResetsAt?.LocalDateTime}");
        }

        var probeClaude = await runner.ProbeAsync(CliKind.Claude, new CliProfile { Executable = "claude" }, workingDir);
        Assert(probeClaude.Succeeded, $"Claude Probe 應成功：{probeClaude.Summary}");
    }
    finally
    {
        Directory.Delete(tempDir, true);
    }
}

static CliProfile FakeProfile(string outputPath, int delayMilliseconds = 0, int exitCode = 0) => new()
{
    Executable = FindTestAppHost(),
    AdditionalArguments = $"--fake-cli --fake-delay-ms {delayMilliseconds} --fake-exit-code {exitCode} --fake-cli-output \"{outputPath}\""
};

static string FindTestAppHost()
{
    var assembly = Assembly.GetExecutingAssembly().Location;
    var appHost = Path.ChangeExtension(assembly, OperatingSystem.IsWindows() ? ".exe" : null);
    if (File.Exists(appHost)) return appHost;
    throw new FileNotFoundException("找不到測試 apphost。", appHost);
}

static string CreateTemporaryDirectory()
{
    var path = Path.Combine(Path.GetTempPath(), "AiWakeSchedulerTests", Guid.NewGuid().ToString("N"));
    Directory.CreateDirectory(path);
    return path;
}

static void Equal<T>(IEnumerable<T> expected, IEnumerable<T> actual)
{
    if (!expected.SequenceEqual(actual))
    {
        throw new InvalidOperationException($"序列不同。預期：[{string.Join(", ", expected)}]，實際：[{string.Join(", ", actual)}]");
    }
}

static void Assert(bool condition, string message)
{
    if (!condition) throw new InvalidOperationException(message);
}

static void Throws<TException>(Action action) where TException : Exception
{
    try
    {
        action();
    }
    catch (TException)
    {
        return;
    }
    catch (Exception ex)
    {
        throw new InvalidOperationException($"預期擲出 {typeof(TException).Name}，實際擲出 {ex.GetType().Name}: {ex.Message}");
    }
    throw new InvalidOperationException($"預期擲出 {typeof(TException).Name}。");
}

static async Task ThrowsAsync<TException>(Func<Task> action) where TException : Exception
{
    try
    {
        await action();
    }
    catch (TException)
    {
        return;
    }
    catch (Exception ex)
    {
        throw new InvalidOperationException($"預期擲出 {typeof(TException).Name}，實際擲出 {ex.GetType().Name}: {ex.Message}");
    }
    throw new InvalidOperationException($"預期擲出 {typeof(TException).Name}。");
}

/// <summary>
/// 假的 CLI 執行器。ScheduleManager 只依賴 ICliRunner，
/// 因此排程邏輯可以完全不啟動任何子程序就驗證。
/// </summary>
internal sealed class CountingCliRunner : ICliRunner
{
    private int _callCount;
    public readonly List<CliKind> ExecutedClis = [];

    public int CallCount => Volatile.Read(ref _callCount);

    public bool LastTokenSaverMode { get; private set; }

    public Task<CliRunResult> RunAsync(
        CliKind kind,
        CliProfile profile,
        string message,
        string workingDirectory,
        TimeSpan timeout,
        bool tokenSaverMode = true,
        CancellationToken cancellationToken = default)
    {
        lock (ExecutedClis)
        {
            ExecutedClis.Add(kind);
        }
        Interlocked.Increment(ref _callCount);
        LastTokenSaverMode = tokenSaverMode;
        return Task.FromResult(new CliRunResult
        {
            Cli = kind,
            Succeeded = true,
            ExitCode = 0,
            StartedAt = DateTimeOffset.Now,
            FinishedAt = DateTimeOffset.Now
        });
    }

    public Task<CliProbeResult> ProbeAsync(
        CliKind kind,
        CliProfile profile,
        string workingDirectory,
        CancellationToken cancellationToken = default) =>
        Task.FromResult(new CliProbeResult { Cli = kind, Succeeded = true, Summary = "fake" });
}

internal sealed class FakeCliUsageReader : ICliUsageReader
{
    public Dictionary<CliKind, CliUsageSnapshot> Snapshots { get; } = [];
    private readonly Dictionary<CliKind, CliUsageSnapshot> _latestSnapshots = [];
    private readonly Dictionary<CliKind, int> _readCounts = [];

    public int GetReadCount(CliKind kind)
    {
        lock (_readCounts)
        {
            return _readCounts.GetValueOrDefault(kind);
        }
    }

    public CliUsageSnapshot? GetLatestSnapshot(CliKind kind)
    {
        lock (_latestSnapshots)
        {
            return _latestSnapshots.GetValueOrDefault(kind);
        }
    }

    public IReadOnlyDictionary<CliKind, CliUsageSnapshot> GetLatestSnapshots()
    {
        lock (_latestSnapshots)
        {
            return new Dictionary<CliKind, CliUsageSnapshot>(_latestSnapshots);
        }
    }

    public Task<CliUsageSnapshot> ReadAsync(
        CliKind kind,
        CliProfile profile,
        string workingDirectory,
        CancellationToken cancellationToken = default)
    {
        lock (_readCounts)
        {
            _readCounts[kind] = _readCounts.GetValueOrDefault(kind) + 1;
        }

        CliUsageSnapshot result;
        if (Snapshots.TryGetValue(kind, out var snapshot))
        {
            result = snapshot;
        }
        else
        {
            result = new CliUsageSnapshot(
                kind,
                CliUsageAvailability.Unavailable,
                [],
                "No fake data",
                DateTimeOffset.Now);
        }

        lock (_latestSnapshots)
        {
            _latestSnapshots[kind] = result;
        }

        return Task.FromResult(result);
    }
}

internal sealed class FakeTimeProvider : TimeProvider
{
    private DateTimeOffset _now;

    public FakeTimeProvider(DateTimeOffset initial) => _now = initial;

    public void SetUtcNow(DateTimeOffset now) => _now = now;

    public override DateTimeOffset GetUtcNow() => _now.ToUniversalTime();

    public override TimeZoneInfo LocalTimeZone => TimeZoneInfo.Local;
}
