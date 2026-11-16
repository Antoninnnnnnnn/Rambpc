# Native audio build and API

This crate uses native **libopus 1.6.1**, not a Rust substitute codec and not
FFmpeg/Python at runtime. `opus = 0.4.0` is the safe Rust wrapper;
`opusic-sys = 0.7.7` is pinned explicitly with its `bundled` feature. Its Cargo
build compiles the included C sources with CMake and emits
`cargo:rustc-link-lib=static=opus`; no system Opus DLL is selected. Generated FFI
bindings are already included: libclang/bindgen is not needed for this profile.
The optional DRED/OSCE features are not enabled. No build-time model downloads.

## Toolchain

Build prerequisites are Rust/Cargo (manifest minimum 1.88), a native C toolchain,
and CMake. Windows was actually checked with Rust **1.99.1**, Visual Studio 2022
Community / MSVC **14.44.35207**, x64, CMake **3.31.10**. Other targets/toolchain
versions are not claimed as validated by that Windows run. On Unix install the
usual C compiler, make (or Ninja), and CMake through your platform's tooling.

### Windows x64 MSVC generator

The repository Cargo environment sets `CMAKE_GENERATOR_x86_64_pc_windows_msvc`
to `Visual Studio 17 2022`. Cargo discovers this configuration from its
**current working directory** and ancestors, not from the manifest location.
Downstream dependency consumers in another workspace, and outside-repo calls
with `--manifest-path`, do not inherit it. There, arrange Ninja or set the
exact target generator environment key explicitly.

Without Ninja in child tool discovery, this default requires VS 2022 C++
build tools and a Windows SDK. VS 2019 or VS 2026 alone is not enough for this
default branch; this is not a universal Windows minimum or toolchain claim.
When opusic-sys finds Ninja, it selects Ninja and never consults this key.
Its Windows tool lookup can include the application/current/system locations
as well as PATH; the functional checker also asserts the actual generator.

Passing an explicit generator also makes cmake 0.1.58 preserve CMake's
normal Release `/O2 /Ob2 /DNDEBUG` flags for bundled Opus. Do not simplify this
setting away on the assumption that it only names the generator.

For another VS version, override the exact same key in the Cargo process
environment (and arrange the corresponding installed tools), for example:

```powershell
$env:CMAKE_GENERATOR_x86_64_pc_windows_msvc = 'Visual Studio 16 2019'
```

`force = false` protects an override of that exact key only; a plain
`CMAKE_GENERATOR` is lower priority and shadowed. The optional hyphenated key
`CMAKE_GENERATOR_x86_64-pc-windows-msvc` has still higher priority.
Linux/macOS target lookups do not consult this Windows-specific key.
These lookup rules do not validate an alternative generator's compatibility.

This takes effect **only when opusic-sys's build script reruns for that
profile**, using a fresh target or a reviewed profile-specific package clean.
Changing the key alone is not a promise that Cargo rebuilds existing Opus;
a rerun against an incompatible CMake cache can fail. Before any clean,
**preserve needed user artifacts first** and inspect the dev-only dry run:

```sh
cargo clean -p opusic-sys --profile dev --dry-run -v
```

Review the listed files before considering the same command without dry-run.
For Release, preserve its artifacts separately and separately inspect
`cargo clean -p opusic-sys --profile release --dry-run -v`; only then consider
the corresponding Release clean without dry-run. Never blindly clean target,
delete a CLI executable, or infer a Release rebuild from a dev clean.

The separate `vs17-opus` functional job pins `windows-2022` and stable Rust,
keeps the ordinary eight native matrix jobs and their PATH unchanged, and runs
only the eight Release `profile_pipeline` unit tests, not the example main or
the release CLI. A child-only PATH excludes Ninja without altering installed
files. The actual cache, VS instance, x64 host tools and 152 compiler records
must pass runtime checks. Native output is located through Cargo's documented
`build-script-executed` JSON `out_dir`, not a private Cargo directory layout.
Built/installed archive SHA-256 identity is local to that checkout, not a
cross-machine checksum or performance comparison. Logs are retained only in
local temporary storage, never uploaded; failure may print a bounded build-log
tail, including Cargo JSON. Tool-discovery logs are never dumped, and no timing
is collected.

The official
[Windows2022 runner-image readme](https://github.com/actions/runner-images/blob/main/images/windows/Windows2022-Readme.md)
lists VS 2022 Enterprise 17.14.37906.3, CMake 3.31.6 and Ninja 1.13.2 at the
source-review checkpoint. Those exact minor versions are not pinned; installed
VS major 17 with x64 C++ tools and the selected CMake are still verified at
runtime. A source proposal is not a passing hosted-CI qualification, and local
Windows gains do not establish gains for other toolsets or other platforms.

If CMake is absent on Windows, an isolated development tool is sufficient; no
administrator/global installation is needed. One reproducible option is the
PyPI `cmake==3.31.10` binary wheel (`cmake-3.31.10-py3-none-win_amd64.whl`):

- SHA-256: `f1ea1fe826355560e8976c3d5794d9357444209bc0e0d56676c71e6a571fd474`
- Download with `python -I -m pip download --only-binary=:all: --no-deps cmake==3.31.10`
  into a separate build-tool directory, verify the wheel hash, and extract it.
- Set `CMAKE` to the absolute extracted `cmake/data/bin/cmake.exe`. Cargo uses
  this native executable directly; Python and the wheel are development tooling
  only, not dependencies of the resulting Rust program.

The actual inspected CMake cache reported `OPUS_BUILD_SHARED_LIBRARY=OFF`,
`Visual Studio 17 2022`, `x64`, and `Release` for the C library. The source's
`opus/package_version` reports `1.6.1`. Native encoder/decoder tests execute that
library. `dumpbin /DEPENDENTS` on the executed Windows encoder test executable
showed only Windows/CRT DLLs, including `VCRUNTIME140.dll`, and no Opus, FFmpeg,
Python or other codec DLL. Static Opus does **not** mean static MSVC CRT or no
system DLL dependencies. Deployment must account for the normal MSVC runtime.

## API and bounds

`Pcm16Opus<S: rambler_engine::AudioSource>::new(source, EncoderConfig)` takes
ownership synchronously and implements the engine's same `AudioSource` trait.
Validation is deferred until its first `next`, so even invalid config closes
upstream without reading it. `EncoderConfig::default()` is 16000 Hz, mono,
24000 bit/s, 20 ms pages. Only this audio profile is accepted; page duration alone
can vary over integers 20..1000 ms. Codec frames are always 320 samples / 20 ms;
a non-multiple page duration rounds up to the next full codec frame.

`WavAudio::open(path, WavConfig).await` validates and owns a single open file.
Its descriptive `format`, `frame_count` and `duration` fields expose the checked
metadata; modifying these descriptive values does not change private read bounds.
`WavConfig` is `{ chunk_ms: u32, realtime: bool }`, default 40 ms / false.
Pacing starts at the first polled `next`, **not** at construction/open. The first
chunk has no artificial delay; later chunks use emitted-PCM start offsets and EOF
has no extra delay. Cancelling a pending read/pacing future preserves progress.
No reopening/whole-file buffer is used. Concurrent in-place file modifications
are not a snapshot guarantee; truncation is detected during consumption.

The WAV adapter accepts classic little-endian RIFF, PCM format 1 or extensible
PCM GUID, exactly mono / 16000 Hz / 16 valid bits, block alignment 2, byte rate
32000. It rejects compressed, stereo, other rates/widths, RF64/RIFX, incomplete
samples, extent overflow and truncation. Unknown RIFF chunks are skipped by seek
with padding validated. Metadata traversal is capped at 100000 chunks. This is
intentionally stricter than the general Python WAV reader, which also supports
rates other than the validated encoder profile.

```rust,ignore
use rambler_audio::{EncoderConfig, Pcm16Opus, WavAudio, WavConfig};
use rambler_engine::AudioSource;

let wav = WavAudio::open(path, WavConfig { realtime: true, ..Default::default() }).await?;
let audio: Box<dyn AudioSource> = Box::new(Pcm16Opus::new(wav, EncoderConfig::default()));
// Hand ownership to Engine::start; engine awaits audio.close() on every outcome.
```

Each PCM input chunk is at most 8 MiB; one current chunk is retained, not a whole
source. An arbitrary upstream `Bytes` slice can itself retain a larger backing
allocation owned by its caller; the byte-length bound is not an allocator/RSS
claim. Encoder PCM staging is 640 bytes. A packet is bounded at 1275 bytes; a mux
page is at most 50 such packets and 255 lacing entries, below 64 KiB. Pages
flush earlier if the next packet's lacing (including its zero terminator when
needed) would exceed 255 entries. Pending
output is explicitly bounded at 64 KiB. Pull backpressure and cooperative yields
apply per input read/frame, including immediately ready/empty sources and huge
chunks. The library itself starts no producer task or growing channel.

Wholly empty PCM emits no Ogg headers and constructs no native encoder. Odd chunk
boundaries are accepted, but an odd final byte fails. At EOF, upstream is closed
successfully before EOS; a failing close cannot produce successful EOS. Positive
encoder lookahead is queried from libopus and converted to 48kHz `pre-skip`.
Zero padding flushes **all** real tail samples plus lookahead, including exact
320-sample boundaries; the final granule is `pre_skip + original_samples * 3`.
Padding is removed by the Ogg container, not silently added to the returned audio
duration. This is lossy coding: decoded PCM is not claimed byte-identical.

`next` tolerates its future being dropped. `close().await` is idempotent, caches
upstream close errors, and resumes an interrupted owned close future rather than
calling upstream close twice. A primary source/codec error survives cancellation
while cleanup is pending; secondary cleanup failure remains available from
`close`. Explicit close and Drop release native state synchronously. WAV close, EOF,
read error and invalid-header return await Tokio's `File::into_std()` before
dropping the descriptor, including cancelled pending reads. Drop schedules owned
asynchronous cleanup only if a Tokio runtime exists. Keep the runtime alive and
explicitly await `close` for a cleanup guarantee; an arbitrary uncooperative
upstream can still block its own close. Neither Drop nor cancellation manufactures
EOS or claims a completed dictation.

## Offline verification

With the workspace member added and `CMAKE` set if needed:

```sh
cargo test -p rambler-audio
cargo fmt -p rambler-audio -- --check
cargo clippy -p rambler-audio --all-targets -- -D warnings
```

The default tests require no Python, service or FFmpeg. An independent `ogg`
reader validates actual page/packet framing and CRC, while native packet decoding
checks frame contents and duration constraints. Tests cover arbitrary/odd
fragmentation, 1/319/320/321-sample boundaries and more, variable page batching,
WAV malformations/truncations, t0 pacing with simulated time, empty source,
budgets, failure, cancellation and close-once/error. Windows tests reopen files
with `share_mode(0)` after cleanup to establish actual descriptor release.

An additional explicitly ignored **development-only** oracle tests complete Ogg
containers, including real pre-skip/final-page trim, with an independent PyAV /
FFmpeg decoder. It is not compiled into the library or invoked during normal use:

```sh
RAMBLER_AUDIO_TEST_PYTHON=/absolute/dev/python \
  cargo test -p rambler-audio --test independent -- --ignored
```

That oracle was actually run with PyAV **19.0.2**: 19 sample lengths (including
1, 319, 320, 321), at page durations 20/37/1000 ms, **57 complete Ogg streams**.
It verifies exactly `N*3` decoded samples at 48kHz and exactly `N` after resampling
to 16kHz. Sub-5ms clips disable only the development resampler's filter, avoiding
its unrelated tiny-clip filter buffering. These checks do not claim parity of
encoded bytes between libopus versions or Python's FFmpeg wrapper, service
acceptance, CPU/RSS performance, or cross-platform runtime validation.

## Licences and redistribution

- `opus` 0.4.0: MIT OR Apache-2.0; the MIT option notice is reproduced here.
- `opusic-sys` 0.7.7 and bundled libopus 1.6.1: BSD-3-Clause. Preserve the copyright,
  conditions, disclaimer and referenced royalty-free patent licence information.
- Complete selected codec notices are in `THIRD_PARTY_LICENSES`; include them in
  materials accompanying distributed binaries. Other workspace/runtime dependency
  notices still need their ordinary application-wide redistribution treatment.
- CMake is BSD-3-Clause build tooling; the isolated wheel is not shipped with a
  Rust binary. Its own notices remain in that separate tool distribution.
- `ogg`, `tempfile`, test-util and PyAV are test/development dependencies only;
  no test-only decoder or Python is pulled into the runtime library.
