using System.Text.Json;
using System.Text.Json.Serialization;

namespace AiWakeScheduler.Core;

/// <summary>
/// 共用的 JSON 序列化選項。
/// <see cref="JsonSerializerOptions"/> 內部會快取型別中繼資料，共用單一實例
/// 可讓所有資料檔共享同一份快取，減少記憶體與首次序列化成本。
/// </summary>
internal static class JsonStoreOptions
{
    // 第一次序列化時會自動補上預設的 TypeInfoResolver 並轉為唯讀，之後即可安全地跨執行緒共用。
    public static readonly JsonSerializerOptions Shared = new()
    {
        WriteIndented = true,
        PropertyNameCaseInsensitive = true,
        Converters = { new JsonStringEnumConverter() }
    };
}

/// <summary>
/// 提供 JSON 檔案的安全讀寫與原子寫入機制。
/// </summary>
/// <typeparam name="T">資料模型類別</typeparam>
public sealed class JsonFileStore<T>(string path, Func<T> defaultFactory) : IDataStore<T>, IDisposable where T : class
{
    // 所有具現型別共用同一組選項，避免每個泛型具現各自建立一份序列化中繼資料。
    private static readonly JsonSerializerOptions Options = JsonStoreOptions.Shared;

    private readonly SemaphoreSlim _gate = new(1, 1);
    private bool _disposed;

    /// <summary>
    /// 載入 JSON 資料檔。若檔案不存在或損毀，會自動備份損毀檔並傳回預設值。
    /// </summary>
    public async Task<T> LoadAsync(CancellationToken cancellationToken = default)
    {
        ObjectDisposedException.ThrowIf(_disposed, this);
        await _gate.WaitAsync(cancellationToken).ConfigureAwait(false);
        try
        {
            if (!File.Exists(path))
            {
                return defaultFactory();
            }

            try
            {
                await using var stream = new FileStream(
                    path,
                    FileMode.Open,
                    FileAccess.Read,
                    FileShare.Read,
                    4096,
                    useAsync: true);

                var result = await JsonSerializer.DeserializeAsync<T>(stream, Options, cancellationToken).ConfigureAwait(false);
                return result ?? defaultFactory();
            }
            catch (JsonException ex)
            {
                BackupCorruptedFile(path);
                throw new InvalidDataException($"無法讀取資料檔：{path}", ex);
            }
            catch (NotSupportedException ex)
            {
                BackupCorruptedFile(path);
                throw new InvalidDataException($"無法讀取資料檔：{path}", ex);
            }
        }
        finally
        {
            _gate.Release();
        }
    }

    /// <summary>
    /// 以原子寫入方式（先寫入暫存檔再移動覆蓋）儲存資料。
    /// </summary>
    public async Task SaveAsync(T value, CancellationToken cancellationToken = default)
    {
        ObjectDisposedException.ThrowIf(_disposed, this);
        await _gate.WaitAsync(cancellationToken).ConfigureAwait(false);
        try
        {
            var directory = Path.GetDirectoryName(path)
                ?? throw new InvalidOperationException("資料檔路徑沒有父目錄。");
            Directory.CreateDirectory(directory);

            var temporaryPath = $"{path}.{Guid.NewGuid():N}.tmp";
            try
            {
                await using (var stream = new FileStream(
                    temporaryPath,
                    FileMode.Create,
                    FileAccess.Write,
                    FileShare.None,
                    4096,
                    useAsync: true))
                {
                    await JsonSerializer.SerializeAsync(stream, value, Options, cancellationToken).ConfigureAwait(false);
                    await stream.FlushAsync(cancellationToken).ConfigureAwait(false);
                }

                File.Move(temporaryPath, path, overwrite: true);
            }
            finally
            {
                if (File.Exists(temporaryPath))
                {
                    try { File.Delete(temporaryPath); } catch { }
                }
            }
        }
        finally
        {
            _gate.Release();
        }
    }

    private static void BackupCorruptedFile(string filePath)
    {
        try
        {
            if (File.Exists(filePath))
            {
                var backupPath = $"{filePath}.corrupted.{DateTime.Now:yyyyMMdd-HHmmss}";
                File.Copy(filePath, backupPath, overwrite: true);
            }
        }
        catch
        {
            // Ignore backup failures for corrupted files
        }
    }

    public void Dispose()
    {
        if (!_disposed)
        {
            _disposed = true;
            _gate.Dispose();
        }
    }
}


