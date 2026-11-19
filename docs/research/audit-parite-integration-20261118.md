# Sambler engine audit before integration

November 18, 2026 · Windows, Android, and comparison with Gboard Advanced Voice Typing.

**I would use the Rust core at `e52e568` as the application base.** Short dictation, audio processing, and session reuse are sufficiently tested for that choice. The most concrete risks concern speech arriving during a segment transition, recovery of already recognized text, and edit fidelity. Measurements give little reason to re-optimize Opus first. [Audit measurements][mesures]

The open checkout is at `9a29ab8`. I tested that version, intermediate revision `7ceb590`, then published revision `e52e568`, fourteen commits ahead of the checkout. The trials below concern the latter unless otherwise stated. Dictations sent to Rambler come from campaigns recorded in the project. My new dictation trials used local peers with known expected results. New Google connections tested the public HTTPS transport. [Audited revision][revision] · [Test record][mesures]

## Four behaviors to address before integration

| Priority | Observation reproduced on `e52e568` | Consequence and acceptance test |
| --- | --- | --- |
| High for microphone | During transition to the next segment, the engine waits for the previous final. With 90 blocks of 20 ms and a final artificially delayed by 600 ms, delivery of one block waited **581 ms on Xiaomi**, **599 ms on Windows**. All three segments completed. In the control without added delay, no wait exceeded 20 ms. | The test source retained its samples. A hardware microphone continues producing them during the wait. Provide a bounded buffer upstream of the one-slot channel and an overflow signal. Check continuity by sample indices with variable delays, including one exceeding buffer capacity. [PCM channel][canal] · [Segment transition][transition] |
| High for recovery | With the interactive CLI, I waited for the second call to start after the first final, then sent Ctrl-C. Exit **130**, with `completed_segments: []`. In the control where the server interrupts the second call, the CLI does return the first segment and its sample range. | Explicit cancellation has abandonment semantics. For a “stop and keep my dictation” action, end the source normally. To recover text after interruption too, expose or retain each validated result as soon as it finishes. Today, the segment-end event supplies its range. Test normal stop, abandonment, and failure separately. [CLI][interruption] · [End event][evenement] |
| Medium for multilingual dictation | The SDK and CLI always add a space between segments. With two synthetic finals, the CLI returns `L’ équipe arrive.` The SDK also returns `明天 开会。` and doubles the space if the second final already starts with one. Individual texts are intact. | Share the SDK and CLI rule, preserve original segments, and define rendering of apostrophes, punctuation, paragraphs, and languages without spaces between words. Replay these three cases through both public inputs. [SDK joining][raccord] · [CLI joining][raccord-cli] |
| Medium for a rich editor | In `oui oui`, the initial selection is the first `oui `, UTF-16 `0..4`. A structured response deletes it and retains the second `oui`. Grapheme-based calculation proposes deleting `3..7`. Both operations produce the expected plain text. | Occurrences can carry different links or styles. If the edit concerns the selection and structured context locates it unambiguously, use that anchor. Otherwise, present the replacement for review. Check the retained occurrence's attributes as well as final text. [Range calculation][splice] |

The long collector also retains responses from all segments. Collection limits apply per segment. For continuous mobile use, an API delivering the validated result and its range together would let the host save it and free its memory. [Collector contract][transition]

## Fidelity to Advanced Voice Typing

The product reference is the behavior described by Gboard and the text ultimately obtained in the editor. The project was reconstructed from PixelBoard 1.3.4. Its phone comparison uses PixelBoard 1.3.5 on Xiaomi. Google documents speech cleanup, targeted changes, tone, emoji, language switching, and basic offline dictation, among other features. It also states that text appears after input ends in this mode. [Official Gboard help][gboard] · [PixelBoard 1.3.5][pixelboard]

| Feature | Level reached |
| --- | --- |
| FR/EN dictation, punctuation, and spoken self-correction | The service produced punctuated sentences and corrected a meeting time after a spoken correction. Retention rates for names, amounts, negations, and intentional repetitions in spontaneous speech remain to be measured. [Dictation trials][dictee] |
| Change a word, rephrase a selection, shorten, and add an emoji | Real responses cover these paths. For some selection rewrites, the SDK requires the explicit `S3ClientCompatible` policy. On Xiaomi, PixelBoard succeeded at blue → green replacement and literally inserted the polite-rephrasing request. The two clients used different voices. [Native campaign][campagne] · [Phone comparison][comparaison] |
| Languages and mid-sentence switches | One synthetic sentence per language produced a final for 13 additional languages. Hindi and Tamil returned in Latin characters. An English then French clip retained both languages. These trials leave open recognition of a speaker spontaneously switching within a sentence. [Multilingual measurements][langues] |
| Compose, continue, or restructure a draft | Targeted trials transcribed or added the instruction. The client can decode `COMPOSE`, `RAMBLE`, `RAMBLE_HISTORY` intents. Their activation and expected result on this path remain to be established. [Composition trials][composition] |
| Offline | Gboard describes recognition with punctuation and basic corrections. Sambler encodes on-device and delegates recognition to the service. Adding this capability would require a local recognition engine. [Gboard help][gboard] · [Rust architecture][architecture] |

One case deserves a place in the quality corpus. Over 122.6 s of sentence repetitions, only one occurrence returned. Over 46.8 s with twelve distinct sentences, all twelve returned. The cause of the difference remains open. The often-cited figures of 3 errors in 109 English words and 3 in 79 French words concern ten read-book clips with the **Python** reference. A separate native campaign gives six deviations in a 520-word passage. None of these figures measures rewrite fidelity on spontaneous conversation. [Repetitions and long passage][fiabilite] · [Python evaluation][wer]

## Speed and stability

For local audio, I timed complete encoding of **10 s** of synthetic mono signal, presented in 20 ms blocks. Numbers are medians of nine passes after warmup. The 48 kHz column includes conversion to 16 kHz. Inputs were prepared in memory. [Per-pass data][mesures]

| Device | PCM 16 kHz → Opus/Ogg | PCM 48 kHz → 16 kHz → Opus/Ogg |
| --- | ---: | ---: |
| Windows 11, Core i5-12450H | 36.1 ms | 52.6 ms |
| Xiaomi, Android 14, ARM64 | 44.3 ms | 53.0 ms |

- **Perceived delay.** A retained real campaign of 100 short dictations on two synthetic sentences gives **351 ms median**, **447 ms at the 95th percentile**, maximum **1.143 s**, between estimated audio end and session end. The calculation subtracts the nominal duration of the paced file. For the future app, timestamp the last sample actually captured and the usable result. The SDK already exposes `validated_eof_at()` for a single session. [Reliability campaign][fiabilite] · [SDK timestamp][horloge]
- **Endurance.** On Windows and Xiaomi, I ran **1,001 sessions per platform** on a local connection. The 901 successes gave the expected text, the 100 injected failures gave the expected error, and the next session succeeded. On Xiaomi, resident memory rose from 6,536 KiB at the first point to 6,588 KiB at the hundredth and each of the next four points, through the last session. Sleep, Wi-Fi/mobile switching, day-long sessions, and battery consumption remain to be characterized. [Endurance record][mesures]
- **Speech duration.** The generic profile retains a 30 s timeout. The published interactive profile uses 120 s segments, a total budget of 135 s per session, and up to 5 s of finalization within that budget. My paced 31 s test finished in 30.993 s on Windows with this profile. The repository also contains a complete paced 300 s local-server trial. Two earlier real sends of 320 and 675 s in a single stream failed around 300 s. Preservation of meaning across a cut remains to be evaluated on natural speech. [Interactive profile][profil] · [Five-minute trial][cinqminutes] · [Service observations][fiabilite]

## Portability and integration

**Android.** In a test application launched under Android 16/API 36, the older revision fails to reach the public Google server while the native HTTPS client receives HTTP 204. On `e52e568`, after `init_android_tls`, the engine receives the HTTP/2 PONG. I then restricted the domain to a test authority in the application alone. The engine returned `CertificateRejected` and the Java client also rejected the certificate. This measurement confirms that the domain trust policy is considered. [Android contract][tls] · [Application measurements][mesures]

The Xiaomi rejected ordinary ADB installation of my APK. Application trials in this audit therefore ran on an Android 16 x86-64 emulator running the ARM64 library by translation. Audio and endurance measurements come from the physical Xiaomi. The project also has evidence of TLS verification in an application on this Xiaomi, with the same transport code. Both audit environments have 4 KiB memory pages. The test library compiled with NDK r27 without alignment settings outputs 4 KiB. The project checked its witness APK alignment for 16 KiB. Loading it on a 16 KiB page system remains to be tested. [Project physical trial][xiaomi] · [Android memory pages][pagesandroid]

**Windows, Linux, and macOS.** On Windows, `e52e568` passes **588 Rust tests**, with four optional checks ignored by the general command. I ran those four checks separately successfully, as well as the six documentation tests, formatting, and strict Clippy. The Python reference passes **931 tests**, with four skipped tests. CI for the same commit completed all ten Rust jobs, including Ubuntu, Windows, and macOS Intel and Apple Silicon. For Linux and macOS, the evidence used here covers CI builds and local tests. [Rust CI][ci] · [Python CI][cipy] · [Windows results][mesures]

**Inputs and data.** The SDK accepts, among other inputs, mono PCM16 at 16 or 48 kHz, and a continuous Ogg/Opus stream conforming to the profile. A 44.1 kHz or stereo source requires upstream conversion. The app can thus retain the same Rust core with a capture adapter per platform. Structured results and JSON output can contain dictation and editing context, including prior segments on failure. To diagnose incidents, log durations, error categories, volumes, and counters. An OSV search of the 150 registry versions in the Rust lock and installed versions of `grpcio` and `av` returned zero indexed advisories. [Audio API][architecture] · [CLI output][interruption] · [Version record][mesures]

## Recommended work order

1. **Protect an ongoing dictation.** Qualify block delivery during finalization, overflow policy, and delivery of already validated segments. Then simulate network loss followed by opening a new dictation.
2. **Fix text rendering rules.** Replay multilingual joins on the SDK and CLI, then the two identical words case in an editor with two styles. Retain the distinction between preview, validated text, and edit proposal.
3. **Measure perceived quality before changing the codec.** The project already has an FR/EN annotation plan. Execute it on several spontaneous voices, with the same sounds and editor state on the engine and on a Pixel with official Gboard. Count fact errors, removed repetitions, changes outside the selection, and delay after speech ends separately. [Existing annotation plan][qualite]

[mesures]: ../evidence/audit-parite-integration-20261118.json
[revision]: https://github.com/Antoninnnnnnnn/Sambler/tree/e52e568ec8a602a224b249970e69f749ffa9ab18
[canal]: ../../crates/rambler-audio/src/input.rs
[transition]: https://github.com/Antoninnnnnnnn/Sambler/blob/e52e568ec8a602a224b249970e69f749ffa9ab18/crates/rambler-audio/src/long_dictation.rs#L174
[evenement]: https://github.com/Antoninnnnnnnn/Sambler/blob/e52e568ec8a602a224b249970e69f749ffa9ab18/crates/rambler-audio/src/long_dictation.rs#L146
[interruption]: https://github.com/Antoninnnnnnnn/Sambler/blob/e52e568ec8a602a224b249970e69f749ffa9ab18/crates/rambler-cli/src/main.rs#L483
[raccord]: https://github.com/Antoninnnnnnnn/Sambler/blob/e52e568ec8a602a224b249970e69f749ffa9ab18/crates/rambler-audio/src/long_dictation.rs#L114
[raccord-cli]: https://github.com/Antoninnnnnnnn/Sambler/blob/e52e568ec8a602a224b249970e69f749ffa9ab18/crates/rambler-cli/src/main.rs#L499
[splice]: ../../crates/rambler-engine/src/splice.rs
[gboard]: https://support.google.com/gboard/answer/17468539?hl=fr
[pixelboard]: https://github.com/Akshayykadam/PixelBoard/releases/tag/1.3.5
[dictee]: ../../README.md
[campagne]: ../evidence/rust-native-real-campaign-20261118.json
[comparaison]: ../evidence/pixelboard-vs-native-real-20261118.json
[langues]: https://github.com/Antoninnnnnnnn/Sambler/blob/e52e568ec8a602a224b249970e69f749ffa9ab18/docs/evidence/rust-native-multilingual-real-20261118.json
[composition]: composition.md
[architecture]: ../../crates/rambler-audio/BUILDING.md
[fiabilite]: https://github.com/Antoninnnnnnnn/Sambler/blob/e52e568ec8a602a224b249970e69f749ffa9ab18/docs/evidence/rust-native-real-reliability-20261118.json
[wer]: ../evidence/natural-expansion-quality.json
[horloge]: https://github.com/Antoninnnnnnnn/Sambler/blob/e52e568ec8a602a224b249970e69f749ffa9ab18/crates/rambler-engine/src/engine.rs#L247
[profil]: https://github.com/Antoninnnnnnnn/Sambler/blob/e52e568ec8a602a224b249970e69f749ffa9ab18/docs/research/interactive-pcm-dictation.md
[cinqminutes]: https://github.com/Antoninnnnnnnn/Sambler/blob/e52e568ec8a602a224b249970e69f749ffa9ab18/docs/evidence/rust-interactive-wall-cadence-windows-20261118.md
[tls]: https://github.com/Antoninnnnnnnn/Sambler/blob/e52e568ec8a602a224b249970e69f749ffa9ab18/crates/rambler-transport/ANDROID_TLS.md
[xiaomi]: https://github.com/Antoninnnnnnnn/Sambler/blob/e52e568ec8a602a224b249970e69f749ffa9ab18/docs/evidence/android-tls-host-app-20261118.md
[pagesandroid]: https://developer.android.com/guide/practices/page-sizes
[ci]: https://github.com/Antoninnnnnnnn/Sambler/actions/runs/44391305891
[cipy]: https://github.com/Antoninnnnnnnn/Sambler/actions/runs/44391305956
[qualite]: https://github.com/Antoninnnnnnnn/Sambler/blob/e52e568ec8a602a224b249970e69f749ffa9ab18/docs/research/quality-annotation-plan.md
