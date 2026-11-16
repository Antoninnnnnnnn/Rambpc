//! Offline public-canary experiment only. No credentials, service, or live authority.
//! With no arguments this program is silent and does nothing.
//! Only --fixed-canary-v1 can reach the bounded, fixed-literal check.
#![forbid(unsafe_op_in_unsafe_fn)]

use std::io::{self, Write};

const MODE: &str = "--fixed-canary-v1";
const CANARY: &[u8; 26] = b"RAMBLER-PRECRED-CANARY-V1\n";
const READ_CAP: usize = 27;
const READY: &[u8] = b"PRECRED_READY_V1\n";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outcome {
    Ok,
    Harden,
    Verify,
    Traced,
    Fd,
    Env,
    Argv,
    Io,
    Reject,
    Internal,
}

impl Outcome {
    fn code(self) -> u8 {
        match self {
            Self::Ok => 0,
            Self::Harden => 10,
            Self::Verify => 11,
            Self::Traced => 12,
            Self::Fd => 13,
            Self::Env => 14,
            Self::Argv => 15,
            Self::Io => 16,
            Self::Reject => 20,
            Self::Internal => 70,
        }
    }

    fn line(self) -> &'static [u8] {
        match self {
            Self::Ok => b"PRECRED_CANARY_OK_V1\n",
            Self::Harden => b"PRECRED_FAIL_HARDEN\n",
            Self::Verify => b"PRECRED_FAIL_VERIFY\n",
            Self::Traced => b"PRECRED_FAIL_TRACED\n",
            Self::Fd => b"PRECRED_FAIL_FD\n",
            Self::Env => b"PRECRED_FAIL_ENV\n",
            Self::Argv => b"PRECRED_FAIL_ARGV\n",
            Self::Io => b"PRECRED_FAIL_IO\n",
            Self::Reject => b"PRECRED_REJECT_PAYLOAD\n",
            Self::Internal => b"PRECRED_FAIL_INTERNAL\n",
        }
    }
}

// Private, fixed-canary state machine; this is not a payload-admission API.
trait Sys {
    fn set_nondumpable(&mut self) -> bool;
    fn dumpable(&mut self) -> i32;
    fn untraced(&mut self) -> bool;
    fn fd_set_valid(&mut self) -> bool;
    fn env_names_valid(&mut self) -> bool;
    fn mode_args_valid(&mut self) -> bool;
    fn read_stdin(&mut self, byte: &mut [u8]) -> Result<usize, ()>;
    fn write_line(&mut self, line: &'static [u8]) -> Result<(), ()>;
}

#[derive(Default)]
struct OutputState {
    attempts: u8,
    broken: bool,
}

impl OutputState {
    fn emit<S: Sys>(&mut self, sys: &mut S, line: &'static [u8]) -> Result<(), ()> {
        if self.broken || self.attempts >= 2 {
            return Err(());
        }
        self.attempts += 1;
        // If a writer fails or panics, do not append a line to a partial write.
        self.broken = true;
        sys.write_line(line)?;
        self.broken = false;
        Ok(())
    }

    fn finish<S: Sys>(&mut self, sys: &mut S, result: Outcome) -> Outcome {
        if self.emit(sys, result.line()).is_ok() {
            result
        } else {
            Outcome::Io
        }
    }
}

struct CanaryBuffer([u8; READ_CAP]);

impl Drop for CanaryBuffer {
    fn drop(&mut self) {
        // Safe best effort, NOT guaranteed erasure; only public bytes belong here.
        self.0.fill(0);
        std::hint::black_box(&mut self.0);
    }
}

fn run<S: Sys>(sys: &mut S, output: &mut OutputState) -> Outcome {
    if !sys.set_nondumpable() {
        return output.finish(sys, Outcome::Harden);
    }
    if sys.dumpable() != 0 {
        return output.finish(sys, Outcome::Verify);
    }
    if !sys.untraced() {
        return output.finish(sys, Outcome::Traced);
    }
    if !sys.fd_set_valid() {
        return output.finish(sys, Outcome::Fd);
    }
    if !sys.env_names_valid() {
        return output.finish(sys, Outcome::Env);
    }
    if !sys.mode_args_valid() {
        return output.finish(sys, Outcome::Argv);
    }
    if output.emit(sys, READY).is_err() {
        return Outcome::Io;
    }
    if sys.dumpable() != 0 {
        return output.finish(sys, Outcome::Verify);
    }

    let mut buffer = CanaryBuffer([0; READ_CAP]);
    let mut used = 0;
    let mut eof = false;
    while used < READ_CAP {
        // One byte per request bounds the SUM of requested lengths to 27,
        // including the EOF probe after the exact 26-byte literal.
        match sys.read_stdin(&mut buffer.0[used..used + 1]) {
            Ok(0) => {
                eof = true;
                break;
            }
            Ok(1) => used += 1,
            Ok(_) | Err(()) => return output.finish(sys, Outcome::Io),
        }
    }
    let result = if eof && &buffer.0[..used] == CANARY.as_slice() {
        Outcome::Ok
    } else {
        Outcome::Reject
    };
    // Drop before output. Unwinding also drops the buffer.
    drop(buffer);
    output.finish(sys, result)
}

fn run_caught<S: Sys>(sys: &mut S) -> Outcome {
    let mut output = OutputState::default();
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(sys, &mut output))) {
        Ok(result) => result,
        Err(_) => {
            // The silent hook is installed by main before this state machine.
            // A second writer panic must not escape or produce diagnostics.
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                output.emit(sys, Outcome::Internal.line())
            }));
            Outcome::Internal
        }
    }
}

fn run_if_requested<S: Sys>(has_arguments: bool, sys: &mut S) -> Outcome {
    if has_arguments {
        run_caught(sys)
    } else {
        Outcome::Ok
    }
}

fn env_key_allowed(key: &std::ffi::OsStr) -> bool {
    let bytes = key.as_encoded_bytes();
    bytes != b"WSLENV" && !bytes.starts_with(b"RAMBLER_")
}

fn args_valid(mut args: impl Iterator<Item = std::ffi::OsString>) -> bool {
    args.next().is_some()
        && args.next().as_deref() == Some(std::ffi::OsStr::new(MODE))
        && args.next().is_none()
}

fn tracer_zero(status: &[u8]) -> bool {
    let mut found = false;
    for line in status.split(|byte| *byte == b'\n') {
        if let Some(value) = line.strip_prefix(b"TracerPid:") {
            if found {
                return false;
            }
            found = true;
            let value = value.trim_ascii();
            if value != b"0" {
                return false;
            }
        }
    }
    found
}

fn fd_name_index(name: &std::ffi::OsStr) -> Option<usize> {
    match name.as_encoded_bytes() {
        b"0" => Some(0),
        b"1" => Some(1),
        b"2" => Some(2),
        b"3" => Some(3),
        _ => None,
    }
}

fn fd_names_valid(names: impl IntoIterator<Item = Result<std::ffi::OsString, ()>>) -> bool {
    let mut seen = [false; 4];
    for name in names {
        let Ok(name) = name else { return false };
        let Some(index) = fd_name_index(&name) else {
            return false;
        };
        if seen[index] {
            return false;
        }
        seen[index] = true;
    }
    seen == [true; 4]
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod platform {
    use super::*;
    use std::io::Read;
    use std::os::raw::{c_int, c_ulong, c_void};

    const PR_GET_DUMPABLE: c_int = 3;
    const PR_SET_DUMPABLE: c_int = 4;
    const F_GETFD: c_int = 1;
    const STATUS_CAP: usize = 64 * 1024;

    unsafe extern "C" {
        fn prctl(option: c_int, ...) -> c_int;
        fn fcntl(fd: c_int, cmd: c_int, ...) -> c_int;
        fn read(fd: c_int, buf: *mut c_void, count: usize) -> isize;
    }

    pub(super) struct Native;

    impl Sys for Native {
        fn set_nondumpable(&mut self) -> bool {
            // SAFETY: Linux x86_64 prctl ABI, four integer arguments, no pointers.
            unsafe {
                prctl(
                    PR_SET_DUMPABLE,
                    0 as c_ulong,
                    0 as c_ulong,
                    0 as c_ulong,
                    0 as c_ulong,
                ) == 0
            }
        }

        fn dumpable(&mut self) -> i32 {
            // SAFETY: Linux x86_64 prctl ABI, four integer arguments, no pointers.
            unsafe {
                prctl(
                    PR_GET_DUMPABLE,
                    0 as c_ulong,
                    0 as c_ulong,
                    0 as c_ulong,
                    0 as c_ulong,
                )
            }
        }

        fn untraced(&mut self) -> bool {
            // This handle is dropped before fd enumeration.
            let Ok(mut file) = std::fs::File::open("/proc/self/status") else {
                return false;
            };
            let mut bytes = [0_u8; STATUS_CAP];
            let mut used = 0;
            while used < bytes.len() {
                match file.read(&mut bytes[used..]) {
                    Ok(0) => return tracer_zero(&bytes[..used]),
                    Ok(count) => used += count,
                    Err(_) => return false,
                }
            }
            // Even exactly 64 KiB is rejected: EOF was not observed in bound.
            false
        }

        fn fd_set_valid(&mut self) -> bool {
            for fd in 0..=2 {
                // SAFETY: F_GETFD takes no third argument and changes no fd.
                if unsafe { fcntl(fd, F_GETFD) } == -1 {
                    return false;
                }
            }
            let Ok(entries) = std::fs::read_dir("/proc/self/fd") else {
                return false;
            };
            // With 0..=2 open, the directory's own descriptor must be 3.
            fd_names_valid(
                entries.map(|entry| entry.map(|entry| entry.file_name()).map_err(|_| ())),
            )
        }

        fn env_names_valid(&mut self) -> bool {
            // Only keys are inspected. std itself materializes key/value pairs;
            // this code never inspects, formats, or retains a value. NO secrets
            // are permitted in the launch environment, even under other names.
            std::env::vars_os()
                .map(|(key, _)| key)
                .all(|key| env_key_allowed(&key))
        }

        fn mode_args_valid(&mut self) -> bool {
            args_valid(std::env::args_os())
        }

        fn read_stdin(&mut self, byte: &mut [u8]) -> Result<usize, ()> {
            // Bypass Rust's buffered stdin: its read-ahead could exceed the cap.
            // SAFETY: writable slice, exact length, fd 0 only; Linux x86_64 ABI.
            let count = unsafe { read(0, byte.as_mut_ptr().cast(), byte.len()) };
            if count < 0 {
                Err(())
            } else {
                Ok(count as usize)
            }
        }

        fn write_line(&mut self, line: &'static [u8]) -> Result<(), ()> {
            let mut stdout = io::stdout().lock();
            stdout
                .write_all(line)
                .and_then(|_| stdout.flush())
                .map_err(|_| ())
        }
    }
}

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
mod platform {
    use super::*;
    pub(super) struct Native;
    impl Sys for Native {
        fn set_nondumpable(&mut self) -> bool {
            false
        }
        fn dumpable(&mut self) -> i32 {
            -1
        }
        fn untraced(&mut self) -> bool {
            false
        }
        fn fd_set_valid(&mut self) -> bool {
            false
        }
        fn env_names_valid(&mut self) -> bool {
            false
        }
        fn mode_args_valid(&mut self) -> bool {
            false
        }
        fn read_stdin(&mut self, _: &mut [u8]) -> Result<usize, ()> {
            Err(())
        }
        fn write_line(&mut self, line: &'static [u8]) -> Result<(), ()> {
            let mut stdout = io::stdout().lock();
            stdout
                .write_all(line)
                .and_then(|_| stdout.flush())
                .map_err(|_| ())
        }
    }
}

fn main() -> std::process::ExitCode {
    // No diagnostics from ordinary unwinding; builds must use panic=unwind.
    std::panic::set_hook(Box::new(|_| {}));
    let result = std::panic::catch_unwind(|| {
        // No arguments: no hardening, proc reads, env scan, output, or stdin.
        // The fixed switch and absence of extra args are checked after hardening.
        run_if_requested(std::env::args_os().nth(1).is_some(), &mut platform::Native)
    });
    // An unexpected failure outside the state machine exits silently.
    std::process::ExitCode::from(result.unwrap_or(Outcome::Internal).code())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::ffi::{OsStr, OsString};

    struct Fake {
        log: Vec<&'static str>,
        harden: bool,
        gets: VecDeque<i32>,
        traced: bool,
        fds: bool,
        fd_names: Vec<OsString>,
        env: bool,
        argv: bool,
        input: Vec<u8>,
        position: usize,
        requested: usize,
        eof_reads: usize,
        output: Vec<u8>,
        fail_write: Option<usize>,
        writes: usize,
        fail_read: bool,
        oversized_read: bool,
        panic_at: Option<&'static str>,
    }

    impl Fake {
        fn new(input: &[u8]) -> Self {
            Self {
                log: Vec::new(),
                harden: true,
                gets: [0, 0].into(),
                traced: true,
                fds: true,
                fd_names: ["0", "1", "2", "3"].map(OsString::from).into(),
                env: true,
                argv: true,
                input: input.to_vec(),
                position: 0,
                requested: 0,
                eof_reads: 0,
                output: Vec::new(),
                fail_write: None,
                writes: 0,
                fail_read: false,
                oversized_read: false,
                panic_at: None,
            }
        }

        fn call(&mut self, name: &'static str) {
            self.log.push(name);
            if self.panic_at == Some(name) {
                panic!("fixed fake panic; must never appear on prototype output");
            }
        }
    }

    impl Sys for Fake {
        fn set_nondumpable(&mut self) -> bool {
            self.call("set");
            self.harden
        }
        fn dumpable(&mut self) -> i32 {
            self.call("get");
            self.gets.pop_front().unwrap_or(-1)
        }
        fn untraced(&mut self) -> bool {
            self.call("tracer");
            self.traced
        }
        fn fd_set_valid(&mut self) -> bool {
            self.call("fd");
            self.fds && fd_names_valid(self.fd_names.clone().into_iter().map(Ok))
        }
        fn env_names_valid(&mut self) -> bool {
            self.call("env");
            self.env
        }
        fn mode_args_valid(&mut self) -> bool {
            self.call("argv");
            self.argv
        }
        fn read_stdin(&mut self, byte: &mut [u8]) -> Result<usize, ()> {
            self.call("read");
            assert_eq!(byte.len(), 1);
            self.requested += byte.len();
            assert!(self.requested <= READ_CAP);
            if self.fail_read {
                return Err(());
            }
            if self.oversized_read {
                return Ok(2);
            }
            if self.position == self.input.len() {
                self.eof_reads += 1;
                return Ok(0);
            }
            byte[0] = self.input[self.position];
            self.position += 1;
            Ok(1)
        }
        fn write_line(&mut self, line: &'static [u8]) -> Result<(), ()> {
            self.call(if line == READY {
                "write-ready"
            } else {
                "write-result"
            });
            self.writes += 1;
            if self.fail_write == Some(self.writes) {
                return Err(());
            }
            self.output.extend_from_slice(line);
            Ok(())
        }
    }

    fn assert_output(fake: &Fake) {
        let allowed: &[&[u8]] = &[
            READY,
            Outcome::Ok.line(),
            Outcome::Harden.line(),
            Outcome::Verify.line(),
            Outcome::Traced.line(),
            Outcome::Fd.line(),
            Outcome::Env.line(),
            Outcome::Argv.line(),
            Outcome::Io.line(),
            Outcome::Reject.line(),
            Outcome::Internal.line(),
        ];
        assert!(fake.output.len() <= 64);
        let mut lines = 0;
        for line in fake.output.split_inclusive(|byte| *byte == b'\n') {
            assert!(allowed.contains(&line));
            lines += 1;
        }
        assert!(lines <= 2);
        assert!(fake.writes <= 2);
        assert!(fake.requested <= READ_CAP);
    }

    #[test]
    fn default_no_op_makes_no_system_calls() {
        let mut fake = Fake::new(CANARY);
        fake.panic_at = Some("set");
        assert_eq!(run_if_requested(false, &mut fake), Outcome::Ok);
        assert!(fake.log.is_empty());
        assert!(fake.output.is_empty());
        assert_eq!(fake.requested, 0);
        assert_output(&fake);
    }

    #[test]
    fn happy_path_is_ordered_and_eof_checked() {
        let mut fake = Fake::new(CANARY);
        assert_eq!(run_caught(&mut fake), Outcome::Ok);
        let mut expected = vec![
            "set",
            "get",
            "tracer",
            "fd",
            "env",
            "argv",
            "write-ready",
            "get",
        ];
        expected.extend(std::iter::repeat("read").take(27));
        expected.push("write-result");
        assert_eq!(fake.log, expected);
        assert_eq!(fake.eof_reads, 1);
        assert_eq!(fake.position, 26);
        assert_eq!(fake.requested, 27);
        assert_eq!(fake.output, [READY, Outcome::Ok.line()].concat());
        assert_output(&fake);
    }

    #[test]
    fn each_pre_ready_failure_stops_before_later_checks_and_stdin() {
        for case in 0..9 {
            let mut fake = Fake::new(CANARY);
            let (outcome, calls) = match case {
                0 => {
                    fake.harden = false;
                    (Outcome::Harden, 1)
                }
                1..=3 => {
                    fake.gets = [match case {
                        1 => -1,
                        2 => 1,
                        _ => 2,
                    }]
                    .into();
                    (Outcome::Verify, 2)
                }
                4 => {
                    fake.traced = false;
                    (Outcome::Traced, 3)
                }
                5 => {
                    fake.fds = false;
                    (Outcome::Fd, 4)
                }
                6 => {
                    fake.env = false;
                    (Outcome::Env, 5)
                }
                7 => {
                    fake.argv = false;
                    (Outcome::Argv, 6)
                }
                _ => {
                    fake.fail_write = Some(1);
                    (Outcome::Io, 6)
                }
            };
            assert_eq!(run_caught(&mut fake), outcome);
            let mut expected = vec!["set", "get", "tracer", "fd", "env", "argv"];
            expected.truncate(calls);
            expected.push(if case == 8 {
                "write-ready"
            } else {
                "write-result"
            });
            assert_eq!(fake.log, expected);
            assert_eq!(fake.requested, 0);
            if case == 8 {
                assert!(fake.output.is_empty())
            } else {
                assert_eq!(fake.output, outcome.line())
            }
            assert_output(&fake);
        }
    }

    #[test]
    fn missing_extra_duplicate_and_invalid_fd_names_stop_before_env_and_stdin() {
        for names in [
            vec!["0", "1", "2"],
            vec!["0", "1", "2", "3", "4"],
            vec!["0", "1", "2", "3", "3"],
            vec!["0", "1", "2", "03"],
            vec!["1", "2", "3"],
        ] {
            let mut fake = Fake::new(CANARY);
            fake.fd_names = names.into_iter().map(OsString::from).collect();
            assert_eq!(run_caught(&mut fake), Outcome::Fd);
            assert_eq!(fake.log, ["set", "get", "tracer", "fd", "write-result"]);
            assert_eq!(fake.requested, 0);
            assert_eq!(fake.output, Outcome::Fd.line());
            assert_output(&fake);
        }
        assert!(!fd_names_valid([Err(())]));
    }

    #[test]
    fn recheck_failure_is_after_ready_but_before_stdin() {
        for value in [-1, 1, 2] {
            let mut fake = Fake::new(CANARY);
            fake.gets = [0, value].into();
            assert_eq!(run_caught(&mut fake), Outcome::Verify);
            assert_eq!(fake.requested, 0);
            assert_eq!(
                fake.log,
                [
                    "set",
                    "get",
                    "tracer",
                    "fd",
                    "env",
                    "argv",
                    "write-ready",
                    "get",
                    "write-result"
                ]
            );
            assert_eq!(fake.output, [READY, Outcome::Verify.line()].concat());
            assert_output(&fake);
        }
    }

    #[test]
    fn only_exact_literal_followed_by_eof_is_accepted() {
        let mut nul = CANARY.to_vec();
        nul[5] = 0;
        let payloads = [
            Vec::new(),
            CANARY[..5].to_vec(),
            CANARY[..25].to_vec(),
            b"RAMBLER-PRECRED-CANARY-V1\r\n".to_vec(),
            [CANARY.as_slice(), b"X"].concat(),
            nul,
            vec![b'X'; 4096],
            [CANARY.as_slice(), &vec![b'X'; 4096]].concat(),
        ];
        for payload in payloads {
            let mut fake = Fake::new(&payload);
            assert_eq!(run_caught(&mut fake), Outcome::Reject);
            assert_eq!(fake.output, [READY, Outcome::Reject.line()].concat());
            assert_eq!(fake.position, payload.len().min(READ_CAP));
            assert_eq!(fake.eof_reads, usize::from(payload.len() < READ_CAP));
            assert_output(&fake);
        }
        // Every proper prefix fails; the newline is part of the fixed literal.
        for length in 0..CANARY.len() {
            let mut fake = Fake::new(&CANARY[..length]);
            assert_eq!(run_caught(&mut fake), Outcome::Reject);
            assert_output(&fake);
        }
    }

    #[test]
    fn read_errors_and_writer_errors_are_closed() {
        for oversized in [false, true] {
            let mut fake = Fake::new(CANARY);
            fake.fail_read = !oversized;
            fake.oversized_read = oversized;
            assert_eq!(run_caught(&mut fake), Outcome::Io);
            assert_eq!(fake.output, [READY, Outcome::Io.line()].concat());
            assert_output(&fake);
        }
        let mut fake = Fake::new(CANARY);
        fake.fail_write = Some(2);
        assert_eq!(run_caught(&mut fake), Outcome::Io);
        assert_eq!(fake.output, READY);
        assert_eq!(fake.writes, 2);
        assert_output(&fake);
    }

    #[test]
    fn fake_panics_are_fixed_and_do_not_read_after_failure() {
        for stage in ["set", "get", "tracer", "fd", "env", "argv"] {
            let mut fake = Fake::new(CANARY);
            fake.panic_at = Some(stage);
            assert_eq!(run_caught(&mut fake), Outcome::Internal);
            assert_eq!(fake.output, Outcome::Internal.line());
            assert_eq!(fake.requested, 0);
            assert_output(&fake);
        }
        let mut fake = Fake::new(CANARY);
        fake.panic_at = Some("read");
        assert_eq!(run_caught(&mut fake), Outcome::Internal);
        assert_eq!(fake.output, [READY, Outcome::Internal.line()].concat());
        assert_output(&fake);
        for stage in ["write-ready", "write-result"] {
            let mut fake = Fake::new(CANARY);
            fake.panic_at = Some(stage);
            assert_eq!(run_caught(&mut fake), Outcome::Internal);
            assert_eq!(
                fake.output,
                if stage == "write-ready" {
                    &[][..]
                } else {
                    READY
                }
            );
            if stage == "write-ready" {
                assert_eq!(fake.requested, 0)
            }
            assert_output(&fake);
        }
    }

    #[test]
    fn tracer_parser_fails_closed() {
        assert!(tracer_zero(b"Name:\tcanary\nTracerPid:\t0\n"));
        for status in [
            &b""[..],
            b"TracerPid:\t1\n",
            b"TracerPid:\t00\n",
            b"TracerPid:\t0x0\n",
            b"TracerPid:\t0\nTracerPid:\t0\n",
            b"TracerPid:\t\n",
            b"TracerPid:\t0\0\n",
        ] {
            assert!(!tracer_zero(status));
        }
    }

    #[test]
    fn fd_names_and_env_keys_and_mode_are_exact() {
        for (name, expected) in [
            ("0", Some(0)),
            ("1", Some(1)),
            ("2", Some(2)),
            ("3", Some(3)),
            ("4", None),
            ("00", None),
            ("-1", None),
        ] {
            assert_eq!(fd_name_index(OsStr::new(name)), expected);
        }
        for name in ["WSLENV", "RAMBLER_", "RAMBLER_TEST"] {
            assert!(!env_key_allowed(OsStr::new(name)));
        }
        for name in ["PATH", "wslEnv", "RAMBLER", "X_RAMBLER_TEST"] {
            assert!(env_key_allowed(OsStr::new(name)));
        }
        for (args, valid) in [
            (vec!["canary", MODE], true),
            (vec!["canary"], false),
            (vec!["canary", "--other"], false),
            (vec!["canary", MODE, "extra"], false),
            (Vec::new(), false),
        ] {
            assert_eq!(args_valid(args.into_iter().map(OsString::from)), valid);
        }
    }

    #[test]
    fn fixed_codes_and_literal_bounds() {
        assert_eq!(CANARY.len(), 26);
        assert_eq!(READ_CAP, CANARY.len() + 1);
        for (outcome, code) in [
            (Outcome::Ok, 0),
            (Outcome::Harden, 10),
            (Outcome::Verify, 11),
            (Outcome::Traced, 12),
            (Outcome::Fd, 13),
            (Outcome::Env, 14),
            (Outcome::Argv, 15),
            (Outcome::Io, 16),
            (Outcome::Reject, 20),
            (Outcome::Internal, 70),
        ] {
            assert_eq!(outcome.code(), code);
            assert!(READY.len() + outcome.line().len() <= 64);
        }
    }

    #[test]
    fn source_guard_is_a_tripwire_not_a_security_proof() {
        let source = include_str!("linux_precredential_canary_20261116.rs");
        let production = source.split("\n#[cfg(test)]\nmod tests {").next().unwrap();
        for forbidden in [
            "Command",
            "fork",
            "exec",
            "setuid",
            "setgid",
            "setfsuid",
            "setfsgid",
            "setsid",
            "TcpStream",
            "UdpSocket",
            "std::net",
            "std::thread",
            "env::var(",
            "env::var_os(",
            "File::create",
            "OpenOptions",
            "println!",
            "eprintln!",
            "stderr",
            "PR_SET_DUMPABLE, 1",
            "getenv",
        ] {
            assert!(
                !production.contains(forbidden),
                "forbidden production token: {forbidden}"
            );
        }
        assert_eq!(production.matches("unsafe {").count(), 4);
        assert_eq!(production.matches("unsafe extern").count(), 1);
    }
}
