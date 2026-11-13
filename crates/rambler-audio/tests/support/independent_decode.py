"""TEST/DEV ONLY: independently demux/decode complete Rust Ogg streams with PyAV.

Never imported or launched by the Rust library. Run Python isolated (-I).
"""
import io
import sys
import av

short = sys.argv[1] == "short"
resampler = av.AudioResampler(
    format="s16", layout="mono", rate=16000,
    options={"filter_size": "0"} if short else None,
)
native_samples = 0
pcm_samples = 0
with av.open(io.BytesIO(sys.stdin.buffer.read()), mode="r") as container:
    assert len(container.streams.audio) == 1
    assert container.streams.audio[0].codec_context.name == "opus"
    for frame in container.decode(audio=0):
        assert frame.sample_rate == 48000
        assert frame.layout.name == "mono"
        native_samples += frame.samples
        for converted in resampler.resample(frame):
            pcm_samples += converted.samples
    for converted in resampler.resample(None):
        pcm_samples += converted.samples
print(native_samples, pcm_samples)
