"""Reject damaged wheels before upload, including Windows RECORD paths."""

import base64
import csv
import hashlib
import importlib.util
import io
import sys
import zipfile
from pathlib import Path

import pytest

SCRIPTS = Path(__file__).resolve().parents[2] / "scripts"
sys.path.insert(0, str(SCRIPTS))
spec = importlib.util.spec_from_file_location("check_artifacts", SCRIPTS / "check_artifacts.py")
assert spec and spec.loader
artifacts = importlib.util.module_from_spec(spec)
spec.loader.exec_module(artifacts)
sys.path.pop(0)


def make_wheel(*, damage=None, windows=False):
    payload = b"original payload"
    name = "ezexb/__init__.py"
    record = "ezexb-0.0.1.dist-info/RECORD"
    digest = base64.urlsafe_b64encode(hashlib.sha256(payload).digest()).rstrip(b"=").decode()
    rows = [[name, f"sha256={digest}", str(len(payload))], [record, "", ""]]
    if damage == "duplicate":
        rows.append(rows[0])
    if windows:
        for row in rows:
            row[0] = row[0].replace("/", "\\")
    buffer = io.StringIO()
    csv.writer(buffer).writerows(rows)
    output = io.BytesIO()
    with zipfile.ZipFile(output, "w") as archive:
        archive.writestr(name, b"modified payload" if damage == "modified" else payload)
        archive.writestr(record, buffer.getvalue())
        if damage == "extra":
            archive.writestr("ezexb/unrecorded.py", b"unexpected")
    output.seek(0)
    return output


@pytest.mark.parametrize("windows", [False, True])
def test_wheel_record_integrity_and_portable_paths(windows):
    with zipfile.ZipFile(make_wheel(windows=windows)) as archive:
        artifacts.check_record(archive, archive.namelist())


@pytest.mark.parametrize("damage", ["modified", "duplicate", "extra"])
def test_wheel_rejects_modified_duplicate_and_unrecorded_members(damage):
    with zipfile.ZipFile(make_wheel(damage=damage)) as archive:
        with pytest.raises(ValueError, match="RECORD|unrecorded"):
            artifacts.check_record(archive, archive.namelist())


def test_metadata_rejects_a_different_version():
    with pytest.raises(ValueError, match="Version"):
        artifacts.check_metadata(b"Name: ezexb\nVersion: 999.0.0\n", "0.0.1")


def test_release_rejects_incomplete_platform_set(tmp_path):
    version = artifacts.check_version(artifacts.ROOT)
    (tmp_path / f"ezexb-{version}.tar.gz").touch()
    (tmp_path / f"ezexb-{version}-cp310-abi3-manylinux_2_28_x86_64.whl").touch()
    with pytest.raises(ValueError, match="release requires"):
        artifacts.check(tmp_path, release=True)
