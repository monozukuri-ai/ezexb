"""Stored table cells and hole-chart fields, without style or link evaluation."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Literal

from .complex import PresentationRecord, _decode
from .content import SourceSpan


@dataclass(frozen=True)
class TableObjectReference:
    status_raw: int
    id_raw: int
    span: SourceSpan


@dataclass(frozen=True)
class TableColumn:
    index: int
    width: float
    span: SourceSpan


@dataclass(frozen=True)
class TableTextContent:
    text: str
    kind: Literal["Text"] = "Text"


@dataclass(frozen=True)
class TableBlockContent:
    auto_scale_raw: int
    reference: TableObjectReference
    scale: float
    rotation: float
    kind: Literal["Block"] = "Block"


@dataclass(frozen=True)
class TableContent:
    index: int
    type_raw: int
    value: TableTextContent | TableBlockContent
    span: SourceSpan


@dataclass(frozen=True)
class TablePropertyValue:
    kind: Literal["U16", "U32", "F64"]
    value: int | float


@dataclass(frozen=True)
class TableProperty:
    index: int
    type_raw: int
    value: TablePropertyValue
    span: SourceSpan


@dataclass(frozen=True)
class TableCell:
    index: int
    state_raw: int
    merged_raw: int
    merge_range_raw: tuple[int, int, int, int]
    flags_raw: int
    style_name: str
    contents: tuple[TableContent, ...]
    overrides: tuple[TableProperty, ...]
    span: SourceSpan


@dataclass(frozen=True)
class TableRow:
    index: int
    height: float
    cells: tuple[TableCell, ...]
    reference: TableObjectReference
    state_raw: int
    index_prefix: str
    index_suffix: str
    span: SourceSpan


@dataclass(frozen=True)
class TableIndexedValue:
    index: int
    value: float
    span: SourceSpan


@dataclass(frozen=True)
class TableBreakData:
    flag_raw: int
    parameters_raw: tuple[int, int]
    spacing_raw: float
    values: tuple[TableIndexedValue, ...]
    vector_raw: tuple[float, float, float]
    span: SourceSpan


@dataclass(frozen=True)
class HoleChartColumn:
    index: int
    property_raw: int
    data_type_raw: int
    layout_raw: int
    precision_raw: int
    span: SourceSpan


@dataclass(frozen=True)
class HoleChartData:
    coordinate_matrix_raw: tuple[float, ...]
    coordinate_status_raw: int
    coordinate_name: str
    parameters_raw: tuple[int, int, int, int]
    flags_raw: tuple[int, int]
    sizes_raw: tuple[float, float]
    columns: tuple[HoleChartColumn, ...]
    references: tuple[TableObjectReference, ...]
    span: SourceSpan


@dataclass(frozen=True)
class Table:
    table_type: Literal["Table", "HoleChart"]
    style_reference: TableObjectReference
    columns: tuple[TableColumn, ...]
    rows: tuple[TableRow, ...]
    location: tuple[float, float, float]
    rotation: float
    scale: float
    overrides: tuple[TableProperty, ...]
    break_data: TableBreakData | None
    hole_chart: HoleChartData | None
    presentation_records: tuple[PresentationRecord, ...]
    presentation_span: SourceSpan
    data_span: SourceSpan
    unresolved_semantics_span: SourceSpan
    span: SourceSpan
    kind: Literal["Table"] = "Table"


def _table(cls, value):
    value = dict(value)
    for key in ("span", "data_span", "presentation_span", "unresolved_semantics_span"):
        if key in value:
            value[key] = SourceSpan(**value[key])
    for key in (
        "location",
        "merge_range_raw",
        "parameters_raw",
        "vector_raw",
        "coordinate_matrix_raw",
        "sizes_raw",
    ):
        if key in value:
            value[key] = tuple(value[key])
    if cls is HoleChartData:
        value["flags_raw"] = tuple(value["flags_raw"])
    for key in ("reference", "style_reference"):
        if key in value:
            value[key] = _table(TableObjectReference, value[key])
    if "references" in value:
        value["references"] = tuple(_table(TableObjectReference, v) for v in value["references"])
    for key, item_cls in (
        ("rows", TableRow),
        ("cells", TableCell),
        ("contents", TableContent),
        ("overrides", TableProperty),
        ("values", TableIndexedValue),
    ):
        if key in value:
            value[key] = tuple(_table(item_cls, v) for v in value[key])
    if "columns" in value:
        column_cls = HoleChartColumn if cls is HoleChartData else TableColumn
        value["columns"] = tuple(_table(column_cls, v) for v in value["columns"])
    if cls is TableProperty:
        value["value"] = TablePropertyValue(**value["value"])
    elif cls is TableContent:
        content_cls = TableBlockContent if value["value"]["kind"] == "Block" else TableTextContent
        value["value"] = _table(content_cls, value["value"])
    elif cls is Table:
        for key, item_cls in (("break_data", TableBreakData), ("hole_chart", HoleChartData)):
            if value[key] is not None:
                value[key] = _table(item_cls, value[key])
        value["presentation_records"] = tuple(
            _decode(PresentationRecord, v) for v in value["presentation_records"]
        )
    return cls(**value)
