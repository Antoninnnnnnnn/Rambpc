"""Diagnostic CLI for S3 with Ogg/Opus or optional PCM WAVE input."""
from __future__ import annotations

import argparse
import asyncio
from contextlib import aclosing
from dataclasses import asdict
import json
import math
import sys

from .credentials import EnvironmentCredentials
from .engine import RamblerEngine
from .protocol import EditingContext, ProtocolConfig, context_request


def _emit(value: dict) -> None:
    # Protobuf floats can contain NaN/Infinity, which are not JSON numbers.
    def safe(item):
        if isinstance(item, float) and not math.isfinite(item):
            return None
        if isinstance(item, dict):
            return {key: safe(child) for key, child in item.items()}
        if isinstance(item, (list, tuple)):
            return [safe(child) for child in item]
        return item
    # ASCII JSON escapes preserve every Unicode character across redirected Windows
    # code pages; JSON consumers recover the original text without mojibake.
    print(json.dumps(safe(value), ensure_ascii=True, allow_nan=False), flush=True)


async def _empty():
    if False:
        yield b""


async def _file_audio(path: str, chunk_bytes: int):
    if isinstance(chunk_bytes, bool) or not isinstance(chunk_bytes, int) or chunk_bytes <= 0:
        raise ValueError("chunk size must be a positive integer")
    with open(path, "rb") as stream:
        # Simple container/codec signature check, not a complete Ogg validator.
        header = stream.read(65536)
        if not header.startswith(b"OggS") or b"OpusHead" not in header:
            raise ValueError("input must be an Ogg/Opus file; convert externally with ffmpeg")
        for offset in range(0, len(header), chunk_bytes):
            yield header[offset:offset + chunk_bytes]
        while True:
            chunk = stream.read(chunk_bytes)
            if not chunk:
                break
            yield chunk


def _response(response):
    hypotheses = []
    for hypothesis in response.hypotheses:
        edit = hypothesis.voice_edit
        hypotheses.append({"text": hypothesis.text, "confidence": hypothesis.confidence,
                           "voice_edit": None if edit is None else {
                               "transcript": edit.transcript,
                               "alternative_text": edit.alternative_text,
                               "edited_text": edit.edited_text,
                               "edited_context": None if edit.edited_context is None else {
                                   "before": edit.edited_context.before,
                                   "selected": edit.edited_context.selected,
                                   "after": edit.edited_context.after},
                               "intent": edit.intent}})
    return {"status": response.status, "error_code": response.error_code,
            "recognition_state": response.recognition_state,
            "speech_event": response.speech_event,
            "hypotheses": hypotheses,
            "partials": [{"text": p.text, "confidence": p.confidence} for p in response.partials]}


class _AudioExtraMissing(Exception):
    pass


async def _run(args):
    if args.command != "dictate-wav" and args.chunk_bytes <= 0:
        raise ValueError("chunk size must be positive")
    if args.sample_rate != 16000:
        raise ValueError("only a declared sample rate of 16000 Hz is supported")
    config = ProtocolConfig(language=args.language, sample_rate=args.sample_rate)
    context = None
    if args.command in ("dictate", "dictate-wav"):
        context = EditingContext(before=args.before or "", selected=args.selected or "",
                                 after=args.after or "", cursor_aware=args.cursor_aware)
        context_request(context)  # Validate before opening a channel or reading audio.
    if args.command == "dictate-wav":
        if not 20 <= args.page_duration_ms <= 1000:
            raise ValueError("page duration must be in 20..1000 ms")
        from .audio import WavAudio
        audio = WavAudio(args.file, chunk_ms=20, realtime=args.realtime)
        if audio.format.sample_rate != 16000:
            raise ValueError("WAVE sample rate must be 16000 Hz")
        try:
            import av  # noqa: F401 - fail before opening an RPC
        except ModuleNotFoundError as exc:
            if exc.name != "av":
                raise
            raise _AudioExtraMissing from None
        from .encoding import pcm16_to_ogg
        source = pcm16_to_ogg(audio.chunks(), sample_rate=16000, channels=1,
                              bitrate=24000, page_duration_ms=args.page_duration_ms)
    else:
        source = _empty() if args.command == "probe" else _file_audio(args.file, args.chunk_bytes)
    usable = False
    failed = False
    s3_end = False
    engine = RamblerEngine(credentials=EnvironmentCredentials())
    try:
        await engine.open(timeout=args.timeout)
        async with aclosing(engine.stream(source, config=config, context=context, timeout=args.timeout)) as events:
            async for event in events:
                if event.response is None:
                    _emit({"type": "eof", "metrics": asdict(event.metrics), "diagnostic": args.command == "probe"})
                    continue
                response = event.response
                failed |= response.status == 2 or response.error_code not in (None, 0)
                s3_end |= response.status == 1
                usable |= any(h.text or (h.voice_edit and (h.voice_edit.transcript or h.voice_edit.alternative_text or h.voice_edit.edited_text or h.voice_edit.edited_context)) for h in response.hypotheses)
                _emit({"type": "response", "response": _response(response), "metrics": asdict(event.metrics)})
    finally:
        await engine.close()
    if failed:
        _emit({"type": "error", "message": "S3 reported an error; see numeric status and error code"})
        return 1
    if not s3_end:
        _emit({"type": "error", "error_type": "IncompleteSession", "message": "S3 end status not observed"})
        return 1
    if args.command in ("dictate", "dictate-wav") and not usable:
        _emit({"type": "error", "message": "no usable hypothesis received"})
        return 1
    return 0


def main(argv=None):
    parser = argparse.ArgumentParser(description="Rambler S3 diagnostic")
    sub = parser.add_subparsers(dest="command", required=True)
    probe = sub.add_parser("probe", help="diagnostic init/end without audio")
    dictate = sub.add_parser("dictate", help="stream an existing Ogg/Opus file")
    wav = sub.add_parser("dictate-wav", help="encode and stream a PCM16 mono WAVE file")
    dictate.add_argument("file")
    wav.add_argument("file")
    for command in (probe, dictate, wav):
        command.add_argument("--language", default="fr-FR")
        command.add_argument("--timeout", type=float, default=30)
        command.add_argument("--sample-rate", type=int, default=16000,
                             help="declared input rate; only 16000 supported")
    probe.add_argument("--chunk-bytes", type=int, default=224)
    dictate.add_argument("--chunk-bytes", type=int, default=8192)
    wav.add_argument("--realtime", action="store_true", help="pace file audio in real time")
    wav.add_argument("--page-duration-ms", type=int, default=20)
    for command in (dictate, wav):
        command.add_argument("--before")
        command.add_argument("--selected")
        command.add_argument("--after")
        command.add_argument("--cursor-aware", action="store_true")
    args = parser.parse_args(argv)
    try:
        return asyncio.run(_run(args))
    except _AudioExtraMissing:
        _emit({"type": "error", "message": "install audio extra: pip install 'rambpc-engine[audio]' (Python >= 3.12)",
               "error_type": "AudioExtraMissing"})
        return 1
    except (asyncio.CancelledError, KeyboardInterrupt):
        _emit({"type": "error", "message": "session cancelled", "error_type": "Cancelled"})
        return 130
    except Exception as exc:
        # Never serialize RPC details, paths, input, metadata, or arbitrary exceptions.
        _emit({"type": "error", "message": "session failed", "error_type": type(exc).__name__})
        return 1


if __name__ == "__main__":
    sys.exit(main())
