//! Paced, single-input qualification of the long PCM collector (no mic, replay,
//! editor context, or default service). Use synthetic audio; JSON omits text
//! unless explicitly requested. A completed RPC may still withhold strict text.
use clap::Parser;
use rambler_audio::{
    LongDictationObservation, LongDictationOptions, WavAudio, WavConfig,
    collect_long_pcm_dictation_with_observer,
};
use rambler_engine::{CollectorLimits, Engine, SessionOptions};
use rambler_transport::{EnvironmentCredentials, MetadataProvider, TransportConfig};
use serde_json::{Value, json};
use std::{
    io, net::SocketAddr, ops::Range, path::PathBuf, process::ExitCode, sync::Arc, time::Duration,
};
use tokio::time::Instant;

#[derive(Parser)]
struct Args {
    /// Explicit TLS authority for an authorized deployment.
    #[arg(
        long,
        required_unless_present = "loopback",
        conflicts_with = "loopback"
    )]
    authority: Option<String>,
    /// Numeric loopback fixture; never reads credentials.
    #[arg(long)]
    loopback: Option<SocketAddr>,
    #[arg(long, default_value = "en-US")]
    lang: String,
    #[arg(long, default_value_t = 180, value_parser = clap::value_parser!(u64).range(15..=240))]
    segment_seconds: u64,
    /// Include strict text in stdout; do not redirect it to a public log.
    #[arg(long)]
    include_text: bool,
    wav: PathBuf,
}

fn coverage(ranges: &[(usize, Range<u64>)], expected: u64) -> bool {
    let mut end = 0;
    for (index, (reported_index, range)) in ranges.iter().enumerate() {
        if index != *reported_index || range.start != end || range.end <= range.start {
            return false;
        }
        end = range.end;
    }
    end == expected
}

async fn qualify(args: Args) -> Result<(bool, Value), Box<dyn std::error::Error>> {
    let wav = WavAudio::open(
        &args.wav,
        WavConfig {
            realtime: true,
            ..WavConfig::default()
        },
    )
    .await?;
    let samples = wav.frame_count;
    if samples == 0 || samples > 900 * 16_000 {
        return Err("expected nonempty 16 kHz mono PCM WAV, at most 15 minutes".into());
    }
    let (transport, credentials): (_, Option<Arc<dyn MetadataProvider>>) =
        match (args.authority, args.loopback) {
            (Some(authority), None) => (
                TransportConfig::tls(&authority)?,
                Some(Arc::new(EnvironmentCredentials.required_snapshot()?)),
            ),
            (None, Some(address)) => (TransportConfig::loopback(address)?, None),
            _ => return Err("choose exactly one transport".into()),
        };
    let mut session = SessionOptions {
        timeout: Duration::from_secs(args.segment_seconds + 30),
        // Permit measuring a result later than the interactive 5 s goal.
        finalize_timeout: Some(Duration::from_secs(15)),
        credentials,
        ..SessionOptions::default()
    };
    session.config.language = args.lang;
    let engine = Engine::connect_ready(transport).await?;
    let start = Instant::now();
    let mut ranges = Vec::new();
    let collected = collect_long_pcm_dictation_with_observer(
        &engine,
        wav,
        session,
        CollectorLimits::default(),
        LongDictationOptions {
            segment_budget: Duration::from_secs(args.segment_seconds),
            pause_search: Duration::from_secs(4),
        },
        &mut |event| {
            if let LongDictationObservation::SegmentCompleted {
                segment_index,
                sample_range,
                ..
            } = event
            {
                ranges.push((segment_index, sample_range));
            }
        },
    )
    .await;
    let collection_seconds = start.elapsed().as_secs_f64();
    let shutdown = engine.close().await.err();
    let (result, failure) = match collected {
        Ok(result) => (result, None),
        Err(error) => (error.completed, Some(error.cause.to_string())),
    };
    let exact_coverage = ranges.len() == result.segments.len() && coverage(&ranges, samples);
    let mut strict_success = true;
    let segments: Vec<_> = result
        .segments
        .iter()
        .enumerate()
        .map(|(i, segment)| {
            let range = ranges.get(i).map(|(_, range)| [range.start, range.end]);
            let eof = segment
                .metrics
                .as_ref()
                .and_then(|metric| metric.eof_seconds);
            let proxy = range
                .zip(eof)
                .map(|([start, end], eof)| eof - (end - start) as f64 / 16_000.0);
            let projected = segment.text();
            strict_success &= projected.is_ok();
            let mut row = json!({
                "index": i,
                "sample_range": range,
                "final_updates": segment.final_updates.len(),
                "responses_received": segment.metrics.as_ref().map(|m| m.responses_received),
                "eof_minus_segment_audio_seconds": proxy,
                "strict_error": projected.as_ref().err().map(ToString::to_string),
            });
            if args.include_text
                && let Ok(text) = projected
            {
                row["text"] = json!(text);
            }
            row
        })
        .collect();
    let ok = failure.is_none() && shutdown.is_none() && exact_coverage && strict_success;
    Ok((
        ok,
        json!({
            "schema": "rambler-long-wav-qualification-v1",
            "input_samples_16khz": samples,
            "collection_wall_seconds": collection_seconds,
            "exact_sample_coverage": exact_coverage,
            "failure": failure,
            "shutdown_error": shutdown.map(|e| e.to_string()),
            "segments": segments,
        }),
    ))
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    match qualify(Args::parse()).await {
        Ok((ok, report)) => {
            if serde_json::to_writer(io::stdout().lock(), &report).is_err() || !ok {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(error) => {
            eprintln!("Qualification preflight failed: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
#[path = "../../rambler-engine/tests/common/mod.rs"]
mod common;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::{TestServer, bounded, drain, replies};
    use bytes::Bytes;
    use rambler_protocol::{
        RECOGNITION_RESPONSE, end_request,
        wire::{bytes_field as b, string_field as s, varint_field as v},
    };

    #[test]
    fn coverage_requires_order_nonempty_ranges_and_all_samples() {
        assert!(coverage(&[(0, 0..11), (1, 11..20)], 20));
        assert!(!coverage(&[(0, 0..11), (1, 12..20)], 20));
        assert!(!coverage(&[(0, 0..11), (1, 10..20)], 20));
        assert!(!coverage(&[(0, 0..11), (2, 11..20)], 20));
        assert!(!coverage(&[(0, 0..0), (1, 0..20)], 20));
        assert!(!coverage(&[(0, 0..11)], 20));
    }

    #[tokio::test]
    async fn cli_report_on_owned_loopback_has_exact_coverage_without_text_by_default() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("synthetic.wav");
        let mut wav = b"RIFF\x26\0\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0".to_vec();
        wav.extend(16_000u32.to_le_bytes());
        wav.extend(32_000u32.to_le_bytes());
        wav.extend(b"\x02\0\x10\0data\x02\0\0\0\x01\0");
        std::fs::write(&path, wav).unwrap();

        let server = TestServer::new(|request| async {
            assert!(!request.metadata().contains_key("x-goog-api-key"));
            let uploaded = drain(request.into_inner()).await;
            assert!(uploaded.len() >= 4);
            assert_eq!(uploaded.last().unwrap().as_ref(), end_request().as_slice());
            let hypothesis = b(3, &s(1, "Owned test text.").unwrap()).unwrap();
            let state_and_result = [v(1, 1).unwrap(), b(3, &hypothesis).unwrap()].concat();
            let recognition = b(1, &state_and_result).unwrap();
            let response = Bytes::from(b(RECOGNITION_RESPONSE, &recognition).unwrap());
            Ok(replies(futures_util::stream::iter([
                Ok(response),
                Ok(Bytes::from_static(b"\x08\x01")),
            ])))
        })
        .await;
        let address = server.address.to_string();
        let args = Args::try_parse_from([
            "long-wav-qualification".as_ref(),
            "--loopback".as_ref(),
            address.as_ref(),
            path.as_os_str(),
        ])
        .unwrap();
        let (ok, value) = bounded(qualify(args)).await.unwrap();
        assert!(ok, "{value}");
        assert_eq!(value["exact_sample_coverage"], true);
        assert_eq!(value["segments"][0]["sample_range"], json!([0, 1]));
        assert_eq!(value["segments"][0]["final_updates"], 1);
        assert!(value["segments"][0].get("text").is_none());
        assert_eq!(server.accepted_connections(), 1);
        bounded(server.close()).await;
    }
}
