#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

command -v cargo >/dev/null 2>&1 || { echo "cargo not found in PATH"; exit 1; }
command -v npm >/dev/null 2>&1 || { echo "npm not found in PATH"; exit 1; }

echo "[LinxPanel] Building web assets..."
cd "${REPO_ROOT}/web"
if [[ -f package-lock.json ]]; then
  npm ci
else
  npm install
fi
npm run build

echo "[LinxPanel] Building backend release binary..."
cd "${REPO_ROOT}"
cargo build --release

BINARY_PATH="${REPO_ROOT}/target/release/linxpanel"
if [[ ! -f "${BINARY_PATH}" ]]; then
  echo "build completed but binary not found: ${BINARY_PATH}"
  exit 1
fi

echo "[LinxPanel] Done. Single deployable file:"
echo "  ${BINARY_PATH}"
