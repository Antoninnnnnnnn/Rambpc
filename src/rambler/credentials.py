"""Explicit, replaceable per-session metadata providers; values are never logged."""
from __future__ import annotations

import os
import re
from collections.abc import Sequence
from typing import Protocol

Metadata = Sequence[tuple[str, str | bytes]]
_KEY = re.compile(r"[0-9a-z_.-]+\Z", re.ASCII)


class MetadataProvider(Protocol):
    async def metadata(self) -> Metadata: ...


def validate_metadata(metadata: Metadata) -> tuple[tuple[str, str | bytes], ...]:
    """Reject invalid headers before gRPC core can log their sensitive values.

    Non-binary values must be printable ASCII; binary keys require bytes. Error
    messages never interpolate a key, value, or repr from the provider. A bounded
    snapshot prevents providers from leaving sessions coupled through mutable data.
    """
    if not isinstance(metadata, Sequence) or isinstance(metadata, (str, bytes, bytearray)):
        raise ValueError("invalid credential metadata")
    if len(metadata) > 100:
        raise ValueError("credential metadata exceeds limits")
    result = []
    size = 0
    for item in metadata:
        if not isinstance(item, (tuple, list)) or len(item) != 2:
            raise ValueError("invalid credential metadata")
        key, value = item
        if not isinstance(key, str) or len(key) > 65536 or _KEY.fullmatch(key) is None:
            raise ValueError("invalid credential metadata")
        if not isinstance(value, (str, bytes)):
            raise ValueError("invalid credential metadata")
        size += len(key) + len(value)
        if size > 65536:
            raise ValueError("credential metadata exceeds limits")
        if key.endswith("-bin"):
            if not isinstance(value, bytes):
                raise ValueError("invalid credential metadata")
        elif not isinstance(value, str) or any(not 32 <= ord(char) <= 126 for char in value):
            raise ValueError("invalid credential metadata")
        result.append((key, value))
    return tuple(result)


class EnvironmentCredentials:
    """Read documented Android API headers explicitly, without retaining secrets.

    All three are required together; no header, identity, token, or default secret
    is fabricated. Read on every call so rotated environment values take effect.
    """

    async def metadata(self) -> Metadata:
        names = ("RAMBLER_API_KEY", "RAMBLER_ANDROID_PACKAGE", "RAMBLER_ANDROID_CERT")
        values = tuple(os.environ.get(name, "") for name in names)
        if any(values) and not all(values):
            raise ValueError("RAMBLER_API_KEY, RAMBLER_ANDROID_PACKAGE and RAMBLER_ANDROID_CERT must be provided together")
        if not any(values):
            return ()
        return validate_metadata(tuple(zip(("x-goog-api-key", "x-android-package", "x-android-cert"), values)))
