namespace AiWakeScheduler.Core;

/// <summary>
/// 支援的 AI CLI 工具種類。
/// </summary>
public enum CliKind
{
    Antigravity,
    AntigravityClaude,
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
    Weekly,
    Interval
}

/// <summary>
/// 思考程度 / 推理強度。
/// </summary>
public enum ThinkingEffort
{
    Default,
    Minimal,
    Low,
    Medium,
    High,
    XHigh,
    Max,
    Ultra
}

/// <summary>
/// 個別 CLI 工具的設定檔。
/// </summary>
public sealed class CliProfile
{
    public string Executable { get; set; } = string.Empty;
    public string Model { get; set; } = string.Empty;
    public ThinkingEffort ThinkingEffort { get; set; } = ThinkingEffort.Default;
    public string AdditionalArguments { get; set; } = string.Empty;

    public CliProfile Clone() => new()
    {
        Executable = Executable ?? string.Empty,
        Model = Model ?? string.Empty,
        ThinkingEffort = ThinkingEffort,
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
    public int QuotaAutoRefreshMinutes { get; set; } = 10;
    public Dictionary<CliKind, CliProfile> CliProfiles { get; set; } = CreateDefaultProfiles();

    public static AppSettings CreateDefault() => new();

    /// <summary>把目前設定套用到另一個實例（保留對方的物件識別）。</summary>
    public void CopyTo(AppSettings destination)
    {
        ArgumentNullException.ThrowIfNull(destination);
        EnsureDefaults();

        destination.StartWithWindows = StartWithWindows;
        destination.MinimizeToTray = MinimizeToTray;
        destination.TokenSaverMode = TokenSaverMode;
        destination.ExecutionTimeoutMinutes = ExecutionTimeoutMinutes;
        destination.QuotaAutoRefreshMinutes = QuotaAutoRefreshMinutes;

        var profiles = new Dictionary<CliKind, CliProfile>(CliProfiles.Count);
        foreach (var pair in CliProfiles)
        {
            profiles[pair.Key] = pair.Value.Clone();
        }
        destination.CliProfiles = profiles;
    }

    public AppSettings Clone()
    {
        var clone = new AppSettings();
        CopyTo(clone);
        return clone;
    }

    /// <summary>
    /// 補齊缺少的 CLI 設定並修正超出範圍的值。
    /// 只有真的缺項時才會配置新物件，正常情況下不產生任何垃圾。
    /// </summary>
    public void EnsureDefaults()
    {
        CliProfiles ??= CreateDefaultProfiles();

        var descriptors = CliCatalog.All;
        for (var i = 0; i < descriptors.Count; i++)
        {
            var descriptor = descriptors[i];
            if (!CliProfiles.TryGetValue(descriptor.Kind, out var profile) || profile is null)
            {
                CliProfiles[descriptor.Kind] = new CliProfile { Executable = descriptor.DefaultCommand };
            }
        }

        ExecutionTimeoutMinutes = Math.Clamp(ExecutionTimeoutMinutes, 1, 120);
        QuotaAutoRefreshMinutes = Math.Clamp(QuotaAutoRefreshMinutes, 0, 1440);
    }

    private static Dictionary<CliKind, CliProfile> CreateDefaultProfiles()
    {
        var descriptors = CliCatalog.All;
        var profiles = new Dictionary<CliKind, CliProfile>(descriptors.Count);
        for (var i = 0; i < descriptors.Count; i++)
        {
            profiles[descriptors[i].Kind] = new CliProfile { Executable = descriptors[i].DefaultCommand };
        }
        return profiles;
    }
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

    public CliRunResult Clone() => new()
    {
        Cli = Cli,
        Succeeded = Succeeded,
        ExitCode = ExitCode,
        StartedAt = StartedAt,
        FinishedAt = FinishedAt,
        ExecutablePath = ExecutablePath ?? string.Empty,
        Error = Error ?? string.Empty,
        LogPath = LogPath ?? string.Empty
    };
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
    public List<CliKind> Targets { get; set; } = [CliKind.Antigravity, CliKind.AntigravityClaude, CliKind.Codex, CliKind.Claude];
    public ScheduleRecurrence Recurrence { get; set; } = ScheduleRecurrence.Daily;
    public TimeSpan? InitialTimeOfDay { get; set; }
    public bool Enabled { get; set; } = true;
    public ScheduleStatus Status { get; set; } = ScheduleStatus.Pending;
    public DateTimeOffset? StartedAt { get; set; }
    public DateTimeOffset? FinishedAt { get; set; }
    public List<CliRunResult> LastResults { get; set; } = [];

    public ScheduledJob Clone()
    {
        var targets = Targets is { Count: > 0 } source
            ? new List<CliKind>(source)
            : DefaultTargets();

        List<CliRunResult> results;
        if (LastResults is { Count: > 0 } sourceResults)
        {
            results = new List<CliRunResult>(sourceResults.Count);
            for (var i = 0; i < sourceResults.Count; i++)
            {
                results.Add(sourceResults[i].Clone());
            }
        }
        else
        {
            results = [];
        }

        return new ScheduledJob
        {
            Id = Id,
            Name = Name ?? "AI 倒數喚醒",
            ScheduledAt = ScheduledAt,
            Message = Message ?? "早安",
            WorkingDirectory = WorkingDirectory ?? Environment.GetFolderPath(Environment.SpecialFolder.UserProfile),
            Targets = targets,
            Recurrence = Recurrence,
            InitialTimeOfDay = InitialTimeOfDay,
            Enabled = Enabled,
            Status = Status,
            StartedAt = StartedAt,
            FinishedAt = FinishedAt,
            LastResults = results
        };
    }

    private static List<CliKind> DefaultTargets()
    {
        var descriptors = CliCatalog.All;
        var targets = new List<CliKind>(descriptors.Count);
        for (var i = 0; i < descriptors.Count; i++)
        {
            targets.Add(descriptors[i].Kind);
        }
        return targets;
    }
}

/// <summary>
/// CLI 顯示名稱對照，實際內容來自 <see cref="CliCatalog"/>。
/// </summary>
public static class CliDisplayNames
{
    public static string Get(CliKind kind) => CliCatalog.Get(kind).DisplayName;

    /// <summary>清單欄位使用的短名稱。</summary>
    public static string GetShort(CliKind kind) => CliCatalog.Get(kind).ShortName;
}

/// <summary>
/// 排程週期顯示名稱對照。
/// </summary>
public static class ScheduleRecurrenceNames
{
    public static string Get(ScheduleRecurrence recurrence) => recurrence switch
    {
        ScheduleRecurrence.Once => "單次",
        ScheduleRecurrence.Daily => "每天固定時間",
        ScheduleRecurrence.Weekly => "每週",
        ScheduleRecurrence.Interval => "自動模式（每 5 小時 1 分鐘）",
        _ => recurrence.ToString()
    };
}

/// <summary>
/// 思考程度顯示名稱對照。
/// </summary>
public static class ThinkingEffortDisplayNames
{
    public static string Get(ThinkingEffort effort) => effort switch
    {
        ThinkingEffort.Default => "預設",
        ThinkingEffort.Minimal => "最低 (Minimal)",
        ThinkingEffort.Low => "低 (Low)",
        ThinkingEffort.Medium => "中 (Medium)",
        ThinkingEffort.High => "高 (High)",
        ThinkingEffort.XHigh => "極高 (XHigh)",
        ThinkingEffort.Max => "最高 (Max)",
        ThinkingEffort.Ultra => "超高 (Ultra)",
        _ => effort.ToString()
    };
}

