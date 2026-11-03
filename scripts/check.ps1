$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')
cargo fmt --all -- --check;                          if ($LASTEXITCODE) { exit 1 }
cargo clippy --workspace --all-targets -- -D warnings; if ($LASTEXITCODE) { exit 1 }
cargo test --workspace;                              if ($LASTEXITCODE) { exit 1 }
cargo deny check bans licenses sources;              if ($LASTEXITCODE) { exit 1 }
Write-Host 'check: OK'
