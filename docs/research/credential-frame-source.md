# Bounded credential frame source — FAKE_QA_ONLY

Developer-only library and finite invented-fake tests. No caller, actual credential
read, native engine execution, network activity, provisioning, or real-session GO
is delivered. Initial authoring was static only. Independent source review and a
narrow opening-flags follow-up permitted one finite Windows/Python 3.14 fake run:
**60 tests, zero failures/errors/skips**, source pins unchanged, outer owned Job
membership and emptiness verified. [QA evidence](../evidence/credential-frame-source-fake-qa-20261117.json).
The source's `SOURCE_UNQUALIFIED` marker remains conservative: actual sensitive
input files, READY integration and a REAL caller are not qualified. Linux/CI
results are not claimed by this local fake record; the later public-file
observation below is separate.

## Contract and API

**FIRST_CONTENT_READ_ONLY_WHEN_CALLED**: importing and `make_producer(path)` perform
no file I/O. Construction does not even validate the path. No default path exists.
`probe(path)` performs metadata checks only, returns `None`, and never opens content.
It neither reserves a source nor supplies cached metadata to a producer.
`make_producer(path)` returns `(producer, state)`. A future MAIN-owned caller MUST
wait for and enforce exact channel READY before explicitly calling `producer()`.
READY/readiness, sender/receiver integration, cancellation and timeout are NOT tested
by this library delivery. READY is not remote attestation.

The callable atomically consumes its one attempt, including failures; later or
concurrent calls cannot reopen. Success returns exactly one immutable `bytes` frame.
State exposes read-only `called`, `read_attempted`, and `length` observations;
`read_attempted` means a content readinto was attempted, not merely an open.
`length` is `None` until success and then a secret-derived integer: never publish it.
State repr/str is always `<SourceState>`, without contents or length.
Operational failures are `SourceClosed()` with empty args/message and no retained
source cause/context. Failure is caught, its handler exited, then the fixed error
is raised. Clearing caller-owned already-active exception contexts is not promised.
`BaseException` is intentionally reduced too: cancellation exceptions become the
same closed source failure. This library does not qualify cancellation handling.
Private `_ops` injection and `_allocate` observation exist solely for invented QA.

## Path and read gate

Paths must be absolute strings, with no empty, dot or dot-dot components or trailing
separator. Every traversed component (including root) is lstat-checked: no symlink
or Windows reparse point; parents must be directories. Leaf: regular, nlink 1,
size 1..65536. Windows requires a drive-rooted path; UNC/device/ADS, reserved device
names, wildcard/control forms and trailing dots/spaces are rejected.
The future caller must select a trusted local fixed disk; the library cannot prove
that a lexical local path is not a mount/network redirect. It performs no ACL check.
Opening is read-only, unbuffered, using binary/noinherit/close-on-exec/no-follow,
nonblocking/no-controlling-terminal flags where supported. Nonblocking prevents a
swapped-in FIFO from blocking at open on POSIX; regular-file read deadlines are
not guaranteed by these flags. Before reading, common lstat/fstat dev, ino, size, mtime,
mode, nlink and available birthtime fields must match. Ctime is compared only
within each API: handle before/after, and path before/after, never across APIs.
A `bytearray(65537)` receives partial readinto reads through explicit EOF; oversize,
invalid counts or size mismatch fail closed. Handle metadata must remain stable;
the handle is closed and all named components are revalidated after reading.
These checks are conservative observations, NOT a race-free, ACL or secrecy guarantee.

## Exact supported grammar and wire format

Input is parsed as byte spans, never decoded or split into private string copies.
LF and CRLF, including a final line without LF, are supported; empty lines and
column-0 `#` comments are ignored. BOM, NUL and lone CR are rejected. Unrelated
column-0 ASCII identifier assignments (`[A-Za-z_][A-Za-z0-9_]*=...`) are ignored
as independent opaque single lines. This is not a general dotenv parser: multiline
quoted values in unrelated assignments are not recognized as a single value.
All three designated names must appear exactly once as column-0 `NAME=value`:
`RAMBLER_API_KEY`, `RAMBLER_ANDROID_PACKAGE`, `RAMBLER_ANDROID_CERT`.
Export prefixes, leading whitespace, whitespace before `=`, duplicates, missing or
malformed designated fields fail. Whitespace-only lines are not empty lines.
Values are unquoted or wrapped in one matching single/double quote pair. Inner
length is 1..4096; permitted bytes are ASCII 0x21..0x7e except both quote characters,
backslash and `#`. No trimming, expansion, unescaping or value normalization occurs.
Magic is `52 4c 41 31 00 0d 0a 1a` (`RLA1` plus NUL/CR/LF/0x1a).
The three fields follow in the listed order, each as u16 big-endian length + value.
Total frame length is 17..12302. The exact 85-byte public fixture tuple
`invented-public-frame-key`, `invalid.public.frame`, `invented-public-frame-cert`
is rejected; the constants are cross-checked locally without importing a receiver.

## One actual Windows public file — separate observation

After its own source review, one new local file containing only invented values
passed the default Windows/Python 3.14 metadata/open/read path and exact frame
comparison. Its second producer call was rejected. The file and source pins were
unchanged, and outer Job membership/emptiness were verified. This observes one
real-filesystem compatibility point, not arbitrary source files, concurrent-write
safety, READY/channel integration, ACL protection or secrecy. No actual credentials,
native engine or service were used; the exclusive attempt is not replayed.
[Public-file evidence](../evidence/credential-frame-source-windows-public-file-20261117.json).

## Memory and qualification limits

There is one `bytes(frame)` private immutable copy for the existing sender contract.
Own mutable read/frame buffers are overwritten best-effort on success and failure;
this is NOT secure erasure. Python immutable copies, SSH/kernel/pipe buffers and
Rust/native Metadata/String copies are not wiped by this library. Pagefile, dumps,
hibernation, same-user processes, same-UID/root, sshd and host-key trust remain risks.
No ACL/at-rest protection, rotation, revocation, provisioning or secrecy is assured.
No hardening, SSH launch, logging, hashing, environment lookup, CLI, retry or live
opt-in exists. Fake tests do not qualify real credentials, a native session, or GO.
