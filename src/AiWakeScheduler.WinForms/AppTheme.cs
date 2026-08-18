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

    public static readonly Font Body = Create(9.5F, FontStyle.Regular);
    public static readonly Font Caption = Create(8.5F, FontStyle.Regular);
    public static readonly Font SectionTitle = Create(12.5F, FontStyle.Bold);
    public static readonly Font HeaderTitle = Create(20F, FontStyle.Bold);
    public static readonly Font TableHeader = Create(9.5F, FontStyle.Bold);

    // 單一冷色中性色盤，僅以藍色作為互動重點。紅、綠只表達語意狀態。
    public static readonly Color WindowBackground = Color.FromArgb(245, 247, 250);
    public static readonly Color Surface = Color.FromArgb(255, 255, 255);
    public static readonly Color SurfaceSubtle = Color.FromArgb(238, 243, 248);
    public static readonly Color Border = Color.FromArgb(207, 216, 228);
    public static readonly Color TextPrimary = Color.FromArgb(28, 39, 57);
    public static readonly Color HeaderBackground = Color.FromArgb(23, 37, 61);
    public static readonly Color HeaderSubtitle = Color.FromArgb(195, 207, 224);
    public static readonly Color Accent = Color.FromArgb(15, 108, 189);
    public static readonly Color AccentHover = Color.FromArgb(12, 90, 158);
    public static readonly Color AccentPressed = Color.FromArgb(10, 74, 130);
    public static readonly Color AccentSubtle = Color.FromArgb(226, 239, 250);
    public static readonly Color Selection = Color.FromArgb(218, 235, 250);
    public static readonly Color Success = Color.FromArgb(22, 101, 72);
    public static readonly Color Danger = Color.FromArgb(176, 45, 45);
    public static readonly Color DangerSubtle = Color.FromArgb(252, 235, 235);
    public static readonly Color Muted = Color.FromArgb(76, 91, 112);

    public static Color Canvas => SystemInformation.HighContrast ? SystemColors.Control : WindowBackground;
    public static Color Panel => SystemInformation.HighContrast ? SystemColors.Window : Surface;
    public static Color PanelSubtle => SystemInformation.HighContrast ? SystemColors.Control : SurfaceSubtle;
    public static Color Divider => SystemInformation.HighContrast ? SystemColors.WindowText : Border;
    public static Color PrimaryText => SystemInformation.HighContrast ? SystemColors.WindowText : TextPrimary;
    public static Color SecondaryText => SystemInformation.HighContrast ? SystemColors.GrayText : Muted;
    public static Color Selected => SystemInformation.HighContrast ? SystemColors.Highlight : Selection;
    public static Color SelectedText => SystemInformation.HighContrast ? SystemColors.HighlightText : TextPrimary;
    public static Color Banner => SystemInformation.HighContrast ? SystemColors.Highlight : HeaderBackground;
    public static Color BannerText => SystemInformation.HighContrast ? SystemColors.HighlightText : Color.White;
    public static Color BannerSubtitle => SystemInformation.HighContrast ? SystemColors.HighlightText : HeaderSubtitle;

    public enum ButtonVariant
    {
        Primary,
        Secondary,
        Danger
    }

    public static void ApplyForm(Form form)
    {
        form.BackColor = Canvas;
        form.ForeColor = PrimaryText;
    }

    public static void StyleButton(Button button, ButtonVariant variant = ButtonVariant.Secondary)
    {
        button.AutoSize = true;
        button.AutoSizeMode = AutoSizeMode.GrowAndShrink;
        button.MinimumSize = new Size(86, 36);
        button.Padding = new Padding(12, 5, 12, 5);
        button.FlatStyle = FlatStyle.Flat;
        button.FlatAppearance.BorderSize = 1;
        button.UseVisualStyleBackColor = false;

        if (SystemInformation.HighContrast)
        {
            button.BackColor = SystemColors.Control;
            button.ForeColor = SystemColors.ControlText;
            button.FlatAppearance.BorderColor = SystemColors.ControlText;
            button.FlatAppearance.MouseOverBackColor = SystemColors.Highlight;
            button.FlatAppearance.MouseDownBackColor = SystemColors.Highlight;
            return;
        }

        switch (variant)
        {
            case ButtonVariant.Primary:
                button.BackColor = Accent;
                button.ForeColor = Color.White;
                button.FlatAppearance.BorderColor = Accent;
                button.FlatAppearance.MouseOverBackColor = AccentHover;
                button.FlatAppearance.MouseDownBackColor = AccentPressed;
                break;
            case ButtonVariant.Danger:
                button.BackColor = Surface;
                button.ForeColor = Danger;
                button.FlatAppearance.BorderColor = Color.FromArgb(224, 170, 170);
                button.FlatAppearance.MouseOverBackColor = DangerSubtle;
                button.FlatAppearance.MouseDownBackColor = Color.FromArgb(247, 218, 218);
                break;
            default:
                button.BackColor = Surface;
                button.ForeColor = TextPrimary;
                button.FlatAppearance.BorderColor = Border;
                button.FlatAppearance.MouseOverBackColor = SurfaceSubtle;
                button.FlatAppearance.MouseDownBackColor = Color.FromArgb(220, 228, 238);
                break;
        }
    }

    public static void StyleInput(Control control)
    {
        if (SystemInformation.HighContrast)
        {
            return;
        }

        control.BackColor = Surface;
        control.ForeColor = TextPrimary;
        if (control is TextBox textBox)
        {
            textBox.BorderStyle = BorderStyle.FixedSingle;
        }
    }

    public static void StyleGroup(GroupBox group)
    {
        group.ForeColor = SystemInformation.HighContrast ? SystemColors.WindowText : TextPrimary;
        group.BackColor = SystemInformation.HighContrast ? SystemColors.Window : Surface;
        group.Padding = new Padding(14, 12, 14, 14);
    }

    /// <summary>在程式結束時釋放共用字型。</summary>
    public static void Release()
    {
        Body.Dispose();
        Caption.Dispose();
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
