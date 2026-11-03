# Real-window screenshots of the release duet.exe (the README's pictures and visual checks).
# Each scene runs duet with its own
# data folder (LOCALAPPDATA), fixture pair, theme (DUET_THEME) and window size
# (DUET_WINDOW_SIZE), waits for the "workspace ready" log line, captures the client area with
# PrintWindow (no other window is involved), saves a PNG and closes duet. The mouse pointer is
# not moved; if it rests over duet's window, a hover state can show.
#   powershell -File scripts\shot.ps1 [-Out <dir>] [-Scenes 03-text-diff-dark,10-folder-dark] [-NoBuild]
# -SceneFile takes a JSON list of scenes instead of the built-in ones, each
# {"name": "...", "theme": "dark"|"light", "args": ["left", "right", ...]} (docs\images\make-demo.py
# writes one for the README pictures); -Settle waits longer after "workspace ready" so a compare
# that is still loading can finish.
param(
    [string]$Out = (Join-Path $env:TEMP 'duet-shots'),
    [string]$Scenes = 'all',
    [string]$Size = '1400x850',
    [string]$SceneFile = '',
    [int]$Settle = 400,
    [switch]$NoBuild
)
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')
if (-not $NoBuild) {
    cargo build --release -p duet-app
    if ($LASTEXITCODE) { throw 'cargo build failed' }
}
$duet = (Resolve-Path 'target\release\duet.exe').Path
New-Item -ItemType Directory -Force $Out | Out-Null
$Out = (Resolve-Path $Out).Path
$work = Join-Path $env:TEMP "duet-shots-work-$PID"
New-Item -ItemType Directory -Force $work | Out-Null

Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class ShotNative {
    [StructLayout(LayoutKind.Sequential)]
    public struct Rect { public int Left; public int Top; public int Right; public int Bottom; }
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr hwnd, out Rect rect);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hwnd, IntPtr hdc, uint flags);
}
'@
# Physical pixels: without this, Windows reports a DPI-aware window's client area scaled down.
[void][ShotNative]::SetProcessDPIAware()

function Write-Fixture([string]$Path, [string]$Text) {
    New-Item -ItemType Directory -Force (Split-Path $Path) | Out-Null
    [IO.File]::WriteAllText($Path, $Text)   # UTF-8 without a BOM
    (Get-Item $Path).LastWriteTime = [datetime]'2026-10-14T21:45:01'
}

# The pair LocalDiff's text shots show: six differences, a left-only and a right-only line,
# 44 lines a side.
$fx = Join-Path $work 'fixtures'
$filler = (0..31 | ForEach-Object { "filler line $_" }) -join "`n"
$leftJs = @(
    'function add(a, b) {', '  return a + b;', '}', '', 'const name = "Alice";',
    'console.log("hello", name);', '// unchanged line', 'let total = 10;', 'let old = 1;',
    'for (let i = 0; i < 5; i++) {', '  total += i;', '}'
) -join "`n"
$rightJs = @(
    'function add(a, b, c) {', '  return a + b + c;', '}', '', 'const name = "Alicia";',
    'console.log("hello", name);', '// unchanged line', 'let total = 20;',
    'for (let i = 0; i < 5; i++) {', '  total += i * 2;', '}', 'new inserted line'
) -join "`n"
Write-Fixture "$fx\text\left\sample.js" "$leftJs`n$filler`n"
Write-Fixture "$fx\text\right\sample.js" "$rightJs`n$filler`n"

# The folder pair LocalDiff's folder shots show, plus two same files (All 7, Same 2).
$tree = [ordered]@{
    'L\docs\a.md' = "# A`n"; 'L\src\util\only-left.js' = "1`n"; 'L\src\main.js' = "m()`n"
    'L\README.md' = "same`n"; 'L\package.json' = "{}`n"
    'R\docs\a.md' = "# AB`n"; 'R\docs\new.md' = "n`n"; 'R\src\util\only-right.js' = "2`n"
    'R\src\main.js' = "main(); run();`n"; 'R\README.md' = "same`n"; 'R\package.json' = "{}`n"
}
foreach ($rel in $tree.Keys) { Write-Fixture (Join-Path "$fx\folder" $rel) $tree[$rel] }

$textArgs = @("$fx\text\left\sample.js", "$fx\text\right\sample.js")
$folderArgs = @("$fx\folder\L", "$fx\folder\R")
$all = [ordered]@{
    '01-home-dark'       = @{ Theme = 'dark';  Args = @() }
    '07-home-light'      = @{ Theme = 'light'; Args = @() }
    '03-text-diff-dark'  = @{ Theme = 'dark';  Args = $textArgs }
    '06-text-diff-light' = @{ Theme = 'light'; Args = $textArgs }
    '10-folder-dark'     = @{ Theme = 'dark';  Args = $folderArgs }
    '09-folder-light'    = @{ Theme = 'light'; Args = $folderArgs }
}
if ($SceneFile) {
    $all = [ordered]@{}
    foreach ($s in (Get-Content -Raw -Encoding UTF8 $SceneFile | ConvertFrom-Json)) {
        $all[$s.name] = @{ Theme = $s.theme; Args = @($s.args); Recent = $s.recent; Settings = $s.settings }
    }
}
$names = if ($Scenes -eq 'all') { @($all.Keys) } else { @($Scenes -split ',' | ForEach-Object { $_.Trim() }) }

try {
foreach ($name in $names) {
    if (-not $all.Contains($name)) { throw "unknown scene ${name}; known: $($all.Keys -join ', ')" }
    $scene = $all[$name]
    $data = Join-Path $work "data-$name"
    $logs = Join-Path $data 'logs'
    New-Item -ItemType Directory -Force (Join-Path $data 'duet'), $logs | Out-Null
    # Home lists both fixture pairs as recent compares.
    $recent = "T`t$($textArgs[0])`t$($textArgs[1])`nF`t$($folderArgs[0])`t$($folderArgs[1])`n"
    # a scene file may list its own recent entries, each "<kind>`t<left>`t<right>"
    if ($scene.Recent) { $recent = (@($scene.Recent) -join "`n") + "`n" }
    # and its own settings.json, for example {"folder.criterion": "content"}
    if ($scene.Settings) {
        [IO.File]::WriteAllText((Join-Path $data 'duet\settings.json'), ($scene.Settings | ConvertTo-Json -Compress))
    }
    [IO.File]::WriteAllText((Join-Path $data 'duet\recent.txt'), $recent)

    $envs = @{ LOCALAPPDATA = $data; DUET_LOG_DIR = $logs; DUET_THEME = $scene.Theme; DUET_WINDOW_SIZE = $Size }
    $saved = @{}
    foreach ($k in $envs.Keys) {
        $saved[$k] = [Environment]::GetEnvironmentVariable($k)
        [Environment]::SetEnvironmentVariable($k, $envs[$k])
    }
    try {
        $start = @{ FilePath = $duet; PassThru = $true }
        if ($scene.Args.Count -gt 0) { $start.ArgumentList = $scene.Args | ForEach-Object { '"' + $_ + '"' } }
        $p = Start-Process @start
    } finally {
        foreach ($k in $saved.Keys) { [Environment]::SetEnvironmentVariable($k, $saved[$k]) }
    }
    try {
        $log = Join-Path $logs 'duet.log'
        # A freshly built exe can take a while on its first launches (Windows Defender scan).
        $deadline = (Get-Date).AddSeconds(90)
        while ($true) {
            if ((Get-Date) -gt $deadline) { throw "${name}: duet never logged 'workspace ready'" }
            if ($p.HasExited) { throw "${name}: duet exited with code $($p.ExitCode)" }
            $p.Refresh()
            $ready = (Test-Path $log) -and (Select-String -Path $log -Pattern 'workspace ready' -Quiet)
            if ($ready -and $p.MainWindowHandle -ne [IntPtr]::Zero) { break }
            Start-Sleep -Milliseconds 100
        }
        Start-Sleep -Milliseconds $Settle   # the compositor presents the ready frame
        $hwnd = $p.MainWindowHandle
        $rect = New-Object ShotNative+Rect
        [void][ShotNative]::GetClientRect($hwnd, [ref]$rect)
        $w = $rect.Right - $rect.Left
        $h = $rect.Bottom - $rect.Top
        if ($w -lt 200 -or $h -lt 200) { throw "${name}: the client area is ${w}x${h}" }
        $bmp = New-Object System.Drawing.Bitmap($w, $h)
        $g = [System.Drawing.Graphics]::FromImage($bmp)
        $hdc = $g.GetHdc()
        # PW_CLIENTONLY (1) | PW_RENDERFULLCONTENT (2): includes DirectX/DirectComposition content.
        $ok = [ShotNative]::PrintWindow($hwnd, $hdc, 3)
        $g.ReleaseHdc($hdc)
        $g.Dispose()
        if (-not $ok) { throw "${name}: PrintWindow failed" }
        # One colour everywhere means PrintWindow saw no content.
        $first = $bmp.GetPixel(0, 0)
        $varied = $false
        foreach ($x in 0..15) {
            foreach ($y in 0..15) {
                $px = $bmp.GetPixel([int]($w * ($x + 0.5) / 16), [int]($h * ($y + 0.5) / 16))
                if ($px -ne $first) { $varied = $true }
            }
        }
        if (-not $varied) { throw "${name}: the capture is blank" }
        $png = Join-Path $Out "$name.png"
        $bmp.Save($png, [System.Drawing.Imaging.ImageFormat]::Png)
        $bmp.Dispose()
        Write-Host "$name -> $png (${w}x${h})"
    } finally {
        if (-not $p.HasExited) {
            [void]$p.CloseMainWindow()
            if (-not $p.WaitForExit(3000)) { $p.Kill() }
        }
    }
}
} finally {
    Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
}
