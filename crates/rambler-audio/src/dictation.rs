use crate::{EncoderConfig, Pcm16Opus, PcmSender, pcm_channel};
use rambler_engine::{
    CollectorLimits, DictationObservation, DictationResult, Engine, EngineError, SessionOptions,
    collect_dictation, collect_dictation_with_observer,
};
use std::future::Future;

/// Starts bounded dictation from caller-supplied PCM16LE, 16 kHz, mono.
///
/// Returns the unique producer for this session and its completion future. This
/// requires an active Tokio runtime: the input is admitted to the engine during
/// this call, not on the future's first poll. Preflight failures are reported by
/// the future, not by the producer, and still close the input.
///
/// Drive the producer and completion concurrently. Use [`PcmSender::reserve`]
/// to wait for the one queue slot before sending each chunk. Call
/// [`PcmSender::finish`] only on normal input EOF; drop the producer on input
/// failure or abandonment. Neither queue acceptance nor finish signals success.
/// An `Ok` result requires clean engine completion; it can still lack a unique,
/// safety-checked text projection. Use [`DictationResult::text`] for strict text.
///
/// Dropping the completion future cancels this session and cleanup closes the
/// input, waking blocked reservations. Keep the runtime alive and await
/// [`Engine::close`] at shutdown to join outstanding cleanup. Use
/// [`SessionOptions::with_recommended_finalize_timeout`] to opt in to the
/// interactive post-audio budget; the default has no separate budget. If the producer
/// also waits on an external input, arrange to stop that wait when collection
/// ends or is cancelled; closing the PCM input cannot wake an external wait.
///
/// Uses [`EncoderConfig::default`]. For a custom encoder configuration or local
/// PCM progress, compose [`pcm_channel`] and [`Pcm16Opus`] directly.
///
/// # Example
///
/// This example feeds an already available slice in bounded fragments. In a live
/// producer, retain the same EOF rule but bound or cancel external input waits.
///
/// ```
/// use bytes::Bytes;
/// use rambler_audio::start_pcm_dictation;
/// use rambler_engine::{CollectorLimits, Engine, EngineError, SessionOptions};
///
/// async fn dictate(engine: &Engine, pcm: &[u8]) -> Result<String, EngineError> {
///     let (mut writer, completed) = start_pcm_dictation(
///         engine, SessionOptions::default().with_recommended_finalize_timeout(),
///         CollectorLimits::default(),
///     );
///     let feed = async move {
///         for fragment in pcm.chunks(641) {
///             let Ok(permit) = writer.reserve().await else { return };
///             // At this fixed fragment size, send can only fail on closure.
///             if permit.send(Bytes::copy_from_slice(fragment)).is_err() {
///                 return;
///             }
///         }
///         let _ = writer.finish(); // If already closed, rely on the engine result.
///     };
///     let ((), result) = tokio::join!(feed, completed);
///     result?.text()
/// }
/// ```
pub fn start_pcm_dictation(
    engine: &Engine,
    options: SessionOptions,
    limits: CollectorLimits,
) -> (
    PcmSender,
    impl Future<Output = Result<DictationResult, EngineError>> + Send + 'static,
) {
    let (writer, input) = pcm_channel();
    let audio = Box::new(Pcm16Opus::new(input, EncoderConfig::default()));
    (writer, collect_dictation(engine, audio, options, limits))
}

/// Starts PCM dictation like [`start_pcm_dictation`], with a borrowed observer.
///
/// See [`collect_dictation_with_observer`] for callback ordering and retention.
/// Keep the callback quick and nonblocking. Its views are provisional, including
/// final-looking responses and S3 END; they are not safety-checked text. Clear
/// previews after completion, failure, or cancellation. Only the completion
/// future delivers the result of the session.
pub fn start_pcm_dictation_with_observer<'a>(
    engine: &Engine,
    options: SessionOptions,
    limits: CollectorLimits,
    observer: &'a mut (dyn for<'event> FnMut(DictationObservation<'event>) + Send + 'a),
) -> (
    PcmSender,
    impl Future<Output = Result<DictationResult, EngineError>> + Send + 'a + use<'a>,
) {
    let (writer, input) = pcm_channel();
    let audio = Box::new(Pcm16Opus::new(input, EncoderConfig::default()));
    (
        writer,
        collect_dictation_with_observer(engine, audio, options, limits, observer),
    )
}
