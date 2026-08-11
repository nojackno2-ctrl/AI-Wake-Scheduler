using System.Text;
using AiWakeScheduler.Core;

namespace AiWakeScheduler.WinForms;

/// <summary>
/// 把排程模型轉成畫面上的文字與色彩。
/// 從視窗拆出來後，格式化規則可以獨立驗證，視窗只負責擺放控制項。
/// </summary>
internal static class JobPresenter
{
    public static string Time(ScheduledJob job) => job.ScheduledAt.LocalDateTime.ToString("HH:mm");

    public static string Countdown(ScheduledJob job, DateTimeOffset now)
    {
        if (job.Status == ScheduleStatus.Running) return "執行中";
        if (job.Status is ScheduleStatus.Completed or ScheduleStatus.Failed) return "已執行";
        if (!job.Enabled) return "已停用";

        var remaining = job.ScheduledAt - now;
        if (remaining <= TimeSpan.Zero) return "即將執行";

        return remaining.TotalDays >= 1
            ? $"{(int)remaining.TotalDays}天 {remaining:hh\\:mm\\:ss}"
            : remaining.ToString(@"hh\:mm\:ss");
    }

    public static string Status(ScheduleStatus status) => status switch
    {
        ScheduleStatus.Pending => "等待中",
        ScheduleStatus.Running => "執行中",
        ScheduleStatus.Completed => "成功",
        ScheduleStatus.Failed => "失敗",
        ScheduleStatus.Disabled => "已停用",
        _ => status.ToString()
    };

    public static Color StatusColor(ScheduleStatus status) => status switch
    {
        ScheduleStatus.Completed => AppTheme.Success,
        ScheduleStatus.Failed => AppTheme.Danger,
        ScheduleStatus.Disabled => Color.Gray,
        _ => SystemColors.ControlText
    };

    public static string Targets(IReadOnlyList<CliKind> targets)
    {
        if (targets.Count == 0) return string.Empty;

        var builder = new StringBuilder(targets.Count * 14);
        for (var i = 0; i < targets.Count; i++)
        {
            if (i > 0) builder.Append('、');
            builder.Append(CliDisplayNames.GetShort(targets[i]));
        }
        return builder.ToString();
    }

    public static string TargetsLong(IReadOnlyList<CliKind> targets)
    {
        var builder = new StringBuilder(targets.Count * 24);
        for (var i = 0; i < targets.Count; i++)
        {
            if (i > 0) builder.Append('、');
            builder.Append(CliDisplayNames.Get(targets[i]));
        }
        return builder.ToString();
    }

    public static string Results(ScheduledJob job)
    {
        var builder = new StringBuilder(job.LastResults.Count * 160);
        for (var i = 0; i < job.LastResults.Count; i++)
        {
            if (i > 0)
            {
                builder.AppendLine().AppendLine();
            }

            var result = job.LastResults[i];
            builder
                .Append(CliDisplayNames.Get(result.Cli)).Append('：')
                .AppendLine(result.Succeeded ? "成功" : "失敗")
                .Append("結束碼：").AppendLine(result.ExitCode?.ToString() ?? "無")
                .Append("錯誤：").AppendLine(string.IsNullOrWhiteSpace(result.Error) ? "無" : result.Error)
                .Append("日誌：").Append(result.LogPath);
        }
        return builder.ToString();
    }
}
