# S3 RecognizerInfo options: explicit presence, without a server promise

Initial implementation on `fdc9fd9`, verified offline on 2026-11-14, then integrated into the CLI and native runner. S3 init uses extension **294500**. No APK execution, new backend or change to result policies. Additional remote verification is described separately below.

## API and encoding

Public `ProtocolConfig` fields, usable with `..Default::default()`:

| Rust field | RecognizerInfo field | `None` | Explicit values |
|---|---|---|---|
| `block_offensive_words: Option<bool>` | **7, INT32**, wire0 | omitted | `Some(false)` → 0 (`38 00`), `Some(true)` → 2 (`38 02`) |
| `auto_punctuation: Option<bool>` | **40, BOOL**, wire0 | omitted | `Some(false)` → 0 (`c0 02 00`), `Some(true)` → 1 (`c0 02 01`) |
| `secondary_routing: Option<String>` | **64, STRING**, wire2 | omitted | nonempty UTF-8 string, key `82 04`, length in bytes |

All three defaults are `None`. **Absence is not false**: the member associated with field 7 in the APK schema has initial value **2**. The API does not translate omission into an assumed server value or policy. Field 7 is neither a BOOL nor a new interpretation of VoiceEdit intentions.

Local assembly example only:

```rust
use rambler_protocol::{initial_request, ProtocolConfig};

let config = ProtocolConfig {
    block_offensive_words: Some(true),
    auto_punctuation: Some(false),
    secondary_routing: Some("caller-secondary-routing".into()),
    ..Default::default()
};
let bytes = initial_request(&config, 0)?;
# Ok::<(), rambler_protocol::Error>(())
```

Primary routing remains `routing`, field 50, independent of field 64. The observed Android client writes field 64 under `kmy.n` (Jetson) and only if `muq.B` is nonempty. The Rust API exposes this verified string without inventing a Jetson flag, forced coupling with an unknown primary key or implicit mode. The caller must provide an appropriate key; no remote key is inferred.

### Bounds and privacy

- `Some("")` is rejected like primary routing; no interpretation of an empty string as a special mode. `None` remains valid.
- Same validation as `routing`: nonempty string, at most `MAX_MESSAGE_BYTES` (**8 MiB**) of UTF-8. This defensive SDK bound is not an observed server limit. An individually valid string can still exceed the submessage or complete request bound with their envelopes.
- Both keys and other extensions remain subject to the assembler's cumulative bounds. Spaces, NUL, combining accents and supplementary characters are neither normalized nor removed, as with `routing`.
- The three new options are excluded from the manual `ProtocolConfig` `Debug`. Errors do not contain their contents. Historical `Debug` of already visible fields, including `routing`, is unchanged.
- These fields are init-only; neither hints nor options are copied to late context, audio or END. No change to selection, edit scope or text application.

## Static evidence and provenance

APK reference and levels of evidence: [rust-feature-parity.md](rust-feature-parity.md). APK SHA-256 **taken from that matrix**, not presented as a new hash: `a4628c2e42d3f62028742ee1880c33ce5d96e2a034fcb72ad16a586320cc8fa8`. Obfuscated names and line numbers concern only this build and the indicated extracts; they are not universal protocol identifiers.

Local extracts reread (text only):

- `C:/Users/FSOS/AppData/Local/Temp/claude/C--Users-FSOS-Documents-project-Rambpc/6a88b7b9-7237-4fb1-9c07-58a4c0a8ca3e/scratchpad/pixelboard-jadx/sources/defpackage/aapf.java` :22,29,44: `g=2`, extension 294500, schema associating `g`→7 INT32, `l`→40 BOOL and `i`→64 STRING (`h`→50 STRING).
- `C:/Users/FSOS/AppData/Local/Temp/claude/C--Users-FSOS-Documents-project-Rambpc/6a88b7b9-7237-4fb1-9c07-58a4c0a8ca3e/scratchpad/pixelboard-jadx-knv-simple/sources/defpackage/knv.java` :35,49–59: value from `sba.s` to `l` with explicit presence; `kmy.h` to `g` with presence, false→0, true→2 (:376–378). :87–105: `kmy.n` branch, primary key `muq.A` to `h`, then nonempty secondary key `muq.B` to `i`.
- `C:/Users/FSOS/AppData/Local/Temp/claude/C--Users-FSOS-Documents-project-Rambpc/6a88b7b9-7237-4fb1-9c07-58a4c0a8ca3e/scratchpad/pixelboard-jadx-ffh-simple/sources/defpackage/ffh.java` :2000–2004: bundle `session_type="jetson"`.
- `C:/Users/FSOS/AppData/Local/Temp/claude/C--Users-FSOS-Documents-project-Rambpc/6a88b7b9-7237-4fb1-9c07-58a4c0a8ca3e/scratchpad/pixelboard-jadx-ihv/sources/defpackage/ihv.java` :106–128: header construction then `ihuVar.a(aaegVar)` before sending.

Call chain verified by Kimi, and passages below reread locally in JADX extracts (class, lines; not executed):

1. `fgr`,98–103 → `ffh.u`,2000–2004 supplies the Jetson bundle.
2. `kla`,183–250 reads this bundle and supplies Jetson parameters (`kmy.n`).
3. `kki`,63–85 conditionally chooses Jetson providers, including NewS3; this is not a guarantee that every device will choose that provider.
4. `knn`,25–35 creates `knt` when the gate accepts `kmy.n`.
5. `knt`,74–110 injects **`new knv(context, kmyVar)`** into `iho`; :340 starts `iho.j`.
6. `iho`,280–304 calls `ihv.a(..., g, ...)` for the stream concerned, including its branch with a secondary stream.
7. `ihv`,106–128 applies `ihuVar.a(builder)`; the injected mutator is `knv.a`.

This establishes a concrete S3 request capability, not just an isolated schema field. It **does not validate the service effect** on punctuation, lexical filtering or routing. No Google test had been performed in the initial protocol slice; the additional tests below prove only request acceptance for fields 7/40 on the tested profile.

## Initial offline protocol verification

Batch files only:

- `crates/rambler-protocol/src/request.rs`: configuration, validation, init encoder.
- `crates/rambler-protocol/tests/recognizer_options.rs`: **7 new tests**.
- `docs/research/rust-recognizer-options.md`: this document.

New tests use a wire reader independent of the production `wire` module and literal hexadecimal vectors. They check:

- omission of all three fields and native golden unchanged byte for byte; the two old Python/custom goldens remain checked by the existing `requests` suite;
- field 7 absent/0/2 and field 40 absent/false/true, including explicit zeroes;
- **18 combinations** of presence/values, with hints, preferences and secondary languages; no other extension changes and configuration is intact;
- exact UTF-8 field 64, intact primary key, spaces/NUL/non-normalization;
- validation identical to primary routing, empty/oversize rejected, UTF-8 bounds, request exactly 8 MiB accepted and +1 byte rejected, oversized sum of keys rejected even if each string is valid;
- `Debug`/error privacy and unchanged context/audio/END vectors.

Command from the worktree, short target `.t`, without debug symbols or incremental compilation:

```sh
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 \
CARGO_TARGET_DIR="$PWD/.t" cargo test --offline --locked -p rambler-protocol
```

Result: **63 tests passed, 0 failed**, including doctest. `rustfmt --check` on the two Rust files in the batch and `cargo clippy --offline --locked
-p rambler-protocol --tests -- -D warnings` also pass with the same debug-free profiles. Temporary target `.t` is deleted after verification. No engine, transport, audio, CLI, Opus or CMake build/test. No manifest or lockfile change.

## Unchanged boundaries

**RecognizerInfo** field 7 is distinct from safety field 30 of **response hypotheses**. No change to `check_safety`, rendering/proposal rejections or their tests: the option is not a safety bypass. No COMPOSE, intention 0/1/5/6 implementation, opaque flag or server mode inferred.

Legacy selection is already encodable with `context.before = selection` and `cursor_aware = false`. No `SessionOptions` flag, destructive proposal or new replacement scope is added. Options remain opt-in. The integrated CLI then exposes `--block-offensive-words true|false`, `--auto-punctuation true|false` and `--secondary-routing VALEUR` for both commands. The `session_reuse` runner accepts the three corresponding members in its manifest, without publishing them in outputs or changing its limits. Loopback tests inspect init fields actually received for omitted, false and true values.

## Bounded remote verification after integration

[Public report](../evidence/rust-recognizer-options-public.json): three sessions on the same ordinary synthetic clip, one engine and one explicit connection call, zero retries. Fields 7/40 omitted, then false/false, then true/true, declared before execution. All finish with PCM actually consumed/hashed, EOF and close, S3 END and gRPC OK status; empty stderr. All three texts have the same hash as the known reference: no formatting change observed here.

This **does not demonstrate** lexical filter or auto-punctuation effectiveness. The text is not offensive; no attempt is made to bypass a blockage. The server could handle these preferences differently by routing or profile. Field 64 remains omitted: no unknown key was guessed. Source/lock/binary hashes and the pre-execution plan distinguish this evidence from protocol tests and old observations.
