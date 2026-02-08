#!/usr/bin/env bash
set -euo pipefail

REPO_URL="${LINXPANEL_REPO_URL:-https://github.com/wdcodeku/LinxPanel-Rust.git}"
INSTALL_ROOT="${LINXPANEL_INSTALL_ROOT:-/opt/linxpanel}"
PORT="${LINXPANEL_PORT:-7200}"

if [[ "$(id -u)" -eq 0 ]]; then
  SUDO=""
  RUN_USER="${SUDO_USER:-root}"
else
  if ! command -v sudo >/dev/null 2>&1; then
    echo "[LinxPanel] sudo not found, please run as root." >&2
    exit 1
  fi
  SUDO="sudo"
  RUN_USER="${USER}"
fi

RUN_GROUP="$(id -gn "${RUN_USER}" 2>/dev/null || echo "${RUN_USER}")"

log() {
  echo "[LinxPanel] $*"
}

require_cmd() {
  command -v "$1" >/dev/null 2>&1
}

install_deps() {
  if require_cmd git && require_cmd npm && require_cmd cargo; then
    return
  fi

  log "Installing build dependencies..."
  if require_cmd apt-get; then
    ${SUDO} apt-get update
    ${SUDO} apt-get install -y curl ca-certificates git build-essential pkg-config libssl-dev nodejs npm cargo
  elif require_cmd dnf; then
    ${SUDO} dnf install -y curl ca-certificates git gcc gcc-c++ make pkgconfig openssl-devel nodejs npm cargo
  elif require_cmd yum; then
    ${SUDO} yum install -y curl ca-certificates git gcc gcc-c++ make pkgconfig openssl-devel nodejs npm cargo
  elif require_cmd pacman; then
    ${SUDO} pacman -Sy --noconfirm curl ca-certificates git base-devel pkgconf openssl nodejs npm rust
  else
    echo "[LinxPanel] Unsupported package manager. Please install git/npm/cargo manually." >&2
    exit 1
  fi
}

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "[LinxPanel] This installer currently supports Linux only." >&2
  exit 1
fi

install_deps

for cmd in git npm cargo bash; do
  if ! require_cmd "${cmd}"; then
    echo "[LinxPanel] Required command missing after dependency install: ${cmd}" >&2
    exit 1
  fi
done

TMP_DIR="$(mktemp -d -t linxpanel-install-XXXXXX)"
trap 'rm -rf "${TMP_DIR}"' EXIT

log "Cloning repository..."
git clone --depth 1 "${REPO_URL}" "${TMP_DIR}/repo"

log "Building LinxPanel (this may take a while)..."
bash "${TMP_DIR}/repo/scripts/build-single.sh"

BIN_SOURCE="${TMP_DIR}/repo/target/release/linxpanel"
if [[ ! -f "${BIN_SOURCE}" ]]; then
  echo "[LinxPanel] Build succeeded but binary not found: ${BIN_SOURCE}" >&2
  exit 1
fi

log "Installing files to ${INSTALL_ROOT}..."
${SUDO} mkdir -p "${INSTALL_ROOT}/bin" "${INSTALL_ROOT}/data" "${INSTALL_ROOT}/logs"
${SUDO} install -m 755 "${BIN_SOURCE}" "${INSTALL_ROOT}/bin/linxpanel"
${SUDO} chown -R "${RUN_USER}:${RUN_GROUP}" "${INSTALL_ROOT}"

SERVICE_FILE="/etc/systemd/system/linxpanel.service"
if require_cmd systemctl; then
  log "Configuring systemd service..."
  ${SUDO} tee "${SERVICE_FILE}" >/dev/null <<EOF
[Unit]
Description=LinxPanel Service
After=network.target

[Service]
Type=simple
User=${RUN_USER}
Group=${RUN_GROUP}
WorkingDirectory=${INSTALL_ROOT}
Environment=PORT=${PORT}
Environment=DATABASE_URL=sqlite:${INSTALL_ROOT}/data/linxpanel.db?mode=rwc
ExecStart=${INSTALL_ROOT}/bin/linxpanel
Restart=always
RestartSec=3
StandardOutput=append:${INSTALL_ROOT}/logs/linxpanel.out.log
StandardError=append:${INSTALL_ROOT}/logs/linxpanel.err.log

[Install]
WantedBy=multi-user.target
EOF

  ${SUDO} systemctl daemon-reload
  ${SUDO} systemctl enable --now linxpanel
  log "Deploy complete."
  log "Service status: systemctl status linxpanel --no-pager"
else
  log "systemd not detected, starting with nohup..."
  nohup env \
    PORT="${PORT}" \
    DATABASE_URL="sqlite:${INSTALL_ROOT}/data/linxpanel.db?mode=rwc" \
    "${INSTALL_ROOT}/bin/linxpanel" \
    >> "${INSTALL_ROOT}/logs/linxpanel.out.log" \
    2>> "${INSTALL_ROOT}/logs/linxpanel.err.log" &
  log "Deploy complete (nohup mode)."
fi

log "LinxPanel is running on: http://<your-server-ip>:${PORT}"
log "Default admin: admin / linxpanel"
