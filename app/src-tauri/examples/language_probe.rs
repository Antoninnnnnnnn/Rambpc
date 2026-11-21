//! Diagnostic (not part of the app): send a 16 kHz mono WAV to the real
//! Rambler service and print the language fields of each final response.
//!
//!   cargo run --example language_probe -- <wav> <primary> [secondary...]
//! Requires the Rambler access (built-in, or `RAMBLER_*` in the environment).

use rambler_audio::{EncoderConfig, Pcm16Opus, WavAudio, WavConfig};
use rambler_engine::{CollectorLimits, Engine, SessionOptions, collect_dictation};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let wav = args.first().expect("wav path").clone();
    let primary = args.get(1).cloned().unwrap_or_else(|| "fr-FR".into());
    let secondary: Vec<String> = args.iter().skip(2).cloned().collect();
    let access = sambler_lib::rambler::RamblerAccess::from_environment()
        .ok()
        .flatten()
        .expect("RAMBLER_* access in the environment");
    let engine = Engine::connect_ready(access.transport_config()).await.expect("connect");
    let audio = WavAudio::open(&wav, WavConfig::default()).await.expect("wav");
    let source = Box::new(Pcm16Opus::new(audio, EncoderConfig::default()));
    let mut options = SessionOptions {
        credentials: Some(Arc::new(access.metadata().clone())),
        ..Default::default()
    };
    options.config.language = primary.clone();
    options.config.secondary_languages = secondary.clone();
    let result = collect_dictation(&engine, source, options, CollectorLimits::default())
        .await
        .expect("dictation");
    for response in &result.final_updates {
        if let Some(r) = &response.result {
            println!(
                "[{primary} + {secondary:?}] language={:?} spoken_language={:?}",
                r.language, r.spoken_language
            );
        }
    }
    let _ = engine.close().await;
}
