import json
import subprocess
import sys
import xml.etree.ElementTree as ET
from dataclasses import asdict
from pathlib import Path

import pytest

import ezexb
from ezexb import raw
from ezexb.document import Line

DATA = Path(__file__).parent / "data"


def test_world_and_svg_public_entrypoints_preserve_domains():
    path = DATA / "document-18.cfb"
    scene = ezexb.read_world(path, -7)
    assert scene.source_document_complete
    # This stored-data fixture intentionally has an inconsistent INSERT matrix.
    assert not scene.transforms_complete
    assert any(d.code == "unsupported_world_transform" for d in scene.diagnostics)
    assert not scene.render_complete
    # TEXT retains its transform without claiming glyph geometry.
    assert not scene.geometry_complete
    assert isinstance(scene.instances[0].geometry, Line)
    assert scene.instances[0].geometry.start == (1.0, 2.0, 0.0)
    with raw.open_bytes(path.read_bytes()) as container:
        assert container.world(-7) == scene
        preview = container.svg(-7)
    assert preview == ezexb.svg_preview(path, -7)
    assert not preview.native_display_equivalent
    assert preview.rendered_curve_count >= 3
    assert preview.diagnostics
    assert ET.fromstring(preview.svg).tag == "{http://www.w3.org/2000/svg}svg"
    doc = ezexb.read(path)
    assert doc.text_style_context.complete
    assert doc.text_styles[0].western_font.value == "sans-serif"
    assert doc.text_styles[0].line_spacing_factor == 1.5


def test_world_cli_matches_binding_and_svg_never_overwrites(tmp_path):
    path = DATA / "document-10.cfb"
    command = [sys.executable, "-m", "ezexb"]
    result = subprocess.run(
        [*command, "world", str(path), "--space=-7", "--json"],
        capture_output=True,
        text=True,
        check=True,
    )
    assert json.loads(result.stdout) == json.loads(json.dumps(asdict(ezexb.read_world(path, -7))))
    destination = tmp_path / "drawing.svg"
    command = [*command, "svg", str(path), "--space=-7", "--output", str(destination), "--json"]
    result = subprocess.run(command, capture_output=True, text=True, check=True)
    report = json.loads(result.stdout)
    assert not report["native_display_equivalent"] and "svg" not in report
    original = destination.read_bytes()
    assert ET.fromstring(original) is not None
    assert subprocess.run(command, capture_output=True).returncode == 1
    assert destination.read_bytes() == original


def test_world_partial_and_input_validation():
    path = DATA / "document-partial.cfb"
    with pytest.raises(ezexb.IncompleteReadError) as exc:
        ezexb.read_world(path, -7)
    assert not exc.value.report.complete
    assert not ezexb.read_world(path, -7, strict=False).source_document_complete
    with raw.open(path) as c:
        with pytest.raises(TypeError):
            c.world(True)
        with pytest.raises(ValueError):
            c.world(1 << 31)
        with pytest.raises(TypeError):
            c.svg(-7, strict=1)
        with pytest.raises(ezexb.InvalidInputError):
            c.world(1234, strict=False)
    with pytest.raises(ezexb.InputLimitError):
        ezexb.read_world(path, -7, strict=False, options=ezexb.ParseOptions(max_records=1))
