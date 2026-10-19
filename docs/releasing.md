# Releasing

The package is experimental. Release notes should state supported profiles and
known limitations, and should not describe it as a complete EXB implementation.

## Workflow

[Release](../.github/workflows/release.yml) runs on `v*` tags and manual dispatch.
It verifies the version and locks, runs the reusable CI suite, creates an sdist,
and builds every release wheel from that sdist. Each wheel is installed and
fully tested outside the checkout on CPython 3.10 and 3.14.

The release bundle must contain exactly one sdist and four `cp310-abi3` wheels:

| Platform | Target |
| --- | --- |
| Linux, glibc 2.28+ | x86_64 |
| Windows | x86_64 |
| macOS | x86_64 |
| macOS | arm64 |

The final gate checks metadata, versions, platform tags, wheel RECORD hashes,
required files and excluded assets. It records `manifest.json` and `SHA256SUMS`.
Publishing jobs download this verified bundle from the same workflow run and
recheck its hashes; they do not rebuild it.

Manual dispatch builds and tests only. A matching tag push additionally publishes
to PyPI and then creates a GitHub prerelease with the distributions and checksums.
The workflow does not publish to crates.io. Numeric `0.0.x` versions are not PyPI
prerelease versions; the project classifier and documentation identify the
experimental status.

## One-time publisher setup

Create the GitHub environment `pypi`. Configure a PyPI Trusted Publisher (a
pending publisher before the first release) with these exact values:

| Field | Value |
| --- | --- |
| PyPI project | `ezexb` |
| GitHub owner | `monozukuri-ai` |
| Repository | `ezexb` |
| Workflow filename | `release.yml` |
| Environment | `pypi` |

The workflow grants `id-token: write` only to the PyPI job. It uses OIDC rather
than a stored PyPI API token. See the official
[PyPI publishing guide](https://docs.pypi.org/trusted-publishers/using-a-publisher/)
and [pending publisher setup](https://docs.pypi.org/trusted-publishers/creating-a-project-through-oidc/).
These account settings are separate from the workflow file.

## Prepare a version

1. Update code and documentation, then use
   `uv run --locked python scripts/bump_version.py X.Y.Z` to synchronize versions.
2. Run the [development checks](development.md) and review the changes.
3. Commit and push the intended release commit. Run Release manually from that
   branch and inspect the `verified-distributions` artifact before tagging.
4. Check `uv run --locked python scripts/bump_version.py --check --tag vX.Y.Z`.
5. Create and push the matching tag on the tested commit:

   ```sh
   git tag -a vX.Y.Z -m "Release vX.Y.Z"
   git push origin vX.Y.Z
   ```

A tag push starts publication automatically. Check both PyPI and the GitHub
Release after the workflow completes; a successful build alone is not publication.

## Failed publication

If PyPI reports `invalid-publisher`, check its registered owner, repository,
workflow and environment against the table above. After correcting account
settings, rerun the failed jobs so the already verified artifacts are reused.

If files were partially uploaded, compare their hashes with the verified manifest
before retrying. The workflow intentionally does not skip existing files or
replace an existing version. Do not move an already published tag to new code.
