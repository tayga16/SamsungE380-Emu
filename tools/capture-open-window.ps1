param(
    [Parameter(Mandatory = $true)][uint32] $ProcessId,
    [Parameter(Mandatory = $true)][string] $OutputPath
)
# Capture an already running GUI without launching or closing it.
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class E830OpenWindowCapture {
    [StructLayout(LayoutKind.Sequential)]
    public struct Rect { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr window, out Rect rect);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr window, IntPtr dc, uint flags);
    private delegate bool EnumProc(IntPtr window, IntPtr arg);
    [DllImport("user32.dll")] private static extern bool EnumWindows(EnumProc callback, IntPtr arg);
    [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr window, out uint pid);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] private static extern int GetWindowText(IntPtr window, StringBuilder text, int count);
    public static IntPtr FindApplicationWindow(uint pid) {
        IntPtr found = IntPtr.Zero;
        EnumWindows((window, arg) => {
            uint owner; GetWindowThreadProcessId(window, out owner);
            if (owner != pid) return true;
            var title = new StringBuilder(256); GetWindowText(window, title, title.Capacity);
            Rect rect;
            if (title.ToString() == "Samsung E830 Emulator" && GetWindowRect(window, out rect)
                && rect.Right - rect.Left >= 640 && rect.Bottom - rect.Top >= 420) found = window;
            return true;
        }, IntPtr.Zero);
        return found;
    }
}
'@
$application = Get-Process -Id $ProcessId
$window = [E830OpenWindowCapture]::FindApplicationWindow($ProcessId)
if ($window -eq [IntPtr]::Zero) { throw 'GUI application window was not found.' }
$windowRect = [E830OpenWindowCapture+Rect]::new()
if (-not [E830OpenWindowCapture]::GetWindowRect($window, [ref] $windowRect)) { throw 'GetWindowRect failed.' }
$bitmap = [System.Drawing.Bitmap]::new($windowRect.Right - $windowRect.Left, $windowRect.Bottom - $windowRect.Top)
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
try {
    $dc = $graphics.GetHdc()
    try {
        if (-not [E830OpenWindowCapture]::PrintWindow($window, $dc, 2)) { throw 'PrintWindow failed.' }
    } finally { $graphics.ReleaseHdc($dc) }
    $bitmap.Save([System.IO.Path]::GetFullPath($OutputPath), [System.Drawing.Imaging.ImageFormat]::Png)
} finally { $graphics.Dispose(); $bitmap.Dispose() }
Write-Output ('GUI process ' + $ProcessId + ' left open; screenshot: ' + $OutputPath)
