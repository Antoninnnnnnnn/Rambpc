// Pure deterministic tests only. No Native, syscall, runtime, codec, real env,
// actual PCM, process or socket is instantiated here.
use super::*;

struct Fake {
    input: Vec<u8>,
    offset: usize,
    read_cap: usize,
    reads: usize,
    mode: Option<Mode>,
    harden_ok: bool,
    safe_fail_at: Option<usize>,
    safe_calls: usize,
    pcm_ok: bool,
    release_ok: bool,
    write_fail_at: Option<usize>,
    writes: Vec<Vec<u8>>,
    events: Vec<&'static str>,
}
impl Fake {
    fn new(input: Vec<u8>) -> Self {
        Self {
            input,
            offset: 0,
            read_cap: usize::MAX,
            reads: 0,
            mode: Some(Mode::PublicFrame),
            harden_ok: true,
            safe_fail_at: None,
            safe_calls: 0,
            pcm_ok: true,
            release_ok: true,
            write_fail_at: None,
            writes: Vec::new(),
            events: Vec::new(),
        }
    }
}
impl Admission for Fake {
    fn harden(&mut self) -> bool {
        self.events.push("harden");
        self.harden_ok
    }
    fn mode(&mut self) -> Option<Mode> {
        self.events.push("mode");
        self.mode
    }
    fn safe(&mut self) -> bool {
        self.events.push("safe");
        self.safe_calls += 1;
        self.safe_fail_at != Some(self.safe_calls)
    }
    fn public_pcm(&mut self) -> Result<Vec<u8>, Failure> {
        self.events.push("verified_public_pcm");
        // Model a successful hash check, never load or synthesize the actual PCM.
        if self.pcm_ok {
            Ok(vec![0; PCM_BYTES])
        } else {
            Err(Failure::Pcm)
        }
    }
    fn release_pcm(&mut self) -> bool {
        self.events.push("release");
        self.release_ok
    }
    fn read(&mut self, bytes: &mut [u8]) -> Result<usize, ()> {
        self.events.push("read");
        self.reads += 1;
        let count = bytes
            .len()
            .min(self.read_cap)
            .min(self.input.len() - self.offset);
        bytes[..count].copy_from_slice(&self.input[self.offset..self.offset + count]);
        self.offset += count;
        Ok(count)
    }
    fn write(&mut self, bytes: &[u8]) -> Result<(), ()> {
        self.events.push("write");
        self.writes.push(bytes.to_vec());
        if self.write_fail_at == Some(self.writes.len()) {
            Err(())
        } else {
            Ok(())
        }
    }
}
fn encoded(fields: [&[u8]; 3]) -> Vec<u8> {
    let mut bytes = MAGIC.to_vec();
    for field in fields {
        bytes.extend_from_slice(&(field.len() as u16).to_be_bytes());
        bytes.extend_from_slice(field);
    }
    bytes
}
fn public_frame() -> Vec<u8> {
    encoded(PUBLIC_FIELDS.map(str::as_bytes))
}
fn unused(_: Admitted) -> Outcome {
    panic!("runtime boundary must not be crossed")
}

#[test]
fn default_does_absolutely_no_admission_work() {
    let mut sys = Fake::new(public_frame());
    let mut output = Output::default();
    assert!(requested(false, &mut sys, &mut output, unused).is_none());
    assert!(sys.events.is_empty());
    assert_eq!(output.attempts, 0);
}
#[test]
fn public_mode_requires_eof_validates_and_never_calls_runtime() {
    let mut sys = Fake::new(public_frame());
    sys.read_cap = 1;
    let mut output = Output::default();
    let outcome = requested(true, &mut sys, &mut output, unused).unwrap();
    assert_eq!(outcome.category, "PUBLIC_FRAME_CHECKED");
    assert!(!outcome.healthy);
    assert_eq!(outcome.sessions, 0);
    assert!(outcome.success());
    assert_eq!(sys.reads, sys.input.len() + 1);
    assert_eq!(sys.writes, [READY.to_vec()]);
    assert_eq!(
        &sys.events[..7],
        &[
            "harden",
            "mode",
            "safe",
            "verified_public_pcm",
            "safe",
            "write",
            "safe"
        ]
    );
    assert_eq!(&sys.events[sys.events.len() - 2..], &["safe", "release"]);
}
#[test]
fn each_pre_ready_gate_stops_before_read_or_runtime() {
    for gate in 0..4 {
        let mut sys = Fake::new(public_frame());
        match gate {
            0 => sys.harden_ok = false,
            1 => sys.mode = None,
            2 => sys.safe_fail_at = Some(1),
            _ => sys.pcm_ok = false,
        }
        let mut output = Output::default();
        let outcome = requested(true, &mut sys, &mut output, unused).unwrap();
        assert!(!outcome.success());
        assert_eq!(sys.reads, 0);
        assert!(sys.writes.is_empty());
    }
}
#[test]
fn repeated_safety_gates_are_not_skipped() {
    for gate in 2..=4 {
        let mut sys = Fake::new(public_frame());
        sys.safe_fail_at = Some(gate);
        let mut output = Output::default();
        let outcome = requested(true, &mut sys, &mut output, unused).unwrap();
        assert_eq!(outcome.category, "BLOCKED_SAFETY");
        assert!(!sys.events.contains(&"release"));
        if gate <= 3 {
            assert_eq!(sys.reads, 0);
        }
    }
}
#[test]
fn ready_write_failure_prevents_all_reads_and_followup_output() {
    let mut sys = Fake::new(public_frame());
    sys.write_fail_at = Some(1);
    let mut output = Output::default();
    let outcome = requested(true, &mut sys, &mut output, unused).unwrap();
    assert_eq!(outcome.category, "IO_FAILURE");
    assert_eq!(sys.reads, 0);
    assert!(output.emit(&mut sys, outcome.line().as_bytes()).is_err());
    assert_eq!(sys.writes.len(), 1);
}
#[test]
fn all_truncations_and_trailing_data_fail_closed() {
    let valid = public_frame();
    for end in 0..valid.len() {
        assert!(frame(&mut Fake::new(valid[..end].to_vec())).is_err());
    }
    let mut trailing = valid;
    trailing.push(0);
    assert!(frame(&mut Fake::new(trailing)).is_err());
}
#[test]
fn bad_magic_empty_oversize_and_non_utf8_are_rejected() {
    let mut wrong = public_frame();
    wrong[0] ^= 1;
    assert!(frame(&mut Fake::new(wrong)).is_err());
    assert!(frame(&mut Fake::new(encoded([b"", b"b", b"c"]))).is_err());
    assert!(frame(&mut Fake::new(encoded([b"\xff", b"b", b"c"]))).is_err());
    let mut oversize = MAGIC.to_vec();
    oversize.extend_from_slice(&((FIELD_CAP + 1) as u16).to_be_bytes());
    let mut sys = Fake::new(oversize);
    assert!(matches!(frame(&mut sys), Err(Failure::Frame)));
    assert_eq!(sys.offset, 10); // No oversized allocation or payload read.
}
#[test]
fn cap_is_inclusive_and_provider_rejects_control_metadata() {
    let field = vec![b'a'; FIELD_CAP];
    let fields = frame(&mut Fake::new(encoded([&field, &field, &field]))).unwrap();
    assert!(snapshot(fields).is_ok());
    let fields = frame(&mut Fake::new(encoded([b"a\nb", b"pkg", b"cert"]))).unwrap();
    assert!(matches!(snapshot(fields), Err(Failure::Credentials)));
    assert!(matches!(
        snapshot([String::new(), "a".into(), "b".into()]),
        Err(Failure::Credentials)
    ));
}
#[test]
fn public_mode_does_not_accept_other_credential_frames() {
    let mut sys = Fake::new(encoded([b"invented-other", b"pkg", b"cert"]));
    let outcome = requested(true, &mut sys, &mut Output::default(), unused).unwrap();
    assert_eq!(outcome.category, "REJECT_FRAME");
    assert!(!outcome.success());
}
#[test]
fn single_mode_rejects_exact_public_tuple_before_runtime() {
    let mut sys = Fake::new(public_frame());
    sys.mode = Some(Mode::SingleSession);
    let outcome = requested(true, &mut sys, &mut Output::default(), unused).unwrap();
    assert_eq!(outcome.category, "REJECT_CREDENTIALS");
    assert!(!outcome.success());
    assert_eq!(outcome.sessions, 0);
    assert_eq!(sys.reads, 8); // Magic, three length/payload pairs, mandatory EOF.
    assert_eq!(sys.offset, sys.input.len());
    assert_eq!(sys.writes, [READY.to_vec()]);
    assert!(!sys.events.contains(&"release"));
}
#[test]
fn single_mode_crosses_fake_runtime_boundary_once_only_after_eof() {
    // Changing any one field is not this exact tuple; existing validation applies.
    for index in 0..3 {
        let mut fields = PUBLIC_FIELDS;
        fields[index] = "invented-other";
        let mut sys = Fake::new(encoded(fields.map(str::as_bytes)));
        sys.mode = Some(Mode::SingleSession);
        let mut calls = 0;
        let outcome = requested(true, &mut sys, &mut Output::default(), |admitted| {
            calls += 1;
            assert_eq!(admitted.mode, Mode::SingleSession);
            Outcome::failed(Failure::Runtime)
        })
        .unwrap();
        assert_eq!(calls, 1);
        assert_eq!(sys.offset, sys.input.len());
        assert_eq!(sys.events.last(), Some(&"release"));
        assert!(!outcome.success());
    }
}
#[test]
fn release_failure_never_crosses_runtime_boundary() {
    let mut sys = Fake::new(encoded([b"invented-other", b"pkg", b"cert"]));
    sys.mode = Some(Mode::SingleSession);
    sys.release_ok = false;
    let outcome = requested(true, &mut sys, &mut Output::default(), unused).unwrap();
    assert_eq!(outcome.category, "BLOCKED_PCM");
}
#[test]
fn args_and_exact_clear_environment_only() {
    let args = |xs: &[&str]| {
        xs.iter()
            .map(|value| OsString::from(*value))
            .collect::<Vec<_>>()
            .into_iter()
    };
    assert_eq!(
        parse_mode(args(&["binary", "--public-frame-check-v1"])),
        Some(Mode::PublicFrame)
    );
    assert_eq!(
        parse_mode(args(&["binary", "--single-session-v1"])),
        Some(Mode::SingleSession)
    );
    for xs in [
        &["binary"][..],
        &["binary", "--other"],
        &["binary", "--single-session-v1", "extra"],
    ] {
        assert!(parse_mode(args(xs)).is_none());
    }
    let clear = || {
        [("PATH", "/usr/bin:/bin"), ("LC_ALL", "C")]
            .map(|(key, value)| (OsString::from(key), OsString::from(value)))
            .to_vec()
    };
    assert!(env_valid(clear()));
    let mut extra = clear();
    extra.push(("SSL_CERT_FILE".into(), "invented".into()));
    assert!(!env_valid(extra));
    assert!(!env_valid(Vec::new()));
    let mut duplicate = clear();
    duplicate.push(("LC_ALL".into(), "C".into()));
    assert!(!env_valid(duplicate));
}
fn status() -> String {
    "Uid:\t1000 1000 1000 1000\nGid:\t1001 1001 1001 1001\nTracerPid:\t0\nThreads:\t1\nNoNewPrivs:\t1\nSigPnd:\t0000000000000000\nShdPnd:\t0000000000000000\nSigBlk:\t0000000000000000\nSigIgn:\t0000000000001000\nSigCgt:\t0000000000000440\nCapInh:\t0000000000000000\nCapPrm:\t0000000000000000\nCapEff:\t0000000000000000\nCapAmb:\t0000000000000000\n".into()
}
#[test]
fn pure_status_rejects_tracing_caps_signals_ids_threads_and_duplicates() {
    let valid = status();
    assert!(status_valid(valid.as_bytes(), 1000, 1001));
    for (from, to) in [
        ("TracerPid:\t0", "TracerPid:\t12"),
        ("Threads:\t1", "Threads:\t2"),
        ("1000 1000 1000 1000", "1000 0 1000 1000"),
        ("CapEff:\t0000000000000000", "CapEff:\t0000000000000001"),
        ("SigBlk:\t0000000000000000", "SigBlk:\t0000000000002000"),
        ("SigPnd:\t0000000000000000", "SigPnd:\t0000000000000001"),
        ("ShdPnd:\t0000000000000000", "ShdPnd:\t0000000000000001"),
        ("SigIgn:\t0000000000001000", "SigIgn:\t1000"),
        ("SigIgn:\t0000000000001000", "SigIgn:\t0000000000003000"),
        ("SigCgt:\t0000000000000440", "SigCgt:\t0000000000002440"),
        ("NoNewPrivs:\t1", "NoNewPrivs:\t0"),
    ] {
        assert!(!status_valid(
            valid.replace(from, to).as_bytes(),
            1000,
            1001
        ));
    }
    assert!(!status_valid(
        format!("{valid}TracerPid:\t0\n").as_bytes(),
        1000,
        1001
    ));
    assert!(!status_valid(b"\xff", 1000, 1001));
}
#[test]
fn fixed_pcm_constants_match_public_report_but_no_pcm_is_loaded() {
    assert_eq!(PCM_BYTES as u64, PCM_SAMPLES * 2);
    assert_eq!(
        hex::encode(PCM_SHA256),
        "cea1bc1614f738048b1a4f1bcad83f70aee42b9621b8b754f8aacbe1395b08c9"
    );
    assert!(!pcm_valid(&[]));
    assert!(!pcm_valid(&vec![0; PCM_BYTES]));
}
#[test]
fn output_allows_only_ready_and_one_terminal_record() {
    let mut sys = Fake::new(Vec::new());
    let mut output = Output::default();
    assert!(output.emit(&mut sys, READY).is_ok());
    assert!(
        output
            .emit(&mut sys, Outcome::public_frame().line().as_bytes())
            .is_ok()
    );
    assert!(output.emit(&mut sys, READY).is_err());
    assert_eq!(sys.writes.len(), 2);
    let mut failed = Output::default();
    let mut sys = Fake::new(Vec::new());
    sys.write_fail_at = Some(2);
    assert!(failed.emit(&mut sys, READY).is_ok());
    assert!(
        failed
            .emit(&mut sys, Outcome::failed(Failure::Frame).line().as_bytes())
            .is_err()
    );
    assert!(failed.emit(&mut sys, READY).is_err());
    assert_eq!(sys.writes.len(), 2);
}
#[test]
fn cleanup_failure_prevents_success_and_preserves_primary_count() {
    let mut outcome = Outcome::failed(Failure::Runtime);
    outcome.primary_errors = 1;
    outcome.cleanup_errors = 2;
    assert!(!outcome.success());
    let line = outcome.line();
    assert!(line.contains("primary_errors=1 cleanup_errors=2"));
    outcome.primary_errors = 0;
    outcome.healthy = true;
    assert!(!outcome.success());
}
