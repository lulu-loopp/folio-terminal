param(
    [Parameter(Mandatory = $true)]
    [string]$ImeName,

    [string]$LogName
)

$ErrorActionPreference = "Stop"
$safeName = if ($LogName) { $LogName } else { $ImeName -replace '[^A-Za-z0-9_-]', '-' }
$log = Join-Path $PSScriptRoot "logs/$safeName.jsonl"

Push-Location $PSScriptRoot
try {
    cargo run --release --locked --bin ime-probe -- --ime-name $ImeName --log $log
    if ($LASTEXITCODE -ne 0) {
        throw "IME probe failed with exit code $LASTEXITCODE"
    }
    cargo run --release --locked --bin ime-log-audit -- $log --strict-ime
    if ($LASTEXITCODE -ne 0) {
        throw "IME log audit failed with exit code $LASTEXITCODE; keep $log as evidence"
    }
} finally {
    Pop-Location
}

Write-Host "Log: $log"
