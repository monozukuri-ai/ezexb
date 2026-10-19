"""Also runnable against an installed wheel from outside the source directory."""

import importlib.metadata
import importlib.resources
import json
import subprocess
import sys

import ezexb


def test_distribution_and_extension_versions_match():
    assert importlib.metadata.version("ezexb") == ezexb.__version__
    assert importlib.resources.files("ezexb").joinpath("py.typed").is_file()
    assert importlib.resources.files("ezexb").joinpath("_core.pyi").is_file()


def test_both_cli_entry_points():
    for command in ([sys.executable, "-m", "ezexb"], ["ezexb"]):
        result = subprocess.run(
            [*command, "capabilities", "--json"], check=True, capture_output=True, text=True
        )
        assert json.loads(result.stdout) == ezexb.capabilities()


def test_cli_errors_and_header_scope(tmp_path):
    path = tmp_path / "sample.exb"
    path.write_bytes(bytes.fromhex("d0cf11e0a1b11ae1"))
    command = [sys.executable, "-m", "ezexb", "probe", str(path), "--json"]
    result = subprocess.run(command, check=True, capture_output=True, text=True)
    assert json.loads(result.stdout) == {
        "signature": "cfb",
        "byte_length": 8,
        "exb_confirmed": False,
    }
    result = subprocess.run([*command, "--max-file-bytes", "0"], capture_output=True, text=True)
    assert result.returncode == 1
    assert not result.stdout
    assert "max_file_bytes=0" in result.stderr
    assert "Traceback" not in result.stderr
