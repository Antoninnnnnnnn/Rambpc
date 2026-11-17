"""Original public Python CLI fixture, not a service/client or a real controller.

Only invented public bytes. No environment reads, credential loading, network,
phone, microphone, arbitrary modes or input/output persistence. Never echo input.
"""
import os
import sys

READY = b'OWNED_PIPE_FIXTURE_READY_V1\n'
CHECKED = b'OWNED_PIPE_FIXTURE_TERMINAL_V1 PUBLIC_FRAME_CHECKED\n'
REJECTED = b'OWNED_PIPE_FIXTURE_TERMINAL_V1 REJECT_FRAME\n'
PUBLIC_FIELDS = (b'invented-public-frame-key', b'invalid.public.frame',
                 b'invented-public-frame-cert')
PUBLIC_FRAME = b'RLA1\0\r\n\x1a' + b''.join(
    len(part).to_bytes(2, 'big') + part for part in PUBLIC_FIELDS)


def _write(raw):
    while raw:
        count = os.write(1, raw)
        if count <= 0:
            raise OSError()
        raw = raw[count:]


def main():
    if sys.argv[1:]:
        return 64
    _write(READY)
    data = bytearray()
    # Exactly 85 public bytes AND EOF required; no terminal while EOF withheld.
    # Retain at most 86 bytes, draining/discarding any additional input until EOF.
    while True:
        chunk = os.read(0, 86)
        if not chunk:
            break
        if len(data) < 86:
            data += chunk[:86 - len(data)]
    checked = data == PUBLIC_FRAME and len(PUBLIC_FRAME) == 85
    data.clear()
    _write(CHECKED if checked else REJECTED)
    return 0 if checked else 1


if __name__ == '__main__':
    try:
        status = main()
    except BaseException:
        status = 125  # No diagnostics, traceback, or raw exception content.
    os._exit(status)
