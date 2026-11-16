use super::*;
use std::{cell::Cell, ffi::OsString};

// No native Admission methods, process inspection, or dumpability mutation here.
struct Fake {
    calls: Vec<&'static str>,
    fail: Option<usize>,
    input: Vec<u8>,
    used: usize,
    requested: usize,
    eof: usize,
    output: Vec<u8>,
    writes: usize,
    write_failure: Option<usize>,
    read_error: bool,
    oversize_read: bool,
}
impl Fake {
    fn new(input: &[u8]) -> Self {
        Self {
            calls: vec![],
            fail: None,
            input: input.to_vec(),
            used: 0,
            requested: 0,
            eof: 0,
            output: vec![],
            writes: 0,
            write_failure: None,
            read_error: false,
            oversize_read: false,
        }
    }
    fn check(&mut self, stage: &'static str) -> bool {
        self.calls.push(stage);
        self.fail != Some(self.calls.len())
    }
    fn bounded(&self) {
        assert!(self.requested <= 27);
        assert!(self.output.len() <= 64);
        assert!(self.output.iter().filter(|b| **b == b'\n').count() <= 2);
        assert!(self.writes <= 2);
    }
}
impl Admission for Fake {
    fn harden(&mut self) -> bool {
        self.check("harden")
    }
    fn dumpable(&mut self) -> i32 {
        if self.check("dumpable") { 0 } else { 1 }
    }
    fn untraced(&mut self) -> bool {
        self.check("tracer")
    }
    fn fds(&mut self) -> bool {
        self.check("fds")
    }
    fn env(&mut self) -> bool {
        self.check("env")
    }
    fn args(&mut self) -> bool {
        self.check("args")
    }
    fn read(&mut self, byte: &mut [u8]) -> Result<usize, ()> {
        assert_eq!(byte.len(), 1);
        self.calls.push("read");
        self.requested += byte.len();
        assert!(self.requested <= 27);
        if self.read_error {
            return Err(());
        }
        if self.oversize_read {
            return Ok(2);
        }
        if self.used == self.input.len() {
            self.eof += 1;
            return Ok(0);
        }
        byte[0] = self.input[self.used];
        self.used += 1;
        Ok(1)
    }
    fn write(&mut self, line: &'static [u8]) -> Result<(), ()> {
        self.writes += 1;
        self.calls
            .push(if line == READY { "ready" } else { "terminal" });
        if self.write_failure == Some(self.writes) {
            return Err(());
        }
        self.output.extend_from_slice(line);
        Ok(())
    }
}
fn drive(fake: &mut Fake, has_args: bool, action: impl FnOnce(Admitted) -> Check) -> Check {
    let mut output = Output::default();
    let result = requested(has_args, fake, &mut output, action);
    if let Err(failure) = result {
        let _ = output.emit(fake, failure.line());
    }
    fake.bounded();
    result
}
#[test]
fn default_noop_never_enters_admission_or_sdk() {
    let mut fake = Fake::new(CANARY);
    assert_eq!(
        drive(&mut fake, false, |_| panic!("no source/provider/socket")),
        Ok(())
    );
    assert!(fake.calls.is_empty());
    assert!(fake.output.is_empty());
}
#[test]
fn admission_failures_precede_all_source_provider_and_socket_actions() {
    let failures = [
        Failure::Harden,
        Failure::Verify,
        Failure::Traced,
        Failure::Fd,
        Failure::Env,
        Failure::Argv,
    ];
    for (index, failure) in failures.into_iter().enumerate() {
        let mut fake = Fake::new(CANARY);
        fake.fail = Some(index + 1);
        assert_eq!(
            drive(&mut fake, true, |_| panic!("no source/provider/socket")),
            Err(failure)
        );
        assert_eq!(fake.requested, 0);
        assert_eq!(fake.output, failure.line());
        assert_eq!(fake.calls.len(), index + 2); // failed check + one fixed result
    }
    let mut fake = Fake::new(CANARY);
    fake.fail = Some(8); // ready is call 7, recheck is call 8
    assert_eq!(
        drive(&mut fake, true, |_| panic!("no source/provider/socket")),
        Err(Failure::Verify)
    );
    assert_eq!(fake.requested, 0);
    assert_eq!(fake.output, [READY, Failure::Verify.line()].concat());
}
#[test]
fn exact_canary_and_eof_are_required_before_action() {
    let mut inputs: Vec<Vec<u8>> = (0..CANARY.len()).map(|n| CANARY[..n].to_vec()).collect();
    inputs.extend([
        b"RAMBLER-PRECRED-CANARY-V1\r\n".to_vec(),
        [CANARY.as_slice(), b"X"].concat(),
        vec![0; 4096],
        b"x-goog-api-key: invented\n".to_vec(),
    ]);
    for input in inputs {
        let mut fake = Fake::new(&input);
        assert_eq!(
            drive(&mut fake, true, |_| panic!("no source/provider/socket")),
            Err(Failure::Payload)
        );
        assert_eq!(fake.used, input.len().min(27));
        assert_eq!(fake.output, [READY, Failure::Payload.line()].concat());
    }
    let calls = Cell::new(0);
    let mut fake = Fake::new(CANARY);
    assert_eq!(
        drive(&mut fake, true, |_| {
            calls.set(calls.get() + 1);
            Ok(())
        }),
        Ok(())
    );
    assert_eq!(calls.get(), 1);
    assert_eq!(fake.requested, 27);
    assert_eq!(fake.eof, 1);
    assert_eq!(
        &fake.calls[..8],
        [
            "harden", "dumpable", "tracer", "fds", "env", "args", "ready", "dumpable"
        ]
    );
    assert_eq!(fake.output, [READY, OK].concat());
}
#[test]
fn io_failure_and_failed_action_cannot_emit_success() {
    for mode in 0..5 {
        let mut fake = Fake::new(CANARY);
        fake.read_error = mode == 0;
        fake.oversize_read = mode == 1;
        fake.write_failure = match mode {
            2 => Some(1),
            3 => Some(2),
            _ => None,
        };
        let calls = Cell::new(0);
        let result = drive(&mut fake, true, |_| {
            calls.set(calls.get() + 1);
            if mode == 4 { Err(Failure::Sdk) } else { Ok(()) }
        });
        assert!(result.is_err());
        assert_eq!(calls.get(), usize::from(mode >= 3));
        assert!(!fake.output.windows(OK.len()).any(|window| window == OK));
        if mode == 2 {
            assert_eq!(fake.requested, 0);
        }
    }
}
#[test]
fn fixed_admission_parsers_fail_closed_in_memory() {
    for key in [
        "WSLENV",
        "RAMBLER_",
        "RAMBLER_API_KEY",
        "UNRELATED_SECRET",
        "LD_PRELOAD",
        "path",
    ] {
        assert!(!env_key_allowed(OsStr::new(key)));
    }
    for key in ["PATH", "LC_ALL"] {
        assert!(env_key_allowed(OsStr::new(key)));
    }
    for (args, valid) in [
        (vec!["example", MODE], true),
        (vec!["example"], false),
        (vec!["example", MODE, "extra"], false),
        (vec!["example", "--endpoint"], false),
    ] {
        assert_eq!(args_valid(args.into_iter().map(OsString::from)), valid);
    }
    assert!(tracer_zero(b"Name: fixed\nTracerPid:\t0\n"));
    for status in [
        b"".as_slice(),
        b"TracerPid: 1\n",
        b"TracerPid: 00\n",
        b"TracerPid: 0\nTracerPid: 0\n",
    ] {
        assert!(!tracer_zero(status));
    }
    assert!(fd_names_valid(["0", "1", "2", "3"].map(|n| Ok(n.into()))));
    for names in [
        vec!["0", "1", "2"],
        vec!["0", "1", "2", "3", "4"],
        vec!["0", "1", "2", "03"],
        vec!["0", "1", "2", "3", "3"],
    ] {
        assert!(!fd_names_valid(names.into_iter().map(|n| Ok(n.into()))));
    }
    assert!(!fd_names_valid([Err(())]));
    for failure in [
        Failure::Harden,
        Failure::Verify,
        Failure::Traced,
        Failure::Fd,
        Failure::Env,
        Failure::Argv,
        Failure::Io,
        Failure::Payload,
        Failure::Sdk,
        Failure::Internal,
    ] {
        assert!(READY.len() + failure.line().len() <= 64);
    }
    assert_eq!(CANARY.len(), 26);
    assert_eq!(READY.len() + OK.len(), 38);
}
#[test]
fn request_order_and_corrupt_ogg_reject_in_memory() {
    let init = Bytes::from(initial_request(&ProtocolConfig::default(), 0).unwrap());
    let audio = Bytes::from(audio_request(b"not an Ogg stream", 0).unwrap());
    let context = Bytes::from(context_request(&fixed_context()).unwrap());
    let end = Bytes::copy_from_slice(&end_request());
    let correct = vec![init, audio, context, end];
    let ogg = validate_requests(&correct).unwrap();
    assert_eq!(validate_ogg(&ogg), Err(Failure::Sdk));
    for order in [[0, 2, 1, 3], [0, 1, 3, 2], [0, 1, 2, 1]] {
        let wrong = order.map(|index| correct[index].clone());
        assert!(validate_requests(&wrong).is_err());
    }
    let mut after_end = correct.clone();
    after_end.push(correct[1].clone());
    assert!(validate_requests(&after_end).is_err());
}
#[test]
fn fixed_source_has_no_real_receiver_or_arbitrary_endpoint() {
    let source = include_str!("../linux_guarded_loopback.rs");
    let compact: String = source.split_whitespace().collect();
    for forbidden in [
        "EnvironmentCredentials",
        "TransportConfig::default",
        "TransportConfig::tls",
        "std::process::Command",
        "std::env::var(",
        "std::env::var_os(",
        "WavAudio",
        "File::create",
        "println!",
        "eprintln!",
        "PR_SET_DUMPABLE,1",
    ] {
        assert!(!compact.contains(forbidden));
    }
    assert!(compact.contains("PR_SET_DUMPABLE,0aslibc::c_ulong"));
    assert_eq!(source.matches("libc::read(").count(), 1);
    assert_eq!(source.matches("libc::prctl(").count(), 2);
    // Tripwire only: not a security proof or a substitute for native qualification.
}
#[tokio::test]
async fn real_sdk_fixed_pcm_opus_loopback_is_healthy_only_after_cleanup_and_close() {
    let mut fake = Fake::new(CANARY);
    let mut output = Output::default();
    let admitted = admit(&mut fake, &mut output).unwrap();
    let result = tokio::time::timeout(Duration::from_secs(30), sdk(admitted, false))
        .await
        .unwrap();
    assert_eq!(result, Ok(()));
    output.emit(&mut fake, OK).unwrap();
    assert_eq!(fake.output, [READY, OK].concat());
    fake.bounded();
}
#[tokio::test]
async fn real_sdk_late_grpc_failure_never_becomes_canary_success() {
    let mut fake = Fake::new(CANARY);
    let mut output = Output::default();
    let admitted = admit(&mut fake, &mut output).unwrap();
    let result = tokio::time::timeout(Duration::from_secs(30), sdk_observed(admitted, true))
        .await
        .unwrap();
    assert_eq!(
        result,
        Err(SdkFailure::LatePermissionDenied {
            peer_calls: 1,
            peer_drained: true
        })
    );
    output.emit(&mut fake, Failure::Sdk.line()).unwrap();
    assert_eq!(fake.output, [READY, Failure::Sdk.line()].concat());
    fake.bounded();
}
