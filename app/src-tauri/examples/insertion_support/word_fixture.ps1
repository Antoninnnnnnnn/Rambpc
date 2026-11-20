# Offline: creates and closes one unsaved Word document. Never saves or opens a file.
param([Parameter(Mandatory=$true)][string]$Probe)
$ErrorActionPreference = 'Stop'
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class WordFixtureWindow {
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr w, out uint pid);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr w);
}
'@
$word = New-Object -ComObject Word.Application
$doc = $null
try {
    $doc = $word.Documents.Add()
    $word.Visible = $true
    $win = $doc.Windows.Item(1)
    $win.Activate()
    $hwnd = [uint32]$win.Hwnd
    $pidWord = [uint32]0
    $null = [WordFixtureWindow]::GetWindowThreadProcessId([IntPtr]$hwnd, [ref]$pidWord)
    if ($pidWord -eq 0) { throw 'No Word fixture HWND' }
    $null = [WordFixtureWindow]::SetForegroundWindow([IntPtr]$hwnd)
    $doc.Range().Text = "before marker`rafter marker"
    $doc.Paragraphs.Item(1).Style = $doc.Styles.Item(-2)
    $doc.Paragraphs.Item(2).Style = $doc.Styles.Item(-1)
    $original = $doc.Content.Text
    $firstStyle = $doc.Paragraphs.Item(1).Style.NameLocal
    $lastStyle = $doc.Paragraphs.Item(2).Style.NameLocal
    $n = $doc.Paragraphs.Count
    foreach ($case in @('one','two')) {
        $position = if ($case -eq 'one') { 6 } else { $doc.Content.End - 1 }
        $win.Selection.SetRange($position, $position)
        Start-Sleep -Milliseconds 350
        $out = & $Probe word $pidWord $case $hwnd
        $out | Write-Output
        if ($LASTEXITCODE -ne 0 -or ($out -join ' ') -notmatch 'outcome=Delivered') { throw 'Word literal insertion refused' }
        $insert = if ($case -eq 'one') { 'office-fixture' } else { "office-fixture`rsecond-fixture" }
        $expected = $original.Insert($position, $insert)
        if ($doc.Content.Text -cne $expected) { throw 'Word fixture text mismatch' }
        $expectedCount = if ($case -eq 'one') { $n } else { $n + 1 }
        if ($doc.Paragraphs.Count -ne $expectedCount -or $doc.Paragraphs.Item(1).Style.NameLocal -cne $firstStyle -or $doc.Paragraphs.Item(2).Style.NameLocal -cne $lastStyle) { throw 'Word fixture paragraph/style mismatch' }
        & $Probe word $pidWord undo $hwnd
        if ($LASTEXITCODE -ne 0) { throw 'Word Ctrl+Z failed' }
        $until = [DateTime]::UtcNow.AddSeconds(2)
        while ($doc.Content.Text -cne $original -and [DateTime]::UtcNow -lt $until) { Start-Sleep -Milliseconds 25 }
        if ($doc.Content.Text -cne $original) { throw 'One Ctrl+Z did not restore the Word fixture' }
        Write-Output "Word $case`: exact; paragraph count and marker styles verified; one Ctrl+Z restored both markers"
    }
} finally {
    if ($null -ne $doc) { $doc.Close(0); Write-Output 'Word fixture: closed without saving' }
}
