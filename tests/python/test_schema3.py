"""Original recursive fixtures exercise the public binding and actual CLI."""

import json
import subprocess
import sys
from dataclasses import asdict
from pathlib import Path

import pytest

import ezexb
from ezexb import raw
from ezexb.content import RecordDisplayCache, RecordExtension, RecordHeader, RecordRelation

DATA = Path(__file__).parent / "data"
BLOCK = "/*BlockStg/*Blk_1"


@pytest.fixture(params=[33, 37])
def source(request):
    return DATA / f"schema3-{request.param}.cfb", request.param


def test_recursive_records_keep_display_children_separate_and_spans_exact(source):
    path, width = source
    with raw.open(path) as container:
        result = container.record_headers(BLOCK)
        assert result.framing_complete and not result.geometry_decoded
        assert result.profile == f"schema3-attributes{width}-experimental"
        assert len(result.records) == 3
        parent, top_line, crx_line = result.records
        assert parent.object_schema == 3
        extension = parent.extension
        assert isinstance(extension, RecordExtension)
        assert isinstance(extension.relations[0], RecordRelation)
        assert isinstance(extension.display_cache, RecordDisplayCache)
        child, cached_line = extension.display_cache.records
        assert isinstance(child, RecordHeader)
        assert child.extension.display_cache.records[0].id_raw == 3
        assert cached_line.id_raw == 4
        assert extension.header_u16_raw == 0
        assert extension.header_u32_raw == (1, 0xF000FE40)
        assert extension.attributes_span.length == width
        assert extension.tail_raw == 0
        data = container.stream_payload(BLOCK).data
        for record in (parent, child):
            ext = record.extension
            assert ext.span.offset == record.payload_span.offset + record.payload_span.length
            assert ext.span.offset + ext.span.length == record.span.offset + record.span.length
            attrs = ext.attributes_span
            body = record.payload_span
            assert (
                data[attrs.offset : attrs.offset + width] == data[body.offset : body.offset + width]
            )
            assert data[ext.tail_span.offset : ext.tail_span.offset + ext.tail_span.length] == b"\0"
            for nested in ext.display_cache.records:
                assert nested.span.offset > ext.display_cache.presence_span.offset
                assert nested.span.offset + nested.span.length <= ext.tail_span.offset
        relation = extension.relations[0]
        assert relation.class_name == "CxDbSingleRelation"
        assert relation.span.length == 24
        assert (
            int.from_bytes(
                data[relation.span.offset + 20 : relation.span.offset + 24], "little", signed=True
            )
            == -24
        )
        assert top_line.id_raw == 5 and crx_line.id_raw == 6
        assert top_line.extension is None and crx_line.extension is None
        lines = container.lines(BLOCK)
        assert lines.framing == result
        assert [item.record_index for item in lines.lines] == [1]
        assert [item.record_index for item in lines.unsupported_lines] == [2]
        assert lines.other_record_count == 1 and not lines.drawing_complete
        assert lines.lines[0].start == (1.0, 2.0, 3.0)
        assert lines.lines[0].end == (4.0, 5.0, 6.0)
    cli = subprocess.run(
        [sys.executable, "-m", "ezexb", "records", str(path), BLOCK, "--json"],
        capture_output=True,
        text=True,
    )
    assert cli.returncode == 0, cli.stderr
    assert json.loads(cli.stdout) == json.loads(json.dumps(asdict(result)))


def test_unknown_tail_and_nested_schema_stop_at_enclosing_record(source):
    path, _ = source
    with raw.open(path) as container:
        tail = container.record_headers("/*BlockStg/*Blk_2")
        assert not tail.framing_complete and tail.stop_reason
        assert len(tail.records) == 1
        assert tail.unparsed_span.offset == 283
        nested = container.record_headers("/*BlockStg/*Blk_4")
        assert not nested.framing_complete and nested.stop_reason
        assert not nested.records and nested.unparsed_span.offset == 172
        absent = container.record_headers("/*BlockStg/*Blk_5")
        assert absent.framing_complete
        cache = absent.records[0].extension.display_cache
        assert not cache.present and cache.type_id is None and not cache.records
        with pytest.raises(ezexb.InvalidStreamError):
            container.record_headers("/*BlockStg/*Blk_3")


def test_profile_selection_does_not_guess_for_unobserved_build(source):
    path, width = source
    data = path.read_bytes()
    old = ("10.0.0.0000" if width == 33 else "18.0.0.18159").encode("utf-16le")
    assert data.count(old) == 1
    changed = data.replace(old, "99".encode("utf-16le") + old[4:])
    with raw.open_bytes(changed) as container:
        result = container.record_headers(BLOCK)
        assert result.profile is None
        assert not result.framing_complete and not result.records
        assert result.unparsed_span.offset == 172 and result.stop_reason


def test_malformed_metadata_keeps_ordinary_prefix_available(source):
    path, width = source
    data = path.read_bytes()
    old = ("10.0.0.0000" if width == 33 else "18.0.0.18159").encode("utf-16le")
    changed = data.replace(old, b"\x00\xd8" + old[2:])
    with raw.open_bytes(changed) as container:
        with pytest.raises(ezexb.InvalidStreamError, match="UTF-16"):
            container.file_info()
        result = container.record_headers("/*BlockStg/*Blk_2")
        assert result.profile is None and not result.framing_complete
        assert len(result.records) == 1 and result.records[0].class_name == "CxDbLine"


def test_record_depth_limit_reaches_rust_and_cli(source):
    path, _ = source
    with raw.open(path, options=ezexb.ParseOptions(max_record_depth=1)) as container:
        with pytest.raises(ezexb.InputLimitError, match="max_record_depth"):
            container.record_headers(BLOCK)
    cli = subprocess.run(
        [
            sys.executable,
            "-m",
            "ezexb",
            "records",
            str(path),
            BLOCK,
            "--max-record-depth",
            "1",
            "--json",
        ],
        capture_output=True,
        text=True,
    )
    assert cli.returncode == 1 and not cli.stdout
    assert "max_record_depth" in cli.stderr and "Traceback" not in cli.stderr
