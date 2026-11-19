# Sambler engine audit as of November 18, 2026

The Rust core can already power a dictation and editing prototype. Two points condition its use as an everyday cross-platform engine. On the tested Xiaomi, the transport fails to establish a secure connection with its default configuration. For a distributed application, the repository documents no Rambler access granted by Google to that application. The engine depends on the remote service for speech recognition and rewriting. [Audit measurements](../evidence/audit-moteur-20261118.json) · [Access investigated](native-access-public-research.md)

Revision examined: `9a29ab8`. New trials covered Windows x64 and the Xiaomi 11 Lite 5G NE, Android 14, ARM64, via ADB. CI for the same commit also covers Ubuntu, macOS Apple Silicon, and macOS Intel. Rambler service recognition observations come from earlier repository campaigns. The Xiaomi was the only reference device available. [Rust CI](https://github.com/Antoninnnnnnnn/Sambler/actions/runs/44350663932) · [Real campaign](../evidence/rust-native-real-campaign-20261118.json)

## Two behaviors reproduced before integration

**1. Android transport finds zero trusted certificates.** The test binary uses the same certificate loader as the engine. On the phone, its 47 ordinary tests pass. The explicit system-store test fails with the generic `Connect` error. A probe opening an HTTP/2 connection to `www.google.com` using the repository transport gives the following result. No audio, application identifier, or dictation call is needed for this trial. [TLS construction](../../crates/rambler-transport/src/connection.rs) · [Connection record](../evidence/audit-moteur-20261118.json)

| Environment | Certificates found | HTTP/2 connection |
| --- | ---: | --- |
| Windows, default configuration | 75 | Successful |
| Xiaomi, default configuration | 0 | Failed |
| Same Xiaomi, Android system store specified only to the control process | 120 | Successful, clean shutdown |

The Unix loader in `rustls-native-certs 0.8.5` delegates discovery to `openssl-probe 0.2.2`. Its default Android paths do not cover the store used on this Xiaomi. **The preferred fix is to integrate Android's trust policy**, retain server verification, and distinguish an empty store from a network failure. The acceptance test is a verified TLS connection from an ordinary Android app with its default settings. The ADB trial isolated the cause in a shell process. The phone uses 4 KiB memory pages. A trial of the future native library on a 16 KiB device or emulator will complete coverage. [Locked dependencies](../../Cargo.lock) · [Android 16 KiB page support](https://developer.android.com/guide/practices/page-sizes)

**2. A 31-second dictation exceeds the default timeout.** I sent the same silent 31-second WAV at real-time pace to a local server that waited for the end of audio before responding. The Release CLI returned `timeout` after 30.287 s. With `--timeout 40`, it returned the expected result after 31.032 s. The SDK and CLI default budget includes the entire capture. For free dictation, define the maximum supported duration, reserve time for the final response, and test that contract with a real-time source. The SDK already exposes `finalize_timeout`. [Session options](../../crates/rambler-engine/src/engine.rs) · [Measurements](../evidence/audit-moteur-20261118.json)

## Fidelity to Google's Rambler

Google describes hesitation removal, punctuation, targeted corrections, tone rewriting, emoji, and mid-sentence language switching. It also documents basic offline corrections. Its help page cites the Pixel 11 series and its November announcement extends rollout to Pixel 10. PixelBoard 1.3.5 is an independent project with its own editing and language-handling fixes. [Google help](https://support.google.com/gboard/answer/17468539?hl=fr) · [Pixel announcement](https://blog.google/products-and-platforms/devices/pixel/november-2026-pixel-drop/) · [PixelBoard 1.3.5 notes](https://github.com/Akshayykadam/PixelBoard/releases/tag/1.3.5)

| Feature | What retained trials demonstrate |
| --- | --- |
| EN/FR dictation and spoken self-correction | Punctuated text and correction of “ten” to “eleven” observed. Hesitation removal rate in spontaneous speech remains to be measured. |
| Replacement with context | In “The kite is blue. Marker stays.”, the engine replaced only “blue” with “green”. PixelBoard also succeeded on this example on the Xiaomi. |
| Tone, concision, and emoji | The service returned structured rewrites. The explicit `S3ClientCompatible` policy can present selection rewrites with intent 3 and field 7. Strict rendering can flag ambiguity. On the Xiaomi, PixelBoard inserted the instruction « Rends ça plus poli » literally. The two trials used different voices. |
| Languages | A sequence of two synthetic English and French clips succeeded. Spontaneous bilingual speech, mid-sentence switching, and other advertised languages remain to be qualified. |
| Composition and continuing a draft | Targeted examples transcribed the instruction or added it to the text. The `COMPOSE`, `RAMBLE`, and `RAMBLE_HISTORY` intents still have no confirmed server behavior for this client. |
| Offline | The SDK encodes audio locally. Local recognition and linguistic cleanup described for Gboard are absent. |

Natural campaigns cover **three read clips and 18.885 seconds for Rust**. The often-cited result of 3 errors in 109 English words and 3 in 79 French words comes from **ten Python reference clips**. In French, another campaign reproduced « Merci de envoyer » on all five synthetic passes of the same sentence. Correct punctuation or a low error rate on read books is insufficient to assess a rewrite that must preserve meaning, a meeting time, or the rest of a selection. [Rust corpus](../evidence/rust-native-natural-public.json) · [Python evaluation](../evidence/natural-expansion-quality.json) · [PixelBoard comparison](../evidence/pixelboard-vs-native-real-20261118.json) · [Synthetic campaign](../evidence/rust-native-real-campaign-20261118.json)

## Speed, memory, and stability

| Measurement | Result | Scope |
| --- | --- | --- |
| Encoding on Xiaomi | For 5.001 s of PCM at 16 kHz, medians of **21.98 ms** for a harmonic signal, **35.44 ms** for silence, and **48.35 ms** for noise | PCM → Opus/Ogg pipeline in Release, 20 ms pages, seven sets of three iterations after warmup. Elapsed time without network. Battery life and prolonged heating remain to be measured. |
| Windows local processing at 48 kHz | In the historical profile, Opus/Ogg accounts for approximately **56%** of cycles on the measured thread. The process consumes approximately **0.92 to 0.97 s of CPU** for 180 synthetic seconds, depending on the PCM source | Accelerated local session with a test server. The profile does not measure perceived delay when the microphone stops. |
| End of dictation on Google | Across five repetitions of one sentence per language, medians of **388 ms in English** and **373 ms in French**. The ten values range from **269 to 410 ms** | Historical Windows campaign. Approximate calculation: session end minus paced file duration. The first protocol response can precede any text. Initial connection and app display are outside this indicator. |
| Endurance and memory | Of two historical trials of a 288.35 s clip, one succeeded and one stopped incomplete after 282.20 s. Maximum sampled resident memory was about 11.3 MiB for each | Two other diagnostic trials succeeded. The cause of the stop and behavior over a day of successive sessions remain open. |

[Xiaomi profile and new tests](../evidence/audit-moteur-20261118.json) · [Windows CPU profile](../evidence/rust-native-pcm48-session-cpu-windows-20261118.md) · [Historical latency](../evidence/rust-native-real-campaign-20261118.json) · [Endurance and memory](../evidence/rust-endurance-300-public.json) · [Incident follow-up](../evidence/rust-incomplete-diagnostic-followup.json)

**Intentional repetition deserves a separate quality case.** A 122.58 s clip repeating a sentence produced that sentence only once. A second 46.8 s clip returned its twelve distinct sentences. These observations leave the source of lost repetitions open. A trial with an annotated sequence containing several intentional repetitions would count what the user actually recovers. [Two long dictations](../evidence/rust-native-real-campaign-20261118.json)

## Robustness verified on this revision

- **Windows.** 558 Rust tests passed and 3 ignored in the all-modules, all-targets command. The 6 documentation tests, explicit certificate check, formatting, and Clippy passed. Rust CI for the commit is green on Windows, Ubuntu, macOS Intel, and Apple Silicon. The Google dictation campaigns examined ran on Windows. [Record](../evidence/audit-moteur-20261118.json) · [Rust CI](https://github.com/Antoninnnnnnnn/Sambler/actions/runs/44350663932)
- **Android.** In addition to transport tests, 7 PCM 48 kHz path tests and 1 input-splitting test pass. Each platform completed **40 sessions on a local server**, divided among literal dictation, rewriting, selection, and preview. Rendered text fingerprints match across all 40 pairs. Ogg size differs between platforms for two of the four signals. Server responses are predefined in this trial. [Measurements and fingerprints](../evidence/audit-moteur-20261118.json) · [Local scenarios](../../crates/rambler-cli/examples/qualification_support/fixtures.rs)
- **Python reference.** A first run remained stuck during the producer's asynchronous close test. It was interrupted after 761 s total. All 51 transport-file tests then passed in isolation, and the full suite finished in 53.63 s with 923 passes and 4 skipped tests. The cause of the wait remains undetermined. [Relevant test](../../tests/test_transport.py) · [Python CI](https://github.com/Antoninnnnnnnn/Sambler/actions/runs/44350663821)
- **Dependencies and data.** An exact-version search in OSV returned no advisories for the 138 Cargo lock registry entries and installed versions of `grpcio` and `av`. This check covers indexed advisories for these packages. Editor context can be sent with audio. The CLI can re-emit it with the result. The future app should keep this content out of telemetry. [Search result](../evidence/audit-moteur-20261118.json) · [OSV](https://osv.dev/) · [CLI output](../../crates/rambler-cli/src/main.rs)

## Proposed work order

1. **Google access and Android TLS, in parallel.** Establish how Google provisions and can revoke application-specific access. Historical connections used metadata from the analyzed client. On Android, validate transport with system trust from an ordinary app. These two points must succeed independently. [Access status](native-access-public-research.md)
2. **Duration, interruption, and next dictation.** Test the chosen maximum duration, network loss mid-utterance, waking from sleep, and opening a new dictation after network recovery. Current transport requires explicit reconnection for new sessions. Preserve verified send completion and the distinction between a complete result and interruption. [Connection lifecycle](rust-connection-lifecycle.md)
3. **Dictation quality and editing safety.** Build an FR/EN spontaneous-voice corpus with noise, accents, proper names, numbers, hesitations, self-corrections, and intentional repetitions. For each edit, annotate the expected change and text to preserve around the cursor. Measure word errors, correct edits, and off-target changes separately. Add a paired comparison on official Gboard when an eligible Pixel is available. [SDK guide](sdk-integration-guide.md) · [Editing contract](edit-acceptance-contract.md)
4. **Perceptible latency.** Timestamp the last captured sample, first usable text, and validated completion, then measure median and 95th percentile across several networks with cold and reused connections. Encoding time observed on the Xiaomi supports prioritizing this end-to-end measurement.
