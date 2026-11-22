param(
  [Parameter(Mandatory)] [int] $Port,
  [int] $X = -1,
  [int] $Y = -1,
  [int] $Width = 0,
  [int] $Height = 0
)

Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class CaptureWindow {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr hWnd, IntPtr after, int x, int y, int cx, int cy, uint flags);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int command);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
}
"@

[void][CaptureWindow]::SetProcessDPIAware()

$process = Get-CimInstance Win32_Process -Filter "Name = 'mediaflick-desktop.exe'" |
  Where-Object { $_.CommandLine -match "--remote-debugging-port[ =]$Port\b" -and $_.CommandLine -notmatch '--type=' } |
  Select-Object -First 1
if (-not $process) { throw "no capture app on port $Port" }
$handle = (Get-Process -Id $process.ProcessId).MainWindowHandle
if ($handle -eq 0) { throw "capture app on port $Port has no main window" }

if ($Width -gt 0 -and $Height -gt 0) {
  [void][CaptureWindow]::ShowWindow($handle, 9)
  [void][CaptureWindow]::SetWindowPos($handle, [IntPtr]::Zero, $X, $Y, $Width, $Height, 0x0014)
}
$rect = New-Object CaptureWindow+RECT
[void][CaptureWindow]::GetWindowRect($handle, [ref]$rect)
@{ handle = [int64]$handle; left = $rect.Left; top = $rect.Top; width = $rect.Right - $rect.Left; height = $rect.Bottom - $rect.Top } | ConvertTo-Json -Compress
