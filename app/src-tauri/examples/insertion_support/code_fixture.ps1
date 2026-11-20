# Disposable local VS Code window. The fixture never submits a chat prompt.
param([Parameter(Mandatory=$true)][string]$Probe,
      [Parameter(Mandatory=$true)][string]$Scratch)
$ErrorActionPreference = 'Stop'
$exe = @('HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\Code.exe', 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\Code.exe') | ForEach-Object { if (Test-Path $_) { (Get-Item $_).GetValue('') } } | Select-Object -First 1
if (!$exe -or !(Test-Path $exe)) { throw 'VS Code executable not found' }
$dir = Join-Path $Scratch ('code-fixture-' + [Guid]::NewGuid().ToString('N'))
$profile = Join-Path $dir 'profile'
$user = Join-Path $profile 'User'
$workspace = Join-Path $dir 'workspace'
$driver = Join-Path $dir 'driver'
$null = New-Item -ItemType Directory -Path $user,$workspace,$driver
$file = Join-Path $workspace 'fixture.txt'
[IO.File]::WriteAllText((Join-Path $user 'settings.json'), '{"telemetry.telemetryLevel":"off","update.mode":"none","workbench.startupEditor":"none","editor.accessibilitySupport":"on","security.workspace.trust.enabled":false}')
[IO.File]::WriteAllText((Join-Path $driver 'package.json'), '{"name":"offline-sambler-fixture","publisher":"local-fixture","version":"0.0.1","engines":{"vscode":"^1.90.0"},"main":"./driver.js","activationEvents":["onStartupFinished"]}')
[IO.File]::WriteAllText((Join-Path $driver 'driver.js'), @'
const vscode = require('vscode');
const fs = require('node:fs');
const path = require('node:path');
const phase = path.join(__dirname, 'phase');
const done = path.join(__dirname, 'done');
exports.activate = context => {
  let previous = '';
  let terminal;
  const timer = setInterval(async () => {
    if (!fs.existsSync(phase)) return;
    const next = fs.readFileSync(phase, 'utf8');
    if (!next || next === previous) return;
    previous = next;
    try {
      if (next === 'chat') await vscode.commands.executeCommand('workbench.action.chat.open');
      else if (next === 'terminal') {
        terminal = vscode.window.createTerminal({ name: 'Sambler fixture' });
        terminal.show(false);
      } else if (next === 'editor') {
        terminal?.dispose();
        const file = vscode.Uri.file(path.join(__dirname, '..', 'workspace', 'fixture.txt'));
        await vscode.workspace.fs.writeFile(file, new Uint8Array());
        await vscode.window.showTextDocument(file, { preview: false, preserveFocus: false });
        await vscode.commands.executeCommand('workbench.action.focusActiveEditorGroup');
      } else return;
      fs.writeFileSync(done, next);
    } catch { fs.writeFileSync(done, 'failed:' + next); }
  }, 100);
  context.subscriptions.push({ dispose: () => { clearInterval(timer); terminal?.dispose(); } });
};
'@)
function Invoke-FixturePhase([string]$Name) {
    [IO.File]::WriteAllText((Join-Path $driver 'phase'), $Name)
    $until = [DateTime]::UtcNow.AddSeconds(9)
    $done = Join-Path $driver 'done'
    while ([DateTime]::UtcNow -lt $until) {
        if (Test-Path $done) {
            $value = [IO.File]::ReadAllText($done)
            if ($value -eq $Name) { Start-Sleep -Milliseconds 400; return $true }
            if ($value -eq "failed:$Name") { return $false }
        }
        Start-Sleep -Milliseconds 120
    }
    return $false
}
$process = Start-Process -FilePath $exe -ArgumentList @('--new-window','--disable-extensions', '--skip-release-notes', '--force-renderer-accessibility', "--extensionDevelopmentPath=`"$driver`"", "--user-data-dir=`"$profile`"", "`"$workspace`"") -PassThru
try {
    if (Invoke-FixturePhase 'chat') {
        Write-Output 'VS Code after Open Chat command (no text or submit):'
        & $Probe code $process.Id code-shape
        & $Probe code $process.Id code-empty
        if ($LASTEXITCODE -eq 0) {
            $inserted = & $Probe code $process.Id one
            $inserted | Write-Output
            if (($inserted -join ' ') -match 'outcome=Delivered') {
                Start-Sleep -Milliseconds 300
                & $Probe code $process.Id code-read
                if ($LASTEXITCODE -eq 0) {
                    Write-Output 'VS Code chat: exact UIA prompt readback; prompt remained in the input without submitting'
                } else { Write-Output 'VS Code chat: insertion was not verified' }
            }
        }
    } else { Write-Output 'VS Code chat command did not activate' }
    if (Invoke-FixturePhase 'terminal') {
        Write-Output 'VS Code integrated terminal (no command entered):'
        & $Probe code $process.Id code-shape
        & $Probe code $process.Id probe
    } else { Write-Output 'VS Code terminal command did not activate' }
    if (!(Invoke-FixturePhase 'editor')) { throw 'VS Code fixture extension did not focus its editor' }
    Write-Output 'VS Code Monaco fixture:'
    & $Probe code $process.Id code-shape
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
