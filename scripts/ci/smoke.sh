#!/usr/bin/env bash
# Smoke-tests a release binary: prints its version, starts the panel on a
# throwaway database and checks that it answers HTTP. Foreign architectures run
# under qemu-user (binfmt), so every published binary is executed before release.
#
# Usage: scripts/ci/smoke.sh <path/to/linxpanel> [port]
set -euo pipefail

BIN="$1"
PORT="${2:-17200}"
WORK="$(mktemp -d)"
cleanup() {
  [[ -n "${PID:-}" ]] && kill "${PID}" 2>/dev/null || true
  rm -rf "${WORK}"
}
trap cleanup EXIT

echo "== $(file -b "${BIN}" | cut -d, -f1-2)"
"${BIN}" --version

# The binary resolves data/ next to itself; run a copy so nothing leaks into
# the source tree.
mkdir -p "${WORK}/bin"
cp "${BIN}" "${WORK}/bin/linxpanel"
(
  cd "${WORK}"
  PORT="${PORT}" BIND=127.0.0.1 RUST_LOG=warn \
    DATABASE_URL="sqlite://${WORK}/panel.db?mode=rwc" \
    exec "${WORK}/bin/linxpanel"
) > "${WORK}/panel.log" 2>&1 &
PID=$!

# Emulated architectures start slowly; give them time.
for _ in $(seq 1 120); do
  if curl -fsS -o /dev/null "http://127.0.0.1:${PORT}/api/health" 2>/dev/null; then
    break
  fi
  if ! kill -0 "${PID}" 2>/dev/null; then
    echo "panel exited during startup:" >&2
    cat "${WORK}/panel.log" >&2
    exit 1
  fi
  sleep 1
done

health="$(curl -fsS "http://127.0.0.1:${PORT}/api/health")"
login_page="$(curl -fsS -o /dev/null -w '%{http_code}' "http://127.0.0.1:${PORT}/login")"
echo "health: ${health}"
echo "login page: HTTP ${login_page}"
[[ "${login_page}" == "200" ]] || { cat "${WORK}/panel.log" >&2; exit 1; }
echo "OK"
