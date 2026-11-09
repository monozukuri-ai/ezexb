"""The legacy ``CaxaEbForWindows`` container, on synthetic streams."""

from __future__ import annotations

import json
import struct
import subprocess
import sys
from dataclasses import asdict

import pytest

import ezexb

BY_LAYER = 0xFB
TRAILER = bytes.fromhex("00000000ffff00000000")
CENTRE_LAYER = "中心线层".encode("gbk")


def _string(value: bytes) -> bytes:
    return bytes([len(value)]) + value


def _prelude(version: int) -> bytes:
    out = ezexb.LEGACY_SIGNATURE + struct.pack("<3I", version, 1, 0) + _string(b"properties")
    out += struct.pack("<5I", 3, 0, 50, 2, 256) + bytes(4)
    layers = [(b"0", b"", 1, 47, 0), (b"1", CENTRE_LAYER, 1, 8, 3)]
    out += struct.pack("<I", len(layers))
    for key, description, visible, color, line_style in layers:
        out += _string(key) + _string(description) + bytes([visible, color, line_style])
        out += bytes([0, 1, 1]) if version >= 1070 else bytes([1])
    # One stored line style behind the 24 built-in ones, the custom colors and flags.
    out += bytes([2]) + struct.pack("<I", 25)
    out += _string(b"Imported") + struct.pack("<IHdB", 5, 0x5FFF, 0.35, 1)
    out += bytes([0]) + struct.pack("<16I", *([0x00FFFFFF] * 16)) + bytes([1]) * 64
    if version >= 1060:
        out += struct.pack("<3I", 1, 10000, 10000)
        out += _string(b"Standard") + _string(b"Arial") + _string("宋体".encode("gbk"))
        out += struct.pack("<7d", 3.5, 0.0, 0.1, 0.4, 0.667, 0.667, 0.0)
    return out


def _doubled(kind: int, layer: int, color: int, line_style: int, tags: int = 0) -> bytes:
    return bytes([kind, kind, layer, color, line_style, 0, 0xFF, 0xFF]) + struct.pack("<I", tags)


def _line(layer: int, ends: tuple[float, float, float, float], line_style: int = BY_LAYER) -> bytes:
    return _doubled(0x02, layer, BY_LAYER, line_style) + struct.pack("<4d", *ends)


def _text(contents: bytes) -> bytes:
    """Centred in a 40 × 10 area whose top-left corner is (10, 20)."""
    out = _doubled(0x06, 0, 9, BY_LAYER, tags=1) + bytes([0x29, 0x29]) + struct.pack("<d", 40.0)
    out += struct.pack("<5d", 10.0, 20.0, 50.0, 10.0, 3.5) + _string(contents)
    return out + struct.pack("<IdIBB", 10000, 0.0, 0, 5, 0x20)


def _file(version: int, count: int, records: bytes) -> bytes:
    return _prelude(version) + struct.pack("<I", count) + records + TRAILER


def _dimension(member: bytes) -> bytes:
    """A dimension group in the plain header form, with undecoded fields behind it."""
    header = bytes([0x05, 0, BY_LAYER, BY_LAYER, 0, 0xFF, 0xFF]) + struct.pack("<I", 0)
    return header + struct.pack("<I", 1) + member + bytes([0xAA]) * 9


def test_read_bytes_returns_a_document_of_the_legacy_profile():
    records = _line(1, (1.0, 2.0, 4.0, 6.0)) + _text("图样".encode("gbk"))
    records += _line(0, (0.0, 0.0, 1.0, 0.0), line_style=24)
    document = ezexb.read_bytes(_file(1070, 3, records))

    assert document.profile == ezexb.LEGACY_PROFILE == "exb-legacy-planar-v1"
    assert document.report.complete and not document.report.diagnostics
    assert document.file_info.product_build.value == "CaxaEbForWindows 1070"
    assert [space.name.value for space in document.spaces] == ["Model"]
    assert [entity.kind for entity in document.entities] == ["Line", "Text", "Line"]
    assert [entity.type_id for entity in document.entities] == [
        "legacy:line",
        "legacy:text",
        "legacy:line",
    ]
    assert document.blocks[0].entity_indices == (0, 1, 2)

    assert [layer.name.value for layer in document.layers] == ["0", "中心线层"]
    assert [layer.color_raw for layer in document.layers] == [0x00FFFFFF, 0x000000FF]
    assert [(linetype.id, linetype.name.value) for linetype in document.linetypes] == [
        (0, "LEGACY_THICK"),
        (1, "LEGACY_THIN"),
        (2, "LEGACY_DASHED"),
        (3, "LEGACY_CENTER"),
        (4, "LEGACY_PHANTOM"),
        (24, "Imported"),
    ]
    assert document.linetypes[-1].dashes == (13.0, 1.0, 1.0, 1.0)

    line, text, styled = document.entities
    assert line.geometry.start == (1.0, 2.0, 0.0) and line.geometry.end == (4.0, 6.0, 0.0)
    assert line.style.layer_name == "中心线层"
    assert (line.style.color_raw.source, line.style.color_raw.value) == ("by_layer", 0xFF)
    assert (line.style.linetype.source, line.style.linetype.value) == ("by_layer", 3)
    assert line.style.line_width.source == "by_layer_thin"
    assert (styled.style.linetype.value, styled.style.line_width.value) == (24, 0.35)

    assert text.geometry.contents == "图样"
    assert text.geometry.insertion == (30.0, 15.0, 0.0)
    assert (text.geometry.alignment_raw, text.geometry.fit_mode) == (4, 1)
    assert (text.geometry.area_width_stored, text.geometry.area_height_stored) == (40.0, 10.0)
    assert (text.style.color_raw.source, text.style.color_raw.value) == ("explicit", 0x0000FFFF)
    (style,) = document.text_styles
    assert (
        (style.id, style.name.value)
        == (10000, "Standard")
        == (text.geometry.style_id_raw, "Standard")
    )
    assert (style.western_font.value, style.cjk_font.value) == ("Arial", "宋体")
    assert style.cjk_width_factor == 0.667


def test_read_recognises_the_signature_of_a_file(tmp_path):
    data = _file(1050, 1, _line(1, (0.0, 0.0, 5.0, 0.0)))
    path = tmp_path / "旧图纸.exb"
    path.write_bytes(data)

    assert ezexb.probe(path).signature == "legacy_caxa_candidate"
    document = ezexb.read(path)
    assert document == ezexb.read(str(path)) == ezexb.read_bytes(data)
    assert document.file_info.stream_version.value == "1050"
    assert document.entities[0].style.layer_name == "中心线层"
    # The legacy stream has no directory: inspection only names the signature.
    inspection = ezexb.inspect(path)
    assert inspection.classification == "legacy_caxa_candidate" and not inspection.entries
    with pytest.raises(ezexb.UnsupportedFeatureError, match="non-CFB"):
        ezexb.metadata(path)


def test_strict_reads_raise_with_the_report_and_the_cli_reads_legacy_files(tmp_path):
    records = _line(0, (5.0, 5.0, 6.0, 6.0)) + _dimension(_line(0, (0.0, 0.0, 10.0, 0.0)))
    records += _line(0, (1.0, 1.0, 2.0, 2.0))
    data = _file(1070, 3, records)
    path = tmp_path / "old.exb"
    path.write_bytes(data)

    document = ezexb.read(path, strict=False)
    assert len(document.entities) == 3 and not document.report.complete
    (diagnostic,) = document.report.diagnostics
    assert (diagnostic.code, diagnostic.type_id) == (
        "legacy_record_tail_skipped",
        "legacy:dimension",
    )
    for strict_read in (lambda: ezexb.read(path), lambda: ezexb.read_bytes(data)):
        with pytest.raises(ezexb.IncompleteReadError) as error:
            strict_read()
        assert error.value.report == document.report

    command = [sys.executable, "-m", "ezexb", "read", str(path), "--json"]
    strict = subprocess.run(command, capture_output=True, text=True)
    assert strict.returncode == 2
    assert json.loads(strict.stdout)["report"]["diagnostics"][0]["code"] == diagnostic.code
    partial = subprocess.run([*command, "--partial"], check=True, capture_output=True, text=True)
    assert json.loads(partial.stdout) == json.loads(json.dumps(asdict(document)))


def test_versions_limits_and_arguments():
    records = _line(0, (0.0, 0.0, 1.0, 0.0)) + _line(0, (1.0, 0.0, 2.0, 0.0))
    data = _file(1070, 2, records)

    with pytest.raises(ezexb.UnsupportedFeatureError, match="legacy container version"):
        ezexb.read_bytes(_file(2000, 2, records))
    unverified = ezexb.read_bytes(_file(1080, 2, records), strict=False)
    assert [d.code for d in unverified.report.diagnostics] == ["unverified_legacy_version"]
    with pytest.raises(ezexb.InputLimitError, match="max_records"):
        ezexb.read_bytes(data, options=ezexb.ParseOptions(max_records=1))
    with pytest.raises(ezexb.InputLimitError, match="max_file_bytes"):
        ezexb.read_bytes(data, options=ezexb.ParseOptions(max_file_bytes=16))
    with pytest.raises(ezexb.InvalidStreamError):
        ezexb.read_bytes(data[:24])
    with pytest.raises(TypeError, match="strict must be a bool"):
        ezexb.read_bytes(data, strict=1)


def test_capabilities_list_the_legacy_profile():
    capabilities = ezexb.capabilities()
    assert capabilities["legacy_profile"] == ezexb.LEGACY_PROFILE
    assert capabilities["legacy_container_versions"] == [1050, 1060, 1070]
    assert capabilities["supported_profiles"] == ["exb-stored-planar-v1"]
    assert ezexb.LEGACY_SIGNATURE == b"\x10CaxaEbForWindows"
