# Python reference: interrupted transport close, Windows

The 2026-11-18 [audit](../research/audit-moteur-20261118.md) reported one interrupted run in the asynchronous producer cleanup test. A later run passed. The audit did not establish the cause of that wait.

Two separate ownership defects were reproduced deterministically against the Python reference from `7ceb590`:

- Cancel the coroutine waiting for `GrpcTransport.close()` while the producer's asynchronous cleanup is gated. A subsequent `close()` returned successfully before the producer released its resources. Regression also checks `open()` cannot create a replacement channel during that cleanup.
- Cancel the waiter for an individual stream's `aclose()` while its cleanup is gated. The transport detached the still-active session. With an injected cleanup error, Python 3.14 also logged the error from the abandoned shield. The transport now retains a joinable cleanup and reports the original failure to the next owner, without automatic logging.

Only the waiter stops on cancellation: the transport continues to own the cleanup task; a following close or reopen joins it. The producer must still cooperate, and the application must keep the event loop alive. No background reconnection or audio replay was added.

**Qualification:** Windows, Python 3.14.5, grpcio 1.85.1, owned IPv4 loopback. The regression tests failed before correction and passed after it. The offline Python suite plus a bounded stress fixture passed **928 tests, 4 skipped, 968 successful subtests**. The stress fixture invoked the originally audited scenario 200 times and found no lingering Python transport tasks after each iteration. Windows Job containment was empty after exit. The optional stress fixture checks for pending tasks and reports code locations without locals on timeout; it is not kept in the checkout. This does not prove the cause of the original 761 s interruption, or that it cannot recur.

## Integrated follow-up

One combined run reached its 120 s owned Job limit with a stack in the test's byte-by-byte zeroization assertion. Checking the same complete region with `bytearray.count(0) == len(buffer)` avoids a Python generator per byte and reports only the count on failure; no production credential code changed. The next run still hit the 120 s limit, this time with the asyncio loop idle. Its quiet progress matched the original producer-cleanup test in a later named run, but the suspended task was not identified in that attempt.

Three subsequent full runs with a per-loop watchdog all passed **931 tests, 4 skipped, 981 successful subtests**, in 35.69 s, 33.40 s and 35.50 s. No watchdog fired, and each owned Job was empty afterwards. Both terminated attempts are retained separately; no assert or failing scenario was skipped.

```sh
python -m pytest -vv -s -o faulthandler_timeout=15 tests/test_transport.py
```

## Independent audit follow-up, 19 November

The audit of `debd06e3fb960eaf2254aa8b330b8918df9dff94` reported three interrupted full runs in the shutdown tests. With the same published candidate, isolated Python 3.14.5 and owned loopback, the original `test_close_waits_for_async_producer_cleanup_and_cancels_server` passed **120/120** times in separate pytest-owned loops, the transport suite passed **72/72** three times, and two full `pytest -q -s` runs each passed **947 tests, 5 skipped, 981 successful subtests**. An asyncio task watchdog was armed in each test loop; no loop snapshot fired. Each Job was empty on exit. The tracing plugin emitted one pytest import-order warning per run, not a transport failure.

The interruptions remain **unexplained**, not fixed or ruled out by these repetitions. No transport change was made based on this follow-up. A future reproduction should capture pending coroutine locations as well as the main thread stack before the test process is stopped, without dumping task locals, audio or metadata.
