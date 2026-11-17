# Native wire termination coverage

A complete item is not proof that its containing stream or message ended correctly. This proof pack tests that boundary at the protobuf wire and gRPC transport layers, without treating an S3 END message as a transport status.

- **Protobuf (`wire_boundaries.rs`).** An independent base-128 field generator uses just original synthetic values. Its 560 field descriptors supply expected values and exact bytes, including padded tags, lengths and scalars. The sweep crosses all tag-width transitions and the maximum legal field number. Every interior cut of the second field is rejected even after a complete first field; size and field-count precedence and exact overflow/tag errors are checked separately. This complements the existing typed proto2 merge/run oracles rather than duplicating them.
- **Transport (`http2.rs`).** A loopback-only shape matrix checks genuine Trailers-Only HEADERS, initial status followed by empty DATA with END_STREAM, and normal DATA plus trailers. Empty DATA and an empty gRPC message are different. Before the fix, the middle shape with `grpc-status: 0` incorrectly returned `Ok(Completion)`; the client now rejects any DATA frame after an initial status, while preserving a gRPC status on either valid shape. A gated stream-reset test checks that a complete synthetic S3 END payload remains visible, an incomplete following envelope never becomes a response, and dropping a pending `finish()` observation does not acknowledge a later failure. A live sibling's explicit gRPC `Cancelled` and the interrupted stream's HTTP/2 reset map to the same public code but have different terminal-status provenance. A healthy follow-up proves channel reuse; counters prove one source close and one request per explicit start.

Reference semantics: [Protocol Buffers wire encoding](https://protobuf.dev/programming-guides/encoding/), including the SDK's documented raw-preserving handling of padded varints; [gRPC over HTTP/2](https://github.com/grpc/grpc/blob/master/doc/PROTOCOL-HTTP2.md), especially Trailers-Only; and [HTTP/2 DATA and RST_STREAM](https://www.rfc-editor.org/rfc/rfc9113.html#section-6). These tests do not require a provider, credentials, recordings, an external service, or captured traffic.

## Reproduce

Run the following Cargo arguments from an isolated checkout with the stable cached toolchain and an owned target directory. On Windows, use the repository's `tools/windows_owned_job.py` to bound subprocesses, with only the approved OS/tool environment. The separate TLS downgrade smoke is omitted here so the test batch does not load host trust roots.

```text
cargo +stable test --locked --offline -j 2 -p rambler-protocol --test wire --test wire_boundaries --test decode --test adversarial --test oneof_runs --test response_containers
cargo +stable test --locked --offline -j 2 -p rambler-transport --test http2 --test lifecycle --test loopback -- --skip tls_never_downgrades_to_loopback_plaintext
cargo +stable clippy --locked --offline -j 2 -p rambler-protocol -p rambler-transport --all-targets -- -D warnings
cargo +stable fmt -p rambler-protocol -p rambler-transport -- --check
```

Windows validation against base `937d3cc683b949446da3621c041d9412cc154a7c` plus this change: 45 protocol and 60 transport tests passed; Clippy and rustfmt passed. Running just `trailers_only_status_cannot_use_data_end_stream` with the base transport implementation reproduced the false success.
