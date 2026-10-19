"""Header, container and stream inspection commands."""

from __future__ import annotations

import argparse
import json
import sys
from collections.abc import Sequence
from dataclasses import asdict, fields
from pathlib import Path

from . import (
    ExbError,
    IncompleteReadError,
    ParseOptions,
    __version__,
    capabilities,
    inspect,
    probe,
    raw,
)


def _add_input(parser: argparse.ArgumentParser, *, container: bool = True) -> None:
    parser.add_argument("path")
    parser.add_argument("--json", action="store_true")
    defaults = ParseOptions()
    for field in fields(defaults):
        if container or field.name == "max_file_bytes":
            parser.add_argument(
                "--" + field.name.replace("_", "-"), type=int, default=getattr(defaults, field.name)
            )


def _hex_dump(data: bytes, offset: int) -> str:
    rows = []
    for start in range(0, len(data), 16):
        chunk = data[start : start + 16]
        text = "".join(chr(value) if 32 <= value < 127 else "." for value in chunk)
        rows.append(f"{offset + start:08x}  {chunk.hex(' '):47s}  |{text}|")
    return "\n".join(rows)


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="EXB stream analysis and experimental LINE inspection"
    )
    parser.add_argument("--version", action="version", version=f"ezexb {__version__}")
    commands = parser.add_subparsers(dest="command", required=True)
    status = commands.add_parser("capabilities", help="show the implemented scope")
    status.add_argument("--json", action="store_true")
    _add_input(commands.add_parser("probe", help="inspect only the header"), container=False)
    _add_input(commands.add_parser("inspect", help="inspect CFB directory and EXB markers"))
    _add_input(commands.add_parser("metadata", help="read observed FileInfo and AppInfo fields"))
    preview = commands.add_parser("preview", help="describe or extract the stored BMP thumbnail")
    _add_input(preview)
    preview.add_argument("--output", type=Path, help="write the original BMP to a new file")
    records = commands.add_parser(
        "records", help="inspect records and nested display caches; stop at unknown structures"
    )
    _add_input(records)
    records.add_argument("stream_path")
    reader = commands.add_parser("read", help="read the limited stored-entity profile")
    _add_input(reader)
    reader.add_argument(
        "--partial",
        action="store_true",
        help="return decoded entities with explicit incompleteness diagnostics",
    )
    for command in ("world", "svg"):
        scene = commands.add_parser(
            command,
            help="expand a space"
            if command == "world"
            else "portable SVG preview with approximate text and styles",
        )
        _add_input(scene)
        scene.add_argument("--space", type=int, required=True, help="stored space ID")
        scene.add_argument(
            "--partial", action="store_true", help="allow incomplete source documents"
        )
        if command == "svg":
            scene.add_argument("--output", type=Path, required=True, help="write a new SVG file")
    lines = commands.add_parser(
        "lines", help="inspect experimental LINE coordinates and raw attributes"
    )
    _add_input(lines)
    lines.add_argument("stream_path")
    payload = commands.add_parser(
        "payload", help="decode a stream envelope and inspect or extract its payload"
    )
    _add_input(payload)
    payload.add_argument("stream_path")
    payload_destination = payload.add_mutually_exclusive_group()
    payload_destination.add_argument("--output", type=Path)
    payload_destination.add_argument(
        "--hex", action="store_true", help="show at most 256 decoded bytes"
    )
    streams = commands.add_parser("streams", help="list streams; optionally hash all contents")
    _add_input(streams)
    streams.add_argument("--sha256", action="store_true", help="read and hash every full stream")
    stream = commands.add_parser("stream", help="read one stream or range")
    _add_input(stream)
    stream.add_argument("stream_path", help="exact absolute CFB path from streams")
    destination = stream.add_mutually_exclusive_group(required=True)
    destination.add_argument(
        "--hex", action="store_true", help="hex dump (default: at most 256 bytes)"
    )
    destination.add_argument(
        "--output", type=Path, help="write bytes to a new file; never overwrite"
    )
    stream.add_argument("--offset", type=int, default=0)
    stream.add_argument("--length", type=int)
    args = parser.parse_args(argv)
    try:
        options = ParseOptions(
            **{
                field.name: getattr(args, field.name)
                for field in fields(ParseOptions)
                if hasattr(args, field.name)
            }
        )
        if args.command == "capabilities":
            result = capabilities()
        elif args.command == "probe":
            result = asdict(probe(args.path, options=options))
        elif args.command == "inspect":
            result = asdict(inspect(args.path, options=options))
        else:
            with raw.open(args.path, options=options) as container:
                if args.command == "read":
                    result = asdict(container.document(strict=not args.partial))
                elif args.command == "world":
                    result = asdict(container.world(args.space, strict=not args.partial))
                elif args.command == "svg":
                    result = asdict(container.svg(args.space, strict=not args.partial))
                    svg = result.pop("svg")
                    with args.output.open("x", encoding="utf-8") as destination_file:
                        destination_file.write(svg)
                    result["output"] = str(args.output)
                elif args.command == "metadata":
                    result = asdict(container.metadata())
                elif args.command == "preview":
                    result = asdict(container.preview_info())
                    if args.output:
                        data = container.preview_bmp()
                        with args.output.open("xb") as destination_file:
                            destination_file.write(data)
                        result["output"] = str(args.output)
                elif args.command == "records":
                    result = asdict(container.record_headers(args.stream_path))
                elif args.command == "lines":
                    result = asdict(container.lines(args.stream_path))
                elif args.command == "payload":
                    decoded = container.stream_payload(args.stream_path)
                    result = asdict(decoded.info)
                    if args.output:
                        with args.output.open("xb") as destination_file:
                            destination_file.write(decoded.data)
                        result["output"] = str(args.output)
                    elif args.hex:
                        if args.json:
                            result["prefix_hex"] = decoded.data[:256].hex()
                        else:
                            print(_hex_dump(decoded.data[:256], 0))
                            return 0
                elif args.command == "streams":
                    result = []
                    for entry in container.streams:
                        row = asdict(entry)
                        if args.sha256:
                            row["sha256"] = container.stream_sha256(entry.path)
                        result.append(row)
                else:
                    length = args.length
                    if args.hex and length is None:
                        entry = next(
                            (e for e in container.streams if e.path == args.stream_path), None
                        )
                        if entry is not None:
                            length = min(256, max(0, entry.size_bytes - args.offset))
                    data = container.read_stream(
                        args.stream_path, offset=args.offset, length=length
                    )
                    if args.hex:
                        if args.json:
                            print(
                                json.dumps(
                                    {
                                        "path": args.stream_path,
                                        "offset": args.offset,
                                        "size_bytes": len(data),
                                        "hex": data.hex(),
                                    }
                                )
                            )
                        else:
                            print(_hex_dump(data, args.offset))
                        return 0
                    with args.output.open("xb") as destination_file:
                        destination_file.write(data)
                    result = {"output": str(args.output), "size_bytes": len(data)}
    except IncompleteReadError as exc:
        if args.json:
            print(
                json.dumps(
                    {"error": "incomplete_read", "report": asdict(exc.report)},
                    ensure_ascii=True,
                    sort_keys=True,
                )
            )
        else:
            print(f"ezexb: {exc}", file=sys.stderr)
        return 2
    except (ExbError, OSError, ValueError, OverflowError) as exc:
        print(f"ezexb: {exc}", file=sys.stderr)
        return 1
    print(json.dumps(result, ensure_ascii=True, sort_keys=True, indent=None if args.json else 2))
    return 0
