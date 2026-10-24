$ErrorActionPreference = 'Stop'
Push-Location (Split-Path $PSScriptRoot -Parent)
try {
    cargo build --locked --release
    if ($LASTEXITCODE -ne 0) { throw 'Windows build failed' }
    Write-Host 'Built: target\release\periph-in-one.exe'
} finally {
    Pop-Location
}
