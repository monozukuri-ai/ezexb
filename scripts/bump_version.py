"""Update the authoritative Cargo version and synchronize Cargo/uv locks."""

from __future__ import annotations

import argparse
import re
import subprocess
from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:
    import tomli as tomllib

ROOT = Path(__file__).resolve().parents[1]


def read_toml(path: Path) -> dict:
    return tomllib.loads(path.read_text(encoding="utf-8"))


def check(root: Path, tag: str | None = None) -> str:
    workspace = read_toml(root / "Cargo.toml")["workspace"]
    version = workspace["package"]["version"]
    if not re.fullmatch(r"(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)", version):
        raise ValueError("only stable X.Y.Z versions are supported")
    project = read_toml(root / "pyproject.toml")["project"]
    if "version" in project or "version" not in project.get("dynamic", []):
        raise ValueError("Python version must be dynamic")
    locked = read_toml(root / "Cargo.lock")["package"]
    for member in workspace["members"]:
        package = read_toml(root / member / "Cargo.toml")["package"]
        if package["version"] != {"workspace": True}:
            raise ValueError(f"{member} must inherit the workspace version")
        if [p["version"] for p in locked if p["name"] == package["name"]] != [version]:
            raise ValueError(f"outdated Cargo.lock entry: {package['name']}")
    local = [p for p in read_toml(root / "uv.lock")["package"] if p["name"] == "ezexb"]
    if len(local) != 1 or local[0].get("source") != {"editable": "."}:
        raise ValueError("uv.lock must contain the local ezexb package")
    if local[0].get("version", version) != version:
        raise ValueError("uv.lock contains an outdated version")
    if tag is not None and tag != f"v{version}":
        raise ValueError(f"expected tag v{version}, got {tag}")
    return version


def bump(root: Path, version: str) -> None:
    if not re.fullmatch(r"(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)", version):
        raise ValueError("version must be X.Y.Z")
    paths = [root / name for name in ("Cargo.toml", "Cargo.lock", "uv.lock")]
    backups = {path: path.read_bytes() for path in paths}
    text, count = re.subn(
        r'(?ms)(^\[workspace\.package\]\n(?:(?!^\[).)*?^version\s*=\s*")[^"]+("[^\n]*)',
        lambda match: f"{match[1]}{version}{match[2]}",
        backups[paths[0]].decode(),
    )
    if count != 1:
        raise ValueError("expected one workspace version")
    try:
        paths[0].write_text(text, encoding="utf-8")
        subprocess.run(["cargo", "update", "--workspace"], cwd=root, check=True)
        subprocess.run(["uv", "lock"], cwd=root, check=True)
        check(root)
    except BaseException:
        for path, data in backups.items():
            path.write_bytes(data)
        raise


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    action = parser.add_mutually_exclusive_group(required=True)
    action.add_argument("version", nargs="?")
    action.add_argument("--check", action="store_true")
    parser.add_argument("--tag")
    args = parser.parse_args()
    if args.tag and not args.check:
        parser.error("--tag requires --check")
    if args.check:
        version = check(ROOT, args.tag)
        subprocess.run(
            ["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"],
            cwd=ROOT,
            check=True,
            stdout=subprocess.DEVNULL,
        )
        subprocess.run(["uv", "lock", "--check"], cwd=ROOT, check=True)
        print(f"Versions and locks are consistent: {version}")
    else:
        bump(ROOT, args.version)
        print(f"Updated to {args.version}; review Cargo.toml, Cargo.lock and uv.lock")


if __name__ == "__main__":
    main()
