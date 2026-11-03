#Requires -RunAsAdministrator
# Blocks all outbound traffic for duet binaries at the OS level (defense in depth: duet has no network code).
param([string]$BinDir = (Join-Path $PSScriptRoot '..\target\release'))
$BinDir = [System.IO.Path]::GetFullPath($BinDir)
foreach ($exe in 'duet.exe', 'duet-cli.exe') {
    $program = Join-Path $BinDir $exe
    $name = "duet: block outbound ($exe)"
    Get-NetFirewallRule -DisplayName $name -ErrorAction SilentlyContinue | Remove-NetFirewallRule
    New-NetFirewallRule -DisplayName $name -Direction Outbound -Program $program -Action Block -Profile Any | Out-Null
    Write-Host "blocked: $program"
}
