namespace AiWakeScheduler.WinForms;

internal static class Program
{
    private const string SingleInstanceMutexName = "Local\\AiWakeScheduler-9E3CCEC5-7BAC-4DEA-9687-BBB9E4982EB9";

    [STAThread]
    private static void Main(string[] args)
    {
        // 必須在啟動任何子程序之前設定：錯誤模式是在建立子程序時繼承下去的。
        AiWakeScheduler.Core.ProcessRunner.SuppressChildProcessErrorDialogs();

        var isMinimized = args.Contains("--minimized", StringComparer.OrdinalIgnoreCase);
        var verifyUi = args.Contains("--verify-ui", StringComparer.OrdinalIgnoreCase);
        var mutexName = verifyUi ? $"{SingleInstanceMutexName}-QA-{Guid.NewGuid():N}" : SingleInstanceMutexName;
        using var mutex = new Mutex(true, mutexName, out var ownsMutex);
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
        var verifyUi = args.Contains("--verify-ui", StringComparer.OrdinalIgnoreCase);
        var startupMigrationError = verifyUi ? null : StartupManager.MigrateLegacyIfNeeded();
        var rootDirectory = verifyUi ? Path.Combine(Path.GetTempPath(), $"AiWakeScheduler-QA-{Guid.NewGuid():N}") : null;

        var host = AppHost.CreateAsync(rootDirectory: rootDirectory).GetAwaiter().GetResult();
        try
        {
            using var mainForm = new MainForm(
                host,
                args.Contains("--minimized", StringComparer.OrdinalIgnoreCase));
            if (verifyUi) mainForm.Text += "（功能驗證）";
            if (startupMigrationError is not null)
            {
                mainForm.Shown += (_, _) => MessageBox.Show(mainForm,
                    $"開機啟動設定未完成遷移：{startupMigrationError}",
                    "AI 倒數喚醒", MessageBoxButtons.OK, MessageBoxIcon.Warning);
            }
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
