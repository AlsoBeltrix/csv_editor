# Run on an interactive Windows desktop with Windows PowerShell's STA clipboard support:
# powershell.exe -NoProfile -Sta -File tests/windows_clipboard.ps1 [-Executable target/release/csv-editor.exe]
# Uses only a disposable CSV, restores the clipboard, and refuses keystrokes if focus moves away.
param([string] $Executable = 'target/debug/csv-editor.exe')
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
if (-not ('ClipboardSmoke.Native' -as [type])) {
    Add-Type @'
using System;
using System.Runtime.InteropServices;
namespace ClipboardSmoke {
    public static class Native {
        [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
        [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window);
        [DllImport("user32.dll")] static extern bool ShowWindow(IntPtr window, int show);
        [DllImport("user32.dll")] static extern bool BringWindowToTop(IntPtr window);
        [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
        [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
        [DllImport("user32.dll")] static extern bool AttachThreadInput(uint first, uint second, bool attach);
        public static void Focus(IntPtr window) {
            uint ignored;
            uint foreground = GetWindowThreadProcessId(GetForegroundWindow(), out ignored);
            uint current = GetCurrentThreadId();
            bool attached = foreground != current && AttachThreadInput(current, foreground, true);
            try { ShowWindow(window, 5); BringWindowToTop(window); SetForegroundWindow(window); }
            finally { if (attached) AttachThreadInput(current, foreground, false); }
        }
    }
}
'@
}

$repoRoot = Split-Path -Parent $PSScriptRoot
$smokeRoot = Join-Path $repoRoot 'target/clipboard-smoke'
[void][IO.Directory]::CreateDirectory($smokeRoot)
if (-not [IO.Path]::IsPathRooted($Executable)) { $Executable = Join-Path $repoRoot $Executable }
$file = Join-Path $smokeRoot 'native-clipboard.csv'
[IO.File]::WriteAllText($file, "old,value`r`n", [Text.UTF8Encoding]::new($false))
$priorClipboard = [Windows.Forms.Clipboard]::GetDataObject()
$clipboardBackup = [Windows.Forms.DataObject]::new()
if ($null -ne $priorClipboard) {
    foreach ($format in $priorClipboard.GetFormats($false)) {
        $data = $priorClipboard.GetData($format, $false)
        if ($null -ne $data) { $clipboardBackup.SetData($format, $false, $data) }
    }
}
$priorWindow = [ClipboardSmoke.Native]::GetForegroundWindow()
$priorNoPrefs = $env:CSV_EDITOR_NO_PREFS
$priorScreenshot = $env:CSV_EDITOR_SCREENSHOT
$probe = $null
$probeWindow = [IntPtr]::Zero

function Send-ProbeKeys([string] $Keys) {
    $probe.Refresh()
    if ([ClipboardSmoke.Native]::GetForegroundWindow() -ne $probe.MainWindowHandle) {
        throw 'The probe window lost focus; no keyboard input was sent.'
    }
    [Windows.Forms.SendKeys]::SendWait($Keys)
    Start-Sleep -Milliseconds 250
}

try {
    $env:CSV_EDITOR_NO_PREFS = '1'
    $env:CSV_EDITOR_SCREENSHOT = $null
    $probe = Start-Process -FilePath (Resolve-Path -LiteralPath $Executable).Path -ArgumentList ('"' + $file + '"') -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $smokeRoot 'native.stdout.log') -RedirectStandardError (Join-Path $smokeRoot 'native.stderr.log')
    for ($attempt = 0; $attempt -lt 80; $attempt++) {
        $probe.Refresh()
        if ($probe.HasExited) { throw 'The probe exited before its window was ready.' }
        if ($probe.MainWindowHandle -ne [IntPtr]::Zero) { break }
        Start-Sleep -Milliseconds 100
    }
    if ($probe.MainWindowHandle -eq [IntPtr]::Zero) { throw 'No probe window was created.' }
    $probeWindow = $probe.MainWindowHandle
    [ClipboardSmoke.Native]::Focus($probe.MainWindowHandle)
    Start-Sleep -Milliseconds 400
    $block = "name`tvalue`r`nalpha`t001`r`nbeta`t002"
    [Windows.Forms.Clipboard]::SetText($block)
    Send-ProbeKeys '^v'
    Send-ProbeKeys '^s'
    $saved = [IO.File]::ReadAllText($file)
    $expected = "name,value`r`nalpha,001`r`nbeta,002`r`n"
    if ($saved -ne $expected) { throw 'Native Ctrl+V then Ctrl+S did not save the expected external 3-by-2 text block.' }
    'PASS: native Ctrl+V reads external multiline tab-separated text; Ctrl+S saves exact values.'

    [Windows.Forms.Clipboard]::SetText('external sentinel')
    Send-ProbeKeys '^c'
    $copied = [Windows.Forms.Clipboard]::GetText()
    if ($copied.Replace("`r`n", "`n") -ne $block.Replace("`r`n", "`n")) { throw 'Native Ctrl+C did not write the selected block to the OS clipboard.' }
    'PASS: native Ctrl+C writes the selected block to the external clipboard.'

    Send-ProbeKeys '^{HOME}'
    Send-ProbeKeys 'renamed'
    Send-ProbeKeys '^s'
    if ([IO.File]::ReadAllText($file) -ne $expected.Replace('name,value', 'renamed,value')) { throw 'Ctrl+S did not commit and save an active cell edit.' }
    Send-ProbeKeys '^z'
    Send-ProbeKeys '^s'
    if ([IO.File]::ReadAllText($file) -ne $expected) { throw 'Undo after saving the active edit did not restore the original value.' }
    'PASS: native Ctrl+S commits the active edit; Undo and save restore the original value.'

    Send-ProbeKeys '^a'
    Send-ProbeKeys '^x'
    Send-ProbeKeys '^{HOME}'
    Send-ProbeKeys '{DOWN 4}'
    Send-ProbeKeys '^v'
    Send-ProbeKeys '^s'
    $moved = [IO.File]::ReadAllText($file)
    if ($moved -ne (",`r`n,`r`n,`r`n,`r`n" + $expected)) { throw 'Native Ctrl+X/Ctrl+V did not move the block to row five.' }
    Send-ProbeKeys '^z'
    Send-ProbeKeys '^s'
    if ([IO.File]::ReadAllText($file) -ne $expected) { throw 'One Undo did not restore the cut block and table extent.' }
    'PASS: native cut/paste moves the block; one Undo restores it and saves correctly.'

    if (-not $probe.CloseMainWindow() -or -not $probe.WaitForExit(5000)) { throw 'The saved probe did not close cleanly.' }
    $probe = Start-Process -FilePath (Resolve-Path -LiteralPath $Executable).Path -ArgumentList ('"' + $file + '"') -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $smokeRoot 'reopen.stdout.log') -RedirectStandardError (Join-Path $smokeRoot 'reopen.stderr.log')
    for ($attempt = 0; $attempt -lt 80; $attempt++) {
        $probe.Refresh()
        if ($probe.HasExited) { throw 'The reopened probe exited before its window was ready.' }
        if ($probe.MainWindowHandle -ne [IntPtr]::Zero) { break }
        Start-Sleep -Milliseconds 100
    }
    if ($probe.MainWindowHandle -eq [IntPtr]::Zero) { throw 'No reopened probe window was created.' }
    $probeWindow = $probe.MainWindowHandle
    [ClipboardSmoke.Native]::Focus($probeWindow)
    Start-Sleep -Milliseconds 400
    Send-ProbeKeys '^a'
    Send-ProbeKeys '^c'
    if ([Windows.Forms.Clipboard]::GetText().Replace("`r`n", "`n") -ne $block.Replace("`r`n", "`n")) { throw 'Reopening the saved CSV changed its cell values.' }
    'PASS: closing and reopening the saved CSV preserves its values, including leading zeros.'
} finally {
    $stillFocused = $probeWindow -ne [IntPtr]::Zero -and [ClipboardSmoke.Native]::GetForegroundWindow() -eq $probeWindow
    if ($null -ne $probe -and -not $probe.HasExited) {
        [void]$probe.CloseMainWindow()
        if (-not $probe.WaitForExit(5000)) { Stop-Process -Id $probe.Id }
    }
    if ($null -eq $priorClipboard) { [Windows.Forms.Clipboard]::Clear() }
    else { [Windows.Forms.Clipboard]::SetDataObject($clipboardBackup, $true) }
    if ($stillFocused -and $priorWindow -ne [IntPtr]::Zero) { [void][ClipboardSmoke.Native]::SetForegroundWindow($priorWindow) }
    $env:CSV_EDITOR_NO_PREFS = $priorNoPrefs
    $env:CSV_EDITOR_SCREENSHOT = $priorScreenshot
}
