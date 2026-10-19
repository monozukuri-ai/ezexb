"""Stored part-number callouts; table lookup and number regeneration are unresolved."""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING, Literal

from .complex import PresentationRecord, _decode
from .content import RecordHeader, SourceSpan, _record_header

if TYPE_CHECKING:
    from .document import EntityAttributes


@dataclass(frozen=True)
class PartNumberObjectReference:
    status_raw: int
    id_raw: int
    span: SourceSpan


@dataclass(frozen=True)
class PartNumberReferencePoint:
    flags_raw: int
    type_raw: int
    parameter_bytes: tuple[int, ...]
    id_arrays: tuple[tuple[int, ...], tuple[int, ...]]
    points_raw: tuple[
        tuple[float, float, float] | None,
        tuple[float, float, float] | None,
        tuple[float, float, float] | None,
    ]
    parameters_raw: tuple[float | None, float | None]
    span: SourceSpan


@dataclass(frozen=True)
class PartItem:
    index_raw: int
    presentation_records: tuple[PresentationRecord, ...]
    presentation_span: SourceSpan
    span: SourceSpan


@dataclass(frozen=True)
class PartNumberItem:
    header: RecordHeader
    attributes: EntityAttributes
    geometry: PartItem


@dataclass(frozen=True)
class PartNumber:
    style_reference: PartNumberObjectReference
    table_reference: PartNumberObjectReference
    horizontal_or_vertical_raw: int
    array_mode_raw: int
    orientation_raw: int
    auto_or_reverse_raw: int
    start_reference: PartNumberReferencePoint
    leader_points: tuple[tuple[float, float, float], ...]
    scale: float
    items: tuple[PartNumberItem, ...]
    override_count_raw: int
    presentation_records: tuple[PresentationRecord, ...]
    presentation_span: SourceSpan
    span: SourceSpan
    kind: Literal["PartNumber"] = "PartNumber"


def _partno(cls, value):
    from .document import EntityAttributes, _convert

    value = dict(value)
    for key in ("span", "presentation_span"):
        if key in value:
            value[key] = SourceSpan(**value[key])
    if "presentation_records" in value:
        value["presentation_records"] = tuple(
            _decode(PresentationRecord, p) for p in value["presentation_records"]
        )
    if cls is PartNumber:
        for key in ("style_reference", "table_reference"):
            value[key] = _partno(PartNumberObjectReference, value[key])
        value["start_reference"] = _partno(PartNumberReferencePoint, value["start_reference"])
        value["leader_points"] = tuple(tuple(p) for p in value["leader_points"])
        value["items"] = tuple(_partno(PartNumberItem, p) for p in value["items"])
    elif cls is PartNumberReferencePoint:
        for key in ("parameter_bytes", "parameters_raw"):
            value[key] = tuple(value[key])
        value["id_arrays"] = tuple(tuple(a) for a in value["id_arrays"])
        value["points_raw"] = tuple(None if p is None else tuple(p) for p in value["points_raw"])
    elif cls is PartNumberItem:
        value["header"] = _record_header(value["header"])
        value["attributes"] = _convert(EntityAttributes, value["attributes"])
        value["geometry"] = _partno(PartItem, value["geometry"])
    return cls(**value)
