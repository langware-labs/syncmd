#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
ARTIFACT_DIR="${PROJECT_DIR}/.artifacts/v0.1.0"
NETWORK_NAME="syncmd-npm-test"
SERVER_NAME="syncmd-artifacts"

cleanup() {
  docker rm -f "$SERVER_NAME" >/dev/null 2>&1 || true
  docker network rm "$NETWORK_NAME" >/dev/null 2>&1 || true
}

trap cleanup EXIT

cd "$PROJECT_DIR"

if [[ ! -f "${PROJECT_DIR}/syncmd-0.1.0.tgz" ]]; then
  echo "Missing npm tarball: syncmd-0.1.0.tgz"
  exit 1
fi

if [[ ! -f "${ARTIFACT_DIR}/syncmd-x86_64-unknown-linux-gnu.tar.gz" ]]; then
  echo "Missing release asset: ${ARTIFACT_DIR}/syncmd-x86_64-unknown-linux-gnu.tar.gz"
  exit 1
fi

docker network create "$NETWORK_NAME" >/dev/null

docker run -d \
  --name "$SERVER_NAME" \
  --network "$NETWORK_NAME" \
  -v "${PROJECT_DIR}/.artifacts:/usr/share/nginx/html:ro" \
  nginx:alpine >/dev/null

docker run --rm \
  --platform linux/amd64 \
  --network "$NETWORK_NAME" \
  -v "${PROJECT_DIR}:/work" \
  -w /work \
  -e SYNCMD_BINARY_MIRROR=http://${SERVER_NAME} \
  node:20-bookworm \
  bash -lc 'npm install -g /work/syncmd-0.1.0.tgz && syncmd --help'
