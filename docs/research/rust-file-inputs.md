# File inputs: bounded preflight and supported types

Backend file sources accept regular files: strict mono 16 kHz PCM16LE WAV (`WavAudio`) and CLI-validated Ogg/Opus. A symbolic link to a regular file is permitted; the opened descriptor is then retained. For a live stream, provide a cooperative asynchronous `AudioSource`: file commands are not an interface for a FIFO, pipe, console, or device.

## Reproduced defect

On the native Linux binary from snapshot R5, a FIFO without a producer passed to `dictate` and then `dictate-wav`, with `--timeout 1`, did not terminate. Each process had to be stopped by an external 2s limit (`timeout` exit 124, empty stdout). The RPC timeout had not yet started: `File::open` was waiting for the producer. Only two local tests, without Google, credentials, or audio data.

A first fix based only on `metadata().is_file()` still failed on a Windows pipe: one of the two tests exceeded 5s. The test kills its child on timeout; it does not stall the test runtime in a blocking system open. This attempt is not presented as a Windows success.

## Fix

- Unix: `O_NONBLOCK` on open, followed by a type check on the descriptor. Regular files retain their normal semantics. The check does not rely on a path check vulnerable to replacement by a FIFO.
- Windows: `GetFileType` requires `FILE_TYPE_DISK` before metadata/read. Metadata alone may not distinguish a pipe correctly. The FFI call uses only a borrowed live handle and neither closes it nor retains a pointer.
- Failure before reading: the owned file is awaited on close, a source error is returned, and there is no RPC or result. No synthetic EOF or EOS.

`libc` (Unix) and `windows-sys` (Windows) were already in the lockfile; they are now direct, targeted dependencies for these constants/APIs. The Tokio `process` feature added to the CLI is **dev-only**, to kill test children on timeout, not to run an encoder or interpreter at runtime.

For the normal endpoint, strict credential preflight occurs after opening the input and before `Engine::connect`; on failure its closure is awaited. It therefore does not dispense with the open operations above. Explicit loopback does not inspect the environment. Details and local qualification: [credential snapshots](rust-credential-snapshots.md) · [A/B evidence](../evidence/rust-pcm-progress-admission.json).

## Regression safeguards

`crates/rambler-cli/tests/file_inputs.rs` checks: directory, Unix FIFO without a producer, link to FIFO, `/dev/null`, `/dev/zero`, and Windows pipe without data. Each Windows command uses a fresh instance. Tests require a JSON audio error, no published path/text, empty stderr, and zero TCP connections. Existing WAV/Opus tests verify that regular files remain intact. Linux snapshot R6 also passes the suite: the real native binary then rejects both FIFO commands with exit 1 and 53 bytes of JSON, without waiting for the external 2s limit. The old exit 124 captures are retained separately.

This fix does not promise to make every disk operation, network share, or uncooperative user source cancellable. The network timeout is not an absolute guarantee on the underlying kernel or hardware.
