"""Observed EXB stream structures. Spans are relative to their containing stream.

AppInfo and record-header spans refer to the decompressed logical payload.
FileInfo and preview spans refer to the original uncompressed stream.
"""

from __future__ import annotations

import json
from dataclasses import dataclass


@dataclass(frozen=True)
class SourceSpan:
    offset: int
    length: int


@dataclass(frozen=True)
class TextField:
    value: str
    span: SourceSpan


@dataclass(frozen=True)
class UnknownField:
    span: SourceSpan
    hex: str


@dataclass(frozen=True)
class MetadataProperty:
    key: TextField
    value: TextField


@dataclass(frozen=True)
class FileInfo:
    evidence: str
    stream_version: TextField
    document_type: TextField
    product_build: TextField
    copyright: TextField
    properties: tuple[MetadataProperty, ...]
    unknown_fields: tuple[UnknownField, ...]


@dataclass(frozen=True)
class AppRecord:
    span: SourceSpan
    type_id_hex: str
    id_raw: int
    owner_raw: int
    schema: int
    strings: tuple[TextField, ...]
    unknown_fields: tuple[UnknownField, ...]


@dataclass(frozen=True)
class AppInfo:
    evidence: str
    stream_version: TextField
    reserved: UnknownField
    records: tuple[AppRecord, ...]


@dataclass(frozen=True)
class Metadata:
    file_info: FileInfo
    app_info: AppInfo


@dataclass(frozen=True)
class PreviewInfo:
    evidence: str
    stream_version: TextField
    record_type_id_hex: str
    bmp_span: SourceSpan
    width: int
    height: int
    bits_per_pixel: int
    compression: int
    row_stride: int
    pixel_bytes: int
    unknown_fields: tuple[UnknownField, ...]


@dataclass(frozen=True)
class StreamPayloadBlock:
    stream_offset: int
    compressed_size: int
    decoded_size: int
    logical_offset: int
    logical_size: int


@dataclass(frozen=True)
class StreamPayloadInfo:
    encoding: str
    logical_size: int
    allocated_size: int
    blocks: tuple[StreamPayloadBlock, ...]


@dataclass(frozen=True)
class DecodedStream:
    info: StreamPayloadInfo
    data: bytes


@dataclass(frozen=True)
class RecordPropertyValue:
    tag: int
    key: TextField | None
    object_type_id: str | None
    value_span: SourceSpan
    span: SourceSpan


@dataclass(frozen=True)
class RecordPropertyGroup:
    type_id: str
    span: SourceSpan
    values: tuple[RecordPropertyValue, ...]


@dataclass(frozen=True)
class RecordRelation:
    """A bounded relation with opaque value bytes available at its source span."""

    type_id: str
    class_name: str
    span: SourceSpan


@dataclass(frozen=True)
class RecordDisplayCache:
    """Nested display records, separate from ordinary top-level entities."""

    span: SourceSpan
    presence_span: SourceSpan
    present: bool
    type_id: str | None
    records: tuple[RecordHeader, ...]


@dataclass(frozen=True)
class RecordExtension:
    span: SourceSpan
    relation_section_span: SourceSpan
    relation_type_id: str
    relations: tuple[RecordRelation, ...]
    header_span: SourceSpan
    header_u16_raw: int
    header_u32_raw: tuple[int, int]
    attributes_span: SourceSpan
    display_cache: RecordDisplayCache
    tail_span: SourceSpan
    tail_raw: int


@dataclass(frozen=True)
class RecordHeader:
    type_id: str
    class_name: str | None
    id_raw: int
    owner_raw: int
    schema: int
    span: SourceSpan
    payload_span: SourceSpan
    object_schema: int = 1
    properties_present: bool = False
    property_groups: tuple[RecordPropertyGroup, ...] = ()
    extension: RecordExtension | None = None


@dataclass(frozen=True)
class RecordInspection:
    evidence: str
    offset_space: str
    stream_version: str
    records: tuple[RecordHeader, ...]
    framing_complete: bool
    unparsed_span: SourceSpan | None
    stop_reason: str | None
    geometry_decoded: bool
    profile: str | None = None


@dataclass(frozen=True)
class LineAttributes:
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


@dataclass(frozen=True)
class LineRecord:
    record_index: int
    start: tuple[float, float, float]
    end: tuple[float, float, float]
    coordinates_span: SourceSpan
    attributes: LineAttributes


@dataclass(frozen=True)
class UnsupportedLine:
    record_index: int
    reason: str


@dataclass(frozen=True)
class LineInspection:
    profile: str
    evidence: str
    drawing_complete: bool
    framing: RecordInspection
    lines: tuple[LineRecord, ...]
    unsupported_lines: tuple[UnsupportedLine, ...]
    other_record_count: int


def _text(data: dict) -> TextField:
    return TextField(value=data["value"], span=SourceSpan(**data["span"]))


def _unknown(data: dict) -> UnknownField:
    return UnknownField(span=SourceSpan(**data["span"]), hex=data["hex"])


def _file_info(value: str) -> FileInfo:
    data = json.loads(value)
    for key in ("stream_version", "document_type", "product_build", "copyright"):
        data[key] = _text(data[key])
    data["properties"] = tuple(
        MetadataProperty(key=_text(p["key"]), value=_text(p["value"])) for p in data["properties"]
    )
    data["unknown_fields"] = tuple(map(_unknown, data["unknown_fields"]))
    return FileInfo(**data)


def _app_info(value: str) -> AppInfo:
    data = json.loads(value)
    data["stream_version"] = _text(data["stream_version"])
    data["reserved"] = _unknown(data["reserved"])
    records = []
    for record in data["records"]:
        record["span"] = SourceSpan(**record["span"])
        record["strings"] = tuple(map(_text, record["strings"]))
        record["unknown_fields"] = tuple(map(_unknown, record["unknown_fields"]))
        records.append(AppRecord(**record))
    data["records"] = tuple(records)
    return AppInfo(**data)


def _preview_info(value: str) -> PreviewInfo:
    data = json.loads(value)
    data["stream_version"] = _text(data["stream_version"])
    data["bmp_span"] = SourceSpan(**data["bmp_span"])
    data["unknown_fields"] = tuple(map(_unknown, data["unknown_fields"]))
    return PreviewInfo(**data)


def _payload(info: str, data: bytes) -> DecodedStream:
    fields = json.loads(info)
    fields["blocks"] = tuple(StreamPayloadBlock(**block) for block in fields["blocks"])
    return DecodedStream(StreamPayloadInfo(**fields), data)


def _property_group(group: dict) -> RecordPropertyGroup:
    group = dict(group)
    values = []
    for original in group["values"]:
        value = dict(original)
        value["span"] = SourceSpan(**value["span"])
        value["value_span"] = SourceSpan(**value["value_span"])
        if value["key"] is not None:
            value["key"] = _text(value["key"])
        values.append(RecordPropertyValue(**value))
    group["values"] = tuple(values)
    group["span"] = SourceSpan(**group["span"])
    return RecordPropertyGroup(**group)


def _record_header(record: dict) -> RecordHeader:
    record["span"] = SourceSpan(**record["span"])
    record["payload_span"] = SourceSpan(**record["payload_span"])
    record["property_groups"] = tuple(_property_group(group) for group in record["property_groups"])
    extension = record.get("extension")
    if extension is not None:
        for field in (
            "span",
            "relation_section_span",
            "header_span",
            "attributes_span",
            "tail_span",
        ):
            extension[field] = SourceSpan(**extension[field])
        extension["header_u32_raw"] = tuple(extension["header_u32_raw"])
        extension["relations"] = tuple(
            RecordRelation(
                type_id=item["type_id"],
                class_name=item["class_name"],
                span=SourceSpan(**item["span"]),
            )
            for item in extension["relations"]
        )
        cache = extension["display_cache"]
        cache["span"] = SourceSpan(**cache["span"])
        cache["presence_span"] = SourceSpan(**cache["presence_span"])
        cache["records"] = tuple(map(_record_header, cache["records"]))
        extension["display_cache"] = RecordDisplayCache(**cache)
        record["extension"] = RecordExtension(**extension)
    return RecordHeader(**record)


def _record_inspection(data: dict) -> RecordInspection:
    data["records"] = tuple(map(_record_header, data["records"]))
    if data["unparsed_span"] is not None:
        data["unparsed_span"] = SourceSpan(**data["unparsed_span"])
    return RecordInspection(**data)


def _records(value: str) -> RecordInspection:
    return _record_inspection(json.loads(value))


def _lines(value: str) -> LineInspection:
    data = json.loads(value)
    data["framing"] = _record_inspection(data["framing"])
    lines = []
    for line in data["lines"]:
        line["start"] = tuple(line["start"])
        line["end"] = tuple(line["end"])
        line["coordinates_span"] = SourceSpan(**line["coordinates_span"])
        line["attributes"]["span"] = SourceSpan(**line["attributes"]["span"])
        line["attributes"] = LineAttributes(**line["attributes"])
        lines.append(LineRecord(**line))
    data["lines"] = tuple(lines)
    data["unsupported_lines"] = tuple(UnsupportedLine(**item) for item in data["unsupported_lines"])
    return LineInspection(**data)
