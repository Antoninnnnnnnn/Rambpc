"""Offline native codec checks; the core test environment need not install PyAV."""

import asyncio
import builtins
import hashlib
import importlib
import io
import math
import struct
from types import SimpleNamespace

import pytest

from rambler import encoding
from rambler.encoding import pcm16_to_ogg


# Do not skip the dependency-free validation/laziness tests in a base install.
@pytest.fixture
def av():
    return pytest.importorskip("av")


def pcm(samples):
    return b"".join(
        struct.pack("<h", round(12000 * math.cos(2 * math.pi * 437 * i / 16000)))
        for i in range(samples)
    )


class Source:
    def __init__(self, chunks):
        self.chunks = iter(chunks)
        self.closed = False
        self.eof = False
        self.reads = 0
        self.digest = hashlib.sha256()

    def __aiter__(self):
        return self

    async def __anext__(self):
        try:
            chunk = next(self.chunks)
        except StopIteration:
            self.eof = True
            raise StopAsyncIteration
        self.reads += 1
        if isinstance(chunk, bytes):
            self.digest.update(chunk)
        return chunk

    async def aclose(self):
        self.closed = True


async def collect(source, **kwargs):
    return b"".join([chunk async for chunk in pcm16_to_ogg(source, **kwargs)])


def fragments(data, sizes):
    offset = 0
    index = 0
    while offset < len(data):
        size = sizes[index % len(sizes)]
        yield data[offset:offset + size]
        offset += size
        index += 1


def pages(data, *, incomplete=False):
    result = []
    offset = 0
    while offset < len(data):
        if len(data) - offset < 27:
            assert incomplete
            break
        assert data[offset:offset + 4] == b"OggS"
        assert data[offset + 4] == 0
        segments = data[offset + 26]
        header_end = offset + 27 + segments
        if header_end > len(data):
            assert incomplete
            break
        lacing = data[offset + 27:header_end]
        end = header_end + sum(lacing)
        if end > len(data):
            assert incomplete
            break
        result.append({
            "flags": data[offset + 5],
            "granule": struct.unpack_from("<Q", data, offset + 6)[0],
            "sequence": struct.unpack_from("<I", data, offset + 18)[0],
            "lacing": lacing,
            "body": data[header_end:end],
        })
        offset = end
    return result


def packets(data):
    packet = bytearray()
    result = []
    for page in pages(data):
        offset = 0
        for size in page["lacing"]:
            packet.extend(page["body"][offset:offset + size])
            offset += size
            if size < 255:
                result.append(bytes(packet))
                packet.clear()
    assert not packet
    return result


def decode(av, data, *, short=False):
    # swresample's default filter drops sub-filter-length clips even on flush.
    # Disable that filter ONLY for the separate sub-5 ms edge-case checks.
    resampler = av.AudioResampler(
        format="s16", layout="mono", rate=16000,
        options={"filter_size": "0"} if short else None,
    )
    result = bytearray()
    native_samples = 0
    with av.open(io.BytesIO(data), mode="r") as container:
        assert len(container.streams.audio) == 1
        assert container.streams.audio[0].codec_context.name == "opus"
        for frame in container.decode(audio=0):
            assert frame.sample_rate == 48000
            assert frame.layout.name == "mono"
            native_samples += frame.samples
            for converted in resampler.resample(frame):
                result.extend(bytes(converted.planes[0])[:converted.samples * 2])
        for converted in resampler.resample(None):
            result.extend(bytes(converted.planes[0])[:converted.samples * 2])
    return bytes(result), native_samples


@pytest.mark.parametrize("samples", [80, 319, 320, 321, 639, 640, 641, 16007, 32123])
@pytest.mark.parametrize("page_duration_ms", [20, 1000])
async def test_real_decoder_exact_duration_energy_and_ogg_headers(av, samples, page_duration_ms):
    original = pcm(samples)
    source = Source(fragments(original, [1, 17, 641, 3, 1280]))
    encoded = await collect(source, page_duration_ms=page_duration_ms)
    decoded, native_samples = decode(av, encoded)
    assert len(decoded) == samples * 2
    assert native_samples == samples * 3
    assert sum(value * value for (value,) in struct.iter_unpack("<h", decoded)) > 0
    assert source.closed and source.eof
    assert source.digest.digest() == hashlib.sha256(original).digest()
    stream_pages = pages(encoded)
    assert stream_pages[0]["flags"] & 2  # BOS
    assert [page["sequence"] for page in stream_pages] == list(range(len(stream_pages)))
    assert not any(page["flags"] & 4 for page in stream_pages[:-1])
    assert stream_pages[-1]["flags"] & 4  # EOS, only once
    head, tags, *audio = packets(encoded)
    assert head[:8] == b"OpusHead"
    assert head[8] == 1  # OpusHead version
    assert head[9] == 1  # mono
    assert struct.unpack_from("<I", head, 12)[0] == 16000
    assert head[18] == 0  # channel mapping family
    assert tags.startswith(b"OpusTags") and audio
    pre_skip = struct.unpack_from("<H", head, 10)[0]
    assert stream_pages[-1]["granule"] - pre_skip == samples * 3


@pytest.mark.parametrize("samples", [1, 2, 15, 16, 17, 31, 32])
async def test_sub_filter_length_audio_has_exact_ogg_duration(av, samples):
    encoded = await collect(Source([pcm(samples)]))
    decoded, native_samples = decode(av, encoded, short=True)
    assert len(decoded) == samples * 2
    assert native_samples == samples * 3


async def test_fragmentation_and_page_duration_preserve_encoded_samples(av):
    original = pcm(32771)
    expected_packets = None
    expected_decoded = None
    for page_duration_ms in (20, 37, 100, 1000):
        for sizes in ([len(original)], [640], [1, 3, 17, 1279, 2, 4097]):
            source = Source(fragments(original, sizes))
            encoded = await collect(source, page_duration_ms=page_duration_ms)
            decoded, _ = decode(av, encoded)
            audio_packets = packets(encoded)[2:]
            if expected_packets is None:
                expected_packets = audio_packets
                expected_decoded = decoded
            assert audio_packets == expected_packets
            # Lossy Opus is deliberately NOT compared with the original PCM hash.
            assert decoded == expected_decoded
            assert source.digest.digest() == hashlib.sha256(original).digest()
            assert source.closed and source.eof


async def test_page_duration_controls_real_mux_cadence(av):
    original = pcm(48017)
    fast = await collect(Source([original]), page_duration_ms=20)
    slow = await collect(Source([original]), page_duration_ms=1000)
    assert len(pages(fast)[2:]) >= 100
    assert 3 <= len(pages(slow)[2:]) <= 5
    assert packets(fast)[2:] == packets(slow)[2:]
    assert decode(av, fast) == decode(av, slow)


async def test_first_audio_page_is_flushed_before_source_eof(av):
    release_eof = asyncio.Event()

    class GatedSource(Source):
        async def __anext__(self):
            if self.reads == 3:
                await release_eof.wait()
            return await super().__anext__()

    source = GatedSource([pcm(320)] * 3)
    encoded = pcm16_to_ogg(source)
    prefix = bytearray()
    try:
        while len(pages(prefix, incomplete=True)) < 3:
            prefix.extend(await asyncio.wait_for(anext(encoded), timeout=2))
        assert not source.eof
        assert source.reads <= 3
        assert not any(page["flags"] & 4 for page in pages(prefix))
        release_eof.set()
        async for chunk in encoded:
            prefix.extend(chunk)
            for page in pages(prefix, incomplete=True):
                if page["flags"] & 4:
                    assert source.eof
    finally:
        await encoded.aclose()
    assert source.closed and source.eof
    assert len(decode(av, prefix)[0]) == 960 * 2


@pytest.mark.parametrize("chunks", [[], [b""], [b"", b"", b""]])
async def test_empty_source_produces_no_headers_or_native_import(monkeypatch, chunks):
    def forbidden(*args):
        pytest.fail("empty PCM must not load a native encoder")

    monkeypatch.setattr(encoding, "_OpusEncoder", forbidden)
    source = Source(chunks)
    assert await collect(source) == b""
    assert source.closed and source.eof


async def test_empty_fragments_do_not_change_audio(av):
    original = pcm(641)
    expected = await collect(Source([original]))
    source = Source([b"", original[:1], b"", original[1:100], b"", original[100:], b""])
    actual = await collect(source)
    assert packets(expected)[2:] == packets(actual)[2:]
    assert decode(av, expected) == decode(av, actual)


@pytest.mark.parametrize("kwargs", [
    {"sample_rate": 48000}, {"sample_rate": True}, {"sample_rate": 16000.0},
    {"channels": 2}, {"channels": True}, {"bitrate": 32000}, {"bitrate": 24000.0},
    {"page_duration_ms": 0}, {"page_duration_ms": 19}, {"page_duration_ms": 1001},
    {"page_duration_ms": True}, {"page_duration_ms": 20.0},
])
async def test_rejects_unvalidated_profile_and_page_duration(kwargs):
    source = Source([b"\0\0"])
    with pytest.raises(ValueError):
        await collect(source, **kwargs)
    assert source.closed and not source.reads


@pytest.mark.parametrize("chunk", [None, "text", bytearray(b"\0\0"), memoryview(b"\0\0"), 123])
async def test_rejects_nonbytes_chunks(chunk):
    source = Source([chunk])
    with pytest.raises(TypeError, match="bytes"):
        await collect(source)
    assert source.closed


@pytest.mark.parametrize("chunks", [[b"\x01"], [b"\0\0", b"\x01"]])
async def test_isolated_final_byte_is_explicit_truncation(chunks):
    source = Source(chunks)
    with pytest.raises(ValueError, match="truncated.*incomplete sample"):
        await collect(source)
    assert source.closed and source.eof


async def test_truncation_after_streaming_does_not_emit_a_successful_eos(av):
    source = Source([pcm(1280), b"\xff"])
    prefix = bytearray()
    with pytest.raises(ValueError, match="truncated"):
        async for chunk in pcm16_to_ogg(source):
            prefix.extend(chunk)
    assert len(pages(prefix)) >= 3
    assert not any(page["flags"] & 4 for page in pages(prefix))
    assert source.closed


async def test_rejects_input_chunk_over_8_mib():
    source = Source([b"\0" * (8 * 1024 * 1024 + 1)])
    with pytest.raises(ValueError, match="8 MiB"):
        await collect(source)
    assert source.closed


async def test_8_mib_input_is_accepted_without_consuming_whole_chunk(av):
    source = Source([b"\0" * (8 * 1024 * 1024)])
    encoded = pcm16_to_ogg(source)
    try:
        header = await asyncio.wait_for(anext(encoded), timeout=2)
        assert b"OpusHead" in header
        assert source.reads == 1 and not source.eof
    finally:
        await encoded.aclose()
    assert source.closed


@pytest.fixture
def tracked_containers(av, monkeypatch):
    opened = []
    native_open = av.open

    class TrackedContainer:
        def __init__(self, native):
            self.native = native
            self.closes = 0
            self.sink = None
            self.open_kwargs = None

        def __getattr__(self, name):
            return getattr(self.native, name)

        def start_encoding(self):
            context = self.native.streams.audio[0].codec_context
            self.codec_options = dict(context.options)
            self.native.start_encoding()
            self.profile = {
                "name": context.name,
                "sample_rate": context.sample_rate,
                "layout": context.layout.name,
                "format": context.format.name,
                "bit_rate": context.bit_rate,
                "frame_size": context.frame_size,
            }

        def close(self):
            self.closes += 1
            return self.native.close()

    def tracked_open(*args, **kwargs):
        native = native_open(*args, **kwargs)
        if kwargs.get("mode") != "w":
            return native
        tracked = TrackedContainer(native)
        tracked.sink = args[0]
        tracked.open_kwargs = kwargs
        opened.append(tracked)
        return tracked

    monkeypatch.setattr(av, "open", tracked_open)
    return opened


def assert_closed(containers):
    assert len(containers) == 1
    assert containers[0].closes == 1
    assert containers[0].sink._pending_bytes == 0
    assert not containers[0].sink._chunks


async def test_native_resources_close_normally_and_options_are_explicit(av, tracked_containers):
    source = Source([pcm(1281)])
    await collect(source, page_duration_ms=37)
    assert source.closed
    assert_closed(tracked_containers)
    options = tracked_containers[0].open_kwargs
    assert options["format"] == "ogg"
    assert options["options"] == {"page_duration": "37000", "flush_packets": "1"}
    assert options["buffer_size"] == 1024
    assert tracked_containers[0].profile == {
        "name": "libopus", "sample_rate": 16000, "layout": "mono",
        "format": "s16", "bit_rate": 24000, "frame_size": 320,
    }
    assert tracked_containers[0].codec_options == {
        "application": "voip", "frame_duration": "20", "vbr": "on",
    }


async def test_source_failure_closes_native_resources_without_emitting_eos(av, tracked_containers):
    failure = RuntimeError("source broke")

    class FailingSource(Source):
        async def __anext__(self):
            if self.reads:
                raise failure
            return await super().__anext__()

    source = FailingSource([pcm(1280)])
    prefix = bytearray()
    with pytest.raises(RuntimeError) as caught:
        async for chunk in pcm16_to_ogg(source):
            prefix.extend(chunk)
    assert caught.value is failure
    assert source.closed
    assert not any(page["flags"] & 4 for page in pages(prefix))
    assert_closed(tracked_containers)


async def test_early_close_closes_upstream_and_native_resources(av, tracked_containers):
    source = Source([pcm(320)] * 10)
    encoded = pcm16_to_ogg(source)
    prefix = bytearray()
    while len(pages(prefix, incomplete=True)) < 3:
        prefix.extend(await anext(encoded))
    await encoded.aclose()
    assert source.closed and not source.eof
    assert source.reads < 10
    assert not any(page["flags"] & 4 for page in pages(prefix))
    assert_closed(tracked_containers)


async def test_cancellation_while_awaiting_source_closes_everything(av, tracked_containers):
    blocked = asyncio.Event()

    class BlockingSource(Source):
        async def __anext__(self):
            if self.reads:
                blocked.set()
                await asyncio.Event().wait()
            return await super().__anext__()

    source = BlockingSource([pcm(320)])
    task = asyncio.create_task(collect(source))
    await asyncio.wait_for(blocked.wait(), timeout=2)
    task.cancel()
    with pytest.raises(asyncio.CancelledError):
        await task
    assert source.closed
    assert_closed(tracked_containers)


async def test_cancellation_cooperates_inside_one_large_chunk(av, tracked_containers):
    source = Source([b"\0" * (8 * 1024 * 1024)])
    first_output = asyncio.Event()

    async def consume():
        async for chunk in pcm16_to_ogg(source):
            first_output.set()

    task = asyncio.create_task(consume())
    await asyncio.wait_for(first_output.wait(), timeout=2)
    task.cancel()
    with pytest.raises(asyncio.CancelledError):
        await asyncio.wait_for(task, timeout=2)
    assert source.closed and not source.eof
    assert_closed(tracked_containers)


@pytest.mark.parametrize("operation", ["encode", "mux", "finish", "close"])
async def test_encoding_failures_still_close_source_and_native_resources(
    av, tracked_containers, monkeypatch, operation,
):
    native_encoder = encoding._OpusEncoder
    failure = RuntimeError(f"native {operation} failed")

    def injected_encoder(page_duration_ms):
        encoder = native_encoder(page_duration_ms)

        def fail(*args, **kwargs):
            raise failure

        if operation in ("encode", "finish"):
            setattr(encoder, operation, fail)
        elif operation == "mux":
            encoder._container.mux = fail
        else:
            native_close = encoder._container.close

            def close_then_fail():
                native_close()
                raise failure

            encoder._container.close = close_then_fail
        return encoder

    monkeypatch.setattr(encoding, "_OpusEncoder", injected_encoder)
    source = Source([pcm(1280)])
    prefix = bytearray()
    with pytest.raises(RuntimeError) as caught:
        async for chunk in pcm16_to_ogg(source):
            prefix.extend(chunk)
    assert caught.value is failure
    assert source.closed
    assert not any(page["flags"] & 4 for page in pages(prefix))
    assert_closed(tracked_containers)


async def test_codec_setup_failure_closes_container_and_upstream(av, monkeypatch):
    containers = []

    class BrokenContainer:
        def __init__(self):
            self.closed = False

        def add_stream(self, *args, **kwargs):
            raise RuntimeError("libopus setup failed")

        def close(self):
            self.closed = True

    def broken_open(*args, **kwargs):
        container = BrokenContainer()
        containers.append(container)
        return container

    monkeypatch.setattr(av, "open", broken_open)
    source = Source([pcm(320)])
    with pytest.raises(RuntimeError, match="libopus setup failed"):
        await collect(source)
    assert source.closed
    assert len(containers) == 1 and containers[0].closed


async def test_open_failure_still_closes_upstream(av, monkeypatch):
    def broken_open(*args, **kwargs):
        raise RuntimeError("AVIO setup failed")

    monkeypatch.setattr(av, "open", broken_open)
    source = Source([pcm(320)])
    with pytest.raises(RuntimeError, match="AVIO setup failed"):
        await collect(source)
    assert source.closed


def test_module_import_is_lazy(monkeypatch):
    native_import = builtins.__import__

    def reject_av(name, *args, **kwargs):
        if name == "av" or name.startswith("av."):
            pytest.fail("importing the encoding module must not import av")
        return native_import(name, *args, **kwargs)

    monkeypatch.setattr(builtins, "__import__", reject_av)
    importlib.reload(encoding)


async def test_missing_optional_dependency_has_actionable_error_and_closes_source(monkeypatch):
    native_import = builtins.__import__

    def missing_av(name, *args, **kwargs):
        if name == "av":
            raise ModuleNotFoundError("No module named 'av'", name="av")
        return native_import(name, *args, **kwargs)

    monkeypatch.setattr(builtins, "__import__", missing_av)
    source = Source([pcm(320)])
    with pytest.raises(ImportError, match=r"audio.*Python >= 3\.12"):
        await collect(source)
    assert source.closed


@pytest.mark.parametrize("byteorder", ["little", "big"])
@pytest.mark.parametrize("padding", [0, 10])
@pytest.mark.parametrize("pcm_type", [memoryview, bytearray])
def test_pcm16le_frame_plane_uses_host_endianness_without_mutating_input(
    monkeypatch, byteorder, padding, pcm_type,
):
    # Fake PyAV plane/frame: this exercises the BE branch, not real BE hardware.
    original = b"\x01\x02\x03\x04\xfe\xff"
    frame_pcm = pcm_type(original)
    calls = []
    plane = SimpleNamespace(buffer_size=len(original) + padding)
    plane.update = calls.append
    frame = SimpleNamespace(planes=[plane], samples=len(original) // 2)

    def make_frame(**kwargs):
        assert kwargs == {"format": "s16", "layout": "mono", "samples": 3}
        return frame

    encoded_frames = []

    def encode_frame(actual):
        encoded_frames.append(actual)
        return ()

    encoder = encoding._OpusEncoder.__new__(encoding._OpusEncoder)
    encoder._av = SimpleNamespace(AudioFrame=make_frame)
    encoder._stream = SimpleNamespace(encode=encode_frame)
    encoder._pts = 0
    encoder.sink = encoding._WriteSink()
    monkeypatch.setattr(encoding.sys, "byteorder", byteorder)
    assert list(encoder.encode(frame_pcm)) == []

    expected = original if byteorder == "little" else b"\x02\x01\x04\x03\xff\xfe"
    assert len(calls) == 1
    assert bytes(calls[0]) == expected + b"\0" * padding
    if byteorder == "little" and not padding:
        assert calls[0] is frame_pcm  # no extra copy or conversion on this path
    assert bytes(frame_pcm) == original
    assert encoded_frames == [frame]
    assert frame.samples == 3  # allocation padding adds no audio samples
    assert frame.sample_rate == 16000
    assert frame.pts == 0 and encoder._pts == 3


def test_write_sink_is_nonseekable_bounded_and_lossless():
    sink = encoding._WriteSink()
    assert sink.writable() and not sink.seekable()
    assert not hasattr(sink, "seek") and not hasattr(sink, "read")
    assert sink.write(b"first") == 5
    assert sink.write(b"second") == 6
    assert sink.write(b"") == 0
    assert list(sink.drain()) == [b"first", b"second"]
    assert sink._pending_bytes == 0
    assert sink.write(b"x" * encoding._MAX_PENDING_BYTES) == encoding._MAX_PENDING_BYTES
    with pytest.raises(BufferError, match="bounded"):
        sink.write(b"y")
    assert b"".join(sink.drain()) == b"x" * encoding._MAX_PENDING_BYTES
    sink.clear()
    assert not sink._chunks and sink._pending_bytes == 0
