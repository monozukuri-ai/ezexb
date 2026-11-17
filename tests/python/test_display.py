import json
import math
import subprocess
import sys
from pathlib import Path

import pytest

import ezexb
from ezexb import raw
from ezexb.complex import CachedDisplay, Dimension, Nurbs
from ezexb.document import Arc, Circle, Insert, Line, Polyline, Text, Xline

DATA = Path(__file__).parent / "data"
UNKNOWN_CLASS = "5a17c0de-0000-4000-8000-00000000c1a5"
UNKNOWN_ITEM = "5a17c0de-0000-4000-8000-0000000017e3"
UNKNOWN_BARE = "5a17c0de-0000-4000-8000-0000000ba4e0"
ROUGHNESS = "64e92135-bb0b-4c37-bee3-e7dcdabf179e"


def test_capabilities_name_the_cached_display_records():
    assert ezexb.capabilities()["cached_display_records"] is True


def _codes(doc, entity_id):
    return [d.code for d in doc.report.diagnostics if d.record_id == entity_id]


@pytest.mark.parametrize("major", [16, 18])
def test_cached_display_records_stand_in_for_undecoded_entities(major):
    path = DATA / f"display-{major}.cfb"
    doc = ezexb.read(path, strict=False)
    assert doc == ezexb.read_bytes(path.read_bytes(), strict=False)
    with raw.open(path) as container:
        assert container.document(strict=False) == doc
    entities = {e.id: e for e in doc.entities}
    # Six entities of the stored-document fixture and eleven added ones; the
    # record without any display records is the only unsupported one.
    assert len(entities) == 17
    assert doc.report.decoded_entity_count == 17
    assert doc.report.unsupported_entity_count == 1
    unsupported = [d for d in doc.report.diagnostics if d.code == "unsupported_entity"]
    assert [(d.record_id, d.type_id) for d in unsupported] == [(308, UNKNOWN_BARE)]
    assert 308 not in entities
    assert not any(d.code == "unframed_records" for d in doc.report.diagnostics)

    unknown = entities[304]
    assert unknown.kind == "CachedDisplay" and unknown.type_id == UNKNOWN_CLASS
    assert unknown.style.layer_name == "Test layer"
    shown = unknown.geometry
    assert isinstance(shown, CachedDisplay) and shown.source == "body"
    assert shown.unresolved_semantics_span.length == 11
    line, text, item = shown.presentation_records
    assert line.geometry.category == "Primitive"
    assert line.geometry.geometry == Line((80.0, 0.0, 0.0), (90.0, 0.0, 0.0))
    assert isinstance(text.geometry.geometry, Text)
    assert text.geometry.geometry.contents == "Original Ω"
    assert item.header.type_id == UNKNOWN_ITEM
    assert item.attributes.hide_flag_raw == 0xFFFF
    assert item.geometry.category == "CachedDisplay"
    nested = item.geometry.geometry
    assert isinstance(nested, CachedDisplay)
    assert [r.geometry.geometry.kind for r in nested.presentation_records] == ["Line", "Text"]
    assert nested.unresolved_semantics_span.length == 4
    assert _codes(doc, 304) == ["unsupported_entity_semantics"]

    # A symbol of a save version whose fields are not qualified.
    roughness = entities[310]
    assert roughness.kind == "CachedDisplay" and roughness.type_id == ROUGHNESS
    kinds = [r.geometry.geometry.kind for r in roughness.geometry.presentation_records]
    assert kinds == ["Line", "Line", "Line", "Text"]
    assert roughness.geometry.presentation_records[3].geometry.geometry.contents == "Ra 3.2"
    note = next(d for d in doc.report.diagnostics if d.record_id == 310)
    assert note.code == "unsupported_entity_semantics"
    assert "save-version" in note.message
    assert note.span == roughness.geometry.unresolved_semantics_span

    # Attributes kept only in the schema-3 extension; shown by a cached block reference.
    array = entities[305]
    assert array.object_schema == 3 and array.attributes.layer_id_raw == -24
    assert array.geometry.source == "display_cache"
    (reference,) = array.geometry.presentation_records
    assert reference.geometry.category == "Insert"
    assert isinstance(reference.geometry.geometry, Insert)
    assert reference.geometry.geometry.block_id == -60
    assert reference.geometry.geometry.insertion == (40.0, 50.0, 0.0)

    # Block arrowheads of a dimension, and the three-point angle dimension class.
    dimension = entities[306].geometry
    assert isinstance(dimension, Dimension) and dimension.value_stored == 10.0
    assert [r.geometry.category for r in dimension.presentation_records] == [
        "Primitive",
        "Insert",
        "Insert",
    ]
    assert dimension.presentation_records[2].geometry.geometry.insertion == (70.0, 10.0, 0.0)
    assert _codes(doc, 306) == ["unsupported_dimension_semantics"]
    angle = entities[307].geometry
    assert isinstance(angle, Dimension)
    assert angle.dimension_type_id == "d0e58c76-f166-4241-9943-62e6113a9da4"
    assert angle.value_stored == math.pi / 4
    assert [r.geometry.geometry.kind for r in angle.presentation_records] == ["Arc", "Line"]
    assert angle.unresolved_semantics_span.length == 24


@pytest.mark.parametrize("major", [16, 18])
def test_stored_bases_normals_and_construction_lines(major):
    doc = ezexb.read(DATA / f"display-{major}.cfb", strict=False)
    entities = {e.id: e for e in doc.entities}
    arc = entities[300].geometry
    assert isinstance(arc, Arc)
    assert (arc.radius, arc.x_axis, arc.y_axis) == (5.0, (1.0, 0.0, 0.0), (0.0, 1.0, 0.0))
    assert arc.start == (35.0, 40.0, 0.0)
    assert arc.end == pytest.approx((30.0, 45.0, 0.0))
    assert arc.stored_axes == ((2.0, 0.0, 0.0), (0.0, 2.0, 0.0))
    assert _codes(doc, 300) == ["nonorthonormal_circular_basis"]
    circle = entities[301].geometry
    assert isinstance(circle, Circle) and circle.radius == 4.0
    assert (circle.x_axis, circle.y_axis) == ((1.0, 0.0, 0.0), (0.0, 1.0, 0.0))
    assert circle.stored_axes == ((1.0, 0.0, 0.0), (0.5, 0.5, 0.0))
    assert _codes(doc, 301) == ["nonorthonormal_circular_basis"]
    # Entities stored with an orthonormal basis have no extra field.
    assert entities[3].geometry.stored_axes is None

    xline = entities[302].geometry
    assert xline == Xline((0.0, 25.0, 0.0), (1.0, 0.0, 0.0))
    assert entities[302].kind == "Xline" and _codes(doc, 302) == []
    residue = entities[303].geometry
    assert isinstance(residue, Line) and residue.end == (35.0, 30.0, 2.5e-28)
    assert _codes(doc, 303) == []

    polyline = entities[309].geometry
    assert isinstance(polyline, Polyline) and polyline.normal == (0.0, 0.0, 0.0)
    assert [v.point for v in polyline.vertices] == [(0.0, 50.0), (10.0, 50.0), (10.0, 60.0)]
    assert _codes(doc, 309) == ["degenerate_polyline_normal"]

    nurbs = entities[311].geometry
    assert isinstance(nurbs, Nurbs)
    assert nurbs.fit_points == ((0.0, 70.0, 0.0), (5.0, 75.0, 0.0), (10.0, 70.0, 0.0))
    assert nurbs.fit_point_count_raw == -1 and nurbs.fit_tolerance == 1e-10
    assert len(nurbs.control_points) == 4 and len(nurbs.knots) == 8
    assert _codes(doc, 311) == []


def test_strict_reading_stays_incomplete_and_world_and_svg_accept_the_kinds():
    path = DATA / "display-16.cfb"
    with pytest.raises(ezexb.IncompleteReadError) as caught:
        ezexb.read(path)
    assert caught.value.report.unsupported_entity_count == 1
    codes = {d.code for d in caught.value.report.diagnostics}
    assert {
        "unsupported_entity",
        "unsupported_entity_semantics",
        "nonorthonormal_circular_basis",
        "degenerate_polyline_normal",
    } <= codes
    scene = ezexb.read_world(path, -7, strict=False)
    xline = next(i for i in scene.instances if i.entity_id == 302)
    assert xline.geometry is None
    assert any(
        d.code == "unsupported_world_geometry" and d.record_id == 302 for d in scene.diagnostics
    )
    arc = next(i for i in scene.instances if i.entity_id == 300)
    assert arc.geometry.radius == 5.0
    preview = ezexb.svg_preview(path, -7, strict=False)
    plain = ezexb.svg_preview(DATA / "document-10.cfb", -7)
    # Lines, arcs and the referenced block of the added entities; hidden items are not drawn.
    assert preview.rendered_curve_count == plain.rendered_curve_count + 15
    assert "Hidden item" not in preview.svg
    assert "Ra 3.2" in preview.svg


def test_cli_reads_cached_display_as_json():
    result = subprocess.run(
        [
            sys.executable,
            "-m",
            "ezexb",
            "read",
            str(DATA / "display-18.cfb"),
            "--partial",
            "--json",
        ],
        capture_output=True,
        text=True,
        check=True,
    )
    document = json.loads(result.stdout)
    kinds = {e["id"]: e["geometry"]["kind"] for e in document["entities"]}
    assert kinds[304] == "CachedDisplay" and kinds[302] == "Xline"
    assert kinds[305] == "CachedDisplay"
