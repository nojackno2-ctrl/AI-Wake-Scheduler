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
            CliKind.Antigravity or CliKind.AntigravityClaude => "agy",
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
        var appData = Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData);
        var profile = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
        var programFiles = Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles);
        var programFilesX86 = Environment.GetFolderPath(Environment.SpecialFolder.ProgramFilesX86);

        switch (kind)
        {
            case CliKind.Antigravity:
            case CliKind.AntigravityClaude:
            {
                yield return Path.Combine(localAppData, "agy", "bin", "agy.exe");

                var agyBin = Path.Combine(localAppData, "agy", "bin");
                foreach (var exe in FindInSubdirectories(agyBin, "agy.exe", maxDepth: 2))
                {
                    yield return exe;
                }

                yield return Path.Combine(localAppData, "Programs", "agy", "bin", "agy.exe");
                yield return Path.Combine(localAppData, "Programs", "agy", "agy.exe");
                yield return Path.Combine(localAppData, "Programs", "Antigravity", "bin", "agy.exe");
                yield return Path.Combine(localAppData, "Programs", "Antigravity", "agy.exe");
                yield return Path.Combine(profile, ".local", "bin", "agy.exe");
                yield return Path.Combine(profile, ".antigravity", "bin", "agy.exe");
                yield return Path.Combine(profile, ".gemini", "antigravity", "bin", "agy.exe");
                yield return Path.Combine(appData, "npm", "agy.cmd");
                yield return Path.Combine(appData, "npm", "agy.exe");
                break;
            }
            case CliKind.Codex:
            {
                // 1. OpenAI Codex bin 子目錄（按最後修改時間遞減，支援 cfac6bda2d141e07 等雜湊目錄）
                var openAiCodexBin = Path.Combine(localAppData, "OpenAI", "Codex", "bin");
                foreach (var exe in FindInSubdirectories(openAiCodexBin, "codex.exe", maxDepth: 2))
                {
                    yield return exe;
                }

                // 2. OpenAI Codex 根目錄與直屬 bin
                yield return Path.Combine(localAppData, "OpenAI", "Codex", "bin", "codex.exe");
                yield return Path.Combine(localAppData, "OpenAI", "Codex", "codex.exe");

                // 3. Programs 目錄中的 OpenAI Codex
                var programsOpenAiCodex = Path.Combine(localAppData, "Programs", "OpenAI", "Codex");
                foreach (var exe in FindInSubdirectories(programsOpenAiCodex, "codex.exe", maxDepth: 2))
                {
                    yield return exe;
                }
                yield return Path.Combine(localAppData, "Programs", "OpenAI", "Codex", "bin", "codex.exe");
                yield return Path.Combine(localAppData, "Programs", "OpenAI", "Codex", "codex.exe");

                // 4. Programs / Codex 歷史路徑
                var programsCodex = Path.Combine(localAppData, "Programs", "Codex");
                foreach (var exe in FindInSubdirectories(programsCodex, "codex.exe", maxDepth: 2))
                {
                    yield return exe;
                }
                yield return Path.Combine(localAppData, "Programs", "Codex", "codex.exe");
                yield return Path.Combine(localAppData, "Programs", "Codex", "bin", "codex.exe");

                // 5. 使用者家目錄與 Local bin
                yield return Path.Combine(profile, ".local", "bin", "codex.exe");
                yield return Path.Combine(profile, ".local", "bin", "codex.cmd");
                yield return Path.Combine(profile, ".codex", "bin", "codex.exe");

                // 6. npm 全域安裝
                yield return Path.Combine(appData, "npm", "codex.cmd");
                yield return Path.Combine(appData, "npm", "codex.exe");

                // 7. Program Files 安裝路徑
                if (!string.IsNullOrWhiteSpace(programFiles))
                {
                    var pfCodex = Path.Combine(programFiles, "OpenAI", "Codex");
                    foreach (var exe in FindInSubdirectories(pfCodex, "codex.exe", maxDepth: 2))
                    {
                        yield return exe;
                    }
                    yield return Path.Combine(programFiles, "OpenAI", "Codex", "codex.exe");
                    yield return Path.Combine(programFiles, "OpenAI", "Codex", "bin", "codex.exe");
                }
                if (!string.IsNullOrWhiteSpace(programFilesX86))
                {
                    var pfx86Codex = Path.Combine(programFilesX86, "OpenAI", "Codex");
                    foreach (var exe in FindInSubdirectories(pfx86Codex, "codex.exe", maxDepth: 2))
                    {
                        yield return exe;
                    }
                    yield return Path.Combine(programFilesX86, "OpenAI", "Codex", "codex.exe");
                    yield return Path.Combine(programFilesX86, "OpenAI", "Codex", "bin", "codex.exe");
                }
                break;
            }
            case CliKind.Claude:
            {
                yield return Path.Combine(profile, ".local", "bin", "claude.exe");
                yield return Path.Combine(profile, ".local", "bin", "claude.cmd");
                yield return Path.Combine(localAppData, "Programs", "Claude", "claude.exe");
                yield return Path.Combine(localAppData, "Claude", "bin", "claude.exe");

                var claudePrograms = Path.Combine(localAppData, "Programs", "Claude");
                foreach (var exe in FindInSubdirectories(claudePrograms, "claude.exe", maxDepth: 2))
                {
                    yield return exe;
                }

                yield return Path.Combine(appData, "npm", "claude.cmd");
                yield return Path.Combine(appData, "npm", "claude.exe");
                break;
            }
            default:
                throw new ArgumentOutOfRangeException(nameof(kind), kind, null);
        }
    }

    private static IEnumerable<string> FindInSubdirectories(string baseDirectory, string fileName, int maxDepth = 2)
    {
        if (string.IsNullOrWhiteSpace(baseDirectory) || !Directory.Exists(baseDirectory))
        {
            yield break;
        }

        List<string> matches;
        try
        {
            var searchOption = new EnumerationOptions
            {
                RecurseSubdirectories = true,
                MaxRecursionDepth = maxDepth,
                IgnoreInaccessible = true,
                MatchCasing = MatchCasing.CaseInsensitive,
                ReturnSpecialDirectories = false
            };
            matches = Directory.EnumerateFiles(baseDirectory, fileName, searchOption)
                .Select(path => new FileInfo(path))
                .OrderByDescending(fi => fi.LastWriteTimeUtc)
                .Select(fi => fi.FullName)
                .ToList();
        }
        catch
        {
            yield break;
        }

        foreach (var match in matches)
        {
            yield return match;
        }
    }
}


