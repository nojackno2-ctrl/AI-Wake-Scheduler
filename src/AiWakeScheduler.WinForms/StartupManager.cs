using System.Diagnostics;
using System.Reflection;
using Microsoft.Win32;

namespace AiWakeScheduler.WinForms;

internal static class StartupManager
{
    private const string RunKey = @"Software\Microsoft\Windows\CurrentVersion\Run";
    private const string ValueName = "AI倒數喚醒";

    public static void SetEnabled(bool enabled)
    {
        try
        {
            using var key = Registry.CurrentUser.OpenSubKey(RunKey, writable: true)
                ?? Registry.CurrentUser.CreateSubKey(RunKey, writable: true);
            if (key is null)
            {
                throw new InvalidOperationException("無法存取 Windows 自動啟動登錄機碼。");
            }

            if (!enabled)
            {
                key.DeleteValue(ValueName, throwOnMissingValue: false);
                return;
            }

            key.SetValue(ValueName, BuildStartupCommand(), RegistryValueKind.String);
        }
        catch (Exception ex) when (ex is UnauthorizedAccessException or System.Security.SecurityException)
        {
            throw new InvalidOperationException("沒有權限修改 Windows 自動啟動登錄設定。", ex);
        }
    }

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


