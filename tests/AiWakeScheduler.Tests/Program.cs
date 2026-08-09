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
    return 0;
}

var tests = new (string Name, Func<Task> Run)[]
{
    ("ArgumentTokenizer", TestArgumentTokenizerAsync),
    ("CliCommandBuilder", TestCliCommandBuilderAsync),
    ("ScheduleCalculator", TestScheduleCalculatorAsync),
    ("JsonFileStore", TestJsonFileStoreAsync),
    ("CliRunnerSafeArguments", TestCliRunnerAsync),
    ("ScheduleManagerDueJob", TestScheduleManagerAsync)
};

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
    Throws<FormatException>(() => ArgumentTokenizer.Parse("\"unfinished"));
    return Task.CompletedTask;
}

static Task TestCliCommandBuilderAsync()
{
    Equal(["--print", "早安"], CliCommandBuilder.Build(CliKind.Antigravity, "早安", tokenSaverMode: false));
    Equal(["exec", "--skip-git-repo-check", "早安"], CliCommandBuilder.Build(CliKind.Codex, "早安", tokenSaverMode: false));
    Equal(["--print", "--model", "sonnet", "早安"], CliCommandBuilder.Build(CliKind.Claude, "早安", "--model sonnet", tokenSaverMode: false));

    var saverCodex = CliCommandBuilder.Build(CliKind.Codex, "早安");
    Assert(saverCodex.Contains("read-only"), "節省 Token 模式應使用 Codex 唯讀沙箱。");
    Assert(saverCodex.Contains("model_reasoning_effort=\"low\""), "節省 Token 模式應降低 Codex 推理量。");
    Assert(saverCodex.Last().Contains("只回覆上面這句", StringComparison.Ordinal), "節省模式應要求短回覆。");

    var saverClaude = CliCommandBuilder.Build(CliKind.Claude, "早安");
    Assert(saverClaude.Contains("--tools") && saverClaude.Contains(string.Empty), "節省模式應停用 Claude 工具。");
    return Task.CompletedTask;
}

static Task TestScheduleCalculatorAsync()
{
    var now = new DateTimeOffset(2026, 8, 10, 12, 0, 0, TimeSpan.FromHours(8));
    var old = new DateTimeOffset(2026, 8, 8, 8, 30, 0, TimeSpan.FromHours(8));
    var daily = ScheduleCalculator.GetNextOccurrence(old, ScheduleRecurrence.Daily, now);
    Assert(daily.LocalDateTime == new DateTime(2026, 8, 11, 8, 30, 0), "每日排程應跳到目前時間之後並保留本地時刻。");

    var weekly = ScheduleCalculator.GetNextOccurrence(old, ScheduleRecurrence.Weekly, now);
    Assert(weekly.LocalDateTime == new DateTime(2026, 8, 15, 8, 30, 0), "每週排程應增加七天。");
    Throws<ArgumentException>(() => ScheduleCalculator.GetNextOccurrence(old, ScheduleRecurrence.Once, now));
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
        Assert(received.Contains("--print"), "Antigravity 應使用 --print。 ");
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
                Targets = [CliKind.Antigravity, CliKind.Codex, CliKind.Claude]
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
            if (finished.Status is ScheduleStatus.Completed or ScheduleStatus.Failed)
            {
                break;
            }
            await Task.Delay(100);
        }
        Assert(finished?.Status == ScheduleStatus.Completed, $"到期排程應成功，實際狀態：{finished?.Status}");
        Assert(finished is { Enabled: false }, "一次性排程成功後應停用。");
        Assert(finished?.LastResults.Count == 3, "排程器應記錄三個 CLI 的結果。");
        foreach (var outputPath in outputPaths.Values)
        {
            Assert(File.Exists(outputPath), "排程器應真的啟動三個假 CLI。");
        }
        concurrentStopwatch.Stop();
        Console.WriteLine($"  concurrent CLI elapsed: {concurrentStopwatch.Elapsed.TotalMilliseconds:0} ms");
        Assert(
            concurrentStopwatch.Elapsed < TimeSpan.FromMilliseconds(2500),
            $"三個各延遲 1 秒的 CLI 應平行完成，不應循序等待；實際 {concurrentStopwatch.Elapsed.TotalMilliseconds:0} ms。");

        var recurringId = Guid.NewGuid();
        await manager.UpsertAsync(new ScheduledJob
        {
            Id = recurringId,
            Name = "每日測試",
            ScheduledAt = DateTimeOffset.Now.AddDays(-2),
            Message = "早安",
            WorkingDirectory = directory,
            Targets = [CliKind.Antigravity],
            Recurrence = ScheduleRecurrence.Daily
        });
        ScheduledJob? recurring = null;
        deadline = DateTime.UtcNow.AddSeconds(20);
        while (DateTime.UtcNow < deadline)
        {
            recurring = (await manager.GetJobsAsync()).Single(job => job.Id == recurringId);
            if (recurring.LastResults.Count > 0 && recurring.ScheduledAt > DateTimeOffset.Now)
            {
                break;
            }
            await Task.Delay(100);
        }
        Assert(recurring is { Status: ScheduleStatus.Pending, Enabled: true }, "每日排程執行後應回到等待狀態。");
        var verifiedRecurring = recurring ?? throw new InvalidOperationException("找不到每日排程結果。");
        Assert(verifiedRecurring.ScheduledAt > DateTimeOffset.Now, "每日排程下一次時間必須在未來。");
    }
    finally
    {
        Directory.Delete(directory, true);
    }
}

static CliProfile FakeProfile(string outputPath, int delayMilliseconds = 0) => new()
{
    Executable = FindTestAppHost(),
    AdditionalArguments = $"--fake-cli --fake-delay-ms {delayMilliseconds} --fake-cli-output \"{outputPath}\""
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
    throw new InvalidOperationException($"預期擲出 {typeof(TException).Name}。");
}
