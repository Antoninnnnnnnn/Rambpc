import asyncio
import hashlib
import struct
import wave

import pytest

from rambler.audio import PcmFormat, WavAudio


def write_wav(path, payload, *, rate=8000, channels=1, width=2, comptype="NONE"):
    with wave.open(str(path), "wb") as wav:
        wav.setnchannels(channels)
        wav.setsampwidth(width)
        wav.setframerate(rate)
        if comptype != "NONE":
            wav.setcomptype(comptype, "compressed")
        wav.writeframes(payload)
    return path


def test_chunks_preserve_audio_and_duration(tmp_path):
    samples = [0, 1, -2, 32767, -32768] * 5
    pcm = struct.pack("<" + "h" * len(samples), *samples)
    path = write_wav(tmp_path / "voice.wav", pcm)
    audio = WavAudio(path, chunk_ms=1)
    chunks = asyncio.run(_collect(audio))
    assert audio.format == PcmFormat(8000)
    assert audio.frame_count == len(samples)
    assert audio.duration_s == len(samples) / 8000
    assert b"".join(chunks) == pcm
    assert all(len(chunk) % 2 == 0 for chunk in chunks)
    assert chunks[-1] == pcm[-(len(pcm) % (8000 * 2 // 1000) or (8000 * 2 // 1000)):]
    assert hashlib.sha256(b"".join(chunks)).digest() == hashlib.sha256(pcm).digest()


async def _collect(audio):
    return [chunk async for chunk in audio.chunks()]


def test_two_independent_reads(tmp_path):
    pcm = struct.pack("<5h", 1, 2, 3, 4, 5)
    audio = WavAudio(write_wav(tmp_path / "twice.wav", pcm), chunk_ms=1)
    first = asyncio.run(_collect(audio))
    second = asyncio.run(_collect(audio))
    assert first == second
    assert b"".join(first) == pcm


def test_empty_wave(tmp_path):
    audio = WavAudio(write_wav(tmp_path / "empty.wav", b""))
    assert audio.frame_count == 0
    assert audio.duration_s == 0
    assert asyncio.run(_collect(audio)) == []


@pytest.mark.parametrize("kwargs", [
    {"channels": 2}, {"width": 1}, {"width": 4},
])
def test_rejects_unsupported_wave_formats(tmp_path, kwargs):
    path = write_wav(tmp_path / "bad.wav", b"\x00" * 8, **kwargs)
    with pytest.raises(ValueError):
        WavAudio(path)


def test_rejects_non_16_bit_container_rounded_up_by_wave(tmp_path):
    path = write_wav(tmp_path / "12bit.wav", b"\x00\x00")
    raw = bytearray(path.read_bytes())
    struct.pack_into("<H", raw, 34, 12)
    path.write_bytes(raw)
    with pytest.raises(ValueError, match="16-bit"):
        WavAudio(path)


def test_rejects_incomplete_sample(tmp_path):
    path = write_wav(tmp_path / "odd.wav", b"\x01\x02\x03")
    with pytest.raises(ValueError, match="frame-aligned"):
        WavAudio(path)


def test_preserves_little_endian_on_big_endian_host(monkeypatch, tmp_path):
    pcm = b"\x01\x02\x03\x04"
    audio = WavAudio(write_wav(tmp_path / "endian.wav", pcm))
    monkeypatch.setattr("sys.byteorder", "big")
    assert b"".join(asyncio.run(_collect(audio))) == pcm


def test_rejects_truncated_data_after_construction(tmp_path):
    path = write_wav(tmp_path / "short.wav", b"\x01\x00" * 20)
    audio = WavAudio(path, chunk_ms=1)
    raw = path.read_bytes()
    path.write_bytes(raw[:-4])
    with pytest.raises(ValueError, match="truncated"):
        asyncio.run(_collect(audio))


@pytest.mark.parametrize("chunk_ms", [0, -1, True, 1.5])
def test_rejects_invalid_chunk_size(tmp_path, chunk_ms):
    path = write_wav(tmp_path / "valid.wav", b"\x00\x00")
    with pytest.raises(ValueError):
        WavAudio(path, chunk_ms=chunk_ms)


def test_rejects_invalid_pcm_format_values():
    for args in ((0,), (-1,), (8000, 0), (8000, 1, 0), (True,)):
        with pytest.raises(ValueError):
            PcmFormat(*args)


def test_realtime_chunks_paced_at_start_offsets(monkeypatch, tmp_path):
    pcm = struct.pack("<5h", 1, 2, 3, 4, 5)
    audio = WavAudio(write_wav(tmp_path / "paced.wav", pcm, rate=1000),
                     chunk_ms=2, realtime=True)
    clock = [50.0]
    sleeps = []
    yielded_at = []
    monkeypatch.setattr("rambler.audio.time.monotonic", lambda: clock[0])

    async def fake_sleep(delay):
        sleeps.append(delay)
        clock[0] += delay

    monkeypatch.setattr("rambler.audio.asyncio.sleep", fake_sleep)

    async def consume():
        result = []
        async for chunk in audio.chunks():
            result.append(chunk)
            yielded_at.append(clock[0] - 50.0)
        return result

    chunks = asyncio.run(consume())
    assert b"".join(chunks) == pcm
    assert yielded_at == pytest.approx([0.0, 0.002, 0.004])
    assert sleeps == pytest.approx([0.002, 0.002])


def test_rejects_invalid_realtime_type(tmp_path):
    path = write_wav(tmp_path / "valid.wav", b"\x00\x00")
    with pytest.raises(ValueError):
        WavAudio(path, realtime=1)
