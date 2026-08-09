using AiWakeScheduler.Core;

namespace AiWakeScheduler.WinForms;

internal static class Program
{
    [STAThread]
    private static void Main(string[] args)
    {
        using var mutex = new Mutex(true, "Local\\AiWakeScheduler-9E3CCEC5-7BAC-4DEA-9687-BBB9E4982EB9", out var ownsMutex);
        if (!ownsMutex)
        {
            MessageBox.Show("AI 倒數喚醒已經在執行。", "AI 倒數喚醒", MessageBoxButtons.OK, MessageBoxIcon.Information);
            return;
        }

        ApplicationConfiguration.Initialize();

        try
        {
            var paths = new AppDataPaths();
            paths.EnsureCreated();
            var settingsStore = new JsonFileStore<AppSettings>(paths.SettingsFile, AppSettings.CreateDefault);
            var settings = settingsStore.LoadAsync().GetAwaiter().GetResult();
            settings.EnsureDefaults();
            var runner = new CliRunner(paths);
            var jobStore = new JsonFileStore<List<ScheduledJob>>(paths.JobsFile, () => []);
            var manager = new ScheduleManager(jobStore, runner, () => settings);
            manager.InitializeAsync().GetAwaiter().GetResult();

            using var mainForm = new MainForm(
                manager,
                runner,
                paths,
                settingsStore,
                settings,
                args.Contains("--minimized", StringComparer.OrdinalIgnoreCase));
            Application.Run(mainForm);
            manager.DisposeAsync().AsTask().GetAwaiter().GetResult();
        }
        catch (Exception ex)
        {
            MessageBox.Show(
                $"程式無法啟動：{ex.Message}",
                "AI 倒數喚醒",
                MessageBoxButtons.OK,
                MessageBoxIcon.Error);
        }
    }
}

