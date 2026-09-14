using System.Diagnostics;
using System.Text.RegularExpressions;

namespace AiWakeScheduler.Core;

/// <summary>
/// CLI 重新登入。
///
/// Claude、Codex、Antigravity 都是 OAuth 登入，一定要在瀏覽器按一次授權，
/// 所以這裡只負責判斷「是不是登入失效」與開一個看得見的主控台跑 CLI 自己的登入流程；
/// 本程式不讀寫任何憑證，也不代填帳號密碼。
/// </summary>
public static class CliLoginCommand
{
    private static readonly Regex LoginRequiredPattern = new(
        @"not logged in|logged out|\blog ?in\b|sign ?in|unauthenticated|unauthori[sz]ed|authentication required|auth(?:entication)? token|token (?:has )?expired|refresh token|\b401\b|未登入|請先登入|重新登入|登入憑證",
        RegexOptions.IgnoreCase | RegexOptions.Compiled | RegexOptions.CultureInvariant);

    /// <summary>
    /// 由錯誤訊息判斷是否為登入失效。
    /// 網路被擋、逾時、429 之類的錯誤不能誤判，否則使用者登入完仍然讀不到。
    /// </summary>
    public static bool LooksLikeLoginRequired(string? message) =>
        !string.IsNullOrWhiteSpace(message) && LoginRequiredPattern.IsMatch(message);

    /// <summary>各 CLI 的登入子命令。agy 沒有 login 子命令，直接進互動模式就會走登入流程。</summary>
    public static IReadOnlyList<string> GetArguments(CliKind kind) => kind switch
    {
        CliKind.Claude => ["auth", "login"],
        CliKind.Codex => ["login"],
        CliKind.Antigravity or CliKind.AntigravityClaude => [],
        _ => throw new ArgumentOutOfRangeException(nameof(kind), kind, "此 CLI 不支援重新登入。")
    };

    /// <summary>
    /// 建立登入用的啟動資訊：透過 <c>cmd.exe</c> 開一個看得見的主控台視窗，
    /// 讓 CLI 能顯示授權網址、開瀏覽器並接收使用者輸入。
    /// 登入失敗（結束碼非 0）時暫停，錯誤訊息才不會隨視窗一閃而逝。
    /// </summary>
    public static ProcessStartInfo CreateStartInfo(CliKind kind, string executable, string workingDirectory)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(executable);

        var shellArguments = ProcessRunner.BuildCommandShellArguments(executable, GetArguments(kind));
        var startInfo = new ProcessStartInfo
        {
            FileName = "cmd.exe",
            // BuildCommandShellArguments 以一對外層引號收尾，把 || pause 放進引號內。
            Arguments = string.Concat(shellArguments.AsSpan(0, shellArguments.Length - 1), " || pause\""),
            UseShellExecute = false,
            CreateNoWindow = false
        };

        if (!string.IsNullOrWhiteSpace(workingDirectory) && Directory.Exists(workingDirectory))
        {
            startInfo.WorkingDirectory = workingDirectory;
        }

        return startInfo;
    }
}
