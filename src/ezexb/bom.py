"""Stored BOM layout and cached row/header presentations, separate from table lookup."""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING, Literal

from .complex import PresentationRecord, _decode
from .content import RecordHeader, SourceSpan, _record_header
from .partno import PartNumberObjectReference, _partno

if TYPE_CHECKING:
    from .document import EntityAttributes


@dataclass(frozen=True)
class BomRewindPoint:
    index_raw: int
    point: tuple[float, float, float]
    span: SourceSpan


@dataclass(frozen=True)
class BomDisplayItem:
    header: RecordHeader
    attributes: EntityAttributes
    item_kind: Literal["Header", "Row"]
    presentation_records: tuple[PresentationRecord, ...]
    presentation_span: SourceSpan
    span: SourceSpan


@dataclass(frozen=True)
class BomTable:
    table_reference: PartNumberObjectReference
    break_direction_raw: int
    relation_type_raw: int
    scale: float
    location: tuple[float, float, float]
    break_point: tuple[float, float, float]
    rewind_points: tuple[BomRewindPoint, ...]
    items: tuple[BomDisplayItem, ...]
    presentation_span: SourceSpan
    span: SourceSpan
    kind: Literal["BomTable"] = "BomTable"


def _bom(value):
    from .document import EntityAttributes, _convert

    value = dict(value)
    for key in ("span", "presentation_span"):
        value[key] = SourceSpan(**value[key])
    for key in ("location", "break_point"):
        value[key] = tuple(value[key])
    value["table_reference"] = _partno(PartNumberObjectReference, value["table_reference"])
    value["rewind_points"] = tuple(
        BomRewindPoint(p["index_raw"], tuple(p["point"]), SourceSpan(**p["span"]))
        for p in value["rewind_points"]
    )
    items = []
    for stored in value["items"]:
        item = dict(stored)
        item["header"] = _record_header(item["header"])
        item["attributes"] = _convert(EntityAttributes, item["attributes"])
        for key in ("span", "presentation_span"):
            item[key] = SourceSpan(**item[key])
        item["presentation_records"] = tuple(
            _decode(PresentationRecord, p) for p in item["presentation_records"]
        )
        items.append(BomDisplayItem(**item))
    value["items"] = tuple(items)
    return BomTable(**value)
