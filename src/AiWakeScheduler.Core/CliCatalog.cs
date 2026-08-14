namespace AiWakeScheduler.Core;

/// <summary>
/// 常用安裝根目錄的快取。
/// <see cref="Environment.GetFolderPath"/> 每次呼叫都會進 Shell API，
/// 這裡只在第一次使用時取一次，之後所有 CLI 搜尋共用同一份結果。
/// </summary>
public sealed class ExecutableSearchPaths
{
    public static ExecutableSearchPaths Current { get; } = new();

    private ExecutableSearchPaths()
    {
        LocalAppData = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
        AppData = Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData);
        UserProfile = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
        ProgramFiles = Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles);
        ProgramFilesX86 = Environment.GetFolderPath(Environment.SpecialFolder.ProgramFilesX86);
    }

    public string LocalAppData { get; }
    public string AppData { get; }
    public string UserProfile { get; }
    public string ProgramFiles { get; }
    public string ProgramFilesX86 { get; }

    /// <summary>
    /// 在指定目錄下尋找檔案，依最後寫入時間由新到舊排序（優先命中最新版本的雜湊子目錄）。
    /// </summary>
    public IEnumerable<string> Search(string baseDirectory, string fileName, int maxDepth = 2)
    {
        if (string.IsNullOrWhiteSpace(baseDirectory) || !Directory.Exists(baseDirectory))
        {
            return [];
        }

        try
        {
            var options = new EnumerationOptions
            {
                RecurseSubdirectories = true,
                MaxRecursionDepth = maxDepth,
                IgnoreInaccessible = true,
                MatchCasing = MatchCasing.CaseInsensitive,
                ReturnSpecialDirectories = false
            };
            return Directory.EnumerateFiles(baseDirectory, fileName, options)
                .Select(path => new FileInfo(path))
                .OrderByDescending(info => info.LastWriteTimeUtc)
                .Select(info => info.FullName)
                .ToArray();
        }
        catch
        {
            return [];
        }
    }
}

/// <summary>
/// 單一 CLI 的完整描述：顯示名稱、預設命令、參數計畫與可執行檔候選路徑。
///
/// 這是「某個 CLI 長什麼樣」的唯一知識來源。參數建構、路徑搜尋與 UI 顯示
/// 都只依賴這份目錄，不再各自維護一份 switch，新增一個 CLI 只需新增一筆描述。
/// </summary>
public sealed record CliDescriptor
{
    public required CliKind Kind { get; init; }

    /// <summary>設定與結果視窗使用的完整名稱。</summary>
    public required string DisplayName { get; init; }

    /// <summary>排程清單欄位使用的短名稱。</summary>
    public required string ShortName { get; init; }

    /// <summary>找不到自訂路徑時，最後回退到 PATH 搜尋的命令名稱。</summary>
    public required string DefaultCommand { get; init; }

    /// <summary>永遠加入的基本參數（子命令必須排在第一個）。</summary>
    public required IReadOnlyList<string> BaseArguments { get; init; }

    /// <summary>
    /// 提示詞要附加在哪個旗標之後。
    ///
    /// null 表示提示詞是結尾的位置參數（Codex、Claude CLI）。
    /// Antigravity 的 <c>--print</c> 是「值就是提示詞」的字串旗標
    /// （單獨給會得到 <c>flag needs an argument: -print</c>），
    /// 而且 Go 的 flag 套件遇到第一個非旗標參數就停止解析，
    /// 所以它必須排在所有旗標的最後面。
    /// </summary>
    public string? PromptFlag { get; init; }

    /// <summary>
    /// 節省 Token 模式追加的參數。
    /// 目標是移除請求中最肥的部分：MCP 工具結構描述、專案說明檔、技能與外掛，
    /// 並把推理與輸出量壓到最低。
    /// </summary>
    public required IReadOnlyList<string> TokenSaverArguments { get; init; }

    /// <summary>使用者未自訂 --model 時採用的預設模型（null 表示交給 CLI 自行決定）。</summary>
    public string? DefaultModel { get; init; }

    /// <summary>UI 下拉選單推薦的最新模型清單（首項為空代表 CLI 預設，亦可自訂輸入）。</summary>
    public IReadOnlyList<string> PresetModels { get; init; } = [];

    /// <summary>此 CLI 支援的思考程度 / 推理強度選項。</summary>
    public IReadOnlyList<ThinkingEffort> SupportedEfforts { get; init; } = [];

    /// <summary>已知模型各自支援的思考程度；自訂模型則回退到 <see cref="SupportedEfforts"/>。</summary>
    public IReadOnlyDictionary<string, IReadOnlyList<ThinkingEffort>> ModelSupportedEfforts { get; init; } =
        new Dictionary<string, IReadOnlyList<ThinkingEffort>>(StringComparer.OrdinalIgnoreCase);

    public IReadOnlyList<ThinkingEffort> GetSupportedEfforts(string? model)
    {
        var normalizedModel = model?.Trim();
        return !string.IsNullOrWhiteSpace(normalizedModel) &&
               ModelSupportedEfforts.TryGetValue(normalizedModel, out var efforts)
            ? efforts
            : SupportedEfforts;
    }

    /// <summary>把舊設定或不相容的 Effort 降到該模型可接受的最接近等級。</summary>
    public ThinkingEffort NormalizeEffort(string? model, ThinkingEffort effort)
    {
        var supported = GetSupportedEfforts(model);
        if (supported.Contains(effort))
        {
            return effort;
        }

        var requestedRank = EffortRank(effort);
        var fallback = ThinkingEffort.Default;
        var fallbackRank = int.MinValue;
        for (var i = 0; i < supported.Count; i++)
        {
            var candidate = supported[i];
            if (candidate == ThinkingEffort.Default)
            {
                continue;
            }

            var candidateRank = EffortRank(candidate);
            if (candidateRank <= requestedRank && candidateRank > fallbackRank)
            {
                fallback = candidate;
                fallbackRank = candidateRank;
            }
        }

        if (fallback != ThinkingEffort.Default)
        {
            return fallback;
        }

        return supported.FirstOrDefault(candidate => candidate != ThinkingEffort.Default);
    }

    private static int EffortRank(ThinkingEffort effort) => effort switch
    {
        ThinkingEffort.Minimal => 0,
        ThinkingEffort.Low => 1,
        ThinkingEffort.Medium => 2,
        ThinkingEffort.High => 3,
        ThinkingEffort.XHigh => 4,
        ThinkingEffort.Max => 5,
        ThinkingEffort.Ultra => 6,
        _ => -1
    };

    /// <summary>依應用程式逾時值產生 CLI 自身的逾時參數，讓子程序自己收尾而不是被強制終止。</summary>
    public Func<TimeSpan, IReadOnlyList<string>>? TimeoutArguments { get; init; }

    /// <summary>已知安裝位置的候選路徑（依優先順序）。</summary>
    public required Func<ExecutableSearchPaths, IEnumerable<string>> ExecutableCandidates { get; init; }
}

/// <summary>
/// 所有支援 CLI 的描述目錄。
/// </summary>
public static class CliCatalog
{
    private static readonly CliDescriptor[] Descriptors =
    [
        new()
        {
            Kind = CliKind.Antigravity,
            DisplayName = "Antigravity (Gemini)",
            ShortName = "AGY(Gemini)",
            DefaultCommand = "agy",
            BaseArguments = [],
            PromptFlag = "--print",
            // --disable-slash-commands 停用技能展開；--mode plan 唯讀工作區；
            // 推理程度由 CliCommandBuilder 依設定或節省模式動態附加。
            TokenSaverArguments = ["--disable-slash-commands", "--mode", "plan"],
            // 以 `agy models` 實際輸出核對：基底模型名稱需搭配獨立的 --effort 旗標
            // （帶後綴的完整 ID，如 gemini-3.7-flash-high，是另一種寫法，這裡固定用前者）。
            PresetModels = ["", "gemini-3.7-flash", "gemini-3.6-flash", "gemini-3.1-pro"],
            SupportedEfforts = [ThinkingEffort.Default, ThinkingEffort.Low, ThinkingEffort.Medium, ThinkingEffort.High],
            ModelSupportedEfforts = new Dictionary<string, IReadOnlyList<ThinkingEffort>>(StringComparer.OrdinalIgnoreCase)
            {
                ["gemini-3.7-flash"] = [ThinkingEffort.Default, ThinkingEffort.Low, ThinkingEffort.Medium, ThinkingEffort.High],
                ["gemini-3.6-flash"] = [ThinkingEffort.Default, ThinkingEffort.Low, ThinkingEffort.Medium, ThinkingEffort.High],
                ["gemini-3.1-pro"] = [ThinkingEffort.Default, ThinkingEffort.Low, ThinkingEffort.High]
            },
            TimeoutArguments = timeout => ["--print-timeout", FormatGoDuration(timeout)],
            ExecutableCandidates = AntigravityCandidates
        },
        new()
        {
            Kind = CliKind.AntigravityClaude,
            DisplayName = "Antigravity (Claude / GPT)",
            ShortName = "AGY(Claude)",
            DefaultCommand = "agy",
            BaseArguments = [],
            PromptFlag = "--print",
            TokenSaverArguments = ["--disable-slash-commands", "--mode", "plan"],
            // 以 `agy models` 實際輸出核對的模型 ID（非顯示名稱）：
            // claude-sonnet-4-6、claude-opus-4-6-thinking、gpt-oss-120b-medium。
            // 這三個模型都不接受獨立的 --effort 旗標（agy 會直接報錯拒絕），
            // 因此本設定檔不提供思考程度選項。
            DefaultModel = "claude-sonnet-4-6",
            PresetModels = ["claude-sonnet-4-6", "claude-opus-4-6-thinking", "gpt-oss-120b-medium"],
            SupportedEfforts = [ThinkingEffort.Default],
            TimeoutArguments = timeout => ["--print-timeout", FormatGoDuration(timeout)],
            ExecutableCandidates = AntigravityCandidates
        },
        new()
        {
            Kind = CliKind.Codex,
            DisplayName = "Codex CLI",
            ShortName = "Codex",
            DefaultCommand = "codex",
            // --ephemeral 不寫入 session 檔（省磁碟與記憶體）、--color never 讓擷取到的輸出不含 ANSI 控制碼。
            BaseArguments = ["exec", "--skip-git-repo-check", "--ephemeral", "--color", "never"],
            // --ignore-user-config 會跳過 config.toml，連帶不載入任何 MCP 伺服器與自訂指示，
            // 這是 Codex 這一側最大的 Token 節省來源（驗證通過：認證仍走 CODEX_HOME，不受影響）。
            TokenSaverArguments =
            [
                "--sandbox", "read-only",
                "--ignore-user-config",
                "--ignore-rules",
                "-c", "model_verbosity=\"low\""
            ],
            // 2026-08-14 由本機 Codex App Server `model/list` 直接取得的帳號可用清單。
            // 空字串保留「由 CLI 自動選擇目前預設模型」的行為，避免未來再次被固定版本綁住。
            PresetModels = ["", "gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna", "gpt-5.5", "gpt-5.4", "gpt-5.4-mini"],
            SupportedEfforts = [ThinkingEffort.Default, ThinkingEffort.Low, ThinkingEffort.Medium, ThinkingEffort.High, ThinkingEffort.XHigh, ThinkingEffort.Max, ThinkingEffort.Ultra],
            ModelSupportedEfforts = new Dictionary<string, IReadOnlyList<ThinkingEffort>>(StringComparer.OrdinalIgnoreCase)
            {
                ["gpt-5.6-sol"] = [ThinkingEffort.Default, ThinkingEffort.Low, ThinkingEffort.Medium, ThinkingEffort.High, ThinkingEffort.XHigh, ThinkingEffort.Max, ThinkingEffort.Ultra],
                ["gpt-5.6-terra"] = [ThinkingEffort.Default, ThinkingEffort.Low, ThinkingEffort.Medium, ThinkingEffort.High, ThinkingEffort.XHigh, ThinkingEffort.Max, ThinkingEffort.Ultra],
                ["gpt-5.6-luna"] = [ThinkingEffort.Default, ThinkingEffort.Low, ThinkingEffort.Medium, ThinkingEffort.High, ThinkingEffort.XHigh, ThinkingEffort.Max],
                ["gpt-5.5"] = [ThinkingEffort.Default, ThinkingEffort.Low, ThinkingEffort.Medium, ThinkingEffort.High, ThinkingEffort.XHigh],
                ["gpt-5.4"] = [ThinkingEffort.Default, ThinkingEffort.Low, ThinkingEffort.Medium, ThinkingEffort.High, ThinkingEffort.XHigh],
                ["gpt-5.4-mini"] = [ThinkingEffort.Default, ThinkingEffort.Low, ThinkingEffort.Medium, ThinkingEffort.High, ThinkingEffort.XHigh]
            },
            ExecutableCandidates = CodexCandidates
        },
        new()
        {
            Kind = CliKind.Claude,
            DisplayName = "Claude CLI",
            ShortName = "Claude",
            DefaultCommand = "claude",
            BaseArguments = ["--print"],
            // --safe-mode 停用 CLAUDE.md、技能、外掛、hooks 與 MCP 伺服器（認證與內建工具照常運作）；
            // --strict-mcp-config 再次確保不載入任何 MCP 工具結構描述；
            // --tools "" 移除全部內建工具定義。三者合計可拿掉請求中絕大多數的固定輸入。
            // 注意：--tools 是可變長度參數，後面必須緊接一個旗標，否則會吃掉下一個位置參數。
            TokenSaverArguments =
            [
                "--safe-mode",
                "--strict-mcp-config",
                "--tools", "",
                "--no-session-persistence",
                "--prompt-suggestions", "false"
            ],
            // 用官方文件的別名（`claude --help` 明載：sonnet/opus/fable 皆代表「最新版」），
            // 而非寫死版本號的完整模型 ID，避免新一代模型上市後這裡的清單直接失效。
            PresetModels = ["", "sonnet", "opus", "fable"],
            SupportedEfforts = [ThinkingEffort.Default, ThinkingEffort.Low, ThinkingEffort.Medium, ThinkingEffort.High, ThinkingEffort.XHigh, ThinkingEffort.Max],
            ExecutableCandidates = ClaudeCandidates
        }
    ];

    private static readonly Dictionary<CliKind, CliDescriptor> Index =
        Descriptors.ToDictionary(descriptor => descriptor.Kind);

    public static IReadOnlyList<CliDescriptor> All => Descriptors;

    public static CliDescriptor Get(CliKind kind) => Index.TryGetValue(kind, out var descriptor)
        ? descriptor
        : throw new ArgumentOutOfRangeException(nameof(kind), kind, null);

    /// <summary>將 TimeSpan 轉為 Go 風格的期間字串（Antigravity CLI 使用）。</summary>
    private static string FormatGoDuration(TimeSpan timeout)
    {
        var seconds = (long)Math.Ceiling(timeout.TotalSeconds);
        return $"{Math.Max(seconds, 1)}s";
    }

    private static IEnumerable<string> AntigravityCandidates(ExecutableSearchPaths paths)
    {
        yield return Path.Combine(paths.LocalAppData, "agy", "bin", "agy.exe");

        foreach (var exe in paths.Search(Path.Combine(paths.LocalAppData, "agy", "bin"), "agy.exe"))
        {
            yield return exe;
        }

        yield return Path.Combine(paths.LocalAppData, "Programs", "agy", "bin", "agy.exe");
        yield return Path.Combine(paths.LocalAppData, "Programs", "agy", "agy.exe");
        yield return Path.Combine(paths.LocalAppData, "Programs", "Antigravity", "bin", "agy.exe");
        yield return Path.Combine(paths.LocalAppData, "Programs", "Antigravity", "agy.exe");
        yield return Path.Combine(paths.UserProfile, ".local", "bin", "agy.exe");
        yield return Path.Combine(paths.UserProfile, ".antigravity", "bin", "agy.exe");
        yield return Path.Combine(paths.UserProfile, ".gemini", "antigravity", "bin", "agy.exe");
        yield return Path.Combine(paths.AppData, "npm", "agy.cmd");
        yield return Path.Combine(paths.AppData, "npm", "agy.exe");
    }

    private static IEnumerable<string> CodexCandidates(ExecutableSearchPaths paths)
    {
        // 1. OpenAI Codex bin 子目錄（雜湊目錄，取最新版本）
        foreach (var exe in paths.Search(Path.Combine(paths.LocalAppData, "OpenAI", "Codex", "bin"), "codex.exe"))
        {
            yield return exe;
        }

        yield return Path.Combine(paths.LocalAppData, "OpenAI", "Codex", "bin", "codex.exe");
        yield return Path.Combine(paths.LocalAppData, "OpenAI", "Codex", "codex.exe");

        // 2. Programs 目錄中的 OpenAI Codex
        foreach (var exe in paths.Search(Path.Combine(paths.LocalAppData, "Programs", "OpenAI", "Codex"), "codex.exe"))
        {
            yield return exe;
        }
        yield return Path.Combine(paths.LocalAppData, "Programs", "OpenAI", "Codex", "bin", "codex.exe");
        yield return Path.Combine(paths.LocalAppData, "Programs", "OpenAI", "Codex", "codex.exe");

        // 3. Programs / Codex 歷史路徑
        foreach (var exe in paths.Search(Path.Combine(paths.LocalAppData, "Programs", "Codex"), "codex.exe"))
        {
            yield return exe;
        }
        yield return Path.Combine(paths.LocalAppData, "Programs", "Codex", "codex.exe");
        yield return Path.Combine(paths.LocalAppData, "Programs", "Codex", "bin", "codex.exe");

        // 4. 使用者家目錄與 Local bin
        yield return Path.Combine(paths.UserProfile, ".local", "bin", "codex.exe");
        yield return Path.Combine(paths.UserProfile, ".local", "bin", "codex.cmd");
        yield return Path.Combine(paths.UserProfile, ".codex", "bin", "codex.exe");

        // 5. npm 全域安裝
        yield return Path.Combine(paths.AppData, "npm", "codex.cmd");
        yield return Path.Combine(paths.AppData, "npm", "codex.exe");

        // 6. Program Files 安裝路徑
        foreach (var root in new[] { paths.ProgramFiles, paths.ProgramFilesX86 })
        {
            if (string.IsNullOrWhiteSpace(root))
            {
                continue;
            }
            foreach (var exe in paths.Search(Path.Combine(root, "OpenAI", "Codex"), "codex.exe"))
            {
                yield return exe;
            }
            yield return Path.Combine(root, "OpenAI", "Codex", "codex.exe");
            yield return Path.Combine(root, "OpenAI", "Codex", "bin", "codex.exe");
        }
    }

    private static IEnumerable<string> ClaudeCandidates(ExecutableSearchPaths paths)
    {
        yield return Path.Combine(paths.UserProfile, ".local", "bin", "claude.exe");
        yield return Path.Combine(paths.UserProfile, ".local", "bin", "claude.cmd");
        yield return Path.Combine(paths.LocalAppData, "Programs", "Claude", "claude.exe");
        yield return Path.Combine(paths.LocalAppData, "Claude", "bin", "claude.exe");

        foreach (var exe in paths.Search(Path.Combine(paths.LocalAppData, "Programs", "Claude"), "claude.exe"))
        {
            yield return exe;
        }

        yield return Path.Combine(paths.AppData, "npm", "claude.cmd");
        yield return Path.Combine(paths.AppData, "npm", "claude.exe");
    }
}
