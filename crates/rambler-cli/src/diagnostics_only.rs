//! Opt-in telemetry projection of the CLI's private result envelope.
//!
//! Add diagnostics by allowlisting their names and value types here. Never copy
//! an entire object from the result, even if its current fields are harmless.
use serde_json::{Map, Value, json};

const TYPES: &[&str] = &["result", "error", "ambiguous"];
const CATEGORIES: &[&str] = &[
    "ambiguous",
    "arguments",
    "audio",
    "cancelled",
    "cleanup",
    "configuration",
    "credentials",
    "engine",
    "finalize_timeout",
    "grpc",
    "incomplete",
    "protocol",
    "result_limit",
    "s3",
    "safety_blocked",
    "signal",
    "timeout",
    "transport",
    "unrecognized_safety",
];
const OUTCOMES: &[&str] = &[
    "usable_text",
    "partial",
    "no_result",
    "needs_review",
    "verified_deletion",
    "safety_blocked",
    "unrecognized_safety",
];
const REASONS: &[&str] = &[
    "empty_text",
    "missing_text",
    "no_finals",
    "no_segments",
    "unknown",
];
const GRPC_CODES: &[&str] = &[
    "Ok",
    "Cancelled",
    "Unknown",
    "InvalidArgument",
    "DeadlineExceeded",
    "NotFound",
    "AlreadyExists",
    "PermissionDenied",
    "ResourceExhausted",
    "FailedPrecondition",
    "Aborted",
    "OutOfRange",
    "Unimplemented",
    "Internal",
    "Unavailable",
    "DataLoss",
    "Unauthenticated",
];
const METRICS: &[&str] = &[
    "requests_produced",
    "request_protobuf_bytes",
    "responses_received",
    "response_protobuf_bytes",
    "elapsed_seconds",
    "first_response_seconds",
    "eof_seconds",
];
const TIMELINE: &[&str] = &[
    "elapsed_seconds",
    "first_response_seconds",
    "last_audio_sent_seconds",
    "context_sent_seconds",
    "end_sent_seconds",
    "half_close_sent_seconds",
    "last_response_seconds",
    "last_response_s3_status",
    "last_partial_response_seconds",
    "recognition_finals_received",
    "requests_produced",
    "requests_submitted",
    "request_protobuf_bytes",
    "request_protobuf_bytes_submitted",
    "responses_received",
    "responses_deframed",
    "response_protobuf_bytes",
    "audio_source_eof",
    "audio_cleanup_succeeded",
    "request_source_exhausted",
    "saw_s3_end",
];
const AUDIO: &[&str] = &[
    "chunks",
    "bytes",
    "eof",
    "cleanup_finished",
    "cleanup_succeeded",
];
const PCM: &[&str] = &[
    "input_bytes",
    "encoded_input_samples",
    "encoded_frames",
    "input_eof",
    "output_eof",
];

pub(crate) fn write_report(report: Value, diagnostics_only: bool) {
    if diagnostics_only {
        println!("{}", project(&report));
    } else {
        println!("{report}");
    }
}

fn project(report: &Value) -> Value {
    let mut output = envelope(report);
    output.insert("diagnostics_only".into(), json!(true));
    if let Some(segments) = report.get("completed_segments").and_then(Value::as_array) {
        output.insert(
            "completed_segments".into(),
            Value::Array(
                segments
                    .iter()
                    .map(|segment| Value::Object(envelope(segment)))
                    .collect(),
            ),
        );
    }
    Value::Object(output)
}

fn envelope(report: &Value) -> Map<String, Value> {
    let mut output = Map::new();
    for (field, values) in [
        ("type", TYPES),
        ("category", CATEGORIES),
        ("close_error_category", CATEGORIES),
        ("outcome", OUTCOMES),
        ("no_result_reason", REASONS),
        ("grpc_code", GRPC_CODES),
        ("close_grpc_code", GRPC_CODES),
    ] {
        copy_label(report, &mut output, field, values);
    }
    if let Some(range) = report.get("sample_range").and_then(Value::as_array)
        && let [start, end] = range.as_slice()
        && let (Some(start), Some(end)) = (start.as_u64(), end.as_u64())
    {
        output.insert("sample_range".into(), json!([start, end]));
    }
    if let Some(metrics) = report.get("metrics") {
        let mut view = counters(metrics, METRICS);
        if let Some(audio) = metrics.get("audio") {
            view.insert("audio".into(), Value::Object(counters(audio, AUDIO)));
        }
        output.insert("metrics".into(), Value::Object(view));
    }
    if let Some(pcm) = report.get("pcm") {
        output.insert("pcm".into(), Value::Object(counters(pcm, PCM)));
    }
    if let Some(timeline) = report.get("timeline") {
        let mut view = counters(timeline, TIMELINE);
        copy_label(timeline, &mut view, "terminal_grpc_code", GRPC_CODES);
        output.insert("timeline".into(), Value::Object(view));
    }
    output
}

fn copy_label(report: &Value, output: &mut Map<String, Value>, field: &str, allowed: &[&str]) {
    if let Some(value) = report.get(field)
        && value.as_str().is_some_and(|label| allowed.contains(&label))
    {
        output.insert(field.into(), value.clone());
    }
}

fn counters(object: &Value, fields: &[&str]) -> Map<String, Value> {
    fields
        .iter()
        .filter_map(|&field| {
            let value = object.get(field)?;
            (value.is_number() || value.is_boolean()).then(|| (field.into(), value.clone()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whitelist_retains_categories_and_counters_without_private_payloads() {
        let marker = "PRIVATE_SYNTHETIC_MARKER";
        let report = json!({
            "type": "error", "category": "grpc", "grpc_code": "Unavailable",
            "close_error_category": marker, "text": marker, "context": {"selected": marker},
            "final_updates": [{"hypotheses": [{"text": marker}]}],
            "timeline": {"elapsed_seconds": 1.25, "terminal_grpc_code": "Unavailable", "unknown": marker},
            "completed_segments": [{
                "type": "result", "outcome": "usable_text", "text": marker, "sample_range": [0, 400],
                "metrics": {"audio": {"bytes": 88, "eof": true, "raw_audio": marker}, "responses_received": 3},
            }, {
                "type": "ambiguous", "outcome": "no_result", "no_result_reason": "missing_text",
                "sample_range": [400, 420], "final_updates": [marker],
            }],
        });
        let view = project(&report);
        assert!(!view.to_string().contains(marker));
        assert_eq!(
            view,
            json!({
                "diagnostics_only": true, "type": "error", "category": "grpc", "grpc_code": "Unavailable",
                "timeline": {"elapsed_seconds": 1.25, "terminal_grpc_code": "Unavailable"},
                "completed_segments": [{
                    "type": "result", "outcome": "usable_text", "sample_range": [0, 400],
                    "metrics": {"audio": {"bytes": 88, "eof": true}, "responses_received": 3},
                }, {
                    "type": "ambiguous", "outcome": "no_result", "no_result_reason": "missing_text",
                    "sample_range": [400, 420],
                }],
            })
        );
    }
}
