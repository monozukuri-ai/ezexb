"""Check wheel/sdist payloads and print an integrity manifest as JSON."""

from __future__ import annotations

import argparse
import base64
import csv
import hashlib
import io
import json
import re
import tarfile
import zipfile
from email.parser import BytesParser
from pathlib import Path, PurePosixPath

from bump_version import ROOT
from bump_version import check as check_version


def check_metadata(data: bytes, version: str) -> None:
    metadata = BytesParser().parsebytes(data)
    for key, expected in (("Name", "ezexb"), ("Version", version), ("Requires-Python", ">=3.10")):
        if metadata[key] != expected:
            raise ValueError(f"metadata {key}: expected {expected!r}, got {metadata[key]!r}")
    if metadata["License-Expression"] != "MIT":
        raise ValueError("MIT license expression missing from metadata")
    description = metadata.get_payload().strip().replace("\r\n", "\n")
    if description != (ROOT / "README.md").read_text(encoding="utf-8").strip():
        raise ValueError("distribution description differs from README.md")


def check_record(archive: zipfile.ZipFile, names: list[str]) -> None:
    records = [name for name in names if name.endswith(".dist-info/RECORD")]
    if len(records) != 1:
        raise ValueError("expected one wheel RECORD")
    record = records[0]
    seen = set()
    rows = csv.reader(io.StringIO(archive.read(record).decode("utf-8")))
    for row in rows:
        if len(row) != 3:
            raise ValueError("invalid RECORD row")
        name, digest, size = row
        # Some Windows builders use backslashes in RECORD, but ZIP paths use '/'.
        name = name.replace("\\", "/")
        if name in seen or name not in names:
            raise ValueError(f"duplicate or missing RECORD member: {name}")
        seen.add(name)
        if name == record:
            if digest or size:
                raise ValueError("RECORD must not hash itself")
            continue
        data = archive.read(name)
        expected = base64.urlsafe_b64encode(hashlib.sha256(data).digest()).rstrip(b"=").decode()
        if digest != f"sha256={expected}" or size != str(len(data)):
            raise ValueError(f"RECORD integrity mismatch: {name}")
    if seen != set(names):
        raise ValueError("wheel has unrecorded members")


def check(directory: Path, *, release: bool = False, tag: str | None = None) -> list[dict]:
    version = check_version(ROOT, tag)
    wheels = sorted(directory.glob("*.whl"))
    sources = sorted(directory.glob("*.tar.gz"))
    if not wheels or len(sources) != 1:
        raise ValueError("one or more wheels and exactly one source distribution are required")
    if sources[0].name != f"ezexb-{version}.tar.gz":
        raise ValueError("source filename does not match the workspace version")
    if release:
        patterns = (
            r"manylinux_2_28_x86_64",
            r"macosx_\d+_\d+_x86_64",
            r"macosx_\d+_\d+_arm64",
            r"win_amd64",
        )
        if len(wheels) != len(patterns) or any(
            sum(
                bool(re.fullmatch(rf"ezexb-{re.escape(version)}-cp310-abi3-{p}\.whl", w.name))
                for w in wheels
            )
            != 1
            for p in patterns
        ):
            raise ValueError("release requires Linux x86_64, macOS x86_64/arm64, Windows x86_64")
        if set(directory.iterdir()) != set(wheels + sources):
            raise ValueError("release distribution directory contains unexpected files")
    result = []
    for path in wheels + sources:
        if path.suffix == ".whl":
            with zipfile.ZipFile(path) as archive:
                names = archive.namelist()
                if len(names) != len(set(names)):
                    raise ValueError("duplicate wheel members")
                metadata = [name for name in names if name.endswith(".dist-info/METADATA")]
                if metadata != [f"ezexb-{version}.dist-info/METADATA"]:
                    raise ValueError("unexpected wheel metadata directory")
                check_metadata(archive.read(metadata[0]), version)
                check_record(archive, names)
                wheel = BytesParser().parsebytes(archive.read(f"ezexb-{version}.dist-info/WHEEL"))
                platform = path.name.removesuffix(".whl").split("-")[-1]
                if set(wheel.get_all("Tag", [])) != {
                    f"cp310-abi3-{item}" for item in platform.split(".")
                }:
                    raise ValueError("wheel filename and WHEEL tags differ")
            required = [
                "ezexb/__init__.py",
                "ezexb/_core.pyi",
                "ezexb/py.typed",
                "ezexb/raw.py",
                "ezexb/inspection.py",
                "ezexb/content.py",
                "ezexb/document.py",
                "ezexb/world.py",
                "ezexb/complex.py",
            ]
            if not any(n.startswith("ezexb/_core") and n.endswith((".so", ".pyd")) for n in names):
                raise ValueError(f"native extension is missing from {path}")
            if not path.name.startswith(f"ezexb-{version}-cp310-abi3-"):
                raise ValueError(f"expected a CPython 3.10 abi3 wheel: {path}")
        else:
            with tarfile.open(path) as archive:
                names = archive.getnames()
                if len(names) != len(set(names)) or any(
                    not (member.isfile() or member.isdir()) for member in archive.getmembers()
                ):
                    raise ValueError("source archive contains duplicate members or special files")
                if any(PurePosixPath(name).parts[0] != f"ezexb-{version}" for name in names):
                    raise ValueError("unexpected source archive root")
                metadata = archive.extractfile(f"ezexb-{version}/PKG-INFO")
                if metadata is None:
                    raise ValueError("source metadata missing")
                check_metadata(metadata.read(), version)
            names = ["/".join(PurePosixPath(n).parts[1:]) for n in names]
            required = [
                "pyproject.toml",
                "Cargo.toml",
                "Cargo.lock",
                "uv.lock",
                "crates/ezexb-core/src/lib.rs",
                "crates/ezexb-core/src/container.rs",
                "crates/ezexb-core/examples/inspect.rs",
                "crates/ezexb-python/src/lib.rs",
                "src/ezexb/__init__.py",
                "tests/python/test_install.py",
                "tests/python/test_container.py",
                "tests/python/data/synthetic.cfb",
                "tests/python/data/analysis.cfb",
                "tests/python/test_content.py",
                "tests/python/test_schema3.py",
                "tests/python/data/schema3-33.cfb",
                "tests/python/data/schema3-37.cfb",
                "tests/python/data/document-10.cfb",
                "tests/python/data/document-18.cfb",
                "tests/python/data/document-partial.cfb",
                "crates/ezexb-core/examples/make_schema3_fixture.rs",
                "crates/ezexb-core/src/compression.rs",
                "crates/ezexb-core/src/metadata.rs",
                "crates/ezexb-core/src/preview.rs",
                "crates/ezexb-core/src/records.rs",
                "crates/ezexb-core/src/lines.rs",
                "docs/inspection.md",
                "docs/reading.md",
                "docs/README.md",
                "docs/releasing.md",
                "scripts/check_docs.py",
                "scripts/check_artifacts.py",
                "scripts/verify_wheel.py",
                "src/ezexb/document.py",
                "crates/ezexb-core/src/document.rs",
                "crates/ezexb-core/src/document_tests.rs",
                "crates/ezexb-core/src/geometry.rs",
                "crates/ezexb-core/src/annotation.rs",
                "crates/ezexb-core/src/context.rs",
                "crates/ezexb-core/examples/read.rs",
                "crates/ezexb-core/examples/support/document_fixture.rs",
                "tests/python/test_document.py",
                "tests/python/test_world.py",
                "src/ezexb/world.py",
                "src/ezexb/complex.py",
                "crates/ezexb-core/src/world.rs",
                "crates/ezexb-core/src/complex.rs",
                "crates/ezexb-core/src/attributes.rs",
                "crates/ezexb-core/src/text_style.rs",
                "crates/ezexb-core/src/render.rs",
                "crates/ezexb-core/examples/world.rs",
            ]
        for name in names:
            parts = PurePosixPath(name).parts
            if (
                PurePosixPath(name).is_absolute()
                or "\\" in name
                or any(
                    p in {".internal", ".local", ".venv", ".git", "__pycache__", ".."}
                    for p in parts
                )
            ):
                raise ValueError(f"private/generated path in {path}: {name}")
            if name.lower().endswith((".exb", ".tpl", ".pdf", ".pyc", ".exe", ".dll", ".msi")):
                raise ValueError(f"unexpected research/generated asset in {path}: {name}")
            if name.lower().endswith(".cfb") and name not in {
                "tests/python/data/synthetic.cfb",
                "tests/python/data/analysis.cfb",
                "tests/python/data/schema3-33.cfb",
                "tests/python/data/schema3-37.cfb",
                "tests/python/data/document-10.cfb",
                "tests/python/data/document-18.cfb",
                "tests/python/data/document-partial.cfb",
            }:
                raise ValueError(f"unexpected CFB asset in {path}: {name}")
        if not any(PurePosixPath(n).name == "LICENSE" for n in names):
            raise ValueError(f"license missing from {path}")
        for expected in required:
            if expected not in names:
                raise ValueError(f"{expected} missing from {path}")
        result.append(
            {
                "file": path.name,
                "size_bytes": path.stat().st_size,
                "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                "entries": len(names),
            }
        )
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--release", action="store_true", help="require the full release wheel set")
    parser.add_argument("--tag")
    parser.add_argument("--write-manifest", type=Path)
    args = parser.parse_args()
    result = check(args.directory, release=args.release, tag=args.tag)
    manifest = json.dumps(result, indent=2) + "\n"
    if args.write_manifest:
        args.write_manifest.parent.mkdir(parents=True, exist_ok=True)
        args.write_manifest.write_text(manifest, encoding="utf-8")
        sums = "".join(f"{item['sha256']}  dist/{item['file']}\n" for item in result)
        args.write_manifest.with_name("SHA256SUMS").write_text(sums, encoding="utf-8")
    print(manifest, end="")
