"""Assemble owned prototype sources with an explicitly pinned installed Java bridge.

Build only: no device attachment, process execution, microphone or service calls.
Generated third-party bridge content must never be committed.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import site

BRIDGE_SHA256 = "02cb922b7fea29a566bd9ae4a190fad24c503076f2497af5706d117033de6a0e"
OWN_SOURCES = ("pcm-core.js", "frida-agent.js", "ui-dispatch.js")


def assemble(bridge: Path, output: Path) -> dict:
    metadata_path = output.with_suffix(output.suffix + ".metadata.json")
    if output.exists() or metadata_path.exists():
        raise ValueError("Output or metadata already exists; refusing overwrite")
    bridge_bytes = bridge.read_bytes()
    if hashlib.sha256(bridge_bytes).hexdigest() != BRIDGE_SHA256:
        raise ValueError("Installed Java bridge does not match the measured SDK pin")
    bridge_text = bridge_bytes.decode("utf-8")
    source_dir = Path(__file__).resolve().parent
    sources = [(name, (source_dir / name).read_bytes()) for name in OWN_SOURCES]
    bridge_wrapper = (
        "(function(){" + bridge_text
        + '\nObject.defineProperty(globalThis,"Java",{value:bridge});return bridge;})();'
    )
    code = (
        'Script.evaluate("/frida/bridges/java.js", '
        + json.dumps(bridge_wrapper, ensure_ascii=True) + ");\n"
        + "\n".join(data.decode("utf-8") for _, data in sources) + "\n"
    ).encode("utf-8")
    metadata = {
        "purpose": "experimental development reference, not a native runtime dependency",
        "bridge_sha256": BRIDGE_SHA256,
        "source_sha256": {name: hashlib.sha256(data).hexdigest() for name, data in sources},
        "assembled_sha256": hashlib.sha256(code).hexdigest(),
        "assembled_bytes": len(code),
        "device_actions": 0,
        "audio_arms": 0,
        "warning": "original constructor hooks reproduce an unarmed ART crash on the measured active IME",
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    # Exclusive creation also prevents a competing builder overwriting either file.
    with metadata_path.open("x", encoding="utf-8") as handle:
        json.dump(metadata, handle, indent=2)
        handle.write("\n")
    with output.open("xb") as handle:
        handle.write(code)
    return metadata


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bridge", type=Path, default=Path(site.getusersitepackages()) / "frida_tools/bridges/java.js")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    metadata = assemble(args.bridge, args.output)
    print(json.dumps(metadata, indent=2))


if __name__ == "__main__":
    main()
