using AiWakeScheduler.Core;

namespace AiWakeScheduler.WinForms;

internal sealed class SettingsForm : Form
{
    private readonly ICliRunner _runner;
    private readonly Dictionary<CliKind, TextBox> _pathInputs = [];
    private readonly Dictionary<CliKind, TextBox> _argumentInputs = [];

    private readonly CheckBox _startupCheck = new() { Text = "登入 Windows 後自動啟動（最小化到系統匣）", AutoSize = true };
    private readonly CheckBox _trayCheck = new() { Text = "關閉主視窗時縮到系統匣，讓排程繼續執行", AutoSize = true };
    private readonly CheckBox _tokenSaverCheck = new()
    {
        Text = "節省 Token 模式（停用工具與 MCP、低推理、短回覆；建議保持開啟）",
        AutoSize = true
    };
    private readonly NumericUpDown _timeoutInput = new() { Minimum = 1, Maximum = 120, Width = 70 };
    private readonly Label _probeStatus = new()
    {
        AutoSize = true,
        ForeColor = AppTheme.Muted,
        Margin = new Padding(0, 6, 0, 0)
    };

    private CancellationTokenSource? _probeCancellation;

    public SettingsForm(AppSettings source, ICliRunner runner)
    {
        _runner = runner;
        ResultSettings = source.Clone();

        Text = "CLI 與程式設定";
        StartPosition = FormStartPosition.CenterParent;
        AutoScaleDimensions = new SizeF(7F, 15F);
        AutoScaleMode = AutoScaleMode.Font;
        Font = AppTheme.Body;
        ApplyPreferredSize();
        SetStyle(ControlStyles.OptimizedDoubleBuffer | ControlStyles.AllPaintingInWmPaint, true);

        BuildLayout();
        LoadValues();
    }

    public AppSettings ResultSettings { get; }

    protected override void Dispose(bool disposing)
    {
        if (disposing)
        {
            _probeCancellation?.Cancel();
            _probeCancellation?.Dispose();
            _probeCancellation = null;
        }
        base.Dispose(disposing);
    }

    private void ApplyPreferredSize()
    {
        var workingArea = Screen.PrimaryScreen?.WorkingArea ?? new Rectangle(0, 0, 1024, 700);
        MinimumSize = new Size(Math.Min(780, workingArea.Width), Math.Min(520, workingArea.Height));
        Size = new Size(Math.Min(880, workingArea.Width), Math.Min(570, workingArea.Height));
    }

    protected override void OnResize(EventArgs e)
    {
        base.OnResize(e);
        // 讓探測結果隨視窗寬度換行，取代原本寫死的 790px 上限。
        _probeStatus.MaximumSize = new Size(Math.Max(200, ClientSize.Width - 48), 0);
    }

    private void BuildLayout()
    {
        SuspendLayout();

        var root = new TableLayoutPanel
        {
            Dock = DockStyle.Fill,
            Padding = new Padding(16),
            ColumnCount = 1,
            RowCount = 5
        };
        root.RowStyles.Add(new RowStyle(SizeType.AutoSize));
        root.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
        root.RowStyles.Add(new RowStyle(SizeType.AutoSize));
        root.RowStyles.Add(new RowStyle(SizeType.AutoSize));
        root.RowStyles.Add(new RowStyle(SizeType.AutoSize));

        root.Controls.Add(new Label
        {
            Text = "可執行檔可填命令名稱（agy / codex / claude）或完整 .exe 路徑。"
                 + "額外參數可留空（Antigravity Claude / GPT 預設使用 Claude Sonnet，亦可於額外參數填寫 --model 自訂）。",
            AutoSize = true,
            ForeColor = AppTheme.Muted,
            Margin = new Padding(0, 0, 0, 12)
        }, 0, 0);

        root.Controls.Add(BuildCliTable(), 0, 1);
        root.Controls.Add(BuildOptions(), 0, 2);
        root.Controls.Add(BuildProbeRow(), 0, 3);
        root.Controls.Add(BuildButtons(), 0, 4);

        Controls.Add(root);
        ResumeLayout(performLayout: true);
    }

    private Control BuildCliTable()
    {
        var descriptors = CliCatalog.All;
        var table = new TableLayoutPanel
        {
            Dock = DockStyle.Top,
            AutoSize = true,
            ColumnCount = 4,
            RowCount = descriptors.Count + 1
        };
        table.ColumnStyles.Add(new ColumnStyle(SizeType.AutoSize));
        table.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 60));
        table.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 40));
        table.ColumnStyles.Add(new ColumnStyle(SizeType.AutoSize));
        table.RowStyles.Add(new RowStyle(SizeType.AutoSize));

        table.Controls.Add(Header("CLI"), 0, 0);
        table.Controls.Add(Header("可執行檔或命令"), 1, 0);
        table.Controls.Add(Header("額外參數（選填）"), 2, 0);

        for (var i = 0; i < descriptors.Count; i++)
        {
            var kind = descriptors[i].Kind;
            var row = i + 1;
            table.RowStyles.Add(new RowStyle(SizeType.AutoSize));

            var pathInput = new TextBox { Dock = DockStyle.Fill };
            var argumentInput = new TextBox { Dock = DockStyle.Fill };
            var browse = new Button { Text = "瀏覽…", AutoSize = true, Dock = DockStyle.Fill, Tag = kind };
            browse.Click += BrowseExecutable;

            _pathInputs[kind] = pathInput;
            _argumentInputs[kind] = argumentInput;

            table.Controls.Add(new Label { Text = descriptors[i].DisplayName, AutoSize = true, Anchor = AnchorStyles.Left, Margin = new Padding(0, 8, 12, 6) }, 0, row);
            table.Controls.Add(pathInput, 1, row);
            table.Controls.Add(argumentInput, 2, row);
            table.Controls.Add(browse, 3, row);
        }

        return table;
    }

    private Control BuildOptions()
    {
        var options = new FlowLayoutPanel { AutoSize = true, Dock = DockStyle.Fill, FlowDirection = FlowDirection.TopDown };
        options.Controls.Add(_startupCheck);
        options.Controls.Add(_trayCheck);
        options.Controls.Add(_tokenSaverCheck);

        var timeoutRow = new FlowLayoutPanel { AutoSize = true };
        timeoutRow.Controls.Add(new Label { Text = "單一 CLI 最長執行時間（分鐘）：", AutoSize = true, Margin = new Padding(0, 7, 5, 0) });
        timeoutRow.Controls.Add(_timeoutInput);
        options.Controls.Add(timeoutRow);
        return options;
    }

    private Control BuildProbeRow()
    {
        var probeRow = new FlowLayoutPanel
        {
            Dock = DockStyle.Fill,
            AutoSize = true,
            FlowDirection = FlowDirection.TopDown,
            WrapContents = false
        };
        var probeButton = new Button { Text = "檢查全部 CLI（只執行 --version，不消耗 Token）", AutoSize = true };
        probeButton.Click += ProbeAllAsync;
        probeRow.Controls.Add(probeButton);
        probeRow.Controls.Add(_probeStatus);
        return probeRow;
    }

    private Control BuildButtons()
    {
        var buttons = new FlowLayoutPanel { Dock = DockStyle.Fill, FlowDirection = FlowDirection.RightToLeft, AutoSize = true };
        var ok = new Button { Text = "儲存", DialogResult = DialogResult.OK, AutoSize = true };
        ok.Click += SaveValues;
        var cancel = new Button { Text = "取消", DialogResult = DialogResult.Cancel, AutoSize = true };
        buttons.Controls.Add(ok);
        buttons.Controls.Add(cancel);
        AcceptButton = ok;
        CancelButton = cancel;
        return buttons;
    }

    private void LoadValues()
    {
        ResultSettings.EnsureDefaults();
        foreach (var descriptor in CliCatalog.All)
        {
            var profile = ResultSettings.CliProfiles[descriptor.Kind];
            _pathInputs[descriptor.Kind].Text = profile.Executable;
            _argumentInputs[descriptor.Kind].Text = profile.AdditionalArguments;
        }
        _startupCheck.Checked = ResultSettings.StartWithWindows;
        _trayCheck.Checked = ResultSettings.MinimizeToTray;
        _tokenSaverCheck.Checked = ResultSettings.TokenSaverMode;
        _timeoutInput.Value = Math.Clamp(ResultSettings.ExecutionTimeoutMinutes, _timeoutInput.Minimum, _timeoutInput.Maximum);
    }

    private void SaveValues(object? sender, EventArgs e)
    {
        foreach (var descriptor in CliCatalog.All)
        {
            var kind = descriptor.Kind;
            var path = _pathInputs[kind].Text.Trim();
            var arguments = _argumentInputs[kind].Text.Trim();

            if (string.IsNullOrWhiteSpace(path))
            {
                RejectSave($"請填寫 {descriptor.DisplayName} 的命令或路徑。");
                return;
            }

            try
            {
                _ = ArgumentTokenizer.Parse(arguments);
            }
            catch (FormatException ex)
            {
                RejectSave($"{descriptor.DisplayName}：{ex.Message}");
                return;
            }

            ResultSettings.CliProfiles[kind].Executable = path;
            ResultSettings.CliProfiles[kind].AdditionalArguments = arguments;
        }

        ResultSettings.StartWithWindows = _startupCheck.Checked;
        ResultSettings.MinimizeToTray = _trayCheck.Checked;
        ResultSettings.TokenSaverMode = _tokenSaverCheck.Checked;
        ResultSettings.ExecutionTimeoutMinutes = (int)_timeoutInput.Value;
    }

    private void RejectSave(string message)
    {
        MessageBox.Show(message, Text, MessageBoxButtons.OK, MessageBoxIcon.Warning);
        DialogResult = DialogResult.None;
    }

    private async void ProbeAllAsync(object? sender, EventArgs e)
    {
        if (sender is not Button button || !button.Enabled)
        {
            return;
        }

        button.Enabled = false;
        _probeStatus.Text = "檢查中…";
        _probeStatus.ForeColor = AppTheme.Muted;

        _probeCancellation?.Cancel();
        _probeCancellation?.Dispose();
        var cancellation = new CancellationTokenSource();
        _probeCancellation = cancellation;

        try
        {
            // 使用者可能剛改過路徑，先讓解析快取失效再探測。
            ExecutableLocator.ClearCache();

            var workingDirectory = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
            var descriptors = CliCatalog.All;
            var checks = new Task<CliProbeResult>[descriptors.Count];
            for (var i = 0; i < descriptors.Count; i++)
            {
                var kind = descriptors[i].Kind;
                checks[i] = _runner.ProbeAsync(
                    kind,
                    new CliProfile
                    {
                        Executable = _pathInputs[kind].Text.Trim(),
                        AdditionalArguments = _argumentInputs[kind].Text.Trim()
                    },
                    workingDirectory,
                    cancellation.Token);
            }

            var results = await Task.WhenAll(checks).ConfigureAwait(true);
            if (IsDisposed || cancellation.IsCancellationRequested)
            {
                return;
            }

            var succeeded = true;
            var lines = new string[results.Length];
            for (var i = 0; i < results.Length; i++)
            {
                succeeded &= results[i].Succeeded;
                lines[i] = $"{CliDisplayNames.Get(results[i].Cli)}：{(results[i].Succeeded ? "✓" : "✗")} {results[i].Summary}";
            }

            _probeStatus.Text = string.Join(Environment.NewLine, lines);
            _probeStatus.ForeColor = succeeded ? AppTheme.Success : AppTheme.Danger;
        }
        catch (OperationCanceledException)
        {
        }
        catch (Exception ex)
        {
            if (!IsDisposed)
            {
                _probeStatus.Text = $"檢查失敗：{ex.Message}";
                _probeStatus.ForeColor = AppTheme.Danger;
            }
        }
        finally
        {
            if (!IsDisposed)
            {
                button.Enabled = true;
            }
        }
    }

    private void BrowseExecutable(object? sender, EventArgs e)
    {
        if (sender is not Button { Tag: CliKind kind })
        {
            return;
        }

        using var dialog = new OpenFileDialog
        {
            Title = $"選擇 {CliDisplayNames.Get(kind)} 可執行檔",
            Filter = "可執行檔 (*.exe;*.cmd;*.bat)|*.exe;*.cmd;*.bat|所有檔案 (*.*)|*.*",
            CheckFileExists = true
        };
        if (dialog.ShowDialog(this) == DialogResult.OK)
        {
            _pathInputs[kind].Text = dialog.FileName;
        }
    }

    private static Label Header(string text) => new()
    {
        Text = text,
        AutoSize = true,
        Font = AppTheme.TableHeader,
        Anchor = AnchorStyles.Left,
        Margin = new Padding(0, 0, 12, 6)
    };
}
