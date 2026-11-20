# Build both Windows x64 downloads from ONE release build:
#   Sambler-<version>-windows-x64-setup.exe    (NSIS, per-user install)
#   Sambler-<version>-windows-x64-portable.zip (extract and run Sambler.exe)
#   SHA256SUMS.txt
# Usage (from anywhere):  powershell -ExecutionPolicy Bypass -File app\scripts\package.ps1
# See docs/windows/09-distribution-et-version-portable.md.
param(
    [string]$OutDir = (Join-Path $PSScriptRoot '..\..\dist-windows'),
    [switch]$SkipBuild
)
$ErrorActionPreference = 'Stop'
$app = Resolve-Path (Join-Path $PSScriptRoot '..')
$repo = Resolve-Path (Join-Path $app '..')
$cmake = Join-Path $env:APPDATA 'Python\Python314\Scripts'
if (Test-Path $cmake) { $env:PATH = "$cmake;$env:PATH" }

$conf = Get-Content (Join-Path $app 'src-tauri\tauri.conf.json') -Raw | ConvertFrom-Json
$version = $conf.version
$base = "Sambler-$version-windows-x64"

Push-Location $app
try {
    if (-not $SkipBuild) {
        npm ci
        npm run tauri build
        if ($LASTEXITCODE -ne 0) { throw "tauri build failed" }
    }
} finally { Pop-Location }

$release = Join-Path $app 'src-tauri\target\release'
$exe = Join-Path $release 'Sambler.exe'
$nsis = Get-ChildItem (Join-Path $release 'bundle\nsis') -Filter '*-setup.exe' |
    Sort-Object LastWriteTime -Descending | Select-Object -First 1
if (-not (Test-Path $exe)) { throw "missing $exe" }
if (-not $nsis) { throw "missing NSIS installer" }

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$OutDir = Resolve-Path $OutDir
$setup = Join-Path $OutDir "$base-setup.exe"
Copy-Item $nsis.FullName $setup -Force

# Portable folder: same executable, marker, licences and notices. No Data/.
$stage = Join-Path $OutDir "$base-portable"
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory -Path (Join-Path $stage 'Sambler') | Out-Null
$dest = Join-Path $stage 'Sambler'
Copy-Item $exe $dest
Set-Content -Path (Join-Path $dest 'portable.flag') -Value 'Sambler portable edition: settings are stored in the Data folder next to Sambler.exe.' -Encoding utf8
Copy-Item (Join-Path $repo 'LICENSE.md') $dest
Copy-Item (Join-Path $repo 'crates\rambler-audio\THIRD_PARTY_LICENSES') (Join-Path $dest 'THIRD_PARTY_LICENSES-opus.txt')
@"
Sambler $version - édition portable pour Windows 11 x64

1. Extrayez tout le dossier « Sambler » dans un emplacement inscriptible.
2. Lancez Sambler.exe (compte standard, aucune installation).
3. Les réglages sont enregistrés dans le dossier Data à côté de Sambler.exe.
   L'accès Rambler n'est jamais enregistré dans ce dossier.

Prérequis : Microsoft Edge WebView2 Runtime (préinstallé avec Windows 11).
La dictée utilise Rambler/Jetson de Google : votre voix est transmise à Google
pendant que vous parlez, après activation dans Sambler.
Exécutables non signés : vérifiez l'empreinte SHA-256 publiée avec la version.
"@ | Set-Content -Path (Join-Path $dest 'LISEZMOI.txt') -Encoding utf8

$zip = Join-Path $OutDir "$base-portable.zip"
if (Test-Path $zip) { Remove-Item $zip -Force }
Compress-Archive -Path (Join-Path $stage 'Sambler') -DestinationPath $zip
Remove-Item $stage -Recurse -Force

$sums = foreach ($file in @($setup, $zip)) {
    $hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $file).Hash.ToLowerInvariant()
    "$hash  $(Split-Path $file -Leaf)"
}
$sums | Set-Content -Path (Join-Path $OutDir 'SHA256SUMS.txt') -Encoding ascii
Write-Host "Paquets prêts dans $OutDir"
$sums | ForEach-Object { Write-Host $_ }
