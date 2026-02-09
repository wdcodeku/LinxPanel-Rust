#!/usr/bin/env bash
set -euo pipefail

REPO_URL="${LINXPANEL_REPO_URL:-https://github.com/wdcodeku/LinxPanel-Rust.git}"
RELEASE_API_URL="${LINXPANEL_RELEASE_API_URL:-https://api.github.com/repos/wdcodeku/LinxPanel-Rust/releases/latest}"
LP_CLI_RAW_URL="${LINXPANEL_LP_CLI_RAW_URL:-https://raw.githubusercontent.com/wdcodeku/LinxPanel-Rust/main/scripts/lp}"
INSTALL_ROOT="${LINXPANEL_INSTALL_ROOT:-/opt/linxpanel}"
PORT="${LINXPANEL_PORT:-7200}"
MIN_CARGO_VERSION="${LINXPANEL_MIN_CARGO_VERSION:-1.78.0}"
LANGUAGE="${LINXPANEL_LANG:-}"
INSTALL_MODE="${LINXPANEL_INSTALL_MODE:-}"

normalize_lang() {
  local value="${1:-}"
  value="$(printf '%s' "${value}" | tr '[:upper:]' '[:lower:]')"
  case "${value}" in
    2 | zh | zh-cn | zh_cn | cn | chinese | 中文) echo "zh" ;;
    *) echo "en" ;;
  esac
}

normalize_install_mode() {
  local value="${1:-}"
  value="$(printf '%s' "${value}" | tr '[:upper:]' '[:lower:]')"
  case "${value}" in
    2 | build | source | compile | src) echo "build" ;;
    *) echo "binary" ;;
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

select_install_mode() {
  if [[ -n "${INSTALL_MODE}" ]]; then
    INSTALL_MODE="$(normalize_install_mode "${INSTALL_MODE}")"
    return
  fi

  if [[ -t 0 ]]; then
    if [[ "${LANGUAGE}" == "zh" ]]; then
      echo "[LinxPanel] 选择安装方式："
      echo "[LinxPanel]   1) 二进制安装（默认，速度更快）"
      echo "[LinxPanel]   2) 编译安装（从源码构建）"
      local choice=""
      read -r -p "[LinxPanel] 请输入 [1/2]: " choice || true
      INSTALL_MODE="$(normalize_install_mode "${choice}")"
    else
      echo "[LinxPanel] Select installation mode:"
      echo "[LinxPanel]   1) Binary install (default, faster)"
      echo "[LinxPanel]   2) Build from source"
      local choice=""
      read -r -p "[LinxPanel] Enter choice [1/2]: " choice || true
      INSTALL_MODE="$(normalize_install_mode "${choice}")"
    fi
  else
    INSTALL_MODE="binary"
  fi
}

msg() {
  local key="$1"
  if [[ "${LANGUAGE}" == "zh" ]]; then
    case "${key}" in
      language_selected) echo "已选择语言：%s" ;;
      install_mode_selected) echo "安装方式：%s" ;;
      mode_binary) echo "二进制安装（默认）" ;;
      mode_build) echo "编译安装（源码）" ;;
      sudo_not_found) echo "未找到 sudo，请使用 root 执行。" ;;
      install_deps) echo "正在安装构建依赖..." ;;
      unsupported_pkg_manager) echo "不支持的包管理器，请手动安装 git/npm/cargo。" ;;
      cargo_ok) echo "检测到 cargo %s，版本校验通过。" ;;
      cargo_ok_stable) echo "检测到 rustup stable cargo %s，版本校验通过。" ;;
      cargo_old_install) echo "Cargo 缺失或版本过低（需要 >= %s），正在安装 Rust stable 工具链..." ;;
      curl_required) echo "当前步骤需要 curl。" ;;
      rustup_bootstrap_retry) echo "rustup 引导安装未完成，正在重新检测现有 cargo 工具链..." ;;
      using_rustup_stable) echo "使用 rustup stable cargo %s。" ;;
      using_system_cargo) echo "使用系统 cargo %s。" ;;
      cargo_too_old) echo "Cargo 版本仍然过低（%s），请手动升级 Rust 工具链。" ;;
      linux_only) echo "当前安装脚本仅支持 Linux。" ;;
      required_cmd_missing) echo "依赖安装后缺少命令：%s" ;;
      clone_repo) echo "正在克隆仓库..." ;;
      build_linxpanel) echo "正在构建 LinxPanel（可能耗时较长）..." ;;
      build_bin_not_found) echo "构建成功但未找到二进制文件：%s" ;;
      fetch_release) echo "正在获取最新发布信息..." ;;
      download_release_asset) echo "正在下载预编译二进制：%s" ;;
      release_download_failed) echo "二进制下载失败：%s" ;;
      release_asset_not_found) echo "未找到匹配当前系统架构的 Linux 预编译包。" ;;
      extract_archive) echo "正在解压二进制包..." ;;
      unzip_required) echo "需要 unzip 或 bsdtar 来解压 zip 包。" ;;
      binary_not_found_after_extract) echo "解压后未找到 linxpanel 二进制文件。" ;;
      binary_mode_fallback_build) echo "二进制安装失败，自动回退为编译安装..." ;;
      install_files) echo "正在安装文件到 %s..." ;;
      install_lp_cli) echo "正在安装 Lp 命令行工具..." ;;
      install_lp_cli_done) echo "Lp 命令可用：%s（别名：lp）" ;;
      install_lp_cli_failed) echo "Lp 安装失败（已跳过）：%s" ;;
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
      install_mode_selected) echo "Install mode: %s" ;;
      mode_binary) echo "Binary install (default)" ;;
      mode_build) echo "Build from source" ;;
      sudo_not_found) echo "sudo not found, please run as root." ;;
      install_deps) echo "Installing build dependencies..." ;;
      unsupported_pkg_manager) echo "Unsupported package manager. Please install git/npm/cargo manually." ;;
      cargo_ok) echo "Detected cargo %s, version check passed." ;;
      cargo_ok_stable) echo "Detected rustup stable cargo %s, version check passed." ;;
      cargo_old_install) echo "Cargo is missing or too old (need >= %s), installing Rust stable toolchain..." ;;
      curl_required) echo "curl is required for this step." ;;
      rustup_bootstrap_retry) echo "rustup bootstrap did not complete, re-checking existing cargo toolchain..." ;;
      using_rustup_stable) echo "Using rustup stable cargo %s." ;;
      using_system_cargo) echo "Using system cargo %s." ;;
      cargo_too_old) echo "Cargo version is still too old (%s). Please update Rust toolchain manually." ;;
      linux_only) echo "This installer currently supports Linux only." ;;
      required_cmd_missing) echo "Required command missing after dependency install: %s" ;;
      clone_repo) echo "Cloning repository..." ;;
      build_linxpanel) echo "Building LinxPanel (this may take a while)..." ;;
      build_bin_not_found) echo "Build succeeded but binary not found: %s" ;;
      fetch_release) echo "Fetching latest release metadata..." ;;
      download_release_asset) echo "Downloading prebuilt binary asset: %s" ;;
      release_download_failed) echo "Failed to download binary asset: %s" ;;
      release_asset_not_found) echo "No Linux prebuilt asset matched current architecture." ;;
      extract_archive) echo "Extracting binary package..." ;;
      unzip_required) echo "unzip or bsdtar is required to extract zip package." ;;
      binary_not_found_after_extract) echo "linxpanel binary not found after extraction." ;;
      binary_mode_fallback_build) echo "Binary install failed, falling back to source build..." ;;
      install_files) echo "Installing files to %s..." ;;
      install_lp_cli) echo "Installing Lp CLI tool..." ;;
      install_lp_cli_done) echo "Lp command ready: %s (alias: lp)" ;;
      install_lp_cli_failed) echo "Failed to install Lp CLI (skipped): %s" ;;
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

  die cargo_too_old "${current_version:-unknown}"
}

detect_arch_candidates() {
  local machine
  machine="$(uname -m 2>/dev/null || true)"
  case "${machine}" in
    x86_64 | amd64) echo "x86_64 amd64" ;;
    aarch64 | arm64) echo "aarch64 arm64" ;;
    armv7l | armv7) echo "armv7 armv7l" ;;
    *) echo "${machine}" ;;
  esac
}

find_binary_in_dir() {
  local search_dir="$1"
  local candidate=""

  candidate="$(find "${search_dir}" -type f -name 'linxpanel' | head -n1 || true)"
  if [[ -z "${candidate}" ]]; then
    candidate="$(find "${search_dir}" -type f -name 'linxpanel-*' | head -n1 || true)"
  fi
  if [[ -z "${candidate}" ]]; then
    candidate="$(find "${search_dir}" -type f -name 'linxpanel_*' | head -n1 || true)"
  fi
  if [[ -z "${candidate}" ]]; then
    return 1
  fi

  chmod +x "${candidate}" || true
  printf '%s' "${candidate}"
}

download_binary_release() {
  local tmp_dir="$1"
  local release_json=""
  local asset_list=""
  local selected_url=""
  local arch_token=""
  local asset_lower=""
  local asset_file=""
  local extract_dir=""
  local binary_path=""

  if ! require_cmd curl; then
    die curl_required
  fi

  logf fetch_release
  release_json="$(curl -fsSL "${RELEASE_API_URL}" 2>/dev/null || true)"
  if [[ -z "${release_json}" ]]; then
    return 2
  fi

  asset_list="$(printf '%s\n' "${release_json}" | grep -Eo '"browser_download_url":[[:space:]]*"[^"]+"' | sed -E 's/.*"([^"]+)"/\1/' || true)"
  if [[ -z "${asset_list}" ]]; then
    return 2
  fi

  for arch_token in $(detect_arch_candidates); do
    while IFS= read -r selected_url; do
      asset_lower="$(printf '%s' "${selected_url}" | tr '[:upper:]' '[:lower:]')"
      if [[ "${asset_lower}" == *"linux"* && "${asset_lower}" == *"${arch_token}"* && "${asset_lower}" == *"linxpanel"* && "${asset_lower}" != *"sha256"* && "${asset_lower}" != *"checksums"* && "${asset_lower}" != *.sig ]]; then
        break 2
      fi
      selected_url=""
    done <<< "${asset_list}"
  done

  if [[ -z "${selected_url}" ]]; then
    while IFS= read -r selected_url; do
      asset_lower="$(printf '%s' "${selected_url}" | tr '[:upper:]' '[:lower:]')"
      if [[ "${asset_lower}" == *"linux"* && "${asset_lower}" == *"linxpanel"* && "${asset_lower}" != *"sha256"* && "${asset_lower}" != *"checksums"* && "${asset_lower}" != *.sig ]]; then
        break
      fi
      selected_url=""
    done <<< "${asset_list}"
  fi

  if [[ -z "${selected_url}" ]]; then
    return 2
  fi

  asset_file="${tmp_dir}/$(basename "${selected_url}")"
  logf download_release_asset "${selected_url}"
  if ! curl -fL --connect-timeout 10 --max-time 600 "${selected_url}" -o "${asset_file}"; then
    logf release_download_failed "${selected_url}"
    return 3
  fi

  extract_dir="${tmp_dir}/release-extract"
  mkdir -p "${extract_dir}"

  case "${asset_file}" in
    *.tar.gz | *.tgz)
      logf extract_archive
      tar -xzf "${asset_file}" -C "${extract_dir}"
      ;;
    *.zip)
      logf extract_archive
      if require_cmd unzip; then
        unzip -oq "${asset_file}" -d "${extract_dir}"
      elif require_cmd bsdtar; then
        bsdtar -xf "${asset_file}" -C "${extract_dir}"
      else
        die unzip_required
      fi
      ;;
    *)
      chmod +x "${asset_file}" || true
      printf '%s' "${asset_file}"
      return 0
      ;;
  esac

  if ! binary_path="$(find_binary_in_dir "${extract_dir}")"; then
    die binary_not_found_after_extract
  fi
  printf '%s' "${binary_path}"
}

build_binary_from_source() {
  local tmp_dir="$1"
  local bin_source=""

  install_deps
  ensure_modern_cargo

  for cmd in git npm cargo bash; do
    if ! require_cmd "${cmd}"; then
      die required_cmd_missing "${cmd}"
    fi
  done

  logf clone_repo
  git clone --depth 1 "${REPO_URL}" "${tmp_dir}/repo"

  logf build_linxpanel
  LINXPANEL_LANG="${LANGUAGE}" bash "${tmp_dir}/repo/scripts/build-single.sh"

  bin_source="${tmp_dir}/repo/target/release/linxpanel"
  if [[ ! -f "${bin_source}" ]]; then
    die build_bin_not_found "${bin_source}"
  fi

  printf '%s' "${bin_source}"
}

escape_sed_replacement() {
  printf '%s' "$1" | sed -e 's/[\/&]/\\&/g'
}

install_lp_cli() {
  local source_cli=""
  local cli_tmp="${TMP_DIR}/lp"
  local cli_target="/usr/local/bin/Lp"
  local cli_alias="/usr/local/bin/lp"
  local install_root_escaped=""

  logf install_lp_cli

  if [[ -f "${TMP_DIR}/repo/scripts/lp" ]]; then
    source_cli="${TMP_DIR}/repo/scripts/lp"
  elif require_cmd curl; then
    if ! curl -fsSL "${LP_CLI_RAW_URL}" -o "${cli_tmp}.raw"; then
      logf install_lp_cli_failed "download ${LP_CLI_RAW_URL}"
      return 0
    fi
    source_cli="${cli_tmp}.raw"
  else
    logf install_lp_cli_failed "curl not found"
    return 0
  fi

  cp "${source_cli}" "${cli_tmp}"
  install_root_escaped="$(escape_sed_replacement "${INSTALL_ROOT}")"
  sed -i "s|__INSTALL_ROOT__|${install_root_escaped}|g" "${cli_tmp}"

  ${SUDO} mkdir -p "/usr/local/bin"
  if ! ${SUDO} install -m 755 "${cli_tmp}" "${cli_target}"; then
    logf install_lp_cli_failed "install ${cli_target}"
    return 0
  fi
  ${SUDO} ln -sf "${cli_target}" "${cli_alias}" || true
  logf install_lp_cli_done "${cli_target}"
}

if [[ "$(uname -s)" != "Linux" ]]; then
  die linux_only
fi

select_language
if [[ "${LANGUAGE}" == "zh" ]]; then
  logf language_selected "中文"
else
  logf language_selected "English"
fi

select_install_mode
if [[ "${INSTALL_MODE}" == "build" ]]; then
  logf install_mode_selected "$(msg mode_build)"
else
  logf install_mode_selected "$(msg mode_binary)"
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

TMP_DIR="$(mktemp -d -t linxpanel-install-XXXXXX)"
trap 'rm -rf "${TMP_DIR}"' EXIT

BIN_SOURCE=""
if [[ "${INSTALL_MODE}" == "binary" ]]; then
  if BIN_SOURCE="$(download_binary_release "${TMP_DIR}")"; then
    :
  else
    if [[ $? -eq 2 ]]; then
      logf release_asset_not_found
    fi
    logf binary_mode_fallback_build
    INSTALL_MODE="build"
  fi
fi

if [[ "${INSTALL_MODE}" == "build" ]]; then
  BIN_SOURCE="$(build_binary_from_source "${TMP_DIR}")"
fi

logf install_files "${INSTALL_ROOT}"
${SUDO} mkdir -p "${INSTALL_ROOT}/bin" "${INSTALL_ROOT}/data" "${INSTALL_ROOT}/logs"
${SUDO} install -m 755 "${BIN_SOURCE}" "${INSTALL_ROOT}/bin/linxpanel"
${SUDO} chown -R "${RUN_USER}:${RUN_GROUP}" "${INSTALL_ROOT}"
install_lp_cli

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
