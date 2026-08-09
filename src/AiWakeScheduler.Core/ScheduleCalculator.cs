namespace AiWakeScheduler.Core;

/// <summary>
/// 提供排程下一次執行時間的計算邏輯。
/// </summary>
public static class ScheduleCalculator
{
    /// <summary>
    /// 計算下一次每日排程的執行時間。
    /// </summary>
    public static DateTimeOffset GetNextDailyOccurrence(
        TimeSpan timeOfDay,
        DateTimeOffset now)
    {
        if (timeOfDay < TimeSpan.Zero || timeOfDay >= TimeSpan.FromDays(1))
        {
            throw new ArgumentOutOfRangeException(nameof(timeOfDay), "每日時間必須介於 00:00 到 23:59。");
        }

        var localNow = now.LocalDateTime;
        var targetTime = new TimeSpan(timeOfDay.Hours, timeOfDay.Minutes, 0);
        var localCandidate = localNow.Date.Add(targetTime);

        if (localCandidate <= localNow)
        {
            localCandidate = localCandidate.AddDays(1);
        }

        return CreateLocalDateTimeOffset(localCandidate);
    }

    /// <summary>
    /// 計算指定週期下一次的執行時間。
    /// </summary>
    public static DateTimeOffset GetNextOccurrence(
        DateTimeOffset previousOccurrence,
        ScheduleRecurrence recurrence,
        DateTimeOffset now)
    {
        if (recurrence == ScheduleRecurrence.Once)
        {
            throw new ArgumentException("單次排程沒有下一次執行時間。", nameof(recurrence));
        }

        var incrementDays = recurrence switch
        {
            ScheduleRecurrence.Daily => 1,
            ScheduleRecurrence.Weekly => 7,
            _ => throw new ArgumentOutOfRangeException(nameof(recurrence), recurrence, null)
        };

        var previousLocal = previousOccurrence.LocalDateTime;
        var localNow = now.LocalDateTime;

        // 若前次時間處於未來（例如系統時鐘倒退或時區變更），依據目前時間重新計算
        if (previousLocal > localNow)
        {
            var targetTime = new TimeSpan(previousLocal.Hour, previousLocal.Minute, 0);
            var baseCandidate = localNow.Date.Add(targetTime);
            if (baseCandidate <= localNow)
            {
                baseCandidate = baseCandidate.AddDays(incrementDays);
            }
            return CreateLocalDateTimeOffset(baseCandidate);
        }

        // 使用日數差快速推算，避免過多迴圈
        var daysDiff = (localNow.Date - previousLocal.Date).Days;
        var periods = daysDiff / incrementDays;
        var nextLocal = previousLocal.AddDays(periods * incrementDays);

        while (CreateLocalDateTimeOffset(nextLocal) <= now)
        {
            nextLocal = nextLocal.AddDays(incrementDays);
        }

        return CreateLocalDateTimeOffset(nextLocal);
    }

    /// <summary>
    /// 安全建立 DateTimeOffset，並妥善處理日光節約時間 (DST) 無效時間片段。
    /// </summary>
    private static DateTimeOffset CreateLocalDateTimeOffset(DateTime localDateTime)
    {
        var tz = TimeZoneInfo.Local;
        var cleanDateTime = DateTime.SpecifyKind(localDateTime, DateTimeKind.Local);

        if (tz.IsInvalidTime(cleanDateTime))
        {
            // 若落在 DST 無效時間，往前平移至有效時間區段（通常為 1 小時）
            TimeSpan gap = TimeSpan.FromHours(1);
            var adjustmentRules = tz.GetAdjustmentRules();
            foreach (var rule in adjustmentRules)
            {
                if (rule.DateStart <= cleanDateTime.Date && rule.DateEnd >= cleanDateTime.Date)
                {
                    gap = rule.DaylightDelta;
                    break;
                }
            }
            cleanDateTime = cleanDateTime.Add(gap);
        }

        return new DateTimeOffset(cleanDateTime, tz.GetUtcOffset(cleanDateTime));
    }
}

