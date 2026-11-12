"""Machine-readable output must not depend on Windows' stdout code page."""
import json
import subprocess
import sys


def test_ndjson_preserves_unicode_even_with_ascii_stdout():
    result = subprocess.run(
        [sys.executable, "-I", "-c", (
            "import sys; from rambler.cli import _emit; "
            "sys.stdout.reconfigure(encoding='ascii'); "
            "_emit({'text': 'dict\\u00e9e \\U0001f642'})"
        )],
        capture_output=True,
        timeout=30,
        check=False,
    )
    assert result.returncode == 0, result.stderr.decode("utf-8", errors="replace")
    assert json.loads(result.stdout.decode("utf-8")) == {"text": "dictée 🙂"}
