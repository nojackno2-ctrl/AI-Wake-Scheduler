using AiWakeScheduler.Core;

namespace AiWakeScheduler.WinForms;

internal sealed class SettingsForm : Form
{
    private readonly CliRunner _runner;
    private readonly Dictionary<CliKind, TextBox> _pathInputs = [];
    private readonly Dictionary<CliKind, TextBox> _argumentInputs = [];
    private readonly CheckBox _startupCheck = new() { Text = "登入 Windows 後自動啟動（最小化到系統匣）", AutoSize = true };
    private readonly CheckBox _trayCheck = new() { Text = "關閉主視窗時縮到系統匣，讓排程繼續執行", AutoSize = true };
    private readonly CheckBox _tokenSaverCheck = new()
    {
        Text = "節省 Token 模式（低推理、短回覆、禁止工具；建議保持開啟）",
        AutoSize = true
    };
    private readonly NumericUpDown _timeoutInput = new() { Minimum = 1, Maximum = 120, Width = 70 };
    private readonly Label _probeStatus = new()
    {
        AutoSize = true,
        ForeColor = Color.DimGray,
        MaximumSize = new Size(790, 0),
        Margin = new Padding(0, 6, 0, 0)
    };
    private readonly Font _headerFont = new("Microsoft JhengHei UI", 9F, FontStyle.Bold);

    public SettingsForm(AppSettings source, CliRunner runner)
    {
        _runner = runner;
        ResultSettings = CloneSettings(source);
        Text = "CLI 與程式設定";
        StartPosition = FormStartPosition.CenterParent;
        MinimumSize = new Size(760, 470);
        Size = new Size(860, 520);
        Font = new Font("Microsoft JhengHei UI", 9F);
        BuildLayout();
        LoadValues();
    }

    public AppSettings ResultSettings { get; }

    protected override void Dispose(bool disposing)
    {
        if (disposing)
        {
            _headerFont.Dispose();
        }
        base.Dispose(disposing);
    }

    private void BuildLayout()
    {
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
            Text = "可執行檔可填命令名稱（agy / codex / claude）或完整 .exe 路徑。額外參數可留空。",
            AutoSize = true,
            ForeColor = Color.DimGray,
            Margin = new Padding(0, 0, 0, 12)
        }, 0, 0);

        var cliTable = new TableLayoutPanel { Dock = DockStyle.Top, AutoSize = true, ColumnCount = 4, RowCount = 4 };
        cliTable.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 130));
        cliTable.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 65));
        cliTable.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 35));
        cliTable.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 86));
        cliTable.RowStyles.Add(new RowStyle(SizeType.Absolute, 30));
        cliTable.RowStyles.Add(new RowStyle(SizeType.Absolute, 38));
        cliTable.RowStyles.Add(new RowStyle(SizeType.Absolute, 38));
        cliTable.RowStyles.Add(new RowStyle(SizeType.Absolute, 38));
        cliTable.Controls.Add(Header("CLI"), 0, 0);
        cliTable.Controls.Add(Header("可執行檔或命令"), 1, 0);
        cliTable.Controls.Add(Header("額外參數（選填）"), 2, 0);

        var row = 1;
        foreach (var kind in Enum.GetValues<CliKind>())
        {
            var pathInput = new TextBox { Dock = DockStyle.Fill };
            var argumentInput = new TextBox { Dock = DockStyle.Fill };
            var browse = new Button { Text = "瀏覽…", Dock = DockStyle.Fill, Tag = kind };
            browse.Click += BrowseExecutable;
            _pathInputs[kind] = pathInput;
            _argumentInputs[kind] = argumentInput;
            cliTable.Controls.Add(new Label { Text = CliDisplayNames.Get(kind), AutoSize = true, Anchor = AnchorStyles.Left }, 0, row);
            cliTable.Controls.Add(pathInput, 1, row);
            cliTable.Controls.Add(argumentInput, 2, row);
            cliTable.Controls.Add(browse, 3, row);
            row++;
        }
        root.Controls.Add(cliTable, 0, 1);

        var options = new FlowLayoutPanel { AutoSize = true, Dock = DockStyle.Fill, FlowDirection = FlowDirection.TopDown };
        options.Controls.Add(_startupCheck);
        options.Controls.Add(_trayCheck);
        options.Controls.Add(_tokenSaverCheck);
        var timeoutRow = new FlowLayoutPanel { AutoSize = true };
        timeoutRow.Controls.Add(new Label { Text = "單一 CLI 最長執行時間（分鐘）：", AutoSize = true, Margin = new Padding(0, 7, 5, 0) });
        timeoutRow.Controls.Add(_timeoutInput);
        options.Controls.Add(timeoutRow);
        root.Controls.Add(options, 0, 2);

        var probeRow = new FlowLayoutPanel
        {
            Dock = DockStyle.Fill,
            AutoSize = true,
            FlowDirection = FlowDirection.TopDown,
            WrapContents = false
        };
        var probeButton = new Button { Text = "檢查三個 CLI（只執行 --version）", AutoSize = true };
        probeButton.Click += ProbeAllAsync;
        probeRow.Controls.Add(probeButton);
        probeRow.Controls.Add(_probeStatus);
        root.Controls.Add(probeRow, 0, 3);

        var buttons = new FlowLayoutPanel { Dock = DockStyle.Fill, FlowDirection = FlowDirection.RightToLeft, AutoSize = true };
        var ok = new Button { Text = "儲存", DialogResult = DialogResult.OK, AutoSize = true };
        ok.Click += SaveValues;
        var cancel = new Button { Text = "取消", DialogResult = DialogResult.Cancel, AutoSize = true };
        buttons.Controls.Add(ok);
        buttons.Controls.Add(cancel);
        root.Controls.Add(buttons, 0, 4);
        AcceptButton = ok;
        CancelButton = cancel;
        Controls.Add(root);
    }

    private void LoadValues()
    {
        ResultSettings.EnsureDefaults();
        foreach (var kind in Enum.GetValues<CliKind>())
        {
            _pathInputs[kind].Text = ResultSettings.CliProfiles[kind].Executable;
            _argumentInputs[kind].Text = ResultSettings.CliProfiles[kind].AdditionalArguments;
        }
        _startupCheck.Checked = ResultSettings.StartWithWindows;
        _trayCheck.Checked = ResultSettings.MinimizeToTray;
        _tokenSaverCheck.Checked = ResultSettings.TokenSaverMode;
        _timeoutInput.Value = Math.Clamp(ResultSettings.ExecutionTimeoutMinutes, _timeoutInput.Minimum, _timeoutInput.Maximum);
    }

    private void SaveValues(object? sender, EventArgs e)
    {
        foreach (var kind in Enum.GetValues<CliKind>())
        {
            if (string.IsNullOrWhiteSpace(_pathInputs[kind].Text))
            {
                MessageBox.Show($"請填寫 {CliDisplayNames.Get(kind)} 的命令或路徑。", Text, MessageBoxButtons.OK, MessageBoxIcon.Warning);
                DialogResult = DialogResult.None;
                return;
            }
            try
            {
                _ = ArgumentTokenizer.Parse(_argumentInputs[kind].Text);
            }
            catch (FormatException ex)
            {
                MessageBox.Show($"{CliDisplayNames.Get(kind)}：{ex.Message}", Text, MessageBoxButtons.OK, MessageBoxIcon.Warning);
                DialogResult = DialogResult.None;
                return;
            }
            ResultSettings.CliProfiles[kind].Executable = _pathInputs[kind].Text.Trim();
            ResultSettings.CliProfiles[kind].AdditionalArguments = _argumentInputs[kind].Text.Trim();
        }
        ResultSettings.StartWithWindows = _startupCheck.Checked;
        ResultSettings.MinimizeToTray = _trayCheck.Checked;
        ResultSettings.TokenSaverMode = _tokenSaverCheck.Checked;
        ResultSettings.ExecutionTimeoutMinutes = (int)_timeoutInput.Value;
    }

    private async void ProbeAllAsync(object? sender, EventArgs e)
    {
        if (sender is not Button button || !button.Enabled) return;
        button.Enabled = false;
        _probeStatus.Text = "檢查中…";
        _probeStatus.ForeColor = Color.DimGray;
        try
        {
            var workingDirectory = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
            var checks = Enum.GetValues<CliKind>().Select(async kind =>
            {
                var profile = new CliProfile
                {
                    Executable = _pathInputs[kind].Text.Trim(),
                    AdditionalArguments = _argumentInputs[kind].Text.Trim()
                };
                return await _runner.ProbeAsync(kind, profile, workingDirectory);
            });
            var results = await Task.WhenAll(checks);
            if (IsDisposed) return;
            _probeStatus.Text = string.Join(Environment.NewLine, results.Select(result =>
                $"{CliDisplayNames.Get(result.Cli)}：{(result.Succeeded ? "✓" : "✗")} {result.Summary}"));
            _probeStatus.ForeColor = results.All(result => result.Succeeded) ? Color.DarkGreen : Color.Firebrick;
        }
        catch (Exception ex)
        {
            if (IsDisposed) return;
            _probeStatus.Text = $"檢查失敗：{ex.Message}";
            _probeStatus.ForeColor = Color.Firebrick;
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
        if (sender is not Button { Tag: CliKind kind }) return;
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

    private Label Header(string text) => new()
    {
        Text = text,
        AutoSize = true,
        Font = _headerFont,
        Anchor = AnchorStyles.Left
    };

    private static AppSettings CloneSettings(AppSettings source)
    {
        source.EnsureDefaults();
        return new AppSettings
        {
            StartWithWindows = source.StartWithWindows,
            MinimizeToTray = source.MinimizeToTray,
            TokenSaverMode = source.TokenSaverMode,
            ExecutionTimeoutMinutes = source.ExecutionTimeoutMinutes,
            CliProfiles = source.CliProfiles.ToDictionary(pair => pair.Key, pair => pair.Value.Clone())
        };
    }
}

