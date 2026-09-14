using System.Drawing.Drawing2D;
using AiWakeScheduler.Core;

namespace AiWakeScheduler.WinForms;

/// <summary>
/// 額度看板。每個 CLI 一段標題，每個額度視窗自成一列：
/// 「視窗名稱 ─ 剩餘量長條 ─ 百分比 ─ 重置倒數」。
///
/// 舊版把同一個 CLI 的所有視窗串成一長串文字塞進單一 Label，
/// 換行之後整段同色，讀者分不出哪個百分比對應哪個倒數。
/// 這裡改成自繪：欄位對齊、以長條表達水位、顏色只用來表示剩餘量級距；
/// 而且整塊每秒只要一次重繪，不必維護數十個 Label。
/// </summary>
internal sealed class UsageBoard : Control
{
    private const TextFormatFlags LineFlags =
        TextFormatFlags.NoPadding | TextFormatFlags.NoPrefix | TextFormatFlags.VerticalCenter | TextFormatFlags.EndEllipsis;
    private const TextFormatFlags MeasureFlags = TextFormatFlags.NoPadding | TextFormatFlags.NoPrefix;
    private const TextFormatFlags WrapFlags =
        TextFormatFlags.NoPadding | TextFormatFlags.NoPrefix | TextFormatFlags.WordBreak;

    private static readonly Size Unbounded = new(int.MaxValue, int.MaxValue);

    private readonly Dictionary<CliKind, CliUsageSnapshot> _sources = [];
    private readonly Dictionary<CliKind, BoardGroup> _groups = [];

    /// <summary>上一次繪製時「重新登入」連結的位置，供滑鼠命中測試。</summary>
    private readonly List<(Rectangle Bounds, CliKind Kind)> _loginLinks = [];
    private readonly HashSet<CliKind> _loginInProgress = [];

    private DateTimeOffset _now = DateTimeOffset.Now;
    private int _preferredHeight;
    private CliKind? _hoveredLogin;

    public UsageBoard()
    {
        SetStyle(
            ControlStyles.UserPaint |
            ControlStyles.AllPaintingInWmPaint |
            ControlStyles.OptimizedDoubleBuffer |
            ControlStyles.ResizeRedraw,
            true);
        BackColor = AppTheme.Panel;
        ForeColor = AppTheme.PrimaryText;
        TabStop = false;
    }

    /// <summary>版面高度變動時通知外層容器調整大小。</summary>
    public event EventHandler? PreferredHeightChanged;

    /// <summary>使用者點了某個 CLI 的「重新登入」。</summary>
    public event EventHandler<CliKind>? LoginRequested;

    /// <summary>標示某個 CLI 的登入視窗是否已開啟，開啟中連結改為提示文字、不可再點。</summary>
    public void SetLoginInProgress(CliKind kind, bool inProgress)
    {
        var changed = inProgress ? _loginInProgress.Add(kind) : _loginInProgress.Remove(kind);
        if (changed)
        {
            RecalculateHeight();
            Invalidate();
        }
    }

    /// <summary>畫完目前資料所需要的高度。</summary>
    public int PreferredHeight => _preferredHeight;

    /// <summary>
    /// 套用最新的快照。只有內容真的換過才重算版面高度，
    /// 每秒的倒數更新只會重繪，不會牽動外層版面。
    /// </summary>
    public void Apply(IReadOnlyDictionary<CliKind, CliUsageSnapshot> snapshots, DateTimeOffset now)
    {
        _now = now;

        var changed = false;
        foreach (var descriptor in CliCatalog.All)
        {
            var kind = descriptor.Kind;
            if (!snapshots.TryGetValue(kind, out var snapshot))
            {
                _sources.Remove(kind);
                changed |= _groups.Remove(kind);
                continue;
            }

            if (_sources.TryGetValue(kind, out var current) && ReferenceEquals(current, snapshot))
            {
                continue;
            }

            _sources[kind] = snapshot;
            _groups[kind] = BuildGroup(descriptor, snapshot);
            changed = true;
        }

        if (changed)
        {
            RecalculateHeight();
        }

        Invalidate();
    }

    protected override void OnPaint(PaintEventArgs e)
    {
        e.Graphics.Clear(BackColor);
        _loginLinks.Clear();
        Render(e.Graphics, ClientSize.Width);
    }

    protected override void OnMouseMove(MouseEventArgs e)
    {
        base.OnMouseMove(e);
        var hit = HitLoginLink(e.Location);
        Cursor = hit is null ? Cursors.Default : Cursors.Hand;
        if (hit != _hoveredLogin)
        {
            _hoveredLogin = hit;
            Invalidate();
        }
    }

    protected override void OnMouseLeave(EventArgs e)
    {
        base.OnMouseLeave(e);
        Cursor = Cursors.Default;
        if (_hoveredLogin is not null)
        {
            _hoveredLogin = null;
            Invalidate();
        }
    }

    protected override void OnMouseClick(MouseEventArgs e)
    {
        base.OnMouseClick(e);
        if (e.Button == MouseButtons.Left && HitLoginLink(e.Location) is { } kind)
        {
            LoginRequested?.Invoke(this, kind);
        }
    }

    private CliKind? HitLoginLink(Point location)
    {
        foreach (var (bounds, kind) in _loginLinks)
        {
            if (bounds.Contains(location) && !_loginInProgress.Contains(kind))
            {
                return kind;
            }
        }

        return null;
    }

    protected override void OnResize(EventArgs e)
    {
        base.OnResize(e);
        RecalculateHeight();
    }

    protected override void OnFontChanged(EventArgs e)
    {
        base.OnFontChanged(e);
        RecalculateHeight();
    }

    private void RecalculateHeight()
    {
        var height = Render(null, ClientSize.Width);
        if (height == _preferredHeight)
        {
            return;
        }

        _preferredHeight = height;
        PreferredHeightChanged?.Invoke(this, EventArgs.Empty);
    }

    // ── 繪製 ──────────────────────────────────────────────────────

    /// <summary>
    /// 量測與繪製共用同一段版面計算：<paramref name="g"/> 為 null 時只回傳所需高度。
    /// </summary>
    private int Render(Graphics? g, int width)
    {
        if (g is not null)
        {
            g.SmoothingMode = SmoothingMode.AntiAlias;
        }

        var gap = Scale(10);
        var barWidth = Scale(88);
        var barHeight = Scale(7);
        var rowHeight = Math.Max(Font.Height + Scale(9), Scale(24));
        var headerHeight = Math.Max(AppTheme.TableHeader.Height + Scale(7), Scale(23));

        var labelWidth = LabelColumnWidth();
        var percentWidth = TextRenderer.MeasureText("100%", AppTheme.TableHeader, Unbounded, MeasureFlags).Width;

        var barX = labelWidth + gap;
        var percentX = barX + barWidth + gap;
        var countdownX = percentX + percentWidth + gap;

        var y = 0;
        var first = true;
        foreach (var descriptor in CliCatalog.All)
        {
            if (!first)
            {
                y += Scale(9);
            }

            first = false;
            _groups.TryGetValue(descriptor.Kind, out var group);

            if (g is not null)
            {
                DrawGroupHeader(g, descriptor.ShortName, group, y, headerHeight, width);
            }

            y += headerHeight + Scale(3);

            if (group is null)
            {
                y += DrawNote(g, "尚未讀取", AppTheme.SecondaryText, y, width);
                continue;
            }

            if (group.Rows.Length == 0)
            {
                var color = group.Availability == CliUsageAvailability.Unavailable
                    ? AppTheme.Danger
                    : AppTheme.SecondaryText;
                y += DrawNote(g, group.Message, color, y, width);
                if (group.RequiresLogin)
                {
                    y += DrawLoginLink(g, descriptor.Kind, y, width);
                }

                continue;
            }

            foreach (var row in group.Rows)
            {
                if (g is not null)
                {
                    DrawRow(g, row, y, rowHeight, labelWidth, barX, barWidth, barHeight, percentX, percentWidth, countdownX, width, gap);
                }

                y += rowHeight;
            }
        }

        return y;
    }

    private void DrawGroupHeader(Graphics g, string title, BoardGroup? group, int y, int headerHeight, int width)
    {
        var bounds = new Rectangle(0, y, width, headerHeight);
        TextRenderer.DrawText(g, title, AppTheme.TableHeader, bounds, AppTheme.PrimaryText, LineFlags | TextFormatFlags.Left);

        if (group is not null)
        {
            var freshness = FormatFreshness(group.ObservedAt);
            TextRenderer.DrawText(g, freshness, AppTheme.Caption, bounds, AppTheme.SecondaryText, LineFlags | TextFormatFlags.Right);
        }

        using var pen = new Pen(AppTheme.Divider);
        var lineY = y + headerHeight - 1;
        g.DrawLine(pen, 0, lineY, Math.Max(0, width - 1), lineY);
    }

    private void DrawRow(
        Graphics g,
        BoardRow row,
        int y,
        int rowHeight,
        int labelWidth,
        int barX,
        int barWidth,
        int barHeight,
        int percentX,
        int percentWidth,
        int countdownX,
        int width,
        int gap)
    {
        TextRenderer.DrawText(
            g,
            row.Label,
            Font,
            new Rectangle(0, y, labelWidth, rowHeight),
            AppTheme.PrimaryText,
            LineFlags | TextFormatFlags.Left);

        var level = LevelColor(row.RemainingPercent);
        DrawMeter(g, new Rectangle(barX, y + ((rowHeight - barHeight) / 2), barWidth, barHeight), row.RemainingPercent, level);

        TextRenderer.DrawText(
            g,
            row.PercentText,
            AppTheme.TableHeader,
            new Rectangle(percentX, y, percentWidth, rowHeight),
            level,
            LineFlags | TextFormatFlags.Right);

        var available = width - countdownX;
        if (available < Scale(44))
        {
            return;
        }

        if (!row.IsCountingDown || row.ResetsAt is not { } resetsAt)
        {
            TextRenderer.DrawText(
                g,
                "額度充足",
                AppTheme.Caption,
                new Rectangle(countdownX, y, available, rowHeight),
                AppTheme.SecondaryText,
                LineFlags | TextFormatFlags.Left);
            return;
        }

        var remaining = resetsAt - _now;
        if (remaining <= TimeSpan.Zero)
        {
            TextRenderer.DrawText(
                g,
                "等待伺服器更新",
                AppTheme.Caption,
                new Rectangle(countdownX, y, available, rowHeight),
                AppTheme.SecondaryText,
                LineFlags | TextFormatFlags.Left);
            return;
        }

        var countdown = FormatCountdown(remaining);
        var countdownWidth = TextRenderer.MeasureText(countdown, AppTheme.Mono, Unbounded, MeasureFlags).Width;
        TextRenderer.DrawText(
            g,
            countdown,
            AppTheme.Mono,
            new Rectangle(countdownX, y, Math.Min(countdownWidth, available), rowHeight),
            AppTheme.PrimaryText,
            LineFlags | TextFormatFlags.Left);

        var stampX = countdownX + countdownWidth + gap;
        var stampWidth = width - stampX;
        if (stampWidth >= Scale(56))
        {
            TextRenderer.DrawText(
                g,
                FormatResetStamp(resetsAt),
                AppTheme.Caption,
                new Rectangle(stampX, y, stampWidth, rowHeight),
                AppTheme.SecondaryText,
                LineFlags | TextFormatFlags.Left);
        }
    }

    /// <summary>畫剩餘量長條。填滿的比例就是剩餘百分比。</summary>
    private static void DrawMeter(Graphics g, Rectangle bounds, int remainingPercent, Color color)
    {
        if (bounds.Width <= 0 || bounds.Height <= 0)
        {
            return;
        }

        var radius = bounds.Height;
        using (var track = new SolidBrush(AppTheme.MeterTrack))
        using (var trackPath = RoundedRectangle(bounds, radius))
        {
            g.FillPath(track, trackPath);
        }

        var percent = Math.Clamp(remainingPercent, 0, 100);
        if (percent <= 0)
        {
            return;
        }

        // 只剩 1% 時也要看得見，至少保留一個圓角的寬度。
        var fillWidth = Math.Clamp(
            (int)Math.Round(bounds.Width * percent / 100.0),
            bounds.Height,
            bounds.Width);

        using var brush = new SolidBrush(color);
        using var path = RoundedRectangle(new Rectangle(bounds.X, bounds.Y, fillWidth, bounds.Height), radius);
        g.FillPath(brush, path);
    }

    /// <summary>畫一段說明文字（尚未讀取、不支援或錯誤訊息），回傳佔用高度。</summary>
    private int DrawNote(Graphics? g, string text, Color color, int y, int width)
    {
        var available = Math.Max(Scale(120), width);
        var size = TextRenderer.MeasureText(text, AppTheme.Caption, new Size(available, 0), WrapFlags);
        if (g is not null)
        {
            TextRenderer.DrawText(
                g,
                text,
                AppTheme.Caption,
                new Rectangle(0, y + Scale(3), available, size.Height),
                color,
                WrapFlags);
        }

        return size.Height + Scale(8);
    }

    /// <summary>畫「重新登入」連結並登記命中範圍，回傳佔用高度。</summary>
    private int DrawLoginLink(Graphics? g, CliKind kind, int y, int width)
    {
        var inProgress = _loginInProgress.Contains(kind);
        var text = inProgress ? "登入視窗已開啟，完成授權後會自動重新讀取" : "重新登入 ›";
        var font = inProgress ? AppTheme.Caption : AppTheme.TableHeader;
        var size = TextRenderer.MeasureText(text, font, Unbounded, MeasureFlags);
        var height = size.Height + Scale(6);

        if (g is not null)
        {
            var bounds = new Rectangle(0, y, Math.Min(size.Width + Scale(4), Math.Max(0, width)), height);
            var color = inProgress
                ? AppTheme.SecondaryText
                : _hoveredLogin == kind ? AppTheme.AccentHover : AppTheme.Accent;
            TextRenderer.DrawText(g, text, font, bounds, color, LineFlags | TextFormatFlags.Left);

            if (!inProgress)
            {
                _loginLinks.Add((bounds, kind));
            }
        }

        return height + Scale(4);
    }

    private int LabelColumnWidth()
    {
        var width = Scale(44);
        foreach (var group in _groups.Values)
        {
            foreach (var row in group.Rows)
            {
                width = Math.Max(width, TextRenderer.MeasureText(row.Label, Font, Unbounded, MeasureFlags).Width);
            }
        }

        return Math.Min(width, Scale(104));
    }

    private static GraphicsPath RoundedRectangle(Rectangle bounds, int diameter)
    {
        var path = new GraphicsPath();
        if (diameter <= 1 || bounds.Height < diameter)
        {
            path.AddRectangle(bounds);
            return path;
        }

        // 剩餘量極少時寬度會小於一個圓角，畫成橢圓比方塊自然。
        if (bounds.Width <= diameter)
        {
            path.AddEllipse(bounds);
            return path;
        }

        var arc = new Rectangle(bounds.X, bounds.Y, diameter, diameter);
        path.AddArc(arc, 180, 90);
        arc.X = bounds.Right - diameter;
        path.AddArc(arc, 270, 90);
        arc.Y = bounds.Bottom - diameter;
        path.AddArc(arc, 0, 90);
        arc.X = bounds.X;
        path.AddArc(arc, 90, 90);
        path.CloseFigure();
        return path;
    }

    private int Scale(int value) => (int)Math.Round(value * DeviceDpi / 96.0);

    private static Color LevelColor(int remainingPercent) => remainingPercent switch
    {
        >= 50 => AppTheme.Success,
        >= 20 => AppTheme.Caution,
        _ => AppTheme.Danger
    };

    // ── 文字格式 ──────────────────────────────────────────────────

    private static string FormatCountdown(TimeSpan remaining) => remaining.TotalDays >= 1
        ? $"{(int)remaining.TotalDays}天 {remaining.Hours:00}:{remaining.Minutes:00}"
        : $"{(int)remaining.TotalHours:00}:{remaining.Minutes:00}:{remaining.Seconds:00}";

    private string FormatResetStamp(DateTimeOffset resetsAt)
    {
        var local = resetsAt.LocalDateTime;
        return local.Date == _now.LocalDateTime.Date
            ? $"{local:HH:mm} 重置"
            : $"{local:MM/dd HH:mm} 重置";
    }

    private string FormatFreshness(DateTimeOffset observedAt)
    {
        var age = _now - observedAt;
        if (age < TimeSpan.FromSeconds(45))
        {
            return "剛更新";
        }

        if (age < TimeSpan.FromHours(1))
        {
            return $"{(int)age.TotalMinutes} 分鐘前更新";
        }

        if (age < TimeSpan.FromDays(1))
        {
            return $"{(int)age.TotalHours} 小時前更新";
        }

        return $"{observedAt.LocalDateTime:MM/dd HH:mm} 更新";
    }

    // ── 資料整形 ──────────────────────────────────────────────────

    private static BoardGroup BuildGroup(CliDescriptor descriptor, CliUsageSnapshot snapshot)
    {
        if (snapshot.Availability != CliUsageAvailability.Available)
        {
            return new BoardGroup(snapshot.Availability, snapshot.Message, [], snapshot.ObservedAt, snapshot.RequiresLogin);
        }

        if (snapshot.Windows.Count == 0)
        {
            return new BoardGroup(snapshot.Availability, "沒有可顯示的額度視窗", [], snapshot.ObservedAt);
        }

        return new BoardGroup(snapshot.Availability, snapshot.Message, BuildRows(descriptor, snapshot), snapshot.ObservedAt);
    }

    private static BoardRow[] BuildRows(CliDescriptor descriptor, CliUsageSnapshot snapshot)
    {
        var windows = snapshot.Windows;
        var labels = new string[windows.Count];
        for (var i = 0; i < windows.Count; i++)
        {
            labels[i] = DurationLabel(windows[i].Duration) ?? CleanWindowName(windows[i].Name, descriptor);
        }

        // 期間長度相同的視窗（例如 Claude 的多個 7 天額度）光看期間分不出來，改用原始名稱區分。
        for (var i = 0; i < labels.Length; i++)
        {
            for (var j = i + 1; j < labels.Length; j++)
            {
                if (!string.Equals(labels[i], labels[j], StringComparison.Ordinal))
                {
                    continue;
                }

                labels[i] = CleanWindowName(windows[i].Name, descriptor);
                labels[j] = CleanWindowName(windows[j].Name, descriptor);
            }
        }

        var rows = new BoardRow[windows.Count];
        for (var i = 0; i < windows.Count; i++)
        {
            var window = windows[i];
            rows[i] = new BoardRow(
                labels[i],
                window.RemainingPercent,
                $"{window.RemainingPercent}%",
                window.ResetsAt,
                window.IsActiveCountdown);
        }

        return rows;
    }

    /// <summary>由額度視窗的週期長度推出短標籤，這是各 CLI 之間最一致的說法。</summary>
    private static string? DurationLabel(TimeSpan? duration)
    {
        if (duration is not { } value || value <= TimeSpan.Zero)
        {
            return null;
        }

        if (value < TimeSpan.FromHours(1))
        {
            return $"{(int)Math.Round(value.TotalMinutes)} 分鐘";
        }

        if (value < TimeSpan.FromDays(1))
        {
            return $"{(int)Math.Round(value.TotalHours)} 小時";
        }

        if (Math.Abs(value.TotalDays - 1) < 0.05)
        {
            return "每日";
        }

        if (Math.Abs(value.TotalDays - 7) < 0.05)
        {
            return "每週";
        }

        if (Math.Abs(value.TotalDays - 30) < 1.5)
        {
            return "每月";
        }

        return $"{(int)Math.Round(value.TotalDays)} 天";
    }

    /// <summary>
    /// 把 API 回傳的視窗名稱縮成標籤：去掉與 CLI 重複的前綴、拆掉括號，
    /// 再把常見的英文額度名稱換成中文。
    /// </summary>
    private static string CleanWindowName(string? name, CliDescriptor descriptor)
    {
        var text = (name ?? string.Empty).Trim();
        if (descriptor.DisplayName.Length > 0 && text.StartsWith(descriptor.DisplayName, StringComparison.OrdinalIgnoreCase))
        {
            text = text[descriptor.DisplayName.Length..];
        }
        else if (descriptor.ShortName.Length > 0 && text.StartsWith(descriptor.ShortName, StringComparison.OrdinalIgnoreCase))
        {
            text = text[descriptor.ShortName.Length..];
        }

        text = text
            .Replace('（', ' ')
            .Replace('）', ' ')
            .Replace('(', ' ')
            .Replace(')', ' ')
            .Replace("Limit Remaining", " ", StringComparison.OrdinalIgnoreCase)
            .Replace("Remaining", " ", StringComparison.OrdinalIgnoreCase)
            .Trim();

        while (text.Contains("  ", StringComparison.Ordinal))
        {
            text = text.Replace("  ", " ", StringComparison.Ordinal);
        }

        if (text.Contains("Five Hour", StringComparison.OrdinalIgnoreCase))
        {
            return "5 小時";
        }

        if (text.Contains("Weekly", StringComparison.OrdinalIgnoreCase))
        {
            return "每週";
        }

        if (text.Contains("Daily", StringComparison.OrdinalIgnoreCase))
        {
            return "每日";
        }

        if (text.Contains("Monthly", StringComparison.OrdinalIgnoreCase))
        {
            return "每月";
        }

        return text.Length == 0 ? "額度" : text;
    }

    private sealed record BoardRow(
        string Label,
        int RemainingPercent,
        string PercentText,
        DateTimeOffset? ResetsAt,
        bool IsCountingDown);

    private sealed record BoardGroup(
        CliUsageAvailability Availability,
        string Message,
        BoardRow[] Rows,
        DateTimeOffset ObservedAt,
        bool RequiresLogin = false);
}
