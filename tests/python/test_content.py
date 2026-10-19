import json
import struct
import subprocess
import sys
from dataclasses import asdict
from pathlib import Path

import pytest

import ezexb
from ezexb import raw

FIXTURE = Path(__file__).parent / "data/analysis.cfb"
GENERIC = Path(__file__).parent / "data/synthetic.cfb"
BLOCK = "/*BlockStg/*Blk_1"


def run_cli(*args):
    return subprocess.run(
        [sys.executable, "-m", "ezexb", *map(str, args)], capture_output=True, text=True
    )


def test_metadata_fields_unicode_and_original_source_spans():
    result = ezexb.metadata(FIXTURE)
    assert result == ezexb.metadata_bytes(FIXTURE.read_bytes())
    assert result.file_info.product_build.value == "Synthetic Build"
    assert result.file_info.properties[0].key.value == "Title"
    assert result.file_info.properties[0].value.value == "Test 日本語"
    assert [s.value for s in result.app_info.records[0].strings] == [
        "SyntheticApp",
        "1.0",
        "",
        "fixture",
    ]
    with raw.open(FIXTURE) as container:
        data = container.read_stream("/*FileInfoStream")
        for field in (result.file_info.product_build, result.file_info.properties[0].value):
            value = field.value.encode("utf-16le") + b"\0\0"
            span = field.span
            assert (
                data[span.offset : span.offset + span.length]
                == len(value).to_bytes(4, "little") + value
            )
        for unknown in result.file_info.unknown_fields:
            assert (
                data[unknown.span.offset : unknown.span.offset + unknown.span.length].hex()
                == unknown.hex
            )


def test_preview_is_exact_bmp_and_bounded():
    info = ezexb.preview(FIXTURE)
    assert info == ezexb.preview_bytes(FIXTURE.read_bytes())
    assert (info.width, info.height, info.bits_per_pixel, info.row_stride) == (2, 1, 24, 8)
    with raw.open(FIXTURE) as container:
        bmp = container.preview_bmp()
        assert bmp[:2] == b"BM"
        assert len(bmp) == int.from_bytes(bmp[2:6], "little") == 62
        assert bmp[-8:] == bytes([0, 0, 255, 0, 255, 0, 0, 0])
        assert container.read_stream("/*PreviewStream")[info.bmp_span.offset :] == bmp
    with pytest.raises(ezexb.InputLimitError, match="max_preview_pixels"):
        ezexb.preview(FIXTURE, options=ezexb.ParseOptions(max_preview_pixels=1))


def test_multiblock_payload_and_raw_access_stay_distinct():
    with raw.open(FIXTURE) as container:
        decoded = container.stream_payload("/multiblock")
        assert decoded.data == b"X" * 262144 + b"hello world"
        assert decoded.info.logical_size == 262155
        assert decoded.info.allocated_size == 524288
        assert [b.logical_size for b in decoded.info.blocks] == [262144, 11]
        original = container.read_stream("/multiblock")
        assert original[:8] == bytes.fromhex("00dee33d00000400")
        assert decoded.info.blocks[0].stream_offset == 20
        assert sum(b.compressed_size + 4 for b in decoded.info.blocks) + 16 == len(original)
        uncompressed = container.stream_payload("/*FileInfoStream")
        assert uncompressed.info.encoding == "raw"
        assert uncompressed.data == container.read_stream("/*FileInfoStream")


def test_record_headers_stop_before_unknown_schema_without_scanning():
    with raw.open(FIXTURE) as container:
        result = container.record_headers(BLOCK)
        assert not result.framing_complete and not result.geometry_decoded
        assert result.offset_space == "decoded_stream"
        assert len(result.records) == 1  # A later LINE marker must not be used to resume parsing.
        first = result.records[0]
        assert first.class_name == "CxDbLine"
        assert first.type_id == "17ecf72d-c046-4e9a-8416-fd8220fd33b5"
        assert (first.span.offset, first.span.length) == (172, 111)
        assert (first.payload_span.offset, first.payload_span.length) == (202, 81)
        assert result.unparsed_span.offset == 283
        assert "0x7777" in result.stop_reason
        with pytest.raises(ezexb.InvalidInputError):
            container.record_headers("/*FileInfoStream")


@pytest.mark.parametrize(
    "method,args,budget,value",
    [
        ("stream_payload", ("/multiblock",), "max_decoded_stream_bytes", 524287),
        ("file_info", (), "max_string_bytes", 12),
        ("app_info", (), "max_records", 0),
        ("record_headers", (BLOCK,), "max_records", 0),
        ("lines", (BLOCK,), "max_records", 0),
    ],
)
def test_analysis_limits_apply_through_binding(method, args, budget, value):
    with raw.open(FIXTURE, options=ezexb.ParseOptions(**{budget: value})) as container:
        with pytest.raises(ezexb.InputLimitError, match=budget):
            getattr(container, method)(*args)


def test_invalid_utf16_is_not_silently_replaced():
    data = FIXTURE.read_bytes()
    old = "Synthetic Build".encode("utf-16le")
    assert data.count(old) == 1
    changed = data.replace(old, b"\x00\xd8" + old[2:])
    with pytest.raises(ezexb.InvalidStreamError, match="UTF-16"):
        ezexb.metadata_bytes(changed)


def test_new_apis_reject_wrong_types_and_do_not_fabricate_missing_content():
    for function in (ezexb.metadata, ezexb.preview):
        with pytest.raises(TypeError):
            function(b"contents")
        with pytest.raises(ezexb.StreamNotFoundError):
            function(GENERIC)
    for function in (ezexb.metadata_bytes, ezexb.preview_bytes):
        with pytest.raises(TypeError):
            function(bytearray(FIXTURE.read_bytes()))
    container = raw.open(FIXTURE)
    container.close()
    for method, args in [
        ("file_info", ()),
        ("app_info", ()),
        ("preview_info", ()),
        ("preview_bmp", ()),
        ("stream_payload", (BLOCK,)),
        ("record_headers", (BLOCK,)),
        ("lines", (BLOCK,)),
    ]:
        with pytest.raises(ezexb.InvalidInputError, match="closed"):
            getattr(container, method)(*args)


def test_cli_analysis_and_extraction_preserve_existing_files(tmp_path):
    result = run_cli("metadata", FIXTURE, "--json")
    assert result.returncode == 0, result.stderr
    assert json.loads(result.stdout) == json.loads(json.dumps(asdict(ezexb.metadata(FIXTURE))))
    result = run_cli("records", FIXTURE, BLOCK, "--json")
    assert result.returncode == 0, result.stderr
    assert json.loads(result.stdout)["records"][0]["class_name"] == "CxDbLine"
    for command, extra, expected in [("preview", (), b"BM"), ("payload", ("/multiblock",), b"XX")]:
        destination = tmp_path / command
        result = run_cli(command, FIXTURE, *extra, "--output", destination, "--json")
        assert result.returncode == 0, result.stderr
        assert destination.read_bytes().startswith(expected)
        destination.write_bytes(b"keep")
        result = run_cli(command, FIXTURE, *extra, "--output", destination)
        assert result.returncode == 1
        assert destination.read_bytes() == b"keep"
        assert not result.stdout and "Traceback" not in result.stderr
    result = run_cli("payload", FIXTURE, "/multiblock", "--hex", "--json")
    assert result.returncode == 0, result.stderr
    assert json.loads(result.stdout)["prefix_hex"] == (b"X" * 256).hex()
    result = run_cli("preview", FIXTURE, "--max-preview-pixels", "0", "--json")
    assert result.returncode == 1 and not result.stdout and "max_preview_pixels" in result.stderr


def test_optional_xref_and_capabilities_remain_observations():
    info = ezexb.inspect(FIXTURE)
    assert info.classification == "cfb_exb_candidate"
    assert info.missing_markers == ("/*XrefInfoStream",)
    assert not info.exb_confirmed
    caps = ezexb.capabilities()
    for key in (
        "stream_decompression",
        "metadata_reading",
        "preview_extraction",
        "record_header_inspection",
    ):
        assert caps[key]
    assert caps["entity_decoding"]
    assert caps["supported_profiles"] == ["exb-stored-planar-v1"]


def test_experimental_line_coordinates_attributes_and_cli():
    with raw.open(FIXTURE) as container:
        result = container.lines(BLOCK)
        assert not result.drawing_complete and not result.framing.framing_complete
        assert len(result.lines) == 1 and not result.unsupported_lines
        line = result.lines[0]
        assert line.start == (1.25, -2.5, 0.0) and line.end == (4.25, 1.5, 0.0)
        assert line.coordinates_span.offset == 235
        assert line.attributes.layer_id_raw == -24
        assert line.attributes.line_width_raw == struct.unpack("<f", struct.pack("<f", 0.6))[0]
        assert line.attributes.linetype_scale == 1.25
        assert result.framing.records[line.record_index].id_raw == 7
        cli = run_cli("lines", FIXTURE, BLOCK, "--json")
        assert cli.returncode == 0, cli.stderr
        assert json.loads(cli.stdout) == json.loads(json.dumps(asdict(result)))
        with pytest.raises(ezexb.InvalidInputError):
            container.lines("/*FileInfoStream")


def test_property_groups_are_bounded_and_preserve_encoded_value_spans():
    stream = "/*BlockStg/*Blk_2"
    with raw.open(FIXTURE) as container:
        result = container.lines(stream)
        assert result.framing.framing_complete
        assert len(result.lines) == 1 and not result.drawing_complete
        record = result.framing.records[0]
        assert record.schema == 0x101 and record.object_schema == 1
        assert record.properties_present
        (group,) = record.property_groups
        string, blob = group.values
        assert string.tag == 7 and string.key.value == "Label"
        assert blob.tag == 12 and blob.key is None
        data = container.stream_payload(stream).data
        span = blob.value_span
        assert data[span.offset : span.offset + span.length] == bytes([10, 20, 30])
        span = string.value_span
        encoded = data[span.offset : span.offset + span.length]
        assert int.from_bytes(encoded[:4], "little") == len(encoded) - 4
        assert encoded[4:-2].decode("utf-16le") == "Original synthetic value"
    with raw.open(FIXTURE, options=ezexb.ParseOptions(max_records=3)) as container:
        with pytest.raises(ezexb.InputLimitError, match="max_records"):
            container.lines(stream)
