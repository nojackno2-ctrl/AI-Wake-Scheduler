namespace AiWakeScheduler.Core;

/// <summary>
/// 管理應用程式資料與工作區路徑。
/// </summary>
public sealed class AppDataPaths
{
    public AppDataPaths(string? rootDirectory = null)
    {
        var rawRoot = string.IsNullOrWhiteSpace(rootDirectory)
            ? Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "AI倒數喚醒")
            : rootDirectory;
        RootDirectory = Path.GetFullPath(rawRoot);
    }

    public string RootDirectory { get; }
    public string JobsFile => Path.Combine(RootDirectory, "schedules.json");
    public string SettingsFile => Path.Combine(RootDirectory, "settings.json");
    public string LogsDirectory => Path.Combine(RootDirectory, "logs");
    public string WakeupWorkspace => Path.Combine(RootDirectory, "workspace");

    /// <summary>
    /// 確保所有必要的目錄結構皆已建立。
    /// </summary>
    public void EnsureCreated()
    {
        Directory.CreateDirectory(RootDirectory);
        Directory.CreateDirectory(LogsDirectory);
        Directory.CreateDirectory(WakeupWorkspace);
    }
}

