# Linux real-admission developer experiment

`linux_real_admission` is an optional `rambler-cli` example requiring the `audio` feature, not a production authentication API. Its native implementation targets Linux x86_64. With no arguments it exits successfully and silently, without hardening or admission work. When arguments are supplied, exactly one of `--public-frame-check-v1` or `--single-session-v1` is accepted; extra or unknown arguments are rejected. Other platforms fail when given arguments.

## Admission boundary

Startup, arguments and environment must contain no secrets: process execution and dynamic loading precede the example's hardening. For a requested mode, admission hardens first, parses the mode, verifies safety, verifies the public PCM, and repeats safety checks around readiness and frame receipt. Failures block admission. The checks include:

- Non-dumpable process, zero soft and hard core limits, stable parent with a parent-death signal, no-new-privileges, and a 90-second alarm. Process status must show a single untraced thread, matching nonzero user/group IDs, zero inheritable/permitted/effective/ambient capabilities (not necessarily a zero bounding set), and the required signal state.
- Exact kernel policy values: `core_pattern=core`, `core_uses_pid=1`, `suid_dumpable=0`, and `yama/ptrace_scope=0`; the example does not change these policies.
- An environment containing exactly `PATH=/usr/bin:/bin` and `LC_ALL=C`. Standard input must be a blocking pipe or socket, standard output a blocking pipe, standard error `/dev/null`, and fd 3 a blocking readable, user-owned regular PCM file not writable by group or others. No other inherited descriptors are allowed.
- A full size- and SHA-256-verified public PCM copy from fd 3, before readiness or frame input. The descriptor must be released successfully before crossing the runtime boundary.

These are bounded checks, not a complete secrecy guarantee. Same-UID or root adversaries, swap, launcher/SSH buffers, and kernel compromise are outside the claim; memory erasure is not guaranteed. A separately reviewed owner must verify the binary and externally observe denial of access to its environment (`EACCES`) before sending any frame. This example alone never authorizes live use; an owned credential channel and sender also require separate qualification.

## Frame and modes

After pre-secret gates pass, the example emits `REAL_ADMISSION_READY_V1` followed by a newline. Input is a binary frame, not a text envelope: eight magic bytes `RLA1\0\r\n\x1a`, then three fields in order: API key, Android package, Android certificate. Each field consists of a two-byte big-endian unsigned length followed by that many UTF-8 bytes. Each length must be 1–4096 bytes. EOF is mandatory immediately after the third field; truncation, invalid UTF-8, invalid lengths and trailing bytes are rejected. The existing credential provider also validates the resulting metadata; there is no environment fallback.

`--public-frame-check-v1` accepts only the exact invented tuple `invented-public-frame-key`, `invalid.public.frame`, `invented-public-frame-cert`. It validates the frame and exits before runtime, codec or socket creation. `--single-session-v1` rejects that exact tuple before runtime or socket creation; other tuples still undergo metadata validation. It admits at most one SDK session, without retry, replay or endpoint override.

## Public audio and outcome

The input is the entire raw signed 16-bit little-endian, mono 16 kHz PCM (not WAV or a prefix), 117,440 bytes / 58,720 samples, SHA-256 `cea1bc1614f738048b1a4f1bcad83f70aee42b9621b8b754f8aacbe1395b08c9`. Its source is LibriSpeech 5142-33396-0002, OpenSLR12, CC BY 4.0, from LibriVox audiobook readings (Panayotov et al., ICASSP 2015); provenance is recorded in [the public evidence](../evidence/rust-native-natural-public.json) and [backend validation](rust-backend-validation-2026-11-14.md). The audio is not embedded and no arbitrary path is opened by the example.

The terminal record contains fixed categories, counts and booleans, not transcripts or metadata. `PUBLIC_FRAME_CHECKED` means the public check passed, not that a session ran. `SESSION_HEALTHY` / `healthy=true` requires the observed full PCM upload, healthy terminal and cleanup, including final gRPC OK; it does **not** establish semantic transcription correctness or require a final recognition result. No transcript is compared or printed.

## Reproducing offline checks

With locked dependencies already fetched, the workflow's pure-Fake example test is:

```sh
cargo test -p rambler-cli --features audio --example linux_real_admission --locked --offline
```

The workflow's broader lint command is:

```sh
cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings
```

The [Windows evidence](../evidence/linux-real-admission-windows-20261116.json) records 17 passing Fake-only tests and targeted Clippy with warnings denied. The tests do not instantiate native admission, real PCM, a credential channel or a service. That record predates CI and pins the measured Windows working bytes; the earlier native Linux workspace proof excludes this example.

On 2026-11-17, the first push CI at `81f9a83` passed all eight OS/toolchain Rust test jobs, VS17, and all four Python jobs, but failed Linux Clippy: the imported `read` signature used `*mut u8` instead of the runtime's `*mut c_void`. The native-only declaration and call-site cast were corrected without suppressing the warning. [Failure provenance and source delta](../evidence/linux-real-admission-ci-20261117.json) preserve the failed run; a passing fresh CI for the correction is not claimed by that initial record. Compilation/offline tests are not native admission execution.

Kernel admission, external observation and the credential sender remain unqualified. No real mode or live call was executed in these records, and live GO remains false.
