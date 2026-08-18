using System.Collections.Concurrent;

namespace AiWakeScheduler.Core;

/// <summary>
/// 管理 AI 喚醒排程的初始化、掃描、手動觸發與背景執行。
///
/// 只依賴 <see cref="IDataStore{T}"/>、<see cref="ICliRunner"/> 與 <see cref="TimeProvider"/>，
/// 不知道資料存在哪裡、CLI 怎麼啟動，也不知道現在幾點。
/// </summary>
public sealed class ScheduleManager : IAsyncDisposable
{
    /// <summary>
    /// 閒置時最長的等待間隔。
    /// 排程器不再每秒輪詢，而是直接睡到下一個到期時間；這個上限只是為了
    /// 在系統休眠、時鐘調整或時區變更後仍能在可接受的時間內重新校準。
    /// </summary>
    private static readonly TimeSpan MaxIdleWait = TimeSpan.FromSeconds(30);

    /// <summary>最短等待間隔，避免時間已到但狀態尚未落地時空轉。</summary>
    private static readonly TimeSpan MinWait = TimeSpan.FromMilliseconds(200);

    private readonly IDataStore<List<ScheduledJob>> _jobStore;
    private readonly ICliRunner _cliRunner;
    private readonly Func<AppSettings> _settingsProvider;
    private readonly TimeProvider _time;

    private readonly SemaphoreSlim _gate = new(1, 1);
    /// <summary>排程異動時用來立刻喚醒背景迴圈，不必等到下一個間隔。</summary>
    private readonly SemaphoreSlim _wakeSignal = new(0);
    private readonly ConcurrentDictionary<Guid, Task> _executions = new();
    private readonly CancellationTokenSource _stopSource = new();

    private List<ScheduledJob> _jobs = [];
    private Task? _schedulerLoop;
    private bool _disposed;

    public ScheduleManager(
        IDataStore<List<ScheduledJob>> jobStore,
        ICliRunner cliRunner,
        Func<AppSettings> settingsProvider,
        TimeProvider? timeProvider = null)
    {
        _jobStore = jobStore;
        _cliRunner = cliRunner;
        _settingsProvider = settingsProvider;
        _time = timeProvider ?? TimeProvider.System;
    }

    public event EventHandler? JobsChanged;
    public event EventHandler<Exception>? BackgroundError;

    private DateTimeOffset Now => _time.GetLocalNow();

    public async Task InitializeAsync(CancellationToken cancellationToken = default)
    {
        ObjectDisposedException.ThrowIf(_disposed, this);
        var loaded = await _jobStore.LoadAsync(cancellationToken).ConfigureAwait(false);
        var now = Now;

        foreach (var job in loaded)
        {
            if (job.Recurrence is ScheduleRecurrence.Once or ScheduleRecurrence.Weekly)
            {
                // 舊版本允許單次與每週排程。保留使用者選的時分，
                // 但一律遷移為本程式的每日 HH:mm 契約。
                job.Recurrence = ScheduleRecurrence.Daily;
            }

            if (job.Recurrence == ScheduleRecurrence.Interval)
            {
                var initialTime = job.InitialTimeOfDay ?? job.ScheduledAt.LocalDateTime.TimeOfDay;
                job.InitialTimeOfDay = initialTime;

                if (job.FinishedAt is { } finished)
                {
                    var localFinished = finished.LocalDateTime;
                    var localNow = now.LocalDateTime;

                    if (localFinished.Date < localNow.Date)
                    {
                        // 上次執行在今日以前（昨天或更早）
                        // 今日的首次喚醒基準時間
                        var todayInitial = ScheduleCalculator.GetPreviousDailyOccurrence(initialTime, now);
                        var alreadyRanToday = finished >= todayInitial;
                        job.ScheduledAt = alreadyRanToday
                            ? ScheduleCalculator.GetNextDailyOccurrence(initialTime, now)
                            : todayInitial;
                    }
                    else
                    {
                        // 上次執行在今天之內
                        var nextOccurrence = ScheduleCalculator.GetNextAutoIntervalOccurrence(initialTime, finished);
                        job.ScheduledAt = nextOccurrence;
                    }
                }
                else
                {
                    // 從未執行過：設定為今日首次喚醒
                    var lastOccurrence = ScheduleCalculator.GetPreviousDailyOccurrence(initialTime, now);
                    job.ScheduledAt = lastOccurrence;
                }
            }
            else
            {
                var timeOfDay = job.ScheduledAt.LocalDateTime.TimeOfDay;
                var lastOccurrence = ScheduleCalculator.GetPreviousDailyOccurrence(timeOfDay, now);

                // 只有「今天這一次真的還沒跑過」才補做。
                // 否則每次重開程式都會再喚醒一輪，白白多花一整輪的 Token。
                var alreadyRan = job.FinishedAt is { } finished && finished >= lastOccurrence;
                job.ScheduledAt = alreadyRan
                    ? ScheduleCalculator.GetNextDailyOccurrence(timeOfDay, now)
                    : lastOccurrence;
            }

            if (job.Status == ScheduleStatus.Running)
            {
                // 上次是被強制關閉的，不可能還在跑。
                job.Status = ScheduleStatus.Pending;
                job.StartedAt = null;
            }
        }

        await _gate.WaitAsync(cancellationToken).ConfigureAwait(false);
        try
        {
            _jobs = loaded;
            await _jobStore.SaveAsync(_jobs, cancellationToken).ConfigureAwait(false);
        }
        finally
        {
            _gate.Release();
        }

        _schedulerLoop = RunSchedulerLoopAsync(_stopSource.Token);
        RaiseJobsChanged();
    }

    public async Task<IReadOnlyList<ScheduledJob>> GetJobsAsync(CancellationToken cancellationToken = default)
    {
        ObjectDisposedException.ThrowIf(_disposed, this);
        await _gate.WaitAsync(cancellationToken).ConfigureAwait(false);
        try
        {
            var snapshot = new List<ScheduledJob>(_jobs.Count);
            for (var i = 0; i < _jobs.Count; i++)
            {
                snapshot.Add(_jobs[i].Clone());
            }
            snapshot.Sort(static (left, right) => left.ScheduledAt.CompareTo(right.ScheduledAt));
            return snapshot;
        }
        finally
        {
            _gate.Release();
        }
    }

    public async Task UpsertAsync(ScheduledJob job, CancellationToken cancellationToken = default)
    {
        ObjectDisposedException.ThrowIf(_disposed, this);
        ValidateJob(job);
        await _gate.WaitAsync(cancellationToken).ConfigureAwait(false);
        try
        {
            var saved = job.Clone();
            if (saved.Recurrence == ScheduleRecurrence.Interval)
            {
                var initialTime = saved.InitialTimeOfDay ?? saved.ScheduledAt.LocalDateTime.TimeOfDay;
                saved.InitialTimeOfDay = initialTime;
                if (saved.ScheduledAt <= Now)
                {
                    saved.ScheduledAt = ScheduleCalculator.GetNextDailyOccurrence(initialTime, Now);
                }
            }
            else
            {
                saved.Recurrence = ScheduleRecurrence.Daily;
                saved.ScheduledAt = ScheduleCalculator.GetNextDailyOccurrence(
                    saved.ScheduledAt.LocalDateTime.TimeOfDay,
                    Now);
            }
            saved.Status = saved.Enabled ? ScheduleStatus.Pending : ScheduleStatus.Disabled;
            saved.StartedAt = null;
            saved.FinishedAt = null;
            saved.LastResults = [];

            var index = _jobs.FindIndex(item => item.Id == saved.Id);
            if (index >= 0)
            {
                if (_jobs[index].Status == ScheduleStatus.Running)
                {
                    throw new InvalidOperationException("排程正在執行，暫時不能修改。");
                }
                _jobs[index] = saved;
            }
            else
            {
                _jobs.Add(saved);
            }

            await _jobStore.SaveAsync(_jobs, cancellationToken).ConfigureAwait(false);
        }
        finally
        {
            _gate.Release();
        }

        RaiseJobsChanged();
        SignalScheduler();
    }

    public async Task DeleteAsync(Guid id, CancellationToken cancellationToken = default)
    {
        ObjectDisposedException.ThrowIf(_disposed, this);
        await _gate.WaitAsync(cancellationToken).ConfigureAwait(false);
        try
        {
            if (_jobs.Any(job => job.Id == id && job.Status == ScheduleStatus.Running))
            {
                throw new InvalidOperationException("排程正在執行，暫時不能刪除。");
            }
            _jobs.RemoveAll(job => job.Id == id);
            await _jobStore.SaveAsync(_jobs, cancellationToken).ConfigureAwait(false);
        }
        finally
        {
            _gate.Release();
        }

        RaiseJobsChanged();
        SignalScheduler();
    }

    public async Task RunNowAsync(Guid id, CancellationToken cancellationToken = default)
    {
        ObjectDisposedException.ThrowIf(_disposed, this);
        ScheduledJob execution;
        await _gate.WaitAsync(cancellationToken).ConfigureAwait(false);
        try
        {
            var job = _jobs.SingleOrDefault(item => item.Id == id)
                ?? throw new InvalidOperationException("找不到指定排程。");
            if (job.Status == ScheduleStatus.Running)
            {
                throw new InvalidOperationException("排程已經在執行中。");
            }

            job.Enabled = true;
            job.Status = ScheduleStatus.Running;
            job.StartedAt = Now;
            job.FinishedAt = null;
            job.LastResults = [];
            execution = job.Clone();
            await _jobStore.SaveAsync(_jobs, cancellationToken).ConfigureAwait(false);
        }
        finally
        {
            _gate.Release();
        }

        RaiseJobsChanged();
        StartExecution(execution, cancellationToken);
    }

    /// <summary>
    /// 背景排程迴圈。每一輪掃描到期排程後，直接睡到下一個到期時間
    /// （上限 <see cref="MaxIdleWait"/>），而不是固定每秒喚醒。
    /// </summary>
    private async Task RunSchedulerLoopAsync(CancellationToken cancellationToken)
    {
        try
        {
            while (!cancellationToken.IsCancellationRequested)
            {
                var nextDelay = await ScanAndStartDueJobsAsync(cancellationToken).ConfigureAwait(false);
                await WaitForNextRoundAsync(nextDelay, cancellationToken).ConfigureAwait(false);
            }
        }
        catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
        {
        }
        catch (Exception ex)
        {
            RaiseBackgroundError(ex);
        }
    }

    /// <summary>
    /// 等待下一輪掃描，期間若有排程異動會被 <see cref="SignalScheduler"/> 立即喚醒。
    /// </summary>
    private async Task WaitForNextRoundAsync(TimeSpan delay, CancellationToken cancellationToken)
    {
        if (delay < MinWait)
        {
            delay = MinWait;
        }
        else if (delay > MaxIdleWait)
        {
            delay = MaxIdleWait;
        }

        // 逾時代表「睡到下一個到期時間」，取得號誌代表「排程被改動了，立刻重新評估」。
        await _wakeSignal.WaitAsync(delay, cancellationToken).ConfigureAwait(false);
    }

    private void SignalScheduler()
    {
        try
        {
            if (_wakeSignal.CurrentCount == 0)
            {
                _wakeSignal.Release();
            }
        }
        catch (ObjectDisposedException)
        {
        }
        catch (SemaphoreFullException)
        {
        }
    }

    /// <summary>
    /// 掃描並啟動到期排程，回傳距離下一個到期時間還有多久。
    /// </summary>
    private async Task<TimeSpan> ScanAndStartDueJobsAsync(CancellationToken cancellationToken)
    {
        List<ScheduledJob>? dueJobs = null;
        TimeSpan nextDelay;

        await _gate.WaitAsync(cancellationToken).ConfigureAwait(false);
        try
        {
            var now = Now;
            List<ScheduledJob>? overdue = null;
            for (var i = 0; i < _jobs.Count; i++)
            {
                var job = _jobs[i];
                if (job.Enabled && job.Status == ScheduleStatus.Pending && job.ScheduledAt <= now)
                {
                    // 只有真的有東西到期時才配置清單，閒置掃描是零配置的。
                    (overdue ??= []).Add(job);
                }
            }

            if (overdue is not null)
            {
                // 電腦睡眠或程式關閉一段時間後，可能同時有多個排程逾時。
                // 只補做時間離現在最近的那一個，其餘直接推到下一次每日時點，
                // 避免同一輪一次觸發多個 CLI 呼叫。
                var primary = overdue[0];
                for (var i = 1; i < overdue.Count; i++)
                {
                    if (overdue[i].ScheduledAt > primary.ScheduledAt)
                    {
                        primary = overdue[i];
                    }
                }

                for (var i = 0; i < overdue.Count; i++)
                {
                    var job = overdue[i];
                    if (ReferenceEquals(job, primary))
                    {
                        (dueJobs ??= []).Add(job.Clone());
                        job.Status = ScheduleStatus.Running;
                        job.StartedAt = now;
                    }
                    else
                    {
                        job.FinishedAt = now;
                        var initialTime = job.InitialTimeOfDay ?? job.ScheduledAt.LocalDateTime.TimeOfDay;
                        job.ScheduledAt = job.Recurrence == ScheduleRecurrence.Interval
                            ? ScheduleCalculator.GetNextAutoIntervalOccurrence(initialTime, now)
                            : ScheduleCalculator.GetNextDailyOccurrence(
                                job.ScheduledAt.LocalDateTime.TimeOfDay,
                                now);
                    }
                }

                await _jobStore.SaveAsync(_jobs, cancellationToken).ConfigureAwait(false);
            }

            nextDelay = CalculateNextDelay(now);
        }
        finally
        {
            _gate.Release();
        }

        if (dueJobs is null)
        {
            return nextDelay;
        }

        RaiseJobsChanged();
        for (var i = 0; i < dueJobs.Count; i++)
        {
            StartExecution(dueJobs[i], cancellationToken);
        }

        return nextDelay;
    }

    /// <summary>計算距離最近一個待執行排程的等待時間。呼叫端必須已持有 <see cref="_gate"/>。</summary>
    private TimeSpan CalculateNextDelay(DateTimeOffset now)
    {
        var shortest = MaxIdleWait;
        for (var i = 0; i < _jobs.Count; i++)
        {
            var job = _jobs[i];
            if (!job.Enabled || job.Status != ScheduleStatus.Pending)
            {
                continue;
            }

            var remaining = job.ScheduledAt - now;
            if (remaining < shortest)
            {
                shortest = remaining;
            }
        }
        return shortest;
    }

    private void StartExecution(ScheduledJob job, CancellationToken cancellationToken)
    {
        var id = job.Id;
        var task = ExecuteJobAsync(job, cancellationToken);
        _executions[id] = task;
        _ = task.ContinueWith(
            _ => _executions.TryRemove(id, out Task? _),
            CancellationToken.None,
            TaskContinuationOptions.ExecuteSynchronously,
            TaskScheduler.Default);
    }

    private async Task ExecuteJobAsync(ScheduledJob job, CancellationToken cancellationToken)
    {
        try
        {
            var settings = _settingsProvider();
            settings.EnsureDefaults();
            var timeout = TimeSpan.FromMinutes(settings.ExecutionTimeoutMinutes);

            // 先把所有 Task 具現化，確保每個選定的 CLI 都獨立啟動；
            // Task.WhenAll 只負責等待完成，不會讓程序變成循序執行。
            var runs = new Task<CliRunResult>[job.Targets.Count];
            for (var i = 0; i < job.Targets.Count; i++)
            {
                var target = job.Targets[i];
                runs[i] = _cliRunner.RunAsync(
                    target,
                    settings.CliProfiles[target].Clone(),
                    job.Message,
                    job.WorkingDirectory,
                    timeout,
                    settings.TokenSaverMode,
                    cancellationToken);
            }

            var results = await Task.WhenAll(runs).ConfigureAwait(false);
            await CompleteJobAsync(job.Id, results).ConfigureAwait(false);
        }
        catch (Exception ex)
        {
            try
            {
                await CompleteJobAsync(job.Id, results: null).ConfigureAwait(false);
            }
            catch
            {
                // 忽略次要的清理錯誤
            }

            RaiseBackgroundError(ex);
        }
    }

    /// <summary>
    /// 收尾一次執行：寫回結果並排到下一個每日時點。
    /// <paramref name="results"/> 為 null 代表執行過程擲出例外。
    /// </summary>
    private async Task CompleteJobAsync(Guid jobId, CliRunResult[]? results)
    {
        await _gate.WaitAsync(CancellationToken.None).ConfigureAwait(false);
        try
        {
            var stored = _jobs.SingleOrDefault(item => item.Id == jobId);
            if (stored is null)
            {
                return;
            }

            if (results is null && stored.Status != ScheduleStatus.Running)
            {
                return;
            }

            var now = Now;
            stored.FinishedAt = now;
            if (results is not null)
            {
                stored.LastResults = [.. results];
            }

            if (stored.Enabled)
            {
                if (stored.Recurrence == ScheduleRecurrence.Interval)
                {
                    var initialTime = stored.InitialTimeOfDay ?? stored.ScheduledAt.LocalDateTime.TimeOfDay;
                    stored.ScheduledAt = ScheduleCalculator.GetNextAutoIntervalOccurrence(initialTime, stored.FinishedAt ?? now);
                }
                else
                {
                    if (stored.ScheduledAt <= now)
                    {
                        stored.ScheduledAt = ScheduleCalculator.GetNextDailyOccurrence(
                            stored.ScheduledAt.LocalDateTime.TimeOfDay,
                            now);
                    }
                }
                stored.Status = ScheduleStatus.Pending;
            }
            else
            {
                stored.Status = results is not null && Array.TrueForAll(results, result => result.Succeeded)
                    ? ScheduleStatus.Completed
                    : ScheduleStatus.Failed;
            }

            await _jobStore.SaveAsync(_jobs, CancellationToken.None).ConfigureAwait(false);
        }
        finally
        {
            _gate.Release();
        }

        RaiseJobsChanged();
        SignalScheduler();
    }

    private static void ValidateJob(ScheduledJob job)
    {
        if (string.IsNullOrWhiteSpace(job.Name))
        {
            throw new ArgumentException("請輸入排程名稱。");
        }
        if (string.IsNullOrWhiteSpace(job.Message))
        {
            throw new ArgumentException("請輸入要傳送的簡短訊息。");
        }
        if (job.Message.Length > 50)
        {
            throw new ArgumentException("為避免過量消耗 Token，喚醒訊息最多 50 個字元。");
        }
        if (job.Targets == null || job.Targets.Count == 0)
        {
            throw new ArgumentException("至少選擇一個 CLI。");
        }
        if (string.IsNullOrWhiteSpace(job.WorkingDirectory) || !Directory.Exists(job.WorkingDirectory))
        {
            throw new DirectoryNotFoundException("指定的工作目錄不存在。");
        }
    }

    private void RaiseJobsChanged()
    {
        try
        {
            JobsChanged?.Invoke(this, EventArgs.Empty);
        }
        catch (Exception ex)
        {
            System.Diagnostics.Debug.WriteLine($"JobsChanged event handler threw: {ex}");
        }
    }

    private void RaiseBackgroundError(Exception ex)
    {
        try
        {
            BackgroundError?.Invoke(this, ex);
        }
        catch (Exception handlerEx)
        {
            System.Diagnostics.Debug.WriteLine($"BackgroundError event handler threw: {handlerEx}");
        }
    }

    public async ValueTask DisposeAsync()
    {
        if (_disposed)
        {
            return;
        }
        _disposed = true;

        await _stopSource.CancelAsync().ConfigureAwait(false);

        if (_schedulerLoop is not null)
        {
            try
            {
                await _schedulerLoop.ConfigureAwait(false);
            }
            catch
            {
                // 忽略背景迴圈關閉時的錯誤
            }
        }

        if (!_executions.IsEmpty)
        {
            try
            {
                await Task.WhenAll(_executions.Values).ConfigureAwait(false);
            }
            catch
            {
                // 忽略關閉期間仍在執行的背景工作錯誤
            }
        }

        _stopSource.Dispose();
        _wakeSignal.Dispose();
        _gate.Dispose();
    }
}
