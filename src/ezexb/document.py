"""Stored entities, context and explicit completeness reports.

Coordinates belong to their owning block. World expansion and approximate SVG
are separate Rust operations; these models preserve stored data and diagnostics.
"""

from __future__ import annotations

import json
from dataclasses import dataclass
from typing import Any, Literal

from .bom import BomTable, _bom
from .complex import Dimension, Ellipse, Hatch, Nurbs, Solid, StoredSolid, Symbol, _decode
from .content import FileInfo, RecordPropertyGroup, SourceSpan, TextField, _file_info
from .inspection import EntryInfo
from .partno import PartNumber, _partno
from .tables import Table, _table
from .views import View2D, _view


@dataclass(frozen=True)
class Diagnostic:
    code: str
    message: str
    stream_path: str | None
    span: SourceSpan | None
    record_id: int | None
    type_id: str | None


@dataclass(frozen=True)
class ReadReport:
    profile: str
    complete: bool
    decoded_entity_count: int
    unsupported_entity_count: int
    top_level_record_count: int
    cached_record_count: int
    decoded_bytes: int
    budget_used: int
    diagnostics: tuple[Diagnostic, ...]


@dataclass(frozen=True)
class EntitySource:
    stream_path: str
    span: SourceSpan
    payload_span: SourceSpan


@dataclass(frozen=True)
class EntityAttributes:
    span: SourceSpan
    layer_status_raw: int
    layer_id_raw: int
    linetype_status_raw: int
    linetype_id_raw: int
    color_raw: int
    line_width_raw: float
    linetype_flag_raw: int
    line_width_flag_raw: int
    color_flag_raw: int
    hide_flag_raw: int
    linetype_scale: float
    extension_raw: int | None


@dataclass(frozen=True)
class StyleValue:
    source: str
    value: int | float | None


@dataclass(frozen=True)
class ResolvedStyle:
    layer_id: int
    layer_name: str | None
    linetype: StyleValue
    color_raw: StyleValue
    line_width: StyleValue
    linetype_scale: float


@dataclass(frozen=True)
class DegenerateArc:
    """Stored zero-radius/zero-basis ARC; not a drawable curve."""

    center: tuple[float, float, float]
    radius: float
    x_axis: tuple[float, float, float]
    y_axis: tuple[float, float, float]
    sweep_angle: float
    kind: Literal["DegenerateArc"] = "DegenerateArc"


@dataclass(frozen=True)
class Point:
    point: tuple[float, float, float]
    kind: Literal["Point"] = "Point"


@dataclass(frozen=True)
class Line:
    start: tuple[float, float, float]
    end: tuple[float, float, float]
    kind: Literal["Line"] = "Line"


@dataclass(frozen=True)
class Circle:
    center: tuple[float, float, float]
    radius: float
    x_axis: tuple[float, float, float]
    y_axis: tuple[float, float, float]
    kind: Literal["Circle"] = "Circle"


@dataclass(frozen=True)
class Arc:
    center: tuple[float, float, float]
    radius: float
    x_axis: tuple[float, float, float]
    y_axis: tuple[float, float, float]
    sweep_angle: float
    start: tuple[float, float, float]
    end: tuple[float, float, float]
    kind: Literal["Arc"] = "Arc"


@dataclass(frozen=True)
class PolylineVertex:
    point: tuple[float, float]
    legacy_z: float | None
    bulge: float
    start_width: float
    end_width: float
    span: SourceSpan


@dataclass(frozen=True)
class Polyline:
    vertices: tuple[PolylineVertex, ...]
    closed: bool
    closed_raw: int
    normal: tuple[float, float, float] | None
    span: SourceSpan
    kind: Literal["Polyline"] = "Polyline"


@dataclass(frozen=True)
class StoredText:
    contents: str
    contents_span: SourceSpan
    insertion: tuple[float, float, float]
    text_height_stored: float
    area_height_stored: float
    area_width_stored: float
    rotation: float
    drawing_scale: float
    mode_flags_raw: int
    alignment_raw: int
    writing_direction: int
    fit_mode: int
    style_status_raw: int
    style_id_raw: int
    legacy_format_raw: int | None
    line_spacing_raw: float
    line_spacing_style_raw: int
    format_type_id: str | None
    formatting_span: SourceSpan
    span: SourceSpan


@dataclass(frozen=True)
class Text(StoredText):
    kind: Literal["Text"] = "Text"


@dataclass(frozen=True)
class Insert:
    block_id: int
    block_status: int
    insertion: tuple[float, float, float]
    rotation: float
    scales: tuple[float, float, float]
    matrix_stored: tuple[float, ...]
    attached_attributes: tuple[AttachedAttribute, ...]
    nickname: TextField
    hide_flag_raw: int
    clip_flag_raw: int | None
    span: SourceSpan
    kind: Literal["Insert"] = "Insert"


@dataclass(frozen=True)
class AttachedAttribute:
    record: ContextRecord
    attributes: EntityAttributes
    tag: TextField
    value: TextField
    text: StoredText
    text_attributes: EntityAttributes
    text_record: ContextRecord
    mode_raw: int | None
    visibility: bool | None
    span: SourceSpan


@dataclass(frozen=True)
class AttributeDefinition:
    tag: TextField
    prompt: TextField
    default_value: TextField
    text: StoredText
    text_attributes: EntityAttributes
    text_record: ContextRecord
    mode_raw: int | None
    visibility: bool | None
    span: SourceSpan
    kind: Literal["AttributeDefinition"] = "AttributeDefinition"


Geometry = (
    DegenerateArc
    | Point
    | Line
    | Circle
    | Arc
    | Polyline
    | Text
    | Insert
    | AttributeDefinition
    | Dimension
    | Hatch
    | Solid
    | Ellipse
    | Nurbs
    | PartNumber
    | BomTable
    | View2D
    | Table
    | Symbol
)


@dataclass(frozen=True)
class Entity:
    id: int
    owner_id: int
    block_id: int | None
    type_id: str
    object_schema: int
    source: EntitySource
    attributes: EntityAttributes
    style: ResolvedStyle
    geometry: Geometry
    properties: tuple[RecordPropertyGroup, ...]

    @property
    def kind(self) -> str:
        return self.geometry.kind


@dataclass(frozen=True)
class ContextRecord:
    class_name: str | None
    type_id: str
    id: int
    owner: int
    object_schema: int
    span: SourceSpan
    payload_span: SourceSpan
    property_span: SourceSpan | None
    extension_span: SourceSpan | None


@dataclass(frozen=True)
class Layer:
    id: int
    owner: int
    span: SourceSpan
    payload_span: SourceSpan
    name: TextField
    alternate_name: TextField
    key_name: TextField
    description: TextField
    color_raw: int
    status_raw: int
    width: float
    width_flag: int
    linetype_id: int
    linetype_status: int
    on: bool
    locked: bool
    printable: bool
    frozen: bool
    unknown_fields: tuple[SourceSpan, ...]


@dataclass(frozen=True)
class Linetype:
    id: int
    owner: int
    span: SourceSpan
    payload_span: SourceSpan
    name: TextField
    description: TextField
    alternate_name: TextField
    scale: float
    dashes: tuple[float, ...]
    pattern: int | None
    advanced_present: bool


@dataclass(frozen=True)
class Space:
    id: int
    owner: int
    span: SourceSpan
    payload_span: SourceSpan
    name: TextField
    scale: float
    block_id: int
    block_status: int
    current_ucs_id: int
    unknown_fields: tuple[SourceSpan, ...]


@dataclass(frozen=True)
class Block:
    solid_attachment: ContextRecord | None
    solid_geometry: StoredSolid | None
    solid_attributes: EntityAttributes | None
    complete: bool
    diagnostics: tuple[str, ...]
    current_ucs_id: int
    id: int
    owner: int
    span: SourceSpan
    payload_span: SourceSpan
    name: TextField
    unknown_fields: tuple[SourceSpan, ...]


@dataclass(frozen=True)
class StyleContext:
    evidence: str
    complete: bool
    layers: tuple[Layer, ...]
    linetypes: tuple[Linetype, ...]
    other_records: tuple[ContextRecord, ...]
    diagnostics: tuple[str, ...]
    unparsed_span: SourceSpan | None
    budget_used: int


@dataclass(frozen=True)
class SpaceContext:
    evidence: str
    complete: bool
    spaces: tuple[Space, ...]
    other_records: tuple[ContextRecord, ...]
    diagnostics: tuple[str, ...]
    unparsed_span: SourceSpan | None
    budget_used: int


@dataclass(frozen=True)
class TextStyle:
    id: int
    owner: int
    span: SourceSpan
    name: TextField
    alternate_name: TextField
    western_font: TextField
    cjk_font: TextField
    western_width_factor: float
    cjk_width_factor: float
    character_spacing_factor: float
    line_spacing_factor: float
    height: float
    oblique_angle: float
    indent: float
    shape_file_flag_raw: int | None
    shape_file_path: TextField | None
    extra_flags_raw: tuple[int, int, int] | None


@dataclass(frozen=True)
class TextStyleContext:
    complete: bool
    styles: tuple[TextStyle, ...]
    diagnostics: tuple[str, ...]


@dataclass(frozen=True)
class DrawingBlock:
    definition: Block
    stream_path: str
    entity_indices: tuple[int, ...]


@dataclass(frozen=True)
class Document:
    profile: str
    evidence: str
    file_info: FileInfo
    coordinate_system: str
    units: str | None
    styles: StyleContext
    text_style_context: TextStyleContext
    space_context: SpaceContext
    blocks: tuple[DrawingBlock, ...]
    entities: tuple[Entity, ...]
    uninterpreted_streams: tuple[EntryInfo, ...]
    report: ReadReport

    @property
    def text_styles(self) -> tuple[TextStyle, ...]:
        return self.text_style_context.styles

    @property
    def layers(self) -> tuple[Layer, ...]:
        return self.styles.layers

    @property
    def linetypes(self) -> tuple[Linetype, ...]:
        return self.styles.linetypes

    @property
    def spaces(self) -> tuple[Space, ...]:
        return self.space_context.spaces

    def entities_in_block(self, block_id: int) -> tuple[Entity, ...]:
        """Direct members only; INSERT and cached display records are not expanded."""
        if not any(block.definition.id == block_id for block in self.blocks):
            raise KeyError(block_id)
        return tuple(entity for entity in self.entities if entity.block_id == block_id)

    def entities_in_space(self, space_id: int) -> tuple[Entity, ...]:
        """Select by the stored space ID, without guessing model/paper names."""
        for space in self.spaces:
            if space.id == space_id:
                return self.entities_in_block(space.block_id)
        raise KeyError(space_id)


def _convert(cls: type, value: dict[str, Any]) -> Any:
    """Convert explicitly registered nested types; preserve all Rust fields."""
    value = dict(value)
    for key in (
        "span",
        "payload_span",
        "property_span",
        "extension_span",
        "unparsed_span",
        "contents_span",
        "formatting_span",
    ):
        if key in value and value[key] is not None:
            value[key] = SourceSpan(**value[key])
    for key in (
        "name",
        "description",
        "alternate_name",
        "key_name",
        "nickname",
        "tag",
        "value",
        "prompt",
        "default_value",
        "western_font",
        "cjk_font",
        "shape_file_path",
    ):
        if key in value and value[key] is not None:
            text = dict(value[key])
            text["span"] = SourceSpan(**text["span"])
            value[key] = TextField(**text)
    for key in ("unknown_fields",):
        if key in value:
            value[key] = tuple(SourceSpan(**span) for span in value[key])
    for key in (
        "start",
        "end",
        "center",
        "x_axis",
        "y_axis",
        "insertion",
        "normal",
        "point",
        "dashes",
        "entity_indices",
        "scales",
        "matrix_stored",
        "extra_flags_raw",
    ):
        if key in value and value[key] is not None:
            value[key] = tuple(value[key])
    if cls in (StyleContext, SpaceContext, ReadReport):
        types = {
            "layers": Layer,
            "linetypes": Linetype,
            "spaces": Space,
            "other_records": ContextRecord,
            "diagnostics": Diagnostic,
        }
        for key, nested in types.items():
            if key in value:
                value[key] = tuple(
                    _convert(nested, item) if isinstance(item, dict) else item
                    for item in value[key]
                )
    if cls is Block:
        value["diagnostics"] = tuple(value["diagnostics"])
        if value["solid_attachment"] is not None:
            value["solid_attachment"] = _convert(ContextRecord, value["solid_attachment"])
        if value["solid_geometry"] is not None:
            value["solid_geometry"] = _decode(StoredSolid, value["solid_geometry"])
        if value["solid_attributes"] is not None:
            value["solid_attributes"] = _convert(EntityAttributes, value["solid_attributes"])
    if cls is TextStyleContext:
        value["styles"] = tuple(_convert(TextStyle, item) for item in value["styles"])
        value["diagnostics"] = tuple(value["diagnostics"])
    if cls is Insert:
        value["attached_attributes"] = tuple(
            _convert(AttachedAttribute, item) for item in value["attached_attributes"]
        )
    if cls in (AttachedAttribute, AttributeDefinition):
        value["text"] = _convert(StoredText, value["text"])
        value["text_attributes"] = _convert(EntityAttributes, value["text_attributes"])
        value["text_record"] = _convert(ContextRecord, value["text_record"])
        if cls is AttachedAttribute:
            value["attributes"] = _convert(EntityAttributes, value["attributes"])
            value["record"] = _convert(ContextRecord, value["record"])
    if cls is Polyline:
        value["vertices"] = tuple(_convert(PolylineVertex, item) for item in value["vertices"])
    return cls(**value)


def _report(value: str | dict[str, Any]) -> ReadReport:
    return _convert(ReadReport, json.loads(value) if isinstance(value, str) else value)


def _document(value: str) -> Document:
    from .content import _property_group

    data = json.loads(value)
    data["file_info"] = _file_info(json.dumps(data["file_info"]))
    data["styles"] = _convert(StyleContext, data["styles"])
    data["text_style_context"] = _convert(TextStyleContext, data["text_style_context"])
    data["space_context"] = _convert(SpaceContext, data["space_context"])
    data["report"] = _report(data["report"])
    data["blocks"] = tuple(
        DrawingBlock(_convert(Block, b["definition"]), b["stream_path"], tuple(b["entity_indices"]))
        for b in data["blocks"]
    )
    entities = []
    classes = {
        "DegenerateArc": DegenerateArc,
        "Point": Point,
        "Line": Line,
        "Circle": Circle,
        "Arc": Arc,
        "Polyline": Polyline,
        "Text": Text,
        "Insert": Insert,
        "AttributeDefinition": AttributeDefinition,
        "Dimension": Dimension,
        "Hatch": Hatch,
        "Solid": Solid,
        "Ellipse": Ellipse,
        "Nurbs": Nurbs,
        "PartNumber": PartNumber,
        "BomTable": BomTable,
        "View2D": View2D,
        "Table": Table,
        "Symbol": Symbol,
    }
    for item in data["entities"]:
        item["source"] = _convert(EntitySource, item["source"])
        item["attributes"] = _convert(EntityAttributes, item["attributes"])
        style = item["style"]
        for key in ("linetype", "color_raw", "line_width"):
            style[key] = StyleValue(**style[key])
        item["style"] = ResolvedStyle(**style)
        cls = classes[item["geometry"]["kind"]]
        decoder = (
            _partno
            if cls is PartNumber
            else _decode
            if cls in (Dimension, Hatch, Solid, Ellipse, Nurbs, Symbol)
            else _convert
        )
        item["geometry"] = (
            _bom(item["geometry"])
            if cls is BomTable
            else _view(item["geometry"])
            if cls is View2D
            else _table(Table, item["geometry"])
            if cls is Table
            else decoder(cls, item["geometry"])
        )
        item["properties"] = tuple(_property_group(prop) for prop in item["properties"])
        entities.append(Entity(**item))
    data["entities"] = tuple(entities)
    data["uninterpreted_streams"] = tuple(EntryInfo(**e) for e in data["uninterpreted_streams"])
    return Document(**data)
