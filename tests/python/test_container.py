import hashlib
import json
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor
from dataclasses import asdict
from pathlib import Path

import pytest

import ezexb
from ezexb import raw

FIXTURE = Path(__file__).parent / "data/synthetic.cfb"
SMALL = b"small stream\x00\xff"


def test_inspection_and_stream_contents():
    info = ezexb.inspect(FIXTURE)
    assert info == ezexb.inspect_bytes(FIXTURE.read_bytes())
    assert info.classification == "cfb"
    assert not info.exb_confirmed
    assert info.cfb_version == 3
    assert info.sector_size == 512
    assert {s.path for s in info.storages} == {"/nested", "/empty"}
    assert {s.path: s.size_bytes for s in info.streams} == {
        "/nested/日本語": len(SMALL),
        "/large": 9000,
        "/zero": 0,
    }
    assert all("\\" not in entry.path for entry in info.entries)
    with raw.open(FIXTURE) as container:
        assert container.info == info
        for path, expected in (("/nested/日本語", SMALL), ("/large", b"Z" * 9000), ("/zero", b"")):
            actual = container.read_stream(path)
            assert type(actual) is bytes
            assert actual == expected
            assert container.stream_sha256(path) == hashlib.sha256(expected).hexdigest()


def test_ranges_and_snapshot_lifetime(tmp_path):
    path = tmp_path / "図面.cfb"
    path.write_bytes(FIXTURE.read_bytes())
    container = raw.open(path)
    path.write_bytes(b"changed after open")
    path.unlink()
    assert container.read_stream("/nested/日本語", offset=6, length=6) == b"stream"
    assert container.read_stream("/large", offset=4090, length=20) == b"Z" * 20
    assert container.read_stream("/large", offset=9000) == b""
    with pytest.raises(ezexb.InvalidInputError):
        container.read_stream("/large", offset=9001)
    with pytest.raises(ezexb.InvalidInputError):
        container.read_stream("/large", offset=9000, length=1)
    container.close()
    container.close()
    with pytest.raises(ezexb.InvalidInputError, match="closed"):
        container.read_stream("/zero")
    with pytest.raises(ezexb.InvalidInputError, match="closed"):
        _ = container.info


@pytest.mark.parametrize(
    "path",
    [
        "large",
        "/",
        "/../large",
        "/nested/../large",
        "/./large",
        "/large/",
        "//large",
        "/nested\\日本語",
        "/C:/large",
        "/a\0b",
    ],
)
def test_selector_validation(path):
    with raw.open_bytes(FIXTURE.read_bytes()) as container:
        with pytest.raises(ezexb.InvalidInputError):
            container.read_stream(path)
        with pytest.raises(ezexb.InvalidInputError):
            container.stream_sha256(path)


def test_missing_and_storage_errors():
    with raw.open(FIXTURE) as container:
        with pytest.raises(ezexb.StreamNotFoundError):
            container.read_stream("/missing")
        with pytest.raises(ezexb.InvalidInputError, match="storage"):
            container.read_stream("/nested")
        for value in (-1, 1 << 64):
            with pytest.raises(ValueError):
                container.read_stream("/large", offset=value)
        for value in (True, 1.5):
            with pytest.raises(TypeError):
                container.read_stream("/large", length=value)


@pytest.mark.parametrize(
    "name,value",
    [
        ("max_file_bytes", 512),
        ("max_entries", 4),
        ("max_stream_bytes", 8999),
        ("max_total_stream_bytes", 9013),
        ("max_storage_depth", 1),
        ("max_container_read_bytes", 511),
    ],
)
def test_limits_cover_file_and_memory_apis(name, value):
    options = ezexb.ParseOptions(**{name: value})
    for operation in (
        lambda: ezexb.inspect(FIXTURE, options=options),
        lambda: ezexb.inspect_bytes(FIXTURE.read_bytes(), options=options),
        lambda: raw.open_bytes(FIXTURE.read_bytes(), options=options),
    ):
        with pytest.raises(ezexb.InputLimitError, match=name):
            operation()


def test_type_and_format_errors(tmp_path):
    with pytest.raises(TypeError):
        ezexb.inspect(b"file.cfb")
    with pytest.raises(TypeError):
        raw.open(b"file.cfb")
    for function in (ezexb.inspect_bytes, raw.open_bytes):
        with pytest.raises(TypeError):
            function(bytearray(b"abc"))
    with pytest.raises(FileNotFoundError):
        ezexb.inspect(tmp_path / "absent")
    with pytest.raises(ezexb.InvalidInputError):
        raw.open(tmp_path)
    assert ezexb.inspect_bytes(b"plain text").classification == "unknown"
    assert ezexb.inspect_bytes(b"\x10CaxaEbForWindows").classification == "legacy_caxa_candidate"
    with pytest.raises(ezexb.UnsupportedFeatureError):
        raw.open_bytes(b"\x10CaxaEbForWindows")
    for data in (FIXTURE.read_bytes()[:8], FIXTURE.read_bytes()[:512]):
        with pytest.raises(ezexb.InvalidContainerError):
            ezexb.inspect_bytes(data)


def test_container_methods_are_serialized_across_threads():
    with raw.open(FIXTURE) as container:
        with ThreadPoolExecutor(max_workers=4) as pool:
            values = list(pool.map(lambda _: container.stream_sha256("/large"), range(12)))
        assert values == [hashlib.sha256(b"Z" * 9000).hexdigest()] * 12


def run_cli(*args):
    return subprocess.run(
        [sys.executable, "-m", "ezexb", *map(str, args)], capture_output=True, text=True
    )


def test_cli_inspection_listing_hash_and_hex():
    result = run_cli("inspect", FIXTURE, "--json")
    assert result.returncode == 0, result.stderr
    assert json.loads(result.stdout) == json.loads(json.dumps(asdict(ezexb.inspect(FIXTURE))))
    result = run_cli("streams", FIXTURE, "--json", "--sha256")
    assert result.returncode == 0, result.stderr
    entries = {entry["path"]: entry for entry in json.loads(result.stdout)}
    assert entries["/nested/日本語"]["sha256"] == hashlib.sha256(SMALL).hexdigest()
    result = run_cli("stream", FIXTURE, "/large", "--hex", "--json")
    assert result.returncode == 0, result.stderr
    assert json.loads(result.stdout)["size_bytes"] == 256
    result = run_cli("stream", FIXTURE, "/nested/日本語", "--hex", "--offset", 6, "--length", 6)
    assert result.returncode == 0, result.stderr
    assert "00000006" in result.stdout and "stream" in result.stdout


def test_cli_extraction_errors_and_no_overwrite(tmp_path):
    destination = tmp_path / "payload.bin"
    result = run_cli("stream", FIXTURE, "/nested/日本語", "--output", destination, "--json")
    assert result.returncode == 0, result.stderr
    assert destination.read_bytes() == SMALL
    destination.write_bytes(b"keep")
    result = run_cli("stream", FIXTURE, "/large", "--output", destination)
    assert result.returncode == 1
    assert destination.read_bytes() == b"keep"
    for args in [
        ("streams", FIXTURE, "--max-entries", 0),
        ("stream", FIXTURE, "/missing", "--hex"),
        ("stream", FIXTURE, "/large", "--hex", "--length", -1),
    ]:
        result = run_cli(*args)
        assert result.returncode == 1
        assert not result.stdout
        assert "Traceback" not in result.stderr
