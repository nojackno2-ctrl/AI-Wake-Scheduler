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

var tests = new (string Name, Func<Task> Run)[]
{
    ("ArgumentTokenizer", TestArgumentTokenizerAsync),
    ("CliCommandBuilder", TestCliCommandBuilderAsync),
    ("ScheduleCalculator", TestScheduleCalculatorAsync),
    ("JsonFileStore", TestJsonFileStoreAsync),
    ("CliRunnerSafeArguments", TestCliRunnerAsync),
    ("ScheduleManagerDueJob", TestScheduleManagerAsync),
    ("ScheduleManagerBoundariesAndState", TestScheduleManagerBoundariesAndStateAsync)
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
    Equal(Array.Empty<string>(), ArgumentTokenizer.Parse(""));
    Equal(Array.Empty<string>(), ArgumentTokenizer.Parse("   \t "));
    Equal(["a", "b", "c"], ArgumentTokenizer.Parse("a   b \t c"));
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
            if (finished.Status == ScheduleStatus.Pending && finished.LastResults.Count == 3)
            {
                break;
            }
            await Task.Delay(100);
        }
        Assert(finished is { Status: ScheduleStatus.Pending, Enabled: true }, "舊排程應轉為每日排程並繼續等待。");
        Assert(finished?.Recurrence == ScheduleRecurrence.Daily, "舊排程載入後應轉為每日排程。");
        Assert(finished?.ScheduledAt > DateTimeOffset.Now, "每日排程執行後應排到下一個未來時分。");
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
    }
    finally
    {
        Directory.Delete(directory, true);
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
