namespace AiWakeScheduler.Core;

/// <summary>
/// 單次 CLI 呼叫的即時活動種類，供 UI 端的終端機面板顯示。
/// </summary>
public enum CliActivityKind
{
    Started,
    Output,
    ErrorOutput,
    Completed,
    Failed
}

/// <summary>
/// 單筆即時活動事件。<see cref="Text"/> 的意義依 <see cref="Kind"/> 而定：
/// Started/Completed/Failed 是一句摘要，Output/ErrorOutput 是子程序輸出的原始片段
/// （可能不是完整一行，UI 端自行拼接）。
/// </summary>
public sealed record CliActivityEvent(
    CliKind Cli,
    string JobName,
    CliActivityKind Kind,
    string Text,
    DateTimeOffset Timestamp);
