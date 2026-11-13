param(
    [string]$OutputDirectory = "artifacts/synthetic-v1",
    [string]$CasesFile,
    [switch]$NormalizePcm
)
$ErrorActionPreference = "Stop"

# Windows-only corpus preparation. The engine itself does not depend on SAPI.
# Original synthetic sentences; no microphone or personal recording is used.
if (Test-Path -LiteralPath $OutputDirectory) {
    throw "Output directory already exists; choose a new directory to preserve prior fixtures."
}
$null = Get-Command ffmpeg -ErrorAction Stop
$null = Get-Command ffprobe -ErrorAction Stop
$directory = New-Item -ItemType Directory -Path $OutputDirectory
$voice = New-Object -ComObject SAPI.SpVoice
$cases = @(
    @{id="plain-en"; language="en-US"; voice="Microsoft Zira Desktop";
      text="Hello. This is a test of intelligent dictation. The meeting is tomorrow at eleven.";
      property="Produce coherent punctuated dictation; do not assume exact spelling."},
    @{id="correction-en"; language="en-US"; voice="Microsoft Zira Desktop";
      text="The meeting is tomorrow at ten. No, make that eleven.";
      property="Observe whether the oral correction replaces ten with eleven."},
    @{id="context-edit-en"; language="en-US"; voice="Microsoft Zira Desktop";
      text="Change ten to eleven.";
      before="The meeting is tomorrow at ten.";
      property="Observe a structured edit using supplied before-cursor context."},
    @{id="plain-fr"; language="fr-FR"; voice="Microsoft Hortense Desktop";
      text="Bonjour. Ceci est un essai de dictee intelligente. La reunion aura lieu demain a onze heures.";
      property="Observe French language support; synthesis accents are not a quality reference."}
)
if ($CasesFile) {
    # Windows PowerShell 5.1 emits a JSON root array as one pipeline object.
    # Assign it directly instead of wrapping it in another array subexpression.
    $cases = Get-Content -LiteralPath $CasesFile -Raw -Encoding UTF8 | ConvertFrom-Json
    if ($cases.Count -eq 0) { throw "Cases file must contain at least one case." }
}
$ids = @{}
foreach ($case in $cases) {
    if ($case.id -isnot [string] -or $case.id -notmatch '^[a-zA-Z0-9_-]+$' -or $ids.ContainsKey($case.id)) {
        throw "Case IDs must be unique, nonempty filename-safe identifiers."
    }
    if ($case.text -isnot [string] -or $case.voice -isnot [string] -or [string]::IsNullOrWhiteSpace($case.text) -or [string]::IsNullOrWhiteSpace($case.voice)) {
        throw "Each case requires text and an installed voice name."
    }
    $ids[$case.id] = $true
}
$results = @()
foreach ($case in $cases) {
    $tokens = @($voice.GetVoices() | Where-Object { $_.GetDescription().StartsWith($case.voice) })
    if ($tokens.Count -ne 1) { throw "Expected exactly one installed voice for $($case.id)." }
    $voice.Voice = $tokens[0]
    $voice.Rate = 0
    $voice.Volume = 100
    $wav = Join-Path $directory.FullName ($case.id + ".wav")
    $ogg = Join-Path $directory.FullName ($case.id + ".ogg")
    $stream = New-Object -ComObject SAPI.SpFileStream
    try {
        $stream.Open($wav, 3, $false)
        $voice.AudioOutputStream = $stream
        $null = $voice.Speak($case.text)
    } finally {
        $stream.Close()
    }
    & ffmpeg -nostdin -hide_banner -loglevel error -n -i $wav -ac 1 -ar 16000 -c:a libopus -b:a 24k -vbr on -application voip -f ogg $ogg
    if ($LASTEXITCODE -ne 0) { throw "FFmpeg failed for $($case.id)." }
    $duration = & ffprobe -v error -show_entries format=duration -of default=noprint_wrappers=1:nokey=1 $ogg
    if ($LASTEXITCODE -ne 0) { throw "FFprobe failed for $($case.id)." }
    $entry = [ordered]@{
        id=$case.id; language=$case.language; text=$case.text;
        before=$case.before; expected_property=$case.property;
        synthesis_voice=$tokens[0].GetDescription(); synthesis_rate=0;
        wav_file=($case.id + ".wav"); ogg_file=($case.id + ".ogg");
        wav_sha256=(Get-FileHash -Algorithm SHA256 $wav).Hash.ToLowerInvariant();
        ogg_sha256=(Get-FileHash -Algorithm SHA256 $ogg).Hash.ToLowerInvariant();
        ogg_bytes=(Get-Item $ogg).Length;
        duration_s=[double]::Parse($duration, [Globalization.CultureInfo]::InvariantCulture)
    }
    if ($NormalizePcm) {
        $pcm = Join-Path $directory.FullName ($case.id + "-16k.wav")
        & ffmpeg -nostdin -hide_banner -loglevel error -n -i $wav -ac 1 -ar 16000 -c:a pcm_s16le $pcm
        if ($LASTEXITCODE -ne 0) { throw "PCM normalization failed for $($case.id)." }
        $entry.pcm16_file = ($case.id + "-16k.wav")
        $entry.pcm16_sha256 = (Get-FileHash -Algorithm SHA256 $pcm).Hash.ToLowerInvariant()
    }
    $results += $entry
}
$manifest = [ordered]@{
    created_at=[DateTime]::UtcNow.ToString("o");
    meaning="Synthetic input fixtures, not recognition results or proof of Rambler support.";
    format="continuous Ogg/Opus, mono, original sample rate 16000 Hz, target bitrate 24000 bit/s, VBR, voip";
    ffmpeg_version=(& ffmpeg -version | Select-Object -First 1);
    samples=$results
}
if ($CasesFile) {
    $manifest.cases_sha256 = (Get-FileHash -Algorithm SHA256 $CasesFile).Hash.ToLowerInvariant()
}
$manifest | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $directory.FullName "manifest.json") -Encoding UTF8
Write-Output "Prepared $($results.Count) synthetic fixtures in $($directory.FullName)."
