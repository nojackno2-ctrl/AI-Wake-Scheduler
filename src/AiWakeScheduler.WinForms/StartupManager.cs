using System.Diagnostics;
using System.Text;
using Microsoft.Win32;

namespace AiWakeScheduler.WinForms;

/// <summary>
/// 以目前使用者 Run key 管理開機啟動，並遷移舊版排程工作與啟動捷徑。
/// </summary>
internal static class StartupManager
{
    private const string RunKey = @"Software\Microsoft\Windows\CurrentVersion\Run";
    private const string LegacyValueName = "AI倒數喚醒";
    private const string TaskName = "AI倒數喚醒";
    private static readonly string[] LegacyShortcutNames = ["AI 倒數喚醒.lnk", "AI倒數喚醒.lnk"];

    /// <summary>
    /// 保留既有啟動偏好；只有成功移除旧排程後才建立 Run key，避免重複啟動。
    /// </summary>
    public static string? MigrateLegacyIfNeeded()
    {
        try
        {
            if (HasLegacyTask() || IsEnabled()) SetEnabled(true);
        }
        catch (Exception ex)
        {
            // 仍讓程式啟動，但明確顯示遷移未完成，不能悄悄當作成功。
            return ex.Message;
        }
        return null;
    }

    public static void SetEnabled(bool enabled)
    {
        if (HasLegacyTask()) RunSchTasks($"/Delete /TN \"{TaskName}\" /F");
        using var key = Registry.CurrentUser.CreateSubKey(RunKey, writable: true)
            ?? throw new InvalidOperationException("無法開啟目前使用者的開機啟動設定。");

        if (!enabled)
        {
            key.DeleteValue(LegacyValueName, throwOnMissingValue: false);
        }
        else
        {
            key.SetValue(LegacyValueName, BuildStartupCommand(), RegistryValueKind.String);
        }
        CleanupStartupShortcuts();
    }

    public static bool IsEnabled()
    {
        using var key = Registry.CurrentUser.OpenSubKey(RunKey, writable: false);
        return !string.IsNullOrWhiteSpace(key?.GetValue(LegacyValueName) as string);
    }

    private static bool HasLegacyTask()
    {
        var startInfo = new ProcessStartInfo("schtasks.exe")
        {
            UseShellExecute = false,
            CreateNoWindow = true,
            RedirectStandardOutput = true,
            RedirectStandardError = true
        };
        startInfo.ArgumentList.Add("/Query");
        startInfo.ArgumentList.Add("/TN");
        startInfo.ArgumentList.Add(TaskName);
        using var process = Process.Start(startInfo)
            ?? throw new InvalidOperationException("無法檢查舊版開機啟動工作。");
        var output = process.StandardOutput.ReadToEndAsync();
        var error = process.StandardError.ReadToEndAsync();
        process.WaitForExit();
        Task.WhenAll(output, error).GetAwaiter().GetResult();
        if (process.ExitCode == 0) return true;
        // 不存在為正常狀態；拒絕存取不能當成不存在，避免建立重複啟動項目。
        if (error.Result.Contains("denied", StringComparison.OrdinalIgnoreCase) ||
            error.Result.Contains("拒絕", StringComparison.Ordinal))
            throw new InvalidOperationException("無法存取舊版開機啟動工作，請以管理員身分執行一次程式完成遷移。");
        return false;
    }

    private static void RunSchTasks(string arguments)
    {
        var startInfo = new ProcessStartInfo
        {
            FileName = "schtasks.exe",
            Arguments = arguments,
            UseShellExecute = false,
            CreateNoWindow = true,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            StandardOutputEncoding = Encoding.UTF8,
            StandardErrorEncoding = Encoding.UTF8
        };

        using var process = new Process { StartInfo = startInfo };
        try
        {
            process.Start();
        }
        catch (Exception ex)
        {
            throw new InvalidOperationException("無法啟動 schtasks.exe 設定自動啟動工作。", ex);
        }

        var output = process.StandardOutput.ReadToEndAsync();
        var error = process.StandardError.ReadToEndAsync();
        process.WaitForExit();
        Task.WhenAll(output, error).GetAwaiter().GetResult();

        if (process.ExitCode != 0)
        {
            throw new InvalidOperationException(
                $"設定 Windows 工作排程器自動啟動工作失敗（結束碼 {process.ExitCode}）：{error.Result.Trim()}");
        }
    }

    private static void CleanupStartupShortcuts()
    {
        try
        {
            var startupFolder = Environment.GetFolderPath(Environment.SpecialFolder.Startup);
            if (string.IsNullOrWhiteSpace(startupFolder) || !Directory.Exists(startupFolder))
            {
                return;
            }

            foreach (var shortcutName in LegacyShortcutNames)
            {
                var shortcutPath = Path.Combine(startupFolder, shortcutName);
                if (File.Exists(shortcutPath))
                {
                    File.Delete(shortcutPath);
                }
            }
        }
        catch
        {
            // 忽略非關鍵啟動資料夾捷徑清理例外
        }
    }

    /// <summary>Run key 的標準 Windows 命令列；路徑含空白時仍正確引用。</summary>
    private static string BuildStartupCommand()
    {
        var processPath = Environment.ProcessPath;
        if (string.IsNullOrWhiteSpace(processPath))
        {
            processPath = Process.GetCurrentProcess().MainModule?.FileName;
        }
        if (string.IsNullOrWhiteSpace(processPath))
        {
            throw new InvalidOperationException("無法取得目前程式路徑。");
        }

        if (string.Equals(Path.GetFileNameWithoutExtension(processPath), "dotnet", StringComparison.OrdinalIgnoreCase))
        {
            var assemblyPath = Path.Combine(AppContext.BaseDirectory, "AI倒數喚醒.dll");
            return $"\"{processPath}\" \"{assemblyPath}\" --minimized";
        }
        return $"\"{processPath}\" --minimized";
    }
}
