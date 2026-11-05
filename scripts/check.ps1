$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')
cargo fmt --all -- --check;                          if ($LASTEXITCODE) { exit 1 }
# Without tests: dev-dependencies switch on gpui-kit's test-support feature, so code that only
# compiles with it passes every line below and still breaks the real build.
cargo check --workspace;                             if ($LASTEXITCODE) { exit 1 }
cargo clippy --workspace --all-targets -- -D warnings; if ($LASTEXITCODE) { exit 1 }
cargo test --workspace;                              if ($LASTEXITCODE) { exit 1 }
cargo deny check bans licenses sources;              if ($LASTEXITCODE) { exit 1 }
Write-Host 'check: OK'
