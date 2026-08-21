using AiWakeScheduler.Core;

namespace AiWakeScheduler.WinForms;

internal sealed class SettingsForm : Form
{
    private sealed record EffortItem(ThinkingEffort Effort, string Text)
    {
        public override string ToString() => Text;
    }

    private readonly ICliRunner _runner;
    private readonly Dictionary<CliKind, TextBox> _pathInputs = [];
    private readonly Dictionary<CliKind, ComboBox> _modelInputs = [];
    private readonly Dictionary<CliKind, ComboBox> _effortInputs = [];
    private readonly Dictionary<CliKind, TextBox> _argumentInputs = [];

    private readonly CheckBox _startupCheck = new() { Text = "登入 Windows 後自動啟動（最小化到系統匣）", AutoSize = true };
    private readonly CheckBox _trayCheck = new() { Text = "關閉主視窗時縮到系統匣，讓排程繼續執行", AutoSize = true };
    private readonly CheckBox _tokenSaverCheck = new()
    {
        Text = "節省 Token 模式（停用工具與 MCP、預設低推理、短回覆；建議保持開啟）",
        AutoSize = true
    };
    private readonly NumericUpDown _timeoutInput = new() { Minimum = 1, Maximum = 120, Width = 70 };
    private readonly NumericUpDown _quotaAutoRefreshInput = new() { Minimum = 0, Maximum = 1440, Width = 70 };
    private readonly Label _probeStatus = new()
    {
        AutoSize = true,
        ForeColor = AppTheme.SecondaryText,
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
        AppTheme.ApplyForm(this);
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
        var workingArea = Screen.PrimaryScreen?.WorkingArea ?? new Rectangle(0, 0, 1280, 800);
        MinimumSize = new Size(Math.Min(980, workingArea.Width), Math.Min(640, workingArea.Height));
        Size = new Size(Math.Min(1120, workingArea.Width), Math.Min(820, workingArea.Height));
    }

    protected override void OnResize(EventArgs e)
    {
        base.OnResize(e);
        _probeStatus.MaximumSize = new Size(Math.Max(200, ClientSize.Width - 80), 0);
    }

    private void BuildLayout()
    {
        SuspendLayout();

        var root = new TableLayoutPanel
        {
            Dock = DockStyle.Fill,
            Padding = new Padding(24, 20, 24, 20),
            ColumnCount = 1,
            RowCount = 6,
            AutoScroll = true,
            BackColor = AppTheme.Canvas
        };
        root.RowStyles.Add(new RowStyle(SizeType.AutoSize));
        root.RowStyles.Add(new RowStyle(SizeType.AutoSize));
        // 這一列（CLI 表格）之前用 Percent(100) 撐滿剩餘空間，
        // 但那會讓 TableLayoutPanel 把表格硬壓縮進目前可見高度，
        // 使超出視窗範圍的 CLI 列直接被裁掉，而不是讓外層 AutoScroll 捲動顯示。
        // 改成 AutoSize，讓表格長到完整自然高度，交由 root 的 AutoScroll 處理捲動。
        root.RowStyles.Add(new RowStyle(SizeType.AutoSize));
        root.RowStyles.Add(new RowStyle(SizeType.AutoSize));
        root.RowStyles.Add(new RowStyle(SizeType.AutoSize));
        root.RowStyles.Add(new RowStyle(SizeType.AutoSize));

        root.Controls.Add(new Label
        {
            Text = "CLI 與程式設定",
            Font = AppTheme.HeaderTitle,
            ForeColor = AppTheme.PrimaryText,
            AutoSize = true,
            Margin = new Padding(0, 0, 0, 4)
        }, 0, 0);

        root.Controls.Add(new Label
        {
            Text = "可為各 CLI 指定最新模型（支援下拉選取或直接輸入任意自訂模型 ID）與思考程度（Reasoning Effort）。"
                 + "可執行檔可填命令名稱（agy / codex / claude）或完整 .exe 路徑。",
            AutoSize = true,
            ForeColor = AppTheme.SecondaryText,
            MaximumSize = new Size(950, 0),
            Margin = new Padding(0, 0, 0, 18)
        }, 0, 1);

        root.Controls.Add(BuildCliTable(), 0, 2);
        root.Controls.Add(BuildOptions(), 0, 3);
        root.Controls.Add(BuildProbeRow(), 0, 4);
        root.Controls.Add(BuildButtons(), 0, 5);

        Controls.Add(root);
        ResumeLayout(performLayout: true);
    }

    private Control BuildCliTable()
    {
        var descriptors = CliCatalog.All;
        var table = new TableLayoutPanel
        {
            Dock = DockStyle.Fill,
            AutoSize = true,
            ColumnCount = 6,
            RowCount = descriptors.Count + 2,
            BackColor = AppTheme.Panel,
            Padding = new Padding(16),
            Margin = new Padding(0, 0, 0, 16)
        };
        table.ColumnStyles.Add(new ColumnStyle(SizeType.AutoSize));
        table.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 26));
        table.ColumnStyles.Add(new ColumnStyle(SizeType.AutoSize));
        table.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 42));
        table.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 32));
        table.ColumnStyles.Add(new ColumnStyle(SizeType.AutoSize));
        table.RowStyles.Add(new RowStyle(SizeType.AutoSize));
        table.RowStyles.Add(new RowStyle(SizeType.AutoSize));

        var sectionTitle = new Label
        {
            Text = "CLI 模型與連線設定",
            Font = AppTheme.SectionTitle,
            ForeColor = AppTheme.PrimaryText,
            AutoSize = true,
            Margin = new Padding(0, 0, 0, 14)
        };
        table.Controls.Add(sectionTitle, 0, 0);
        table.SetColumnSpan(sectionTitle, 6);
        table.Controls.Add(Header("CLI"), 0, 1);
        table.Controls.Add(Header("模型 (Model)"), 1, 1);
        table.Controls.Add(Header("思考程度 (Effort)"), 2, 1);
        table.Controls.Add(Header("可執行檔或命令"), 3, 1);
        table.Controls.Add(Header("額外參數（選填）"), 4, 1);
        table.Controls.Add(Header(string.Empty), 5, 1);

        for (var i = 0; i < descriptors.Count; i++)
        {
            var descriptor = descriptors[i];
            var kind = descriptor.Kind;
            var row = i + 2;
            table.RowStyles.Add(new RowStyle(SizeType.AutoSize));

            var modelCombo = new ComboBox
            {
                Dock = DockStyle.Fill,
                DropDownStyle = ComboBoxStyle.DropDown,
                Margin = new Padding(0, 3, 8, 3)
            };
            foreach (var preset in descriptor.PresetModels)
            {
                if (!string.IsNullOrEmpty(preset))
                {
                    modelCombo.Items.Add(preset);
                }
            }

            var effortCombo = new ComboBox
            {
                Dock = DockStyle.Fill,
                DropDownStyle = ComboBoxStyle.DropDownList,
                Margin = new Padding(0, 3, 8, 3),
                MinimumSize = new Size(130, 0)
            };

            var pathInput = new TextBox { Dock = DockStyle.Fill, Margin = new Padding(0, 3, 8, 3) };
            var argumentInput = new TextBox { Dock = DockStyle.Fill, Margin = new Padding(0, 3, 8, 3) };
            var browse = new Button { Text = "瀏覽…", AutoSize = true, Dock = DockStyle.Fill, Tag = kind };

            AppTheme.StyleInput(modelCombo);
            AppTheme.StyleInput(effortCombo);
            AppTheme.StyleInput(pathInput);
            AppTheme.StyleInput(argumentInput);
            AppTheme.StyleButton(browse);
            browse.Click += BrowseExecutable;

            _modelInputs[kind] = modelCombo;
            _effortInputs[kind] = effortCombo;
            _pathInputs[kind] = pathInput;
            _argumentInputs[kind] = argumentInput;
            RefreshEffortOptions(kind, ThinkingEffort.Default);
            modelCombo.TextChanged += (_, _) => RefreshEffortOptions(kind);

            table.Controls.Add(new Label
            {
                Text = descriptor.DisplayName,
                AutoSize = true,
                Anchor = AnchorStyles.Left,
                Margin = new Padding(0, 8, 14, 8)
            }, 0, row);
            table.Controls.Add(modelCombo, 1, row);
            table.Controls.Add(effortCombo, 2, row);
            table.Controls.Add(pathInput, 3, row);
            table.Controls.Add(argumentInput, 4, row);
            table.Controls.Add(browse, 5, row);
        }

        return table;
    }

    private Control BuildOptions()
    {
        var options = new FlowLayoutPanel
        {
            AutoSize = true,
            Dock = DockStyle.Fill,
            FlowDirection = FlowDirection.TopDown,
            WrapContents = false,
            BackColor = AppTheme.Panel,
            Padding = new Padding(16),
            Margin = new Padding(0, 0, 0, 16)
        };
        options.Controls.Add(new Label { Text = "啟動與資源", Font = AppTheme.SectionTitle, AutoSize = true, Margin = new Padding(0, 0, 0, 12) });
        _startupCheck.Margin = new Padding(0, 3, 0, 6);
        _trayCheck.Margin = new Padding(0, 3, 0, 6);
        _tokenSaverCheck.Margin = new Padding(0, 3, 0, 10);
        options.Controls.Add(_startupCheck);
        options.Controls.Add(_trayCheck);
        options.Controls.Add(_tokenSaverCheck);

        var timeoutRow = new FlowLayoutPanel { AutoSize = true, Margin = new Padding(0, 4, 0, 0) };
        timeoutRow.Controls.Add(new Label { Text = "單一 CLI 最長執行時間（分鐘）：", AutoSize = true, Margin = new Padding(0, 7, 5, 0) });
        AppTheme.StyleInput(_timeoutInput);
        timeoutRow.Controls.Add(_timeoutInput);
        options.Controls.Add(timeoutRow);

        var refreshRow = new FlowLayoutPanel { AutoSize = true, Margin = new Padding(0, 4, 0, 0) };
        refreshRow.Controls.Add(new Label { Text = "背景自動讀取配額間隔（分鐘，0 表示關閉）：", AutoSize = true, Margin = new Padding(0, 7, 5, 0) });
        AppTheme.StyleInput(_quotaAutoRefreshInput);
        refreshRow.Controls.Add(_quotaAutoRefreshInput);
        options.Controls.Add(refreshRow);
        return options;
    }

    private Control BuildProbeRow()
    {
        var probeRow = new FlowLayoutPanel
        {
            Dock = DockStyle.Fill,
            AutoSize = true,
            FlowDirection = FlowDirection.TopDown,
            WrapContents = false,
            BackColor = AppTheme.Panel,
            Padding = new Padding(16),
            Margin = new Padding(0, 0, 0, 16)
        };
        probeRow.Controls.Add(new Label { Text = "連線檢查", Font = AppTheme.SectionTitle, AutoSize = true, Margin = new Padding(0, 0, 0, 10) });
        var probeButton = new Button { Text = "檢查全部 CLI（只執行 --version，不消耗 Token）", AutoSize = true };
        AppTheme.StyleButton(probeButton);
        probeButton.Click += ProbeAllAsync;
        probeRow.Controls.Add(probeButton);
        probeRow.Controls.Add(_probeStatus);
        return probeRow;
    }

    private Control BuildButtons()
    {
        var buttons = new FlowLayoutPanel { Dock = DockStyle.Fill, FlowDirection = FlowDirection.RightToLeft, AutoSize = true, Margin = new Padding(0) };
        var ok = new Button { Text = "儲存", DialogResult = DialogResult.OK, AutoSize = true };
        AppTheme.StyleButton(ok, AppTheme.ButtonVariant.Primary);
        ok.Click += SaveValues;
        var cancel = new Button { Text = "取消", DialogResult = DialogResult.Cancel, AutoSize = true };
        AppTheme.StyleButton(cancel);
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
            _modelInputs[descriptor.Kind].Text = profile.Model;
            _argumentInputs[descriptor.Kind].Text = profile.AdditionalArguments;
            RefreshEffortOptions(descriptor.Kind, profile.ThinkingEffort);
        }
        _startupCheck.Checked = ResultSettings.StartWithWindows;
        _trayCheck.Checked = ResultSettings.MinimizeToTray;
        _tokenSaverCheck.Checked = ResultSettings.TokenSaverMode;
        _timeoutInput.Value = Math.Clamp(ResultSettings.ExecutionTimeoutMinutes, _timeoutInput.Minimum, _timeoutInput.Maximum);
        _quotaAutoRefreshInput.Value = Math.Clamp(ResultSettings.QuotaAutoRefreshMinutes, _quotaAutoRefreshInput.Minimum, _quotaAutoRefreshInput.Maximum);
    }

    private void RefreshEffortOptions(CliKind kind, ThinkingEffort? preferredEffort = null)
    {
        var descriptor = CliCatalog.Get(kind);
        var model = _modelInputs[kind].Text;
        var effortCombo = _effortInputs[kind];
        var currentEffort = preferredEffort ??
            (effortCombo.SelectedItem is EffortItem current ? current.Effort : ThinkingEffort.Default);
        var normalizedEffort = descriptor.NormalizeEffort(model, currentEffort);

        effortCombo.BeginUpdate();
        try
        {
            effortCombo.Items.Clear();
            var supported = descriptor.GetSupportedEfforts(model);
            var selectedIndex = 0;
            for (var i = 0; i < supported.Count; i++)
            {
                var effort = supported[i];
                effortCombo.Items.Add(new EffortItem(effort, ThinkingEffortDisplayNames.Get(effort)));
                if (effort == normalizedEffort)
                {
                    selectedIndex = i;
                }
            }
            effortCombo.SelectedIndex = effortCombo.Items.Count == 0 ? -1 : selectedIndex;
        }
        finally
        {
            effortCombo.EndUpdate();
        }
    }

    private void SaveValues(object? sender, EventArgs e)
    {
        foreach (var descriptor in CliCatalog.All)
        {
            var kind = descriptor.Kind;
            var path = _pathInputs[kind].Text.Trim();
            var model = _modelInputs[kind].Text.Trim();
            var effort = _effortInputs[kind].SelectedItem is EffortItem item ? item.Effort : ThinkingEffort.Default;
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
            ResultSettings.CliProfiles[kind].Model = model;
            ResultSettings.CliProfiles[kind].ThinkingEffort = effort;
            ResultSettings.CliProfiles[kind].AdditionalArguments = arguments;
        }

        ResultSettings.StartWithWindows = _startupCheck.Checked;
        ResultSettings.MinimizeToTray = _trayCheck.Checked;
        ResultSettings.TokenSaverMode = _tokenSaverCheck.Checked;
        ResultSettings.ExecutionTimeoutMinutes = (int)_timeoutInput.Value;
        ResultSettings.QuotaAutoRefreshMinutes = (int)_quotaAutoRefreshInput.Value;
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
        _probeStatus.ForeColor = AppTheme.SecondaryText;

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
                        Model = _modelInputs[kind].Text.Trim(),
                        ThinkingEffort = _effortInputs[kind].SelectedItem is EffortItem item ? item.Effort : ThinkingEffort.Default,
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
        ForeColor = AppTheme.SecondaryText,
        Anchor = AnchorStyles.Left,
        Margin = new Padding(0, 0, 12, 8)
    };
}
