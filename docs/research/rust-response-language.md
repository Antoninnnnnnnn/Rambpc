# Recognition response language fields

The native Rust `RecognitionResponse` names for `aapd` fields 3 and 4 were
incorrect. The supplied decompilation evidence identifies `suz.java:130–133`
calling `getLanguage()` and `getSpokenLanguage()`; `ihm.java:156,244` copies
field 3 into `VoiceText.f`. This identifies the fields as language-related,
not a result ID or alternative transcript. These source references are the
basis supplied for this correction; this note does not claim an additional
independent decompilation audit.

| Protobuf field | Rust 0.1 before | Rust 0.1 now |
| --- | --- | --- |
| 3 | `RecognitionResponse.result_id` | `RecognitionResponse.language` |
| 4 | `RecognitionResponse.alternative_text` | `RecognitionResponse.spoken_language` |

This is a source-breaking Rust 0.1 API change for field access and struct
literals; downstream users must rename both fields. No deprecated aliases are
provided. The unrelated `VoiceEditResult.alternative_text` is unchanged.

Wire numbers, UTF-8 validation, presence, singular last-value behavior, raw
bytes, and the independent storage of both strings are unchanged. Identical
values are not deduplicated, normalized, or interpreted as an identifier.
No Python 0.3 behavior or frozen JSON fixture is changed by this native API
correction. It does not claim any additional semantics about the distinction
between the two language strings.
