# Native qualification kit

## Scope and status

This contributor tool exercises fixed synthetic fixtures on an owned IPv4
localhost peer using one shared native Engine and transport. It does not accept
an address, files, credentials, or an external corpus and does not activate a
real server mode or use a device. See [STATUS](../STATUS.md) for observed results
rather than treating the procedure below as a measurement.

## Reproduce and compare

From the repository root, with the project's Rust toolchain and locked
dependencies already available offline, run:

```sh
cargo run --offline --locked --release -p rambler-cli --features audio --example native_qualification -- --repeats 1
```

The default is one repeat; `--repeats` accepts a canonical integer from 1 to 10.
On success stdout contains one JSON report. Capture the output separately for
both versions as `baseline.json` and `candidate.json`. Keep reports ephemeral or
version them only after checking that they contain synthetic and nonsecret
metadata. Never include credentials, private audio, context or service traffic.

```sh
python -I -S -B tools/compare_native_qualification.py baseline.json candidate.json
```

Build and capture are outside the timed region. Run baseline and candidate on the **same machine**, with the same toolchain, environment, workload, repeat count and declared run conditions; record those facts independently, not as an inference from matching report output. Alternate baseline and candidate runs to reduce order bias, preserving individual reports and noting warm-up and disturbances. Compare only like-for-like cases. Report the sample count and descriptive median and range for each timing; small bounded samples are not a basis for a universal performance claim.

## Fixture and fidelity gate

Each fixture must preserve its exact initialization, audio, late context, END and EOF sequence. The cases exercise distinct response semantics:

- **literal**: final only.
- **rewrite**: known intent 3 / member 3, checked with strict rendering.
- **selection**: intent 2 / member 7, with a known Unicode range and no UI effect.
- **preview**: two field 4 estimates before a distinct final response.

Before interpreting timing, the reports must agree for every corresponding case:
matching PCM length and hash, ordered wire-response and strict-rendered digests,
request/response counts, and Ogg byte counts. If a report is invalid or a field
differs, the comparator exits nonzero and omits timing. Otherwise it keeps the
paired deltas (**candidate minus baseline**, in microseconds), with their median
and range per case. Matching reports do not attest the build or the environment;
record the exact binaries and test conditions separately.

The timing metrics `first_response_us` and `session_us` describe the synthetic owned-loopback session in microseconds. They are not TLS, device, WAN, or real ASR measurements. Do not attribute these client-observed timings to a remote engine or extrapolate them to production.

## From kit to authorized service acceptance

A passing synthetic comparison is a local regression check, not service qualification. For a genuinely authorized service, use the [current acceptance criteria](../ROADMAP.md) and [corpus annotation plan](quality-annotation-plan.md). Use an appropriately consented corpus, document the actual authorized setup, and record explicit pass/fail evidence separately for transport, recognition and Rambler behavior. Preserve only nonsecret, sanitized evidence and artifact hashes under the project's evidence policy; never guess provider access, collect private traffic, or claim service results from loopback alone. Consult [STATUS](../STATUS.md) for observed results.
