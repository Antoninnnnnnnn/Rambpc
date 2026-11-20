# Launch Sambler in development mode.
# Optional: loads Rambler access values from the repository's local .env file
# (never committed) into THIS process only, then runs `npm run tauri dev`.
param(
    [string]$EnvFile = (Join-Path $PSScriptRoot '..\..\.env')
)
$ErrorActionPreference = 'Stop'
if (Test-Path -LiteralPath $EnvFile) {
    foreach ($line in Get-Content -LiteralPath $EnvFile) {
        if ($line -match '^\s*(RAMBLER_[A-Z0-9_]+)\s*=\s*(.*)\s*$') {
            $value = $Matches[2].Trim().Trim('"').Trim("'")
            if ($value) { [Environment]::SetEnvironmentVariable($Matches[1], $value, 'Process') }
        }
    }
    Write-Host "Accès Rambler chargé depuis .env pour cette session."
} else {
    Write-Host "Aucun .env : configurez l'accès Rambler dans Sambler (Réglages)."
}
$cmake = Join-Path $env:APPDATA 'Python\Python314\Scripts'
if (Test-Path $cmake) { $env:PATH = "$cmake;$env:PATH" }
Set-Location (Join-Path $PSScriptRoot '..')
if (-not (Test-Path node_modules)) { npm ci }
npm run tauri dev
