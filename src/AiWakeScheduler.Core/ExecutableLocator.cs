namespace AiWakeScheduler.Core;

public static class ExecutableLocator
{
    public static string? Resolve(CliKind kind, string? configuredValue, string workingDirectory)
    {
        var configured = Environment.ExpandEnvironmentVariables(configuredValue?.Trim() ?? string.Empty);
        if (!string.IsNullOrWhiteSpace(configured))
        {
            var explicitPath = ResolveExplicitPath(configured, workingDirectory);
            if (explicitPath is not null)
            {
                return explicitPath;
            }

            if (!configured.Contains(Path.DirectorySeparatorChar) &&
                !configured.Contains(Path.AltDirectorySeparatorChar))
            {
                var fromPath = FindOnPath(configured);
                if (fromPath is not null)
                {
                    return fromPath;
                }
            }
        }

        foreach (var candidate in KnownCandidates(kind))
        {
            if (File.Exists(candidate))
            {
                return Path.GetFullPath(candidate);
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

    private static string? FindOnPath(string command)
    {
        var extensions = Path.HasExtension(command)
            ? [string.Empty]
            : GetPathExtensions();

        foreach (var directory in (Environment.GetEnvironmentVariable("PATH") ?? string.Empty)
                     .Split(Path.PathSeparator, StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries))
        {
            foreach (var extension in extensions)
            {
                try
                {
                    var candidate = Path.Combine(directory.Trim('"'), command + extension);
                    if (File.Exists(candidate))
                    {
                        return Path.GetFullPath(candidate);
                    }
                }
                catch (Exception) when (directory.Length > 0)
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

