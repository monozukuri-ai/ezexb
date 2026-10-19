"""Stored 2D view references and clipping data, without inferred view expansion."""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING, Literal

from .complex import BoundaryCurve, _decode
from .content import SourceSpan, TextField

if TYPE_CHECKING:
    from .document import Insert


@dataclass(frozen=True)
class ViewClip:
    reference_bytes: tuple[int, ...]
    reference_span: SourceSpan
    type_raw: int
    flag_raw: int
    curves: tuple[BoundaryCurve, ...]
    span: SourceSpan


@dataclass(frozen=True)
class View2D:
    block_reference: Insert
    name: TextField
    view_type_raw: int
    clip: ViewClip | None
    span: SourceSpan
    kind: Literal["View2D"] = "View2D"


def _view(value):
    from .document import Insert, _convert

    value = dict(value)
    value["block_reference"] = _convert(Insert, value["block_reference"])
    value["name"] = TextField(value["name"]["value"], SourceSpan(**value["name"]["span"]))
    value["span"] = SourceSpan(**value["span"])
    if value["clip"] is not None:
        clip = dict(value["clip"])
        clip["reference_bytes"] = tuple(clip["reference_bytes"])
        clip["reference_span"] = SourceSpan(**clip["reference_span"])
        clip["span"] = SourceSpan(**clip["span"])
        clip["curves"] = tuple(_decode(BoundaryCurve, c) for c in clip["curves"])
        value["clip"] = ViewClip(**clip)
    return View2D(**value)
