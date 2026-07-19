#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
REPO_SLUG="langware-labs/syncmd"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

cd "$PROJECT_DIR"

need_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo -e "${RED}Missing required command: $1${NC}"
    exit 1
  fi
}

need_env() {
  if [[ -z "${!1:-}" ]]; then
    echo -e "${RED}Missing required environment variable: $1${NC}"
    exit 1
  fi
}

version_from_cargo() {
  sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1
}

echo -e "${GREEN}=== syncmd deployment ===${NC}"

need_cmd git
need_cmd gh

need_env CARGO_REGISTRY_TOKEN
need_env TWINE_API_TOKEN
need_env NPM_TOKEN

VERSION="$(version_from_cargo)"
TAG="v${VERSION}"

echo -e "Version: ${GREEN}${VERSION}${NC}"
echo -e "Repo:    ${GREEN}${REPO_SLUG}${NC}"

if ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  echo -e "${RED}Not inside a git repository${NC}"
  exit 1
fi

if [[ -n "$(git status --short)" ]]; then
  echo -e "${RED}Working tree is not clean. Commit or stash changes first.${NC}"
  exit 1
fi

echo ""
echo -e "${YELLOW}Running preflight...${NC}"
"$SCRIPT_DIR/release-check.sh"

echo ""
echo -e "${YELLOW}Syncing GitHub Actions secrets from local env...${NC}"
printf '%s' "$CARGO_REGISTRY_TOKEN" | gh secret set CARGO_REGISTRY_TOKEN --repo "$REPO_SLUG"
printf '%s' "$TWINE_API_TOKEN" | gh secret set PYPI_API_TOKEN --repo "$REPO_SLUG"
printf '%s' "$NPM_TOKEN" | gh secret set NPM_TOKEN --repo "$REPO_SLUG"
echo -e "${GREEN}GitHub secrets updated${NC}"

if git rev-parse "$TAG" >/dev/null 2>&1; then
  echo -e "${RED}Tag ${TAG} already exists locally${NC}"
  exit 1
fi

if git ls-remote --tags origin "refs/tags/${TAG}" | grep -q .; then
  echo -e "${RED}Tag ${TAG} already exists on origin${NC}"
  exit 1
fi

echo ""
echo -e "${YELLOW}Pushing main and tag ${TAG}...${NC}"
git push origin main
git tag -a "$TAG" -m "Release ${VERSION}"
git push origin "$TAG"

echo ""
echo -e "${YELLOW}Watching release workflow...${NC}"
gh run watch --repo "$REPO_SLUG"

echo ""
echo -e "${GREEN}Deployment submitted.${NC}"
echo -e "Watch runs with: ${YELLOW}gh run list --repo ${REPO_SLUG} --workflow release${NC}"
