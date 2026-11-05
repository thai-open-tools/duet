# Installs duet for the current user: copies the release build to %LOCALAPPDATA%\Programs\duet
# and adds "duet compare" shortcuts to the Desktop and the Start menu. Run it again after a new build.
# Remove: run `duet --unregister-shell` first (the Explorer right-click verbs), then delete that
# folder, %LOCALAPPDATA%\duet (settings, recent list) and the two shortcuts.
param(
    [string]$BinDir = (Join-Path $PSScriptRoot '..\target\release'),
    [switch]$Build
)
$ErrorActionPreference = 'Stop'

if ($Build) {
    Push-Location (Join-Path $PSScriptRoot '..')
    try {
        cargo build --release -p duet-app -p duet-cli
        if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
    } finally {
        Pop-Location
    }
}

$dest = Join-Path $env:LOCALAPPDATA 'Programs\duet'
$exe = Join-Path $dest 'duet.exe'

$running = Get-Process -Name duet -ErrorAction SilentlyContinue |
    Where-Object { $_.Path -eq $exe }
if ($running) { throw "duet is running from $exe; close it first." }

New-Item -ItemType Directory -Force -Path $dest | Out-Null
foreach ($name in 'duet.exe', 'duet-cli.exe') {
    $src = Join-Path $BinDir $name
    if (-not (Test-Path $src)) { throw "$src not found; build first or pass -Build." }
    Copy-Item -Path $src -Destination (Join-Path $dest $name) -Force
}

$shell = New-Object -ComObject WScript.Shell
$places = @(
    [Environment]::GetFolderPath('Desktop'),
    [Environment]::GetFolderPath('Programs')
)
# WScript.Shell cannot save into a folder with a non-ASCII name (a Thai "Desktop", say), so the
# shortcut is saved in the temp folder and moved into place.
$staged = Join-Path ([IO.Path]::GetTempPath()) 'duet-install.lnk'
foreach ($dir in $places) {
    $link = $shell.CreateShortcut($staged)
    $link.TargetPath = $exe
    $link.WorkingDirectory = $dest
    $link.Description = 'duet compare: folders and files'
    $link.IconLocation = "$exe,0"
    $link.Save()
    Move-Item -LiteralPath $staged -Destination (Join-Path $dir 'duet compare.lnk') -Force
    # The name these shortcuts had before: removed only when it is this install's own. Read from
    # a copy in the temp folder, as WScript.Shell cannot read a non-ASCII path either.
    $old = Join-Path $dir 'duet.lnk'
    if (Test-Path -LiteralPath $old) {
        $peek = Join-Path ([IO.Path]::GetTempPath()) 'duet-old.lnk'
        Copy-Item -LiteralPath $old -Destination $peek -Force
        $target = $shell.CreateShortcut($peek).TargetPath
        Remove-Item -LiteralPath $peek
        if ($target -eq $exe) { Remove-Item -LiteralPath $old }
    }
}
# The Explorer right-click verbs, written by the exe itself. Start-Process -Wait, because duet.exe is
# a GUI-subsystem exe and `&` would not wait for it.
$reg = Start-Process -FilePath $exe -ArgumentList '--register-shell' -Wait -PassThru
if ($reg.ExitCode -ne 0) { throw '--register-shell failed' }
# Explorer caches icons: without this, a shortcut made before the exe had its icon stays blank.
$refresh = Join-Path $env:SystemRoot 'System32\ie4uinit.exe'
if (Test-Path $refresh) { & $refresh -show }

Write-Output "installed: $exe"
Write-Output ("shortcuts: " + ($places -join '; '))
