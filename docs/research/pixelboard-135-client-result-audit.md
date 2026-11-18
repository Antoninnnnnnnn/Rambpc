# PixelBoard 1.3.5: client-result source audit

**NO_CANDIDATE** for the current Rambpc engine. PixelBoard 1.3.5 changes how its Android client handles an already-created edit action, but the public source does not establish a new S3 projection or a safe mapping from that local action to a Rambpc proposal.

Reviewed against Rambpc `2658ffb619ce91b0f456b5330e5eac64b286f637` on 2026-11-18. GitHub resolves PixelBoard `1.3.4` to the [previously audited source][baseline] and `1.3.5` to [the release source][tag-diff] (published 2026-11-17). Public `main` was [`4b4e48d`][main-diff], one commit after the tag, with no further Java/Kotlin client-source changes. This is a read-only audit of public GPL-3.0 source and release metadata; no artifacts, upstream tests, device, or provider were run.

## What actually changed

- **Edit preservation (#167):** [The old filter][old-filter] could discard an existing diff when it did not recognize the local action as a command. For example, a local dictation action carrying an in-place word replacement lost its diff under 1.3.4; [the 1.3.5 filter][new-edit] preserves it when the cursor has text after it. A nonblank selection or diff suffix also protects an edit from the end-of-field echo heuristic. [The pinned tests][edit-tests] model mid-sentence and selected-text replacements, a respoken sentence, and a spoken replacement command. They assert retention on fake local actions, not an S3 result or an actual editor replacement.
- **Insertion instead of context echo (#166):** The old non-command branch could rebuild a span with surrounding text. [The new branch][new-span] no longer prepends that context: it restores the transcript for a discarded errant diff when available, or for a diff-free dictation span that is empty or echoes preceding text. The [pasted-text fixture][dup-test] expects only the newly spoken text. This is a local insertion-span rule, not a reason to strip context from a wire replacement window or label a transcript as ASR text.
- **Not a new result contract:** [Rotation continuity and language-fallback changes][release-changes] affect Android lifecycle and recognition policy, respectively; neither supplies a new result field or proves recognition accuracy. [`417ff1f`][readiness] is native split-loading readiness for Gboard `18.6.3.100`, the base [already specified in 1.3.4][old-base]. [`a5d74d2`][writing-tools] changes configuration/defaults for the separate Writing Tools feature, not Rambler result handling; the release's connectivity claim was not verified. The [tagged source notes][tag-compat] and [release page][release] disagree on Android minimum (10/API 29 vs. 12L); installed compatibility is not established here.

## What Rambpc would need

The [contract audit](rust-advanced-mode-contract-audit.md) and [parity matrix](rust-feature-parity.md) distinguish the S3 helper from the later Android filter. At the reviewed Rust revision, `S3ClientCompatible` still accepts wire intents 2/3 with member 3/7 and otherwise falls back to `Hypothesis.text` with provenance. Replacement proposals preserve the captured window and do not mutate an editor. PixelBoard's [local action-4 rewrite][new-span] already existed at 1.3.4 and still does not establish a mapping from wire intents 4/5/6 to those local actions. Nothing here warrants changing strict refusals or safety checks.

The missing discriminator is a sanitized, version-matched reference pair for an in-place edit and an end-of-field echo: the raw final (intent, active member, ASR and transcript), the captured editor snapshot/revision, the local action/diff and span before and after filtering, and the actual applied range and resulting editor snapshot. This would show whether Rust is missing a portable projection or whether the difference belongs only in an editor adapter.

[baseline]: https://github.com/Akshayykadam/PixelBoard/commit/9a81f8b03d1d26b2ebac07355362865f72452137
[tag-diff]: https://github.com/Akshayykadam/PixelBoard/compare/9a81f8b03d1d26b2ebac07355362865f72452137...5530251157dfe8733696ca40d7eb7cf70097d740
[main-diff]: https://github.com/Akshayykadam/PixelBoard/compare/5530251157dfe8733696ca40d7eb7cf70097d740...4b4e48dd4486f29d1140a3b36144933d90a1b7c4
[old-filter]: https://github.com/Akshayykadam/PixelBoard/blob/9a81f8b03d1d26b2ebac07355362865f72452137/pixelboard-patches/extensions/extension/src/main/java/com/akshaykadam/pixelboard/extension/rambler/GboardRamblerDictationRuntime.java#L174-L224
[new-edit]: https://github.com/Akshayykadam/PixelBoard/blob/5530251157dfe8733696ca40d7eb7cf70097d740/pixelboard-patches/extensions/extension/src/main/java/com/akshaykadam/pixelboard/extension/rambler/GboardRamblerDictationRuntime.java#L217-L313
[new-span]: https://github.com/Akshayykadam/PixelBoard/blob/5530251157dfe8733696ca40d7eb7cf70097d740/pixelboard-patches/extensions/extension/src/main/java/com/akshaykadam/pixelboard/extension/rambler/GboardRamblerDictationRuntime.java#L174-L210
[edit-tests]: https://github.com/Akshayykadam/PixelBoard/blob/5530251157dfe8733696ca40d7eb7cf70097d740/pixelboard-patches/extensions/extension/src/test/java/com/akshaykadam/pixelboard/extension/rambler/GboardRamblerDictationRuntimeTest.java#L373-L495
[dup-test]: https://github.com/Akshayykadam/PixelBoard/blob/5530251157dfe8733696ca40d7eb7cf70097d740/pixelboard-patches/extensions/extension/src/test/java/com/akshaykadam/pixelboard/extension/rambler/GboardRamblerDictationRuntimeTest.java#L174-L198
[release-changes]: https://github.com/Akshayykadam/PixelBoard/blob/4b4e48dd4486f29d1140a3b36144933d90a1b7c4/RELEASE_NOTES.md#L20-L29
[readiness]: https://github.com/Akshayykadam/PixelBoard/commit/417ff1f4dba4c26607f22e058203eccb7955ca57
[writing-tools]: https://github.com/Akshayykadam/PixelBoard/commit/a5d74d20498b4e624fcfde59d92c1acfba986433
[old-base]: https://github.com/Akshayykadam/PixelBoard/blob/9a81f8b03d1d26b2ebac07355362865f72452137/README.md#L47-L53
[tag-compat]: https://github.com/Akshayykadam/PixelBoard/blob/5530251157dfe8733696ca40d7eb7cf70097d740/RELEASE_NOTES.md#L67-L75
[release]: https://github.com/Akshayykadam/PixelBoard/releases/tag/1.3.5
