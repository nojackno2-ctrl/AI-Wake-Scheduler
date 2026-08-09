namespace AiWakeScheduler.Core;

public static class ScheduleCalculator
{
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

        var nextLocal = previousOccurrence.LocalDateTime;
        do
        {
            nextLocal = nextLocal.AddDays(incrementDays);
        }
        while (new DateTimeOffset(DateTime.SpecifyKind(nextLocal, DateTimeKind.Local)) <= now);

        return new DateTimeOffset(DateTime.SpecifyKind(nextLocal, DateTimeKind.Local));
    }
}
