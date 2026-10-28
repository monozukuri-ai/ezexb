import json
import subprocess
import sys
from dataclasses import asdict
from pathlib import Path

import pytest

import ezexb
from ezexb import raw
from ezexb.document import Arc, Circle, Insert, Line, Polyline, Text

DATA = Path(__file__).parent / "data"


@pytest.mark.parametrize("major", [10, 18])
def test_read_entrypoints_and_typed_stored_entities(major):
    path = DATA / f"document-{major}.cfb"
    doc = ezexb.read(path)
    assert doc == ezexb.read_bytes(path.read_bytes())
    with raw.open(path) as container:
        assert doc == container.document()
    assert doc.report.complete and not doc.report.diagnostics
    assert doc.units is None and doc.coordinate_system == "stored_block_coordinates"
    assert [type(e.geometry) for e in doc.entities] == [Line, Circle, Arc, Polyline, Text, Insert]
    assert doc.entities[0].geometry.start == (1.0, 2.0, 0.0)
    assert doc.entities[4].geometry.contents == "Original 日本語"
    assert doc.entities[5].geometry.block_id == -60
    assert len(doc.entities_in_space(-7)) == 5
    assert len(doc.entities_in_block(-60)) == 1
    assert doc.entities[0].style.layer_name == "Test layer"
    assert doc.layers[0].name.value == "Test layer"
    assert doc.entities[0].style.linetype.value == doc.linetypes[0].id
    with pytest.raises(KeyError):
        doc.entities_in_space(123)
    with pytest.raises(KeyError):
        doc.entities_in_block(123)


def test_strict_error_report_partial_diagnostics_and_cli_exit_status():
    path = DATA / "document-partial.cfb"
    doc = ezexb.read(path, strict=False)
    assert not doc.report.complete and len(doc.entities) == 6
    assert doc.report.unsupported_entity_count == 1
    assert doc.report.diagnostics[0].span is not None
    with pytest.raises(ezexb.IncompleteReadError) as exc:
        ezexb.read(path)
    assert exc.value.report == doc.report
    strict = subprocess.run(
        [sys.executable, "-m", "ezexb", "read", str(path), "--json"], capture_output=True, text=True
    )
    assert strict.returncode == 2
    assert json.loads(strict.stdout) == {
        "error": "incomplete_read",
        "report": json.loads(json.dumps(asdict(doc.report))),
    }
    partial = subprocess.run(
        [sys.executable, "-m", "ezexb", "read", str(path), "--json", "--partial"],
        check=True,
        capture_output=True,
        text=True,
    )
    assert json.loads(partial.stdout) == json.loads(json.dumps(asdict(doc)))


def test_cumulative_limits_and_argument_validation():
    path = DATA / "document-18.cfb"
    report = ezexb.read(path).report
    for strict in (True, False):
        for options in (
            ezexb.ParseOptions(max_records=report.budget_used - 1),
            ezexb.ParseOptions(max_total_decoded_bytes=report.decoded_bytes - 1),
        ):
            with pytest.raises(ezexb.InputLimitError):
                ezexb.read(path, strict=strict, options=options)
    with pytest.raises(TypeError):
        ezexb.read(path, strict=1)
    with pytest.raises(TypeError):
        ezexb.read_bytes(bytearray(path.read_bytes()))
    with pytest.raises(TypeError):
        ezexb.read(bytes(path))


def test_unverified_build_of_a_qualified_major_reads_with_a_diagnostic(tmp_path):
    # Field files carry builds such as 18.0.0.17670; same major, so the verified rules apply
    verified = ezexb.read(DATA / "document-18.cfb")
    needle = "18.0.0.18159".encode("utf-16-le")
    data = (DATA / "document-18.cfb").read_bytes()
    assert data.count(needle) == 1
    path = tmp_path / "field.cfb"
    path.write_bytes(data.replace(needle, "18.0.0.17670".encode("utf-16-le")))
    doc = ezexb.read(path, strict=False)
    assert [type(e.geometry) for e in doc.entities] == [type(e.geometry) for e in verified.entities]
    assert [d.code for d in doc.report.diagnostics] == ["unverified_product_build"]
    assert "18.0.0.18159" in doc.report.diagnostics[0].message
    assert not doc.report.complete
    with pytest.raises(ezexb.IncompleteReadError):
        ezexb.read(path)
    unsupported = tmp_path / "newer.cfb"
    unsupported.write_bytes(data.replace(needle, "19.0.0.18159".encode("utf-16-le")))
    with pytest.raises(ezexb.UnsupportedFeatureError):
        ezexb.read(unsupported, strict=False)
