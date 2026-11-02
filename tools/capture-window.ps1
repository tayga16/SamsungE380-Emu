param([string] $OutputPath, [string] $ExecutablePath)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class E830WindowCapture {
    [StructLayout(LayoutKind.Sequential)]
    public struct Rect { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr window, out Rect rect);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr window, IntPtr dc, uint flags);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);
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
$projectRoot = Split-Path -Parent $PSScriptRoot
$packageRoot = Join-Path $projectRoot 'dist'
$executable = if ($ExecutablePath) { (Resolve-Path -LiteralPath $ExecutablePath).Path }
    else { Join-Path $packageRoot 'e830-emulator.exe' }
$application = Start-Process -FilePath $executable -ArgumentList '--run' -WorkingDirectory (Split-Path -Parent $executable) -WindowStyle Hidden -PassThru
$window = [IntPtr]::Zero
try {
    $deadline = [DateTime]::UtcNow.AddSeconds(15)
    do {
        Start-Sleep -Milliseconds 200
        $application.Refresh()
        if ($application.HasExited) { throw ('GUI exited with code ' + $application.ExitCode) }
        # WGL can create a 1x1 helper before the actual application window.
        $window = [E830WindowCapture]::FindApplicationWindow($application.Id)
    } while ($window -eq [IntPtr]::Zero -and [DateTime]::UtcNow -lt $deadline)
    if ($window -eq [IntPtr]::Zero) { throw 'GUI application window did not appear.' }
    Start-Sleep -Seconds 3
    $windowRect = [E830WindowCapture+Rect]::new()
    if (-not [E830WindowCapture]::GetWindowRect($window, [ref] $windowRect)) { throw 'GetWindowRect failed.' }
    $bitmap = [System.Drawing.Bitmap]::new($windowRect.Right - $windowRect.Left, $windowRect.Bottom - $windowRect.Top)
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $dc = $graphics.GetHdc()
    try {
        if (-not [E830WindowCapture]::PrintWindow($window, $dc, 2)) { throw 'PrintWindow failed.' }
    } finally { $graphics.ReleaseHdc($dc) }
    $capturePath = if ($OutputPath) { [System.IO.Path]::GetFullPath($OutputPath) }
        else { Join-Path $projectRoot 'proof\native-windows\gui-current.png' }
    try { $bitmap.Save($capturePath, [System.Drawing.Imaging.ImageFormat]::Png) }
    finally { $graphics.Dispose(); $bitmap.Dispose() }
    Write-Output ('GUI verified, process ' + $application.Id + ', screenshot: ' + $capturePath)
} finally {
    $application.Refresh()
    if (-not $application.HasExited) {
        if ($window -ne [IntPtr]::Zero) { [void] [E830WindowCapture]::PostMessage($window, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) }
        if (-not $application.WaitForExit(10000)) { Stop-Process -Id $application.Id }
    }
}
