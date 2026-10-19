"""Read-only CFB snapshots, stream analysis, and experimental LINE inspection."""

from __future__ import annotations

import os

from . import IncompleteReadError, ParseOptions, _core
from .content import (
    AppInfo,
    DecodedStream,
    FileInfo,
    LineInspection,
    Metadata,
    PreviewInfo,
    RecordInspection,
    _app_info,
    _file_info,
    _lines,
    _payload,
    _preview_info,
    _records,
)
from .document import Document, _document, _report
from .inspection import EntryInfo, Inspection, _from_json
from .world import SvgPreview, WorldScene, _svg, _world


class Container:
    """An owned snapshot. Use as a context manager to release it promptly."""

    def __init__(self, native: _core.Container) -> None:
        self._native = native

    @property
    def info(self) -> Inspection:
        return _from_json(self._native.info_json())

    @property
    def streams(self) -> tuple[EntryInfo, ...]:
        return self.info.streams

    def read_stream(self, path: str, *, offset: int = 0, length: int | None = None) -> bytes:
        """Read the selected range. Path must exactly match a listed CFB path."""
        for name, value in (("offset", offset), ("length", length)):
            if value is None and name == "length":
                continue
            if type(value) is not int:
                raise TypeError(f"{name} must be an integer")
            if not 0 <= value <= (1 << 64) - 1:
                raise ValueError(f"{name} must be between 0 and 2**64 - 1")
        return self._native.read_stream(path, offset, length)

    def stream_sha256(self, path: str) -> str:
        """Hash a full stream and detect a truncated chain while reading it."""
        return self._native.stream_sha256(path)

    def file_info(self) -> FileInfo:
        return _file_info(self._native.file_info_json())

    def app_info(self) -> AppInfo:
        return _app_info(self._native.app_info_json())

    def metadata(self) -> Metadata:
        return Metadata(file_info=self.file_info(), app_info=self.app_info())

    def preview_info(self) -> PreviewInfo:
        return _preview_info(self._native.preview_info_json())

    def preview_bmp(self) -> bytes:
        """Return the validated, original embedded BMP without re-encoding."""
        return self._native.preview_bmp()

    def stream_payload(self, path: str) -> DecodedStream:
        """Decode every zlib block and validate allocation padding before trimming it."""
        return _payload(*self._native.stream_payload(path))

    def record_headers(self, path: str) -> RecordInspection:
        """Frame records and nested display caches using an observed build profile.

        Unknown structures stop before the enclosing top-level record. All spans
        remain relative to the decoded stream. Check framing_complete explicitly.
        """
        return _records(self._native.record_headers_json(path))

    def lines(self, path: str) -> LineInspection:
        """Inspect top-level LINE coordinates; cached display children are not included."""
        return _lines(self._native.lines_json(path))

    def document(self, *, strict: bool = True) -> Document:
        """Read stored entities; incomplete strict reads raise with a report attribute."""
        if type(strict) is not bool:
            raise TypeError("strict must be a bool")
        try:
            return _document(self._native.document_json(strict))
        except IncompleteReadError as error:
            error.report = _report(error.report_json)
            raise

    def world(self, space_id: int, *, strict: bool = True) -> WorldScene:
        """Expand an explicitly selected space; completeness is reported by domain."""
        return self._scene(space_id, strict, False)

    def svg(self, space_id: int, *, strict: bool = True) -> SvgPreview:
        """Portable world-XY preview with approximate text; not native display fidelity."""
        return self._scene(space_id, strict, True)

    def _scene(self, space_id: int, strict: bool, svg: bool):
        if type(space_id) is not int:
            raise TypeError("space_id must be an integer")
        if not -(1 << 31) <= space_id < (1 << 31):
            raise ValueError("space_id must fit in a signed 32-bit integer")
        if type(strict) is not bool:
            raise TypeError("strict must be a bool")
        try:
            if svg:
                return _svg(self._native.svg_json(space_id, strict))
            return _world(self._native.world_json(space_id, strict))
        except IncompleteReadError as error:
            error.report = _report(error.report_json)
            raise

    def close(self) -> None:
        self._native.close()

    def __enter__(self) -> Container:
        return self

    def __exit__(self, *_: object) -> None:
        self.close()


def open(path: str | os.PathLike[str], *, options: ParseOptions | None = None) -> Container:
    """Open a regular file as a read-only snapshot; later file changes are invisible."""
    filename = os.fspath(path)
    if not isinstance(filename, str):
        raise TypeError("path must be str or PathLike[str]; use open_bytes for file contents")
    return Container(_core.Container.open_file(filename, (options or ParseOptions())._limits()))


def open_bytes(data: bytes, *, options: ParseOptions | None = None) -> Container:
    """Own a copy of the supplied bytes. Non-CFB inputs are unsupported."""
    if not isinstance(data, bytes):
        raise TypeError("data must be bytes")
    return Container(_core.Container.open_bytes(data, (options or ParseOptions())._limits()))
