# HTTP/2 boundaries: MIME type and invalid metadata

## Why this batch

The parallel pass covered ten backend areas read-only, then cross-checked two proposals and summarized coverage (13 Sol agents in total, no errors/skips reported). One proposal was confirmed by reading the code; the other remained unresolved without an observation of tonic/h2 conversion. Local RED/GREEN tests are distinct from these static conclusions. This pass is not proof of perfection or exhaustive coverage.

## Case-insensitive gRPC MIME type

[RFC 9110 §8.3.1](https://www.rfc-editor.org/rfc/rfc9110.html#section-8.3.1) specifies: “The type and subtype tokens are case-insensitive.” The client nevertheless compared Content-Type bytes to lowercase literals: an HTTP 200 response `Application/Grpc`, with valid gRPC DATA and OK trailers, was rejected before messages were read.

The fix compares only the `application/grpc` base case-insensitively in ASCII, without allocating a lowercase string. The same boundaries remain required: exact end, a suffix beginning with `+`, or parameters beginning with `;`. `application/grpc-web`, a base extended with `X`, duplicates, and encoding/status checks are not made permissive. Payloads are neither rewritten nor decoded differently. This is not an ignored safety-blocking response.

No real service incident is attributed to this defect: the case of its headers was not reobserved in this batch. The reproduction is local HTTP/2.

## Connection headers rejected during validation

Five names prohibited by HTTP/2 are now rejected by `Metadata::new` with `InvalidMetadata`:

- `connection`
- `keep-alive`
- `proxy-connection`
- `transfer-encoding`
- `upgrade`

[RFC 9113 §8.2.2](https://www.rfc-editor.org/rfc/rfc9113.html#section-8.2.2). In the locked dependencies, `tonic` 0.14.9 `MetadataMap::into_headers` does not remove these values and `h2` 0.4.20 rejects them during local request validation. A test with invented metadata and an in-memory Tokio duplex establishes this path, with valid custom metadata as a positive control. No DNS, remote socket, TLS, or account is needed for this observation.

The previous behavior accepted these names when constructing the snapshot, then failed later on h2 submission. The new validation is an explicit early rejection, **not** silent removal. It does not extend the prohibition to all `grpc-*` keys, does not change provider values, and preserves limits, duplicates, binary values, sensitive markers, and the contract of the three environment variables. This tightens handling of inputs that could not form a valid HTTP/2 request; it is not a new authentication mechanism.

## Evidence and reproduction

```sh
cargo test -p rambler-transport --locked --offline
cargo +1.88.0 test -p rambler-transport --locked --offline
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings
```

The [structured evidence](../evidence/rust-http2-boundaries.json) separates initial reproductions, fixes, cross-reviews, and final-tree checks. Full logs remain private; source/log hashes and results are retained.

No Google/device call, renewed real access, replayed request, or change to the S3 protocol, codec, or result policy. No speed or latency gain is claimed; MIME type comparisons occur in header validation, not in the audio encoding path.
