use super::RamblerAccess;
use crate::{
    ipc::{DraftPart, InterruptReason, PartStatus, UnavailableReason},
    model::Language,
    ports::PcmRate,
};
use rambler_audio::{
    CaptureError, CaptureInput, CompletedDictationSegment, InteractivePcmProfile, Pcm48kTo16k,
    stream_long_pcm_dictation,
};
use rambler_engine::{
    CollectorLimits, ContextInput, DictationOutcome, Engine, EngineError, RenderPolicy,
};
use rambler_transport::TransportError;
use std::{ops::Range, sync::Arc};
use tokio::sync::{Notify, mpsc};
use tonic::Code;

pub(crate) const MAX_DRAFT_BYTES: usize = 256 * 1024;

pub(crate) struct Segment {
    pub index: usize,
    pub range: Range<u64>,
    pub part: DraftPart,
}

impl Segment {
    fn project(segment: CompletedDictationSegment) -> Self {
        let (status, text) = match segment.result.interpret(RenderPolicy::AsrOnly) {
            DictationOutcome::UsableText(text) => (PartStatus::Admitted, Some(text.text)),
            DictationOutcome::NoResult(_) => (PartStatus::NoText, None),
            DictationOutcome::SafetyBlocked | DictationOutcome::UnrecognizedSafety => {
                (PartStatus::Blocked, None)
            }
            // Review, deletion, and any future effect stay out of the text.
            _ => (PartStatus::NeedsReview, None),
        };
        Self {
            index: segment.segment_index,
            range: segment.sample_range,
            part: DraftPart { status, text },
        }
    }
}

pub(crate) async fn collect(
    engine: Arc<Engine>,
    access: RamblerAccess,
    language: Language,
    rate: PcmRate,
    input: CaptureInput,
    segments: mpsc::Sender<Segment>,
    #[cfg(test)] segment_budget: Option<std::time::Duration>,
) -> Result<(), InterruptReason> {
    let mut profile = InteractivePcmProfile::default();
    #[cfg(test)]
    if let Some(segment_budget) = segment_budget {
        profile.segmentation.segment_budget = segment_budget;
        profile.segmentation.pause_search = std::time::Duration::ZERO;
    }
    profile.session.config.language = language.code().into();
    profile.session.credentials = Some(Arc::new(access.metadata().clone()));
    profile.session.context = ContextInput::None;
    collect_profile(&engine, profile, rate, input, segments).await
}

async fn collect_profile(
    engine: &Engine,
    profile: InteractivePcmProfile,
    rate: PcmRate,
    input: CaptureInput,
    segments: mpsc::Sender<Segment>,
) -> Result<(), InterruptReason> {
    let unavailable = Notify::new();
    let mut sink = |segment| {
        let segment = Segment::project(segment);
        if segment
            .part
            .text
            .as_ref()
            .is_some_and(|text| text.len() > MAX_DRAFT_BYTES)
            || segments.try_send(segment).is_err()
        {
            unavailable.notify_one();
        }
    };
    let run = async {
        let limits = CollectorLimits::default();
        match rate {
            PcmRate::Hz16000 => {
                stream_long_pcm_dictation(
                    engine,
                    input,
                    profile.session,
                    limits,
                    profile.segmentation,
                    &mut sink,
                )
                .await
            }
            PcmRate::Hz48000 => {
                stream_long_pcm_dictation(
                    engine,
                    Pcm48kTo16k::new(input),
                    profile.session,
                    limits,
                    profile.segmentation,
                    &mut sink,
                )
                .await
            }
        }
    };
    let result = tokio::select! {
        biased;
        _ = unavailable.notified() => return Err(InterruptReason::Service),
        result = run => result,
    };
    // The SDK may have reached EOF in the same poll as a rejected delivery.
    tokio::select! {
        biased;
        _ = unavailable.notified() => Err(InterruptReason::Service),
        result = std::future::ready(result) => result.map_err(|error| interruption(&error)),
    }
}

fn transport_reason(error: &TransportError) -> InterruptReason {
    use InterruptReason as R;
    use TransportError as T;
    match error {
        T::Grpc(Code::Unauthenticated | Code::PermissionDenied) => R::AccessRefused,
        T::Grpc(Code::ResourceExhausted) => R::Throttled,
        T::Timeout | T::Grpc(Code::DeadlineExceeded) => R::Timeout,
        T::Connect
        | T::Resolve
        | T::TrustUnavailable
        | T::CertificateRejected
        | T::Closed
        | T::Grpc(Code::Unavailable) => R::Network,
        T::PrimaryAndCleanup(failure) => transport_reason(failure.primary()),
        T::Source(source)
            if source.original().downcast_ref::<CaptureError>()
                == Some(&CaptureError::Overflow) =>
        {
            R::CaptureOverflow
        }
        _ => R::Service,
    }
}

fn interruption(error: &EngineError) -> InterruptReason {
    match error {
        EngineError::Transport(error) => transport_reason(error),
        EngineError::FinalizeTimeout | EngineError::ContextTimeout => InterruptReason::Timeout,
        EngineError::Closed => InterruptReason::Network,
        EngineError::Audio(source)
            if source.original().downcast_ref::<CaptureError>()
                == Some(&CaptureError::Overflow) =>
        {
            InterruptReason::CaptureOverflow
        }
        _ => InterruptReason::Service,
    }
}

pub(crate) fn connection_failure(error: &EngineError) -> UnavailableReason {
    match interruption(error) {
        InterruptReason::AccessRefused => UnavailableReason::AccessRefused,
        InterruptReason::Network | InterruptReason::Timeout => UnavailableReason::Network,
        InterruptReason::Throttled => UnavailableReason::Throttled,
        _ => UnavailableReason::Service,
    }
}
