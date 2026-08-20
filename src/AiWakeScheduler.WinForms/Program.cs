namespace AiWakeScheduler.WinForms;

internal static class Program
{
    private const string SingleInstanceMutexName = "Local\\AiWakeScheduler-9E3CCEC5-7BAC-4DEA-9687-BBB9E4982EB9";

    [STAThread]
    private static void Main(string[] args)
    {
        var isMinimized = args.Contains("--minimized", StringComparer.OrdinalIgnoreCase);
        using var mutex = new Mutex(true, SingleInstanceMutexName, out var ownsMutex);
        if (!ownsMutex)
        {
            if (!isMinimized)
            {
                MessageBox.Show("AI 倒數喚醒已經在執行。", "AI 倒數喚醒", MessageBoxButtons.OK, MessageBoxIcon.Information);
            }
            return;
        }

        ApplicationConfiguration.Initialize();

        try
        {
            Run(args);
        }
        catch (Exception ex)
        {
            MessageBox.Show(
                $"程式無法啟動：{ex.Message}",
                "AI 倒數喚醒",
                MessageBoxButtons.OK,
                MessageBoxIcon.Error);
        }
        finally
        {
            AppTheme.Release();
            ReleaseMutex(mutex);
        }
    }

    private static void Run(string[] args)
    {
        StartupManager.MigrateLegacyIfNeeded();

        var host = AppHost.CreateAsync().GetAwaiter().GetResult();
        try
        {
            using var mainForm = new MainForm(
                host,
                args.Contains("--minimized", StringComparer.OrdinalIgnoreCase));
            Application.Run(mainForm);
        }
        finally
        {
            host.DisposeAsync().AsTask().GetAwaiter().GetResult();
        }
    }

    private static void ReleaseMutex(Mutex mutex)
    {
        try
        {
            mutex.ReleaseMutex();
        }
        catch (ObjectDisposedException)
        {
        }
        catch (ApplicationException)
        {
        }
    }
}
