namespace AiWakeScheduler.Core;

/// <summary>
/// 提供排程下一次執行時間的計算邏輯。
/// </summary>
public static class ScheduleCalculator
{
    /// <summary>
    /// 自動模式的循環間隔（5 個小時 + 1 分鐘 = 301 分鐘）。
    /// </summary>
    public static readonly TimeSpan AutoInterval = TimeSpan.FromHours(5) + TimeSpan.FromMinutes(1);

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
    /// 計算「不晚於現在」的最近一次每日執行時間。
    /// 用來判斷程式關閉期間是否錯過了今天的喚醒。
    /// </summary>
    public static DateTimeOffset GetPreviousDailyOccurrence(
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

        if (localCandidate > localNow)
        {
            localCandidate = localCandidate.AddDays(-1);
        }

        return CreateLocalDateTimeOffset(localCandidate);
    }

    /// <summary>
    /// 計算自動模式（每 5 小時 1 分鐘）在指定執行完成時間後的下一次執行時間。
    /// 當日完成後若加 5 小時 1 分鐘仍在當日（24:00 前），排在該時間；
    /// 若跨入隔天（超過 24:00），則不排在半夜，直接排定為隔天的首次喚醒時間。
    /// </summary>
    public static DateTimeOffset GetNextAutoIntervalOccurrence(
        TimeSpan initialTimeOfDay,
        DateTimeOffset finishedAt)
    {
        if (initialTimeOfDay < TimeSpan.Zero || initialTimeOfDay >= TimeSpan.FromDays(1))
        {
            throw new ArgumentOutOfRangeException(nameof(initialTimeOfDay), "首次喚醒時間必須介於 00:00 到 23:59。");
        }

        var localFinished = finishedAt.LocalDateTime;
        var candidateLocal = localFinished.Add(AutoInterval);

        // 若推算時間仍在完成當日（未跨過 24:00）
        if (candidateLocal.Date == localFinished.Date)
        {
            return CreateLocalDateTimeOffset(candidateLocal);
        }

        // 跨日：不於半夜執行，排定為隔日的首次喚醒時間
        var nextDayDate = localFinished.Date.AddDays(1);
        var targetTime = new TimeSpan(initialTimeOfDay.Hours, initialTimeOfDay.Minutes, 0);
        var nextDayCandidate = nextDayDate.Add(targetTime);
        return CreateLocalDateTimeOffset(nextDayCandidate);
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

        if (recurrence == ScheduleRecurrence.Interval)
        {
            if (previousOccurrence > now)
            {
                return previousOccurrence;
            }

            var initialTime = previousOccurrence.LocalDateTime.TimeOfDay;
            var next = GetNextAutoIntervalOccurrence(initialTime, previousOccurrence);
            while (next <= now)
            {
                next = GetNextAutoIntervalOccurrence(initialTime, next);
            }
            return next;
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

