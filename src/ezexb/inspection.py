"""Typed results shared by the high-level inspection and raw container API."""

from __future__ import annotations

import json
from dataclasses import dataclass
from typing import Literal


@dataclass(frozen=True)
class EntryInfo:
    path: str
    kind: Literal["storage", "stream"]
    size_bytes: int | None


@dataclass(frozen=True)
class Inspection:
    signature: str
    classification: Literal["unknown", "legacy_caxa_candidate", "cfb", "cfb_exb_candidate"]
    byte_length: int
    cfb_version: int | None
    sector_size: int | None
    exb_confirmed: bool
    entries: tuple[EntryInfo, ...]
    total_stream_bytes: int
    matched_markers: tuple[str, ...]
    missing_markers: tuple[str, ...]
    diagnostics: tuple[str, ...]

    @property
    def streams(self) -> tuple[EntryInfo, ...]:
        return tuple(entry for entry in self.entries if entry.kind == "stream")

    @property
    def storages(self) -> tuple[EntryInfo, ...]:
        return tuple(entry for entry in self.entries if entry.kind == "storage")


def _from_json(value: str) -> Inspection:
    data = json.loads(value)
    data["entries"] = tuple(EntryInfo(**entry) for entry in data["entries"])
    for key in ("matched_markers", "missing_markers", "diagnostics"):
        data[key] = tuple(data[key])
    return Inspection(**data)
