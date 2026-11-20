# Offline Monaco proxy: no extensions, no chat, one disposable plaintext file.
param([Parameter(Mandatory=$true)][string]$Probe,
      [Parameter(Mandatory=$true)][string]$Scratch)
$ErrorActionPreference = 'Stop'
$exe = @('HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\Code.exe', 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\Code.exe') | ForEach-Object { if (Test-Path $_) { (Get-Item $_).GetValue('') } } | Select-Object -First 1
if (!$exe -or !(Test-Path $exe)) { throw 'VS Code executable not found' }
$dir = Join-Path $Scratch ('code-fixture-' + [Guid]::NewGuid().ToString('N'))
$profile = Join-Path $dir 'profile'
$user = Join-Path $profile 'User'
$null = New-Item -ItemType Directory -Path $user
$file = Join-Path $dir 'fixture.txt'
[IO.File]::WriteAllText($file, '')
[IO.File]::WriteAllText((Join-Path $user 'settings.json'), '{"telemetry.telemetryLevel":"off","update.mode":"none","workbench.startupEditor":"none","editor.accessibilitySupport":"on","security.workspace.trust.enabled":false}')
$process = Start-Process -FilePath $exe -ArgumentList @('--new-window','--disable-extensions', '--skip-release-notes', "--user-data-dir=`"$profile`"", "`"$file`"") -PassThru
try {
    Start-Sleep -Seconds 4
    # Cold Electron accessibility initialization can take longer than one probe deadline.
    for ($i = 0; $i -lt 3; $i++) {
        $warmup = & $Probe code $process.Id probe
        if (($warmup -join ' ') -match 'verdict=Eligible') { break }
        Start-Sleep -Milliseconds 750
    }
    $out = & $Probe code $process.Id one
    $out | Write-Output
    if ($LASTEXITCODE -ne 0 -or ($out -join ' ') -notmatch 'outcome=Delivered') { throw 'Monaco fixture not qualified' }
    & $Probe code $process.Id save
    Start-Sleep -Milliseconds 300
    if ([IO.File]::ReadAllText($file) -cne 'office-fixture') { throw 'Monaco fixture readback was not exact' }
    & $Probe code $process.Id undo
    Start-Sleep -Milliseconds 150
    & $Probe code $process.Id save
    Start-Sleep -Milliseconds 300
    if ([IO.File]::ReadAllText($file) -cne '') { throw 'Monaco undo did not restore the fixture' }
    Write-Output 'Monaco file editor: exact single-line readback; one Ctrl+Z restored the empty file'
} finally {
    if (!$process.HasExited) { $null = $process.CloseMainWindow(); if (!$process.WaitForExit(2000)) { $process.Kill(); $process.WaitForExit() } }
    Remove-Item -LiteralPath $dir -Recurse -Force
}
