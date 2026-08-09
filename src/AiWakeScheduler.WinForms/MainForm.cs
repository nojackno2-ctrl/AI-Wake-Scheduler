using System.Diagnostics;
using AiWakeScheduler.Core;

namespace AiWakeScheduler.WinForms;

internal sealed class MainForm : Form
{
    private readonly ScheduleManager _manager;
    private readonly CliRunner _runner;
    private readonly AppDataPaths _paths;
    private readonly JsonFileStore<AppSettings> _settingsStore;
    private readonly AppSettings _settings;
    private readonly bool _startMinimized;

    private readonly DataGridView _grid = new();
    private SplitContainer? _mainSplit;
    private readonly TextBox _nameInput = new() { Dock = DockStyle.Fill };
    private readonly DateTimePicker _dateInput = new() { Format = DateTimePickerFormat.Custom, CustomFormat = "yyyy/MM/dd", Dock = DockStyle.Fill };
    private readonly DateTimePicker _timeInput = new() { Format = DateTimePickerFormat.Custom, CustomFormat = "HH:mm:ss", ShowUpDown = true, Dock = DockStyle.Fill };
    private readonly TextBox _messageInput = new() { Text = "早安", Dock = DockStyle.Fill };
    private readonly ComboBox _recurrenceInput = new() { DropDownStyle = ComboBoxStyle.DropDownList, Dock = DockStyle.Fill };
    private readonly TextBox _workingDirectoryInput = new() { Dock = DockStyle.Fill };
    private readonly CheckBox _agyCheck = new() { Text = "Antigravity CLI", Checked = true, AutoSize = true };
    private readonly CheckBox _codexCheck = new() { Text = "Codex CLI", Checked = true, AutoSize = true };
    private readonly CheckBox _claudeCheck = new() { Text = "Claude CLI", Checked = true, AutoSize = true };
    private readonly CheckBox _enabledCheck = new() { Text = "啟用此排程", Checked = true, AutoSize = true };
    private readonly ToolStripStatusLabel _statusLabel = new() { Text = "就緒" };
    private readonly ToolStripStatusLabel _clockLabel = new() { Spring = true, TextAlign = ContentAlignment.MiddleRight };
    private readonly System.Windows.Forms.Timer _uiTimer = new() { Interval = 1000 };
    private readonly NotifyIcon _notifyIcon;
    private Guid? _editingId;
    private bool _reallyExit;
    private bool _refreshingSelection;

    public MainForm(
        ScheduleManager manager,
        CliRunner runner,
        AppDataPaths paths,
        JsonFileStore<AppSettings> settingsStore,
        AppSettings settings,
        bool startMinimized)
    {
        _manager = manager;
        _runner = runner;
        _paths = paths;
        _settingsStore = settingsStore;
        _settings = settings;
        _startMinimized = startMinimized;

        Text = "AI 倒數喚醒";
        StartPosition = FormStartPosition.CenterScreen;
        MinimumSize = new Size(980, 650);
        Size = new Size(1180, 760);
        Font = new Font("Microsoft JhengHei UI", 9F);
        Icon = SystemIcons.Application;

        _notifyIcon = BuildNotifyIcon();
        _recurrenceInput.DataSource = Enum.GetValues<ScheduleRecurrence>();
        _recurrenceInput.FormattingEnabled = true;
        _recurrenceInput.Format += (_, e) =>
        {
            if (e.ListItem is ScheduleRecurrence recurrence)
            {
                e.Value = ScheduleRecurrenceNames.Get(recurrence);
            }
        };
        BuildLayout();
        ResetEditor();

        _manager.JobsChanged += ManagerOnJobsChanged;
        _manager.BackgroundError += ManagerOnBackgroundError;
        _uiTimer.Tick += UiTimerOnTick;
        _uiTimer.Start();

        Shown += MainFormOnShown;
        FormClosing += MainFormOnFormClosing;
    }

    private void BuildLayout()
    {
        var root = new TableLayoutPanel { Dock = DockStyle.Fill, RowCount = 3, ColumnCount = 1 };
        root.RowStyles.Add(new RowStyle(SizeType.AutoSize));
        root.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
        root.RowStyles.Add(new RowStyle(SizeType.AutoSize));

        var header = new Panel { Dock = DockStyle.Fill, Height = 72, BackColor = Color.FromArgb(30, 41, 59), Padding = new Padding(18, 10, 18, 10) };
        header.Controls.Add(new Label
        {
            Text = "AI 倒數喚醒",
            Font = new Font("Microsoft JhengHei UI", 18F, FontStyle.Bold),
            ForeColor = Color.White,
            AutoSize = true,
            Location = new Point(16, 9)
        });
        header.Controls.Add(new Label
        {
            Text = "指定時間向勾選的 CLI 傳送一則簡短訊息（預設：早安）",
            ForeColor = Color.FromArgb(203, 213, 225),
            AutoSize = true,
            Location = new Point(19, 43)
        });
        root.Controls.Add(header, 0, 0);

        var split = new SplitContainer
        {
            Dock = DockStyle.Fill,
            Orientation = Orientation.Vertical,
            Padding = new Padding(12)
        };
        _mainSplit = split;
        BuildScheduleList(split.Panel1);
        BuildEditor(split.Panel2);
        root.Controls.Add(split, 0, 1);

        var status = new StatusStrip();
        status.Items.Add(_statusLabel);
        status.Items.Add(_clockLabel);
        root.Controls.Add(status, 0, 2);
        Controls.Add(root);
    }

    private void BuildScheduleList(Control parent)
    {
        var panel = new TableLayoutPanel { Dock = DockStyle.Fill, RowCount = 3, ColumnCount = 1 };
        panel.RowStyles.Add(new RowStyle(SizeType.AutoSize));
        panel.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
        panel.RowStyles.Add(new RowStyle(SizeType.AutoSize));
        panel.Controls.Add(new Label
        {
            Text = "已儲存排程",
            Font = new Font("Microsoft JhengHei UI", 12F, FontStyle.Bold),
            AutoSize = true,
            Margin = new Padding(0, 0, 0, 8)
        }, 0, 0);

        _grid.Dock = DockStyle.Fill;
        _grid.AllowUserToAddRows = false;
        _grid.AllowUserToDeleteRows = false;
        _grid.AllowUserToResizeRows = false;
        _grid.AutoGenerateColumns = false;
        _grid.BackgroundColor = Color.White;
        _grid.BorderStyle = BorderStyle.Fixed3D;
        _grid.MultiSelect = false;
        _grid.ReadOnly = true;
        _grid.RowHeadersVisible = false;
        _grid.SelectionMode = DataGridViewSelectionMode.FullRowSelect;
        _grid.Columns.Add(new DataGridViewTextBoxColumn { Name = "Name", HeaderText = "名稱", Width = 135 });
        _grid.Columns.Add(new DataGridViewTextBoxColumn { Name = "ScheduledAt", HeaderText = "執行時間", Width = 145 });
        _grid.Columns.Add(new DataGridViewTextBoxColumn { Name = "Countdown", HeaderText = "倒數", Width = 100 });
        _grid.Columns.Add(new DataGridViewTextBoxColumn { Name = "Recurrence", HeaderText = "週期", Width = 58 });
        _grid.Columns.Add(new DataGridViewTextBoxColumn { Name = "Targets", HeaderText = "CLI", AutoSizeMode = DataGridViewAutoSizeColumnMode.Fill, MinimumWidth = 125 });
        _grid.Columns.Add(new DataGridViewTextBoxColumn { Name = "Status", HeaderText = "狀態", Width = 80 });
        _grid.SelectionChanged += GridOnSelectionChanged;
        _grid.CellDoubleClick += (_, _) => ShowSelectedResult();
        panel.Controls.Add(_grid, 0, 1);

        var buttons = new FlowLayoutPanel { Dock = DockStyle.Fill, AutoSize = true, Margin = new Padding(0, 8, 0, 0) };
        buttons.Controls.Add(ActionButton("新增", (_, _) => ResetEditor()));
        buttons.Controls.Add(ActionButton("立即執行", RunSelectedNowAsync));
        buttons.Controls.Add(ActionButton("查看結果", (_, _) => ShowSelectedResult()));
        buttons.Controls.Add(ActionButton("刪除", DeleteSelectedAsync));
        buttons.Controls.Add(ActionButton("開啟日誌資料夾", (_, _) => OpenFolder(_paths.LogsDirectory)));
        panel.Controls.Add(buttons, 0, 2);
        parent.Controls.Add(panel);
    }

    private void BuildEditor(Control parent)
    {
        var editor = new TableLayoutPanel
        {
            Dock = DockStyle.Fill,
            AutoScroll = true,
            ColumnCount = 2,
            RowCount = 10,
            Padding = new Padding(14, 0, 0, 0)
        };
        editor.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 86));
        editor.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
        editor.Controls.Add(new Label
        {
            Text = "排程內容",
            Font = new Font("Microsoft JhengHei UI", 12F, FontStyle.Bold),
            AutoSize = true,
            Margin = new Padding(0, 0, 0, 12)
        }, 0, 0);
        editor.SetColumnSpan(editor.GetControlFromPosition(0, 0)!, 2);

        AddEditorRow(editor, 1, "名稱", _nameInput);
        var timePanel = new TableLayoutPanel { Dock = DockStyle.Fill, ColumnCount = 2, AutoSize = true };
        timePanel.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 55));
        timePanel.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 45));
        timePanel.Controls.Add(_dateInput, 0, 0);
        timePanel.Controls.Add(_timeInput, 1, 0);
        AddEditorRow(editor, 2, "執行時間", timePanel);
        AddEditorRow(editor, 3, "重複", _recurrenceInput);
        AddEditorRow(editor, 4, "訊息", _messageInput);

        var directoryPanel = new TableLayoutPanel { Dock = DockStyle.Fill, ColumnCount = 2, AutoSize = true };
        directoryPanel.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
        directoryPanel.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 70));
        directoryPanel.Controls.Add(_workingDirectoryInput, 0, 0);
        var browse = new Button { Text = "瀏覽…", Dock = DockStyle.Fill };
        browse.Click += BrowseWorkingDirectory;
        directoryPanel.Controls.Add(browse, 1, 0);
        AddEditorRow(editor, 5, "工作目錄", directoryPanel);

        var targets = new FlowLayoutPanel { Dock = DockStyle.Fill, AutoSize = true, FlowDirection = FlowDirection.TopDown, WrapContents = false };
        targets.Controls.Add(_agyCheck);
        targets.Controls.Add(_codexCheck);
        targets.Controls.Add(_claudeCheck);
        AddEditorRow(editor, 6, "傳送至", targets);
        AddEditorRow(editor, 7, string.Empty, _enabledCheck);

        var note = new Label
        {
            Text = "預設節省 Token：每個 CLI 只送一次、不重試、低推理且只要求短回覆。程式可縮到系統匣持續排程。",
            AutoSize = true,
            MaximumSize = new Size(350, 0),
            ForeColor = Color.DimGray,
            Margin = new Padding(0, 12, 0, 12)
        };
        editor.Controls.Add(note, 0, 8);
        editor.SetColumnSpan(note, 2);

        var buttons = new FlowLayoutPanel { Dock = DockStyle.Fill, AutoSize = true };
        var save = ActionButton("儲存排程", SaveScheduleAsync);
        save.BackColor = Color.FromArgb(37, 99, 235);
        save.ForeColor = Color.White;
        save.FlatStyle = FlatStyle.Flat;
        buttons.Controls.Add(save);
        buttons.Controls.Add(ActionButton("CLI 設定…", OpenSettingsAsync));
        editor.Controls.Add(buttons, 0, 9);
        editor.SetColumnSpan(buttons, 2);
        parent.Controls.Add(editor);
    }

    private async void MainFormOnShown(object? sender, EventArgs e)
    {
        if (_mainSplit is not null)
        {
            _mainSplit.Panel1MinSize = 520;
            _mainSplit.Panel2MinSize = 360;
            var available = _mainSplit.ClientSize.Width - _mainSplit.SplitterWidth;
            _mainSplit.SplitterDistance = Math.Clamp((int)(available * 0.62), 520, available - 360);
        }
        await RefreshGridAsync();
        if (_startMinimized)
        {
            BeginInvoke(() =>
            {
                Hide();
                _notifyIcon.Visible = true;
            });
        }
    }

    private async void ManagerOnJobsChanged(object? sender, EventArgs e)
    {
        if (IsDisposed) return;
        try
        {
            if (InvokeRequired)
            {
                BeginInvoke(async () => await RefreshGridAsync());
            }
            else
            {
                await RefreshGridAsync();
            }
        }
        catch (InvalidOperationException) when (IsDisposed)
        {
        }
    }

    private void ManagerOnBackgroundError(object? sender, Exception e)
    {
        if (IsDisposed) return;
        BeginInvoke(() =>
        {
            _statusLabel.Text = $"背景錯誤：{e.Message}";
            _statusLabel.ForeColor = Color.Firebrick;
        });
    }

    private async Task RefreshGridAsync()
    {
        var selectedId = SelectedJob()?.Id ?? _editingId;
        var jobs = await _manager.GetJobsAsync();
        _refreshingSelection = true;
        try
        {
            _grid.Rows.Clear();
            foreach (var job in jobs)
            {
                var rowIndex = _grid.Rows.Add(
                    job.Name,
                    job.ScheduledAt.LocalDateTime.ToString("yyyy/MM/dd HH:mm:ss"),
                    FormatCountdown(job),
                    ScheduleRecurrenceNames.Get(job.Recurrence),
                    string.Join("、", job.Targets.Select(ShortCliName)),
                    StatusText(job.Status));
                _grid.Rows[rowIndex].Tag = job;
                if (job.Id == selectedId)
                {
                    _grid.Rows[rowIndex].Selected = true;
                    _grid.CurrentCell = _grid.Rows[rowIndex].Cells[0];
                }
                ApplyRowStyle(_grid.Rows[rowIndex], job.Status);
            }
        }
        finally
        {
            _refreshingSelection = false;
        }
        UpdateStatusSummary(jobs);
    }

    private async void SaveScheduleAsync(object? sender, EventArgs e)
    {
        try
        {
            var scheduledAt = ReadScheduledAt();
            if (scheduledAt <= DateTimeOffset.Now && MessageBox.Show(
                    "設定時間已到或已過，儲存後會立即執行。要繼續嗎？",
                    Text,
                    MessageBoxButtons.YesNo,
                    MessageBoxIcon.Question) != DialogResult.Yes)
            {
                return;
            }

            var job = new ScheduledJob
            {
                Id = _editingId ?? Guid.NewGuid(),
                Name = _nameInput.Text.Trim(),
                ScheduledAt = scheduledAt,
                Message = _messageInput.Text.Trim(),
                WorkingDirectory = _workingDirectoryInput.Text.Trim(),
                Targets = SelectedTargets(),
                Recurrence = (ScheduleRecurrence)(_recurrenceInput.SelectedItem ?? ScheduleRecurrence.Once),
                Enabled = _enabledCheck.Checked
            };
            await _manager.UpsertAsync(job);
            _editingId = job.Id;
            _statusLabel.Text = "排程已儲存";
            _statusLabel.ForeColor = Color.DarkGreen;
        }
        catch (Exception ex)
        {
            ShowError(ex.Message);
        }
    }

    private async void RunSelectedNowAsync(object? sender, EventArgs e)
    {
        var selected = SelectedJob();
        if (selected is null)
        {
            MessageBox.Show("請先選擇一個已儲存排程。", Text, MessageBoxButtons.OK, MessageBoxIcon.Information);
            return;
        }
        if (MessageBox.Show(
                $"現在向 {string.Join("、", selected.Targets.Select(CliDisplayNames.Get))} 傳送「{selected.Message}」？",
                "立即執行",
                MessageBoxButtons.YesNo,
                MessageBoxIcon.Question) != DialogResult.Yes)
        {
            return;
        }
        try
        {
            await _manager.RunNowAsync(selected.Id);
            _statusLabel.Text = "已交給排程器立即執行";
        }
        catch (Exception ex)
        {
            ShowError(ex.Message);
        }
    }

    private async void DeleteSelectedAsync(object? sender, EventArgs e)
    {
        var selected = SelectedJob();
        if (selected is null) return;
        if (MessageBox.Show($"刪除排程「{selected.Name}」？", Text, MessageBoxButtons.YesNo, MessageBoxIcon.Warning) != DialogResult.Yes)
        {
            return;
        }
        try
        {
            await _manager.DeleteAsync(selected.Id);
            ResetEditor();
        }
        catch (Exception ex)
        {
            ShowError(ex.Message);
        }
    }

    private async void OpenSettingsAsync(object? sender, EventArgs e)
    {
        using var dialog = new SettingsForm(_settings, _runner);
        if (dialog.ShowDialog(this) != DialogResult.OK) return;
        try
        {
            StartupManager.SetEnabled(dialog.ResultSettings.StartWithWindows);
            CopySettings(dialog.ResultSettings, _settings);
            await _settingsStore.SaveAsync(_settings);
            _statusLabel.Text = "設定已儲存";
            _statusLabel.ForeColor = Color.DarkGreen;
        }
        catch (Exception ex)
        {
            ShowError($"設定無法儲存：{ex.Message}");
        }
    }

    private void GridOnSelectionChanged(object? sender, EventArgs e)
    {
        if (_refreshingSelection) return;
        var job = SelectedJob();
        if (job is null) return;
        _editingId = job.Id;
        _nameInput.Text = job.Name;
        _dateInput.Value = ClampPickerValue(_dateInput, job.ScheduledAt.LocalDateTime.Date);
        _timeInput.Value = ClampPickerValue(_timeInput, DateTime.Today + job.ScheduledAt.LocalDateTime.TimeOfDay);
        _messageInput.Text = job.Message;
        _recurrenceInput.SelectedItem = job.Recurrence;
        _workingDirectoryInput.Text = job.WorkingDirectory;
        _agyCheck.Checked = job.Targets.Contains(CliKind.Antigravity);
        _codexCheck.Checked = job.Targets.Contains(CliKind.Codex);
        _claudeCheck.Checked = job.Targets.Contains(CliKind.Claude);
        _enabledCheck.Checked = job.Enabled;
    }

    private void ResetEditor()
    {
        _editingId = null;
        _grid.ClearSelection();
        var proposed = DateTime.Now.AddMinutes(5);
        _nameInput.Text = "AI 倒數喚醒";
        _dateInput.Value = proposed.Date;
        _timeInput.Value = DateTime.Today + proposed.TimeOfDay;
        _messageInput.Text = "早安";
        _recurrenceInput.SelectedItem = ScheduleRecurrence.Once;
        _workingDirectoryInput.Text = _paths.WakeupWorkspace;
        _agyCheck.Checked = _codexCheck.Checked = _claudeCheck.Checked = true;
        _enabledCheck.Checked = true;
    }

    private void UiTimerOnTick(object? sender, EventArgs e)
    {
        _clockLabel.Text = $"現在：{DateTime.Now:yyyy/MM/dd HH:mm:ss}";
        foreach (DataGridViewRow row in _grid.Rows)
        {
            if (row.Tag is ScheduledJob job)
            {
                row.Cells["Countdown"].Value = FormatCountdown(job);
            }
        }
    }

    private void MainFormOnFormClosing(object? sender, FormClosingEventArgs e)
    {
        if (!_reallyExit && _settings.MinimizeToTray && e.CloseReason == CloseReason.UserClosing)
        {
            e.Cancel = true;
            Hide();
            _notifyIcon.Visible = true;
            _notifyIcon.ShowBalloonTip(2500, "AI 倒數喚醒", "程式仍在系統匣執行，排程會繼續倒數。", ToolTipIcon.Info);
            return;
        }

        _uiTimer.Stop();
        _manager.JobsChanged -= ManagerOnJobsChanged;
        _manager.BackgroundError -= ManagerOnBackgroundError;
        _notifyIcon.Visible = false;
        _notifyIcon.Dispose();
    }

    private NotifyIcon BuildNotifyIcon()
    {
        var menu = new ContextMenuStrip();
        menu.Items.Add("開啟主視窗", null, (_, _) => ShowFromTray());
        menu.Items.Add("開啟日誌資料夾", null, (_, _) => OpenFolder(_paths.LogsDirectory));
        menu.Items.Add(new ToolStripSeparator());
        menu.Items.Add("結束程式", null, (_, _) =>
        {
            _reallyExit = true;
            Close();
        });
        var icon = new NotifyIcon
        {
            Text = "AI 倒數喚醒",
            Icon = SystemIcons.Application,
            ContextMenuStrip = menu,
            Visible = true
        };
        icon.DoubleClick += (_, _) => ShowFromTray();
        return icon;
    }

    private void ShowFromTray()
    {
        Show();
        WindowState = FormWindowState.Normal;
        Activate();
    }

    private void ShowSelectedResult()
    {
        var job = SelectedJob();
        if (job is null)
        {
            MessageBox.Show("請先選擇排程。", Text, MessageBoxButtons.OK, MessageBoxIcon.Information);
            return;
        }
        if (job.LastResults.Count == 0)
        {
            MessageBox.Show("這個排程尚無執行結果。", Text, MessageBoxButtons.OK, MessageBoxIcon.Information);
            return;
        }

        var details = string.Join(Environment.NewLine + Environment.NewLine, job.LastResults.Select(result =>
            $"{CliDisplayNames.Get(result.Cli)}：{(result.Succeeded ? "成功" : "失敗")}{Environment.NewLine}" +
            $"結束碼：{result.ExitCode?.ToString() ?? "無"}{Environment.NewLine}" +
            $"錯誤：{(string.IsNullOrWhiteSpace(result.Error) ? "無" : result.Error)}{Environment.NewLine}" +
            $"日誌：{result.LogPath}"));
        MessageBox.Show(details, $"{job.Name}－執行結果", MessageBoxButtons.OK,
            job.LastResults.All(result => result.Succeeded) ? MessageBoxIcon.Information : MessageBoxIcon.Warning);
    }

    private ScheduledJob? SelectedJob() => _grid.SelectedRows.Count == 1
        ? _grid.SelectedRows[0].Tag as ScheduledJob
        : null;

    private List<CliKind> SelectedTargets()
    {
        var result = new List<CliKind>();
        if (_agyCheck.Checked) result.Add(CliKind.Antigravity);
        if (_codexCheck.Checked) result.Add(CliKind.Codex);
        if (_claudeCheck.Checked) result.Add(CliKind.Claude);
        return result;
    }

    private DateTimeOffset ReadScheduledAt()
    {
        var local = DateTime.SpecifyKind(_dateInput.Value.Date + _timeInput.Value.TimeOfDay, DateTimeKind.Local);
        return new DateTimeOffset(local);
    }

    private void BrowseWorkingDirectory(object? sender, EventArgs e)
    {
        using var dialog = new FolderBrowserDialog
        {
            Description = "選擇三個 CLI 執行時使用的工作目錄",
            UseDescriptionForTitle = true,
            InitialDirectory = Directory.Exists(_workingDirectoryInput.Text) ? _workingDirectoryInput.Text : string.Empty
        };
        if (dialog.ShowDialog(this) == DialogResult.OK)
        {
            _workingDirectoryInput.Text = dialog.SelectedPath;
        }
    }

    private static void OpenFolder(string path)
    {
        Directory.CreateDirectory(path);
        Process.Start(new ProcessStartInfo("explorer.exe", path) { UseShellExecute = true });
    }

    private static void AddEditorRow(TableLayoutPanel panel, int row, string label, Control control)
    {
        panel.RowStyles.Add(new RowStyle(SizeType.AutoSize));
        panel.Controls.Add(new Label
        {
            Text = label,
            AutoSize = true,
            Anchor = AnchorStyles.Left,
            Margin = new Padding(0, 7, 8, 10)
        }, 0, row);
        control.Margin = new Padding(0, 3, 0, 10);
        panel.Controls.Add(control, 1, row);
    }

    private static Button ActionButton(string text, EventHandler handler)
    {
        var button = new Button { Text = text, AutoSize = true, Padding = new Padding(4, 1, 4, 1) };
        button.Click += handler;
        return button;
    }

    private static string FormatCountdown(ScheduledJob job)
    {
        if (job.Status == ScheduleStatus.Running) return "執行中";
        if (job.Status is ScheduleStatus.Completed or ScheduleStatus.Failed) return "已執行";
        if (!job.Enabled) return "已停用";
        var remaining = job.ScheduledAt - DateTimeOffset.Now;
        if (remaining <= TimeSpan.Zero) return "即將執行";
        return remaining.TotalDays >= 1
            ? $"{(int)remaining.TotalDays}天 {remaining:hh\\:mm\\:ss}"
            : remaining.ToString(@"hh\:mm\:ss");
    }

    private static string StatusText(ScheduleStatus status) => status switch
    {
        ScheduleStatus.Pending => "等待中",
        ScheduleStatus.Running => "執行中",
        ScheduleStatus.Completed => "成功",
        ScheduleStatus.Failed => "失敗",
        ScheduleStatus.Disabled => "已停用",
        _ => status.ToString()
    };

    private static string ShortCliName(CliKind kind) => kind switch
    {
        CliKind.Antigravity => "AGY",
        CliKind.Codex => "Codex",
        CliKind.Claude => "Claude",
        _ => kind.ToString()
    };

    private static void ApplyRowStyle(DataGridViewRow row, ScheduleStatus status)
    {
        row.DefaultCellStyle.ForeColor = status switch
        {
            ScheduleStatus.Completed => Color.DarkGreen,
            ScheduleStatus.Failed => Color.Firebrick,
            ScheduleStatus.Disabled => Color.Gray,
            _ => SystemColors.ControlText
        };
    }

    private void UpdateStatusSummary(IReadOnlyCollection<ScheduledJob> jobs)
    {
        var pending = jobs.Count(job => job.Enabled && job.Status == ScheduleStatus.Pending);
        var failed = jobs.Count(job => job.Status == ScheduleStatus.Failed);
        _statusLabel.Text = $"等待中：{pending}　失敗：{failed}";
        _statusLabel.ForeColor = failed > 0 ? Color.Firebrick : SystemColors.ControlText;
    }

    private static DateTime ClampPickerValue(DateTimePicker picker, DateTime value) =>
        value < picker.MinDate ? picker.MinDate : value > picker.MaxDate ? picker.MaxDate : value;

    private static void CopySettings(AppSettings source, AppSettings destination)
    {
        destination.StartWithWindows = source.StartWithWindows;
        destination.MinimizeToTray = source.MinimizeToTray;
        destination.TokenSaverMode = source.TokenSaverMode;
        destination.ExecutionTimeoutMinutes = source.ExecutionTimeoutMinutes;
        destination.CliProfiles = source.CliProfiles.ToDictionary(pair => pair.Key, pair => pair.Value.Clone());
    }

    private void ShowError(string message)
    {
        _statusLabel.Text = message;
        _statusLabel.ForeColor = Color.Firebrick;
        MessageBox.Show(message, Text, MessageBoxButtons.OK, MessageBoxIcon.Error);
    }
}
