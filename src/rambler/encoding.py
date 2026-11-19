"""Optional, bounded-memory native PCM16LE -> Ogg/Opus streaming.

The ``audio`` extra needs Python >= 3.12; importing this module does not import
PyAV. Only the backend-validated mono, 16 kHz, 24 kbit/s VoIP profile is exposed.
"""

from __future__ import annotations

import asyncio
import sys
from collections import deque
from collections.abc import AsyncIterable, AsyncIterator, Iterator
from fractions import Fraction


_MAX_INPUT_BYTES = 8 * 1024 * 1024
_FRAME_SAMPLES = 320  # 20 ms at 16 kHz
_FRAME_BYTES = _FRAME_SAMPLES * 2
_TIME_BASE = Fraction(1, 16000)
_MAX_PENDING_BYTES = 64 * 1024


class _WriteSink:
    """Nonseekable AVIO target, drained after every encode/mux operation.

    The hard bound detects accidental buffering regressions instead of silently
    dropping bytes (as deque(maxlen=...) would). This is not a file accumulator.
    """

    def __init__(self) -> None:
        self._chunks: deque[bytes] = deque()
        self._pending_bytes = 0

    def writable(self) -> bool:
        return True

    def seekable(self) -> bool:
        return False

    def write(self, data: bytes) -> int:
        size = len(data)
        if self._pending_bytes + size > _MAX_PENDING_BYTES:
            raise BufferError("Ogg output exceeded the bounded AVIO drain buffer")
        if size:
            self._chunks.append(bytes(data))
            self._pending_bytes += size
        return size

    def drain(self) -> Iterator[bytes]:
        while self._chunks:
            chunk = self._chunks.popleft()
            self._pending_bytes -= len(chunk)
            yield chunk

    def clear(self) -> None:
        self._chunks.clear()
        self._pending_bytes = 0


def _byteswap_pcm16(pcm: memoryview | bytearray) -> bytearray:
    """Convert one sample-aligned frame from LE to BE, not a whole source."""
    swapped = bytearray(len(pcm))
    swapped[0::2] = pcm[1::2]
    swapped[1::2] = pcm[0::2]
    return swapped


class _OpusEncoder:
    def __init__(self, page_duration_ms: int) -> None:
        try:
            import av
        except ModuleNotFoundError as exc:
            if exc.name != "av":
                raise
            raise ImportError(
                "PCM encoding requires Sambler's audio extra. Install it from the checkout "
                "with python -m pip install '.[audio]' (Python >= 3.12)."
            ) from exc

        self._av = av
        self.sink = _WriteSink()
        self._container = None
        self._pts = 0
        try:
            self._container = av.open(
                self.sink,
                mode="w",
                format="ogg",
                # Ogg's page_duration is in microseconds. flush_packets makes
                # complete pages reach Python instead of waiting for 32 KiB AVIO.
                options={
                    "page_duration": str(page_duration_ms * 1000),
                    "flush_packets": "1",
                },
                buffer_size=1024,
            )
            self._stream = self._container.add_stream("libopus", rate=16000)
            self._stream.layout = "mono"
            self._stream.format = "s16"
            self._stream.bit_rate = 24000
            self._stream.codec_context.time_base = _TIME_BASE
            self._stream.codec_context.options = {
                "application": "voip",
                "frame_duration": "20",
                "vbr": "on",
            }
            self._container.start_encoding()
        except BaseException:
            try:
                self.close()
            finally:
                self.sink.clear()
            raise

    def encode(self, pcm: memoryview | bytearray) -> Iterator[bytes]:
        samples = len(pcm) // 2
        frame = self._av.AudioFrame(format="s16", layout="mono", samples=samples)
        plane = frame.planes[0]
        # FFmpeg's s16 is native-endian, whereas the API always accepts LE.
        # Keep the normal little-endian path free of any conversion/copy.
        if sys.byteorder == "big":
            pcm = _byteswap_pcm16(pcm)
        if plane.buffer_size == len(pcm):
            plane.update(pcm)
        else:
            # Only align the native allocation, never extend frame.samples.
            padded = bytearray(plane.buffer_size)
            padded[:len(pcm)] = pcm
            plane.update(padded)
        frame.sample_rate = 16000
        frame.time_base = _TIME_BASE
        frame.pts = self._pts
        self._pts += samples
        for packet in self._stream.encode(frame):
            self._container.mux(packet)
            yield from self.sink.drain()
        yield from self.sink.drain()

    def finish(self) -> Iterator[bytes]:
        # libopus and the Ogg muxer preserve the exact final sample count through
        # pre-skip, packet duration/discard padding, and the EOS granule position.
        for packet in self._stream.encode(None):
            self._container.mux(packet)
            yield from self.sink.drain()
        self.close()
        yield from self.sink.drain()

    def close(self) -> None:
        container, self._container = self._container, None
        if container is not None:
            container.close()


async def pcm16_to_ogg(
    audio: AsyncIterable[bytes],
    *,
    sample_rate: int = 16000,
    channels: int = 1,
    bitrate: int = 24000,
    page_duration_ms: int = 20,
) -> AsyncIterator[bytes]:
    """Encode arbitrarily fragmented PCM16LE without accumulating a whole file.

    Input must be mono/16 kHz with bitrate 24000. Each input chunk must be
    ``bytes`` of at most 8 MiB; empty chunks are ignored. A wholly empty source
    produces no output and does not load PyAV. Page duration is an integer in
    20..1000 ms (independent of the fixed 20 ms codec frame).

    Odd chunk boundaries are allowed, but an isolated final byte is rejected as
    truncated PCM. A partial *sample-aligned* last frame is encoded without
    adding silence or dropping samples. On an input error, any already emitted
    Ogg prefix is incomplete: no successful EOS is yielded.

    The upstream iterator is closed on completion, failure, cancellation, or
    explicit ``aclose()``. Consumers stopping early must close this async
    generator (e.g. with ``contextlib.aclosing``). Cancellation is cooperative
    even for huge chunks and sources whose ``__anext__`` never suspends.
    """
    upstream = aiter(audio)
    encoder = None
    pending = bytearray()
    try:
        for name, value, expected in (
            ("sample_rate", sample_rate, 16000),
            ("channels", channels, 1),
            ("bitrate", bitrate, 24000),
        ):
            if type(value) is not int or value != expected:
                raise ValueError(f"{name} must be {expected} for the validated PCM profile")
        if type(page_duration_ms) is not int or not 20 <= page_duration_ms <= 1000:
            raise ValueError("page_duration_ms must be an integer in 20..1000")

        async for chunk in upstream:
            await asyncio.sleep(0)
            if not isinstance(chunk, bytes):
                raise TypeError("PCM chunks must be bytes")
            if len(chunk) > _MAX_INPUT_BYTES:
                raise ValueError("PCM input chunk exceeds the 8 MiB limit")
            view = memoryview(chunk)
            offset = 0
            while offset < len(view):
                if pending or len(view) - offset < _FRAME_BYTES:
                    count = min(_FRAME_BYTES - len(pending), len(view) - offset)
                    pending.extend(view[offset:offset + count])
                    offset += count
                    if len(pending) < _FRAME_BYTES:
                        continue
                    frame_pcm = pending
                else:
                    frame_pcm = view[offset:offset + _FRAME_BYTES]
                    offset += _FRAME_BYTES
                await asyncio.sleep(0)
                if encoder is None:
                    encoder = _OpusEncoder(page_duration_ms)
                    for data in encoder.sink.drain():
                        yield data
                for data in encoder.encode(frame_pcm):
                    yield data
                pending.clear()

        if len(pending) % 2:
            raise ValueError("truncated PCM16LE: isolated final byte (incomplete sample)")
        if pending:
            if encoder is None:
                encoder = _OpusEncoder(page_duration_ms)
                for data in encoder.sink.drain():
                    yield data
            for data in encoder.encode(pending):
                yield data
        if encoder is not None:
            for data in encoder.finish():
                yield data
    finally:
        try:
            if encoder is not None:
                try:
                    encoder.close()
                finally:
                    encoder.sink.clear()
        finally:
            close = getattr(upstream, "aclose", None)
            if close is not None:
                await close()
