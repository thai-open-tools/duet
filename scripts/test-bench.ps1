# Guards for bench.ps1: a failed run must never print PASS, and an incomplete dataset is rebuilt.
# Uses tiny datasets so it finishes in seconds.
$ErrorActionPreference = 'Stop'
$bench = Join-Path $PSScriptRoot 'bench.ps1'
$tiny = @{ TreeFiles = 200; TextMB = 1; BigTextMB = 2; TimeoutSec = 30 }
$failures = 0
function Check([bool]$ok, [string]$what) {
    if ($ok) { Write-Host "ok   $what" } else { Write-Host "FAIL $what"; $script:failures++ }
}

# Case A: datasets marked complete but missing their files -> the CLI exits 2 -> no row may PASS.
$a = Join-Path ([IO.Path]::GetTempPath()) "duet-bench-guard-a-$PID"
foreach ($name in 'tree', 'text10', 'text1g') {
    New-Item -ItemType Directory -Force (Join-Path $a $name) | Out-Null
    Set-Content (Join-Path $a "$name\.done") ''
}
$rows = & $bench -Data $a @tiny 2>$null | Where-Object { $_ -like '|*' }
Check ($rows.Count -eq 4) "case A prints four rows (got $($rows.Count))"
Check (-not ($rows -match '\| PASS \|')) "case A: a run whose CLI fails never reports PASS"
Remove-Item -Recurse -Force $a

# Case B: dataset folders left half-written (no .done marker) are regenerated before measuring.
$b = Join-Path ([IO.Path]::GetTempPath()) "duet-bench-guard-b-$PID"
foreach ($name in 'tree', 'text10', 'text1g') { New-Item -ItemType Directory -Force (Join-Path $b $name) | Out-Null }
Set-Content (Join-Path $b 'text10\left.txt') 'partial'
$rows = & $bench -Data $b @tiny 2>$null | Where-Object { $_ -like '|*' }
Check (Test-Path (Join-Path $b 'text10\right.txt')) "case B regenerates an incomplete text dataset"
Check (Test-Path (Join-Path $b 'tree\right')) "case B regenerates an incomplete tree dataset"
Check (@($rows | Where-Object { $_ -match '\| (PASS|FAIL) \|' }).Count -eq 4) "case B measures all four rows"
Remove-Item -Recurse -Force $b

if ($failures) { Write-Host "test-bench: $failures failure(s)"; exit 1 }
Write-Host 'test-bench: OK'
