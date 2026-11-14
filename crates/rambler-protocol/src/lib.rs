//! Native, bounded S3 protobuf subset, following the frozen Python 0.3 reference.
//!
//! The engine sends [`initial_request`], continuous-stream [`audio_request`] chunks,
//! a separately obtained late [`context_request`], then [`end_request`] and half-closes.
//! All fallible helpers return [`Error`]; caller-editable configs are validated on use.
//! Strings are Rust UTF-8, contexts are preserved verbatim (including supplementary
//! characters); this crate does not calculate or apply UTF-16 edit ranges.
//!
//! Decoded optional scalars preserve absence, enums remain signed numeric values,
//! repeated items preserve duplicates/order, and unknown fields retain exact wire
//! bytes. Root `raw` is exact. A merged singular submessage's `raw` (and `active_raw`)
//! is the logical concatenation of original occurrence payloads, NOT a standalone
//! original capture. Each occurrence is validated separately before merging; invalid
//! fragments cannot heal one another. Opaque unknown payloads are not recursively
//! interpreted. The global field budget charges each interpreted field once.
//!
//! # Prototype 0.1 raw-storage API
//!
//! Model `raw` fields and `RecognitionResponse::active_raw` use immutable
//! [`bytes::Bytes`] instead of `Vec<u8>`. Decoding copies the exact root payload
//! once, then shares that storage for single-occurrence children; merged raw is
//! allocated as a logical concatenation. A retained child can keep its entire
//! exact root payload alive (at most 8 MiB), but never an unrelated larger
//! transport/framer allocation. Detach a small child explicitly with
//! `bytes::Bytes::copy_from_slice(&child.raw)` or `child.raw.to_vec()` when needed.
//! Unknown [`wire::Field`] / [`wire::Value`] and request return types are unchanged.
//! Migrate construction with `vec.into()`, slice access with `raw.as_ref()` or
//! `&raw[..]`, and explicit mutable copies with `raw.to_vec()`. `raw.clone()` now
//! shares immutable storage; replace the field to change bytes. Read-only slice
//! callers and the `decode_response(&[u8])` signature are unchanged.
//!
//! These are data only: no finality inference, collector, edit application, logging,
//! network, credentials, Python/Android runtime, or alternative backend is included.
//!
//! # Engine-facing use
//!
//! ```
//! use rambler_protocol::{ProtocolConfig, EditingContext, initial_request,
//!     audio_request, context_request, end_request, decode_response};
//!
//! let config = ProtocolConfig::default(); // platform="Rust", 16kHz/mono/Opus10
//! let init = initial_request(&config, 0)?;
//! let audio = audio_request(b"opaque continuous Ogg/Opus chunk", 150)?;
//! // Obtain the actual text snapshot late; do not copy init hints here.
//! let context = EditingContext {
//!     before: "avant 𝄞".into(), selected: "sélection".into(),
//!     after: " après".into(), cursor_aware: true, ..Default::default()
//! };
//! let late_context = context_request(&context)?;
//! let terminal = end_request(); // then half-close and drain transport responses
//! let response = decode_response(&[0x08, 0x01])?;
//! assert_eq!(response.status, Some(1)); // S3 END, not proof of gRPC success
//! # Ok::<(), rambler_protocol::Error>(())
//! ```

mod decode;
mod models;
mod request;
pub mod wire;

pub use decode::decode_response;
pub use models::*;
pub use request::{ProtocolConfig, audio_request, context_request, end_request, initial_request};

pub const MAX_MESSAGE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_FIELDS: usize = 100_000;
/// Defensive SDK limits, not observed server limits.
pub const MAX_HINTS_PER_CATEGORY: usize = 1024;
pub const MAX_HINT_UTF8_BYTES: usize = 1024 * 1024;
pub const MAX_SECONDARY_LANGUAGES: usize = 16;
pub const MAX_SECONDARY_LANGUAGE_UTF8_BYTES: usize = 1024;

pub const USER_INFO: u32 = 293000;
pub const AUDIO_INFO: u32 = 293100;
pub const AUDIO_DATA: u32 = 293101;
pub const CLIENT_INFO: u32 = 294000;
pub const RECOGNIZER_INFO: u32 = 294500;
pub const INPUT_BOX_INFO: u32 = 1203;
pub const INITIAL_PARAMETER: u32 = 29735924;
pub const RECOGNITION_RESPONSE: u32 = 1253625;
pub const VOICE_EDITING_RESULT: u32 = 74956224;
/// Verified closed safety enum on each recognition hypothesis (0/1 only).
pub const HYPOTHESIS_SAFETY_STATUS: u32 = 30;

/// Errors contain no caller text, hint contents, audio, or response data.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("message exceeds size limit")]
    SizeLimit,
    #[error("input exceeds max_fields")]
    FieldLimit,
    #[error("truncated protobuf field")]
    Truncated,
    #[error("varint overflows unsigned 64-bit integer")]
    VarintOverflow,
    #[error("invalid protobuf field number")]
    FieldNumber,
    #[error("unsupported protobuf wire type")]
    WireType,
    #[error("known string must be valid UTF-8")]
    Utf8,
    #[error("invalid configuration: {0}")]
    InvalidConfig(&'static str),
}

pub(crate) fn bounded(len: usize) -> Result<(), Error> {
    if len > MAX_MESSAGE_BYTES {
        Err(Error::SizeLimit)
    } else {
        Ok(())
    }
}
