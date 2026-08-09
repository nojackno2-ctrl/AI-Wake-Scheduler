namespace AiWakeScheduler.Core;

/// <summary>
/// 支援的 AI CLI 工具種類。
/// </summary>
public enum CliKind
{
    Antigravity,
    Codex,
    Claude
}

/// <summary>
/// 排程狀態。
/// </summary>
public enum ScheduleStatus
{
    Pending,
    Running,
    Completed,
    Failed,
    Disabled
}

/// <summary>
/// 排程週期模式。
/// </summary>
public enum ScheduleRecurrence
{
    // Once and Weekly are retained only so settings written by older versions
    // can still be deserialized and migrated to Daily by ScheduleManager.
    Once,
    Daily,
    Weekly
}

/// <summary>
/// 個別 CLI 工具的設定檔。
/// </summary>
public sealed class CliProfile
{
    public string Executable { get; set; } = string.Empty;
    public string AdditionalArguments { get; set; } = string.Empty;

    public CliProfile Clone() => new()
    {
        Executable = Executable ?? string.Empty,
        AdditionalArguments = AdditionalArguments ?? string.Empty
    };
}

/// <summary>
/// 應用程式整體設定。
/// </summary>
public sealed class AppSettings
{
    public bool StartWithWindows { get; set; }
    public bool MinimizeToTray { get; set; } = true;
    public bool TokenSaverMode { get; set; } = true;
    public int ExecutionTimeoutMinutes { get; set; } = 3;
    public Dictionary<CliKind, CliProfile> CliProfiles { get; set; } = CreateDefaultProfiles();

    public static AppSettings CreateDefault() => new();

    public void EnsureDefaults()
    {
        CliProfiles ??= CreateDefaultProfiles();
        foreach (var pair in CreateDefaultProfiles())
        {
            if (!CliProfiles.ContainsKey(pair.Key))
            {
                CliProfiles[pair.Key] = pair.Value;
            }
            else if (CliProfiles[pair.Key] is null)
            {
                CliProfiles[pair.Key] = pair.Value;
            }
        }

        ExecutionTimeoutMinutes = Math.Clamp(ExecutionTimeoutMinutes, 1, 120);
    }

    private static Dictionary<CliKind, CliProfile> CreateDefaultProfiles() => new()
    {
        [CliKind.Antigravity] = new CliProfile { Executable = "agy" },
        [CliKind.Codex] = new CliProfile { Executable = "codex" },
        [CliKind.Claude] = new CliProfile { Executable = "claude" }
    };
}

/// <summary>
/// CLI 執行結果。
/// </summary>
public sealed class CliRunResult
{
    public CliKind Cli { get; set; }
    public bool Succeeded { get; set; }
    public int? ExitCode { get; set; }
    public DateTimeOffset StartedAt { get; set; }
    public DateTimeOffset FinishedAt { get; set; }
    public string ExecutablePath { get; set; } = string.Empty;
    public string Error { get; set; } = string.Empty;
    public string LogPath { get; set; } = string.Empty;
}

/// <summary>
/// 排程工作模型。
/// </summary>
public sealed class ScheduledJob
{
    public Guid Id { get; set; } = Guid.NewGuid();
    public string Name { get; set; } = "AI 倒數喚醒";
    public DateTimeOffset ScheduledAt { get; set; } = DateTimeOffset.Now.AddMinutes(5);
    public string Message { get; set; } = "早安";
    public string WorkingDirectory { get; set; } = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
    public List<CliKind> Targets { get; set; } = [CliKind.Antigravity, CliKind.Codex, CliKind.Claude];
    public ScheduleRecurrence Recurrence { get; set; } = ScheduleRecurrence.Daily;
    public bool Enabled { get; set; } = true;
    public ScheduleStatus Status { get; set; } = ScheduleStatus.Pending;
    public DateTimeOffset? StartedAt { get; set; }
    public DateTimeOffset? FinishedAt { get; set; }
    public List<CliRunResult> LastResults { get; set; } = [];

    public ScheduledJob Clone() => new()
    {
        Id = Id,
        Name = Name ?? "AI 倒數喚醒",
        ScheduledAt = ScheduledAt,
        Message = Message ?? "早安",
        WorkingDirectory = WorkingDirectory ?? Environment.GetFolderPath(Environment.SpecialFolder.UserProfile),
        Targets = Targets != null ? [.. Targets] : [CliKind.Antigravity, CliKind.Codex, CliKind.Claude],
        Recurrence = Recurrence,
        Enabled = Enabled,
        Status = Status,
        StartedAt = StartedAt,
        FinishedAt = FinishedAt,
        LastResults = LastResults != null ? LastResults.Select(result => new CliRunResult
        {
            Cli = result.Cli,
            Succeeded = result.Succeeded,
            ExitCode = result.ExitCode,
            StartedAt = result.StartedAt,
            FinishedAt = result.FinishedAt,
            ExecutablePath = result.ExecutablePath ?? string.Empty,
            Error = result.Error ?? string.Empty,
            LogPath = result.LogPath ?? string.Empty
        }).ToList() : []
    };
}

/// <summary>
/// CLI 顯示名稱對照。
/// </summary>
public static class CliDisplayNames
{
    public static string Get(CliKind kind) => kind switch
    {
        CliKind.Antigravity => "Antigravity CLI",
        CliKind.Codex => "Codex CLI",
        CliKind.Claude => "Claude CLI",
        _ => kind.ToString()
    };
}

/// <summary>
/// 排程週期顯示名稱對照。
/// </summary>
public static class ScheduleRecurrenceNames
{
    public static string Get(ScheduleRecurrence recurrence) => recurrence switch
    {
        ScheduleRecurrence.Once => "單次",
        ScheduleRecurrence.Daily => "每天",
        ScheduleRecurrence.Weekly => "每週",
        _ => recurrence.ToString()
    };
}

