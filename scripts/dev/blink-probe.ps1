param([int]$Trials = 15, [string]$Key = "^c", [int]$Sx = -1, [int]$Sy = -1, [switch]$Full)
# Drives capture -> select -> Ctrl+C and samples screen pixels around the close, to catch a blink.
Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @"
using System; using System.Runtime.InteropServices; using System.Drawing;
public static class P {
  [DllImport("user32.dll")] public static extern IntPtr SetProcessDpiAwarenessContext(IntPtr v);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int x, int y, uint d, UIntPtr e);
  [DllImport("user32.dll")] public static extern IntPtr GetDC(IntPtr h);
  [DllImport("user32.dll")] public static extern int ReleaseDC(IntPtr h, IntPtr dc);
  [DllImport("gdi32.dll")] public static extern uint GetPixel(IntPtr dc, int x, int y);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
  public static void Drag(int x0, int y0, int x1, int y1) {
    SetCursorPos(x0, y0); mouse_event(2, 0, 0, 0, UIntPtr.Zero);
    for (int i = 1; i <= 10; i++) { SetCursorPos(x0 + (x1 - x0) * i / 10, y0 + (y1 - y0) * i / 10); System.Threading.Thread.Sleep(15); }
    mouse_event(4, 0, 0, 0, UIntPtr.Zero);
  }
  // Sum of RGB at a few points (screen DC = what DWM composed).
  public static int Lum(int x, int y) { IntPtr dc = GetDC(IntPtr.Zero); uint c = GetPixel(dc, x, y); ReleaseDC(IntPtr.Zero, dc); return (int)((c & 255) + ((c >> 8) & 255) + ((c >> 16) & 255)); }
}
"@
[P]::SetProcessDpiAwarenessContext([IntPtr]-4) | Out-Null
Add-Type -AssemblyName System.Windows.Forms
$W = [P]::GetSystemMetrics(0); $H = [P]::GetSystemMetrics(1)
$exe = (Get-Process screenshot-app | Select -First 1).Path
"screen ${W}x${H}, exe $exe"
$out = [int]($W * 0.15), [int]($H * 0.2)
if ($Sx -ge 0) { $out = $Sx, $Sy }
$a = if ($Full) { 0.02 } else { 0.35 }; $b = if ($Full) { 0.98 } else { 0.65 }        # outside the selection: dimmed while open
$in  = [int]($W * 0.5), [int]($H * 0.5)         # inside the selection
for ($t = 1; $t -le $Trials; $t++) {
  Start-Process $exe -ArgumentList "--capture"
  Start-Sleep -Milliseconds 700
  [P]::Drag([int]($W * $a), [int]($H * $a), [int]($W * $b), [int]($H * $b))
  [P]::SetCursorPos([int]($W * 0.9), [int]($H * 0.9)) | Out-Null
  Start-Sleep -Milliseconds 250
  $live = [P]::Lum($out[0], $out[1])
  [System.Windows.Forms.SendKeys]::SendWait($Key)
  $sw = [Diagnostics.Stopwatch]::StartNew(); $s = New-Object System.Collections.Generic.List[string]
  $prev = -1
  while ($sw.ElapsedMilliseconds -lt 1500) {
    $o = [P]::Lum($out[0], $out[1])
    if ($o -ne $prev) { $s.Add(("{0}ms:{1}" -f $sw.ElapsedMilliseconds, $o)); $prev = $o }
  }
  "trial $t (open value $live): " + (($s | Select -First 10) -join "  ") + "  ... $($s.Count) changes"
  Start-Sleep -Milliseconds 400
}
