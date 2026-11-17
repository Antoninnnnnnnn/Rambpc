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

On 2026-11-17, the first push CI at `81f9a83` passed all eight OS/toolchain Rust test jobs, VS17, and all four Python jobs, but failed Linux Clippy: the imported `read` signature used `*mut u8` instead of the runtime's `*mut c_void`. The native-only declaration and call-site cast were corrected without suppressing the warning. [Failure provenance and source delta](../evidence/linux-real-admission-ci-20261117.json) preserve the failed run. Fresh CI at `f27a01b` subsequently passed **all 14 jobs**, including Linux Clippy, without workflow reruns. Compilation/offline tests are not native admission execution.

## Native public-only observation — 17 November 2026

On one native nonroot Linux x86_64 host, the optional example's **Debug** image passed 11/11 finite public cases: one silent no-argument case (intentionally not hardened), two pre-READY descriptor safety negatives with no input bytes, one valid public frame and seven malformed frames. For all eight framed cases, READY and external `EACCES` opening `environ` were observed before public input; environment content was never read. All 11 cases were cleaned up. The separately built Debug Fake image passed 17 Rust tests; the build phase had three direct child launches and did not launch example main. The supervisor's 10 pure Python tests passed separately on Windows and Linux. These counts are not sessions or independent end-to-end proofs.

The source and original cache checks passed, as did owned cleanup and the Windows job checks. The measured archive contains 122 Windows working-tree files associated with `f27a01b`; 22 differ from Git blobs only by CRLF normalization, so this is not a bit-exact clean-Git snapshot. The images are Debug, not the Release CLI. [Sanitized observation, hashes and limits](../evidence/linux-native-public-admission-20261117.json).

History is not rewritten: R2 was rejected before native execution because ignored SIGXFSZ was inherited; R3's pure run passed 10 assertions but exited 1 on a stale expected count of 9. Initial preparation failed before reservation on an own marker with mode 0664; its mode was tightened to 0600 without changing bytes, inode or mtime, and a separate preparation followed. Neither the G1 crash campaign nor the four-run Opus performance study was rerun.

This qualifies only the observed optional public path, not general kernel admission or a secrecy guarantee. There was no single-session path, credential, real service, upload or ASR. The zero service-call count is a closed-plan/source invariant, not independent syscall or network-isolation telemetry; there was no network namespace, seccomp or cgroup sandbox. Same-UID/root, kernel/procfs, swap, SSH/launcher buffers, toolchain/dependencies and uninterruptible or SIGKILL cleanup remain limitations. Neither latency nor semantic ASR correctness follows. The Linux caller, Windows stdin PIPE secret sender, provisioning and full backend remain unqualified; **live NO_GO remains**.

## Public supervisor reproduction boundaries

The byte-exact [public supervisor](../../tools/reference/linux_admission_public_supervisor.py) has SHA-256 `bb711f7ce1ada062cee9ed47136ae0496c38d8095d8ced26229bb2f8328ec0d9`. Its inherited R2 header is historical; the file is the observed R4 copy. Pure public-data checks can be run with `python -I -S -B tools/reference/linux_admission_public_supervisor.py --unit`; these do not launch processes or qualify native operation.

Native phases require independent review and preparation, not an automatic live command. On nonroot Linux x86_64 with matching IDs, pidfd/procfs support and the exact policy values above, provide an absolute owned 0700 workspace and pinned, single-link owned files not group/other writable. The workspace requires `owner-public-v1.json` with exactly `schema: 1`, `purpose: "public-native-rambler-qualification"`, `name` equal to the workspace basename, and `contains_credentials: false`; `admission-public-owner-v1.json` has exactly `schema: "linux-admission-public-owner-v1"` and `public_only: true`. Each marker's bytes are SHA-256 pinned. Under `admission-input-v1`, supply `source`, `source-manifest.json` and `public.pcm`; source manifest schema `linux-admission-public-source-v1` contains only `schema` and `files`, each file entry exactly `path`, `size`, `sha256`, with the source allowlist and exact tree enforced by the supervisor. Build additionally requires a pinned `cargo-cache-manifest.json` using schema `linux-admission-public-cache-v1` and its exact `cargo-cache` tree. The PCM must match the public hash above.

Choose exactly one phase. Both require `--workspace`, `--owner-sha256`, `--workspace-owner-sha256`, `--supervisor-sha256`, `--source-manifest-sha256`, `--example-sha256` and `--fake-tests-sha256`. `--build-only` additionally requires `--cache-manifest-sha256` and absolute `--cargo`, `--rustc`, `--cmake` paths with their respective SHA-256 pins; it performs locked offline builds and the frozen Fake test, not example main. `--observe-only` instead requires `--build-id`, `--build-receipt-sha256`, `--build-final-sha256`, `--example-binary-sha256` and `--fake-binary-sha256`; it validates the exact successful build receipt/final schemas and consumes an exclusive permanent observation reservation. It executes only the finite public plan, never a single-session or arbitrary input path. See the supervisor for all ownership, file, tool, signal and schema checks; satisfying these prerequisites is not authorization for credentials or live use.
