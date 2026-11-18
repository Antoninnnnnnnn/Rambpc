#[path = "../../../rambler-audio/tests/common/mod.rs"]
mod audio_common;
#[path = "../../../rambler-engine/tests/common/mod.rs"]
mod common;
#[allow(dead_code)]
#[path = "../qualification_support/fixtures.rs"]
mod fixtures;
mod meter;

use bytes::Bytes;
use meter::{Measured, Stage};
use rambler_audio::{EncoderConfig, FRAME_SAMPLES, Pcm16Opus, Pcm48kTo16k, PcmSender, pcm_channel};
use rambler_engine::{
    AudioSource, CollectorLimits, ContextInput, Engine, SessionOptions, SourceError, SourceFuture,
    collect_session,
};
use rambler_protocol::{
    AUDIO_DATA, AUDIO_INFO, ProtocolConfig, context_request, end_request, initial_request,
    wire::{Field, Value, parse_fields},
};
use rambler_transport::{GrpcTransport, TransportConfig};
use serde_json::{Value as Json, json};
use sha2::{Digest, Sha256};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::oneshot;
use tonic::Status;

const SAMPLES48: usize = 180 * 48_000 + 7;
const CHUNK: usize = 1921; // Odd boundaries exercise PCM byte carry.
const SAMPLES16: usize = SAMPLES48.div_ceil(3);
const PAGES: usize = SAMPLES16.div_ceil(FRAME_SAMPLES) + 2;
const REQUESTS: usize = PAGES + 3; // INIT, Ogg pages, late context, protocol END.
const MAX_MESSAGE: usize = 2048;
const DEADLINE: Duration = Duration::from_secs(30);

// Synthetic, integer-only triangle harmonics with a changing envelope, quiet
// intervals and deterministic low-amplitude dither. Not representative speech.
fn synthetic(samples: usize) -> Bytes {
    let mut out = Vec::with_capacity(samples * 2);
    let mut seed = 0x1234_5678_u32;
    for i in 0..samples {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let triangle = |period: usize| {
            let phase = i % period;
            ((4 * phase.min(period - phase)) as i32 - period as i32) * 6000 / period as i32
        };
        let envelope = [8, 7, 3, 0, 4, 8, 0, 5][i / 9600 % 8];
        let sample =
            ((triangle(233) + triangle(109) / 3 + (seed >> 24) as i32 - 128) * envelope / 8) as i16;
        out.extend(sample.to_le_bytes());
    }
    out.into()
}

struct Ready {
    pcm: Bytes,
    offset: usize,
}
impl AudioSource for Ready {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
        Box::pin(async move {
            if self.offset == self.pcm.len() {
                return Ok(None);
            }
            let end = (self.offset + CHUNK).min(self.pcm.len());
            let chunk = self.pcm.slice(self.offset..end);
            self.offset = end;
            Ok(Some(chunk))
        })
    }
    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        assert_eq!(self.offset, self.pcm.len());
        Box::pin(async { Ok(()) })
    }
}

async fn produce(pcm: Bytes, mut sender: PcmSender) -> usize {
    let mut sent = 0;
    for start in (0..pcm.len()).step_by(CHUNK) {
        sender
            .reserve()
            .await
            .unwrap()
            .send(pcm.slice(start..(start + CHUNK).min(pcm.len())))
            .unwrap();
        sent += 1;
    }
    sender.finish().unwrap();
    sent
}

fn audio<S: AudioSource + 'static>(source: S, metered: bool) -> Box<dyn AudioSource> {
    let config = EncoderConfig::default();
    if metered {
        Box::new(Measured(
            Stage::OpusOgg,
            Pcm16Opus::new(
                Measured(
                    Stage::Decimator,
                    Pcm48kTo16k::new(Measured(Stage::Ingress, source)),
                ),
                config,
            ),
        ))
    } else {
        Box::new(Pcm16Opus::new(Pcm48kTo16k::new(source), config))
    }
}

fn proof(
    upload: &[Bytes],
    config: &ProtocolConfig,
    fixture: &fixtures::Case,
    emit_ogg: bool,
) -> Json {
    assert_eq!(upload.len(), REQUESTS);
    assert_eq!(upload[0], initial_request(config, 0).unwrap());
    assert_eq!(
        upload[REQUESTS - 2],
        context_request(&fixture.context).unwrap()
    );
    assert_eq!(upload[REQUESTS - 1].as_ref(), end_request());
    let mut ogg = Vec::new();
    let mut previous_elapsed = 0;
    for payload in &upload[1..REQUESTS - 2] {
        let fields = parse_fields(payload).unwrap();
        let [
            Field {
                number: AUDIO_INFO,
                value: Value::Bytes(info),
                ..
            },
            Field {
                number: AUDIO_DATA,
                value: Value::Bytes(data),
                ..
            },
        ] = fields.as_slice()
        else {
            panic!("unexpected audio protobuf shape");
        };
        let info = parse_fields(info).unwrap();
        let data = parse_fields(data).unwrap();
        let (
            [
                Field {
                    number: 9,
                    value: Value::Varint(elapsed),
                    ..
                },
            ],
            [
                Field {
                    number: 1,
                    value: Value::Bytes(chunk),
                    ..
                },
            ],
        ) = (info.as_slice(), data.as_slice())
        else {
            panic!("unexpected inner audio protobuf shape");
        };
        assert!(*elapsed >= previous_elapsed);
        previous_elapsed = *elapsed;
        ogg.extend_from_slice(chunk);
    }
    let pages = audio_common::pages(&ogg);
    let packets = audio_common::packets(&ogg); // Independent Ogg framing/CRC reader.
    assert_eq!(pages.len(), PAGES);
    assert_eq!(packets.len(), PAGES);
    assert!(packets[0].starts_with(b"OpusHead"));
    assert!(packets[1].starts_with(b"OpusTags"));
    let pre_skip = u16::from_le_bytes(packets[0][10..12].try_into().unwrap());
    let final_granule = pages.last().unwrap().granule;
    assert_eq!(final_granule - u64::from(pre_skip), SAMPLES16 as u64 * 3);
    for (i, page) in pages.iter().enumerate() {
        assert_eq!(page.sequence, i as u32);
        assert_eq!(page.serial, pages[0].serial);
        assert_eq!(
            page.flags,
            if i == 0 {
                2
            } else if i + 1 == PAGES {
                4
            } else {
                0
            }
        );
    }
    let mut packet_hash = Sha256::new();
    for packet in &packets[2..] {
        packet_hash.update((packet.len() as u32).to_le_bytes());
        packet_hash.update(packet);
    }
    if emit_ogg {
        println!("native_cpu_ogg {}", hex::encode(&ogg));
    }
    json!({
        "pages": pages.len(), "ogg_bytes": ogg.len(), "serial": pages[0].serial,
        "ogg_sha256": format!("{:x}", Sha256::digest(&ogg)), // No normalization.
        "opus_packets": packets.len() - 2, "opus_sha256": format!("{:x}", packet_hash.finalize()),
        "pre_skip": pre_skip, "final_granule": final_granule,
    })
}

async fn run_session(pcm: Bytes, channel: bool, metered: bool, emit_ogg: bool) -> Json {
    let fixture = fixtures::cases().remove(0);
    let config = ProtocolConfig {
        application_id: "synthetic-local-cpu-profile".to_string(),
        ..Default::default()
    };
    let (receipt, received) = oneshot::channel();
    let receipt = Arc::new(Mutex::new(Some(receipt)));
    let replies = fixture.replies.clone();
    let server = common::TestServer::new(move |request| {
        let receipt = Arc::clone(&receipt);
        let replies = replies.clone();
        async move {
            assert!(
                request
                    .remote_addr()
                    .is_some_and(|addr| addr.ip().is_loopback())
            );
            let receipt = receipt.lock().unwrap().take().unwrap();
            let stream = futures_util::stream::once(async move {
                let mut input = request.into_inner();
                let mut upload = Vec::with_capacity(REQUESTS);
                while let Some(payload) = input.message().await? {
                    if upload.len() == REQUESTS || payload.len() > MAX_MESSAGE {
                        return Err(Status::resource_exhausted("synthetic profile upload bound"));
                    }
                    upload.push(payload);
                }
                receipt.send(upload).unwrap();
                Ok(futures_util::stream::iter(replies.into_iter().map(Ok)))
            });
            use futures_util::TryStreamExt;
            Ok(common::replies(stream.try_flatten()))
        }
    })
    .await;
    let transport = GrpcTransport::connect(
        TransportConfig::loopback(server.address)
            .unwrap()
            .with_max_message_bytes(MAX_MESSAGE)
            .unwrap(),
    )
    .await
    .unwrap();
    let engine = Engine::new(transport.clone());
    let options = SessionOptions {
        config: config.clone(),
        context: ContextInput::Static(fixture.context.clone()),
        timeout: DEADLINE,
        ..Default::default()
    };
    let (source, sender) = if channel {
        let (sender, input) = pcm_channel();
        (audio(input, metered), Some(sender))
    } else {
        (
            audio(
                Ready {
                    pcm: pcm.clone(),
                    offset: 0,
                },
                metered,
            ),
            None,
        )
    };
    let chunks = pcm.len().div_ceil(CHUNK);
    let cpu_start = meter::process_cpu_100ns();
    let wall_start = Instant::now();
    let cycle_start = meter::cycles();
    if metered {
        meter::start(cycle_start);
    }
    let session = engine.start(source, options).unwrap();
    let producer = sender.map(|sender| {
        let future = produce(pcm, sender);
        if metered {
            tokio::spawn(meter::measured(Stage::Ingress, future))
        } else {
            tokio::spawn(future)
        }
    });
    let result = collect_session(session, CollectorLimits::default())
        .await
        .unwrap();
    if let Some(producer) = producer {
        assert_eq!(producer.await.unwrap(), chunks);
    }
    let upload = received.await.unwrap();
    let cycle_end = meter::cycles();
    let wall_ns = wall_start.elapsed().as_nanos() as u64;
    let cpu_100ns = meter::process_cpu_100ns() - cpu_start;
    let thread_cycles = cycle_end - cycle_start;
    let breakdown = metered.then(|| {
        let (cycles, entries) = meter::finish(cycle_end);
        assert_eq!(cycles.iter().sum::<u64>(), thread_cycles);
        assert!(cycles.iter().all(|&value| value > 0));
        json!({"session":cycles[0], "decimator":cycles[1], "opus_ogg":cycles[2], "ingress":cycles[3],
            "poll_and_invocation_entries": {"decimator":entries[1], "opus_ogg":entries[2], "ingress":entries[3]}})
    });
    engine.close().await.unwrap();
    transport.close().await.unwrap();
    server.close().await;

    assert_eq!(result.context.as_ref(), Some(&fixture.context));
    assert_eq!(result.final_updates.len(), 1);
    assert_eq!(result.final_updates[0].raw, fixture.replies[0]);
    let metrics = result.metrics.unwrap();
    assert!(metrics.eof_seconds.is_some());
    assert!(metrics.audio.eof && metrics.audio.cleanup_finished && metrics.audio.cleanup_succeeded);
    assert_eq!(metrics.responses_received, fixture.replies.len() as u64);
    assert_eq!(metrics.requests_produced, REQUESTS as u64);
    assert_eq!(
        metrics.request_protobuf_bytes,
        upload.iter().map(|p| p.len() as u64).sum::<u64>()
    );
    assert_eq!(metrics.audio.chunks, PAGES as u64);
    let identity = proof(&upload, &config, &fixture, emit_ogg);
    assert_eq!(Some(metrics.audio.bytes), identity["ogg_bytes"].as_u64());
    json!({"wall_ns":wall_ns, "cpu_100ns":cpu_100ns, "thread_cycles":thread_cycles,
        "stage_cycles":breakdown, "identity":identity, "requests":metrics.requests_produced,
        "responses":metrics.responses_received})
}

#[allow(clippy::assertions_on_constants)]
pub fn main() {
    assert!(!cfg!(debug_assertions), "profile requires release");
    let args: Vec<_> = std::env::args().skip(1).collect();
    let (channel, metered, emit_ogg) = match args.as_slice() {
        [mode, measurement, rest @ ..]
            if matches!(mode.as_str(), "ready" | "channel")
                && matches!(measurement.as_str(), "control" | "metered")
                && (rest.is_empty() || (rest == ["--emit-ogg"] && measurement == "control")) =>
        {
            (
                mode == "channel",
                measurement == "metered",
                !rest.is_empty(),
            )
        }
        _ => {
            eprintln!("usage: native_cpu_profile (ready|channel) (control [--emit-ogg]|metered)");
            std::process::exit(2);
        }
    };
    let pcm = synthetic(SAMPLES48);
    let pcm_hash = format!("{:x}", Sha256::digest(&pcm));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (warmup, mut sample) = runtime.block_on(async {
        let warmup =
            tokio::time::timeout(DEADLINE, run_session(pcm.clone(), channel, false, false))
                .await
                .unwrap();
        let sample = tokio::time::timeout(DEADLINE, run_session(pcm, channel, metered, emit_ogg))
            .await
            .unwrap();
        (warmup, sample)
    });
    assert_eq!(warmup["identity"]["serial"], 1);
    assert_eq!(sample["identity"]["serial"], 2);
    assert_eq!(
        sample["identity"]["opus_sha256"],
        warmup["identity"]["opus_sha256"]
    );
    sample["mode"] = json!(args[0]);
    sample["measurement"] = json!(args[1]);
    sample["input_sha256"] = json!(pcm_hash);
    sample["samples48"] = json!(SAMPLES48);
    sample["chunk_bytes"] = json!(CHUNK);
    sample["libopus"] = json!(rambler_audio::opus_version());
    println!("native_cpu_profile {sample}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthetic_has_exact_size_and_repeatable_content() {
        let pcm = synthetic(12_000);
        assert_eq!(pcm.len(), 24_000);
        assert_eq!(pcm, synthetic(12_000));
        assert!(pcm.iter().any(|byte| *byte != 0));
    }
}
