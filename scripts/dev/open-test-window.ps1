param(
  [Parameter(Mandatory)][string]$Url,   # a page, file:/// path or data: URL to test against
  [string]$Title = 'test',              # names the throwaway profile, so several can run at once
  [int]$X = 100, [int]$Y = 100, [int]$W = 900, [int]$H = 700,
  [switch]$Close                        # close this test window (and only it)
)
# Opens an app-mode Chrome window with its own throwaway profile, for real-app tests
# (scrolling capture, the blink probe). Prints the window's physical client rect as
# "x,y,w,h", ready for SCROLL_TEST="x,y,w,h,out.png".
# The profile owns its own chrome.exe processes; -Close kills only those, never the owner's Chrome.
$ErrorActionPreference = 'Stop'
$profileDir = Join-Path $env:TEMP "chrome-test-profile-$Title"
$mine = { Get-CimInstance Win32_Process -Filter "Name='chrome.exe'" | Where-Object { $_.CommandLine -like "*$profileDir*" } }

if ($Close) {
  & $mine | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
  return
}

$chrome = @(
  "$env:ProgramFiles\Google\Chrome\Application\chrome.exe",
  "${env:ProgramFiles(x86)}\Google\Chrome\Application\chrome.exe",
  "$env:LOCALAPPDATA\Google\Chrome\Application\chrome.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $chrome) { throw 'Chrome not found' }

Add-Type @"
using System; using System.Runtime.InteropServices;
public static class TW {
  [DllImport("user32.dll")] public static extern IntPtr SetProcessDpiAwarenessContext(IntPtr v);
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr a, int x, int y, int cx, int cy, uint f);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
}
"@
# Without this, rects come back scaled at 125/150% display scaling.
[TW]::SetProcessDpiAwarenessContext([IntPtr]-4) | Out-Null

Start-Process $chrome -ArgumentList "--user-data-dir=`"$profileDir`"", '--no-first-run', '--no-default-browser-check', "--window-position=$X,$Y", "--window-size=$W,$H", "--app=$Url"
for ($i = 0; $i -lt 40; $i++) {
  Start-Sleep -Milliseconds 250
  $win = & $mine | ForEach-Object { Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue } | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
  if ($win) { break }
}
if (-not $win) { throw 'test window did not appear' }
Start-Sleep -Milliseconds 800   # let the page lay out

# Keep it on top: Chrome pauses rendering (and animations) in a covered or minimized window.
[TW]::SetWindowPos($win.MainWindowHandle, [IntPtr]-1, 0, 0, 0, 0, 0x0003) | Out-Null
$r = New-Object TW+RECT; [TW]::GetClientRect($win.MainWindowHandle, [ref]$r) | Out-Null
$p = New-Object TW+POINT; [TW]::ClientToScreen($win.MainWindowHandle, [ref]$p) | Out-Null
"{0},{1},{2},{3}" -f $p.X, $p.Y, ($r.R - $r.L), ($r.B - $r.T)
