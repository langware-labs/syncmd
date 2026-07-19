# Deployment

This document records the current deployment state for `syncmd` as of July 19, 2026 and the
exact steps needed to publish it.

## Current state

* GitHub repo: `https://github.com/langware-labs/syncmd`

* Default branch: `main`

* Visibility: public

* GitHub Actions workflow present: `release`

* Repo secrets configured: none yet

## What is already working

### GitHub

* Local git repository initialized

* Commits created and pushed to `langware-labs/syncmd`

* Release workflow checked in at `.github/workflows/release.yml`

* Workflow is visible to GitHub Actions

### Cargo

* `syncmd-core` passes `cargo publish --dry-run`

* `syncmd` is wired for publish, but `cargo publish --dry-run -p syncmd` fails before the
  first release because it depends on `syncmd-core`, which is not yet present on crates.io

* This is expected for the initial publish; the workflow publishes `syncmd-core` first, waits,
  then publishes `syncmd`

### npm

* `npm pack --dry-run ./npm/syncmd` passes

* `npm view syncmd version` returned `404`, so the package name appears available as of
  July 19, 2026

* The npm package is a wrapper that downloads the GitHub Release binary for the current platform

* Local Docker install test now passes for `linux/amd64` using a locally served release asset and
  the packed npm tarball

### PyPI

* `crates/syncmd-py/pyproject.toml` is set up for `maturin`

* `https://pypi.org/pypi/syncmd/json` returned `404`, so the package name appears available as of
  July 19, 2026

* Local Linux wheel build now passes

* Local Docker wheel install/import test now passes for `linux/amd64`

## Token schema

`syncmd` now follows the same token model as `flowpad-oss`:

* local shell environment variables are the source of truth

* the deploy script copies them into GitHub Actions secrets

* the release workflow consumes the GitHub secrets

Expected local environment variables:

* `CARGO_REGISTRY_TOKEN`

* `TWINE_API_TOKEN`

* `NPM_TOKEN`

When you run `./scripts/deploy_to_github.sh`, it maps them to repo secrets:

* `CARGO_REGISTRY_TOKEN` -> `CARGO_REGISTRY_TOKEN`

* `TWINE_API_TOKEN` -> `PYPI_API_TOKEN`

* `NPM_TOKEN` -> `NPM_TOKEN`

This matches the `flowpad-oss` pattern where PyPI publishing is driven by
`TWINE_API_TOKEN` locally.

## Preflight

Run this before tagging:

```bash
./scripts/release-check.sh
```

Current checks performed by that script:

* version alignment across Cargo, PyPI, and npm

* `cargo check -p syncmd`

* `cargo check -p syncmd-py`

* npm wrapper script syntax checks

* `npm pack --dry-run ./npm/syncmd`

## Local Docker npm test

This repo now has a repeatable local Docker test for the npm install path.

### What it validates

* the npm tarball installs successfully

* the `postinstall` downloader fetches the expected release archive

* the downloaded Linux binary runs inside Docker

### Required local artifacts

For the current version, the test expects:

* `syncmd-0.1.0.tgz`

* `.artifacts/v0.1.0/syncmd-x86_64-unknown-linux-gnu.tar.gz`

### Build the Linux test artifact

The compatible `x86_64-unknown-linux-gnu` test binary was built on an older Debian baseline to
avoid glibc incompatibility with the Node test container:

```bash
docker run --rm --platform linux/amd64 \
  -v /Users/shlom/Documents/dev/syncmd:/work \
  -w /work \
  rust:1.89-bullseye \
  bash -lc '
    export CARGO_TARGET_DIR=/tmp/syncmd-target
    /usr/local/cargo/bin/rustup target add x86_64-unknown-linux-gnu
    /usr/local/cargo/bin/cargo build --release -p syncmd --target x86_64-unknown-linux-gnu
    cp /tmp/syncmd-target/x86_64-unknown-linux-gnu/release/syncmd .artifacts/v0.1.0/syncmd
    tar -C .artifacts/v0.1.0 -czf .artifacts/v0.1.0/syncmd-x86_64-unknown-linux-gnu.tar.gz syncmd
  '
```

### Run the Docker npm test

```bash
./scripts/test_npm_docker.sh
```

Current result on July 19, 2026: pass.

## Local Docker PyPI test

This repo now has a validated local Docker test for the Python wheel install path.

### What it validates

* a Linux wheel can be built for the package

* `pip install` succeeds inside Docker

* `import syncmd` works

* `syncmd.plan()` works against a real temporary git repository

* `to_json()` round-trips to `to_dict()`

### Built wheel

The validated wheel produced on July 19, 2026 was:

* `dist-py-linux/syncmd-0.1.0-cp39-abi3-manylinux_2_17_x86_64.manylinux2014_x86_64.whl`

### Build the Linux wheel

```bash
docker run --rm --platform linux/amd64 \
  -v /Users/shlom/Documents/dev/syncmd:/io \
  ghcr.io/pyo3/maturin \
  build --release \
  -m /io/crates/syncmd-py/Cargo.toml \
  --interpreter python3.11 \
  --out /io/dist-py-linux
```

### Run the Docker wheel install test

```bash
docker run --rm --platform linux/amd64 \
  -v /Users/shlom/Documents/dev/syncmd:/work \
  -w /work \
  python:3.11-bullseye \
  bash -lc 'pip install /work/dist-py-linux/syncmd-0.1.0-cp39-abi3-manylinux_2_17_x86_64.manylinux2014_x86_64.whl && python - <<\"PY\"
import json, os, subprocess, tempfile, syncmd

def git(tmp, *args):
    subprocess.run([\"git\", *args], cwd=tmp, check=True,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

with tempfile.TemporaryDirectory() as tmp:
    git(tmp, \"init\", \"-q\", \"-b\", \"main\")
    git(tmp, \"config\", \"user.name\", \"t\")
    git(tmp, \"config\", \"user.email\", \"t@t.dev\")
    with open(os.path.join(tmp, \"CLAUDE.md\"), \"w\") as f:
        f.write(\"rules v1\\n\")
    git(tmp, \"add\", \"-A\")
    git(tmp, \"commit\", \"-q\", \"-m\", \"only claude\")
    report = syncmd.plan(tmp)
    assert report.groups[0].name == \"instructions\"
    assert report.groups[0].decision == \"propagated\"
    assert json.loads(report.to_json()) == report.to_dict()
    print(\"python syncmd ok\")
PY'
```

Current result on July 19, 2026: pass.

## Deployment command

With the three local token variables exported:

```bash
./scripts/deploy_to_github.sh
```

That script:

1. runs the local preflight
2. syncs local tokens into GitHub repo secrets
3. pushes `main`
4. creates and pushes `v<version>`
5. waits on the `release` GitHub Actions workflow

## First release

1. Confirm the version in:

   * `Cargo.toml`

   * `crates/syncmd-py/pyproject.toml`

   * `npm/syncmd/package.json`
2. Export the local tokens:

```bash
export CARGO_REGISTRY_TOKEN=...
export TWINE_API_TOKEN=...
export NPM_TOKEN=...
```

3. Run:

```bash
./scripts/deploy_to_github.sh
```

## Release behavior

On tag `vX.Y.Z`, the workflow:

1. Builds CLI archives for:

   * `x86_64-unknown-linux-gnu`

   * `aarch64-unknown-linux-gnu`

   * `x86_64-apple-darwin`

   * `aarch64-apple-darwin`

   * `x86_64-pc-windows-msvc`
2. Attaches those archives to the GitHub Release
3. Publishes `syncmd-core` to crates.io
4. Waits briefly for the crates.io index to update
5. Publishes `syncmd` to crates.io
6. Builds and uploads Python wheels for CPython 3.9 through 3.13
7. Publishes the npm package from `npm/syncmd`

## Install commands after publish

```bash
cargo install syncmd
pip install syncmd
npm install -g syncmd
```

## Known gaps

* No GitHub repo secrets are set yet; the deploy script will populate them from local env vars.

* No GitHub Release exists yet for `syncmd`, and there are no official release assets uploaded yet.

* The latest local crate rename and documentation changes are not pushed yet.

* `syncmd-core` must be published first on the first release so the `syncmd` CLI crate can resolve
  its dependency from crates.io.

