namespace AiWakeScheduler.Core;

/// <summary>
/// 資料持久化的抽象。排程管理器只依賴這個介面，
/// 不需要知道資料實際上是 JSON 檔、記憶體還是資料庫。
/// </summary>
public interface IDataStore<T> where T : class
{
    Task<T> LoadAsync(CancellationToken cancellationToken = default);

    Task SaveAsync(T value, CancellationToken cancellationToken = default);
}

/// <summary>
/// CLI 執行的抽象。排程管理器只依賴這個介面，
/// 測試可用假實作取代，不必真的啟動子程序。
/// </summary>
public interface ICliRunner
{
    Task<CliRunResult> RunAsync(
        CliKind kind,
        CliProfile profile,
        string message,
        string workingDirectory,
        TimeSpan timeout,
        bool tokenSaverMode = true,
        CancellationToken cancellationToken = default);

    Task<CliProbeResult> ProbeAsync(
        CliKind kind,
        CliProfile profile,
        string workingDirectory,
        CancellationToken cancellationToken = default);
}

/// <summary>
/// CLI 額度與倒數讀取的抽象。排程管理器可藉此判斷個別 CLI 是否仍在倒數中。
/// </summary>
public interface ICliUsageReader
{
    Task<CliUsageSnapshot> ReadAsync(
        CliKind kind,
        CliProfile profile,
        string workingDirectory,
        CancellationToken cancellationToken = default);

    CliUsageSnapshot? GetLatestSnapshot(CliKind kind) => null;

    IReadOnlyDictionary<CliKind, CliUsageSnapshot> GetLatestSnapshots() =>
        new Dictionary<CliKind, CliUsageSnapshot>();
}
