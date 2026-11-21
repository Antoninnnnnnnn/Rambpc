use super::*;
use decode::{AudioError, decode};
use std::{
    fs,
    io::Cursor,
    sync::atomic::{AtomicUsize, Ordering},
};

// Reuse the SDK's loopback peer without widening the controller's test API.
#[allow(clippy::duplicate_mod)]
#[path = "../../../../crates/rambler-engine/tests/common/mod.rs"]
mod fixture;

struct AudioFixture(PathBuf);
impl AudioFixture {
    fn new(ext: &str, contents: &[u8]) -> Self {
        let path = std::env::temp_dir().join(format!(
            "sambler-audio-test-{}.{}",
            uuid::Uuid::new_v4(),
            ext
        ));
        fs::write(&path, contents).unwrap();
        Self(path)
    }
}
impl Drop for AudioFixture {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn wave(tag: u16, rate: u32, bits: u16, channels: u16, data: &[u8]) -> Vec<u8> {
    let mut bytes = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
    bytes.extend((if tag == 0xfffe { 40_u32 } else { 16 }).to_le_bytes());
    let alignment = channels * (bits / 8);
    bytes.extend(tag.to_le_bytes());
    bytes.extend(channels.to_le_bytes());
    bytes.extend(rate.to_le_bytes());
    bytes.extend((rate * u32::from(alignment)).to_le_bytes());
    bytes.extend(alignment.to_le_bytes());
    bytes.extend(bits.to_le_bytes());
    if tag == 0xfffe {
        bytes.extend(22_u16.to_le_bytes());
        bytes.extend(bits.to_le_bytes());
        bytes.extend(0_u32.to_le_bytes());
        bytes.extend(b"\x01\0\0\0\0\0\x10\0\x80\0\0\xaa\0\x38\x9b\x71");
    }
    bytes.extend(b"JUNK\x01\0\0\0\x42\0data"); // Odd metadata chunk and pad.
    bytes.extend((data.len() as u32).to_le_bytes());
    bytes.extend(data);
    let size = (bytes.len() - 8) as u32;
    bytes[4..8].copy_from_slice(&size.to_le_bytes());
    bytes
}

fn decode_all(file: &AudioFixture) -> Result<(Option<u64>, u64, Vec<i16>), AudioError> {
    let mut samples = Vec::new();
    let mut known = None;
    let duration = decode(
        &file.0,
        &CancellationToken::new(),
        |ms| {
            known = ms;
            Ok(())
        },
        |chunk| {
            assert!(chunk.len() <= 3200);
            samples.extend(
                chunk
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|&pair| i16::from_le_bytes(pair)),
            );
            Ok(())
        },
    )?;
    Ok((known, duration, samples))
}

#[test]
fn wav_pcm_and_float_widths_and_multichannel_mix() {
    for (tag, bits, frames) in [
        (1, 8, vec![128, 192, 192, 0, 128, 64]),
        (
            1,
            16,
            [0_i16, 16384, 16384, -32768, 0, -16384]
                .into_iter()
                .flat_map(i16::to_le_bytes)
                .collect(),
        ),
        (
            1,
            24,
            vec![
                0, 0, 0, 0, 0, 0x40, 0, 0, 0x40, 0, 0, 0x80, 0, 0, 0, 0, 0, 0xc0,
            ],
        ),
        (
            0xfffe,
            32,
            [0_i32, 1 << 30, 1 << 30, i32::MIN, 0, -(1 << 30)]
                .into_iter()
                .flat_map(i32::to_le_bytes)
                .collect(),
        ),
        (
            3,
            32,
            [f32::NAN, 0.5, 0.5, -2.0, 0.0, -0.5]
                .into_iter()
                .flat_map(f32::to_le_bytes)
                .collect(),
        ),
    ] {
        let file = AudioFixture::new("wav", &wave(tag, 16_000, bits, 3, &frames));
        assert_eq!(
            decode_all(&file).unwrap().2,
            [10923, -16384],
            "tag={tag}, bits={bits}"
        );
    }
}

#[test]
fn wav_resamples_continuously_across_blocks_and_flushes_tail() {
    for rate in [8_000, 22_050, 44_100, 48_000, 96_000] {
        let len = rate / 2 + 13;
        let data: Vec<_> = (0..len)
            .flat_map(|n| {
                ((32767.0
                    * 0.5
                    * (f64::from(n) * std::f64::consts::TAU * 440.0 / f64::from(rate)).sin())
                    as i16)
                    .to_le_bytes()
            })
            .collect();
        let file = AudioFixture::new("wav", &wave(1, rate, 16, 1, &data));
        let (known, actual, output) = decode_all(&file).unwrap();
        assert_eq!(output.len() as u32, len * 16_000 / rate);
        assert_eq!(known, Some(actual));
        let peak = output[100..output.len() - 100]
            .iter()
            .copied()
            .max()
            .unwrap();
        assert!((15_000..18_000).contains(&peak));
        let jumps = output
            .windows(2)
            .map(|p| (i32::from(p[1]) - i32::from(p[0])).abs())
            .max()
            .unwrap();
        assert!(
            jumps < 6000,
            "decoder block introduced a discontinuity at {rate} Hz: {jumps}"
        );
    }
}

#[test]
fn wav_rejects_non_audio_and_enforces_both_limits() {
    let truncated = AudioFixture::new("wav", &wave(1, 16000, 16, 1, &[0, 1, 2]));
    let not_wav = AudioFixture::new("wav", b"This is not WAVE audio.");
    let long = AudioFixture::new("wav", &wave(1, 1, 8, 1, &vec![128; 7201]));
    let empty = AudioFixture::new("wav", &wave(1, 16000, 16, 1, &[]));
    let big = AudioFixture::new("wav", &[]);
    fs::OpenOptions::new()
        .write(true)
        .open(&big.0)
        .unwrap()
        .set_len(2 * 1024 * 1024 * 1024 + 1)
        .unwrap();
    for (file, error) in [
        (truncated, AudioError::Invalid),
        (not_wav, AudioError::Unsupported),
        (long, AudioError::Duration),
        (empty, AudioError::Empty),
        (big, AudioError::Size),
    ] {
        assert_eq!(decode_all(&file), Err(error));
    }
}

fn ogg_opus() -> Vec<u8> {
    use ogg::writing::{PacketWriteEndInfo as End, PacketWriter};
    let mut encoder =
        opus::Encoder::new(48000, opus::Channels::Stereo, opus::Application::Audio).unwrap();
    let pre_skip = encoder.get_lookahead().unwrap() as u16;
    let mut head = b"OpusHead\x01\x02".to_vec();
    head.extend(pre_skip.to_le_bytes());
    head.extend(48000_u32.to_le_bytes());
    head.extend([0, 0, 0]);
    let mut file = Cursor::new(Vec::new());
    {
        let mut mux = PacketWriter::new(&mut file);
        mux.write_packet(head, 7, End::EndPage, 0).unwrap();
        mux.write_packet(b"OpusTags\0\0\0\0\0\0\0\0".to_vec(), 7, End::EndPage, 0)
            .unwrap();
        let mut packet = [0; 4096];
        for i in 0..5 {
            let frames: Vec<i16> = (0..960)
                .flat_map(|n| {
                    let v = ((f64::from(i * 960 + n) * 0.1).sin() * 12000.0) as i16;
                    [v, v]
                })
                .collect();
            let n = encoder.encode(&frames, &mut packet).unwrap();
            let (end, granule) = if i < 4 {
                (End::NormalPacket, (i + 1) as u64 * 960)
            } else {
                (End::EndStream, 4440)
            };
            mux.write_packet(packet[..n].to_vec(), 7, end, granule)
                .unwrap();
        }
    }
    file.into_inner()
}

#[test]
fn ogg_opus_preskip_tail_and_crc() {
    let bytes = ogg_opus();
    let file = AudioFixture::new("opus", &bytes);
    let (hint, duration, output) = decode_all(&file).unwrap();
    assert_eq!(hint, Some(duration));
    assert_eq!(output.len(), (4440 - 312) / 3);
    assert!(output.iter().any(|sample| sample.unsigned_abs() > 3000));
    let file = AudioFixture::new("ogg", &bytes[..bytes.len() - 1]);
    assert_eq!(decode_all(&file), Err(AudioError::Invalid));
}

#[cfg(windows)]
#[test]
fn media_foundation_reads_local_pcm_source_and_respects_end_of_stream() {
    let data: Vec<_> = [4096_i16, 8192]
        .into_iter()
        .cycle()
        .take(4_800 * 2)
        .flat_map(i16::to_le_bytes)
        .collect();
    let file = AudioFixture::new("wav", &wave(1, 48_000, 16, 2, &data));
    let mut native = super::native::MediaFile::open(&file.0, "wav").unwrap();
    assert_eq!(native.duration_ms, Some(100));
    assert_eq!(native.rate, 48_000);
    let mut mono = Vec::new();
    while native.read(&mut mono).unwrap() {}
    assert_eq!(mono, vec![0.1875; 4800]);
}

#[test]
fn registry_rejects_paths_never_picked_or_dropped() {
    use zeroize::Zeroizing;
    let registry = Arc::new(Transcriptions::new(Arc::new(|_, _| {
        panic!("no note was authorized")
    })));
    let file = AudioFixture::new("WAV", &[]);
    let unauthorized = AudioFixture::new("wav", &[]);
    assert!(registry.allow(Path::new("relative.wav")).is_none());
    assert!(registry.allow(&file.0.with_extension("txt")).is_none());
    assert!(registry.allow(&file.0).is_some());
    let credentials = RamblerAccess::new(
        "localhost:1",
        Zeroizing::new("test".into()),
        Zeroizing::new("test".into()),
        Zeroizing::new("test".into()),
    )
    .unwrap();
    let result = registry.start(
        unauthorized.0.to_str().unwrap().into(),
        credentials,
        TextRules::default(),
        Arc::new(|_| panic!("no event was authorized")),
    );
    assert_eq!(result.unwrap_err().code, "file-not-authorized");
    assert!(registry.list().is_empty());
}

#[tokio::test]
async fn loopback_segments_save_clean_and_partial_notes_without_broadcasting_text() {
    for fail in [false, true] {
        use fixture::{TestServer, bounded, replies};
        use rambler_protocol::{
            RECOGNITION_RESPONSE, end_request,
            wire::{bytes_field as b, string_field as s, varint_field as v},
        };
        let index = Arc::new(AtomicUsize::new(0));
        let server = TestServer::new({
            let index = index.clone();
            move |request| {
                let i = index.fetch_add(1, Ordering::Relaxed);
                async move {
                    let frames = fixture::drain(request.into_inner()).await;
                    assert_eq!(frames.last().unwrap().as_ref(), end_request());
                    let text = if i == 0 {
                        "Sambler est prêt."
                    } else {
                        "Deuxième passage."
                    };
                    let hypothesis = b(3, &s(1, text).unwrap()).unwrap();
                    let recognition = [v(1, 1).unwrap(), b(3, &hypothesis).unwrap()].concat();
                    let mut result = vec![
                        Ok(bytes::Bytes::from(
                            b(RECOGNITION_RESPONSE, &b(1, &recognition).unwrap()).unwrap(),
                        )),
                        Ok(bytes::Bytes::from_static(b"\x08\x01")),
                    ];
                    if fail && i == 1 {
                        result.push(Err(tonic::Status::unavailable("local fixture")));
                    }
                    Ok(replies(futures_util::stream::iter(result)))
                }
            }
        })
        .await;
        let config = rambler_transport::TransportConfig::loopback(server.address).unwrap();
        let mut profile = rambler_audio::InteractivePcmProfile::default();
        profile.segmentation.segment_budget = Duration::from_millis(300);
        profile.segmentation.pause_search = Duration::ZERO;
        let file = AudioFixture::new("wav", &wave(1, 16_000, 16, 1, &vec![0; 16_000]));
        let job = Arc::new(Job::new(
            "test.wav".into(),
            Arc::new(|view| {
                let json = serde_json::to_string(&view).unwrap();
                assert!(!json.contains("Sambler est prêt"));
                assert!(!json.contains("Deuxième passage"));
            }),
        ));
        let note = Arc::new(Mutex::new(None));
        let sink: NoteSink = {
            let note = note.clone();
            Arc::new(move |title, text| {
                *lock(&note) = Some((title.to_owned(), text.to_owned()));
                Ok("local-note".into())
            })
        };
        let rules = TextRules {
            profile: crate::model::RenderProfile::Faithful,
            ..TextRules::default()
        };
        let outcome = bounded(recognize::run_with_profile(
            file.0.clone(),
            &job,
            config,
            profile,
            rules,
            sink,
        ))
        .await;
        let view = job.finish(outcome);
        assert_eq!(index.load(Ordering::Relaxed), 2);
        assert_eq!(view.note_id.as_deref(), Some("local-note"));
        assert_eq!(view.duration_ms, Some(500));
        assert_eq!(
            view.status,
            if fail { Status::Failed } else { Status::Done }
        );
        let (title, body) = lock(&note).take().unwrap();
        assert_eq!(title, "Transcription · test.wav");
        assert_eq!(
            body,
            if fail {
                "Sambler est prêt.\n\n[Passage non transcrit]"
            } else {
                "Sambler est prêt. Deuxième passage."
            }
        );
        bounded(server.close()).await;
    }
}

#[tokio::test]
async fn cancellation_closes_a_stalled_rpc_without_saving() {
    use fixture::{TestServer, bounded, replies};
    use tokio::sync::Notify;
    let started = Arc::new(Notify::new());
    let server = TestServer::new({
        let started = started.clone();
        move |request| {
            let started = started.clone();
            async move {
                // Wait for clean audio EOF, then leave recognition unfinished.
                fixture::drain(request.into_inner()).await;
                started.notify_one();
                Ok(replies(futures_util::stream::pending()))
            }
        }
    })
    .await;
    let file = AudioFixture::new("wav", &wave(1, 16_000, 16, 1, &vec![0; 16_000]));
    let path = file.0.clone();
    let config = rambler_transport::TransportConfig::loopback(server.address).unwrap();
    let job = Arc::new(Job::new("test.wav".into(), Arc::new(|_| {})));
    let writes = Arc::new(AtomicUsize::new(0));
    let worker = tokio::spawn({
        let job = job.clone();
        let writes = writes.clone();
        async move {
            recognize::run_with_profile(
                path,
                &job,
                config,
                rambler_audio::InteractivePcmProfile::default(),
                TextRules::default(),
                Arc::new(move |_, _| {
                    writes.fetch_add(1, Ordering::Relaxed);
                    Ok("unexpected-note".into())
                }),
            )
            .await
        }
    });
    bounded(started.notified()).await;
    job.stop();
    let view = job.finish(bounded(worker).await.unwrap());
    assert_eq!(view.status, Status::Cancelled);
    assert_eq!(writes.load(Ordering::Relaxed), 0);
    assert!(view.note_id.is_none());
    bounded(server.close()).await;
}

/// Manual check against the real service (never run by default):
/// `SAMBLER_REAL_AUDIO=<absolute path> cargo test real_service_file -- --ignored --nocapture`
/// with the `RAMBLER_*` access in the environment.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "calls the real Rambler service"]
async fn real_service_file_transcription_creates_a_note() {
    let Some(path) = std::env::var_os("SAMBLER_REAL_AUDIO") else {
        return;
    };
    let access = crate::rambler::RamblerAccess::from_environment()
        .expect("valid access")
        .expect("RAMBLER_* access in the environment");
    let job = Arc::new(Job::new("real.wav".into(), Arc::new(|_| {})));
    let saved: Arc<std::sync::Mutex<Option<String>>> = Arc::default();
    let sink_saved = saved.clone();
    let outcome = recognize::run(
        PathBuf::from(path),
        &job,
        access,
        TextRules::default(),
        Arc::new(move |title: &str, text: &str| {
            *sink_saved.lock().unwrap() = Some(format!("{title}\n\n{text}"));
            Ok("real-note".into())
        }),
    )
    .await;
    let view = job.finish(outcome);
    println!(
        "status={:?} words={} duration_ms={:?} note={:?} error={:?}",
        view.status, view.words, view.duration_ms, view.note_id, view.error
    );
    println!("note text: {:?}", saved.lock().unwrap());
    assert_eq!(view.status, Status::Done);
    assert!(view.words > 0);
}
