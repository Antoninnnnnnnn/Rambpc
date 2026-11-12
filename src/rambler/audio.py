"""Local PCM16LE WAVE reader; no claim about remote service support."""

import asyncio
import struct
import sys
import time
import wave
from dataclasses import dataclass
from pathlib import Path
from typing import AsyncIterator


@dataclass(frozen=True)
class PcmFormat:
    sample_rate: int
    channels: int = 1
    sample_width: int = 2

    def __post_init__(self) -> None:
        for name, value in (("sample_rate", self.sample_rate), ("channels", self.channels),
                            ("sample_width", self.sample_width)):
            if not isinstance(value, int) or isinstance(value, bool) or value <= 0:
                raise ValueError(f"{name} must be a positive integer")


class _PcmWaveRead(wave.Wave_read):
    def _read_fmt_chunk(self, chunk) -> None:
        header = chunk.read(16)
        if len(header) < 16:
            raise EOFError
        bits = struct.unpack_from("<H", header, 14)[0]
        if bits != 16:
            raise ValueError("only 16-bit WAVE is supported")
        chunk.seek(0)
        super()._read_fmt_chunk(chunk)


class WavAudio:
    """Read uncompressed, 16-bit, mono PCM WAVE in aligned asynchronous chunks."""

    def __init__(self, path: str | Path, *, chunk_ms: int = 40, realtime: bool = False):
        if not isinstance(chunk_ms, int) or isinstance(chunk_ms, bool) or chunk_ms <= 0:
            raise ValueError("chunk_ms must be a positive integer")
        if not isinstance(realtime, bool):
            raise ValueError("realtime must be bool")
        self.path = Path(path)
        self.chunk_ms = chunk_ms
        self.realtime = realtime
        try:
            with _PcmWaveRead(str(self.path)) as wav:
                self._validate(wav)
                self.format = PcmFormat(wav.getframerate(), wav.getnchannels(), wav.getsampwidth())
                self.frame_count = wav.getnframes()
        except (wave.Error, EOFError, OSError) as exc:
            raise ValueError(f"invalid or unreadable WAVE file: {self.path}") from exc
        self.duration_s = self.frame_count / self.format.sample_rate

    @staticmethod
    def _validate(wav: wave.Wave_read) -> None:
        if wav.getcomptype() != "NONE":
            raise ValueError("only uncompressed PCM WAVE is supported")
        if wav.getnchannels() != 1:
            raise ValueError("only mono WAVE is supported")
        if wav.getsampwidth() != 2:
            raise ValueError("only 16-bit WAVE is supported")
        if wav.getframerate() <= 0:
            raise ValueError("sample rate must be positive")
        # wave.getnframes() rounds down, otherwise hiding an incomplete sample.
        if wav._data_chunk.chunksize % (wav.getnchannels() * wav.getsampwidth()):
            raise ValueError("WAVE audio data is not frame-aligned")

    async def chunks(self) -> AsyncIterator[bytes]:
        """Yield file chunks paced by each chunk's start offset, not capture completion.

        Realtime pacing models sending already-available file data at audio-time
        offsets; it does not simulate capture finishing at each chunk boundary.
        The first chunk is immediate and no delay follows the last chunk.
        """
        expected_format: PcmFormat
        expected_frames: int
        try:
            wav = _PcmWaveRead(str(self.path))
        except (wave.Error, EOFError, OSError) as exc:
            raise ValueError(f"invalid or unreadable WAVE file: {self.path}") from exc
        try:
            self._validate(wav)
            expected_format = PcmFormat(wav.getframerate(), wav.getnchannels(), wav.getsampwidth())
            expected_frames = wav.getnframes()
            if expected_format != self.format or expected_frames != self.frame_count:
                raise ValueError("WAVE header changed since construction")
            bytes_per_frame = self.format.channels * self.format.sample_width
            frames_per_chunk = max(1, self.format.sample_rate * self.chunk_ms // 1000)
            frames_read = 0
            start = time.monotonic()
            while frames_read < expected_frames:
                count = min(frames_per_chunk, expected_frames - frames_read)
                payload = wav.readframes(count)
                expected_bytes = count * bytes_per_frame
                if len(payload) != expected_bytes:
                    raise ValueError("WAVE audio data is truncated")
                # wave.readframes converts little-endian samples to host order.
                if sys.byteorder == "big":
                    swapped = bytearray(payload)
                    swapped[0::2], swapped[1::2] = payload[1::2], payload[0::2]
                    payload = bytes(swapped)
                chunk_offset = frames_read / self.format.sample_rate
                if self.realtime and chunk_offset > 0:
                    deadline = start + chunk_offset
                    delay = deadline - time.monotonic()
                    if delay > 0:
                        await asyncio.sleep(delay)
                yield payload
                frames_read += count
        except (wave.Error, EOFError) as exc:
            raise ValueError("invalid or truncated WAVE audio data") from exc
        finally:
            wav.close()
