# syncmd

`npm install -g syncmd` installs the prebuilt `syncmd` CLI for your platform.

The package downloads release artifacts from:

- `https://github.com/langware/syncmd/releases`

Environment overrides:

- `SYNCMD_BINARY_PATH` runs an already-installed binary instead of the vendored one.
- `SYNCMD_BINARY_MIRROR` replaces the GitHub Releases base URL.
- `SYNCMD_SKIP_DOWNLOAD=1` skips the postinstall fetch.
