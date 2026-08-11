namespace AiWakeScheduler.WinForms;

/// <summary>
/// 全應用程式共用的字型與色彩。
///
/// 字型是 GDI 控制代碼，每個視窗各自 new 一份會重複佔用資源，
/// 而且原本的實作沒有釋放。這裡集中建立、集中釋放，
/// 兩個視窗共用同一組實例。
/// </summary>
internal static class AppTheme
{
    private const string FamilyName = "Microsoft JhengHei UI";

    public static readonly Font Body = Create(9F, FontStyle.Regular);
    public static readonly Font SectionTitle = Create(12F, FontStyle.Bold);
    public static readonly Font HeaderTitle = Create(18F, FontStyle.Bold);
    public static readonly Font TableHeader = Create(9F, FontStyle.Bold);

    public static readonly Color HeaderBackground = Color.FromArgb(30, 41, 59);
    public static readonly Color HeaderSubtitle = Color.FromArgb(203, 213, 225);
    public static readonly Color Accent = Color.FromArgb(37, 99, 235);
    public static readonly Color Success = Color.DarkGreen;
    public static readonly Color Danger = Color.Firebrick;
    public static readonly Color Muted = Color.DimGray;

    /// <summary>在程式結束時釋放共用字型。</summary>
    public static void Release()
    {
        Body.Dispose();
        SectionTitle.Dispose();
        HeaderTitle.Dispose();
        TableHeader.Dispose();
    }

    private static Font Create(float size, FontStyle style)
    {
        try
        {
            var font = new Font(FamilyName, size, style);
            // 找不到字族時 GDI+ 會靜默改用預設字族，這裡不需要額外處理。
            return font;
        }
        catch
        {
            return new Font(SystemFonts.MessageBoxFont?.FontFamily ?? FontFamily.GenericSansSerif, size, style);
        }
    }
}
