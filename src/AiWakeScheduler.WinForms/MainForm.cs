using System.Diagnostics;
using AiWakeScheduler.Core;

namespace AiWakeScheduler.WinForms;

internal sealed class MainForm : Form
{
    private const int ColumnName = 0;
    private const int ColumnTime = 1;
    private const int ColumnCountdown = 2;
    private const int ColumnTargets = 3;
    private const int ColumnStatus = 4;

    private readonly AppHost _host;
    private readonly bool _startMinimized;

    private readonly DataGridView _grid = new();
    private readonly TextBox _nameInput = new() { Dock = DockStyle.Fill };
    private readonly DateTimePicker _timeInput = new() { Format = DateTimePickerFormat.Custom, CustomFormat = "HH:mm", ShowUpDown = true, Dock = DockStyle.Fill };
    private readonly TextBox _messageInput = new() { Text = "早安", Dock = DockStyle.Fill, MaxLength = 50 };
    private readonly TextBox _workingDirectoryInput = new() { Dock = DockStyle.Fill };
    private readonly CheckBox _enabledCheck = new() { Text = "啟用此排程", Checked = true, AutoSize = true };
    private readonly ToolStripStatusLabel _statusLabel = new() { Text = "就緒" };
    private readonly ToolStripStatusLabel _clockLabel = new() { Spring = true, TextAlign = ContentAlignment.MiddleRight };
    private readonly System.Windows.Forms.Timer _uiTimer = new() { Interval = 1000 };
    private readonly Dictionary<CliKind, CheckBox> _targetChecks = [];
    private readonly Dictionary<CliKind, Label> _usageLabels = [];
    private readonly Dictionary<CliKind, CliUsageSnapshot> _usageSnapshots = [];
    private readonly NotifyIcon _notifyIcon;

    private SplitContainer? _mainSplit;
    private Button? _saveButton;
    private Button? _refreshUsageButton;
    private Label? _emptyStateLabel;
    private CancellationTokenSource? _usageRefreshCancellation;
    private Guid? _editingId;
    private bool _reallyExit;
    private bool _refreshingSelection;
    private int _refreshRequested;
    private bool _refreshDeferred;
    private string _lastClockText = string.Empty;

    public MainForm(AppHost host, bool startMinimized)
    {
        _host = host;
        _startMinimized = startMinimized;

        Text = "AI 倒數喚醒";
        StartPosition = FormStartPosition.CenterScreen;
        AutoScaleDimensions = new SizeF(7F, 15F);
        AutoScaleMode = AutoScaleMode.Font;
        Font = AppTheme.Body;
        AppTheme.ApplyForm(this);
        ApplyPreferredSize();

        // 主視窗自身也開雙緩衝，避免調整大小時整片重繪造成閃爍。
        SetStyle(ControlStyles.OptimizedDoubleBuffer | ControlStyles.AllPaintingInWmPaint, true);

        var appIcon = GetApplicationIcon();
        Icon = appIcon;

        _notifyIcon = BuildNotifyIcon(appIcon);
        BuildLayout();
        ResetEditor();

        _host.Manager.JobsChanged += ManagerOnJobsChanged;
        _host.Manager.BackgroundError += ManagerOnBackgroundError;
        _uiTimer.Tick += UiTimerOnTick;

        Shown += MainFormOnShown;
        FormClosing += MainFormOnFormClosing;
    }

    private ScheduleManager Manager => _host.Manager;

    protected override void Dispose(bool disposing)
    {
        if (disposing)
        {
            _uiTimer.Stop();
            _uiTimer.Tick -= UiTimerOnTick;
            _uiTimer.Dispose();

            Manager.JobsChanged -= ManagerOnJobsChanged;
            Manager.BackgroundError -= ManagerOnBackgroundError;

            _usageRefreshCancellation?.Cancel();
            _usageRefreshCancellation?.Dispose();
            _usageRefreshCancellation = null;

            if (_notifyIcon is not null)
            {
                _notifyIcon.Visible = false;
                _notifyIcon.ContextMenuStrip?.Dispose();
                _notifyIcon.ContextMenuStrip = null;
                _notifyIcon.Dispose();
            }
        }
        base.Dispose(disposing);
    }

    /// <summary>依目前螢幕工作區決定視窗大小，避免在小螢幕或高 DPI 下超出畫面。</summary>
    private void ApplyPreferredSize()
    {
        var workingArea = Screen.PrimaryScreen?.WorkingArea ?? new Rectangle(0, 0, 1280, 800);
        MinimumSize = new Size(
            Math.Min(980, workingArea.Width),
            Math.Min(650, workingArea.Height));
        Size = new Size(
            Math.Min(1180, workingArea.Width),
            Math.Min(760, workingArea.Height));
    }

    private void BuildLayout()
    {
        SuspendLayout();

        AppTheme.StyleInput(_nameInput);
        AppTheme.StyleInput(_timeInput);
        AppTheme.StyleInput(_messageInput);
        AppTheme.StyleInput(_workingDirectoryInput);

        var root = new TableLayoutPanel
        {
            Dock = DockStyle.Fill,
            RowCount = 3,
            ColumnCount = 1,
            BackColor = AppTheme.Canvas
        };
        root.RowStyles.Add(new RowStyle(SizeType.AutoSize));
        root.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
        root.RowStyles.Add(new RowStyle(SizeType.AutoSize));
        root.Controls.Add(BuildHeader(), 0, 0);

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

        var status = new StatusStrip
        {
            BackColor = AppTheme.Panel,
            ForeColor = AppTheme.SecondaryText,
            SizingGrip = false,
            Padding = new Padding(14, 5, 14, 5)
        };
        status.Items.Add(_statusLabel);
        status.Items.Add(_clockLabel);
        root.Controls.Add(status, 0, 2);

        Controls.Add(root);
        ResumeLayout(performLayout: true);
    }

    /// <summary>
    /// 標題列改用 TableLayoutPanel。原本用絕對座標擺放標籤，
    /// 在非 100% 縮放的螢幕上會錯位。
    /// </summary>
    private static Control BuildHeader()
    {
        var header = new TableLayoutPanel
        {
            Dock = DockStyle.Fill,
            AutoSize = true,
            AutoSizeMode = AutoSizeMode.GrowAndShrink,
            ColumnCount = 1,
            RowCount = 2,
            BackColor = AppTheme.Banner,
            Padding = new Padding(24, 16, 24, 18)
        };
        header.RowStyles.Add(new RowStyle(SizeType.AutoSize));
        header.RowStyles.Add(new RowStyle(SizeType.AutoSize));
        header.Controls.Add(new Label
        {
            Text = "AI 倒數喚醒",
            Font = AppTheme.HeaderTitle,
            ForeColor = AppTheme.BannerText,
            AutoSize = true,
            Margin = new Padding(0)
        }, 0, 0);
        header.Controls.Add(new Label
        {
            Text = "每天在指定的幾點幾分，向勾選的 CLI 傳送一則簡短訊息（預設：早安）",
            ForeColor = AppTheme.BannerSubtitle,
            AutoSize = true,
            Margin = new Padding(1, 5, 0, 0)
        }, 0, 1);
        return header;
    }

    private void BuildScheduleList(Control parent)
    {
        var panel = new TableLayoutPanel
        {
            Dock = DockStyle.Fill,
            RowCount = 3,
            ColumnCount = 1,
            BackColor = AppTheme.Panel,
            Padding = new Padding(16)
        };
        panel.RowStyles.Add(new RowStyle(SizeType.AutoSize));
        panel.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
        panel.RowStyles.Add(new RowStyle(SizeType.AutoSize));
        panel.Controls.Add(new Label
        {
            Text = "已儲存排程",
            Font = AppTheme.SectionTitle,
            AutoSize = true,
            Margin = new Padding(0, 0, 0, 12)
        }, 0, 0);

        _grid.Dock = DockStyle.Fill;
        _grid.AllowUserToAddRows = false;
        _grid.AllowUserToDeleteRows = false;
        _grid.AllowUserToResizeRows = false;
        _grid.AutoGenerateColumns = false;
        _grid.BackgroundColor = AppTheme.Panel;
        _grid.BorderStyle = BorderStyle.FixedSingle;
        _grid.CellBorderStyle = DataGridViewCellBorderStyle.SingleHorizontal;
        _grid.ColumnHeadersBorderStyle = DataGridViewHeaderBorderStyle.None;
        _grid.ColumnHeadersHeight = 40;
        _grid.ColumnHeadersHeightSizeMode = DataGridViewColumnHeadersHeightSizeMode.DisableResizing;
        _grid.EnableHeadersVisualStyles = false;
        _grid.GridColor = AppTheme.Divider;
        _grid.MultiSelect = false;
        _grid.ReadOnly = true;
        _grid.RowTemplate.Height = 42;
        _grid.RowHeadersVisible = false;
        _grid.SelectionMode = DataGridViewSelectionMode.FullRowSelect;
        _grid.DefaultCellStyle.BackColor = AppTheme.Panel;
        _grid.DefaultCellStyle.ForeColor = AppTheme.PrimaryText;
        _grid.DefaultCellStyle.SelectionBackColor = AppTheme.Selected;
        _grid.DefaultCellStyle.SelectionForeColor = AppTheme.SelectedText;
        _grid.DefaultCellStyle.Padding = new Padding(6, 2, 6, 2);
        _grid.AlternatingRowsDefaultCellStyle.BackColor = AppTheme.Canvas;
        _grid.ColumnHeadersDefaultCellStyle.BackColor = AppTheme.PanelSubtle;
        _grid.ColumnHeadersDefaultCellStyle.ForeColor = AppTheme.PrimaryText;
        _grid.ColumnHeadersDefaultCellStyle.Font = AppTheme.TableHeader;
        _grid.ColumnHeadersDefaultCellStyle.Padding = new Padding(6, 0, 6, 0);
        _grid.Columns.Add(new DataGridViewTextBoxColumn { Name = "Name", HeaderText = "名稱", Width = 135 });
        _grid.Columns.Add(new DataGridViewTextBoxColumn { Name = "ScheduledAt", HeaderText = "每天時間", Width = 90 });
        _grid.Columns.Add(new DataGridViewTextBoxColumn { Name = "Countdown", HeaderText = "倒數", Width = 100 });
        _grid.Columns.Add(new DataGridViewTextBoxColumn { Name = "Targets", HeaderText = "CLI", AutoSizeMode = DataGridViewAutoSizeColumnMode.Fill, MinimumWidth = 125 });
        _grid.Columns.Add(new DataGridViewTextBoxColumn { Name = "Status", HeaderText = "狀態", Width = 80 });
        _grid.SelectionChanged += GridOnSelectionChanged;
        _grid.CellDoubleClick += (_, _) => ShowSelectedResult();
        NativeMethods.EnableDoubleBuffering(_grid);
        var gridHost = new Panel { Dock = DockStyle.Fill, BackColor = AppTheme.Panel };
        gridHost.Controls.Add(_grid);
        _emptyStateLabel = new Label
        {
            Text = "目前沒有排程\r\n選擇下方的「新增」，建立每天自動喚醒。",
            AutoSize = false,
            Dock = DockStyle.Fill,
            TextAlign = ContentAlignment.MiddleCenter,
            ForeColor = AppTheme.SecondaryText,
            BackColor = AppTheme.Panel,
            Font = AppTheme.Body,
            Visible = false
        };
        gridHost.Controls.Add(_emptyStateLabel);
        panel.Controls.Add(gridHost, 0, 1);

        var buttons = new FlowLayoutPanel { Dock = DockStyle.Fill, AutoSize = true, Margin = new Padding(0, 12, 0, 0) };
        buttons.Controls.Add(ActionButton("新增", (_, _) => ResetEditor(), AppTheme.ButtonVariant.Primary));
        buttons.Controls.Add(ActionButton("立即執行", RunSelectedNowAsync));
        buttons.Controls.Add(ActionButton("查看結果", (_, _) => ShowSelectedResult()));
        buttons.Controls.Add(ActionButton("刪除", DeleteSelectedAsync, AppTheme.ButtonVariant.Danger));
        buttons.Controls.Add(ActionButton("開啟日誌資料夾", (_, _) => OpenFolder(_host.Paths.LogsDirectory)));
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
            Padding = new Padding(18, 16, 14, 16),
            BackColor = AppTheme.Canvas
        };
        editor.ColumnStyles.Add(new ColumnStyle(SizeType.AutoSize));
        editor.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));

        var title = new Label
        {
            Text = "排程內容",
            Font = AppTheme.SectionTitle,
            AutoSize = true,
            Margin = new Padding(0, 0, 0, 16)
        };
        editor.Controls.Add(title, 0, 0);
        editor.SetColumnSpan(title, 2);

        AddEditorRow(editor, 1, "名稱", _nameInput);
        AddEditorRow(editor, 2, "每天時間", _timeInput);
        AddEditorRow(editor, 3, "訊息", _messageInput);

        var directoryPanel = new TableLayoutPanel { Dock = DockStyle.Fill, ColumnCount = 2, AutoSize = true };
        directoryPanel.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
        directoryPanel.ColumnStyles.Add(new ColumnStyle(SizeType.AutoSize));
        directoryPanel.Controls.Add(_workingDirectoryInput, 0, 0);
        var browse = new Button { Text = "瀏覽…", AutoSize = true, Dock = DockStyle.Fill };
        AppTheme.StyleButton(browse);
        browse.Click += BrowseWorkingDirectory;
        directoryPanel.Controls.Add(browse, 1, 0);
        AddEditorRow(editor, 4, "工作目錄", directoryPanel);

        // CLI 勾選項直接由目錄產生，新增 CLI 不需要改動視窗程式碼。
        var targets = new FlowLayoutPanel { Dock = DockStyle.Fill, AutoSize = true, FlowDirection = FlowDirection.TopDown, WrapContents = false };
        foreach (var descriptor in CliCatalog.All)
        {
            var check = new CheckBox { Text = descriptor.DisplayName, Checked = true, AutoSize = true, Margin = new Padding(0, 2, 0, 5) };
            _targetChecks[descriptor.Kind] = check;
            targets.Controls.Add(check);
        }
        AddEditorRow(editor, 5, "傳送至", targets);
        AddEditorRow(editor, 6, string.Empty, _enabledCheck);

        var usagePanel = BuildUsagePanel();
        editor.Controls.Add(usagePanel, 0, 7);
        editor.SetColumnSpan(usagePanel, 2);

        var note = new Label
        {
            Text = "此排程每天在指定時分執行。節省 Token 模式會停用工具、MCP 伺服器與專案說明檔，"
                 + "並以最低推理量要求最短回覆，把每次喚醒的用量壓到接近下限。",
            AutoSize = true,
            MaximumSize = new Size(420, 0),
            ForeColor = AppTheme.SecondaryText,
            Font = AppTheme.Caption,
            Margin = new Padding(0, 14, 0, 14)
        };
        editor.Controls.Add(note, 0, 8);
        editor.SetColumnSpan(note, 2);

        var buttons = new FlowLayoutPanel { Dock = DockStyle.Fill, AutoSize = true };
        _saveButton = ActionButton("建立排程", SaveScheduleAsync, AppTheme.ButtonVariant.Primary);
        buttons.Controls.Add(_saveButton);
        buttons.Controls.Add(ActionButton("CLI 設定…", OpenSettingsAsync));
        editor.Controls.Add(buttons, 0, 9);
        editor.SetColumnSpan(buttons, 2);
        parent.Controls.Add(editor);
    }

    private Control BuildUsagePanel()
    {
        var group = new GroupBox
        {
            Text = "剩餘流量與重置倒數",
            Dock = DockStyle.Fill,
            AutoSize = true,
            Margin = new Padding(0, 8, 0, 8)
        };
        AppTheme.StyleGroup(group);
        var table = new TableLayoutPanel { Dock = DockStyle.Fill, AutoSize = true, ColumnCount = 2 };
        table.ColumnStyles.Add(new ColumnStyle(SizeType.AutoSize));
        table.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));

        var row = 0;
        foreach (var descriptor in CliCatalog.All)
        {
            table.RowStyles.Add(new RowStyle(SizeType.AutoSize));
            table.Controls.Add(new Label
            {
                Text = descriptor.ShortName,
                AutoSize = true,
                Font = AppTheme.TableHeader,
                Margin = new Padding(0, 7, 14, 7)
            }, 0, row);
            var value = new Label
            {
                Text = "尚未讀取",
                AutoSize = true,
                MaximumSize = new Size(430, 0),
                ForeColor = AppTheme.SecondaryText,
                Margin = new Padding(0, 7, 0, 7)
            };
            _usageLabels[descriptor.Kind] = value;
            table.Controls.Add(value, 1, row);
            row++;
        }

        _refreshUsageButton = ActionButton("重新讀取額度", RefreshUsageOnClick);
        _refreshUsageButton.Margin = new Padding(0, 7, 0, 0);
        table.Controls.Add(_refreshUsageButton, 0, row);
        table.SetColumnSpan(_refreshUsageButton, 2);
        group.Controls.Add(table);
        return group;
    }

    private async void MainFormOnShown(object? sender, EventArgs e)
    {
        ApplySplitterLayout();
        await RefreshGridAsync().ConfigureAwait(true);

        if (_startMinimized)
        {
            BeginInvoke(HideToTray);
        }
        else
        {
            StartClock();
            await RefreshUsageAsync(showStatus: false).ConfigureAwait(true);
        }
    }

    private void ApplySplitterLayout()
    {
        if (_mainSplit is null)
        {
            return;
        }

        var available = _mainSplit.ClientSize.Width - _mainSplit.SplitterWidth;
        // 視窗較窄時硬套最小寬度會擲出例外，先確認空間夠再設定。
        var panel1Min = Math.Min(520, Math.Max(120, available / 2));
        var panel2Min = Math.Min(360, Math.Max(120, available - panel1Min - 1));
        if (available <= panel1Min + panel2Min)
        {
            return;
        }

        try
        {
            _mainSplit.Panel1MinSize = panel1Min;
            _mainSplit.Panel2MinSize = panel2Min;
            _mainSplit.SplitterDistance = Math.Clamp((int)(available * 0.62), panel1Min, available - panel2Min);
        }
        catch (InvalidOperationException)
        {
            // 版面尚未穩定時交給預設分割位置
        }
        catch (ArgumentOutOfRangeException)
        {
        }
    }

    // ── 排程清單更新 ──────────────────────────────────────────────

    /// <summary>
    /// 排程異動事件。四個 CLI 平行完成時會連續觸發數次，
    /// 這裡把它們合併成一次實際更新，避免重複重繪整個清單。
    /// </summary>
    private void ManagerOnJobsChanged(object? sender, EventArgs e) => RequestRefresh();

    private void RequestRefresh()
    {
        if (IsDisposed || !IsHandleCreated)
        {
            return;
        }

        // 已經有一次更新排隊中就直接略過。
        if (Interlocked.Exchange(ref _refreshRequested, 1) == 1)
        {
            return;
        }

        try
        {
            BeginInvoke(RefreshRequestedCore);
        }
        catch (InvalidOperationException)
        {
            Interlocked.Exchange(ref _refreshRequested, 0);
        }
    }

    private async void RefreshRequestedCore()
    {
        Interlocked.Exchange(ref _refreshRequested, 0);

        // 縮在系統匣時不需要維護看不見的清單，只記下它已經過期。
        if (!Visible || WindowState == FormWindowState.Minimized)
        {
            _refreshDeferred = true;
            return;
        }

        await RefreshGridAsync().ConfigureAwait(true);
    }

    private async Task RefreshGridAsync()
    {
        if (IsDisposed)
        {
            return;
        }

        try
        {
            var jobs = await Manager.GetJobsAsync().ConfigureAwait(true);
            if (IsDisposed)
            {
                return;
            }

            _refreshDeferred = false;
            ApplyJobs(jobs);
            UpdateStatusSummary(jobs);
        }
        catch (ObjectDisposedException)
        {
        }
        catch (InvalidOperationException) when (IsDisposed)
        {
        }
    }

    /// <summary>
    /// 就地更新表格內容，只改真正變動的儲存格。
    /// 原本每次都 Clear + Add，等於重新配置所有列物件並強制整片重繪。
    /// </summary>
    private void ApplyJobs(IReadOnlyList<ScheduledJob> jobs)
    {
        var selectedId = SelectedJob()?.Id ?? _editingId;
        var now = DateTimeOffset.Now;

        _refreshingSelection = true;
        _grid.SuspendLayout();
        try
        {
            while (_grid.Rows.Count > jobs.Count)
            {
                _grid.Rows.RemoveAt(_grid.Rows.Count - 1);
            }
            while (_grid.Rows.Count < jobs.Count)
            {
                _grid.Rows.Add();
            }

            _grid.Visible = jobs.Count > 0;
            if (_emptyStateLabel is not null)
            {
                _emptyStateLabel.Visible = jobs.Count == 0;
                _emptyStateLabel.BringToFront();
            }

            for (var i = 0; i < jobs.Count; i++)
            {
                var job = jobs[i];
                var row = _grid.Rows[i];
                row.Tag = job;

                SetCell(row, ColumnName, job.Name);
                SetCell(row, ColumnTime, JobPresenter.Time(job));
                SetCell(row, ColumnCountdown, JobPresenter.Countdown(job, now));
                SetCell(row, ColumnTargets, JobPresenter.Targets(job.Targets));
                SetCell(row, ColumnStatus, JobPresenter.Status(job.Status));

                var color = JobPresenter.StatusColor(job.Status);
                if (row.DefaultCellStyle.ForeColor != color)
                {
                    row.DefaultCellStyle.ForeColor = color;
                }

                if (job.Id == selectedId && !row.Selected)
                {
                    row.Selected = true;
                    _grid.CurrentCell = row.Cells[ColumnName];
                }
            }
        }
        finally
        {
            _grid.ResumeLayout(performLayout: true);
            _refreshingSelection = false;
        }
    }

    private static void SetCell(DataGridViewRow row, int index, string value)
    {
        var cell = row.Cells[index];
        if (cell.Value is not string current || !string.Equals(current, value, StringComparison.Ordinal))
        {
            cell.Value = value;
        }
    }

    private async void RefreshUsageOnClick(object? sender, EventArgs e) =>
        await RefreshUsageAsync(showStatus: true).ConfigureAwait(true);

    /// <summary>
    /// 額度只在開啟主視窗或使用者要求時查詢一次；之後每秒只用已取得的
    /// resetsAt 更新本地倒數，不會持續啟動 CLI 或發出網路要求。
    /// </summary>
    private async Task RefreshUsageAsync(bool showStatus)
    {
        var cancellation = new CancellationTokenSource();
        var previous = Interlocked.Exchange(ref _usageRefreshCancellation, cancellation);
        previous?.Cancel();
        previous?.Dispose();

        if (_refreshUsageButton is not null)
        {
            _refreshUsageButton.Enabled = false;
            _refreshUsageButton.Text = "讀取中…";
        }

        try
        {
            var descriptors = CliCatalog.All;
            var tasks = new Task<CliUsageSnapshot>[descriptors.Count];
            for (var i = 0; i < descriptors.Count; i++)
            {
                var kind = descriptors[i].Kind;
                var profile = _host.Settings.CliProfiles[kind].Clone();
                tasks[i] = _host.UsageReader.ReadAsync(
                    kind,
                    profile,
                    _host.Paths.WakeupWorkspace,
                    cancellation.Token);
            }

            var snapshots = await Task.WhenAll(tasks).ConfigureAwait(true);
            if (IsDisposed || cancellation.IsCancellationRequested)
            {
                return;
            }

            _usageSnapshots.Clear();
            for (var i = 0; i < snapshots.Length; i++)
            {
                _usageSnapshots[snapshots[i].Cli] = snapshots[i];
            }
            UpdateUsageLabels(DateTimeOffset.Now);

            if (showStatus)
            {
                var codex = snapshots.First(snapshot => snapshot.Cli == CliKind.Codex);
                SetStatus(
                    codex.Availability == CliUsageAvailability.Available
                        ? "Codex 剩餘流量已更新"
                        : $"額度讀取：{codex.Message}",
                    codex.Availability == CliUsageAvailability.Unavailable ? AppTheme.Danger : SystemColors.ControlText);
            }
        }
        catch (OperationCanceledException)
        {
        }
        catch (Exception ex)
        {
            if (!IsDisposed)
            {
                SetStatus($"額度讀取失敗：{ex.Message}", AppTheme.Danger);
            }
        }
        finally
        {
            if (ReferenceEquals(Interlocked.CompareExchange(ref _usageRefreshCancellation, null, cancellation), cancellation))
            {
                cancellation.Dispose();
                if (!IsDisposed && _refreshUsageButton is not null)
                {
                    _refreshUsageButton.Enabled = true;
                    _refreshUsageButton.Text = "重新讀取額度";
                }
            }
        }
    }

    private void UpdateUsageLabels(DateTimeOffset now)
    {
        foreach (var descriptor in CliCatalog.All)
        {
            if (!_usageLabels.TryGetValue(descriptor.Kind, out var label) ||
                !_usageSnapshots.TryGetValue(descriptor.Kind, out var snapshot))
            {
                continue;
            }

            label.Text = FormatUsage(snapshot, now);
            label.ForeColor = snapshot.Availability switch
            {
                CliUsageAvailability.Available => AppTheme.Success,
                CliUsageAvailability.Unavailable => AppTheme.Danger,
                _ => AppTheme.SecondaryText
            };
        }
    }

    private static string FormatUsage(CliUsageSnapshot snapshot, DateTimeOffset now)
    {
        if (snapshot.Availability != CliUsageAvailability.Available)
        {
            return snapshot.Message;
        }

        return string.Join("；", snapshot.Windows.Select(window =>
        {
            var resetText = window.ResetsAt is { } resetsAt
                ? FormatResetCountdown(resetsAt, now)
                : "重置時間未提供";
            return $"{window.Name}：剩餘 {window.RemainingPercent}%（{resetText}）";
        }));
    }

    private static string FormatResetCountdown(DateTimeOffset resetsAt, DateTimeOffset now)
    {
        var remaining = resetsAt - now;
        if (remaining <= TimeSpan.Zero)
        {
            return "等待伺服器更新";
        }

        var countdown = remaining.Days > 0
            ? $"{remaining.Days}天 {remaining.Hours:00}:{remaining.Minutes:00}:{remaining.Seconds:00}"
            : $"{(int)remaining.TotalHours:00}:{remaining.Minutes:00}:{remaining.Seconds:00}";
        return $"{countdown} 後重置，{resetsAt.LocalDateTime:MM/dd HH:mm}";
    }

    // ── 使用者操作 ────────────────────────────────────────────────

    private async void SaveScheduleAsync(object? sender, EventArgs e)
    {
        try
        {
            var job = new ScheduledJob
            {
                Id = _editingId ?? Guid.NewGuid(),
                Name = _nameInput.Text.Trim(),
                ScheduledAt = ReadScheduledAt(),
                Message = _messageInput.Text.Trim(),
                WorkingDirectory = _workingDirectoryInput.Text.Trim(),
                Targets = SelectedTargets(),
                Recurrence = ScheduleRecurrence.Daily,
                Enabled = _enabledCheck.Checked
            };
            await Manager.UpsertAsync(job).ConfigureAwait(true);
            _editingId = job.Id;
            if (_saveButton is not null)
            {
                _saveButton.Text = "儲存修改";
            }
            SetStatus("排程已儲存", AppTheme.Success);
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
                $"現在向 {JobPresenter.TargetsLong(selected.Targets)} 傳送「{selected.Message}」？",
                "立即執行",
                MessageBoxButtons.YesNo,
                MessageBoxIcon.Question) != DialogResult.Yes)
        {
            return;
        }

        try
        {
            await Manager.RunNowAsync(selected.Id).ConfigureAwait(true);
            SetStatus("已交給排程器立即執行", SystemColors.ControlText);
        }
        catch (Exception ex)
        {
            ShowError(ex.Message);
        }
    }

    private async void DeleteSelectedAsync(object? sender, EventArgs e)
    {
        var selected = SelectedJob();
        if (selected is null)
        {
            return;
        }

        if (MessageBox.Show($"刪除排程「{selected.Name}」？", Text, MessageBoxButtons.YesNo, MessageBoxIcon.Warning) != DialogResult.Yes)
        {
            return;
        }

        try
        {
            await Manager.DeleteAsync(selected.Id).ConfigureAwait(true);
            ResetEditor();
        }
        catch (Exception ex)
        {
            ShowError(ex.Message);
        }
    }

    private async void OpenSettingsAsync(object? sender, EventArgs e)
    {
        using var dialog = new SettingsForm(_host.Settings, _host.Runner);
        if (dialog.ShowDialog(this) != DialogResult.OK)
        {
            return;
        }

        try
        {
            StartupManager.SetEnabled(dialog.ResultSettings.StartWithWindows);
            dialog.ResultSettings.CopyTo(_host.Settings);
            // 使用者可能改了可執行檔路徑，讓下一次執行重新解析。
            ExecutableLocator.ClearCache();
            await _host.SaveSettingsAsync().ConfigureAwait(true);
            await RefreshUsageAsync(showStatus: false).ConfigureAwait(true);
            SetStatus("設定已儲存", AppTheme.Success);
        }
        catch (Exception ex)
        {
            ShowError($"設定無法儲存：{ex.Message}");
        }
    }

    private void GridOnSelectionChanged(object? sender, EventArgs e)
    {
        if (_refreshingSelection)
        {
            return;
        }

        var job = SelectedJob();
        if (job is null)
        {
            return;
        }

        _editingId = job.Id;
        if (_saveButton is not null)
        {
            _saveButton.Text = "儲存修改";
        }
        _nameInput.Text = job.Name;
        _timeInput.Value = ClampPickerValue(_timeInput, DateTime.Today + job.ScheduledAt.LocalDateTime.TimeOfDay);
        _messageInput.Text = job.Message;
        _workingDirectoryInput.Text = job.WorkingDirectory;
        foreach (var (kind, check) in _targetChecks)
        {
            check.Checked = job.Targets.Contains(kind);
        }
        _enabledCheck.Checked = job.Enabled;
    }

    private void ResetEditor()
    {
        _editingId = null;
        if (_saveButton is not null)
        {
            _saveButton.Text = "建立排程";
        }
        _grid.ClearSelection();
        var proposed = DateTime.Now.AddMinutes(5);
        _nameInput.Text = "AI 倒數喚醒";
        _timeInput.Value = DateTime.Today.AddHours(proposed.Hour).AddMinutes(proposed.Minute);
        _messageInput.Text = "早安";
        _workingDirectoryInput.Text = _host.Paths.WakeupWorkspace;
        foreach (var check in _targetChecks.Values)
        {
            check.Checked = true;
        }
        _enabledCheck.Checked = true;
    }

    // ── 計時器與系統匣 ────────────────────────────────────────────

    private void StartClock()
    {
        if (!_uiTimer.Enabled)
        {
            _uiTimer.Start();
        }
        UiTimerOnTick(null, EventArgs.Empty);
    }

    /// <summary>
    /// 每秒更新時鐘與倒數。視窗看不見時計時器會完全停掉，
    /// 常駐系統匣期間不會有任何每秒喚醒。
    /// </summary>
    private void UiTimerOnTick(object? sender, EventArgs e)
    {
        if (IsDisposed)
        {
            return;
        }

        if (!Visible || WindowState == FormWindowState.Minimized)
        {
            _uiTimer.Stop();
            return;
        }

        var clockText = $"現在：{DateTime.Now:yyyy/MM/dd HH:mm:ss}";
        if (!string.Equals(clockText, _lastClockText, StringComparison.Ordinal))
        {
            _lastClockText = clockText;
            _clockLabel.Text = clockText;
        }

        var now = DateTimeOffset.Now;
        var rows = _grid.Rows;
        for (var i = 0; i < rows.Count; i++)
        {
            var row = rows[i];
            if (row.Tag is ScheduledJob job)
            {
                SetCell(row, ColumnCountdown, JobPresenter.Countdown(job, now));
            }
        }
        UpdateUsageLabels(now);
    }

    private void MainFormOnFormClosing(object? sender, FormClosingEventArgs e)
    {
        if (!_reallyExit && _host.Settings.MinimizeToTray && e.CloseReason == CloseReason.UserClosing)
        {
            e.Cancel = true;
            HideToTray();
            _notifyIcon.ShowBalloonTip(2500, "AI 倒數喚醒", "程式仍在系統匣執行，排程會繼續倒數。", ToolTipIcon.Info);
            return;
        }

        _uiTimer.Stop();
        Manager.JobsChanged -= ManagerOnJobsChanged;
        Manager.BackgroundError -= ManagerOnBackgroundError;
        _notifyIcon.Visible = false;
    }

    private void HideToTray()
    {
        Hide();
        _uiTimer.Stop();
        _notifyIcon.Visible = true;
        // 進入常駐狀態，把視窗建立期間配置的記憶體還給系統。
        NativeMethods.TrimWorkingSet();
    }

    private void ShowFromTray()
    {
        Show();
        WindowState = FormWindowState.Normal;
        Activate();

        if (_refreshDeferred)
        {
            RequestRefresh();
        }
        StartClock();
    }

    private NotifyIcon BuildNotifyIcon(Icon appIcon)
    {
        var menu = new ContextMenuStrip();
        menu.Items.Add("開啟主視窗", null, (_, _) => ShowFromTray());
        menu.Items.Add("開啟日誌資料夾", null, (_, _) => OpenFolder(_host.Paths.LogsDirectory));
        menu.Items.Add(new ToolStripSeparator());
        menu.Items.Add("結束程式", null, (_, _) =>
        {
            _reallyExit = true;
            Close();
        });

        var icon = new NotifyIcon
        {
            Text = "AI 倒數喚醒",
            Icon = appIcon,
            ContextMenuStrip = menu,
            Visible = true
        };
        icon.DoubleClick += (_, _) => ShowFromTray();
        return icon;
    }

    private static Icon GetApplicationIcon()
    {
        try
        {
            var appPath = Environment.ProcessPath;
            if (!string.IsNullOrWhiteSpace(appPath) && File.Exists(appPath))
            {
                var extracted = Icon.ExtractAssociatedIcon(appPath);
                if (extracted is not null)
                {
                    return extracted;
                }
            }

            var localIconPath = Path.Combine(AppContext.BaseDirectory, "app.ico");
            if (File.Exists(localIconPath))
            {
                return new Icon(localIconPath);
            }
        }
        catch
        {
            // 若提取圖示失敗，回退至系統預設
        }

        return SystemIcons.Application;
    }

    // ── 輔助 ──────────────────────────────────────────────────────

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

        var allSucceeded = job.LastResults.TrueForAll(result => result.Succeeded);
        MessageBox.Show(
            JobPresenter.Results(job),
            $"{job.Name}－執行結果",
            MessageBoxButtons.OK,
            allSucceeded ? MessageBoxIcon.Information : MessageBoxIcon.Warning);
    }

    private ScheduledJob? SelectedJob() => _grid.SelectedRows.Count == 1
        ? _grid.SelectedRows[0].Tag as ScheduledJob
        : null;

    private List<CliKind> SelectedTargets()
    {
        var result = new List<CliKind>(_targetChecks.Count);
        foreach (var descriptor in CliCatalog.All)
        {
            if (_targetChecks.TryGetValue(descriptor.Kind, out var check) && check.Checked)
            {
                result.Add(descriptor.Kind);
            }
        }
        return result;
    }

    private DateTimeOffset ReadScheduledAt() => ScheduleCalculator.GetNextDailyOccurrence(
        new TimeSpan(_timeInput.Value.Hour, _timeInput.Value.Minute, 0),
        DateTimeOffset.Now);

    private void BrowseWorkingDirectory(object? sender, EventArgs e)
    {
        using var dialog = new FolderBrowserDialog
        {
            Description = "選擇 CLI 執行時使用的工作目錄",
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
        try
        {
            Directory.CreateDirectory(path);
            using var process = Process.Start(new ProcessStartInfo("explorer.exe", path) { UseShellExecute = true });
        }
        catch (Exception ex)
        {
            MessageBox.Show($"無法開啟資料夾：{ex.Message}", "AI 倒數喚醒", MessageBoxButtons.OK, MessageBoxIcon.Error);
        }
    }

    private static void AddEditorRow(TableLayoutPanel panel, int row, string label, Control control)
    {
        panel.RowStyles.Add(new RowStyle(SizeType.AutoSize));
        panel.Controls.Add(new Label
        {
            Text = label,
            AutoSize = true,
            Anchor = AnchorStyles.Left,
            ForeColor = AppTheme.PrimaryText,
            Font = AppTheme.TableHeader,
            Margin = new Padding(0, 9, 14, 12)
        }, 0, row);
        if (!string.IsNullOrWhiteSpace(label))
        {
            control.AccessibleName = label;
        }
        AppTheme.StyleInput(control);
        control.Margin = new Padding(0, 4, 0, 12);
        panel.Controls.Add(control, 1, row);
    }

    private static Button ActionButton(
        string text,
        EventHandler handler,
        AppTheme.ButtonVariant variant = AppTheme.ButtonVariant.Secondary)
    {
        var button = new Button { Text = text };
        AppTheme.StyleButton(button, variant);
        button.Click += handler;
        return button;
    }

    private void UpdateStatusSummary(IReadOnlyList<ScheduledJob> jobs)
    {
        var pending = 0;
        var failed = 0;
        for (var i = 0; i < jobs.Count; i++)
        {
            var job = jobs[i];
            if (job.Enabled && job.Status == ScheduleStatus.Pending) pending++;
            if (job.Status == ScheduleStatus.Failed) failed++;
        }
        SetStatus($"等待中：{pending}　失敗：{failed}", failed > 0 ? AppTheme.Danger : SystemColors.ControlText);
    }

    private void ManagerOnBackgroundError(object? sender, Exception e)
    {
        if (IsDisposed || !IsHandleCreated)
        {
            return;
        }

        try
        {
            BeginInvoke(() =>
            {
                if (!IsDisposed)
                {
                    SetStatus($"背景錯誤：{e.Message}", AppTheme.Danger);
                }
            });
        }
        catch (InvalidOperationException)
        {
        }
    }

    private void SetStatus(string text, Color color)
    {
        _statusLabel.Text = text;
        _statusLabel.ForeColor = color;
    }

    private static DateTime ClampPickerValue(DateTimePicker picker, DateTime value) =>
        value < picker.MinDate ? picker.MinDate : value > picker.MaxDate ? picker.MaxDate : value;

    private void ShowError(string message)
    {
        SetStatus(message, AppTheme.Danger);
        MessageBox.Show(message, Text, MessageBoxButtons.OK, MessageBoxIcon.Error);
    }
}
