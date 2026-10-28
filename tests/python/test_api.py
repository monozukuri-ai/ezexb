from pathlib import Path

import pytest

import ezexb

CFB_MAGIC = bytes.fromhex("d0cf11e0a1b11ae1")
LEGACY_MAGIC = b"\x10CaxaEbForWindows"


@pytest.mark.parametrize("magic", [CFB_MAGIC, LEGACY_MAGIC])
def test_truncation_never_confirms_exb(magic):
    for length in range(len(magic)):
        info = ezexb.probe_bytes(magic[:length])
        assert info.signature == "unknown"
        assert not info.exb_confirmed


def test_file_and_bytes_agree_without_claiming_exb(tmp_path):
    path = tmp_path / "図面 with spaces.exb"
    data = CFB_MAGIC + b"not actually a valid compound file"
    path.write_bytes(data)
    assert ezexb.probe(path) == ezexb.probe_bytes(data)
    assert ezexb.probe(str(path)).signature == "cfb"
    assert not ezexb.probe(path).exb_confirmed


def test_file_limit_is_checked_before_reading_large_input(tmp_path):
    path = tmp_path / "large.exb"
    with path.open("wb") as stream:
        stream.seek(1024 * 1024)
        stream.write(b"x")
    with pytest.raises(ezexb.InputLimitError, match="max_file_bytes=8"):
        ezexb.probe(path, options=ezexb.ParseOptions(8))
    with pytest.raises(ezexb.ExbError):
        ezexb.probe_bytes(CFB_MAGIC, options=ezexb.ParseOptions(7))
    assert ezexb.probe_bytes(CFB_MAGIC, options=ezexb.ParseOptions(8)).byte_length == 8


def test_zero_budget_and_option_validation():
    assert ezexb.probe_bytes(b"", options=ezexb.ParseOptions(0)).byte_length == 0
    for value in (-1, 1 << 64):
        with pytest.raises(ValueError):
            ezexb.ParseOptions(value)
    for value in (True, 1.0, "8"):
        with pytest.raises(TypeError):
            ezexb.ParseOptions(value)


def test_path_and_contents_are_not_interchangeable(tmp_path):
    with pytest.raises(TypeError):
        ezexb.probe(b"drawing.exb")
    with pytest.raises(TypeError):
        ezexb.probe_bytes(Path("drawing.exb"))
    with pytest.raises(FileNotFoundError):
        ezexb.probe(tmp_path / "absent.exb")
    with pytest.raises(ezexb.InvalidInputError):
        ezexb.probe(tmp_path)


@pytest.mark.parametrize("strict", [True, False])
def test_readers_validate_actual_input(strict):
    with pytest.raises(FileNotFoundError):
        ezexb.read("not-opened.exb", strict=strict)
    with pytest.raises(ezexb.InvalidContainerError):
        ezexb.read_bytes(CFB_MAGIC, strict=strict)


def test_capabilities_are_fresh_and_honest():
    capabilities = ezexb.capabilities()
    assert capabilities["entity_decoding"]
    assert capabilities["container_inspection"]
    assert capabilities["stream_reading"]
    assert capabilities["supported_profiles"] == ["exb-stored-planar-v1"]
    assert capabilities["supported_product_builds"][0] == "10.0.0.0000"
    assert capabilities["qualified_product_majors"] == [10, 18]
    capabilities["supported_profiles"].append("not real")
    assert ezexb.capabilities()["supported_profiles"] == ["exb-stored-planar-v1"]
