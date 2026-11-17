//! Optional Linux x86_64 developer experiment, NOT a production authentication API.
//! No args: silent no-op, including no native hardening. Startup/argv/env MUST be
//! secret-free: exec/dynamic loading happen BEFORE main can disable dumps.
//! This binary alone NEVER authorizes live GO. A separately reviewed owner must
//! verify the binary and externally observe environ EACCES BEFORE sending a frame.
//! Same-UID/root adversaries, swap, sshd/launcher buffers and kernel compromise are
//! outside this bounded protection claim. No memory-erasure guarantee is made.
//!
//! Sender format (no text envelope): magic RLA1\0\r\n\x1a (8 bytes), then exactly
//! three [u16 big-endian byte length, UTF-8 bytes] fields, in order API key,
//! Android package, Android cert. Each length is 1..=4096, chosen independently
//! of actual credentials. EOF is mandatory; any trailing byte is rejected.
//! --public-frame-check-v1 accepts ONLY the three invented PUBLIC_FIELDS below,
//! delegates validation, then exits BEFORE runtime/codec/socket creation.
//! --single-session-v1 rejects that exact invented public tuple before runtime or
//! socket creation; otherwise admits one SDK session with no retry/replay or
//! endpoint override. SESSION_HEALTHY / healthy=true attest terminal and upload
//! health only, NOT semantic transcription correctness; no text is compared.
#![forbid(unsafe_op_in_unsafe_fn)]
#![cfg_attr(
    not(all(target_os = "linux", target_arch = "x86_64")),
    allow(dead_code)
)]

use bytes::Bytes;
use futures_util::FutureExt;
use rambler_audio::{EncoderConfig, Pcm16Opus};
use rambler_engine::{
    AudioSource, CollectorLimits, Engine, SessionOptions, SourceError, SourceFuture,
    collect_dictation,
};
use rambler_transport::{EnvironmentCredentials, GrpcTransport, Metadata, TransportConfig};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    io::{self, Write},
    sync::{Arc, Mutex},
    time::Duration,
};

const MAGIC: &[u8; 8] = b"RLA1\0\r\n\x1a";
const FIELD_CAP: usize = 4096;
const READY: &[u8] = b"REAL_ADMISSION_READY_V1\n";
const PUBLIC_FIELDS: [&str; 3] = [
    "invented-public-frame-key",
    "invalid.public.frame",
    "invented-public-frame-cert",
];
const NAMES: [&str; 3] = [
    "RAMBLER_API_KEY",
    "RAMBLER_ANDROID_PACKAGE",
    "RAMBLER_ANDROID_CERT",
];
// Full raw s16le mono16k PCM, NOT WAV bytes or a prefix. Provenance:
// docs/evidence/rust-native-natural-public.json inputs[0], sessions[0].pcm;
// docs/research/rust-backend-validation-2026-11-14.md, natural public voices.
// LibriSpeech 5142-33396-0002, OpenSLR12, CC BY4.0; Panayotov et al., ICASSP2015,
// read LibriVox audiobooks. Audio is not embedded or read from an arbitrary path.
const PCM_BYTES: usize = 117440;
const PCM_SAMPLES: u64 = 58720;
const PCM_SHA256: [u8; 32] = [
    0xce, 0xa1, 0xbc, 0x16, 0x14, 0xf7, 0x38, 0x04, 0x8b, 0x1a, 0x4f, 0x1b, 0xca, 0xd8, 0x3f, 0x70,
    0xae, 0xe4, 0x2b, 0x96, 0x21, 0xb8, 0xb7, 0x54, 0xf8, 0xaa, 0xcb, 0xe1, 0x39, 0x5b, 0x08, 0xc9,
];
const SESSION_BUDGET: Duration = Duration::from_secs(20);
const CLOSE_BUDGET: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    PublicFrame,
    SingleSession,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Failure {
    Harden,
    Safety,
    Args,
    Pcm,
    Frame,
    Credentials,
    Io,
    Runtime,
    Internal,
}
impl Failure {
    fn category(self) -> &'static str {
        match self {
            Self::Harden => "BLOCKED_HARDEN",
            Self::Safety => "BLOCKED_SAFETY",
            Self::Args => "BLOCKED_ARGS",
            Self::Pcm => "BLOCKED_PCM",
            Self::Frame => "REJECT_FRAME",
            Self::Credentials => "REJECT_CREDENTIALS",
            Self::Io => "IO_FAILURE",
            Self::Runtime => "RUNTIME_FAILURE",
            Self::Internal => "INTERNAL_FAILURE",
        }
    }
}
fn require(ok: bool, failure: Failure) -> Result<(), Failure> {
    if ok { Ok(()) } else { Err(failure) }
}
fn parse_mode(mut args: impl Iterator<Item = OsString>) -> Option<Mode> {
    args.next()?;
    let mode = match args.next()?.as_encoded_bytes() {
        b"--public-frame-check-v1" => Mode::PublicFrame,
        b"--single-session-v1" => Mode::SingleSession,
        _ => return None,
    };
    args.next().is_none().then_some(mode)
}
fn pcm_valid(bytes: &[u8]) -> bool {
    bytes.len() == PCM_BYTES && Sha256::digest(bytes)[..] == PCM_SHA256
}

// Native is instantiated only by main; libtest uses Fake exclusively.
trait Admission {
    fn harden(&mut self) -> bool;
    fn mode(&mut self) -> Option<Mode>;
    fn safe(&mut self) -> bool;
    // Returns a size/hash-verified public copy; Fake may model that assertion.
    fn public_pcm(&mut self) -> Result<Vec<u8>, Failure>;
    fn release_pcm(&mut self) -> bool;
    fn read(&mut self, bytes: &mut [u8]) -> Result<usize, ()>;
    fn write(&mut self, bytes: &[u8]) -> Result<(), ()>;
}
#[derive(Default)]
struct Output {
    attempts: u8,
    broken: bool,
}
impl Output {
    fn emit(&mut self, sys: &mut impl Admission, bytes: &[u8]) -> Result<(), Failure> {
        require(!self.broken && self.attempts < 2, Failure::Io)?;
        self.attempts += 1;
        self.broken = true; // Never append after a failed/partial write.
        sys.write(bytes).map_err(|_| Failure::Io)?;
        self.broken = false;
        Ok(())
    }
}
fn read_exact(sys: &mut impl Admission, mut bytes: &mut [u8]) -> Result<(), Failure> {
    while !bytes.is_empty() {
        match sys.read(bytes) {
            Ok(n) if n > 0 && n <= bytes.len() => bytes = &mut bytes[n..],
            Ok(0) => return Err(Failure::Frame),
            _ => return Err(Failure::Io),
        }
    }
    Ok(())
}
fn frame(sys: &mut impl Admission) -> Result<[String; 3], Failure> {
    let mut magic = [0; 8];
    read_exact(sys, &mut magic)?;
    require(&magic == MAGIC, Failure::Frame)?;
    let mut fields = [String::new(), String::new(), String::new()];
    for field in &mut fields {
        let mut length = [0; 2];
        read_exact(sys, &mut length)?;
        let length = u16::from_be_bytes(length) as usize;
        require((1..=FIELD_CAP).contains(&length), Failure::Frame)?;
        let mut bytes = vec![0; length];
        read_exact(sys, &mut bytes)?;
        *field = String::from_utf8(bytes).map_err(|_| Failure::Frame)?;
    }
    require(
        sys.read(&mut [0; 1]).map_err(|_| Failure::Io)? == 0,
        Failure::Frame,
    )?;
    Ok(fields)
}
fn snapshot(mut fields: [String; 3]) -> Result<Metadata, Failure> {
    // No getenv, fallback, rewrites or anonymous path. The existing provider
    // performs its own exact three-name, nonempty and metadata validation.
    EnvironmentCredentials
        .required_snapshot_with(|name| {
            let index = NAMES
                .iter()
                .position(|expected| *expected == name)
                .ok_or(std::env::VarError::NotPresent)?;
            Ok(std::mem::take(&mut fields[index]))
        })
        .map_err(|_| Failure::Credentials)
}
struct Admitted {
    pcm: Vec<u8>,
    credentials: Metadata,
    mode: Mode,
}
fn admit(sys: &mut impl Admission, output: &mut Output) -> Result<Admitted, Failure> {
    require(sys.harden(), Failure::Harden)?;
    let mode = sys.mode().ok_or(Failure::Args)?;
    require(sys.safe(), Failure::Safety)?;
    // Hash the bounded, public-only immutable copy BEFORE READY or any secrets.
    let pcm = sys.public_pcm()?;
    require(pcm.len() == PCM_BYTES, Failure::Pcm)?;
    require(sys.safe(), Failure::Safety)?;
    output.emit(sys, READY)?;
    require(sys.safe(), Failure::Safety)?;
    let fields = frame(sys)?;
    require(sys.safe(), Failure::Safety)?;
    let public_fields = fields.iter().map(String::as_str).eq(PUBLIC_FIELDS);
    match mode {
        Mode::PublicFrame => require(public_fields, Failure::Frame)?,
        // Reject only this exact invented tuple, not a policy for real values.
        Mode::SingleSession => require(!public_fields, Failure::Credentials)?,
    }
    let credentials = snapshot(fields)?;
    require(sys.release_pcm(), Failure::Pcm)?;
    Ok(Admitted {
        pcm,
        credentials,
        mode,
    })
}

#[derive(Default)]
struct Receipt {
    bytes: u64,
    eof: bool,
    closes: u64,
}
struct PublicPcm {
    pcm: Bytes,
    offset: usize,
    next_due: Option<tokio::time::Instant>,
    receipt: Arc<Mutex<Receipt>>,
}
impl AudioSource for PublicPcm {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
        Box::pin(async move {
            if self.offset == self.pcm.len() {
                self.receipt.lock().unwrap().eof = true;
                return Ok(None);
            }
            // 320 samples/640 bytes every 20ms. Do not catch up in bursts after
            // backpressure; cancellation before the sleep completes consumes none.
            if let Some(due) = self.next_due {
                tokio::time::sleep_until(due).await;
            }
            let end = (self.offset + 640).min(self.pcm.len());
            let chunk = self.pcm.slice(self.offset..end);
            self.offset = end;
            self.next_due = Some(tokio::time::Instant::now() + Duration::from_millis(20));
            self.receipt.lock().unwrap().bytes += chunk.len() as u64;
            Ok(Some(chunk))
        })
    }
    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        Box::pin(async move {
            self.pcm = Bytes::new();
            self.offset = 0;
            self.next_due = None;
            self.receipt.lock().unwrap().closes += 1;
            Ok(())
        })
    }
}
struct Outcome {
    category: &'static str,
    primary_errors: u64,
    cleanup_errors: u64,
    sessions: u64,
    healthy: bool,
    pcm_bytes: u64,
    pcm_eof: bool,
    source_closed: bool,
}
impl Outcome {
    fn failed(failure: Failure) -> Self {
        Self {
            category: failure.category(),
            primary_errors: 1,
            cleanup_errors: 0,
            sessions: 0,
            healthy: false,
            pcm_bytes: 0,
            pcm_eof: false,
            source_closed: false,
        }
    }
    fn public_frame() -> Self {
        Self {
            category: "PUBLIC_FRAME_CHECKED",
            primary_errors: 0,
            cleanup_errors: 0,
            sessions: 0,
            healthy: false,
            pcm_bytes: 0,
            pcm_eof: false,
            source_closed: false,
        }
    }
    fn success(&self) -> bool {
        self.primary_errors == 0
            && self.cleanup_errors == 0
            && (self.healthy || self.category == "PUBLIC_FRAME_CHECKED")
    }
    fn line(&self) -> String {
        // Closed output schema: fixed category, counts and booleans only. Never
        // print errors, Debug, transcripts, metadata, response IDs or secret hashes.
        format!(
            "REAL_ADMISSION_TERMINAL_V1 {} primary_errors={} cleanup_errors={} sessions={} healthy={} pcm_bytes={} pcm_eof={} source_closed={}\n",
            self.category,
            self.primary_errors,
            self.cleanup_errors,
            self.sessions,
            self.healthy,
            self.pcm_bytes,
            self.pcm_eof,
            self.source_closed
        )
    }
}
async fn session(admitted: Admitted) -> Outcome {
    let mut outcome = Outcome::failed(Failure::Runtime);
    // Default verified TLS + OS native roots + mandatory ALPN h2. No alternate
    // endpoint, plaintext, custom roots, cert verifier, credentials or RPC retry.
    let transport = match GrpcTransport::connect(TransportConfig::default()).await {
        Ok(transport) => transport,
        Err(_) => {
            outcome.category = "CONNECT_FAILURE";
            return outcome;
        }
    };
    let engine = Engine::new(transport.clone());
    let receipt = Arc::new(Mutex::new(Receipt::default()));
    let (source, progress) = Pcm16Opus::new_with_progress(
        PublicPcm {
            pcm: Bytes::from(admitted.pcm),
            offset: 0,
            next_due: None,
            receipt: receipt.clone(),
        },
        EncoderConfig::default(),
    );
    outcome.sessions = 1;
    let result = std::panic::AssertUnwindSafe(tokio::time::timeout(
        SESSION_BUDGET,
        collect_dictation(
            &engine,
            Box::new(source),
            SessionOptions {
                credentials: Some(Arc::new(admitted.credentials)),
                timeout: SESSION_BUDGET,
                ..Default::default()
            },
            CollectorLimits {
                max_final_updates: 32,
                max_result_bytes: 65536,
            },
        ),
    ))
    .catch_unwind()
    .await;
    // Always attempt BOTH owner closures, even after primary/other cleanup error.
    // Preserve distinct observations, not guessed deduplication by error text.
    let engine_closed =
        std::panic::AssertUnwindSafe(tokio::time::timeout(CLOSE_BUDGET, engine.close()))
            .catch_unwind()
            .await;
    let transport_closed =
        std::panic::AssertUnwindSafe(tokio::time::timeout(CLOSE_BUDGET, transport.close()))
            .catch_unwind()
            .await;
    outcome.cleanup_errors = u64::from(!matches!(engine_closed, Ok(Ok(Ok(())))))
        + u64::from(!matches!(transport_closed, Ok(Ok(Ok(())))));
    let pcm = progress.snapshot();
    let Ok(observed) = receipt.lock() else {
        outcome.category = if outcome.cleanup_errors > 0 {
            "SESSION_AND_CLEANUP_FAILURE"
        } else {
            "SESSION_FAILURE"
        };
        return outcome;
    };
    outcome.pcm_bytes = observed.bytes;
    outcome.pcm_eof = observed.eof;
    outcome.source_closed = observed.closes == 1;
    let full_pcm = pcm.input_bytes == PCM_BYTES as u64
        && pcm.encoded_input_samples == PCM_SAMPLES
        && pcm.input_eof
        && pcm.output_eof
        && observed.bytes == PCM_BYTES as u64
        && observed.eof
        && observed.closes == 1;
    // collect_dictation requires unique healthy terminal EOF, S3 END, complete
    // upload/cleanup AND final gRPC OK. No render, text comparison or text output.
    let healthy_terminal = match result {
        Ok(Ok(Ok(result))) => result.metrics.is_some_and(|metrics| {
            metrics.eof_seconds.is_some()
                && metrics.audio.eof
                && metrics.audio.cleanup_finished
                && metrics.audio.cleanup_succeeded
        }),
        _ => false,
    };
    outcome.primary_errors = u64::from(!(healthy_terminal && full_pcm));
    outcome.healthy = healthy_terminal && full_pcm && outcome.cleanup_errors == 0;
    outcome.category = match (outcome.primary_errors > 0, outcome.cleanup_errors > 0) {
        (true, true) => "SESSION_AND_CLEANUP_FAILURE",
        (true, false) => "SESSION_FAILURE",
        (false, true) => "CLEANUP_FAILURE",
        (false, false) => "SESSION_HEALTHY",
    };
    outcome
}
fn requested(
    has_args: bool,
    sys: &mut impl Admission,
    output: &mut Output,
    run: impl FnOnce(Admitted) -> Outcome,
) -> Option<Outcome> {
    if !has_args {
        return None;
    }
    Some(match admit(sys, output) {
        Err(failure) => Outcome::failed(failure),
        Ok(admitted) if admitted.mode == Mode::PublicFrame => Outcome::public_frame(),
        Ok(admitted) => run(admitted),
    })
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod native {
    use super::*;
    use std::{
        fs::{self, File},
        io::Read,
        os::{
            fd::{FromRawFd, IntoRawFd},
            raw::{c_int, c_uint, c_ulong},
            unix::fs::MetadataExt,
        },
    };
    #[repr(C)]
    #[derive(PartialEq, Eq)]
    struct Limit {
        soft: u64,
        hard: u64,
    }
    unsafe extern "C" {
        fn prctl(op: c_int, ...) -> c_int;
        fn getppid() -> c_int;
        fn getpid() -> c_int;
        fn getuid() -> c_uint;
        fn geteuid() -> c_uint;
        fn getgid() -> c_uint;
        fn getegid() -> c_uint;
        fn setrlimit(resource: c_int, limit: *const Limit) -> c_int;
        fn getrlimit(resource: c_int, limit: *mut Limit) -> c_int;
        fn fcntl(fd: c_int, cmd: c_int, ...) -> c_int;
        fn signal(sig: c_int, handler: usize) -> usize;
        fn alarm(seconds: c_uint) -> c_uint;
        fn read(fd: c_int, bytes: *mut u8, count: usize) -> isize;
        fn close(fd: c_int) -> c_int;
    }
    fn pr(op: c_int, value: c_ulong) -> c_int {
        // SAFETY: Linux x86_64 ABI; all five arguments are integer values.
        unsafe { prctl(op, value, 0 as c_ulong, 0 as c_ulong, 0 as c_ulong) }
    }
    fn bounded(path: &str, cap: usize) -> Option<Vec<u8>> {
        let mut bytes = Vec::new();
        File::open(path)
            .ok()?
            .take(cap as u64 + 1)
            .read_to_end(&mut bytes)
            .ok()?;
        (bytes.len() <= cap).then_some(bytes)
    }
    fn core_zero() -> bool {
        let mut got = Limit {
            soft: u64::MAX,
            hard: u64::MAX,
        };
        // SAFETY: Linux x86_64 RLIMIT_CORE=4; valid writable rlimit pointer.
        (unsafe { getrlimit(4, &mut got) == 0 }) && got == Limit { soft: 0, hard: 0 }
    }
    pub struct Native {
        parent: c_int,
        file: Option<File>,
    }
    impl Native {
        pub fn new() -> Self {
            Self {
                parent: 0,
                file: None,
            }
        }
    }
    impl Admission for Native {
        fn harden(&mut self) -> bool {
            // PR_SET_DUMPABLE=4 / GET=3. No positive dumpable branch exists.
            if pr(4, 0) != 0 || pr(3, 0) != 0 {
                return false;
            }
            let zero = Limit { soft: 0, hard: 0 };
            // SAFETY: Linux rlimit ABI; pointer valid for the duration of the call.
            if unsafe { setrlimit(4, &zero) } != 0 || !core_zero() {
                return false;
            }
            // SAFETY: getppid has no preconditions. Set/verify before receiving.
            self.parent = unsafe { getppid() };
            if self.parent <= 1 || pr(1, 9) != 0 || unsafe { getppid() } != self.parent {
                return false;
            }
            if pr(38, 1) != 0 || pr(39, 0) != 1 {
                return false;
            } // NO_NEW_PRIVS
            // SIGALRM: fixed 90s total wall bound, no core. ABRT: default action.
            // Reject inherited ignored handlers instead of silently accepting them.
            // SAFETY: Linux signal ABI; SIG_DFL=0 and SIG_ERR=usize::MAX.
            unsafe {
                if signal(14, 0) != 0 || signal(6, 0) != 0 {
                    return false;
                }
                alarm(90);
            }
            true
        }
        fn mode(&mut self) -> Option<Mode> {
            parse_mode(std::env::args_os())
        }
        fn safe(&mut self) -> bool {
            let mut death_signal: c_int = 0;
            // SAFETY: PR_GET_PDEATHSIG=2 writes one c_int through this pointer.
            let death_ok = unsafe {
                prctl(
                    2,
                    &mut death_signal as *mut c_int,
                    0 as c_ulong,
                    0 as c_ulong,
                    0 as c_ulong,
                ) == 0
            };
            if !death_ok
                || death_signal != 9
                || unsafe { getppid() } != self.parent
                || pr(3, 0) != 0
                || !core_zero()
                || pr(39, 0) != 1
            {
                return false;
            }
            let Some(status) = bounded("/proc/self/status", 65536) else {
                return false;
            };
            // SAFETY: uid/gid accessors have no preconditions.
            let (uid, euid, gid, egid) = unsafe { (getuid(), geteuid(), getgid(), getegid()) };
            uid != 0
                && gid != 0
                && uid == euid
                && gid == egid
                && status_valid(&status, uid, gid)
                && policy()
                && env_valid(std::env::vars_os())
                && descriptors(uid)
        }
        fn public_pcm(&mut self) -> Result<Vec<u8>, Failure> {
            require(self.file.is_none(), Failure::Pcm)?;
            // SAFETY: fd3 has been validated as a readable owned regular file;
            // this is its unique Rust owner. No other code closes it until release.
            let mut file = unsafe { File::from_raw_fd(3) };
            use std::io::{Seek, SeekFrom};
            let mut bytes = Vec::new();
            let read_ok = file.seek(SeekFrom::Start(0)).is_ok()
                && (&mut file)
                    .take(PCM_BYTES as u64 + 1)
                    .read_to_end(&mut bytes)
                    .is_ok();
            self.file = Some(file);
            require(read_ok && pcm_valid(&bytes), Failure::Pcm)?;
            Ok(bytes)
        }
        fn release_pcm(&mut self) -> bool {
            let Some(file) = self.file.take() else {
                return false;
            };
            // SAFETY: unique owner transferred to close; never retry close after
            // an error (Linux may already have released/reused the descriptor).
            unsafe { close(file.into_raw_fd()) == 0 }
        }
        fn read(&mut self, bytes: &mut [u8]) -> Result<usize, ()> {
            // SAFETY: exact writable slice, fd0 only, no buffered read-ahead.
            let count = unsafe { read(0, bytes.as_mut_ptr(), bytes.len()) };
            if count < 0 {
                Err(())
            } else {
                Ok(count as usize)
            }
        }
        fn write(&mut self, bytes: &[u8]) -> Result<(), ()> {
            write_stdout(bytes)
        }
    }
    fn policy() -> bool {
        // Exact reviewed G1 policy. No global policy is changed by this example.
        [
            ("/proc/sys/kernel/core_pattern", b"core\n".as_slice()),
            ("/proc/sys/kernel/core_uses_pid", b"1\n".as_slice()),
            ("/proc/sys/fs/suid_dumpable", b"0\n".as_slice()),
            ("/proc/sys/kernel/yama/ptrace_scope", b"0\n".as_slice()),
        ]
        .iter()
        .all(|(path, expected)| bounded(path, 128).as_deref() == Some(*expected))
    }
    fn descriptors(uid: u32) -> bool {
        // F_GETFL=3; access mode bits0..1. Reject O_PATH/nonblocking descriptors.
        for (fd, access) in [(0, 0), (1, 1), (2, 1), (3, 0)] {
            let flags = unsafe { fcntl(fd, 3) };
            // fd0 may be a bidirectional socket, never a regular/terminal input.
            if flags < 0
                || flags & (0x200000 | 0x800) != 0
                || if fd == 0 {
                    flags & 3 != 0 && flags & 3 != 2
                } else {
                    flags & 3 != access
                }
            {
                return false;
            }
        }
        let metadata = |fd| fs::metadata(format!("/proc/self/fd/{fd}"));
        let (Ok(input), Ok(output), Ok(error), Ok(pcm)) =
            (metadata(0), metadata(1), metadata(2), metadata(3))
        else {
            return false;
        };
        let kind = |mode: u32| mode & 0o170000;
        if !matches!(kind(input.mode()), 0o010000 | 0o140000)
            || kind(output.mode()) != 0o010000
            || kind(error.mode()) != 0o020000
            || error.rdev() != 259
            || !pcm.is_file()
            || pcm.uid() != uid
            || pcm.len() != PCM_BYTES as u64
            || pcm.mode() & 0o022 != 0
        {
            return false;
        }
        if fs::read_link("/proc/self/fd/2").ok().as_deref()
            != Some(std::path::Path::new("/dev/null"))
        {
            return false;
        }
        // With 0..3 present, read_dir MUST occupy 4. Reject a preexisting 4 and
        // any other descriptor; verify that 4 really is our proc directory.
        if unsafe { fcntl(4, 1) } != -1 {
            return false;
        } // F_GETFD=1
        let Ok(entries) = fs::read_dir("/proc/self/fd") else {
            return false;
        };
        let mut names = Vec::new();
        // Keep iterator alive while checking its fd4 symlink.
        let directory = format!("/proc/{}/fd", unsafe { getpid() });
        if fs::read_link("/proc/self/fd/4").ok().as_deref()
            != Some(std::path::Path::new(&directory))
        {
            return false;
        }
        for entry in entries {
            let Ok(entry) = entry else { return false };
            names.push(entry.file_name());
        }
        names.sort();
        names == ["0", "1", "2", "3", "4"].map(OsString::from)
    }
}
fn field<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let mut values = text.lines().filter_map(|line| line.strip_prefix(key));
    let value = values.next()?.trim();
    values.next().is_none().then_some(value)
}
fn signal_mask(value: &str) -> Option<u64> {
    (value.len() == 16 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| u64::from_str_radix(value, 16).ok())
        .flatten()
}
fn status_valid(bytes: &[u8], uid: u32, gid: u32) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    for (key, id) in [("Uid:", uid), ("Gid:", gid)] {
        let Some(value) = field(text, key) else {
            return false;
        };
        let values: Vec<_> = value.split_whitespace().collect();
        if values.len() != 4 || values.iter().any(|value| *value != id.to_string()) {
            return false;
        }
    }
    field(text, "TracerPid:") == Some("0") && field(text, "Threads:") == Some("1")
        && field(text, "NoNewPrivs:") == Some("1")
        && ["SigBlk:", "SigPnd:", "ShdPnd:"].iter()
            .all(|key| field(text, key) == Some("0000000000000000"))
        // Rust startup may ignore SIGPIPE and install BUS/SEGV overflow handlers.
        // No other ignored/caught signals are admitted, especially ABRT/ALRM.
        && field(text, "SigIgn:").and_then(signal_mask)
            .is_some_and(|bits| bits & !0x1000 == 0)
        && field(text, "SigCgt:").and_then(signal_mask)
            .is_some_and(|bits| bits & !0x440 == 0)
        && ["CapInh:", "CapPrm:", "CapEff:", "CapAmb:"].iter()
            .all(|key| field(text, key) == Some("0000000000000000"))
}
fn env_valid(values: impl IntoIterator<Item = (OsString, OsString)>) -> bool {
    let mut values: Vec<_> = values.into_iter().collect();
    values.sort();
    let mut expected = [
        (OsString::from("PATH"), OsString::from("/usr/bin:/bin")),
        (OsString::from("LC_ALL"), OsString::from("C")),
    ];
    expected.sort();
    values.as_slice() == expected
}
fn write_stdout(bytes: &[u8]) -> Result<(), ()> {
    let mut stdout = io::stdout().lock();
    stdout
        .write_all(bytes)
        .and_then(|()| stdout.flush())
        .map_err(|_| ())
}
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn main() -> std::process::ExitCode {
    // Default no-args path has no hooks, hardening, output or runtime side effect.
    if std::env::args_os().nth(1).is_none() {
        return std::process::ExitCode::SUCCESS;
    }
    std::panic::set_hook(Box::new(|_| {}));
    let mut sys = native::Native::new();
    let mut output = Output::default();
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        requested(true, &mut sys, &mut output, |admitted| {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(_) => return Outcome::failed(Failure::Runtime),
            };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                runtime.block_on(session(admitted))
            }));
            let mut outcome = result.unwrap_or_else(|_| {
                let mut failed = Outcome::failed(Failure::Internal);
                failed.cleanup_errors = 1; // Owner cleanup was not established.
                failed.category = "SESSION_AND_CLEANUP_FAILURE";
                failed
            });
            // Runtime Drop joins remaining blocking work. The native alarm still
            // bounds total wall time. Output success ONLY AFTER that join/drop.
            let stopped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(runtime)));
            if stopped.is_err() {
                outcome.cleanup_errors += 1;
                outcome.healthy = false;
                outcome.category = if outcome.primary_errors > 0 {
                    "SESSION_AND_CLEANUP_FAILURE"
                } else {
                    "CLEANUP_FAILURE"
                };
            }
            outcome
        })
    }))
    .unwrap_or_else(|_| Some(Outcome::failed(Failure::Internal)));
    let Some(outcome) = outcome else {
        return std::process::ExitCode::SUCCESS;
    };
    let ok = outcome.success();
    let written = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        output.emit(&mut sys, outcome.line().as_bytes())
    }));
    if ok && matches!(written, Ok(Ok(()))) {
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::from(1)
    }
}
#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
fn main() -> std::process::ExitCode {
    if std::env::args_os().nth(1).is_none() {
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::from(1)
    }
}

#[cfg(test)]
#[path = "support/linux_real_admission_tests.rs"]
mod tests;
