"""Bounded EXB stream analysis and a limited stored-entity reader."""

from __future__ import annotations

import os
from collections.abc import Callable
from dataclasses import dataclass
from typing import Literal

from . import _core
from ._core import (
    DEFAULT_MAX_FILE_BYTES,
    ExbError,
    IncompleteReadError,
    InputLimitError,
    InvalidContainerError,
    InvalidInputError,
    InvalidStreamError,
    StreamNotFoundError,
    UnsupportedFeatureError,
    __version__,
)
from .content import AppInfo, FileInfo, Metadata, PreviewInfo
from .document import Document, _document, _report
from .inspection import EntryInfo, Inspection, _from_json
from .world import SvgPreview, WorldScene

__all__ = [
    "DEFAULT_MAX_FILE_BYTES",
    "ExbError",
    "HeaderInfo",
    "InputLimitError",
    "IncompleteReadError",
    "LEGACY_PROFILE",
    "LEGACY_SIGNATURE",
    "Document",
    "WorldScene",
    "SvgPreview",
    "InvalidContainerError",
    "InvalidInputError",
    "InvalidStreamError",
    "StreamNotFoundError",
    "EntryInfo",
    "Inspection",
    "ParseOptions",
    "UnsupportedFeatureError",
    "__version__",
    "capabilities",
    "inspect",
    "inspect_bytes",
    "probe",
    "probe_bytes",
    "read",
    "read_bytes",
    "read_world",
    "svg_preview",
    "metadata",
    "metadata_bytes",
    "preview",
    "preview_bytes",
    "FileInfo",
    "AppInfo",
    "Metadata",
    "PreviewInfo",
]


#: Profile name of documents read from the legacy ``CaxaEbForWindows`` container.
LEGACY_PROFILE = "exb-legacy-planar-v1"
#: The counted signature string that opens a legacy file.
LEGACY_SIGNATURE = b"\x10CaxaEbForWindows"


@dataclass(frozen=True)
class ParseOptions:
    """Inclusive resource budgets; record depth zero allows top-level records only."""

    max_file_bytes: int = DEFAULT_MAX_FILE_BYTES
    max_entries: int = 10_000
    max_stream_bytes: int = 64 * 1024 * 1024
    max_total_stream_bytes: int = 256 * 1024 * 1024
    max_storage_depth: int = 64
    max_container_read_bytes: int = 64 * 1024 * 1024
    max_decoded_stream_bytes: int = 64 * 1024 * 1024
    max_string_bytes: int = 64 * 1024
    max_preview_pixels: int = 16 * 1024 * 1024
    max_records: int = 100_000
    max_record_depth: int = 32
    max_total_decoded_bytes: int = 256 * 1024 * 1024

    def __post_init__(self) -> None:
        for name, value in vars(self).items():
            if type(value) is not int:
                raise TypeError(f"{name} must be an integer")
            if not 0 <= value <= (1 << 64) - 1:
                raise ValueError(f"{name} must be between 0 and 2**64 - 1")

    def _limits(self) -> tuple[int, int, int, int, int, int, int, int, int, int, int, int]:
        return (
            self.max_file_bytes,
            self.max_entries,
            self.max_stream_bytes,
            self.max_total_stream_bytes,
            self.max_storage_depth,
            self.max_container_read_bytes,
            self.max_decoded_stream_bytes,
            self.max_string_bytes,
            self.max_preview_pixels,
            self.max_records,
            self.max_record_depth,
            self.max_total_decoded_bytes,
        )


@dataclass(frozen=True, slots=True)
class HeaderInfo:
    """Signature observation, without validating EXB or its CFB contents."""

    signature: Literal["cfb", "legacy_caxa_candidate", "unknown"]
    byte_length: int
    exb_confirmed: bool = False


def capabilities() -> dict[str, object]:
    """Return the implemented scope, suitable for JSON serialization."""
    return {
        "version": __version__,
        "stage": "limited_stored_entity_read",
        "header_probe": True,
        "container_inspection": True,
        "stream_reading": True,
        "stream_decompression": True,
        "metadata_reading": True,
        "preview_extraction": True,
        "record_header_inspection": True,
        "experimental_record_profiles": [
            "schema3-attributes33-experimental",
            "schema3-attributes37-experimental",
        ],
        "experimental_line_inspection": True,
        "experimental_line_profiles": ["cxdbline-81-experimental"],
        "entity_decoding": True,
        "supported_profiles": ["exb-stored-planar-v1"],
        "supported_product_builds": [
            "10.0.0.0000",
            "11.7.0.5203",
            "15.0.0.5681",
            "16.4.0.13104",
            "18.0.0.18159",
        ],
        # Other builds of these majors are read with the same rules plus an
        # ``unverified_product_build`` diagnostic. A platform note after the
        # build (``15.2.0.5869 (x64)``) is ignored.
        "qualified_product_majors": [10, 19],
        "world_coordinate_flattening": True,
        "portable_svg_preview": True,
        "text_style_reading": True,
        "native_display_equivalence": False,
        "text_rendering": False,
        # Files written before the compound-document format (CaxaEbForWindows
        # signature): one flat model space; splines as their sample polylines.
        "legacy_profile": LEGACY_PROFILE,
        "legacy_container_versions": [1050, 1060, 1070],
    }


def probe(path: str | os.PathLike[str], *, options: ParseOptions | None = None) -> HeaderInfo:
    """Inspect at most 17 header bytes of a regular file. Does not confirm EXB."""
    filename = os.fspath(path)
    if not isinstance(filename, str):
        raise TypeError("path must be str or PathLike[str]; use probe_bytes for file contents")
    signature, size = _core.probe_file(filename, (options or ParseOptions()).max_file_bytes)
    return HeaderInfo(signature, size)


def inspect(path: str | os.PathLike[str], *, options: ParseOptions | None = None) -> Inspection:
    """Inspect a directory and EXB markers. Does not validate unread stream contents."""
    filename = os.fspath(path)
    if not isinstance(filename, str):
        raise TypeError("path must be str or PathLike[str]; use inspect_bytes for file contents")
    return _from_json(_core.inspect_file_json(filename, (options or ParseOptions())._limits()))


def inspect_bytes(data: bytes, *, options: ParseOptions | None = None) -> Inspection:
    """Inspect in-memory contents. Malformed CFB raises InvalidContainerError."""
    if not isinstance(data, bytes):
        raise TypeError("data must be bytes")
    return _from_json(_core.inspect_bytes_json(data, (options or ParseOptions())._limits()))


def probe_bytes(data: bytes, *, options: ParseOptions | None = None) -> HeaderInfo:
    """Inspect a bytes object without copying its contents into Rust."""
    if not isinstance(data, bytes):
        raise TypeError("data must be bytes")
    signature, size = _core.probe_bytes(data, (options or ParseOptions()).max_file_bytes)
    return HeaderInfo(signature, size)


def _legacy_document(
    native: Callable[..., str], source: str | bytes, limits: ParseOptions, strict: bool
) -> Document:
    if type(strict) is not bool:
        raise TypeError("strict must be a bool")
    try:
        return _document(native(source, limits._limits(), strict))
    except IncompleteReadError as error:
        error.report = _report(error.report_json)
        raise


def _is_legacy_file(filename: str, limits: ParseOptions) -> bool:
    try:
        return _core.probe_file(filename, limits.max_file_bytes)[0] == "legacy_caxa_candidate"
    except (OSError, ExbError):
        # Let the compound-document path report unreadable or oversized input.
        return False


def read(
    path: str | os.PathLike[str], *, strict: bool = True, options: ParseOptions | None = None
) -> Document:
    """Read the stored-entity profile; strict=False retains explicit partial results.

    A file with the legacy ``CaxaEbForWindows`` signature (CAXA releases before the
    compound-document format) is read with the legacy profile instead; its
    ``Document.profile`` is ``capabilities()["legacy_profile"]``.
    """
    from . import raw

    filename = os.fspath(path)
    limits = options or ParseOptions()
    if isinstance(filename, str) and _is_legacy_file(filename, limits):
        return _legacy_document(_core.read_legacy_file_json, filename, limits, strict)
    with raw.open(path, options=options) as container:
        return container.document(strict=strict)


def metadata(path: str | os.PathLike[str], *, options: ParseOptions | None = None) -> Metadata:
    """Read observed FileInfo/AppInfo fields; unknown fields retain their bytes and spans."""
    from . import raw

    with raw.open(path, options=options) as container:
        return container.metadata()


def metadata_bytes(data: bytes, *, options: ParseOptions | None = None) -> Metadata:
    from . import raw

    with raw.open_bytes(data, options=options) as container:
        return container.metadata()


def preview(path: str | os.PathLike[str], *, options: ParseOptions | None = None) -> PreviewInfo:
    """Describe the stored BMP thumbnail; it is not a render of decoded geometry."""
    from . import raw

    with raw.open(path, options=options) as container:
        return container.preview_info()


def preview_bytes(data: bytes, *, options: ParseOptions | None = None) -> PreviewInfo:
    from . import raw

    with raw.open_bytes(data, options=options) as container:
        return container.preview_info()


def read_bytes(
    data: bytes, *, strict: bool = True, options: ParseOptions | None = None
) -> Document:
    """Read bytes under the same profile and strictness contract as read()."""
    from . import raw

    if isinstance(data, bytes) and data.startswith(LEGACY_SIGNATURE):
        limits = options or ParseOptions()
        return _legacy_document(_core.read_legacy_bytes_json, data, limits, strict)
    with raw.open_bytes(data, options=options) as container:
        return container.document(strict=strict)


def read_world(
    path: str | os.PathLike[str],
    space_id: int,
    *,
    strict: bool = True,
    options: ParseOptions | None = None,
) -> WorldScene:
    """Read and expand a stored space; see each WorldScene completeness field."""
    from . import raw

    with raw.open(path, options=options) as container:
        return container.world(space_id, strict=strict)


def svg_preview(
    path: str | os.PathLike[str],
    space_id: int,
    *,
    strict: bool = True,
    options: ParseOptions | None = None,
) -> SvgPreview:
    """Return an approximate portable SVG and explicit fidelity diagnostics."""
    from . import raw

    with raw.open(path, options=options) as container:
        return container.svg(space_id, strict=strict)
