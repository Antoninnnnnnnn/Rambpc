//! PUBLIC-ONLY standalone experiment, NOT production or live admission.
//! Build on Linux x86_64, edition 2021, panic=unwind. No stdin reads.
#![forbid(unsafe_op_in_unsafe_fn)]

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod native {
    use std::{fs, io::{Read, Write}, os::raw::{c_int, c_uint, c_ulong}, time::Duration};
    const CAP: u64 = 16 * 1024 * 1024;
    #[repr(C)]
    #[derive(PartialEq, Eq)]
    struct Limit { soft: u64, hard: u64 }
    unsafe extern "C" {
        fn prctl(op: c_int, ...) -> c_int;
        fn getppid() -> c_int;
        fn getuid() -> c_uint;
        fn geteuid() -> c_uint;
        fn getgid() -> c_uint;
        fn getegid() -> c_uint;
        fn setrlimit(resource: c_int, limit: *const Limit) -> c_int;
        fn getrlimit(resource: c_int, limit: *mut Limit) -> c_int;
        fn fcntl(fd: c_int, cmd: c_int, ...) -> c_int;
        fn signal(sig: c_int, handler: usize) -> usize;
        fn alarm(seconds: c_uint) -> c_uint;
    }
    fn pr(op: c_int, value: c_ulong) -> c_int {
        // Linux x86_64 prctl ABI: integer arguments only.
        unsafe { prctl(op, value, 0 as c_ulong, 0 as c_ulong, 0 as c_ulong) }
    }
    fn limit(resource: c_int, value: u64) -> bool {
        let wanted = Limit { soft: value, hard: value };
        let mut got = Limit { soft: u64::MAX, hard: u64::MAX };
        // Linux x86_64 rlimit ABI; both pointers are valid for their calls.
        (unsafe { setrlimit(resource, &wanted) == 0 && getrlimit(resource, &mut got) == 0 })
            && got == wanted
    }
    fn limit_is(resource: c_int, value: u64) -> bool {
        let mut got = Limit { soft: u64::MAX, hard: u64::MAX };
        (unsafe { getrlimit(resource, &mut got) == 0 })
            && got == Limit { soft: value, hard: value }
    }
    fn bounded(path: &str, cap: u64) -> Option<Vec<u8>> {
        let mut bytes = Vec::new();
        fs::File::open(path).ok()?.take(cap + 1).read_to_end(&mut bytes).ok()?;
        (bytes.len() as u64 <= cap).then_some(bytes)
    }
    fn field<'a>(text: &'a str, key: &str) -> Option<&'a str> {
        let mut values = text.lines().filter_map(|line| line.strip_prefix(key));
        let value = values.next()?.trim();
        values.next().is_none().then_some(value)
    }
    fn status() -> bool {
        let Some(bytes) = bounded("/proc/self/status", 65536) else { return false };
        let Ok(text) = std::str::from_utf8(&bytes) else { return false };
        let (uid, gid) = unsafe { (getuid(), getgid()) };
        if uid == 0 || gid == 0 || unsafe { geteuid() != uid || getegid() != gid } {
            return false;
        }
        for (key, id) in [("Uid:", uid), ("Gid:", gid)] {
            let Some(value) = field(text, key) else { return false };
            let values: Vec<_> = value.split_whitespace().collect();
            if values.len() != 4 || values.iter().any(|v| *v != id.to_string()) {
                return false;
            }
        }
        field(text, "TracerPid:") == Some("0")
            && field(text, "SigBlk:") == Some("0000000000000000")
            && ["CapInh:", "CapPrm:", "CapEff:", "CapAmb:"].iter().all(|key| {
                field(text, key).is_some_and(|v| v.len() == 16 && v.bytes().all(|b| b == b'0'))
            })
    }
    fn policy() -> bool {
        [("/proc/sys/kernel/core_pattern", b"core\n".as_slice()),
         ("/proc/sys/kernel/core_uses_pid", b"1\n".as_slice()),
         ("/proc/sys/fs/suid_dumpable", b"0\n".as_slice()),
         ("/proc/sys/kernel/yama/ptrace_scope", b"0\n".as_slice())]
            .iter().all(|(p, expected)| bounded(p, 128).as_deref() == Some(*expected))
    }
    fn descriptors() -> bool {
        for fd in 0..=2 {
            if unsafe { fcntl(fd, 1) } < 0 { return false; }
        }
        let Ok(entries) = fs::read_dir("/proc/self/fd") else { return false };
        let mut names = Vec::new();
        for entry in entries {
            let Ok(entry) = entry else { return false };
            names.push(entry.file_name());
        }
        names.sort();
        if names != ["0", "1", "2", "3"].map(std::ffi::OsString::from) { return false; }
        for fd in [0, 2] {
            if fs::read_link(format!("/proc/self/fd/{fd}")).ok().as_deref()
                != Some(std::path::Path::new("/dev/null")) { return false; }
        }
        let Ok(link) = fs::read_link("/proc/self/fd/1") else { return false };
        let Some(link) = link.to_str() else { return false };
        link.strip_prefix("pipe:[").and_then(|s| s.strip_suffix(']'))
            .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
    }
    pub fn run() -> bool {
        // Start nondumpable even in the public positive case. No secret is allowed
        // during exec/startup; exec resets dumpability before main can act.
        if pr(4, 0) != 0 || pr(3, 0) != 0 { return false; }
        let parent = unsafe { getppid() };
        if parent <= 1 || pr(1, 9) != 0 || unsafe { getppid() } != parent { return false; }
        // SIGALRM terminates without a core. SIGABRT must have its default action.
        unsafe {
            if signal(14, 0) == usize::MAX || signal(6, 0) == usize::MAX { return false; }
            alarm(20);
        }
        let args: Vec<_> = std::env::args_os().collect();
        let positive = match args.get(1).and_then(|v| v.to_str()) {
            Some("--public-positive-v1") if args.len() == 2 => true,
            Some("--public-protected-v1") if args.len() == 2 => false,
            _ => return false,
        };
        let env: std::collections::BTreeMap<_, _> = std::env::vars_os().collect();
        let expected: std::collections::BTreeMap<_, _> =
            [("PATH", "/usr/bin:/bin"), ("LC_ALL", "C")].map(|(k, v)|
            (std::ffi::OsString::from(k), std::ffi::OsString::from(v))).into_iter().collect();
        if env != expected || !status() || !policy() || !descriptors() { return false; }
        let core_limit = if positive { CAP } else { 0 };
        // RLIMIT_FSIZE=1, RLIMIT_CORE=4 on Linux x86_64. Never raise global policy.
        if !limit(1, CAP) || !limit(4, core_limit) { return false; }
        if positive && pr(4, 1) != 0 { return false; } // PUBLIC CONTROL ONLY.
        let dumpable = if positive { 1 } else { 0 };
        if pr(3, 0) != dumpable || !policy() || !status() { return false; }
        if std::io::stdout().write_all(b"G1_PUBLIC_READY_V1\n").is_err()
            || std::io::stdout().flush().is_err() { return false; }
        loop {
            if unsafe { getppid() } != parent || pr(3, 0) != dumpable
                || !limit_is(4, core_limit) || !limit_is(1, CAP) { return false; }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}
fn main() -> std::process::ExitCode {
    std::panic::set_hook(Box::new(|_| {}));
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    let _ = std::panic::catch_unwind(native::run);
    // Only an externally owned SIGABRT is a successful experimental termination.
    std::process::ExitCode::from(1)
}
