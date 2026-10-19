"""World-coordinate instances and a portable SVG preview produced by the Rust core."""

from __future__ import annotations

import json
from dataclasses import dataclass
from typing import Literal

from .document import (
    Arc,
    Circle,
    Diagnostic,
    Line,
    ResolvedStyle,
    StyleValue,
    _convert,
)


@dataclass(frozen=True)
class WorldTransform:
    matrix: tuple[float, ...]


@dataclass(frozen=True)
class EllipticCurve:
    center: tuple[float, float, float]
    axis_u: tuple[float, float, float]
    axis_v: tuple[float, float, float]
    sweep_angle: float
    closed: bool
    start: tuple[float, float, float]
    end: tuple[float, float, float]
    kind: Literal["EllipticCurve"] = "EllipticCurve"


WorldCurve = Line | Circle | Arc | EllipticCurve


@dataclass(frozen=True)
class WorldPolylineSegment:
    curve: WorldCurve
    start_width_stored: float
    end_width_stored: float


@dataclass(frozen=True)
class WorldPolyline:
    closed: bool
    segments: tuple[WorldPolylineSegment, ...]
    kind: Literal["Polyline"] = "Polyline"


@dataclass(frozen=True)
class WorldInstance:
    entity_index: int
    entity_id: int
    insert_path: tuple[int, ...]
    transform: WorldTransform
    style: ResolvedStyle
    visible: bool
    geometry: WorldCurve | WorldPolyline | None


@dataclass(frozen=True)
class WorldScene:
    space_id: int
    root_block_id: int
    source_document_complete: bool
    complete: bool
    transforms_complete: bool
    geometry_complete: bool
    styles_complete: bool
    render_complete: bool
    budget_used: int
    instances: tuple[WorldInstance, ...]
    diagnostics: tuple[Diagnostic, ...]


@dataclass(frozen=True)
class SvgPreview:
    svg: str
    space_id: int
    source_document_complete: bool
    native_display_equivalent: bool
    rendered_curve_count: int
    rendered_text_count: int
    diagnostics: tuple[str, ...]


def _curve(value: dict) -> WorldCurve | WorldPolyline:
    value = dict(value)
    kind = value["kind"]
    if kind == "Polyline":
        value["segments"] = tuple(
            WorldPolylineSegment(_curve(v["curve"]), v["start_width_stored"], v["end_width_stored"])
            for v in value["segments"]
        )
        return WorldPolyline(**value)
    for key in ("axis_u", "axis_v"):
        if key in value:
            value[key] = tuple(value[key])
    return _convert(
        {"Line": Line, "Circle": Circle, "Arc": Arc, "EllipticCurve": EllipticCurve}[kind], value
    )


def _world(value: str) -> WorldScene:
    data = json.loads(value)
    instances = []
    for item in data["instances"]:
        item["insert_path"] = tuple(item["insert_path"])
        item["transform"] = WorldTransform(tuple(item["transform"]["matrix"]))
        style = item["style"]
        for key in ("linetype", "color_raw", "line_width"):
            style[key] = StyleValue(**style[key])
        item["style"] = ResolvedStyle(**style)
        if item["geometry"] is not None:
            item["geometry"] = _curve(item["geometry"])
        instances.append(WorldInstance(**item))
    data["instances"] = tuple(instances)
    data["diagnostics"] = tuple(_convert(Diagnostic, d) for d in data["diagnostics"])
    return WorldScene(**data)


def _svg(value: str) -> SvgPreview:
    data = json.loads(value)
    data["diagnostics"] = tuple(data["diagnostics"])
    return SvgPreview(**data)
