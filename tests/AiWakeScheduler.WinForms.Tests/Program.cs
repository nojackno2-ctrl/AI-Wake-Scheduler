using System.Reflection;
using Microsoft.Win32;
using AiWakeScheduler.Core;
using AiWakeScheduler.WinForms;

namespace AiWakeScheduler.WinForms.Tests;

internal static class Program
{
    [STAThread]
    private static int Main(string[] args)
    {
        Application.SetHighDpiMode(HighDpiMode.PerMonitorV2);
        Application.EnableVisualStyles();
        Application.SetCompatibleTextRenderingDefault(false);
        var tests = new List<(string Name, Action Run)>
        {
            ("ModelEffortSwitchAndSave", TestModelSwitch),
            ("SettingsCancelIsolation", TestSettingsIsolation),
            ("SettingsFooterAlwaysVisible", TestSettingsFooter),
            ("IndependentCountdownPresentation", TestCountdown),
            ("MainFormEditorAndTray", TestMainForm),
            ("TerminalOutputAndRetention", TestTerminal),
            ("UsageBoardLoginAndRendering", TestUsageBoard)
        };
        // Explicit opt-in: temporarily changes the real Windows startup entry, then restores it.
        if (args.Contains("--startup-integration")) tests.Add(("UserStartupEnableDisable", TestStartup));
        var failed = 0;
        foreach (var test in tests)
        {
            try { test.Run(); Console.WriteLine($"PASS {test.Name}"); }
            catch (Exception ex) { failed++; Console.WriteLine($"FAIL {test.Name}: {ex}"); }
        }
        AppTheme.Release();
        Console.WriteLine($"{tests.Count - failed}/{tests.Count} WinForms tests passed");
        return failed == 0 ? 0 : 1;
    }

    private static T Field<T>(object source, string name) =>
        (T)(source.GetType().GetField(name, BindingFlags.Instance | BindingFlags.NonPublic)?.GetValue(source)
            ?? throw new InvalidOperationException($"Missing field: {name}"));

    private static void Assert(bool condition, string message)
    {
        if (!condition) throw new InvalidOperationException(message);
    }

    private static void TestModelSwitch()
    {
        var source = AppSettings.CreateDefault();
        source.CliProfiles[CliKind.Codex].Model = "gpt-6.1-sol";
        source.CliProfiles[CliKind.Codex].ThinkingEffort = ThinkingEffort.Ultra;
        using var form = new SettingsForm(source, new CliRunner(new AppDataPaths(Path.GetTempPath())));
        var models = Field<Dictionary<CliKind, ComboBox>>(form, "_modelInputs");
        var efforts = Field<Dictionary<CliKind, ComboBox>>(form, "_effortInputs");
        Assert(efforts[CliKind.Codex].Text.Contains("Ultra"), "Sol should preserve Ultra.");
        models[CliKind.Codex].Text = "gpt-6-luna";
        Assert(efforts[CliKind.Codex].Items.Count == 6, "Luna must offer default through Max, without Ultra.");
        Assert(efforts[CliKind.Codex].Text.Contains("Max"), "Switching Sol Ultra to Luna should select Max.");
        models[CliKind.Antigravity].Text = "gemini-3.8-flash-high";
        Assert(efforts[CliKind.Antigravity].Items.Count == 1, "Full Gemini ID should suppress extra effort.");
        models[CliKind.AntigravityClaude].Text = "claude-opus-5-5-high";
        Assert(efforts[CliKind.AntigravityClaude].Items.Count == 1, "Full AGY Claude ID must not offer extra effort.");
        // Invoke the actual save handler through the WinForms AcceptButton.
        form.Show();
        ((Button)form.AcceptButton!).PerformClick();
        Assert(form.ResultSettings.CliProfiles[CliKind.Codex].Model == "gpt-6-luna", "Save should persist selected model.");
        Assert(form.ResultSettings.CliProfiles[CliKind.Codex].ThinkingEffort == ThinkingEffort.Max, "Save should persist normalized effort.");
        Assert(source.CliProfiles[CliKind.Codex].Model == "gpt-6.1-sol", "Dialog edits must not mutate the source before acceptance.");
        form.Close();
    }

    private static void TestSettingsIsolation()
    {
        var source = AppSettings.CreateDefault();
        var originalModel = source.CliProfiles[CliKind.Claude].Model;
        using var form = new SettingsForm(source, new CliRunner(new AppDataPaths(Path.GetTempPath())));
        Field<Dictionary<CliKind, ComboBox>>(form, "_modelInputs")[CliKind.Claude].Text = "custom-model";
        Assert(source.CliProfiles[CliKind.Claude].Model == originalModel, "Unaccepted dialog changes must remain isolated.");
        Assert(form.ResultSettings.CliProfiles[CliKind.Claude].Model == originalModel, "Typing alone must not save values.");
    }

    private static void TestCountdown()
    {
        var now = new DateTimeOffset(2026, 10, 4, 12, 0, 0, TimeSpan.FromHours(8));
        var job = new ScheduledJob { Recurrence = ScheduleRecurrence.Interval, Targets = [CliKind.Codex, CliKind.Claude] };
        var usage = new Dictionary<CliKind, CliUsageSnapshot>
        {
            [CliKind.Codex] = new(CliKind.Codex, CliUsageAvailability.Available,
                [new("Weekly", 30, TimeSpan.FromDays(7), now.AddDays(2), true),
                 new("Primary", 10, TimeSpan.FromHours(5), now.AddHours(2), true)], "OK", now),
            [CliKind.Claude] = new(CliKind.Claude, CliUsageAvailability.Available,
                [new("Primary", 0, TimeSpan.FromHours(5), now.AddHours(5), false)], "OK", now)
        };
        var text = JobPresenter.Countdown(job, now, usage);
        Assert(text.Contains("Codex 02:00:00") && text.Contains("Claude 未倒數"), "Each target should use its own short-window countdown.");
        Assert(JobPresenter.Countdown(job, now.AddSeconds(1), usage).Contains("01:59:59"), "Countdown should tick using server reset time.");
        job.Enabled = false;
        Assert(JobPresenter.Countdown(job, now, usage) == "已停用", "Disabled jobs must not display an active wake countdown.");
    }

    private static void TestSettingsFooter()
    {
        using var form = new SettingsForm(AppSettings.CreateDefault(), new CliRunner(new AppDataPaths(Path.GetTempPath())));
        form.MinimumSize = Size.Empty;
        form.Size = new Size(760, 540);
        form.Show();
        var content = form.Controls.OfType<TableLayoutPanel>().Single();
        void AssertButtonsVisible()
        {
            form.PerformLayout();
            Application.DoEvents();
            foreach (var button in new[] { (Button)form.AcceptButton!, (Button)form.CancelButton! })
            {
                var bounds = form.RectangleToClient(button.Parent!.RectangleToScreen(button.Bounds));
                Assert(form.ClientRectangle.Contains(bounds) && button.Visible,
                    $"Dialog action must be fully inside the client area: {button.Text}, {bounds}.");
                Assert(!content.Bounds.IntersectsWith(bounds), "Scroll content must not overlap the fixed footer.");
            }
        }
        AssertButtonsVisible();
        Field<Label>(form, "_probeStatus").Text = string.Join(Environment.NewLine, Enumerable.Repeat(new string('x', 180), 20));
        content.AutoScrollPosition = new Point(500, 10000);
        AssertButtonsVisible();
        form.Close();
    }

    private static void TestMainForm()
    {
        var root = Path.Combine(Path.GetTempPath(), $"AiWakeScheduler-FormTests-{Guid.NewGuid():N}");
        var host = AppHost.CreateAsync(rootDirectory: root).GetAwaiter().GetResult();
        try
        {
            using var form = new MainForm(host, false);
            Assert(Field<TextBox>(form, "_messageInput").MaxLength == 50, "UI wake messages must be capped at 50 characters.");
            Assert(Field<Dictionary<CliKind, CheckBox>>(form, "_targetChecks").Count == 4, "All four target checkboxes must exist.");
            Assert(Field<TextBox>(form, "_workingDirectoryInput").Text == host.Paths.WakeupWorkspace, "Editor must use the isolated workspace.");
            Assert(Field<Button>(form, "_saveButton").Text == "建立排程", "New editor should offer schedule creation.");
            Field<TextBox>(form, "_nameInput").Text = "QA daily";
            Field<CheckBox>(form, "_enabledCheck").Checked = false;
            Invoke(form, "SaveScheduleAsync", null, EventArgs.Empty);
            PumpUntil(() => Field<Button>(form, "_saveButton").Text == "儲存修改");
            var first = host.Manager.GetJobsAsync().GetAwaiter().GetResult().Single();
            Assert(first.Name == "QA daily" && !first.Enabled && first.Recurrence == ScheduleRecurrence.Daily,
                "Create event must save a disabled daily job without running it.");
            Field<TextBox>(form, "_nameInput").Text = "QA interval";
            Field<ComboBox>(form, "_recurrenceInput").SelectedIndex = 1;
            Invoke(form, "SaveScheduleAsync", null, EventArgs.Empty);
            PumpUntil(() => host.Manager.GetJobsAsync().GetAwaiter().GetResult().Single().Name == "QA interval");
            var edited = host.Manager.GetJobsAsync().GetAwaiter().GetResult().Single();
            Assert(edited.Id == first.Id && edited.Recurrence == ScheduleRecurrence.Interval && edited.InitialTimeOfDay is not null,
                "Edit event must update the same job and preserve interval anchor.");
            Assert(File.ReadAllText(host.Paths.JobsFile).Contains("QA interval"), "Edited schedule must be persisted.");
            // Keep the tray test offline; the real Shown handler starts account queries.
            form.Shown -= (EventHandler)Delegate.CreateDelegate(typeof(EventHandler), form, "MainFormOnShown");
            form.Show();
            form.Close();
            Assert(!form.IsDisposed && !form.Visible && Field<NotifyIcon>(form, "_notifyIcon").Visible,
                "Closing must keep the scheduler alive in the tray.");
            Assert(!Field<System.Windows.Forms.Timer>(form, "_uiTimer").Enabled, "Hidden UI must stop its per-second timer.");
            Invoke(form, "ShowFromTray");
            Assert(form.Visible && Field<System.Windows.Forms.Timer>(form, "_uiTimer").Enabled,
                "Restoring must show the form and restart its UI countdown timer.");
        }
        finally
        {
            host.DisposeAsync().AsTask().GetAwaiter().GetResult();
            Directory.Delete(root, true);
        }
    }

    private static void TestTerminal()
    {
        using var panel = new TerminalPanel();
        var output = Field<RichTextBox>(panel, "_output");
        _ = output.Handle; // Exercise RichEdit selection/deletion after native control creation.
        var now = DateTimeOffset.Now;
        panel.LogActivity(new(CliKind.Codex, "QA", CliActivityKind.Output, "stdout marker", now));
        panel.LogActivity(new(CliKind.Claude, "QA", CliActivityKind.ErrorOutput, "stderr marker", now));
        Invoke(panel, "Flush");
        Assert(output.Text.Contains("stdout marker") && output.Text.Contains("stderr marker"),
            "Both stdout and stderr must reach the terminal.");
        panel.LogActivity(new(CliKind.Codex, "QA", CliActivityKind.Output, new string('x', 210_000) + "tail marker", now));
        Invoke(panel, "Flush");
        Assert(output.TextLength <= 200_000 && output.Text.EndsWith("tail marker"),
            $"Output retention must keep newest text. Length={output.TextLength}, tail={output.Text[^Math.Min(35, output.TextLength)..].Replace("\n", "<LF>").Replace("\r", "<CR>")}");
        panel.Clear();
        Assert(output.TextLength == 0, "Terminal clear must remove retained output.");
    }

    private static void TestUsageBoard()
    {
        using var board = new UsageBoard { Width = 700 };
        var now = DateTimeOffset.Now;
        board.Apply(new Dictionary<CliKind, CliUsageSnapshot>
        {
            [CliKind.Claude] = new(CliKind.Claude, CliUsageAvailability.Unavailable, [], "登入失效", now, true),
            [CliKind.Codex] = new(CliKind.Codex, CliUsageAvailability.Available,
                [new("5h", 20, TimeSpan.FromHours(5), now.AddHours(2), true)], "OK", now)
        }, now);
        board.Height = board.PreferredHeight;
        using var bitmap = new Bitmap(board.Width, board.Height);
        board.DrawToBitmap(bitmap, board.ClientRectangle);
        var link = Field<List<(Rectangle Bounds, CliKind Kind)>>(board, "_loginLinks").Single();
        CliKind? requested = null;
        board.LoginRequested += (_, kind) => requested = kind;
        var click = new MouseEventArgs(MouseButtons.Left, 1, link.Bounds.Left + 1, link.Bounds.Top + 1, 0);
        Invoke(board, "OnMouseClick", click);
        Assert(requested == CliKind.Claude, "Login link must route to the correct target.");
        requested = null;
        board.SetLoginInProgress(CliKind.Claude, true);
        Invoke(board, "OnMouseClick", click);
        Assert(requested is null, "An active login must suppress duplicate login requests.");
        board.Apply(new Dictionary<CliKind, CliUsageSnapshot>(), now);
        board.Height = board.PreferredHeight;
        board.DrawToBitmap(bitmap, new Rectangle(0, 0, bitmap.Width, Math.Min(bitmap.Height, board.Height)));
        Assert(Field<List<(Rectangle Bounds, CliKind Kind)>>(board, "_loginLinks").Count == 0,
            "Removing stale snapshots must also remove stale login hit targets.");
    }

    private static void Invoke(object target, string method, params object?[] arguments) =>
        target.GetType().GetMethod(method, BindingFlags.Instance | BindingFlags.NonPublic)!.Invoke(target, arguments);

    private static void PumpUntil(Func<bool> completed)
    {
        var deadline = DateTime.UtcNow.AddSeconds(5);
        while (!completed() && DateTime.UtcNow < deadline)
        {
            Application.DoEvents();
            Thread.Sleep(10);
        }
        Assert(completed(), "WinForms asynchronous event did not complete.");
    }

    private static void TestStartup()
    {
        Assert(!(bool)typeof(StartupManager).GetMethod("HasLegacyTask", BindingFlags.Static | BindingFlags.NonPublic)!.Invoke(null, null)!,
            "Startup integration requires no existing legacy task; it must not delete user tasks.");
        var startupFolder = Environment.GetFolderPath(Environment.SpecialFolder.Startup);
        Assert(!File.Exists(Path.Combine(startupFolder, "AI 倒數喚醒.lnk")) &&
            !File.Exists(Path.Combine(startupFolder, "AI倒數喚醒.lnk")),
            "Startup integration must not remove existing user shortcuts.");
        const string runKey = @"Software\Microsoft\Windows\CurrentVersion\Run";
        const string name = "AI倒數喚醒";
        using var key = Registry.CurrentUser.CreateSubKey(runKey, writable: true);
        var original = key.GetValue(name);
        var originalKind = original is null ? RegistryValueKind.String : key.GetValueKind(name);
        try
        {
            StartupManager.SetEnabled(true);
            Assert(StartupManager.IsEnabled(), "Enabling startup must write the actual HKCU Run value.");
            var command = key.GetValue(name) as string;
            Assert(command == $"\"{Environment.ProcessPath}\" --minimized", "Startup command must quote the executable path correctly.");
            StartupManager.SetEnabled(false);
            Assert(!StartupManager.IsEnabled() && key.GetValue(name) is null, "Disabling startup must remove the actual value.");
        }
        finally
        {
            if (original is null) key.DeleteValue(name, throwOnMissingValue: false);
            else key.SetValue(name, original, originalKind);
        }
    }
}
