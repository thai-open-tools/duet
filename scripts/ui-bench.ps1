# Measures the GUI budgets (spec section 8) with a release duet.exe on this machine:
# cold start to first window, folder first rows (100k files per side), 1M-line scroll at 60 fps,
# hex and table (60k x 20) first rows and scroll, picture (4096 x 4096 pair) diff time,
# 3-way merge of 12,000 lines, a wrapped text scroll at the wrap row limit,
# typing latency, the re-diff after an edit and binary size.
param([string]$Data = (Join-Path $env:TEMP 'duet-bench'), [int]$Runs = 5)
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')

function Invoke-Native([string]$Exe, [string[]]$NativeArgs) {
    $ErrorActionPreference = 'Continue'
    & $Exe @NativeArgs
    if ($LASTEXITCODE) { throw "$Exe $($NativeArgs -join ' ') failed with exit code $LASTEXITCODE" }
}

Invoke-Native 'cargo' @('build', '--release', '-p', 'duet-app', '-p', 'duet-benchgen')
$duet = Resolve-Path 'target\release\duet.exe'
$gen = Resolve-Path 'target\release\duet-benchgen.exe'
$logs = Join-Path ([IO.Path]::GetTempPath()) "duet-ui-bench-$PID"
New-Item -ItemType Directory -Force $logs | Out-Null

if (-not (Test-Path "$Data\tree\.done")) { throw "Run scripts\bench.ps1 first: it builds $Data\tree." }
if (-not (Test-Path "$Data\lines1m\.done")) {
    if (Test-Path "$Data\lines1m") { Remove-Item -Recurse -Force "$Data\lines1m" }
    Invoke-Native $gen @('text', "$Data\lines1m", '--size-mb', '70', '--change-pct', '1')
    Set-Content "$Data\lines1m\.done" ''
}
if (-not (Test-Path "$Data\rediff10\.done")) {
    if (Test-Path "$Data\rediff10") { Remove-Item -Recurse -Force "$Data\rediff10" }
    Invoke-Native $gen @('text', "$Data\rediff10", '--size-mb', '10', '--change-pct', '1')
    Set-Content "$Data\rediff10\.done" ''
}
if (-not (Test-Path "$Data\hex4g\.done")) {
    if (Test-Path "$Data\hex4g") { Remove-Item -Recurse -Force "$Data\hex4g" }
    Invoke-Native $gen @('binary', "$Data\hex4g", '--size-mb', '4096', '--change-pct', '0.01')
    Set-Content "$Data\hex4g\.done" ''
}
if (-not (Test-Path "$Data\table60k\.done")) {
    if (Test-Path "$Data\table60k") { Remove-Item -Recurse -Force "$Data\table60k" }
    Invoke-Native $gen @('table', "$Data\table60k", '--rows', '60000', '--cols', '20', '--change-pct', '2')
    Set-Content "$Data\table60k\.done" ''
}
if (-not (Test-Path "$Data\picture4k\.done")) {
    if (Test-Path "$Data\picture4k") { Remove-Item -Recurse -Force "$Data\picture4k" }
    Invoke-Native $gen @('picture', "$Data\picture4k", '--size', '4096', '--change-pct', '1')
    Set-Content "$Data\picture4k\.done" ''
}
if (-not (Test-Path "$Data\merge12k\.done")) {
    if (Test-Path "$Data\merge12k") { Remove-Item -Recurse -Force "$Data\merge12k" }
    Invoke-Native $gen @('merge', "$Data\merge12k", '--lines', '12000', '--change-pct', '5', '--conflict-pct', '1')
    Set-Content "$Data\merge12k\.done" ''
}
if (-not (Test-Path "$Data\wraplong\.done")) {
    if (Test-Path "$Data\wraplong") { Remove-Item -Recurse -Force "$Data\wraplong" }
    # About 30% of the lines run on to 150-400 characters (mean 276 bytes), so they wrap at any
    # window width; the rest are 72 bytes: 133.2 bytes a line, so 22 MiB is about 173,200 rows, under
    # WRAP_MAX_ROWS (200,000): S = floor(0.9 * 200000 * 133.2 / 2^20) (P9a PF5, final review M7).
    # Raise it with the limit.
    Invoke-Native $gen @('text', "$Data\wraplong", '--size-mb', '22', '--change-pct', '1', '--long-pct', '30')
    Set-Content "$Data\wraplong\.done" ''
}

# Runs duet with one probe switch and returns the matching log line from this run; the whole
# log stays in $script:lastLog for Last-Line.
function Measure-Duet([hashtable]$Env, [string[]]$DuetArgs, [string]$Pattern) {
    $log = Join-Path $logs 'duet.log'
    if (Test-Path $log) { Remove-Item $log }
    $saved = @{}
    foreach ($k in $Env.Keys + 'DUET_LOG_DIR') { $saved[$k] = [Environment]::GetEnvironmentVariable($k) }
    try {
        [Environment]::SetEnvironmentVariable('DUET_LOG_DIR', $logs)
        foreach ($k in $Env.Keys) { [Environment]::SetEnvironmentVariable($k, $Env[$k]) }
        # Start-Process rejects an empty -ArgumentList, so pass it only when there are arguments.
        $start = @{ FilePath = $duet; PassThru = $true }
        if ($DuetArgs.Count -gt 0) { $start.ArgumentList = $DuetArgs }
        $p = Start-Process @start
        if (-not $p.WaitForExit(120000)) { $p.Kill(); throw "duet did not exit: $($DuetArgs -join ' ')" }
    } finally {
        foreach ($k in $saved.Keys) { [Environment]::SetEnvironmentVariable($k, $saved[$k]) }
    }
    $script:lastLog = Get-Content $log
    Last-Line $Pattern
}

function Last-Line([string]$Pattern) {
    $line = $script:lastLog | Where-Object { $_ -match $Pattern } | Select-Object -Last 1
    if (-not $line) { throw "no '$Pattern' line in the last duet log" }
    $line
}

$starts = foreach ($i in 1..$Runs) {
    $line = Measure-Duet @{ DUET_EXIT_AFTER_FIRST_FRAME = '1' } @() 'first frame'
    [int]($line -replace '.*ms=(\d+).*', '$1')
}
$startMedian = ($starts | Sort-Object)[[int][math]::Floor($Runs / 2)]

$rows = foreach ($i in 1..$Runs) {
    $line = Measure-Duet @{ DUET_EXIT_AFTER_FIRST_ROWS = '1' } @("$Data\tree\left", "$Data\tree\right") 'first rows'
    [int]($line -replace '.*ms=(\d+).*', '$1')
}
$rowsMedian = ($rows | Sort-Object)[[int][math]::Floor($Runs / 2)]

$scroll = Measure-Duet @{ DUET_AUTOSCROLL_SECS = '20' } @("$Data\lines1m\left.txt", "$Data\lines1m\right.txt") 'autoscroll'
$frames = [int]($scroll -replace '.*frames=(\d+).*', '$1')
$gaps = [int]($scroll -replace '.*gaps_over_two_vsyncs=(\d+).*', '$1')
$mean = [double]($scroll -replace '.*mean_ms=([0-9.]+).*', '$1')

# Scroll a wrapped text view: a 22 MiB pair (about 173,200 rows, under the wrap limit, 30% of
# them long enough to wrap) with wrap forced on by DUET_TEXT_WRAP=1, 20 s of autoscroll, read
# like the 1M-line row.
$wrapScroll = Measure-Duet @{ DUET_AUTOSCROLL_SECS = '20'; DUET_TEXT_WRAP = '1' } @("$Data\wraplong\left.txt", "$Data\wraplong\right.txt") 'autoscroll'
$wrapFrames = [int]($wrapScroll -replace '.*frames=(\d+).*', '$1')
$wrapGaps = [int]($wrapScroll -replace '.*gaps_over_two_vsyncs=(\d+).*', '$1')
$wrapMean = [double]($wrapScroll -replace '.*mean_ms=([0-9.]+).*', '$1')

# Hex compare on a 4 GiB binary pair: the first rows (median of $Runs) and a 20 s autoscroll.
$hexRows = foreach ($i in 1..$Runs) {
    $line = Measure-Duet @{ DUET_EXIT_AFTER_FIRST_ROWS = '1' } @("$Data\hex4g\left.bin", "$Data\hex4g\right.bin") 'first rows'
    [int]($line -replace '.*ms=(\d+).*', '$1')
}
$hexRowsMedian = ($hexRows | Sort-Object)[[int][math]::Floor($Runs / 2)]
$hexScroll = Measure-Duet @{ DUET_AUTOSCROLL_SECS = '20' } @("$Data\hex4g\left.bin", "$Data\hex4g\right.bin") 'autoscroll'
$hexFrames = [int]($hexScroll -replace '.*frames=(\d+).*', '$1')
$hexGaps = [int]($hexScroll -replace '.*gaps_over_two_vsyncs=(\d+).*', '$1')
$hexMean = [double]($hexScroll -replace '.*mean_ms=([0-9.]+).*', '$1')

# Table compare on a 60,000-row, 20-column pair: load and pair (median of $Runs) and a 20 s
# autoscroll. `--table` forces the table kind; the probes are the same as the hex rows'.
$tableRows = foreach ($i in 1..$Runs) {
    $line = Measure-Duet @{ DUET_EXIT_AFTER_FIRST_ROWS = '1' } @('--table', "$Data\table60k\left.csv", "$Data\table60k\right.csv") 'first rows'
    [int]($line -replace '.*ms=(\d+).*', '$1')
}
$tableRowsMedian = ($tableRows | Sort-Object)[[int][math]::Floor($Runs / 2)]
$tableScroll = Measure-Duet @{ DUET_AUTOSCROLL_SECS = '20' } @('--table', "$Data\table60k\left.csv", "$Data\table60k\right.csv") 'autoscroll'
$tableFrames = [int]($tableScroll -replace '.*frames=(\d+).*', '$1')
$tableGaps = [int]($tableScroll -replace '.*gaps_over_two_vsyncs=(\d+).*', '$1')
$tableMean = [double]($tableScroll -replace '.*mean_ms=([0-9.]+).*', '$1')

# Picture compare on a 4096 x 4096 pair (1% of the 16 px cells changed): the time from both sides
# decoded to the diff applied and drawn, as the view logs it on its `picture diff` line (median of
# $Runs). Two picture extensions route to Picture on their own, so no flag is needed.
$pictureDiffs = foreach ($i in 1..$Runs) {
    $line = Measure-Duet @{ DUET_EXIT_AFTER_FIRST_ROWS = '1' } @("$Data\picture4k\left.png", "$Data\picture4k\right.png") 'picture diff'
    [int]($line -replace '.*ms=(\d+).*', '$1')
}
$pictureMedian = ($pictureDiffs | Sort-Object)[[int][math]::Floor($Runs / 2)]

# 3-way merge of 12,000 lines (5% changed per side, 1% changed on both): the time from the three
# texts read to the cards drawn, as the view logs it on its `merge ready` line (median of $Runs).
# The command line takes LEFT RIGHT BASE in this order.
$mergeTimes = foreach ($i in 1..$Runs) {
    $line = Measure-Duet @{ DUET_EXIT_AFTER_FIRST_ROWS = '1' } @("$Data\merge12k\left.txt", "$Data\merge12k\right.txt", "$Data\merge12k\base.txt") 'merge ready'
    [int]($line -replace '.*ms=(\d+).*', '$1')
}
$mergeMedian = ($mergeTimes | Sort-Object)[[int][math]::Floor($Runs / 2)]

# Typing: a 60 MiB plain-text pair (over the 10 MiB syntax limit, so no highlighting) and an
# ~8 MiB Rust pair full of macro calls (syntax on), 300 keys each into the middle of the right side.
function New-TypingFixture([string]$Dir, [string]$Name, [string]$Unit, [long]$Bytes) {
    if (Test-Path "$Dir\.done") { return }
    if (Test-Path $Dir) { Remove-Item -Recurse -Force $Dir }
    New-Item -ItemType Directory -Force $Dir | Out-Null
    $sb = [System.Text.StringBuilder]::new()
    $i = 0
    while ($sb.Length -lt $Bytes) { [void]$sb.Append(($Unit -replace '@N@', $i)); $i++ }
    # WriteAllText without an encoding writes UTF-8 without a BOM; the text is ASCII.
    [System.IO.File]::WriteAllText("$Dir\left.$Name", $sb.ToString())
    [System.IO.File]::WriteAllText("$Dir\right.$Name", $sb.ToString())
    Set-Content "$Dir\.done" ''
}
New-TypingFixture "$Data\type60m" 'txt' "line @N@ of the typing fixture, some words to shape`n" 60MB
$rustUnit = "fn f@N@(a: i32) -> Vec<i32> {`n    println!(`"f@N@ {}`", a);`n    let v = vec![a, a + 1, a + 2];`n    assert_eq!(v.len(), 3, `"len {}`", @N@);`n    v`n}`n"
New-TypingFixture "$Data\typerust" 'rs' $rustUnit 8MB
# A minified file: 2 MiB of JavaScript on one line, typed into at its end (syntax on).
New-TypingFixture "$Data\typelong" 'js' "var a@N@=function(b){return b+@N@};" 2MB
# Thai prose on one line, 2 MiB of UTF-8 (22 UTF-16 units make 64 bytes), where counting columns
# goes through grapheme segmentation. Built from code points so this script stays ASCII.
$thaiUnit = -join [char[]](0x0E20, 0x0E32, 0x0E29, 0x0E32, 0x0E44, 0x0E17, 0x0E22, 0x0E17, 0x0E35, 0x0E48,
    0x0E21, 0x0E35, 0x0E27, 0x0E23, 0x0E23, 0x0E13, 0x0E22, 0x0E38, 0x0E01, 0x0E15, 0x0E4C, 0x20)
New-TypingFixture "$Data\typethai" 'txt' $thaiUnit ([int](2MB * 22 / 64))

function Field([string]$Line, [string]$Name) { [double]($Line -replace ".*[ ]$Name=([0-9.]+).*", '$1') }
# One typing row set per fixture. The whole-frame budget uses samples 1..n whenever the slowest
# sample is the first one; the first keystroke is reported on its own, split into the edit call
# and the frame after it, and the edit calls are judged on their own against 16 ms.
function Typing-Rows([string]$Line) {
    $at = [int](Field $Line 'max_at')
    if ($at -eq 0) { $m = Field $Line 'steady_mean_ms'; $x = Field $Line 'steady_max_ms'; $scope = 'samples 1..n' }
    else { $m = Field $Line 'mean_ms'; $x = Field $Line 'max_ms'; $scope = 'all samples' }
    [pscustomobject]@{
        Mean = $m; Max = $x; Scope = $scope; At = $at
        First = Field $Line 'first_ms'; FirstEdit = Field $Line 'first_edit_ms'; FirstFrame = Field $Line 'first_frame_ms'
        EditMean = Field $Line 'steady_edit_mean_ms'; EditMax = Field $Line 'steady_edit_max_ms'
    }
}
$typing = Measure-Duet @{ DUET_TYPE_PROBE = '300' } @("$Data\type60m\left.txt", "$Data\type60m\right.txt") 'typing'
$t60 = Typing-Rows $typing
# After its last key the probe waits for the re-diff the pause starts and logs its parts.
$pause60 = Last-Line ' rediff'
$typingRust = Measure-Duet @{ DUET_TYPE_PROBE = '300' } @("$Data\typerust\left.rs", "$Data\typerust\right.rs") 'typing'
$trust = Typing-Rows $typingRust
$typingLong = Measure-Duet @{ DUET_TYPE_PROBE = '300'; DUET_TYPE_PROBE_AT = 'end' } @("$Data\typelong\left.js", "$Data\typelong\right.js") 'typing'
$tlong = Typing-Rows $typingLong
# The same file typed into at the line start, where its row is cut, and a Thai one; at the Thai
# line's end each frame walks the line to the caret (pre-existing, measured only).
$typingLongStart = Measure-Duet @{ DUET_TYPE_PROBE = '300'; DUET_TYPE_PROBE_AT = 'start' } @("$Data\typelong\left.js", "$Data\typelong\right.js") 'typing'
$tlongStart = Typing-Rows $typingLongStart
$typingThai = Measure-Duet @{ DUET_TYPE_PROBE = '300'; DUET_TYPE_PROBE_AT = 'start' } @("$Data\typethai\left.txt", "$Data\typethai\right.txt") 'typing'
$tthai = Typing-Rows $typingThai
$typingThaiEnd = Measure-Duet @{ DUET_TYPE_PROBE = '300'; DUET_TYPE_PROBE_AT = 'end' } @("$Data\typethai\left.txt", "$Data\typethai\right.txt") 'typing'
$tthaiEnd = Typing-Rows $typingThaiEnd
Write-Host "typing 60 MiB: $typing"
Write-Host "typing rust:   $typingRust"
Write-Host "typing long:   $typingLong"
Write-Host "typing long at the start: $typingLongStart"
Write-Host "typing Thai at the start: $typingThai"
Write-Host "typing Thai at the end:   $typingThaiEnd"
Write-Host "pause 60 MiB:  $pause60"

# Re-diff after an edit: 10 keys into a 10 MiB pair with 1% of its lines changed, then the pause.
# Three runs; the median by snapshot + diff.
$rediffs = foreach ($i in 1..3) {
    $line = Measure-Duet @{ DUET_TYPE_PROBE = '10' } @("$Data\rediff10\left.txt", "$Data\rediff10\right.txt") ' rediff'
    Write-Host "rediff 10 MiB: $line"
    [pscustomobject]@{
        Snapshot = Field $line 'snapshot_ms'; Diff = Field $line 'diff_ms'; Land = Field $line 'land_ms'
        Find = Field $line 'find_copy_ms'; Total = (Field $line 'snapshot_ms') + (Field $line 'diff_ms')
    }
}
$rediff = ($rediffs | Sort-Object Total)[1]
$p60 = [pscustomobject]@{
    Snapshot = Field $pause60 'snapshot_ms'; Diff = Field $pause60 'diff_ms'; Land = Field $pause60 'land_ms'
    Find = Field $pause60 'find_copy_ms'
}
$exeBytes = (Get-Item $duet).Length
$sizeMb = $exeBytes / 1MB

function Verdict($ok) { if ($ok) { 'PASS' } else { 'FAIL' } }
function R2($v) { [math]::Round($v, 2) }
function Typing-Out([string]$What, $t, [double]$Frame) {
    $steady = "| Typing into a $What (300 keys), whole frames, $($t.Scope) | steady max < 33 ms, mean within 1 ms of the scroll frame | mean $(R2 $t.Mean) ms, steady max $(R2 $t.Max) ms (sample 0 excluded when it is the slowest; slowest at sample $($t.At)) | $(Verdict (($t.Max -lt 33) -and ($t.Mean -le $Frame + 1))) |"
    $edit = "| Typing into a $What, the edit call alone, keys 2..n | each edit < 16 ms | mean $(R2 $t.EditMean) ms, max $(R2 $t.EditMax) ms | $(Verdict ($t.EditMax -lt 16)) |"
    $first = "| First keystroke into a $What (flagged: first-keystroke cost) | edit < 16 ms; edit + frame < 33 ms | edit $(R2 $t.FirstEdit) ms, then frame $(R2 $t.FirstFrame) ms, total $(R2 $t.First) ms | $(Verdict (($t.FirstEdit -lt 16) -and ($t.First -lt 33))) |"
    "$steady`n$edit`n$first"
}
$rowsOut = @(
    "| Cold start to first window | < 400 ms | $startMedian ms (median of $Runs) | $(Verdict ($startMedian -lt 400)) |",
    "| Folder first rows, 100k files per side | < 200 ms | $rowsMedian ms (median of $Runs) | $(Verdict ($rowsMedian -lt 200)) |",
    "| Scroll a 1M-line text view | 60 fps (frame <= 16 ms) | $frames frames in 20 s, mean $([math]::Round($mean, 2)) ms, $gaps gaps over two vsyncs | $(Verdict (($mean -le 16.9) -and ($gaps -le [math]::Ceiling($frames / 1000)))) |",
    "| Scroll a wrapped text view at the row limit (22 MiB pair, about 173,200 lines, 30% wrapping) | 60 fps (frame <= 16 ms) | $wrapFrames frames in 20 s, mean $([math]::Round($wrapMean, 2)) ms, $wrapGaps gaps over two vsyncs | $(Verdict (($wrapMean -le 16.9) -and ($wrapGaps -le [math]::Ceiling($wrapFrames / 1000)))) |",
    "| Hex first rows, 4 GiB pair | < 100 ms | $hexRowsMedian ms (median of $Runs) | $(Verdict ($hexRowsMedian -lt 100)) |",
    "| Hex scroll, 4 GiB pair | 60 fps (frame <= 16 ms) | $hexFrames frames in 20 s, mean $([math]::Round($hexMean, 2)) ms, $hexGaps gaps over two vsyncs | $(Verdict (($hexMean -le 16.9) -and ($hexGaps -le [math]::Ceiling($hexFrames / 1000)))) |",
    "| Table load + pair, 60k $([char]0xD7) 20 | < 1000 ms | $tableRowsMedian ms (median of $Runs) | $(Verdict ($tableRowsMedian -lt 1000)) |",
    "| Table scroll, 60k rows | 60 fps (frame <= 16 ms) | $tableFrames frames in 20 s, mean $([math]::Round($tableMean, 2)) ms, $tableGaps gaps over two vsyncs | $(Verdict (($tableMean -le 16.9) -and ($tableGaps -le [math]::Ceiling($tableFrames / 1000)))) |",
    "| Picture diff, 4096 $([char]0xD7) 4096 pair | < 300 ms | $pictureMedian ms (median of $Runs) | $(Verdict ($pictureMedian -lt 300)) |",
    "| 3-way merge, 12,000 lines | < 200 ms | $mergeMedian ms (median of $Runs) | $(Verdict ($mergeMedian -lt 200)) |",
    (Typing-Out '60 MiB file, syntax off' $t60 $mean),
    (Typing-Out '8 MiB Rust file, syntax on' $trust $mean),
    (Typing-Out '2 MiB one-line .js file at the line end, syntax on' $tlong $mean),
    (Typing-Out '2 MiB one-line .js file at the line start, syntax on' $tlongStart $mean),
    (Typing-Out '2 MiB one-line Thai text file at the line start' $tthai $mean),
    (Typing-Out '2 MiB one-line Thai text file at the line end (pre-existing per-frame walk, not fixed)' $tthaiEnd $mean),
    "| Re-diff after an edit, 10 MiB pair (1% of lines changed), snapshot + diff | < 100 ms | snapshot $(R2 $rediff.Snapshot) ms (UI thread) + diff $(R2 $rediff.Diff) ms (background) = $(R2 $rediff.Total) ms (median of 3); then landing $(R2 $rediff.Land) ms (UI thread) | $(Verdict ($rediff.Total -lt 100)) |",
    "| Typing pause in a 60 MiB file, UI-thread parts of the re-diff | no spec budget; each part judged against one frame (16 ms) | snapshot $(R2 $p60.Snapshot) ms, landing $(R2 $p60.Land) ms, the find bar's copy when it is open $(R2 $p60.Find) ms; diff $(R2 $p60.Diff) ms in the background | $(Verdict (($p60.Snapshot -lt 16) -and ($p60.Land -lt 16) -and ($p60.Find -lt 16))) |",
    "| Release binary size | growth <= 25 MB over Plan 3a | $([math]::Round($sizeMb, 1)) MB ($($exeBytes)) | compare with the Plan 3a build |"
)
$rowsOut | ForEach-Object { Write-Host $_ }
Remove-Item -Recurse -Force $logs
$rowsOut
