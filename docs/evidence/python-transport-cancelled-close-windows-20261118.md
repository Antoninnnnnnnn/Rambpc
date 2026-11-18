# Python reference: interrupted transport close, Windows

The 2026-11-18 [audit](../research/audit-moteur-20261118.md) reported one interrupted run in the asynchronous producer cleanup test. A later run passed. The audit did not establish the cause of that wait.

Two separate ownership defects were reproduced deterministically against the Python reference from `7ceb590`:

- Cancel the coroutine waiting for `GrpcTransport.close()` while the producer's asynchronous cleanup is gated. A subsequent `close()` returned successfully before the producer released its resources. Regression also checks `open()` cannot create a replacement channel during that cleanup.
- Cancel the waiter for an individual stream's `aclose()` while its cleanup is gated. The transport detached the still-active session. With an injected cleanup error, Python 3.14 also logged the error from the abandoned shield. The transport now retains a joinable cleanup and reports the original failure to the next owner, without automatic logging.

Only the waiter stops on cancellation: the transport continues to own the cleanup task; a following close or reopen joins it. The producer must still cooperate, and the application must keep the event loop alive. No background reconnection or audio replay was added.

**Qualification:** Windows, Python 3.14.5, grpcio 1.85.1, owned IPv4 loopback. The regression tests failed before correction and passed after it. The offline Python suite plus a bounded stress fixture passed **928 tests, 4 skipped, 968 successful subtests**. The stress fixture invoked the originally audited scenario 200 times and found no lingering Python transport tasks after each iteration. Windows Job containment was empty after exit. The optional stress fixture checks for pending tasks and reports code locations without locals on timeout; it is not kept in the checkout. This does not prove the cause of the original 761 s interruption, or that it cannot recur.

```sh
python -m pytest -q tests/test_transport.py
```
