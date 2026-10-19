"""Check public Markdown links, Python syntax, and private-path exclusions."""

from __future__ import annotations

import ast
import re
from pathlib import Path
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[1]


def check(root: Path = ROOT) -> None:
    paths = [root / "README.md", *sorted((root / "docs").rglob("*.md"))]
    paths += sorted((root / "crates").glob("*/README.md"))
    paths += [root / "tests/python/data/README.md"]
    links = examples = 0
    for path in paths:
        content = path.read_text(encoding="utf-8")
        if re.search(
            r"\.internal(?:/|\\)|/home/|file://|private research|native Viewer reports", content
        ):
            raise ValueError(f"internal information in public document: {path}")
        for language, code in re.findall(r"```([^\n]*)\n(.*?)```", content, re.S):
            if language.strip() == "python":
                ast.parse(code, filename=str(path))
                examples += 1
        for target in re.findall(r"\[[^\]]*\]\(([^\s)]+)\)", content):
            url = urlsplit(target)
            if url.scheme or url.netloc:
                continue
            destination = (path.parent / unquote(url.path)).resolve() if url.path else path
            if not destination.is_relative_to(root.resolve()) or not destination.exists():
                raise ValueError(f"broken or external local link: {path}: {target}")
            if url.fragment and destination.suffix == ".md":
                headings = re.findall(r"^#+ (.+)$", destination.read_text(encoding="utf-8"), re.M)
                anchors = {
                    re.sub(r"[^\w\- ]", "", heading.lower()).replace(" ", "-")
                    for heading in headings
                }
                if unquote(url.fragment) not in anchors:
                    raise ValueError(f"missing heading: {path}: {target}")
            links += 1
    print(f"Public docs: {len(paths)} files, {links} local links, {examples} Python examples")


if __name__ == "__main__":
    check()
