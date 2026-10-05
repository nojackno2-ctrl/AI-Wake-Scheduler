using System.Collections.Concurrent;
using AiWakeScheduler.Core;

namespace AiWakeScheduler.WinForms;

/// <summary>
/// 即時終端機面板：把 <see cref="ScheduleManager.ActivityLogged"/> 的事件流
/// 顯示成一個會捲動的終端機畫面（開始/完成/失敗以顏色標示，stdout/stderr 逐段輸出）。
///
/// 事件可能來自任意背景執行緒（子程序讀取迴圈），這裡只把它們塞進執行緒安全佇列，
/// 實際畫面更新一律在 <see cref="_flushTimer"/> 的 Tick（UI 執行緒）批次處理，
/// 避免高頻輸出時對 UI 訊息佇列造成灌爆。
/// </summary>
internal sealed class TerminalPanel : UserControl
{
    private const int MaxRetainedCharacters = 200_000;
    private const int TrimToCharacters = 120_000;
    private const int MaxEntriesPerFlush = 400;

    private static readonly Color BackgroundColor = Color.FromArgb(18, 20, 24);
    private static readonly Color DefaultTextColor = Color.FromArgb(214, 218, 224);
    private static readonly Color StderrColor = Color.FromArgb(240, 128, 118);
    private static readonly Color StartedColor = Color.FromArgb(120, 180, 240);
    private static readonly Color CompletedColor = Color.FromArgb(120, 210, 150);
    private static readonly Color FailedColor = Color.FromArgb(240, 110, 110);
    private static readonly Color TimestampColor = Color.FromArgb(120, 128, 140);

    private readonly ConcurrentQueue<CliActivityEvent> _pending = new();
    private readonly RichTextBox _output = new();
    private readonly CheckBox _autoScrollCheck = new() { Text = "自動捲動", Checked = true, AutoSize = true };
    private readonly System.Windows.Forms.Timer _flushTimer = new() { Interval = 150 };

    private CliKind? _lastWriter;
    private bool _atLineStart = true;

    public TerminalPanel()
    {
        Dock = DockStyle.Fill;
        BackColor = AppTheme.Panel;
        BuildLayout();

        _flushTimer.Tick += (_, _) => Flush();
        _flushTimer.Start();
    }

    /// <summary>執行緒安全：任何執行緒都可以呼叫，只會排入佇列。</summary>
    public void LogActivity(CliActivityEvent activity)
    {
        _pending.Enqueue(activity);
    }

    public void Clear()
    {
        while (_pending.TryDequeue(out _))
        {
        }
        _output.Clear();
        _lastWriter = null;
        _atLineStart = true;
    }

    protected override void Dispose(bool disposing)
    {
        if (disposing)
        {
            _flushTimer.Stop();
            _flushTimer.Dispose();
        }
        base.Dispose(disposing);
    }

    private void BuildLayout()
    {
        var root = new TableLayoutPanel
        {
            Dock = DockStyle.Fill,
            ColumnCount = 1,
            RowCount = 2,
            BackColor = AppTheme.Panel
        };
        root.RowStyles.Add(new RowStyle(SizeType.AutoSize));
        root.RowStyles.Add(new RowStyle(SizeType.Percent, 100));

        var toolbar = new FlowLayoutPanel
        {
            Dock = DockStyle.Fill,
            AutoSize = true,
            FlowDirection = FlowDirection.LeftToRight,
            Margin = new Padding(0),
            Padding = new Padding(0, 4, 0, 4)
        };
        toolbar.Controls.Add(new Label
        {
            Text = "即時終端輸出",
            Font = AppTheme.TableHeader,
            AutoSize = true,
            Margin = new Padding(0, 6, 16, 0)
        });
        var clearButton = new Button { Text = "清除", AutoSize = true, Margin = new Padding(0, 0, 8, 0) };
        AppTheme.StyleButton(clearButton);
        clearButton.Click += (_, _) => Clear();
        toolbar.Controls.Add(clearButton);
        _autoScrollCheck.Margin = new Padding(4, 8, 0, 0);
        toolbar.Controls.Add(_autoScrollCheck);
        root.Controls.Add(toolbar, 0, 0);

        _output.Dock = DockStyle.Fill;
        _output.ReadOnly = true;
        _output.BorderStyle = BorderStyle.FixedSingle;
        _output.BackColor = BackgroundColor;
        _output.ForeColor = DefaultTextColor;
        _output.Font = AppTheme.Mono;
        _output.WordWrap = true;
        _output.HideSelection = true;
        _output.DetectUrls = false;
        NativeMethods.EnableDoubleBuffering(_output);
        root.Controls.Add(_output, 0, 1);

        Controls.Add(root);
    }

    /// <summary>UI 執行緒上批次處理排隊事件，避免逐筆 Invoke 造成灌爆。</summary>
    private void Flush()
    {
        if (_pending.IsEmpty || IsDisposed)
        {
            return;
        }

        _output.SuspendLayout();
        try
        {
            var processed = 0;
            while (processed < MaxEntriesPerFlush && _pending.TryDequeue(out var activity))
            {
                AppendEntry(activity);
                processed++;
            }

            TrimIfNeeded();

            if (_autoScrollCheck.Checked)
            {
                _output.SelectionStart = _output.TextLength;
                _output.ScrollToCaret();
            }
        }
        finally
        {
            _output.ResumeLayout(performLayout: true);
        }
    }

    private void AppendEntry(CliActivityEvent activity)
    {
        var cliName = CliDisplayNames.GetShort(activity.Cli);
        switch (activity.Kind)
        {
            case CliActivityKind.Started:
                AppendLine($"▶ {cliName} {activity.Text}", StartedColor, activity.Timestamp);
                break;
            case CliActivityKind.Completed:
                AppendLine($"✓ {cliName} {activity.Text}", CompletedColor, activity.Timestamp);
                break;
            case CliActivityKind.Failed:
                AppendLine($"✗ {cliName} {activity.Text}", FailedColor, activity.Timestamp);
                break;
            case CliActivityKind.Output:
                AppendStream(activity.Cli, cliName, activity.Text, DefaultTextColor);
                break;
            case CliActivityKind.ErrorOutput:
                AppendStream(activity.Cli, cliName, activity.Text, StderrColor);
                break;
        }
    }

    /// <summary>Started/Completed/Failed 一律另起獨立一行，並附上時間戳記。</summary>
    private void AppendLine(string text, Color color, DateTimeOffset timestamp)
    {
        EnsureNewLine();
        AppendText($"[{timestamp:HH:mm:ss}] ", TimestampColor);
        AppendText(text + Environment.NewLine, color);
        _lastWriter = null;
        _atLineStart = true;
    }

    /// <summary>
    /// stdout/stderr 的原始片段。多個 CLI 可能同時在跑，寫入者換人時才另起一行並標註來源，
    /// 減少每個片段都插入標籤造成的雜訊。
    /// </summary>
    private void AppendStream(CliKind writer, string cliName, string text, Color color)
    {
        if (text.Length == 0)
        {
            return;
        }

        if (_lastWriter != writer)
        {
            EnsureNewLine();
            AppendText($"[{cliName}] ", TimestampColor);
            _lastWriter = writer;
        }

        AppendText(text, color);
        _atLineStart = text.EndsWith('\n');
    }

    private void EnsureNewLine()
    {
        if (!_atLineStart && _output.TextLength > 0)
        {
            AppendText(Environment.NewLine, DefaultTextColor);
        }
    }

    private void AppendText(string text, Color color)
    {
        _output.SelectionStart = _output.TextLength;
        _output.SelectionLength = 0;
        _output.SelectionColor = color;
        _output.AppendText(text);
    }

    /// <summary>
    /// 超過保留上限時從頭截掉舊內容。用 Select + SelectedText 刪除而不是重設 Text，
    /// 才不會把剩餘內容的顏色格式一併洗掉。
    /// </summary>
    private void TrimIfNeeded()
    {
        if (_output.TextLength <= MaxRetainedCharacters)
        {
            return;
        }

        var cut = _output.TextLength - TrimToCharacters;
        _output.Select(0, cut);
        var wasReadOnly = _output.ReadOnly;
        try
        {
            // RichEdit rejects selection replacement while read-only; keep this
            // synchronous UI operation writable only for the deletion itself.
            _output.ReadOnly = false;
            _output.SelectedText = string.Empty;
        }
        finally
        {
            _output.ReadOnly = wasReadOnly;
        }
        _atLineStart = true;
        _lastWriter = null;
    }
}
