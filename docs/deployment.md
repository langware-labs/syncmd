# Deployment

This document records the current deployment state for `syncmd` as of July 19, 2026 and the
exact steps needed to publish it.

## Current state

- GitHub repo: `https://github.com/langware-labs/syncmd`
- Default branch: `main`
- Visibility: public
- GitHub Actions workflow present: `release`
- Repo secrets configured: none yet

## What is already working

### GitHub

- Local git repository initialized
- Commits created and pushed to `langware-labs/syncmd`
- Release workflow checked in at `.github/workflows/release.yml`
- Workflow is visible to GitHub Actions

### Cargo

- `syncmd-core` passes `cargo publish --dry-run`
- `syncmd-cli` is wired for publish, but `cargo publish --dry-run -p syncmd-cli` fails before the
  first release because it depends on `syncmd-core`, which is not yet present on crates.io
- This is expected for the initial publish; the workflow publishes `syncmd-core` first, waits,
  then publishes `syncmd-cli`

### npm

- `npm pack --dry-run ./npm/syncmd` passes
- `npm view syncmd version` returned `404`, so the package name appears available as of
  July 19, 2026
- The npm package is a wrapper that downloads the GitHub Release binary for the current platform

### PyPI

- `crates/syncmd-py/pyproject.toml` is set up for `maturin`
- `https://pypi.org/pypi/syncmd/json` returned `404`, so the package name appears available as of
  July 19, 2026
- Local wheel build was not executed in this shell because `maturin` is not installed here

## Required GitHub secrets

Set these on the GitHub repo before creating the first release tag:

- `CARGO_REGISTRY_TOKEN`
- `PYPI_API_TOKEN`
- `NPM_TOKEN`

You can set them with `gh` once you have the token values:

```bash
gh secret set CARGO_REGISTRY_TOKEN --repo langware-labs/syncmd
gh secret set PYPI_API_TOKEN --repo langware-labs/syncmd
gh secret set NPM_TOKEN --repo langware-labs/syncmd
```

## Preflight

Run this before tagging:

```bash
./scripts/release-check.sh
```

Current checks performed by that script:

- version alignment across Cargo, PyPI, and npm
- `cargo check -p syncmd-cli`
- `cargo check -p syncmd-py`
- npm wrapper script syntax checks
- `npm pack --dry-run ./npm/syncmd`

## First release

1. Confirm the version in:
   - `Cargo.toml`
   - `crates/syncmd-py/pyproject.toml`
   - `npm/syncmd/package.json`
2. Add the three GitHub secrets.
3. Tag and push:

```bash
git tag v0.1.0
git push origin v0.1.0
```

4. Watch the workflow:

```bash
gh run list --repo langware-labs/syncmd --workflow release
gh run watch --repo langware-labs/syncmd
```

## Release behavior

On tag `vX.Y.Z`, the workflow:

1. Builds CLI archives for:
   - `x86_64-unknown-linux-gnu`
   - `aarch64-unknown-linux-gnu`
   - `x86_64-apple-darwin`
   - `aarch64-apple-darwin`
   - `x86_64-pc-windows-msvc`
2. Attaches those archives to the GitHub Release
3. Publishes `syncmd-core` to crates.io
4. Waits briefly for the crates.io index to update
5. Publishes `syncmd-cli` to crates.io
6. Builds and uploads Python wheels for CPython 3.9 through 3.13
7. Publishes the npm package from `npm/syncmd`

## Install commands after publish

```bash
cargo install syncmd-cli
pip install syncmd
npm install -g syncmd
```

## Known gaps

- No GitHub repo secrets are set yet, so publishing cannot succeed until they are added.
- `maturin` was not installed in this local shell, so the Python wheel build was not exercised
  locally before handoff.
- Cargo users still install with `cargo install syncmd-cli`, not `cargo install syncmd`. That is
  by design in the current crate naming.
