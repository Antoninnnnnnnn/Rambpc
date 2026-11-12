"""Public imports must not load the optional native audio stack."""
import subprocess
import sys


def test_public_import_keeps_native_encoder_lazy():
    result = subprocess.run(
        [sys.executable, "-I", "-c", (
            "import sys; from rambler import RamblerEngine, ProtocolConfig, "
            "EditingContext, S3Status, IncompleteSessionError; "
            "assert 'av' not in sys.modules; assert S3Status(1).value == 1"
        )],
        capture_output=True,
        timeout=30,
        check=False,
    )
    assert result.returncode == 0, result.stderr.decode("utf-8", errors="replace")
