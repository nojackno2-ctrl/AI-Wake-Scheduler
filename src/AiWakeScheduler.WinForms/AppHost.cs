using AiWakeScheduler.Core;

namespace AiWakeScheduler.WinForms;

/// <summary>
/// 組合根：唯一知道所有元件如何組裝起來的地方。
/// 視窗只拿到它需要的服務，Program 只負責啟動與關閉。
/// </summary>
internal sealed class AppHost : IAsyncDisposable
{
    private AppHost(
        AppDataPaths paths,
        JsonFileStore<AppSettings> settingsStore,
        JsonFileStore<List<ScheduledJob>> jobStore,
        AppSettings settings,
        CliRunner runner,
        CliUsageReader usageReader,
        ScheduleManager manager)
    {
        Paths = paths;
        SettingsStore = settingsStore;
        JobStore = jobStore;
        Settings = settings;
        Runner = runner;
        UsageReader = usageReader;
        Manager = manager;
    }

    public AppDataPaths Paths { get; }
    public JsonFileStore<AppSettings> SettingsStore { get; }
    public JsonFileStore<List<ScheduledJob>> JobStore { get; }
    public AppSettings Settings { get; }
    public CliRunner Runner { get; }
    public CliUsageReader UsageReader { get; }
    public ScheduleManager Manager { get; }

    public static async Task<AppHost> CreateAsync(CancellationToken cancellationToken = default)
    {
        var paths = new AppDataPaths();
        paths.EnsureCreated();

        var settingsStore = new JsonFileStore<AppSettings>(paths.SettingsFile, AppSettings.CreateDefault);
        var settings = await settingsStore.LoadAsync(cancellationToken).ConfigureAwait(false);
        settings.EnsureDefaults();

        var jobStore = new JsonFileStore<List<ScheduledJob>>(paths.JobsFile, static () => []);
        var runner = new CliRunner(paths);
        var usageReader = new CliUsageReader();
        var manager = new ScheduleManager(jobStore, runner, () => settings, usageReader);

        var host = new AppHost(paths, settingsStore, jobStore, settings, runner, usageReader, manager);
        await manager.InitializeAsync(cancellationToken).ConfigureAwait(false);
        return host;
    }

    /// <summary>持久化目前設定。</summary>
    public Task SaveSettingsAsync(CancellationToken cancellationToken = default) =>
        SettingsStore.SaveAsync(Settings, cancellationToken);

    public async ValueTask DisposeAsync()
    {
        await Manager.DisposeAsync().ConfigureAwait(false);
        SettingsStore.Dispose();
        JobStore.Dispose();
    }
}
