"""Stored boundaries and dimension presentation, separate from model entities."""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING, Literal

from .content import RecordHeader, SourceSpan, TextField, _record_header

if TYPE_CHECKING:
    from .document import Arc, Circle, DegenerateArc, EntityAttributes, Line, Point, Polyline, Text
    from .partno import PartItem


@dataclass(frozen=True)
class BoundaryPoint:
    point: tuple[float, float, float]
    kind: Literal["Point"] = "Point"


@dataclass(frozen=True)
class BoundaryCircular:
    center: tuple[float, float, float]
    radius: float
    x_axis: tuple[float, float, float]
    y_axis: tuple[float, float, float]
    sweep_angle: float
    kind: Literal["Circular"] = "Circular"


@dataclass(frozen=True)
class BoundaryEllipse:
    center: tuple[float, float, float]
    x_axis: tuple[float, float, float]
    y_axis: tuple[float, float, float]
    major_radius: float
    minor_radius: float
    start_angle: float
    end_angle: float
    rotation: float
    kind: Literal["Ellipse"] = "Ellipse"


@dataclass(frozen=True)
class BoundaryNurbs:
    plane: tuple[float, float, float, float]
    degree: int
    type_raw: int
    planar_type_raw: int
    control_points: tuple[tuple[float, float, float, float], ...]
    knots: tuple[float, ...]
    kind: Literal["Nurbs"] = "Nurbs"


@dataclass(frozen=True)
class BoundaryPolyline:
    vertices: tuple[BoundaryVertex, ...]
    closed: bool
    normal: tuple[float, float, float] | None
    kind: Literal["Polyline"] = "Polyline"


@dataclass(frozen=True)
class PresentationGeometry:
    category: str
    geometry: DegenerateArc | Point | Line | Circle | Arc | Polyline | Text | Solid | PartItem


@dataclass(frozen=True)
class Nurbs:
    plane: tuple[float, float, float, float]
    degree: int
    type_raw: int
    planar_type_raw: int
    control_points: tuple[tuple[float, float, float, float], ...]
    knots: tuple[float, ...]
    span: SourceSpan
    kind: Literal["Nurbs"] = "Nurbs"


@dataclass(frozen=True)
class Ellipse:
    center: tuple[float, float, float]
    x_axis: tuple[float, float, float]
    y_axis: tuple[float, float, float]
    major_radius: float
    minor_radius: float
    start_angle: float
    end_angle: float
    rotation: float
    span: SourceSpan
    kind: Literal["Ellipse"] = "Ellipse"


@dataclass(frozen=True)
class BoundaryLoop:
    curves: tuple[BoundaryCurve, ...]
    loop_type_raw: int | None
    span: SourceSpan


@dataclass(frozen=True)
class BoundaryCurve:
    native_type: int
    geometry: BoundaryGeometry
    span: SourceSpan


@dataclass(frozen=True)
class BoundaryVertex:
    point: tuple[float, float]
    legacy_z: float | None
    bulge: float
    start_width: float
    end_width: float


@dataclass(frozen=True)
class FillBoundary:
    pick_point: tuple[float, float, float]
    loops: tuple[BoundaryLoop, ...]
    settings: FillSettings | None
    span: SourceSpan


@dataclass(frozen=True)
class FillSettings:
    associated_raw: int
    relation_flags_raw: int
    relation_tail_raw: int
    evaluation: tuple[float, float, float]
    normal: tuple[float, float, float]
    flags_raw: tuple[int, int, int, int, int]
    vector_raw: tuple[float, float, float]
    flag_raw: int
    pixel_size: float
    span: SourceSpan


@dataclass(frozen=True)
class Hatch:
    boundary: FillBoundary
    pattern: HatchPattern
    span: SourceSpan
    kind: Literal["Hatch"] = "Hatch"


@dataclass(frozen=True)
class HatchPattern:
    name_bytes: tuple[int, ...]
    name_ascii: str | None
    scale: float
    angle_stored: float
    spacing: float
    lines: tuple[HatchPatternLine, ...]
    span: SourceSpan


@dataclass(frozen=True)
class HatchPatternLine:
    parameters_stored: tuple[float, float, float, float, float, float, float]
    dash_count: int
    dashes: tuple[float, ...]
    span: SourceSpan
    reserved_dash_span: SourceSpan


@dataclass(frozen=True)
class StoredSolid:
    boundary: FillBoundary
    gradient: SolidGradient | None
    span: SourceSpan


@dataclass(frozen=True)
class Solid(StoredSolid):
    kind: Literal["Solid"] = "Solid"


@dataclass(frozen=True)
class SolidGradient:
    type_raw: int
    name: TextField
    angle: float
    shift: float
    one_color_mode_raw: int
    shade_tint: float
    colors_raw: tuple[int, int]
    values: tuple[float, float]
    span: SourceSpan


@dataclass(frozen=True)
class Dimension:
    dimension_type_id: str
    value_stored: float
    text_fields_stored: tuple[TextField, ...]
    flags_raw: tuple[int, int, int]
    paper_scale: float
    location: tuple[float, float, float]
    text_location: tuple[float, float, float]
    note_location: tuple[float, float, float]
    presentation_records: tuple[PresentationRecord, ...]
    presentation_span: SourceSpan
    unresolved_semantics_span: SourceSpan
    span: SourceSpan
    kind: Literal["Dimension"] = "Dimension"


@dataclass(frozen=True)
class Roughness:
    symbol_raw: int
    same_requirement_raw: int
    vein_raw: int
    upper_value: TextField
    lower_value: TextField
    upper_caption: TextField
    lower_caption: TextField
    excess: TextField
    minimum_roughness: TextField
    leader_points: tuple[tuple[float, float, float], ...]
    kind: Literal["Roughness"] = "Roughness"


@dataclass(frozen=True)
class GeometricTolerance:
    leader_points: tuple[tuple[float, float, float], ...]
    direction_raw: int
    top_note: TextField
    bottom_note: TextField
    all_round_symbol_raw: int
    kind: Literal["GeometricTolerance"] = "GeometricTolerance"


@dataclass(frozen=True)
class SymbolLabel:
    location: tuple[float, float, float]
    text: TextField


@dataclass(frozen=True)
class SectionMarker:
    cut_points: tuple[tuple[float, float, float], ...]
    labels: tuple[SymbolLabel, ...]
    start_arrow_point: tuple[float, float, float]
    end_arrow_point: tuple[float, float, float]
    arrow_direction_raw: int
    kind: Literal["SectionMarker"] = "SectionMarker"


@dataclass(frozen=True)
class Datum:
    location_reference_raw: tuple[int, int]
    location: tuple[float, float, float]
    lead_line_raw: int
    leader_points: tuple[tuple[float, float, float], ...]
    upper_caption: TextField
    lower_caption: TextField
    aim_raw: int
    mode_raw: int
    name: TextField
    symbol_angle: float
    kind: Literal["Datum"] = "Datum"


SymbolData = Roughness | GeometricTolerance | SectionMarker | Datum


@dataclass(frozen=True)
class Symbol:
    symbol_type_id: str
    data: SymbolData
    presentation_records: tuple[PresentationRecord, ...]
    presentation_span: SourceSpan
    data_span: SourceSpan
    unresolved_semantics_span: SourceSpan
    span: SourceSpan
    kind: Literal["Symbol"] = "Symbol"


@dataclass(frozen=True)
class PresentationRecord:
    header: RecordHeader
    attributes: EntityAttributes | None
    geometry: PresentationGeometry | None
    unsupported_reason: str | None


if TYPE_CHECKING:
    BoundaryGeometry = (
        BoundaryPoint | Line | BoundaryCircular | BoundaryEllipse | BoundaryNurbs | BoundaryPolyline
    )


def _decode(cls, value):
    from .document import (
        Arc,
        Circle,
        DegenerateArc,
        EntityAttributes,
        Line,
        Point,
        Polyline,
        Text,
        _convert,
    )

    value = dict(value)
    for key in (
        "span",
        "reserved_dash_span",
        "presentation_span",
        "data_span",
        "unresolved_semantics_span",
    ):
        if key in value:
            value[key] = SourceSpan(**value[key])
    for key in (
        "center",
        "x_axis",
        "y_axis",
        "pick_point",
        "evaluation",
        "normal",
        "flags_raw",
        "vector_raw",
        "name_bytes",
        "parameters_stored",
        "dashes",
        "colors_raw",
        "values",
        "location",
        "text_location",
        "note_location",
        "point",
        "plane",
        "knots",
        "control_points",
        "leader_points",
        "cut_points",
        "start_arrow_point",
        "end_arrow_point",
        "location_reference_raw",
    ):
        if key in value and value[key] is not None:
            value[key] = tuple(tuple(v) if isinstance(v, list) else v for v in value[key])
    for key, nested in (
        ("boundary", FillBoundary),
        ("pattern", HatchPattern),
        ("settings", FillSettings),
        ("gradient", SolidGradient),
    ):
        if key in value and value[key] is not None:
            value[key] = _decode(nested, value[key])
    for key, nested in (
        ("loops", BoundaryLoop),
        ("curves", BoundaryCurve),
        ("lines", HatchPatternLine),
        ("vertices", BoundaryVertex),
        ("presentation_records", PresentationRecord),
        ("labels", SymbolLabel),
    ):
        if key in value:
            value[key] = tuple(_decode(nested, v) for v in value[key])
    if "text_fields_stored" in value:
        value["text_fields_stored"] = tuple(
            TextField(t["value"], SourceSpan(**t["span"])) for t in value["text_fields_stored"]
        )
    if cls is Symbol:
        classes = {
            "Roughness": Roughness,
            "GeometricTolerance": GeometricTolerance,
            "SectionMarker": SectionMarker,
            "Datum": Datum,
        }
        value["data"] = _decode(classes[value["data"]["kind"]], value["data"])
    if cls in (Roughness, GeometricTolerance, Datum, SymbolLabel):
        for key in (
            "upper_value",
            "lower_value",
            "upper_caption",
            "lower_caption",
            "excess",
            "minimum_roughness",
            "top_note",
            "bottom_note",
            "name",
            "text",
        ):
            if key in value:
                t = value[key]
                value[key] = TextField(t["value"], SourceSpan(**t["span"]))
    if cls is SolidGradient:
        t = value["name"]
        value["name"] = TextField(t["value"], SourceSpan(**t["span"]))
    if cls is BoundaryCurve:
        g = value["geometry"]
        classes = {
            "Point": BoundaryPoint,
            "Line": Line,
            "Circular": BoundaryCircular,
            "Ellipse": BoundaryEllipse,
            "Nurbs": BoundaryNurbs,
            "Polyline": BoundaryPolyline,
        }
        nested = classes[g["kind"]]
        value["geometry"] = (
            _decode(nested, g)
            if nested in (BoundaryNurbs, BoundaryPolyline)
            else _convert(nested, g)
        )
    if cls is PresentationRecord:
        value["header"] = _record_header(value["header"])
        if value["attributes"] is not None:
            value["attributes"] = _convert(EntityAttributes, value["attributes"])
        if value["geometry"] is not None:
            g = value["geometry"]
            nested = g["geometry"]
            if g["category"] == "Solid":
                geometry = _decode(StoredSolid, nested)
            elif g["category"] == "PartItem":
                from .partno import PartItem, _partno

                geometry = _partno(PartItem, nested)
            else:
                geometry = _convert(
                    {
                        "DegenerateArc": DegenerateArc,
                        "Point": Point,
                        "Line": Line,
                        "Circle": Circle,
                        "Arc": Arc,
                        "Polyline": Polyline,
                        "Text": Text,
                    }[nested["kind"]],
                    nested,
                )
            value["geometry"] = PresentationGeometry(g["category"], geometry)
    return cls(**value)
