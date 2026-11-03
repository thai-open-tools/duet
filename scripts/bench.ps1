# Measures the spec's engine budgets (section 8) with release builds on this machine.
param(
    [string]$Data = (Join-Path $env:TEMP 'duet-bench'),
    [int]$TimeoutSec = 120,
    # Dataset sizes. The defaults are the spec's; scripts/test-bench.ps1 passes tiny ones.
    [int]$TreeFiles = 100000,
    [int]$TextMB = 10,
    [int]$BigTextMB = 1024
)
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')

# Native tools write progress to stderr; under 'Stop' a redirected stderr would abort the script.
function Invoke-Native([string]$Exe, [string[]]$NativeArgs) {
    $ErrorActionPreference = 'Continue'
    & $Exe @NativeArgs
    if ($LASTEXITCODE) { throw "$Exe $($NativeArgs -join ' ') failed with exit code $LASTEXITCODE" }
}

Invoke-Native 'cargo' @('build', '--release', '-p', 'duet-cli', '-p', 'duet-benchgen')
$cli = Resolve-Path 'target\release\duet-cli.exe'
$gen = Resolve-Path 'target\release\duet-benchgen.exe'

# A dataset counts only once its .done marker exists; a half-written one is rebuilt.
function Initialize-Dataset([string]$Name, [string]$Kind, [string[]]$GenArgs) {
    $dir = Join-Path $Data $Name
    if (Test-Path (Join-Path $dir '.done')) { return }
    if (Test-Path $dir) { Remove-Item -Recurse -Force $dir }
    Invoke-Native $gen (@($Kind, $dir) + $GenArgs)
    Set-Content (Join-Path $dir '.done') ''
}
Initialize-Dataset 'tree'   'tree' @('--files', $TreeFiles, '--change-pct', 1)
Initialize-Dataset 'text10' 'text' @('--size-mb', $TextMB, '--change-pct', 1)
Initialize-Dataset 'text1g' 'text' @('--size-mb', $BigTextMB, '--change-pct', 1)

# Every dataset differs, so a valid run exits 1; anything else (2 = error, timeout) is a failed run.
function Measure-Cli([string[]]$CliArgs) {
    $runs = @()
    foreach ($i in 0..3) {   # run 0 warms the cache and is discarded
        $sw = [Diagnostics.Stopwatch]::StartNew()
        $p = Start-Process -FilePath $cli -ArgumentList $CliArgs -NoNewWindow -PassThru `
             -RedirectStandardOutput "$env:TEMP\duet-bench-out.txt"
        $null = $p.Handle
        $peak = 0
        while (-not $p.HasExited) {
            try { $p.Refresh(); if ($p.PeakPagedMemorySize64 -gt $peak) { $peak = $p.PeakPagedMemorySize64 } } catch {}
            if ($sw.Elapsed.TotalSeconds -gt $TimeoutSec) {   # a budget miss this large needs no median
                $p.Kill(); $p.WaitForExit()
                return [pscustomobject]@{ Ms = $sw.ElapsedMilliseconds; PeakMB = [math]::Round($peak / 1MB); Exit = 'timeout' }
            }
            Start-Sleep -Milliseconds 10
        }
        $p.WaitForExit(); $sw.Stop()
        $run = [pscustomobject]@{ Ms = $sw.ElapsedMilliseconds; PeakMB = [math]::Round($peak / 1MB); Exit = $p.ExitCode }
        if ($run.Exit -ne 1) { return $run }
        if ($i -gt 0) { $runs += $run }
    }
    $sorted = $runs | Sort-Object Ms
    $sorted[1]   # median of 3
}

$text10  = Measure-Cli @("$Data\text10\left.txt", "$Data\text10\right.txt", '--quiet')
$text1g  = Measure-Cli @("$Data\text1g\left.txt", "$Data\text1g\right.txt", '--quiet')
$quick   = Measure-Cli @("$Data\tree\left", "$Data\tree\right", '--quiet')
$content = Measure-Cli @("$Data\tree\left", "$Data\tree\right", '--quiet', '--content')
$readall = $null
try {
    $readallLine = Invoke-Native $gen @('readall', "$Data\tree\left", "$Data\tree\right")
    if ("$readallLine" -match 'ms=(\d+)') { $readall = [int]$Matches[1] }
} catch { Write-Host "readall baseline failed: $_" }

function Measured($r, [string]$Text) {
    if ($r.Exit -eq 'timeout') { "killed after $TimeoutSec s" } elseif ($r.Exit -ne 1) { "run failed (exit $($r.Exit))" } else { $Text }
}
function Verdict($r, [bool]$WithinBudget) { if (($r.Exit -eq 1) -and $WithinBudget) { 'PASS' } else { 'FAIL' } }
$rows = @(
    "| Text $TextMB MB pair, 1% changed | < 300 ms | $(Measured $text10 "$($text10.Ms) ms") | $(Verdict $text10 ($text10.Ms -lt 300)) |",
    "| Text $BigTextMB MB pair, 1% changed | < 10 s, peak private < 1 GB | $(Measured $text1g "$($text1g.Ms) ms"), $($text1g.PeakMB) MB | $(Verdict $text1g (($text1g.Ms -lt 10000) -and ($text1g.PeakMB -lt 1024))) |",
    "| Folder quick, $TreeFiles files per side | < 3 s | $(Measured $quick "$($quick.Ms) ms") | $(Verdict $quick ($quick.Ms -lt 3000)) |",
    "| Folder content vs read-all baseline | >= 80% of baseline throughput | $(Measured $content "$($content.Ms) ms vs $(if ($null -ne $readall) { "$readall ms" } else { 'failed baseline' })") | $(Verdict $content (($null -ne $readall) -and ($content.Ms -le ($readall / 0.8)))) |"
)
$rows | ForEach-Object { Write-Host $_ }
$rows
