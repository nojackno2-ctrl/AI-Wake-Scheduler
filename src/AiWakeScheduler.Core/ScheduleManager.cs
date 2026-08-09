using System.Collections.Concurrent;

namespace AiWakeScheduler.Core;

/// <summary>
/// 管理 AI 喚醒排程的初始化、掃描、手動觸發與背景對話執行。
/// </summary>
public sealed class ScheduleManager(
    JsonFileStore<List<ScheduledJob>> jobStore,
    CliRunner cliRunner,
    Func<AppSettings> settingsProvider) : IAsyncDisposable
{
    private readonly SemaphoreSlim _gate = new(1, 1);
    private readonly ConcurrentDictionary<Guid, Task> _executions = new();
    private readonly CancellationTokenSource _stopSource = new();
    private List<ScheduledJob> _jobs = [];
    private Task? _schedulerLoop;
    private bool _disposed;

    public event EventHandler? JobsChanged;
    public event EventHandler<Exception>? BackgroundError;

    public async Task InitializeAsync(CancellationToken cancellationToken = default)
    {
        ObjectDisposedException.ThrowIf(_disposed, this);
        var loaded = await jobStore.LoadAsync(cancellationToken).ConfigureAwait(false);
        foreach (var job in loaded)
        {
            // Older versions allowed one-time and weekly schedules. Keep their
            // chosen hour/minute, but migrate every saved schedule to the app's
            // daily HH:mm contract.
            var local = job.ScheduledAt.LocalDateTime;
            job.ScheduledAt = ScheduleCalculator.GetNextDailyOccurrence(local.TimeOfDay, DateTimeOffset.Now.AddDays(-1));
            job.Recurrence = ScheduleRecurrence.Daily;
            if (job.Status == ScheduleStatus.Running)
            {
                job.Status = ScheduleStatus.Pending;
                job.StartedAt = null;
            }
        }

        await _gate.WaitAsync(cancellationToken).ConfigureAwait(false);
        try
        {
            _jobs = loaded;
            await jobStore.SaveAsync(_jobs, cancellationToken).ConfigureAwait(false);
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
            return _jobs.Select(job => job.Clone()).OrderBy(job => job.ScheduledAt).ToList();
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
            saved.Recurrence = ScheduleRecurrence.Daily;
            saved.ScheduledAt = ScheduleCalculator.GetNextDailyOccurrence(
                saved.ScheduledAt.LocalDateTime.TimeOfDay,
                DateTimeOffset.Now);
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
            await jobStore.SaveAsync(_jobs, cancellationToken).ConfigureAwait(false);
        }
        finally
        {
            _gate.Release();
        }
        RaiseJobsChanged();
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
            await jobStore.SaveAsync(_jobs, cancellationToken).ConfigureAwait(false);
        }
        finally
        {
            _gate.Release();
        }
        RaiseJobsChanged();
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
            job.StartedAt = DateTimeOffset.Now;
            job.FinishedAt = null;
            job.LastResults = [];
            execution = job.Clone();
            await jobStore.SaveAsync(_jobs, cancellationToken).ConfigureAwait(false);
        }
        finally
        {
            _gate.Release();
        }
        RaiseJobsChanged();
        StartExecution(execution, cancellationToken);
    }

    private async Task RunSchedulerLoopAsync(CancellationToken cancellationToken)
    {
        using var timer = new PeriodicTimer(TimeSpan.FromSeconds(1));
        try
        {
            await ScanAndStartDueJobsAsync(cancellationToken).ConfigureAwait(false);
            while (await timer.WaitForNextTickAsync(cancellationToken).ConfigureAwait(false))
            {
                await ScanAndStartDueJobsAsync(cancellationToken).ConfigureAwait(false);
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

    private async Task ScanAndStartDueJobsAsync(CancellationToken cancellationToken)
    {
        List<ScheduledJob> dueJobs;
        await _gate.WaitAsync(cancellationToken).ConfigureAwait(false);
        try
        {
            dueJobs = _jobs
                .Where(job => job.Enabled && job.Status == ScheduleStatus.Pending && job.ScheduledAt <= DateTimeOffset.Now)
                .Select(job => job.Clone())
                .ToList();
            if (dueJobs.Count == 0)
            {
                return;
            }

            foreach (var due in dueJobs)
            {
                var stored = _jobs.Single(job => job.Id == due.Id);
                stored.Status = ScheduleStatus.Running;
                stored.StartedAt = DateTimeOffset.Now;
            }
            await jobStore.SaveAsync(_jobs, cancellationToken).ConfigureAwait(false);
        }
        finally
        {
            _gate.Release();
        }
        RaiseJobsChanged();

        foreach (var job in dueJobs)
        {
            StartExecution(job, cancellationToken);
        }
    }

    private void StartExecution(ScheduledJob job, CancellationToken cancellationToken)
    {
        var task = ExecuteJobAsync(job, cancellationToken);
        _executions[job.Id] = task;
        _ = task.ContinueWith(
            _ => _executions.TryRemove(job.Id, out Task? _),
            CancellationToken.None,
            TaskContinuationOptions.ExecuteSynchronously,
            TaskScheduler.Default);
    }

    private async Task ExecuteJobAsync(ScheduledJob job, CancellationToken cancellationToken)
    {
        try
        {
            var settings = settingsProvider();
            settings.EnsureDefaults();
            var timeout = TimeSpan.FromMinutes(settings.ExecutionTimeoutMinutes);
            // Materialize every task first so all selected CLIs are launched independently.
            // Task.WhenAll only waits for completion; it does not serialize the processes.
            var runs = job.Targets.Select(target => cliRunner.RunAsync(
                target,
                settings.CliProfiles[target].Clone(),
                job.Message,
                job.WorkingDirectory,
                timeout,
                settings.TokenSaverMode,
                cancellationToken)).ToArray();
            var results = await Task.WhenAll(runs).ConfigureAwait(false);

            await _gate.WaitAsync(CancellationToken.None).ConfigureAwait(false);
            try
            {
                var stored = _jobs.SingleOrDefault(item => item.Id == job.Id);
                if (stored is not null)
                {
                    stored.LastResults = [.. results];
                    stored.FinishedAt = DateTimeOffset.Now;
                    if (!stored.Enabled)
                    {
                        stored.Status = results.All(result => result.Succeeded)
                            ? ScheduleStatus.Completed
                            : ScheduleStatus.Failed;
                    }
                    else
                    {
                        if (stored.ScheduledAt <= DateTimeOffset.Now)
                        {
                            stored.ScheduledAt = ScheduleCalculator.GetNextDailyOccurrence(
                                stored.ScheduledAt.LocalDateTime.TimeOfDay,
                                DateTimeOffset.Now);
                        }
                        stored.Status = ScheduleStatus.Pending;
                        stored.Enabled = true;
                    }
                    await jobStore.SaveAsync(_jobs, CancellationToken.None).ConfigureAwait(false);
                }
            }
            finally
            {
                _gate.Release();
            }
            RaiseJobsChanged();
        }
        catch (Exception ex)
        {
            try
            {
                await _gate.WaitAsync(CancellationToken.None).ConfigureAwait(false);
                try
                {
                    var stored = _jobs.SingleOrDefault(item => item.Id == job.Id);
                    if (stored is not null && stored.Status == ScheduleStatus.Running)
                    {
                        stored.FinishedAt = DateTimeOffset.Now;
                        if (stored.ScheduledAt <= DateTimeOffset.Now)
                        {
                            stored.ScheduledAt = ScheduleCalculator.GetNextDailyOccurrence(
                                stored.ScheduledAt.LocalDateTime.TimeOfDay,
                                DateTimeOffset.Now);
                        }
                        stored.Status = stored.Enabled ? ScheduleStatus.Pending : ScheduleStatus.Failed;
                        await jobStore.SaveAsync(_jobs, CancellationToken.None).ConfigureAwait(false);
                    }
                }
                finally
                {
                    _gate.Release();
                }
                RaiseJobsChanged();
            }
            catch
            {
                // Ignore secondary cleanup errors
            }

            RaiseBackgroundError(ex);
        }
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

        _stopSource.Cancel();
        if (_schedulerLoop is not null)
        {
            try
            {
                await _schedulerLoop.ConfigureAwait(false);
            }
            catch
            {
                // Ignore background loop shutdown errors
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
                // Ignore background execution errors during dispose
            }
        }
        _stopSource.Dispose();
        _gate.Dispose();
    }
}

