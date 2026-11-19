# Android application context: S3 boundary already encodable

## Conclusion — November 16, 2026

**No missing S3 field established.** In the static chain examined, Android application
context text reaches **S3Request → extension 1203 InputBoxInfo → field 5, repeated
string**. Rust already exposes and encodes this representation through
`ProtocolConfig::preferences`. This does not demonstrate runtime activation of
this branch, or server-side interpretation or effect of the text.

Android collection (`EditorInfo`, application information), its gates and its
formatting have not been ported. These are missing platform integrations, not a
protobuf capability to add. This trace alone does not justify a new switch, field,
sent prompt or remote test.

## Verifiable static chain

The names refer to the APK build identified in the [parity matrix](rust-feature-parity.md#sources-and-levels-of-evidence).
The decompiled files are unexecuted local data; they are not redistributed here.

1. `ffh.u(kmx)`, targeted `ffh-simple` extraction, lines 2000–2077: under gate
   `muq.X`, application information feeds an element of the Bundle's
   `user_preferences` list. The distinct gate `muq.Y` adds custom instructions
   to the same list, without a new network message type.
2. `kla.java:207–219,234–250` reads the list, filters null entries and passes it
   through `kmw.j`. `kmw.java:122–127` / `kmy.java:25–42,65–68` call it
   `userPreferences` and store it in `kmy.o`.
3. `knt.java:78,98–110` passes `kmy.o` through `iib.e` to the `iic` constructor,
   member `g`, then to `iho`. `iho.java:280–304` calls the `ihv.a` initializer.
4. The instruction stream recovered from `ihv.a` selects the NewS3 recognizer
   at lines 15–27, copies `iic.g` to `aamo.e` at lines 155–164 and attaches
   extension `aamo.f` at lines 205–212. The simplified JADX output had an
   incomplete attachment branch; it is not used as negative evidence.
5. `aamo.java:13–18` defines message extension **1203** of parent `aama`; metadata
   at lines 27–31 places `e` at **field 5, repeated string**.

This is the **initial** request, not late editing context. An empty list has no
scalar presence; the extension can be omitted if the other InputBox contents are
also absent. The request mutator is still applied after this assembly; no dynamic
observation of its effects is inferred.

## Existing Rust and serialization evidence

- `crates/rambler-protocol/src/request.rs:11–31` exposes `preferences: Vec<String>`.
- `initial_request`, lines 172–180, encodes each preference at field 5, each bias
  at field 3, then wraps them in `INPUT_BOX_INFO` (1203).
- The existing golden `hint_goldens_empty_entries_unicode_order_duplicates`, in
  `crates/rambler-protocol/tests/requests.rs:191–221`, fixes preference `P` as
  `2a 01 50` in this extension and covers order, duplicates and Unicode.
- Preferences remain separate from late context. No dedicated
  `application_context` member is needed to represent the same bytes.

Verification of this boundary is a targeted static reading, not a new test run or
server call. It would be refuted by a trace showing that the same element reaches
another field on this route, or by a failure of the golden to emit extension
1203/field 5. Effective flags, runtime route selection and server effect remain
unknown. No end-to-end Android parity, COMPOSE feature, automatic OS collection
or additional quality is claimed.
