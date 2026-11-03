# Builds everything a GitHub release carries, into dist\:
#   duet-<version>-setup-x64.exe     the installer (Inno Setup 6.7+, installer\duet.iss)
#   duet-<version>-portable-x64.zip  duet.exe, duet-cli.exe, the licences, ready to unzip and run
#   SHA256SUMS.txt                   checksums of both
#   powershell -File scripts\release.ps1 [-NoBuild] [-Iscc <path to ISCC.exe>]
param(
    [switch]$NoBuild,
    [string]$Iscc = ''
)
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')

if (-not $NoBuild) {
    cargo build --release -p duet-app -p duet-cli
    if ($LASTEXITCODE) { throw 'cargo build failed' }
}
$bin = Resolve-Path 'target\release'
$version = (Get-Item (Join-Path $bin 'duet.exe')).VersionInfo.ProductVersion
if (-not $version) { throw 'duet.exe has no version block (was it built without the Windows SDK rc.exe?)' }

New-Item -ItemType Directory -Force dist | Out-Null
python -I scripts\third-party-notices.py dist\THIRD_PARTY_NOTICES.md
if ($LASTEXITCODE) { throw 'third-party-notices.py failed' }

if (-not $Iscc) {
    $Iscc = @(
        (Join-Path $env:LOCALAPPDATA 'Programs\Inno Setup 6\ISCC.exe'),
        (Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6\ISCC.exe'),
        (Join-Path $env:ProgramFiles 'Inno Setup 6\ISCC.exe')
    ) | Where-Object { Test-Path $_ } | Select-Object -First 1
}
if (-not $Iscc) { throw 'ISCC.exe not found: install Inno Setup 6.7+ or pass -Iscc' }
& $Iscc /Q installer\duet.iss
if ($LASTEXITCODE) { throw 'ISCC failed' }

$setup = "dist\duet-$version-setup-x64.exe"
$zip = "dist\duet-$version-portable-x64.zip"
$stage = Join-Path $env:TEMP "duet-portable-$PID"
$folder = Join-Path $stage "duet-$version"
New-Item -ItemType Directory -Force $folder | Out-Null
try {
    Copy-Item (Join-Path $bin 'duet.exe'), (Join-Path $bin 'duet-cli.exe') $folder
    Copy-Item LICENSE (Join-Path $folder 'LICENSE.txt')
    Copy-Item dist\THIRD_PARTY_NOTICES.md $folder
    if (Test-Path $zip) { Remove-Item $zip }
    Compress-Archive -Path $folder -DestinationPath $zip -CompressionLevel Optimal
} finally {
    Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue
}

$sums = foreach ($f in $setup, $zip) {
    '{0}  {1}' -f (Get-FileHash $f -Algorithm SHA256).Hash.ToLower(), (Split-Path $f -Leaf)
}
[IO.File]::WriteAllText((Join-Path (Resolve-Path dist) 'SHA256SUMS.txt'), ($sums -join "`n") + "`n")
Get-ChildItem dist | Format-Table Name, Length -AutoSize
