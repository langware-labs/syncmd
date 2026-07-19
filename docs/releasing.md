# Releasing syncmd

`syncmd` ships in three user-facing forms from one release tag:

* Cargo: `cargo install syncmd`

* PyPI: `pip install syncmd`

* npm: `npm install -g syncmd`

## What the release workflow publishes

Tagging `vX.Y.Z` runs `.github/workflows/release.yml`, which:

1. builds precompiled CLI archives for:

   * `x86_64-unknown-linux-gnu`

   * `aarch64-unknown-linux-gnu`

   * `x86_64-apple-darwin`

   * `aarch64-apple-darwin`

   * `x86_64-pc-windows-msvc`
2. attaches those archives to the matching GitHub Release
3. publishes `syncmd-core` then `syncmd` to crates.io
4. builds and uploads Python wheels for CPython 3.9-3.13
5. publishes the npm wrapper package in `npm/syncmd`

The npm package downloads the matching release asset on install, so GitHub Release assets must
exist before `npm publish`.

## Required secrets

* `CARGO_REGISTRY_TOKEN`

* `PYPI_API_TOKEN`

* `NPM_TOKEN`

## Preflight

Run this before tagging:

```bash
./scripts/release-check.sh
```

It verifies:

* version alignment across Cargo, PyPI, and npm

* `cargo check` for the CLI and Python binding crates

* npm wrapper script syntax

* `npm pack --dry-run` for the published npm package

## Versioning notes

* Keep `Cargo.toml` workspace version, `crates/syncmd-py/pyproject.toml`, and
  `npm/syncmd/package.json` aligned.

* Create the Git tag as `v<version>`.
