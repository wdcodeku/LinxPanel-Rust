$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Push-Location $repoRoot
try {
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        throw "cargo not found in PATH"
    }
    if (-not (Get-Command npm -ErrorAction SilentlyContinue)) {
        throw "npm not found in PATH"
    }

    Write-Host "[LinxPanel] Building web assets..."
    Push-Location (Join-Path $repoRoot "web")
    try {
        if (Test-Path "package-lock.json") {
            npm ci
        } else {
            npm install
        }
        npm run build
    } finally {
        Pop-Location
    }

    Write-Host "[LinxPanel] Building backend release binary..."
    cargo build --release

    $binaryPath = Join-Path $repoRoot "target\\release\\linxpanel.exe"
    if (-not (Test-Path $binaryPath)) {
        throw "build completed but binary not found: $binaryPath"
    }
    Write-Host "[LinxPanel] Done. Single deployable file:"
    Write-Host "  $binaryPath"
} finally {
    Pop-Location
}
