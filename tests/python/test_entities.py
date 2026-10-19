import json
import subprocess
import sys
from dataclasses import asdict
from pathlib import Path

import pytest

import ezexb
from ezexb import raw
from ezexb.complex import Datum, Dimension, GeometricTolerance, Roughness, SectionMarker, Symbol
from ezexb.document import Point
from ezexb.partno import PartItem, PartNumber

DATA = Path(__file__).parent / "data"


@pytest.mark.parametrize("major", [10, 18])
def test_new_entities_keep_typed_fields_and_nested_presentations(major):
    path = DATA / f"entities-{major}.cfb"
    doc = ezexb.read(path, strict=False)
    assert doc == ezexb.read_bytes(path.read_bytes(), strict=False)
    with raw.open(path) as container:
        assert container.document(strict=False) == doc
    entities = {e.id: e for e in doc.entities}
    assert len(entities) == (18 if major == 10 else 13)
    assert doc.report.unsupported_entity_count == 0
    assert not doc.report.complete
    assert entities[100].geometry == Point((12.5, -8.25, 0.0))
    assert entities[100].attributes.layer_id_raw == -24
    assert 900 not in entities
    for i in range(110, 116):
        dimension = entities[i].geometry
        assert isinstance(dimension, Dimension)
        assert dimension.dimension_type_id == entities[i].type_id
        assert dimension.value_stored == 12.5
        assert dimension.text_fields_stored[1].value == "Original note"
        assert dimension.presentation_records[0].geometry.geometry == entities[100].geometry
        assert dimension.unresolved_semantics_span.length == 128
    codes = [d.code for d in doc.report.diagnostics]
    assert codes.count("unsupported_dimension_semantics") == 6
    if major == 10:
        symbols = [entities[i].geometry for i in range(120, 124)]
        assert all(isinstance(s, Symbol) for s in symbols)
        assert [type(s.data) for s in symbols] == [
            Roughness,
            GeometricTolerance,
            SectionMarker,
            Datum,
        ]
        assert symbols[0].data.lower_value.value == "1.6"
        assert symbols[1].data.top_note.value == "Top note"
        assert symbols[2].data.labels[0].location == (4.0, 5.0, 0.0)
        assert symbols[2].data.labels[0].text.value == "A"
        assert symbols[3].data.name.value == "B"
        for symbol in symbols:
            assert (
                symbol.data_span.offset + symbol.data_span.length
                == symbol.unresolved_semantics_span.offset
            )
            assert symbol.unresolved_semantics_span.length == 40
            assert symbol.presentation_records[0].geometry.geometry == entities[100].geometry
        part = entities[130].geometry
        assert isinstance(part, PartNumber)
        assert part.style_reference.id_raw == -61
        assert part.table_reference.id_raw == -60
        assert part.horizontal_or_vertical_raw == 1
        assert part.auto_or_reverse_raw == 2
        assert part.orientation_raw == 3
        assert part.array_mode_raw == 1
        assert part.start_reference.id_arrays == ((17, 18), (-3,))
        assert part.start_reference.points_raw[0] == (11.0, 12.0, 0.0)
        assert part.start_reference.parameters_raw == (0.25, 0.75)
        assert part.leader_points == ((1.0, 2.0, 0.0), (3.0, 4.0, 0.0))
        assert part.scale == 2.5
        cached = part.presentation_records[1].geometry
        assert cached.category == "PartItem" and isinstance(cached.geometry, PartItem)
        assert cached.geometry.index_raw == 101
        assert part.items[0].geometry.index_raw == 102
        assert part.items[0].geometry.presentation_records[0].geometry.geometry.start == (
            2.0,
            3.0,
            0.0,
        )
        assert codes.count("unsupported_symbol_semantics") == 4
        assert codes.count("unsupported_part_number_semantics") == 1
    with pytest.raises(ezexb.IncompleteReadError) as error:
        ezexb.read(path)
    assert error.value.report == doc.report


def test_entity_cli_world_and_svg_keep_partial_status():
    path = DATA / "entities-10.cfb"
    command = [sys.executable, "-m", "ezexb", "read", str(path), "--json"]
    partial = subprocess.run([*command, "--partial"], check=True, capture_output=True, text=True)
    doc = ezexb.read(path, strict=False)
    assert json.loads(partial.stdout) == json.loads(json.dumps(asdict(doc)))
    strict = subprocess.run(command, capture_output=True, text=True)
    assert strict.returncode == 2
    assert json.loads(strict.stdout)["report"] == json.loads(json.dumps(asdict(doc.report)))
    scene = ezexb.read_world(path, -7, strict=False)
    point = next(i for i in scene.instances if i.entity_id == 100)
    assert point.geometry == Point((12.5, -8.25, 0.0))
    assert not scene.source_document_complete
    preview = ezexb.svg_preview(path, -7, strict=False)
    assert not preview.source_document_complete and not preview.native_display_equivalent
    assert preview.svg.count('stroke-linecap="round"') == 11
    # One top-level point, six dimension points and four symbol points. PartNo
    # renders its cached LINEs; its semantic item copies must not render twice.
    baseline = ezexb.svg_preview(DATA / "document-10.cfb", -7)
    assert preview.rendered_curve_count == baseline.rendered_curve_count + 13


@pytest.mark.parametrize("strict", [False, True])
def test_entity_resource_limits_are_enforced_in_both_modes(strict):
    path = DATA / "entities-10.cfb"
    report = ezexb.read(path, strict=False).report
    for options in (
        ezexb.ParseOptions(max_records=report.budget_used - 1),
        ezexb.ParseOptions(max_record_depth=1),
    ):
        with pytest.raises(ezexb.InputLimitError):
            ezexb.read(path, strict=strict, options=options)
    assert (
        ezexb.read(
            path, strict=False, options=ezexb.ParseOptions(max_records=report.budget_used)
        ).report
        == report
    )
