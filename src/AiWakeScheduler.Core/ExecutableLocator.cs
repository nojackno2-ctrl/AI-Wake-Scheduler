namespace AiWakeScheduler.Core;

/// <summary>
/// 負責搜尋與定位 CLI 可執行檔路徑。
/// </summary>
public static class ExecutableLocator
{
    /// <summary>
    /// 解析指定 CLI 種類的可執行檔路徑。
    /// </summary>
    public static string? Resolve(CliKind kind, string? configuredValue, string workingDirectory)
    {
        var expanded = string.IsNullOrWhiteSpace(configuredValue)
            ? string.Empty
            : Environment.ExpandEnvironmentVariables(configuredValue.Trim());

        if (!string.IsNullOrWhiteSpace(expanded))
        {
            var explicitPath = ResolveExplicitPath(expanded, workingDirectory);
            if (explicitPath is not null)
            {
                return explicitPath;
            }

            if (!expanded.Contains(Path.DirectorySeparatorChar) &&
                !expanded.Contains(Path.AltDirectorySeparatorChar))
            {
                var fromPath = FindOnPath(expanded);
                if (fromPath is not null)
                {
                    return fromPath;
                }
            }
        }

        foreach (var candidate in KnownCandidates(kind))
        {
            try
            {
                if (File.Exists(candidate))
                {
                    return Path.GetFullPath(candidate);
                }
            }
            catch (Exception)
            {
                // Ignore invalid candidate paths
            }
        }

        return FindOnPath(kind switch
        {
            CliKind.Antigravity => "agy",
            CliKind.Codex => "codex",
            CliKind.Claude => "claude",
            _ => throw new ArgumentOutOfRangeException(nameof(kind), kind, null)
        });
    }

    private static string? ResolveExplicitPath(string value, string workingDirectory)
    {
        try
        {
            if (Path.IsPathRooted(value))
            {
                return File.Exists(value) ? Path.GetFullPath(value) : null;
            }

            if (!value.Contains(Path.DirectorySeparatorChar) && !value.Contains(Path.AltDirectorySeparatorChar))
            {
                return null;
            }

            var combined = Path.Combine(workingDirectory, value);
            return File.Exists(combined) ? Path.GetFullPath(combined) : null;
        }
        catch (Exception)
        {
            return null;
        }
    }

    private static string? FindOnPath(string command)
    {
        var extensions = Path.HasExtension(command)
            ? [string.Empty]
            : GetPathExtensions();

        var pathEnv = Environment.GetEnvironmentVariable("PATH");
        if (string.IsNullOrWhiteSpace(pathEnv))
        {
            return null;
        }

        var directories = pathEnv.Split(Path.PathSeparator, StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
        foreach (var rawDir in directories)
        {
            var directory = rawDir.Trim('"');
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
                catch (Exception)
                {
                    // Ignore malformed or inaccessible PATH entries and continue searching.
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

    private static IEnumerable<string> KnownCandidates(CliKind kind)
    {
        var localAppData = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
        var profile = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);

        return kind switch
        {
            CliKind.Antigravity =>
            [
                Path.Combine(localAppData, "agy", "bin", "agy.exe")
            ],
            CliKind.Codex =>
            [
                Path.Combine(localAppData, "OpenAI", "Codex", "bin", "codex.exe"),
                Path.Combine(localAppData, "Programs", "Codex", "codex.exe")
            ],
            CliKind.Claude =>
            [
                Path.Combine(profile, ".local", "bin", "claude.exe")
            ],
            _ => throw new ArgumentOutOfRangeException(nameof(kind), kind, null)
        };
    }
}


