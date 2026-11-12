"""One bounded, anonymous S3 RPC diagnostic; does not test recognition.

Run from the repository: python tools/probe_empty_request.py --output local/probe.json
No credentials, cookies, context or audio are sent. TLS verification stays enabled.
"""
from __future__ import annotations

import argparse
import asyncio
import datetime as dt
import json
from pathlib import Path
import platform
import time

import grpc

TARGET = "speechs3proto2-pa.googleapis.com:443"
METHOD = "/speech.s3.S3StubbyClientConnectorService/S3Bidi"


async def probe(timeout: float) -> dict:
    started = time.perf_counter()
    result = {
        "observed_at": dt.datetime.now(dt.timezone.utc).isoformat(),
        "target": TARGET,
        "method": METHOD,
        "python": platform.python_version(),
        "grpcio": grpc.__version__,
        "input": "one empty protobuf message followed by client half-close",
        "credentials": "none",
        "audio_bytes": 0,
        "timeout_s": timeout,
        "responses": [],
        "meaning": "Diagnostic only; no recognition or Rambler success is established.",
    }

    async def requests():
        yield b""

    options = (
        ("grpc.enable_retries", 0),
        ("grpc.max_receive_message_length", 1024 * 1024),
    )
    async with grpc.aio.secure_channel(
        TARGET, grpc.ssl_channel_credentials(), options=options
    ) as channel:
        call = channel.stream_stream(METHOD)(requests(), timeout=timeout)
        try:
            async for payload in call:
                result["responses"].append({
                    "elapsed_ms": round((time.perf_counter() - started) * 1000, 3),
                    "bytes": len(payload),
                    "hex_prefix": payload[:4096].hex(),
                    "truncated": len(payload) > 4096,
                })
                if len(result["responses"]) >= 10:
                    call.cancel()
                    result["stopped_at_response_limit"] = True
                    break
        except grpc.aio.AioRpcError as error:
            result["status"] = error.code().name
            result["details"] = error.details()
        finally:
            result.setdefault("status", (await call.code()).name)
            result.setdefault("details", await call.details())
    result["elapsed_ms"] = round((time.perf_counter() - started) * 1000, 3)
    return result


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--timeout", type=float, default=10.0)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if not 0 < args.timeout <= 60:
        parser.error("timeout must be in (0, 60] seconds")
    result = asyncio.run(probe(args.timeout))
    rendered = json.dumps(result, indent=2, ensure_ascii=False) + "\n"
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        # Refuse accidental overwrite of a previous observation.
        with args.output.open("x", encoding="utf-8") as output:
            output.write(rendered)
    print(rendered, end="")


if __name__ == "__main__":
    main()
