import json
import math
import subprocess
import sys
from dataclasses import asdict
from pathlib import Path

import pytest

import ezexb
from ezexb import raw
from ezexb.bom import BomTable
from ezexb.document import DegenerateArc
from ezexb.tables import Table, TableBlockContent, TableTextContent
from ezexb.views import View2D

DATA = Path(__file__).parent / "data"


@pytest.mark.parametrize("major", [10, 18])
def test_remaining_entities_keep_storage_values_and_partial_report(major):
    path = DATA / f"remaining-{major}.cfb"
    doc = ezexb.read(path, strict=False)
    assert doc == ezexb.read_bytes(path.read_bytes(), strict=False)
    with raw.open(path) as container:
        assert container.document(strict=False) == doc
    entities = {e.id: e for e in doc.entities}
    assert len(entities) == 9
    assert doc.report.unsupported_entity_count == 0
    assert not doc.report.complete
    assert entities[140].geometry == DegenerateArc(
        (2.0, -3.5, 0.0), 0.0, (0.0, 0.0, 0.0), (0.0, 0.0, 0.0), math.tau
    )
    codes = [d.code for d in doc.report.diagnostics]
    assert codes.count("degenerate_geometry") == 1
    if major == 10:
        view = entities[150].geometry
        assert isinstance(view, View2D)
        assert view.name.value == "Detail view A"
        assert view.block_reference.block_id == -60
        assert view.block_reference.scales == (2.0, 3.0, 1.0)
        assert view.block_reference.nickname.value == "detail-ref"
        assert view.clip.reference_bytes == tuple(range(32))
        assert view.clip.reference_span.length == 32
        assert view.clip.type_raw == 2 and view.clip.flag_raw == 65535
        assert len(view.clip.curves) == 1
        bom = entities[160].geometry
        assert isinstance(bom, BomTable)
        assert bom.table_reference.id_raw == -61
        assert bom.break_direction_raw == 2 and bom.relation_type_raw == 4
        assert bom.scale == 2.5 and bom.location == (12.0, 34.0, 0.0)
        assert [p.index_raw for p in bom.rewind_points] == [-3, 5]
        assert [i.item_kind for i in bom.items] == ["Header", "Row"]
        for item in bom.items:
            line, text = [r.geometry.geometry for r in item.presentation_records]
            assert line.start == (2.0, 3.0, 0.0)
            assert text.contents == "Part Ω"
            assert item.header.id_raw not in entities
        assert codes.count("unsupported_view_semantics") == 1
        assert codes.count("unsupported_bom_table_semantics") == 1
    else:
        table, hole = entities[170].geometry, entities[171].geometry
        assert isinstance(table, Table) and isinstance(hole, Table)
        assert table.table_type == "Table" and hole.table_type == "HoleChart"
        for t in (table, hole):
            assert [c.width for c in t.columns] == [14.0, 23.0]
            assert t.location == (11.0, 12.0, 0.0)
            assert t.rotation == 0.25 and t.scale == 2.0
            assert len(t.rows) == 1 and t.rows[0].index == 3
            row = t.rows[0]
            assert row.height == 7.5
            assert (row.index_prefix, row.index_suffix) == ("P-", "-S")
            cell = row.cells[0]
            assert cell.style_name == "Custom Ω"
            assert cell.contents[0].value == TableTextContent("Cell 日本語")
            assert [(p.type_raw, p.value.kind, p.value.value) for p in cell.overrides] == [
                (121, "F64", 0.5),
                (130, "U32", 5),
                (131, "U16", 65535),
            ]
            block = row.cells[1].contents[1].value
            assert isinstance(block, TableBlockContent)
            assert block.reference.id_raw == -83 and block.scale == 1.25
            assert t.unresolved_semantics_span == t.data_span
            assert len(t.presentation_records) == 1
        assert table.hole_chart is None and hole.break_data is None
        assert table.break_data.values[0].index == 2
        assert table.break_data.values[0].value == 6.5
        assert hole.hole_chart.coordinate_name == "Origin Ω"
        assert hole.hole_chart.coordinate_matrix_raw[12:15] == (20.0, 30.0, 0.0)
        assert [r.id_raw for r in hole.hole_chart.references] == [-101, -102, -103, -104, -105]
        assert codes.count("unsupported_table_semantics") == 2
    with pytest.raises(ezexb.IncompleteReadError) as error:
        ezexb.read(path)
    assert error.value.report == doc.report


@pytest.mark.parametrize("major", [10, 18])
def test_remaining_cli_world_and_svg_preserve_limits(major):
    path = DATA / f"remaining-{major}.cfb"
    doc = ezexb.read(path, strict=False)
    command = [sys.executable, "-m", "ezexb", "read", str(path), "--json"]
    result = subprocess.run([*command, "--partial"], check=True, capture_output=True, text=True)
    assert json.loads(result.stdout) == json.loads(json.dumps(asdict(doc)))
    result = subprocess.run(command, capture_output=True, text=True)
    assert result.returncode == 2
    assert json.loads(result.stdout)["report"] == json.loads(json.dumps(asdict(doc.report)))
    scene = ezexb.read_world(path, -7, strict=False)
    assert not scene.source_document_complete and not scene.geometry_complete
    assert next(i for i in scene.instances if i.entity_id == 140).geometry is None
    if major == 10:
        assert next(i for i in scene.instances if i.entity_id == 150).geometry is None
        assert all(150 not in i.insert_path for i in scene.instances)
        assert any(
            d.code == "unsupported_world_clip" and d.record_id == 150 for d in scene.diagnostics
        )
        assert not scene.transforms_complete
    preview = ezexb.svg_preview(path, -7, strict=False)
    baseline = ezexb.svg_preview(DATA / f"document-{major}.cfb", -7)
    assert not preview.source_document_complete and not preview.native_display_equivalent
    # Only each saved presentation is drawn, not semantic cells, view references,
    # inferred degenerate points, or table placement applied a second time.
    assert preview.rendered_curve_count == baseline.rendered_curve_count + 2
    assert preview.rendered_text_count == baseline.rendered_text_count + (2 if major == 10 else 0)
    assert preview.svg.count("Part Ω") == (2 if major == 10 else 0)
    assert "Cell 日本語" not in preview.svg


@pytest.mark.parametrize("major", [10, 18])
@pytest.mark.parametrize("strict", [False, True])
def test_remaining_cumulative_limits_cannot_be_bypassed(major, strict):
    path = DATA / f"remaining-{major}.cfb"
    report = ezexb.read(path, strict=False).report
    with pytest.raises(ezexb.InputLimitError):
        ezexb.read(
            path, strict=strict, options=ezexb.ParseOptions(max_records=report.budget_used - 1)
        )
    assert (
        ezexb.read(
            path, strict=False, options=ezexb.ParseOptions(max_records=report.budget_used)
        ).report
        == report
    )
