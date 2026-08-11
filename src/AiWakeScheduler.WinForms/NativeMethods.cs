using System.ComponentModel;
using System.Reflection;
using System.Runtime.InteropServices;

namespace AiWakeScheduler.WinForms;

/// <summary>
/// 視窗與程序層級的最佳化輔助。
/// </summary>
internal static class NativeMethods
{
    [DllImport("kernel32.dll", SetLastError = true)]
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool SetProcessWorkingSetSize(IntPtr process, IntPtr minimumWorkingSetSize, IntPtr maximumWorkingSetSize);

    [DllImport("kernel32.dll")]
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    private static extern IntPtr GetCurrentProcess();

    /// <summary>
    /// 縮到系統匣後回收記憶體：先做一次壓縮式 GC，再請作業系統把
    /// 目前用不到的分頁移出工作集。對長時間常駐系統匣的程式，
    /// 這是使用者在工作管理員裡看得到的差異。
    /// </summary>
    public static void TrimWorkingSet()
    {
        try
        {
            GC.Collect(GC.MaxGeneration, GCCollectionMode.Forced, blocking: true, compacting: true);
            GC.WaitForPendingFinalizers();
            SetProcessWorkingSetSize(GetCurrentProcess(), -1, -1);
        }
        catch (Exception ex) when (ex is EntryPointNotFoundException or DllNotFoundException or Win32Exception)
        {
            // 非 Windows 或受限環境下略過即可
        }
    }

    /// <summary>
    /// 開啟控制項的雙緩衝。DataGridView 沒有公開這個屬性，
    /// 但整列重繪時的閃爍幾乎都來自這裡。
    /// </summary>
    public static void EnableDoubleBuffering(Control control)
    {
        try
        {
            var property = control.GetType().GetProperty(
                "DoubleBuffered",
                BindingFlags.Instance | BindingFlags.NonPublic);
            property?.SetValue(control, true, null);
        }
        catch
        {
            // 拿不到就維持預設行為，不影響功能
        }
    }
}
