# Deployment

This document records the current deployment state for `syncmd` as of July 19, 2026 and the
exact steps needed to complete the remaining public package releases.

## Current state

* GitHub repo: `https://github.com/langware-labs/syncmd`

* Default branch: `main`

* Visibility: public

* GitHub Actions workflow present: `release`

* GitHub repo secrets configured:
  * `CARGO_REGISTRY_TOKEN`
  * `PYPI_API_TOKEN`
  * `NPM_TOKEN`

## What is already working

### GitHub

* Local git repository initialized

* Commits created and pushed to `langware-labs/syncmd`

* Release workflow checked in at `.github/workflows/release.yml`

* GitHub release `v0.1.0` exists at:
  * `https://github.com/langware-labs/syncmd/releases/tag/v0.1.0`

* Official release archives were uploaded manually for:
  * `x86_64-unknown-linux-gnu`
  * `aarch64-unknown-linux-gnu`
  * `x86_64-apple-darwin`
  * `aarch64-apple-darwin`

### Cargo

* `syncmd-core` passes `cargo publish --dry-run`

* `syncmd` is wired for public install as `cargo install syncmd`

* Local Docker source install test passes with:
  * `cargo install --locked --path crates/syncmd-cli`

* Official crates.io publish is currently blocked by the crates.io account, not by the package:
  * crates.io rejected publish because the publishing account still needs a verified email address

### npm

* `npm pack --dry-run ./npm/syncmd` passes

* The npm package is a wrapper that downloads the GitHub Release binary for the current platform

* Local Docker install test passes for `linux/amd64` using the packed npm tarball

* Official npm publish is currently blocked by npm account policy, not by the package:
  * npm rejected publish because the current token does not satisfy publish-time 2FA requirements
  * a publish-capable granular token with 2FA bypass, or an interactive publish with account 2FA,
    is required

### PyPI

* `crates/syncmd-py/pyproject.toml` is set up for `maturin`

* `syncmd 0.1.0` is published on PyPI:
  * `https://pypi.org/project/syncmd/0.1.0/`

* Published artifacts:
  * macOS arm64 wheel
  * Linux x86_64 manylinux wheel
  * source distribution

* Official Docker install/import test passes for `linux/amd64` with:
  * `pip install syncmd`

## Token schema

`syncmd` follows the same token model as `flowpad-oss`:

* local shell environment variables are the source of truth

* the deploy script copies them into GitHub Actions secrets

* the release workflow consumes the GitHub secrets

Expected local environment variables:

* `CARGO_REGISTRY_TOKEN`

* `TWINE_API_TOKEN` or a configured `~/.pypirc`

* `NPM_TOKEN`

When you run `./scripts/deploy_to_github.sh`, it maps them to repo secrets:

* `CARGO_REGISTRY_TOKEN` -> `CARGO_REGISTRY_TOKEN`

* `TWINE_API_TOKEN` -> `PYPI_API_TOKEN`

* `NPM_TOKEN` -> `NPM_TOKEN`

This matches the `flowpad-oss` pattern where PyPI publishing is driven by
`TWINE_API_TOKEN` locally. A local `~/.pypirc` also works for manual `twine` upload.

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

## Official Docker validation

### Cargo

Local source-install validation passed in Docker:

```bash
docker run --rm --platform linux/amd64 \
  -v /Users/shlom/Documents/dev/syncmd:/work \
  -w /work \
  rust:1.89-bullseye \
  bash -lc 'export CARGO_TARGET_DIR=/tmp/syncmd-target; /usr/local/cargo/bin/cargo install --locked --path crates/syncmd-cli --root /tmp/syncmd-install && /tmp/syncmd-install/bin/syncmd --help'
```

Status: not yet validated from crates.io because crates.io publish is still blocked by missing
email verification on the publishing account.

### PyPI

Official install validation passed in Docker:

```bash
docker run --rm --platform linux/amd64 \
  python:3.11-bullseye \
  bash -lc 'pip install syncmd && python - <<\"PY\"
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

Status: published and validated.

### npm

Status: not yet validated from npm registry because npm rejected publish for the current token:

* `403 Forbidden`
* publish requires account 2FA approval or a granular token with publish permission and 2FA bypass

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

For `v0.1.0`, the workflow was not used as the final source of truth because the wheel matrix had a
Windows failure. The release artifacts and PyPI publication were completed manually.

## Remaining steps

1. Cargo publish unblock:

   * sign in to crates.io with the publishing account
   * verify the account email at `https://crates.io/settings/profile`
   * then run:

```bash
source env.local
cargo publish --locked -p syncmd-core
sleep 30
cargo publish --locked -p syncmd
```

2. npm publish unblock:

   * either create a granular npm token that can publish with 2FA bypass
   * or run an interactive `npm publish` from an account session that can satisfy 2FA

3. After those two unblocks, validate the official public installs:

```bash
docker run --rm --platform linux/amd64 rust:1.89-bullseye bash -lc 'cargo install --locked syncmd && syncmd --help'
docker run --rm --platform linux/amd64 node:20-bookworm bash -lc 'npm install -g syncmd && syncmd --help'
```

## Release behavior

On tag `vX.Y.Z`, the workflow is intended to:

1. Build CLI archives for:

   * `x86_64-unknown-linux-gnu`

   * `aarch64-unknown-linux-gnu`

   * `x86_64-apple-darwin`

   * `aarch64-apple-darwin`

2. Attach those archives to the GitHub Release
3. Publish `syncmd-core` to crates.io
4. Wait briefly for the crates.io index to update
5. Publish `syncmd` to crates.io
6. Build and upload Python distributions
7. Publish the npm package from `npm/syncmd`

## Install commands

Once all registries are fully live:

```bash
cargo install syncmd
pip install syncmd
npm install -g syncmd
```

## Version checklist

Before the next release, confirm the version in:

   * `Cargo.toml`

   * `crates/syncmd-py/pyproject.toml`

   * `npm/syncmd/package.json`

Export the local tokens before running deployment or manual publish commands:

```bash
export CARGO_REGISTRY_TOKEN=...
export TWINE_API_TOKEN=...
export NPM_TOKEN=...
```
