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
    ("CliCatalog", TestCliCatalogAsync),
    ("CliCommandBuilder", TestCliCommandBuilderAsync),
    ("ScheduleCalculator", TestScheduleCalculatorAsync),
    ("JsonFileStore", TestJsonFileStoreAsync),
    ("CliRunnerSafeArguments", TestCliRunnerAsync),
    ("ScheduleManagerDueJob", TestScheduleManagerAsync),
    ("ScheduleManagerAdaptiveWait", TestScheduleManagerAdaptiveWaitAsync),
    ("ScheduleManagerRestartDoesNotRefire", TestScheduleManagerRestartDoesNotRefireAsync),
    ("ScheduleManagerBoundariesAndState", TestScheduleManagerBoundariesAndStateAsync),
    ("ExecutableLocatorResolution", TestExecutableLocatorAsync)
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
    // Antigravity 的 --print 是「值就是提示詞」的字串旗標，必須排在所有旗標的最後
    Equal(["--print", "早安"], CliCommandBuilder.Build(CliKind.Antigravity, "早安", tokenSaverMode: false));
    Equal(["--model", "claude-sonnet-4-6", "--print", "早安"], CliCommandBuilder.Build(CliKind.AntigravityClaude, "早安", tokenSaverMode: false));
    Equal(["--model", "custom-model", "--print", "早安"], CliCommandBuilder.Build(CliKind.AntigravityClaude, "早安", "--model custom-model", tokenSaverMode: false));
    Equal(["exec", "--skip-git-repo-check", "--ephemeral", "--color", "never", "早安"], CliCommandBuilder.Build(CliKind.Codex, "早安", tokenSaverMode: false));
    Equal(["--print", "--model", "sonnet", "早安"], CliCommandBuilder.Build(CliKind.Claude, "早安", "--model sonnet", tokenSaverMode: false));

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
    Assert(!shortModelOverride.Contains("Claude Sonnet 4.6 (Thinking)"), "使用者以 -m 指定模型時不應覆寫。");

    var saverAgy = CliCommandBuilder.Build(CliKind.Antigravity, "早安", timeout: TimeSpan.FromMinutes(3));
    Assert(saverAgy.Contains("--effort") && saverAgy.Contains("low"), "Antigravity 節省模式應包含 low effort。");
    Assert(saverAgy.Contains("--disable-slash-commands"), "Antigravity 節省模式應停用斜線指令。");
    Assert(saverAgy.Contains("--print-timeout") && saverAgy.Contains("180s"), "應把應用程式逾時轉為 CLI 自身的 --print-timeout。");

    var saverAgyClaude = CliCommandBuilder.Build(CliKind.AntigravityClaude, "早安");
    Assert(saverAgyClaude.Contains("--model") && saverAgyClaude.Contains("claude-sonnet-4-6"), "AntigravityClaude 預設應使用 Claude Sonnet 模型 ID。");
    Assert(saverAgyClaude.Contains("--disable-slash-commands"), "AntigravityClaude 節省模式應停用技能展開。");
    // Claude 系列模型不接受 --effort，帶上去 agy 會直接拒絕整個呼叫
    Assert(!saverAgyClaude.Contains("--effort"), "AntigravityClaude 不可帶 --effort，Claude 模型不支援。");
    Assert(!saverAgyClaude.Contains("--print-timeout"), "未指定逾時時不應產生 --print-timeout。");

    var saverCodex = CliCommandBuilder.Build(CliKind.Codex, "早安");
    Assert(saverCodex.Contains("read-only"), "節省 Token 模式應使用 Codex 唯讀沙箱。");
    Assert(saverCodex.Contains("model_reasoning_effort=\"low\""), "節省 Token 模式應降低 Codex 推理量。");
    Assert(saverCodex.Contains("--ignore-user-config"), "節省 Token 模式應略過 config.toml，避免載入 MCP 伺服器與自訂指示。");
    Assert(saverCodex.Last().Contains("只回「OK」", StringComparison.Ordinal), "節省模式應要求最短回覆。");

    var saverClaude = CliCommandBuilder.Build(CliKind.Claude, "早安");
    Assert(saverClaude.Contains("--tools") && saverClaude.Contains(string.Empty), "節省模式應停用 Claude 內建工具。");
    Assert(saverClaude.Contains("--safe-mode"), "節省模式應停用 CLAUDE.md、技能、外掛與 MCP 伺服器。");
    Assert(saverClaude.Contains("--strict-mcp-config"), "節省模式應確保不載入任何 MCP 工具結構描述。");

    // --tools 是可變長度參數，後面必須緊接旗標，否則會吃掉提示詞
    var toolsIndex = saverClaude.ToList().IndexOf("--tools");
    Assert(toolsIndex >= 0 && saverClaude[toolsIndex + 1].Length == 0, "--tools 之後應緊接空字串。");
    Assert(saverClaude[toolsIndex + 2].StartsWith("--", StringComparison.Ordinal), "--tools 的值之後必須是旗標，避免吞掉位置參數。");

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
