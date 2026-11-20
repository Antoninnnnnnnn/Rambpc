# Acceptance check for the RUNNING Sambler app in Word (not part of the app).
# Creates one blank, never-saved Word document, presses Ctrl+Shift+F2 like a
# keyboard (hold), plays the WAV on the speakers, releases, then reads the
# document and closes it without saving. Sambler must run with consent and
# access, in hold mode with the default shortcut.
param([Parameter(Mandatory=$true)][string]$Wav)
$ErrorActionPreference = 'Stop'
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class K {
  [StructLayout(LayoutKind.Sequential)] public struct KEYBDINPUT { public ushort wVk; public ushort wScan; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
  [StructLayout(LayoutKind.Explicit, Size=40)] public struct INPUT { [FieldOffset(0)] public uint type; [FieldOffset(8)] public KEYBDINPUT ki; }
  [DllImport("user32.dll")] public static extern uint SendInput(uint n, INPUT[] inputs, int size);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr w);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  public static void Keys(ushort[] vks, bool up) {
    var a = new INPUT[vks.Length];
    for (int i = 0; i < vks.Length; i++) { a[i].type = 1; a[i].ki.wVk = vks[i]; a[i].ki.dwFlags = up ? 2u : 0u; }
    SendInput((uint)a.Length, a, Marshal.SizeOf(typeof(INPUT)));
  }
}
'@
$word = New-Object -ComObject Word.Application
$doc = $null
try {
    $doc = $word.Documents.Add()
    $word.Visible = $true
    $win = $doc.Windows.Item(1)
    $win.Activate()
    $hwnd = [IntPtr][int64]$win.Hwnd
    [K]::Keys(@(0x12), $false); [K]::Keys(@(0x12), $true)   # Alt tap lifts the foreground lock
    $null = [K]::SetForegroundWindow($hwnd)
    $doc.Range().Text = "Avant : "
    $end = $doc.Content.End - 1
    $win.Selection.SetRange($end, $end)
    Start-Sleep -Milliseconds 800
    if ([K]::GetForegroundWindow() -ne $hwnd) { throw 'Word fixture is not foreground' }
    $before = $doc.Content.Text
    [K]::Keys(@(0x11, 0x10, 0x71), $false)                    # Ctrl, Shift, F2 down
    Start-Sleep -Milliseconds 700
    (New-Object Media.SoundPlayer $Wav).PlaySync()
    Start-Sleep -Milliseconds 400
    $stop = [Diagnostics.Stopwatch]::StartNew()
    [K]::Keys(@(0x71, 0x10, 0x11), $true)                     # release
    $after = $before
    while ($stop.ElapsedMilliseconds -lt 10000) {
        $after = $doc.Content.Text
        if ($after -ne $before) { break }
        Start-Sleep -Milliseconds 10
    }
    $ms = $stop.ElapsedMilliseconds
    Start-Sleep -Milliseconds 500
    $after = $doc.Content.Text
    if ($after -eq $before) { Write-Output "[FAIL] word: nothing inserted within 10 s" }
    else { Write-Output ("[OK]   word: document={0}; visible {1} ms after the stop" -f ($after.Trim() | ConvertTo-Json), $ms) }
} finally {
    if ($null -ne $doc) { $doc.Close(0); Write-Output 'cleanup: document closed without saving' }
}
