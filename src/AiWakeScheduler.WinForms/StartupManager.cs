using System.Reflection;
using Microsoft.Win32;

namespace AiWakeScheduler.WinForms;

internal static class StartupManager
{
    private const string RunKey = @"Software\Microsoft\Windows\CurrentVersion\Run";
    private const string ValueName = "AI倒數喚醒";

    public static void SetEnabled(bool enabled)
    {
        using var key = Registry.CurrentUser.OpenSubKey(RunKey, writable: true)
            ?? Registry.CurrentUser.CreateSubKey(RunKey, writable: true);
        if (!enabled)
        {
            key.DeleteValue(ValueName, throwOnMissingValue: false);
            return;
        }

        key.SetValue(ValueName, BuildStartupCommand(), RegistryValueKind.String);
    }

    private static string BuildStartupCommand()
    {
        var processPath = Environment.ProcessPath
            ?? throw new InvalidOperationException("無法取得目前程式路徑。");
        if (string.Equals(Path.GetFileNameWithoutExtension(processPath), "dotnet", StringComparison.OrdinalIgnoreCase))
        {
            var assemblyPath = Assembly.GetExecutingAssembly().Location;
            return $"\"{processPath}\" \"{assemblyPath}\" --minimized";
        }
        return $"\"{processPath}\" --minimized";
    }
}

