//! Offline native SDK qualification: fixed synthetic PCM and S3 fixtures, one
//! owned ephemeral IPv4 localhost peer, and one shared Engine/transport. This
//! is neither a live-provider probe nor an editor integration.

#[path = "../../rambler-audio/tests/common/mod.rs"]
mod audio_common;
#[path = "../../rambler-engine/tests/common/mod.rs"]
mod common;
#[cfg(test)]
#[path = "qualification_runner/edit_acceptance.rs"]
mod edit_acceptance;
#[path = "qualification_support/fixtures.rs"]
mod fixtures;
#[path = "qualification_runner/peer.rs"]
mod peer;

use common::TestServer;
use fixtures::Case;
use peer::Peer;
use rambler_audio::{EncoderConfig, FRAME_BYTES, Pcm16Opus};
use rambler_engine::{
    AudioSource, ContextInput, DictationResult, Engine, EngineEvent, FallbackReason,
    PartialTranscript, Presentation, PresentationContent, ProtocolConfig, RecognitionCompletion,
    RenderPolicy, S3Response, SessionOptions, TextProvenance, replacement_proposal,
};
use rambler_protocol::{context_request, initial_request};
use rambler_transport::{GrpcTransport, TransportConfig};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    ffi::OsString,
    io::{self, Write},
    panic::{AssertUnwindSafe, catch_unwind},
    process::ExitCode,
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};
use tokio::time::timeout;

const CASE_IDS: [&str; 4] = ["literal", "rewrite", "selection", "preview"];
const SESSION_DEADLINE: Duration = Duration::from_secs(5);
const MAX_REPEATS: usize = 10;
const MAX_PCM_SAMPLES: usize = 16_000;
const MAX_FIXTURE_MESSAGE_BYTES: usize = 16 * 1024;
const MAX_PREVIEWS: usize = 8;
const FRAGMENTS: [usize; 5] = [1, 317, 641, 3, 1009];
const S3_END: &[u8] = b"\x08\x01";

struct Failed;
type Check<T> = Result<T, Failed>;

fn check(condition: bool) -> Check<()> {
    condition.then_some(()).ok_or(Failed)
}

fn guarded<T>(run: impl FnOnce() -> T) -> Check<T> {
    catch_unwind(AssertUnwindSafe(run)).map_err(|_| Failed)
}

fn parse_repeats(args: impl IntoIterator<Item = OsString>) -> Check<usize> {
    let mut args = args.into_iter();
    let Some(flag) = args.next() else {
        return Ok(1);
    };
    check(flag == "--repeats")?;
    let value = args.next().ok_or(Failed)?;
    let digits = value.to_str().ok_or(Failed)?;
    check(
        (1..=2).contains(&digits.len())
            && !digits.starts_with('0')
            && digits.bytes().all(|byte| byte.is_ascii_digit())
            && args.next().is_none(),
    )?;
    let repeats = digits.parse().map_err(|_| Failed)?;
    check((1..=MAX_REPEATS).contains(&repeats))?;
    Ok(repeats)
}

struct PreparedCase {
    fixture: Case,
    context: Vec<u8>,
    pcm_sha256: String,
    codec_packets: Vec<Vec<u8>>,
    first_media_end: usize,
}

impl PreparedCase {
    async fn new(fixture: Case) -> Check<Self> {
        let sample_count = fixture.pcm.len() / 2;
        let previews = fixture.expected.partials.len();
        check(
            fixture.pcm.len().is_multiple_of(2)
                && (1..=MAX_PCM_SAMPLES).contains(&sample_count)
                && (previews > 0) == (fixture.id == "preview")
                && (previews == 0 || fixture.pcm.len() > FRAME_BYTES)
                && previews <= MAX_PREVIEWS
                && fixture.replies.len() == previews + 2
                && fixture
                    .replies
                    .last()
                    .is_some_and(|last| last.as_ref() == S3_END)
                && fixture
                    .replies
                    .iter()
                    .all(|reply| !reply.is_empty() && reply.len() <= MAX_FIXTURE_MESSAGE_BYTES)
                && fixture.expected.selection_utf16.is_some() == (fixture.id == "selection"),
        )?;
        let context = context_request(&fixture.context).map_err(|_| Failed)?;
        check(context.len() <= MAX_FIXTURE_MESSAGE_BYTES)?;
        // Separate encoder and source, taking the exact same raw PCM in one
        // contiguous chunk. Never compare Ogg bytes (stream serials differ).
        let mut reference = Pcm16Opus::new(
            audio_common::Source::new([fixture.pcm.clone()]),
            EncoderConfig::default(),
        );
        let ogg = audio_common::collect(&mut reference)
            .await
            .map_err(|_| Failed)?;
        reference.close().await.map_err(|_| Failed)?;
        let (codec_packets, first_media_end) = guarded(|| {
            let mut packets = audio_common::packets(&ogg);
            let pages = audio_common::pages(&ogg);
            // The fixed 20 ms page profile places the first frame just after
            // the two header pages. Used only to gate the live preview prefix.
            let prefix = pages[..3]
                .iter()
                .map(|page| 27 + page.lacing.len() + page.body.len())
                .sum();
            (packets.split_off(2), prefix)
        })?;
        check(!codec_packets.is_empty())?;
        let pcm_sha256 = hex::encode(Sha256::digest(&fixture.pcm));
        Ok(Self {
            fixture,
            context,
            pcm_sha256,
            codec_packets,
            first_media_end,
        })
    }
}

struct Invocation {
    case_index: usize,
    repeat: usize,
    config: ProtocolConfig,
    initial: Vec<u8>,
}

async fn prepare_cases(fixtures: Vec<Case>, repeats: usize) -> Check<Peer> {
    let mut cases = Vec::with_capacity(fixtures.len());
    for fixture in fixtures {
        cases.push(PreparedCase::new(fixture).await?);
    }
    let mut invocations = Vec::with_capacity(cases.len() * repeats);
    for repeat in 0..repeats {
        for (case_index, case) in cases.iter().enumerate() {
            // Give every invocation a distinct, verified init to catch a replay
            // even when two repeats use byte-identical PCM and response fixtures.
            let config = ProtocolConfig {
                application_id: format!("synthetic-qualification-{}-{repeat}", case.fixture.id),
                ..Default::default()
            };
            let initial = initial_request(&config, 0).map_err(|_| Failed)?;
            invocations.push(Invocation {
                case_index,
                repeat,
                config,
                initial,
            });
        }
    }
    Ok(Peer::new(cases, invocations))
}

fn verify_preview(response: &S3Response, expected: (&str, &str)) -> Check<()> {
    let PresentationContent::Recognition {
        completion: RecognitionCompletion::Other,
        has_final_segment: false,
        has_partials: true,
        result,
        ..
    } = Presentation::new(response).content
    else {
        return Err(Failed);
    };
    let partial = result.partial_result.as_ref().ok_or(Failed)?;
    let preview = PartialTranscript::from_result(partial);
    check(
        response.recognition_state() == Some(0)
            && preview.parts.len() == 2
            && !preview.has_nonfinite_scores
            && preview.stable_prefix == expected.0
            && preview.tentative_suffix == expected.1
            && preview.text() == [expected.0, expected.1].concat(),
    )
}

fn render_verified(result: &DictationResult, case: &Case) -> Check<String> {
    let expected = &case.expected;
    let rendered = result.render(RenderPolicy::Strict).map_err(|_| Failed)?;
    check(
        rendered.text == expected.final_text
            && rendered.provenance == expected.provenance
            && rendered.intent == expected.intent
            && rendered.fallback_reason
                == (expected.provenance == TextProvenance::HypothesisText)
                    .then_some(FallbackReason::VoiceEditAbsent),
    )?;
    if let Some(range) = expected.selection_utf16 {
        // This only validates a replacement proposal against the captured
        // original window; its range is local to the proposed text. No UI acts.
        let proposal = replacement_proposal(result, None).map_err(|_| Failed)?;
        proposal
            .validate_current(&case.context, None)
            .map_err(|_| Failed)?;
        check(
            proposal.snapshot == case.context
                && proposal.expected_revision.is_none()
                && proposal.replacement_text == rendered.text
                && proposal.selection_utf16_range() == Some(range),
        )?;
    }
    Ok(rendered.text)
}

async fn qualify_case(engine: &Engine, peer: &Peer, ordinal: usize) -> Check<(u64, Value)> {
    let step = &peer.invocations[ordinal];
    let case = &peer.cases[step.case_index];
    let fixture = &case.fixture;
    let partials = &fixture.expected.partials;
    let input = if partials.is_empty() {
        audio_common::Source::new(audio_common::fragment(&fixture.pcm, &FRAGMENTS))
    } else {
        let (prefix, tail) = fixture.pcm.split_at(FRAME_BYTES);
        let mut chunks = audio_common::fragment(prefix, &FRAGMENTS);
        let gate_after = chunks.len();
        chunks.extend(audio_common::fragment(tail, &FRAGMENTS));
        let mut source = audio_common::Source::new(chunks);
        source.block_after = Some(gate_after);
        source
    };
    let input_reads = input.chunks.len();
    let release_preview = Arc::clone(&input.read_gate);
    let input_stats = Arc::clone(&input.stats);
    let (source, pcm_progress) = Pcm16Opus::new_with_progress(input, EncoderConfig::default());
    let source = Box::new(source);
    let options = SessionOptions {
        config: step.config.clone(),
        context: ContextInput::Static(fixture.context.clone()),
        timeout: SESSION_DEADLINE,
        ..Default::default()
    };

    let started = Instant::now();
    let mut session = engine.start(source, options).map_err(|_| Failed)?;
    let mut digest = Sha256::new();
    let mut first_response_us = None;
    let mut response_bytes = 0;
    let mut session_id = None;
    let mut final_update = None;
    for (index, expected_wire) in fixture.replies.iter().enumerate() {
        let EngineEvent { response, metrics } =
            session.next().await.map_err(|_| Failed)?.ok_or(Failed)?;
        if index == 0 {
            first_response_us = Some(started.elapsed().as_micros() as u64);
        }
        let response = response.ok_or(Failed)?;
        let length = response.raw.len() as u64;
        response_bytes += length;
        check(
            response.raw.as_ref() == expected_wire.as_ref()
                && response.error_code.is_none()
                && metrics.responses_received == index as u64 + 1
                && metrics.response_protobuf_bytes == response_bytes
                && metrics.eof_seconds.is_none()
                && *session_id.get_or_insert(metrics.session_id) == metrics.session_id,
        )?;
        digest.update(length.to_le_bytes());
        digest.update(&response.raw);
        if let Some(expected_preview) = partials.get(index) {
            check(response.status.is_none())?;
            verify_preview(&response, *expected_preview)?;
            let pcm = pcm_progress.snapshot();
            check(
                pcm.input_bytes == FRAME_BYTES as u64
                    && !pcm.input_eof
                    && !pcm.output_eof
                    && !metrics.audio.eof,
            )?;
            if index + 1 == partials.len() {
                release_preview.notify_one();
            }
        } else if index == partials.len() {
            check(
                response.status.is_none()
                    && matches!(
                        Presentation::new(&response).content,
                        PresentationContent::Recognition {
                            completion: RecognitionCompletion::Complete,
                            has_final_segment: true,
                            has_partials: false,
                            ..
                        }
                    ),
            )?;
            final_update = Some(response);
        } else {
            check(
                response.raw.as_ref() == S3_END
                    && response.status == Some(1)
                    && matches!(
                        Presentation::new(&response).content,
                        PresentationContent::None
                    ),
            )?;
        }
    }

    let EngineEvent { response, metrics } =
        session.next().await.map_err(|_| Failed)?.ok_or(Failed)?;
    let session_us = started.elapsed().as_micros() as u64;
    let first_response_us = first_response_us.ok_or(Failed)?;
    let session_id = session_id.ok_or(Failed)?;
    check(
        response.is_none()
            && metrics.eof_seconds.is_some()
            && metrics.session_id == session_id
            && metrics.responses_received == fixture.replies.len() as u64
            && metrics.response_protobuf_bytes == response_bytes
            && first_response_us <= session_us,
    )?;
    // The only healthy terminal event is the EOF above, followed by clean None.
    check(session.next().await.map_err(|_| Failed)?.is_none())?;
    let progress = session.audio_progress();
    let context = session.captured_context();
    let pcm = pcm_progress.snapshot();
    let receipt = peer.confirm(ordinal)?;
    check(
        progress == metrics.audio
            && progress.eof
            && progress.cleanup_finished
            && progress.cleanup_succeeded
            && pcm.input_eof
            && pcm.output_eof
            && pcm.input_bytes == fixture.pcm.len() as u64
            && pcm.encoded_input_samples == (fixture.pcm.len() / 2) as u64
            && pcm.encoded_frames == case.codec_packets.len() as u64
            && input_stats.eof.load(Ordering::SeqCst)
            && input_stats.reads.load(Ordering::SeqCst) == input_reads
            && input_stats.closes.load(Ordering::SeqCst) == 1
            && context.as_ref() == Some(&fixture.context)
            && receipt.requests == metrics.requests_produced
            && receipt.request_bytes == metrics.request_protobuf_bytes
            && receipt.audio_messages == progress.chunks
            && receipt.ogg_bytes == progress.bytes,
    )?;
    // No result or renderer is exposed on a partial, state1, or S3 END. Require
    // verified peer consumption, a unique successful engine EOF and cleanup.
    let response_count = metrics.responses_received;
    let result = DictationResult {
        final_updates: vec![final_update.ok_or(Failed)?],
        context,
        metrics: Some(metrics),
    };
    let rendered = render_verified(&result, fixture)?;
    Ok((
        session_id,
        json!({
            "case": fixture.id,
            "repeat": step.repeat,
            "pcm_samples": fixture.pcm.len() / 2,
            "pcm_sha256": case.pcm_sha256,
            "wire_responses_sha256": hex::encode(digest.finalize()),
            "rendered_sha256": hex::encode(Sha256::digest(rendered.as_bytes())),
            "request_count": receipt.requests,
            "response_count": response_count,
            "ogg_bytes": receipt.ogg_bytes,
            "first_response_us": first_response_us,
            "session_us": session_us,
        }),
    ))
}

async fn qualify_rows(engine: &Engine, peer: &Peer) -> Check<Vec<Value>> {
    let mut rows = Vec::with_capacity(peer.invocations.len());
    let mut ids = HashSet::with_capacity(peer.invocations.len());
    for ordinal in 0..peer.invocations.len() {
        let (id, row) = timeout(SESSION_DEADLINE, qualify_case(engine, peer, ordinal))
            .await
            .map_err(|_| Failed)??;
        check(ids.insert(id))?;
        rows.push(row);
    }
    Ok(rows)
}

async fn with_peer<T>(peer: Peer, run: impl AsyncFnOnce(&Engine, &Peer) -> Check<T>) -> Check<T> {
    let peer = Arc::new(peer);
    let server = timeout(
        SESSION_DEADLINE,
        TestServer::new({
            let peer = Arc::clone(&peer);
            move |request| Arc::clone(&peer).serve(request)
        }),
    )
    .await
    .map_err(|_| Failed)?;
    let config = TransportConfig::loopback(server.address)
        .and_then(|config| config.with_max_message_bytes(MAX_FIXTURE_MESSAGE_BYTES));
    let Ok(config) = config else {
        let _ = timeout(SESSION_DEADLINE, server.close()).await;
        return Err(Failed);
    };
    let Ok(Ok(transport)) = timeout(SESSION_DEADLINE, GrpcTransport::connect(config)).await else {
        let _ = timeout(SESSION_DEADLINE, server.close()).await;
        return Err(Failed);
    };
    let engine = Engine::new(transport.clone());
    let result = run(&engine, &peer).await;
    // Attempt all cleanup even when a case rejected or hit its deadline.
    let engine_closed = timeout(SESSION_DEADLINE, engine.close()).await;
    let transport_closed = timeout(SESSION_DEADLINE, transport.close()).await;
    let server_closed = timeout(SESSION_DEADLINE, server.close()).await;
    check(
        matches!(engine_closed, Ok(Ok(())))
            && matches!(transport_closed, Ok(Ok(())))
            && server_closed.is_ok(),
    )?;
    let result = result?;
    peer.verify_closed()?;
    Ok(result)
}

async fn qualify(repeats: usize) -> Check<Value> {
    let fixtures = fixtures::cases();
    check(fixtures.iter().map(|case| case.id).eq(CASE_IDS))?;
    let peer = timeout(SESSION_DEADLINE, prepare_cases(fixtures, repeats))
        .await
        .map_err(|_| Failed)??;
    let rows = with_peer(peer, qualify_rows).await?;
    Ok(json!({
        "schema": "rambler-native-qualification-v1",
        "scope": "synthetic-localhost-only",
        "repeats": repeats,
        "rows": rows,
    }))
}

fn run(repeats: usize) -> Check<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| Failed)?;
    let report = runtime.block_on(qualify(repeats))?;
    drop(runtime);
    let mut line = serde_json::to_vec(&report).map_err(|_| Failed)?;
    line.push(b'\n');
    let mut stdout = io::stdout().lock();
    stdout
        .write_all(&line)
        .and_then(|()| stdout.flush())
        .map_err(|_| Failed)
}

fn main() -> ExitCode {
    // The reused test helpers can panic on malformed input or server setup.
    // This standalone executable never discloses their payloads or backtraces.
    std::panic::set_hook(Box::new(|_| {}));
    let Ok(repeats) = parse_repeats(std::env::args_os().skip(1)) else {
        let _ = writeln!(
            io::stderr().lock(),
            "invalid native qualification arguments"
        );
        return ExitCode::from(2);
    };
    if !matches!(guarded(|| run(repeats)), Ok(Ok(()))) {
        let _ = writeln!(io::stderr().lock(), "native qualification failed");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Check<usize> {
        parse_repeats(args.iter().copied().map(OsString::from))
    }

    #[test]
    fn accepts_only_the_optional_canonical_bounded_repeats_pair() {
        assert_eq!(parse(&[]).ok(), Some(1));
        for repeats in 1..=MAX_REPEATS {
            assert_eq!(
                parse(&["--repeats", &repeats.to_string()]).ok(),
                Some(repeats)
            );
        }
        for args in [
            &["--repeats"][..],
            &["--repeats", "0"],
            &["--repeats", "11"],
            &["--repeats", "99999999999999999999999999"],
            &["--repeats", "01"],
            &["--repeats", "+1"],
            &["--repeats", "-1"],
            &["--repeats", " 1"],
            &["--repeats", "１"],
            &["--repeats", "1", "--repeats", "2"],
            &["--repeats", "1", "--"],
            &["--repeats=1"],
            &["--help"],
            &["--loopback", "127.0.0.1:1"],
            &["external-corpus"],
        ] {
            assert!(parse(args).is_err());
        }
    }
}
