//! Windows implementations. PLACEHOLDER skeleton: the platform work package
//! replaces each stub with the real implementation (CPAL/WASAPI capture, UIA
//! target probe, SendInput insertion, private clipboard, DPAPI, WTS/power events).

use crate::model::{MicChoice, SystemEvent};
use crate::ports::*;

pub struct WasapiMicrophone;
impl WasapiMicrophone {
    pub fn new() -> Self {
        Self
    }
}
impl Microphone for WasapiMicrophone {
    fn inputs(&self) -> Result<Vec<MicInfo>, MicError> {
        Err(MicError::NoInput)
    }
    fn open(&self, _: &MicChoice, _: SinkFactory, _: FaultCallback) -> Result<Box<dyn MicStream>, MicError> {
        Err(MicError::NoInput)
    }
}

pub struct UiaTargetProbe;
impl UiaTargetProbe {
    pub fn new() -> Self {
        Self
    }
}
impl TargetProbe for UiaTargetProbe {
    fn capture(&self) -> TargetVerdict {
        TargetVerdict::Uncertain { reason: UncertainReason::Unsupported }
    }
    fn watch(&self, _: &TargetToken, _: Box<dyn FnOnce(TargetChange) + Send>) -> WatchGuard {
        Box::new(())
    }
    fn revalidate(&self, _: &TargetToken) -> Result<(), TargetChange> {
        Err(TargetChange::Unknown)
    }
}

pub struct SendInputInserter;
impl SendInputInserter {
    pub fn new() -> Self {
        Self
    }
}
impl Inserter for SendInputInserter {
    fn insert_once(&self, _: &TargetToken, _: &str) -> InsertOutcome {
        InsertOutcome::NotWritten { reason: NotWrittenReason::Unsupported }
    }
}

pub struct WinClipboard;
impl WinClipboard {
    pub fn new() -> Self {
        Self
    }
}
impl Clipboard for WinClipboard {
    fn copy_private(&self, _: &str) -> Result<(), ClipboardError> {
        Err(ClipboardError::Other)
    }
}

pub struct Dpapi;
impl Dpapi {
    pub fn new() -> Self {
        Self
    }
}
impl Protector for Dpapi {
    fn protect(&self, _: &[u8]) -> Result<Vec<u8>, ProtectError> {
        Err(ProtectError::Protect)
    }
    fn unprotect(&self, _: &[u8]) -> Result<zeroize::Zeroizing<Vec<u8>>, ProtectError> {
        Err(ProtectError::Unreadable)
    }
}

pub struct WtsSystemEvents;
impl WtsSystemEvents {
    pub fn new() -> Self {
        Self
    }
}
impl SystemEvents for WtsSystemEvents {
    fn subscribe(&self, _: Box<dyn Fn(SystemEvent) + Send + Sync>) -> Subscription {
        Box::new(())
    }
}

pub fn open_mic_privacy_settings() {}
