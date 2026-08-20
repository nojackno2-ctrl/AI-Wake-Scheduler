using System.Diagnostics;
using System.Reflection;
using System.Text;
using Microsoft.Win32;

namespace AiWakeScheduler.WinForms;

/// <summary>
/// 管理開機自動啟動。程式現在需要系統管理員權限執行（見 app.manifest，
/// 用來啟用 SeDebugPrivilege 讀取 Antigravity CSRF Token），因此自動啟動
/// 不能再用登錄機碼 Run key ——那只會以標準權限啟動、且無法在無人值守時
/// 自動取得提升權限。改用工作排程器建立「以最高權限執行」＋「不論使用者
/// 是否登入均可執行」的登入觸發工作，開機仍會靜默啟動、不會跳出 UAC。
/// </summary>
internal static class StartupManager
{
    private const string RunKey = @"Software\Microsoft\Windows\CurrentVersion\Run";
    private const string LegacyValueName = "AI倒數喚醒";
    private const string TaskName = "AI倒數喚醒";
    private static readonly string[] LegacyShortcutNames = ["AI 倒數喚醒.lnk", "AI倒數喚醒.lnk"];

    /// <summary>
    /// 升級相容：v1.3.0 前的版本用登錄機碼 Run key 記錄「開機自動啟動」。
    /// 主程式改成需要系統管理員權限後，Run key 啟動的程序無法自動取得提升權限，
    /// 靜默開機會失敗。啟動時偵測到舊機碼就自動遷移成工作排程器的提升權限工作，
    /// 使用者不需要重新手動勾選一次設定。
    /// </summary>
    public static void MigrateLegacyIfNeeded()
    {
        try
        {
            using var key = Registry.CurrentUser.OpenSubKey(RunKey, writable: false);
            var legacyValue = key?.GetValue(LegacyValueName) as string;
            if (string.IsNullOrWhiteSpace(legacyValue))
            {
                return;
            }

            SetEnabled(true);
        }
        catch
        {
            // 遷移失敗不阻擋程式啟動，使用者仍可在設定視窗手動重新勾選。
        }
    }

    public static void SetEnabled(bool enabled)
    {
        CleanupStartupShortcuts();
        CleanupLegacyRunKeyValue();

        if (!enabled)
        {
            RunSchTasks($"/Delete /TN \"{TaskName}\" /F", allowNotFound: true);
            return;
        }

        var command = BuildStartupCommand();
        RunSchTasks(
            $"/Create /F /SC ONLOGON /RL HIGHEST /TN \"{TaskName}\" /TR {command}");
    }

    private static void RunSchTasks(string arguments, bool allowNotFound = false)
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

        var stdErr = process.StandardError.ReadToEnd();
        process.WaitForExit();

        if (process.ExitCode != 0)
        {
            // 工作原本就不存在時刪除會回傳非 0，允許忽略（例如從未啟用過就直接關閉設定）。
            if (allowNotFound)
            {
                return;
            }

            throw new InvalidOperationException(
                $"設定 Windows 工作排程器自動啟動工作失敗（結束碼 {process.ExitCode}）：{stdErr.Trim()}");
        }
    }

    private static void CleanupLegacyRunKeyValue()
    {
        try
        {
            using var key = Registry.CurrentUser.OpenSubKey(RunKey, writable: true);
            key?.DeleteValue(LegacyValueName, throwOnMissingValue: false);
        }
        catch
        {
            // 舊版登錄機碼清理失敗不影響新的工作排程器設定。
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

    /// <summary>回傳給 schtasks /TR 使用、已含外層引號的完整命令字串。</summary>
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

        // schtasks /TR 的值若含空白，整段要再包一層引號，內層路徑引號用 \" 逸出。
        if (string.Equals(Path.GetFileNameWithoutExtension(processPath), "dotnet", StringComparison.OrdinalIgnoreCase))
        {
            var assemblyPath = Path.Combine(AppContext.BaseDirectory, "AI倒數喚醒.dll");
            return $"\"\\\"{processPath}\\\" \\\"{assemblyPath}\\\" --minimized\"";
        }
        return $"\"\\\"{processPath}\\\" --minimized\"";
    }
}
