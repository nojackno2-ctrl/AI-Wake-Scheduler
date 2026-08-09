namespace AiWakeScheduler.Core;

public sealed class AppDataPaths
{
    public AppDataPaths(string? rootDirectory = null)
    {
        RootDirectory = rootDirectory ?? Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
            "AI倒數喚醒");
    }

    public string RootDirectory { get; }
    public string JobsFile => Path.Combine(RootDirectory, "schedules.json");
    public string SettingsFile => Path.Combine(RootDirectory, "settings.json");
    public string LogsDirectory => Path.Combine(RootDirectory, "logs");
    public string WakeupWorkspace => Path.Combine(RootDirectory, "workspace");

    public void EnsureCreated()
    {
        Directory.CreateDirectory(RootDirectory);
        Directory.CreateDirectory(LogsDirectory);
        Directory.CreateDirectory(WakeupWorkspace);
    }
}
