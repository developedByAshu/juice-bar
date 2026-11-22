# Saves a screenshot of one Juice Bar window, found by its window class, as a PNG.
# Usage: pwsh windows/tools/capture_window.ps1 -Class JuiceBarFlyout -Out flyout.png [-Margin 24]
# Classes: JuiceBarFlyout (the tray flyout), JuiceBarSettings (the settings window).
param(
    [Parameter(Mandatory)][string]$Class,
    [Parameter(Mandatory)][string]$Out,
    [int]$Margin = 24
)

Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class CaptureNative {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindowW(string cls, string title);
    [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr hwnd, int attr, out RECT rect, int size);
}
'@
[CaptureNative]::SetProcessDPIAware() | Out-Null
$window = [CaptureNative]::FindWindowW($Class, [NullString]::Value)
if ($window -eq [IntPtr]::Zero) { throw "No window of class $Class is open" }
$rect = New-Object CaptureNative+RECT
# Extended frame bounds: the window as drawn, without the invisible resize border.
[CaptureNative]::DwmGetWindowAttribute($window, 9, [ref]$rect, 16) | Out-Null
$left = $rect.Left - $Margin
$top = $rect.Top - $Margin
$width = $rect.Right - $rect.Left + 2 * $Margin
$height = $rect.Bottom - $rect.Top + 2 * $Margin
$bitmap = New-Object System.Drawing.Bitmap $width, $height
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
$graphics.CopyFromScreen($left, $top, 0, 0, $bitmap.Size)
$bitmap.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$graphics.Dispose()
$bitmap.Dispose()
"Saved $Out ($width x $height)"
