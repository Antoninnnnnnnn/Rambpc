//! Acceptance harness (NOT part of the shipped app): the real controller,
//! the real Rambler service, the real Windows target probe and insertion, and
//! a fresh Windows 11 Notepad window. Only the microphone is replaced by a WAV
//! file played in real time (then silence while the "key" is still held), so
//! the scenario is repeatable without a person speaking.
//!
//! Requires the Rambler access in the environment (RAMBLER_API_KEY,
//! RAMBLER_ANDROID_PACKAGE, RAMBLER_ANDROID_CERT, optional RAMBLER_AUTHORITY).
//!
//!   cargo run --example recette_bloc_notes -- <wav> <fr|en> <scenario>
//!   scenarios: hold | toggle | switch | cancel | all
//!   optional 4th argument `mic`: real microphone, the WAV is played on the speakers.

#[cfg(windows)]
mod recette {
    use sambler_lib::controller::{Controller, ControllerDeps, ControllerInit};
    use sambler_lib::ipc::{Availability, Phase, TakeOutcome, UiState};
    use sambler_lib::model::{Gesture, KeyEdge, Language, MicChoice};
    use sambler_lib::platform::windows::{SendInputInserter, UiaTargetProbe, WinClipboard};
    use sambler_lib::ports::{
        FaultCallback, MicError, MicInfo, MicStream, Microphone, PcmRate, SinkFactory,
    };
    use sambler_lib::rambler::RamblerAccess;
    use sambler_lib::settings::Settings;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;
    use std::time::{Duration, Instant};
    use tokio::sync::watch;

    #[path = "../recette_support/notepad.rs"]
    mod notepad;

    // ------------------------------------------------------------ WAV "microphone"

    struct Wav {
        rate: PcmRate,
        samples: Vec<i16>,
    }

    fn read_wav(path: &str) -> Result<Wav, String> {
        let bytes = std::fs::read(path).map_err(|_| "cannot read WAV")?;
        if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
            return Err("not a RIFF/WAVE file".into());
        }
        let (mut pos, mut fmt, mut data) = (12usize, None, None);
        while pos + 8 <= bytes.len() {
            let id = &bytes[pos..pos + 4];
            let len = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
            let body = &bytes[pos + 8..(pos + 8 + len).min(bytes.len())];
            match id {
                b"fmt " => fmt = Some(body.to_vec()),
                b"data" => data = Some(body.to_vec()),
                _ => {}
            }
            pos += 8 + len + (len & 1);
        }
        let fmt = fmt.ok_or("missing fmt chunk")?;
        let data = data.ok_or("missing data chunk")?;
        let format = u16::from_le_bytes([fmt[0], fmt[1]]);
        let channels = u16::from_le_bytes([fmt[2], fmt[3]]);
        let rate = u32::from_le_bytes(fmt[4..8].try_into().unwrap());
        let bits = u16::from_le_bytes([fmt[14], fmt[15]]);
        if format != 1 || channels != 1 || bits != 16 {
            return Err("expected PCM16 mono".into());
        }
        let rate = match rate {
            16_000 => PcmRate::Hz16000,
            48_000 => PcmRate::Hz48000,
            _ => return Err("expected 16 or 48 kHz".into()),
        };
        let samples = data
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();
        Ok(Wav { rate, samples })
    }

    /// Plays the WAV in 20 ms blocks at real-time pace, then silent blocks until stopped.
    struct WavMicrophone {
        wav: Arc<Wav>,
    }

    struct WavStream {
        stop: Arc<AtomicBool>,
        abort: Arc<AtomicBool>,
        thread: Option<thread::JoinHandle<()>>,
    }

    impl MicStream for WavStream {
        fn stop(mut self: Box<Self>) {
            self.stop.store(true, Ordering::SeqCst);
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
        fn abort(mut self: Box<Self>) {
            self.abort.store(true, Ordering::SeqCst);
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }

    impl Drop for WavStream {
        fn drop(&mut self) {
            self.abort.store(true, Ordering::SeqCst);
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }

    impl Microphone for WavMicrophone {
        fn inputs(&self) -> Result<Vec<MicInfo>, MicError> {
            Ok(vec![MicInfo {
                id: "wav".into(),
                name: "WAV de recette".into(),
                is_default: true,
            }])
        }

        fn open(
            &self,
            _choice: &MicChoice,
            sink_for: SinkFactory,
            _on_fault: FaultCallback,
        ) -> Result<Box<dyn MicStream>, MicError> {
            let wav = self.wav.clone();
            let mut sink = sink_for(wav.rate);
            let (stop, abort) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
            let (stop2, abort2) = (stop.clone(), abort.clone());
            let block = (wav.rate.hz() / 50) as usize;
            let thread = thread::spawn(move || {
                let start = Instant::now();
                let silence = vec![0i16; block];
                let mut index = 0usize;
                loop {
                    if abort2.load(Ordering::SeqCst) {
                        sink.abort();
                        return;
                    }
                    if stop2.load(Ordering::SeqCst) {
                        sink.finish();
                        return;
                    }
                    let offset = index * block;
                    if offset < wav.samples.len() {
                        let end = (offset + block).min(wav.samples.len());
                        sink.push(&wav.samples[offset..end]);
                    } else {
                        sink.push(&silence);
                    }
                    index += 1;
                    let due = start + Duration::from_millis(20 * index as u64);
                    if let Some(wait) = due.checked_duration_since(Instant::now()) {
                        thread::sleep(wait);
                    }
                }
            });
            Ok(Box::new(WavStream {
                stop,
                abort,
                thread: Some(thread),
            }))
        }
    }

    // ------------------------------------------------------------ scenarios

    async fn wait_for(
        rx: &mut watch::Receiver<UiState>,
        timeout: Duration,
        mut pred: impl FnMut(&UiState) -> bool,
    ) -> Result<UiState, String> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            {
                let state = rx.borrow_and_update().clone();
                if pred(&state) {
                    return Ok(state);
                }
            }
            match tokio::time::timeout_at(deadline, rx.changed()).await {
                Ok(Ok(())) => {}
                _ => return Err(format!("timeout; last state {:?}", rx.borrow().clone())),
            }
        }
    }

    fn outcome_of(state: &UiState) -> Option<TakeOutcome> {
        (state.phase == Phase::Idle).then(|| state.last_outcome.clone()).flatten()
    }

    pub async fn main(args: Vec<String>) -> Result<(), String> {
        let wav_path = args.get(1).ok_or("usage: <wav> <fr|en> <scenario>")?;
        let language = match args.get(2).map(String::as_str) {
            Some("en") => Language::English,
            Some("auto") => Language::Auto,
            _ => Language::French,
        };
        let scenario = args.get(3).cloned().unwrap_or_else(|| "all".into());
        let wav = Arc::new(read_wav(wav_path)?);
        let wav_ms = wav.samples.len() as u64 * 1000 / wav.rate.hz() as u64;
        // "mic": real WASAPI microphone; the WAV is played on the speakers.
        let real_mic = args.get(4).is_some_and(|a| a == "mic");
        let speaker_wav = real_mic.then(|| wav_path.clone());
        let access = RamblerAccess::from_environment()
            .map_err(|e| format!("access: {e}"))?
            .ok_or("missing RAMBLER_* access in the environment")?;

        let scenarios: Vec<&str> = if scenario == "all" {
            vec!["hold", "toggle", "switch", "cancel"]
        } else {
            vec![scenario.as_str()]
        };
        let mut failures = 0;
        for name in scenarios {
            let mut settings = Settings::default();
            settings.language = language;
            settings.gesture = if name == "toggle" { Gesture::Toggle } else { Gesture::Hold };
            let deps = ControllerDeps {
                microphone: if real_mic {
                    Arc::new(sambler_lib::platform::windows::WasapiMicrophone::new())
                } else {
                    Arc::new(WavMicrophone { wav: wav.clone() })
                },
                target: Arc::new(UiaTargetProbe::new()),
                inserter: Arc::new(SendInputInserter::new()),
                clipboard: Arc::new(WinClipboard::new()),
            };
            let controller = Controller::spawn(
                deps,
                ControllerInit {
                    settings,
                    consent: true,
                    access: Some(access.clone()),
                },
            );
            let mut rx = controller.state();
            wait_for(&mut rx, Duration::from_secs(15), |s| {
                matches!(s.availability, Availability::Ready)
            })
            .await
            .map_err(|e| format!("engine not ready: {e}"))?;

            let pad = notepad::Fixture::open().map_err(|e| format!("notepad: {e}"))?;
            let result =
                run_scenario(name, &controller, &mut rx, &pad, wav_ms, speaker_wav.as_deref()).await;
            let cleanup = pad.close();
            controller.shutdown().await;
            match result {
                Ok(report) => println!("[OK]   {name}: {report}; cleanup={cleanup}"),
                Err(error) => {
                    failures += 1;
                    println!("[FAIL] {name}: {error}; cleanup={cleanup}");
                }
            }
        }
        if failures > 0 {
            Err(format!("{failures} scenario(s) failed"))
        } else {
            Ok(())
        }
    }

    async fn run_scenario(
        name: &str,
        controller: &sambler_lib::controller::ControllerHandle,
        rx: &mut watch::Receiver<UiState>,
        pad: &notepad::Fixture,
        wav_ms: u64,
        speaker_wav: Option<&str>,
    ) -> Result<String, String> {
        pad.focus()?;
        tokio::time::sleep(Duration::from_millis(300)).await;
        controller.hotkey(KeyEdge::Pressed);
        if name == "toggle" {
            tokio::time::sleep(Duration::from_millis(120)).await;
            controller.hotkey(KeyEdge::Released); // ignored in toggle mode
        }
        let listening = wait_for(rx, Duration::from_secs(5), |s| {
            s.phase == Phase::Listening || outcome_of(s).is_some()
        })
        .await?;
        if listening.phase != Phase::Listening {
            return Err(format!("did not listen: {:?}", listening.last_outcome));
        }
        let take = listening.take.ok_or("no take id")?;

        if name == "cancel" {
            tokio::time::sleep(Duration::from_millis(wav_ms / 2)).await;
            controller.cancel(None);
            let done = wait_for(rx, Duration::from_secs(5), |s| outcome_of(s).is_some()).await?;
            tokio::time::sleep(Duration::from_secs(4)).await; // no late write allowed
            let text = pad.contents()?;
            return match (outcome_of(&done), text.is_empty(), controller.draft().await) {
                (Some(TakeOutcome::Cancelled), true, None) => {
                    Ok("cancelled, field untouched, no draft".into())
                }
                other => Err(format!("unexpected after cancel: {:?}", other)),
            };
        }

        match speaker_wav {
            Some(path) => {
                // Play the phrase on the speakers and wait until it ends.
                let script = format!("(New-Object Media.SoundPlayer '{path}').PlaySync()");
                let _ = tokio::task::spawn_blocking(move || {
                    std::process::Command::new("powershell")
                        .args(["-NoProfile", "-Command", &script])
                        .status()
                })
                .await;
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            None => tokio::time::sleep(Duration::from_millis(wav_ms + 400)).await,
        }
        let stop_at = Instant::now();
        match name {
            "toggle" => controller.hotkey(KeyEdge::Pressed),
            _ => controller.hotkey(KeyEdge::Released),
        }
        // A second stop must be a no-op.
        controller.finish(take);

        if name == "switch" {
            // Move to another window while Rambler finalizes.
            pad.open_second()?;
        }

        // Measure stop gesture -> text visible in the field.
        let mut visible_ms = None;
        if name != "switch" {
            let deadline = Instant::now() + Duration::from_secs(10);
            while Instant::now() < deadline {
                if !pad.contents()?.is_empty() {
                    visible_ms = Some(stop_at.elapsed().as_millis());
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
        let done = wait_for(rx, Duration::from_secs(15), |s| outcome_of(s).is_some()).await?;
        tokio::time::sleep(Duration::from_millis(500)).await;
        let text = pad.contents()?;
        let draft = controller.draft().await;
        let outcome = outcome_of(&done);
        match name {
            "switch" => {
                let full = draft.and_then(|d| d.full_text).unwrap_or_default();
                let second = pad.second_contents()?;
                if matches!(outcome, Some(TakeOutcome::KeptInSambler { .. }))
                    && text.is_empty()
                    && second.is_empty()
                    && !full.is_empty()
                {
                    Ok(format!("kept in Sambler: {full:?}; both windows untouched"))
                } else {
                    Err(format!(
                        "outcome={outcome:?} first={text:?} second={second:?} draft={full:?}"
                    ))
                }
            }
            _ => {
                // Exactly the rendered draft text, once, and nothing else.
                let expected = controller.draft().await.and_then(|d| d.full_text);
                let once = expected.as_deref().is_some_and(|e| !e.is_empty() && text == e);
                tokio::time::sleep(Duration::from_millis(1500)).await;
                let later = pad.contents()?;
                let drafted = controller.draft().await.and_then(|d| d.full_text);
                if later != text || drafted.as_deref() != Some(text.as_str()) {
                    println!("  note: field later={later:?}; draft={drafted:?}");
                }
                if matches!(outcome, Some(TakeOutcome::Inserted)) && once {
                    Ok(format!(
                        "inserted {text:?}; visible {} ms after the stop gesture",
                        visible_ms.map(|m| m.to_string()).unwrap_or_else(|| "?".into())
                    ))
                } else {
                    Err(format!("outcome={outcome:?} field={text:?} expected={expected:?}"))
                }
            }
        }
    }
}

#[cfg(windows)]
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("runtime");
    if let Err(error) = runtime.block_on(recette::main(args)) {
        eprintln!("recette: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("This harness requires Windows.");
}
