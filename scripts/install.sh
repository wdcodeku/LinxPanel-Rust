#!/usr/bin/env bash
set -euo pipefail

REPO_URL="${LINXPANEL_REPO_URL:-https://github.com/wdcodeku/LinxPanel-Rust.git}"
INSTALL_ROOT="${LINXPANEL_INSTALL_ROOT:-/opt/linxpanel}"
PORT="${LINXPANEL_PORT:-7200}"
MIN_CARGO_VERSION="${LINXPANEL_MIN_CARGO_VERSION:-1.78.0}"
LANGUAGE="${LINXPANEL_LANG:-}"

normalize_lang() {
  local value="${1:-}"
  value="$(printf '%s' "${value}" | tr '[:upper:]' '[:lower:]')"
  case "${value}" in
    2 | zh | zh-cn | zh_cn | cn | chinese | 中文) echo "zh" ;;
    *) echo "en" ;;
  esac
}

select_language() {
  if [[ -n "${LANGUAGE}" ]]; then
    LANGUAGE="$(normalize_lang "${LANGUAGE}")"
    return
  fi

  if [[ -t 0 ]]; then
    echo "[LinxPanel] Select language / 选择语言:"
    echo "[LinxPanel]   1) English (default)"
    echo "[LinxPanel]   2) 中文"
    local choice=""
    read -r -p "[LinxPanel] Enter choice [1/2]: " choice || true
    LANGUAGE="$(normalize_lang "${choice}")"
  else
    LANGUAGE="en"
  fi
}

msg() {
  local key="$1"
  if [[ "${LANGUAGE}" == "zh" ]]; then
    case "${key}" in
      language_selected) echo "已选择语言：%s" ;;
      sudo_not_found) echo "未找到 sudo，请使用 root 执行。" ;;
      install_deps) echo "正在安装构建依赖..." ;;
      unsupported_pkg_manager) echo "不支持的包管理器，请手动安装 git/npm/cargo。" ;;
      cargo_ok) echo "检测到 cargo %s，版本校验通过。" ;;
      cargo_ok_stable) echo "检测到 rustup stable cargo %s，版本校验通过。" ;;
      cargo_old_install) echo "Cargo 缺失或版本过低（需要 >= %s），正在安装 Rust stable 工具链..." ;;
      curl_required) echo "引导安装 rustup 需要 curl。" ;;
      rustup_bootstrap_retry) echo "rustup 引导安装未完成，正在重新检测现有 cargo 工具链..." ;;
      using_rustup_stable) echo "使用 rustup stable cargo %s。" ;;
      using_system_cargo) echo "使用系统 cargo %s。" ;;
      cargo_too_old) echo "Cargo 版本仍然过低（%s），请手动升级 Rust 工具链。" ;;
      linux_only) echo "当前安装脚本仅支持 Linux。" ;;
      required_cmd_missing) echo "依赖安装后缺少命令：%s" ;;
      clone_repo) echo "正在克隆仓库..." ;;
      build_linxpanel) echo "正在构建 LinxPanel（可能耗时较长）..." ;;
      build_bin_not_found) echo "构建成功但未找到二进制文件：%s" ;;
      install_files) echo "正在安装文件到 %s..." ;;
      configure_systemd) echo "正在配置 systemd 服务..." ;;
      deploy_complete) echo "部署完成。" ;;
      service_status) echo "服务状态：systemctl status linxpanel --no-pager" ;;
      no_systemd) echo "未检测到 systemd，改为 nohup 启动..." ;;
      deploy_complete_nohup) echo "部署完成（nohup 模式）。" ;;
      running_url) echo "LinxPanel 已运行：http://%s:%s" ;;
      default_admin) echo "默认管理员：admin / linxpanel" ;;
      *) echo "${key}" ;;
    esac
  else
    case "${key}" in
      language_selected) echo "Language selected: %s" ;;
      sudo_not_found) echo "sudo not found, please run as root." ;;
      install_deps) echo "Installing build dependencies..." ;;
      unsupported_pkg_manager) echo "Unsupported package manager. Please install git/npm/cargo manually." ;;
      cargo_ok) echo "Detected cargo %s, version check passed." ;;
      cargo_ok_stable) echo "Detected rustup stable cargo %s, version check passed." ;;
      cargo_old_install) echo "Cargo is missing or too old (need >= %s), installing Rust stable toolchain..." ;;
      curl_required) echo "curl is required to bootstrap rustup." ;;
      rustup_bootstrap_retry) echo "rustup bootstrap did not complete, re-checking existing cargo toolchain..." ;;
      using_rustup_stable) echo "Using rustup stable cargo %s." ;;
      using_system_cargo) echo "Using system cargo %s." ;;
      cargo_too_old) echo "Cargo version is still too old (%s). Please update Rust toolchain manually." ;;
      linux_only) echo "This installer currently supports Linux only." ;;
      required_cmd_missing) echo "Required command missing after dependency install: %s" ;;
      clone_repo) echo "Cloning repository..." ;;
      build_linxpanel) echo "Building LinxPanel (this may take a while)..." ;;
      build_bin_not_found) echo "Build succeeded but binary not found: %s" ;;
      install_files) echo "Installing files to %s..." ;;
      configure_systemd) echo "Configuring systemd service..." ;;
      deploy_complete) echo "Deploy complete." ;;
      service_status) echo "Service status: systemctl status linxpanel --no-pager" ;;
      no_systemd) echo "systemd not detected, starting with nohup..." ;;
      deploy_complete_nohup) echo "Deploy complete (nohup mode)." ;;
      running_url) echo "LinxPanel is running on: http://%s:%s" ;;
      default_admin) echo "Default admin: admin / linxpanel" ;;
      *) echo "${key}" ;;
    esac
  fi
}

logf() {
  local key="$1"
  shift
  local template
  template="$(msg "${key}")"
  printf "[LinxPanel] "
  printf "${template}\n" "$@"
}

die() {
  local key="$1"
  shift
  local template
  template="$(msg "${key}")"
  printf "[LinxPanel] " >&2
  printf "${template}\n" "$@" >&2
  exit 1
}

select_language
if [[ "${LANGUAGE}" == "zh" ]]; then
  logf language_selected "中文"
else
  logf language_selected "English"
fi

if [[ "$(id -u)" -eq 0 ]]; then
  SUDO=""
  RUN_USER="${SUDO_USER:-root}"
else
  if ! command -v sudo >/dev/null 2>&1; then
    die sudo_not_found
  fi
  SUDO="sudo"
  RUN_USER="${USER}"
fi

RUN_GROUP="$(id -gn "${RUN_USER}" 2>/dev/null || echo "${RUN_USER}")"
RUN_HOME="$(getent passwd "${RUN_USER}" 2>/dev/null | cut -d: -f6 || true)"
if [[ -z "${RUN_HOME}" ]]; then
  RUN_HOME="${HOME}"
fi

export PATH="${RUN_HOME}/.cargo/bin:${HOME}/.cargo/bin:${PATH}"

require_cmd() {
  command -v "$1" >/dev/null 2>&1
}

version_ge() {
  local current="$1"
  local required="$2"
  [[ "$(printf '%s\n%s\n' "${required}" "${current}" | sort -V | head -n1)" == "${required}" ]]
}

is_valid_ipv4() {
  local ip="$1"
  [[ "${ip}" =~ ^([0-9]{1,3}\.){3}[0-9]{1,3}$ ]]
}

is_valid_ipv6() {
  local ip="$1"
  [[ "${ip}" =~ : ]]
}

format_host_for_url() {
  local host="$1"
  if is_valid_ipv6 "${host}"; then
    printf '[%s]' "${host}"
  else
    printf '%s' "${host}"
  fi
}

detect_server_ip() {
  local ip_candidate=""

  if [[ -n "${LINXPANEL_PUBLIC_IP:-}" ]]; then
    printf '%s' "${LINXPANEL_PUBLIC_IP}"
    return
  fi

  if require_cmd curl; then
    ip_candidate="$(curl -4fsS --connect-timeout 3 --max-time 5 https://api.ipify.org 2>/dev/null || true)"
    if is_valid_ipv4 "${ip_candidate}"; then
      printf '%s' "${ip_candidate}"
      return
    fi

    ip_candidate="$(curl -6fsS --connect-timeout 3 --max-time 5 https://api64.ipify.org 2>/dev/null || true)"
    if is_valid_ipv6 "${ip_candidate}"; then
      printf '%s' "${ip_candidate}"
      return
    fi
  fi

  if require_cmd ip; then
    ip_candidate="$(ip -4 route get 1.1.1.1 2>/dev/null | awk '{for(i=1;i<=NF;i++){if($i==\"src\"){print $(i+1); exit}}}' || true)"
    if is_valid_ipv4 "${ip_candidate}"; then
      printf '%s' "${ip_candidate}"
      return
    fi

    ip_candidate="$(ip -6 route get 2001:4860:4860::8888 2>/dev/null | awk '{for(i=1;i<=NF;i++){if($i==\"src\"){print $(i+1); exit}}}' || true)"
    if is_valid_ipv6 "${ip_candidate}"; then
      printf '%s' "${ip_candidate}"
      return
    fi
  fi

  ip_candidate="$(hostname -I 2>/dev/null | awk '{for(i=1;i<=NF;i++){if($i!~ /^127\\./){print $i; exit}}}' || true)"
  if [[ -n "${ip_candidate}" ]]; then
    printf '%s' "${ip_candidate}"
    return
  fi

  printf '%s' "127.0.0.1"
}

get_cargo_version() {
  cargo --version 2>/dev/null | grep -Eo '[0-9]+\.[0-9]+\.[0-9]+' | head -n1 || true
}

get_stable_cargo_version() {
  if ! require_cmd rustup; then
    return
  fi
  cargo +stable --version 2>/dev/null | grep -Eo '[0-9]+\.[0-9]+\.[0-9]+' | head -n1 || true
}

install_deps() {
  if require_cmd git && require_cmd npm && require_cmd cargo; then
    return
  fi

  logf install_deps
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
    die unsupported_pkg_manager
  fi
}

ensure_modern_cargo() {
  local current_version
  local stable_version
  current_version="$(get_cargo_version)"

  if [[ -n "${current_version}" ]] && version_ge "${current_version}" "${MIN_CARGO_VERSION}"; then
    logf cargo_ok "${current_version}"
    return
  fi

  stable_version="$(get_stable_cargo_version)"
  if [[ -n "${stable_version}" ]] && version_ge "${stable_version}" "${MIN_CARGO_VERSION}"; then
    logf cargo_ok_stable "${stable_version}"
    return
  fi

  logf cargo_old_install "${MIN_CARGO_VERSION}"
  if ! require_cmd curl; then
    die curl_required
  fi

  if ! require_cmd rustup; then
    if ! RUSTUP_INIT_SKIP_PATH_CHECK=yes curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable; then
      logf rustup_bootstrap_retry
    fi
  fi

  export PATH="${RUN_HOME}/.cargo/bin:${HOME}/.cargo/bin:${PATH}"

  if require_cmd rustup; then
    rustup toolchain install stable --profile minimal >/dev/null 2>&1 || true
    rustup default stable >/dev/null 2>&1 || true
    stable_version="$(get_stable_cargo_version)"
    if [[ -n "${stable_version}" ]] && version_ge "${stable_version}" "${MIN_CARGO_VERSION}"; then
      logf using_rustup_stable "${stable_version}"
      return
    fi
  fi

  current_version="$(get_cargo_version)"
  if [[ -n "${current_version}" ]] && version_ge "${current_version}" "${MIN_CARGO_VERSION}"; then
    logf using_system_cargo "${current_version}"
    return
  fi

  if [[ -z "${current_version}" ]] || ! version_ge "${current_version}" "${MIN_CARGO_VERSION}"; then
    die cargo_too_old "${current_version:-unknown}"
  fi
}

if [[ "$(uname -s)" != "Linux" ]]; then
  die linux_only
fi

install_deps
ensure_modern_cargo

for cmd in git npm cargo bash; do
  if ! require_cmd "${cmd}"; then
    die required_cmd_missing "${cmd}"
  fi
done

TMP_DIR="$(mktemp -d -t linxpanel-install-XXXXXX)"
trap 'rm -rf "${TMP_DIR}"' EXIT

logf clone_repo
git clone --depth 1 "${REPO_URL}" "${TMP_DIR}/repo"

logf build_linxpanel
LINXPANEL_LANG="${LANGUAGE}" bash "${TMP_DIR}/repo/scripts/build-single.sh"

BIN_SOURCE="${TMP_DIR}/repo/target/release/linxpanel"
if [[ ! -f "${BIN_SOURCE}" ]]; then
  die build_bin_not_found "${BIN_SOURCE}"
fi

logf install_files "${INSTALL_ROOT}"
${SUDO} mkdir -p "${INSTALL_ROOT}/bin" "${INSTALL_ROOT}/data" "${INSTALL_ROOT}/logs"
${SUDO} install -m 755 "${BIN_SOURCE}" "${INSTALL_ROOT}/bin/linxpanel"
${SUDO} chown -R "${RUN_USER}:${RUN_GROUP}" "${INSTALL_ROOT}"

SERVICE_FILE="/etc/systemd/system/linxpanel.service"
if require_cmd systemctl; then
  logf configure_systemd
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
  logf deploy_complete
  logf service_status
else
  logf no_systemd
  nohup env \
    PORT="${PORT}" \
    DATABASE_URL="sqlite:${INSTALL_ROOT}/data/linxpanel.db?mode=rwc" \
    "${INSTALL_ROOT}/bin/linxpanel" \
    >> "${INSTALL_ROOT}/logs/linxpanel.out.log" \
    2>> "${INSTALL_ROOT}/logs/linxpanel.err.log" &
  logf deploy_complete_nohup
fi

SERVER_IP="$(detect_server_ip)"
SERVER_HOST="$(format_host_for_url "${SERVER_IP}")"
logf running_url "${SERVER_HOST}" "${PORT}"
logf default_admin
