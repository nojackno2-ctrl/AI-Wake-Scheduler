using System.Collections.Concurrent;

namespace AiWakeScheduler.Core;

/// <summary>
/// 搜尋與定位 CLI 可執行檔路徑。
///
/// 候選清單來自 <see cref="CliCatalog"/>；解析結果會快取，
/// 避免每次執行或探測都重新遞迴掃描 LocalAppData / Program Files。
/// </summary>
public static class ExecutableLocator
{
    private static readonly ConcurrentDictionary<(CliKind Kind, string Configured, string WorkingDirectory), string> Cache = new();

    /// <summary>
    /// 解析指定 CLI 種類的可執行檔路徑，找不到時傳回 null。
    /// </summary>
    public static string? Resolve(CliKind kind, string? configuredValue, string workingDirectory)
    {
        var configured = configuredValue?.Trim() ?? string.Empty;
        var directory = workingDirectory ?? string.Empty;
        var key = (kind, configured, directory);

        // 快取命中後仍確認檔案存在：CLI 自動更新換到新的雜湊目錄時會自然失效。
        if (Cache.TryGetValue(key, out var cached) && File.Exists(cached))
        {
            return cached;
        }

        var resolved = ResolveCore(kind, configured, directory);
        if (resolved is not null)
        {
            Cache[key] = resolved;
        }
        else
        {
            Cache.TryRemove(key, out _);
        }

        return resolved;
    }

    /// <summary>清除路徑快取（使用者變更設定或手動重新探測時使用）。</summary>
    public static void ClearCache() => Cache.Clear();

    private static string? ResolveCore(CliKind kind, string configured, string workingDirectory)
    {
        var descriptor = CliCatalog.Get(kind);
        var configuredIsDefaultCommand = IsDefaultCommand(configured, descriptor.DefaultCommand);

        if (!string.IsNullOrWhiteSpace(configured))
        {
            var expanded = Environment.ExpandEnvironmentVariables(configured);

            var explicitPath = ResolveExplicitPath(expanded, workingDirectory);
            if (explicitPath is not null)
            {
                return explicitPath;
            }

            if (!expanded.Contains(Path.DirectorySeparatorChar) &&
                !expanded.Contains(Path.AltDirectorySeparatorChar) &&
                !configuredIsDefaultCommand)
            {
                var fromPath = FindOnPath(expanded);
                if (fromPath is not null)
                {
                    return fromPath;
                }
            }
        }

        foreach (var candidate in descriptor.ExecutableCandidates(ExecutableSearchPaths.Current))
        {
            try
            {
                if (File.Exists(candidate))
                {
                    return Path.GetFullPath(candidate);
                }
            }
            catch
            {
                // 忽略不合法的候選路徑
            }
        }

        // WindowsApps 中的 Codex app execution alias 可能看似存在但無法由一般程序啟動。
        // 使用預設命令名稱時，必須先檢查 Desktop-managed 安裝位置；只有找不到才回退 PATH。
        if (!string.IsNullOrWhiteSpace(configured) &&
            !configured.Contains(Path.DirectorySeparatorChar) &&
            !configured.Contains(Path.AltDirectorySeparatorChar))
        {
            var configuredFromPath = FindOnPath(configured);
            if (configuredFromPath is not null)
            {
                return configuredFromPath;
            }
        }

        return FindOnPath(descriptor.DefaultCommand);
    }

    private static bool IsDefaultCommand(string configured, string defaultCommand)
    {
        if (string.IsNullOrWhiteSpace(configured))
        {
            return false;
        }

        var fileName = Path.GetFileNameWithoutExtension(configured);
        return string.Equals(fileName, defaultCommand, StringComparison.OrdinalIgnoreCase);
    }

    private static string? ResolveExplicitPath(string value, string workingDirectory)
    {
        try
        {
            if (Path.IsPathRooted(value))
            {
                return File.Exists(value) ? Path.GetFullPath(value) : null;
            }

            if (!string.IsNullOrWhiteSpace(workingDirectory))
            {
                var combined = Path.Combine(workingDirectory, value);
                if (File.Exists(combined))
                {
                    return Path.GetFullPath(combined);
                }
            }

            return null;
        }
        catch
        {
            return null;
        }
    }

    private static string? FindOnPath(string command)
    {
        var pathEnv = Environment.GetEnvironmentVariable("PATH");
        if (string.IsNullOrWhiteSpace(pathEnv))
        {
            return null;
        }

        var extensions = Path.HasExtension(command) ? [string.Empty] : GetPathExtensions();
        var directories = pathEnv.Split(
            Path.PathSeparator,
            StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);

        foreach (var rawDirectory in directories)
        {
            var directory = rawDirectory.Trim('"');
            if (directory.Length == 0)
            {
                continue;
            }

            foreach (var extension in extensions)
            {
                try
                {
                    var candidate = Path.Combine(directory, command + extension);
                    if (File.Exists(candidate))
                    {
                        return Path.GetFullPath(candidate);
                    }
                }
                catch
                {
                    // 忽略格式錯誤或無法存取的 PATH 項目，繼續搜尋
                }
            }
        }

        return null;
    }

    private static string[] GetPathExtensions()
    {
        if (!OperatingSystem.IsWindows())
        {
            return [string.Empty];
        }

        var configured = Environment.GetEnvironmentVariable("PATHEXT");
        return string.IsNullOrWhiteSpace(configured)
            ? [".exe", ".cmd", ".bat"]
            : configured.Split(';', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
    }
}
