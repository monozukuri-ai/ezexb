"""Install one platform wheel and test it outside the checkout using uv."""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def verify(directory: Path, python: str) -> None:
    wheels = list(directory.resolve().glob("*.whl"))
    if len(wheels) != 1:
        raise ValueError("provide a directory containing exactly one platform wheel")
    with tempfile.TemporaryDirectory(prefix="ezexb-wheel-") as temporary:
        work = Path(temporary)
        environment = work / "venv"
        subprocess.run(["uv", "venv", "--python", python, str(environment)], check=True)
        bin_dir = environment / ("Scripts" if os.name == "nt" else "bin")
        interpreter = bin_dir / ("python.exe" if os.name == "nt" else "python")
        subprocess.run(
            ["uv", "pip", "install", "--python", str(interpreter), str(wheels[0]), "pytest>=8,<10"],
            check=True,
        )
        shutil.copytree(
            ROOT / "tests/python", work / "tests", ignore=shutil.ignore_patterns("__pycache__")
        )
        env = os.environ.copy()
        env.pop("PYTHONPATH", None)
        env.pop("PYTHONHOME", None)
        env["PATH"] = str(bin_dir) + os.pathsep + env.get("PATH", "")
        subprocess.run(
            [
                str(interpreter),
                "-I",
                "-c",
                "import pathlib,sys,ezexb,ezexb._core; "
                "root=pathlib.Path(sys.prefix).resolve(); "
                "assert pathlib.Path(ezexb.__file__).resolve().is_relative_to(root); "
                "assert pathlib.Path(ezexb._core.__file__).resolve().is_relative_to(root); "
                "print('Installed native module:',ezexb._core.__file__)",
            ],
            cwd=work,
            env=env,
            check=True,
        )
        subprocess.run(
            [str(interpreter), "-I", "-m", "pytest", "-q", "tests"],
            cwd=work,
            env=env,
            check=True,
        )


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--python", required=True)
    args = parser.parse_args()
    verify(args.directory, args.python)
