# Real-app test scripts

These drive the running app from PowerShell. Use them for what unit tests and the browser mock can't see: timing, flicker, real windows.

| Script | Use |
|---|---|
| `open-test-window.ps1 -Url <url> [-Title t]` | Opens a throwaway Chrome window, kept on top, and prints its physical rect `x,y,w,h`. Run `-Title t -Close` to close it. |
| `blink-probe.ps1 [-Trials n] [-Full] [-Key ^c] [-Sx x -Sy y]` | Runs capture, select and Ctrl+C in a loop, sampling a screen pixel around the close. Each trial prints `ms:value` changes. A long flat run after the fade means the frozen frame stayed up (the 0.2.0 blink). |

## Blink check

The probe needs content that changes behind the overlay; a still screen hides the blink. Point it at a colour-cycling page:

```powershell
$page = 'data:text/html,' + [uri]::EscapeDataString('<body style="margin:0"><script>let t=0;(function f(){document.body.style.background=`rgb(${t*7%256},${t*13%256},${t*3%256})`;t++;requestAnimationFrame(f)})()</script>')
scripts\dev\open-test-window.ps1 -Url $page -Title blink -X 100 -Y 100 -W 500 -H 400
scripts\dev\blink-probe.ps1 -Trials 5 -Full -Sx 300 -Sy 300
scripts\dev\open-test-window.ps1 -Url x -Title blink -Close
```

Results with 0.2.1: values change about every 16 ms from roughly 150 ms after the keypress. In 0.2.0 they stayed flat for 160–290 ms.

## Scrolling capture

```powershell
$rect = scripts\dev\open-test-window.ps1 -Url 'https://zaidhossain.com' -Title scroll
$env:SCROLL_TEST = "$rect,$env:TEMP\out.png"; cd src-tauri; cargo test real_scroll_capture -- --nocapture
```

Check the stitch by OCR, not by eye; CLAUDE.md has the steps.
