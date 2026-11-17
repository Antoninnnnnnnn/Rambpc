"""Original, fixed public-only non-reading target for owned-pipe native QA.

No modes, environment reads, files, network, input reads, echoes or diagnostics.
Only READY is emitted. The fixed wait is a backstop, not a service lifetime.
This is NOT the positive fixture, a Linux receiver or a Rambler endpoint.
"""
import os
import sys
import time

READY = b'OWNED_PIPE_BLOCKED_PUBLIC_READY_V1\n'
WAIT_SECONDS = 25.0


def main():
    if sys.argv[1:]:
        return 64
    pending = READY
    while pending:
        count = os.write(1, pending)
        if count <= 0:
            return 125
        pending = pending[count:]
    # Deliberately never read stdin (including EOF). Owned Job cleanup should
    # terminate this target before this fixed, <=30-second wait expires.
    time.sleep(WAIT_SECONDS)
    return 1  # Natural expiry is never protocol success; emit nothing further.


if __name__ == '__main__':
    try:
        status = main()
    except BaseException:
        status = 125
    os._exit(status)  # No traceback, logging or arbitrary exception text.
