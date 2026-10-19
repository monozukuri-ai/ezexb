# Development

The workspace contains `ezexb-core` (pure Rust) and `ezexb-python` (PyO3 bindings).
The Python API and CLI live in `src/ezexb`. Python has no runtime dependencies
beyond the packaged native extension.

## Set up

Install Rust 1.83+, GIL-enabled CPython 3.10+, and [uv](https://docs.astral.sh/uv/).
From the repository root:

```sh
uv sync --locked
uv run ezexb capabilities --json
```

`uv run` also makes the console entry point available to subprocesses in tests.
After editing Rust, run `uv sync --locked` to rebuild the editable extension.

## Run checks

```sh
cargo fmt --all -- --check
cargo test --locked -p ezexb-core
cargo clippy --locked --workspace --all-targets -- -D warnings
uv run --locked ruff check .
uv run --locked ruff format --check .
uv run --locked pytest
uv run --locked python scripts/check_docs.py
uv run --locked python scripts/bump_version.py --check
```

CI runs these checks on Linux, Windows and macOS, using CPython 3.10 and 3.14.
A separate job checks Rust 1.83. Workflow syntax is checked with
[actionlint](https://github.com/rhysd/actionlint).

## Build and test distributions

Use a fresh output directory for each build:

```sh
uv build --out-dir dist/check
uv run --locked python scripts/check_artifacts.py dist/check
uv run --locked python scripts/verify_wheel.py dist/check --python 3.10
uv run --locked python scripts/verify_wheel.py dist/check --python 3.14
```

`check_artifacts.py` checks metadata, required files, wheel RECORD integrity,
and distribution exclusions. `verify_wheel.py` installs a wheel into a temporary
environment, confirms the native module is loaded from that environment, and runs
the Python tests outside the checkout. CI also rebuilds a wheel from the sdist.

All committed fixtures are original synthetic inputs. Their generators and
provenance are described in [the fixture README](../tests/python/data/README.md).
Tests and packages must work without vendor software or external sample files.

## API conventions

- Keep unsupported data visible through structured diagnostics and source spans.
- Strict reads reject incomplete required content. Partial reads never bypass
  input, decompression, record-count or recursion budgets.
- Stored block coordinates and expanded world coordinates are separate APIs.
- Container marker matches identify candidates; they do not certify EXB support.
- Display caches and attached objects remain separate from ordinary entities.

See [reading](reading.md) for the public contract and [inspection](inspection.md)
for container limits.

## Version maintenance

`[workspace.package].version` in the root `Cargo.toml` is authoritative. Both
crates inherit it and maturin supplies Python's dynamic version.

```sh
uv run --locked python scripts/bump_version.py X.Y.Z
uv run --locked python scripts/bump_version.py --check --tag vX.Y.Z
```

The helper accepts numeric `X.Y.Z` versions and updates Cargo/uv locks together.
The package remains experimental regardless of the numeric version. See
[releasing](releasing.md) for release gates and publication setup.
