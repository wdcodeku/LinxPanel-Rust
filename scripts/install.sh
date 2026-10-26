#!/usr/bin/env bash
# LinxPanel one-click installer / upgrader.
#
#   bash -c "$(curl -fsSL https://raw.githubusercontent.com/wdcodeku/LinxPanel-Rust/main/scripts/install.sh)"
#
# Fresh install: downloads a static release binary (seconds, no toolchain), sets a
# random admin password, opens the firewall port, and health-checks the service.
# Re-running it upgrades in place: data is backed up and a failed upgrade rolls back.
set -euo pipefail
# Make `set -e` apply inside $(...) too (bash >= 4.4; older bash relies on explicit checks).
shopt -s inherit_errexit 2>/dev/null || true

GITHUB_REPO="${LINXPANEL_GITHUB_REPO:-wdcodeku/LinxPanel-Rust}"
# Optional prefix for github.com downloads where GitHub is slow (e.g. https://ghfast.top/).
# The proxy sees and serves the files, so only use one you trust.
GITHUB_PROXY="${LINXPANEL_GITHUB_PROXY:-}"
REPO_URL="${LINXPANEL_REPO_URL:-https://github.com/${GITHUB_REPO}.git}"
RELEASE_API_URL="${LINXPANEL_RELEASE_API_URL:-https://api.github.com/repos/${GITHUB_REPO}/releases/latest}"
LP_CLI_RAW_URL="${LINXPANEL_LP_CLI_RAW_URL:-https://raw.githubusercontent.com/${GITHUB_REPO}/main/scripts/lp}"
VERSION="${LINXPANEL_VERSION:-latest}"
# Self-hosted mirror of the release packages (a directory holding
# linxpanel-linux-<arch>.tar.gz and .sha256 files), e.g. on your own CDN.
RELEASE_BASE_URL="${LINXPANEL_RELEASE_BASE_URL:-}"
INSTALL_ROOT="${LINXPANEL_INSTALL_ROOT:-/opt/linxpanel}"
PORT="${LINXPANEL_PORT:-}"
# Whether the port was asked for explicitly (it then overrides one saved in the panel).
PORT_EXPLICIT=0
[[ -n "${PORT}" ]] && PORT_EXPLICIT=1
DEFAULT_PORT=7200
MIN_CARGO_VERSION="${LINXPANEL_MIN_CARGO_VERSION:-1.80.0}"
MIN_NODE_MAJOR=18
NODE_DIST_VERSION="${LINXPANEL_NODE_VERSION:-v20.18.1}"
NODE_MIRROR="${LINXPANEL_NODE_MIRROR:-https://nodejs.org/dist}"
LANGUAGE="${LINXPANEL_LANG:-}"
INSTALL_MODE="${LINXPANEL_INSTALL_MODE:-}"
ADMIN_PASSWORD="${LINXPANEL_ADMIN_PASSWORD:-}"
OPEN_FIREWALL="${LINXPANEL_OPEN_FIREWALL:-1}"
# Security entrance: random (default on fresh installs) | off | /custom-path.
ENTRANCE_MODE="${LINXPANEL_ENTRANCE:-}"
# Panel HTTPS with a self-signed certificate: 1 (default on fresh installs) | 0.
SSL_MODE="${LINXPANEL_SSL:-}"
HEALTH_TIMEOUT="${LINXPANEL_HEALTH_TIMEOUT:-30}"
# Kernel network hardening (SYN cookies, backlog, anti-spoofing): 1 (default) | 0.
HARDEN_KERNEL="${LINXPANEL_HARDEN:-1}"
SERVICE_NAME="linxpanel"
SERVICE_FILE="/etc/systemd/system/${SERVICE_NAME}.service"

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

# Prompts read from /dev/tty so they also work under `curl ... | bash`.
has_tty() {
  [[ -t 0 ]] || { [[ -r /dev/tty ]] && { : </dev/tty; } 2>/dev/null; }
}

prompt() {
  local text="$1"
  local reply=""
  if [[ -t 0 ]]; then
    read -r -p "${text}" reply || true
  else
    read -r -p "${text}" reply </dev/tty || true
  fi
  printf '%s' "${reply}"
}

select_language() {
  if [[ -n "${LANGUAGE}" ]]; then
    LANGUAGE="$(normalize_lang "${LANGUAGE}")"
    return
  fi

  if has_tty; then
    echo "[LinxPanel] Select language / 选择语言:"
    echo "[LinxPanel]   1) English (default)"
    echo "[LinxPanel]   2) 中文"
    LANGUAGE="$(normalize_lang "$(prompt "[LinxPanel] Enter choice [1/2]: ")")"
  else
    LANGUAGE="en"
  fi
}

select_install_mode() {
  if [[ -n "${INSTALL_MODE}" ]]; then
    INSTALL_MODE="$(normalize_install_mode "${INSTALL_MODE}")"
    return
  fi

  if has_tty; then
    if [[ "${LANGUAGE}" == "zh" ]]; then
      echo "[LinxPanel] 选择安装方式："
      echo "[LinxPanel]   1) 二进制安装（默认，速度更快）"
      echo "[LinxPanel]   2) 编译安装（从源码构建）"
      INSTALL_MODE="$(normalize_install_mode "$(prompt "[LinxPanel] 请输入 [1/2]: ")")"
    else
      echo "[LinxPanel] Select installation mode:"
      echo "[LinxPanel]   1) Binary install (default, faster)"
      echo "[LinxPanel]   2) Build from source"
      INSTALL_MODE="$(normalize_install_mode "$(prompt "[LinxPanel] Enter choice [1/2]: ")")"
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
      install_base_deps) echo "正在安装基础依赖：%s" ;;
      harden_kernel) echo "已应用内核网络加固（SYN Cookie、连接队列、防伪造）" ;;
      harden_skipped) echo "内核网络加固未应用（容器或只读内核参数），可稍后在 安全 > 主动防御 中重试" ;;
      unsupported_pkg_manager) echo "不支持的包管理器，请手动安装 git/npm/cargo。" ;;
      cargo_ok) echo "检测到 cargo %s，版本校验通过。" ;;
      cargo_ok_stable) echo "检测到 rustup stable cargo %s，版本校验通过。" ;;
      cargo_old_install) echo "Cargo 缺失或版本过低（需要 >= %s），正在安装 Rust stable 工具链..." ;;
      curl_required) echo "需要 curl 或 wget。" ;;
      rustup_bootstrap_retry) echo "rustup 引导安装未完成，正在重新检测现有 cargo 工具链..." ;;
      using_rustup_stable) echo "使用 rustup stable cargo %s。" ;;
      using_system_cargo) echo "使用系统 cargo %s。" ;;
      cargo_too_old) echo "Cargo 版本仍然过低（%s），请手动升级 Rust 工具链。" ;;
      node_ok) echo "检测到 Node.js %s。" ;;
      node_install) echo "Node.js 缺失或版本过低（需要 >= %s），正在安装 Node.js %s..." ;;
      node_failed) echo "Node.js 安装失败，请手动安装 Node.js >= %s。" ;;
      low_memory) echo "内存不足 2GB，编译将使用单线程以避免内存耗尽（会更慢）。" ;;
      linux_only) echo "当前安装脚本仅支持 Linux。" ;;
      unsupported_arch) echo "不支持的 CPU 架构：%s（仅支持 x86_64 / aarch64）。" ;;
      required_cmd_missing) echo "依赖安装后缺少命令：%s" ;;
      clone_repo) echo "正在克隆仓库..." ;;
      clone_failed) echo "克隆仓库失败：%s" ;;
      build_failed) echo "编译失败，请查看上方输出。" ;;
      build_linxpanel) echo "正在构建 LinxPanel（可能耗时较长）..." ;;
      build_bin_not_found) echo "构建成功但未找到二进制文件：%s" ;;
      fetch_release) echo "正在获取最新发布信息..." ;;
      download_release_asset) echo "正在下载预编译二进制：%s" ;;
      release_download_failed) echo "二进制下载失败：%s" ;;
      release_asset_not_found) echo "未找到匹配当前系统架构的 Linux 预编译包。" ;;
      checksum_ok) echo "SHA256 校验通过。" ;;
      checksum_failed) echo "SHA256 校验失败，文件可能已损坏或被篡改：%s" ;;
      checksum_missing) echo "未找到校验文件，跳过 SHA256 校验。" ;;
      extract_archive) echo "正在解压二进制包..." ;;
      unzip_required) echo "需要 unzip 或 bsdtar 来解压 zip 包。" ;;
      binary_not_found_after_extract) echo "解压后未找到 linxpanel 二进制文件。" ;;
      binary_broken) echo "二进制无法运行（架构不匹配或文件损坏）：%s" ;;
      binary_mode_fallback_build) echo "二进制安装失败，自动回退为编译安装..." ;;
      port_in_use) echo "端口 %s 已被占用，请使用 LINXPANEL_PORT=<端口> 指定其他端口。" ;;
      upgrade_detected) echo "检测到已安装版本 %s，将执行升级（数据会保留）。" ;;
      backup_done) echo "已备份旧版本到 %s" ;;
      install_files) echo "正在安装文件到 %s..." ;;
      set_password) echo "正在生成管理员密码..." ;;
      set_password_failed) echo "设置管理员密码失败，请稍后执行：Lp reset-password" ;;
      install_lp_cli) echo "正在安装 Lp 命令行工具..." ;;
      install_lp_cli_done) echo "Lp 命令可用：%s（别名：lp）" ;;
      install_lp_cli_failed) echo "Lp 安装失败（已跳过）：%s" ;;
      configure_systemd) echo "正在配置 systemd 服务..." ;;
      firewall_opened) echo "已在 %s 中放行端口 %s/tcp。" ;;
      health_wait) echo "正在等待面板启动..." ;;
      health_ok) echo "面板已启动并通过健康检查。" ;;
      health_failed) echo "面板未能在 %s 秒内启动，最近日志：" ;;
      rollback) echo "升级失败，正在回滚到旧版本..." ;;
      rollback_done) echo "已回滚到旧版本。" ;;
      no_systemd) echo "未检测到 systemd，改为 nohup 启动..." ;;
      deploy_complete) echo "部署完成" ;;
      upgrade_complete) echo "升级完成" ;;
      panel_url_public) echo "外网地址：%s" ;;
      panel_url_internal) echo "内网地址：%s" ;;
      panel_user) echo "用户名：  %s" ;;
      panel_password) echo "密码：    %s" ;;
      password_unchanged) echo "密码：    （未修改，沿用原密码）" ;;
      entrance_invalid) echo "安全入口无效，已跳过：%s" ;;
      ssl_failed) echo "启用面板 HTTPS 失败，将继续使用 HTTP。" ;;
      https_notice) echo "面板使用自签证书：浏览器提示「不安全」时，点击「高级」→「继续访问」即可。" ;;
      entrance_notice) echo "必须通过上面带安全入口的完整地址访问，直接访问 IP:端口 会返回 404。忘记入口：Lp entrance" ;;
      entrance_hint) echo "未启用安全入口，建议执行：Lp entrance random" ;;
      password_notice) echo "请立即保存以上密码，它不会再次显示。忘记密码可执行：Lp reset-password" ;;
      cloud_notice) echo "若无法访问，请在云服务器安全组中放行 TCP 端口 %s。" ;;
      lp_hint) echo "管理命令：Lp status | Lp restart | Lp logs | Lp update | Lp help" ;;
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
      install_base_deps) echo "Installing base dependencies: %s" ;;
      harden_kernel) echo "Applied kernel network hardening (SYN cookies, backlog, anti-spoofing)" ;;
      harden_skipped) echo "Kernel network hardening not applied (container or read-only sysctl); retry later under Security > Defense" ;;
      unsupported_pkg_manager) echo "Unsupported package manager. Please install git/npm/cargo manually." ;;
      cargo_ok) echo "Detected cargo %s, version check passed." ;;
      cargo_ok_stable) echo "Detected rustup stable cargo %s, version check passed." ;;
      cargo_old_install) echo "Cargo is missing or too old (need >= %s), installing Rust stable toolchain..." ;;
      curl_required) echo "curl or wget is required." ;;
      rustup_bootstrap_retry) echo "rustup bootstrap did not complete, re-checking existing cargo toolchain..." ;;
      using_rustup_stable) echo "Using rustup stable cargo %s." ;;
      using_system_cargo) echo "Using system cargo %s." ;;
      cargo_too_old) echo "Cargo version is still too old (%s). Please update Rust toolchain manually." ;;
      node_ok) echo "Detected Node.js %s." ;;
      node_install) echo "Node.js is missing or too old (need >= %s), installing Node.js %s..." ;;
      node_failed) echo "Failed to install Node.js. Please install Node.js >= %s manually." ;;
      low_memory) echo "Less than 2GB RAM: compiling single-threaded to avoid running out of memory (slower)." ;;
      linux_only) echo "This installer currently supports Linux only." ;;
      unsupported_arch) echo "Unsupported CPU architecture: %s (x86_64 / aarch64 only)." ;;
      required_cmd_missing) echo "Required command missing after dependency install: %s" ;;
      clone_repo) echo "Cloning repository..." ;;
      clone_failed) echo "Failed to clone repository: %s" ;;
      build_failed) echo "Build failed, see the output above." ;;
      build_linxpanel) echo "Building LinxPanel (this may take a while)..." ;;
      build_bin_not_found) echo "Build succeeded but binary not found: %s" ;;
      fetch_release) echo "Fetching latest release metadata..." ;;
      download_release_asset) echo "Downloading prebuilt binary asset: %s" ;;
      release_download_failed) echo "Failed to download binary asset: %s" ;;
      release_asset_not_found) echo "No Linux prebuilt asset matched current architecture." ;;
      checksum_ok) echo "SHA256 checksum verified." ;;
      checksum_failed) echo "SHA256 mismatch, the download is corrupt or was tampered with: %s" ;;
      checksum_missing) echo "No checksum file published, skipping SHA256 verification." ;;
      extract_archive) echo "Extracting binary package..." ;;
      unzip_required) echo "unzip or bsdtar is required to extract zip package." ;;
      binary_not_found_after_extract) echo "linxpanel binary not found after extraction." ;;
      binary_broken) echo "The binary does not run (wrong architecture or corrupt): %s" ;;
      binary_mode_fallback_build) echo "Binary install failed, falling back to source build..." ;;
      port_in_use) echo "Port %s is already in use. Choose another with LINXPANEL_PORT=<port>." ;;
      upgrade_detected) echo "Found existing installation (%s); upgrading in place, data is kept." ;;
      backup_done) echo "Previous version backed up to %s" ;;
      install_files) echo "Installing files to %s..." ;;
      set_password) echo "Generating admin password..." ;;
      set_password_failed) echo "Could not set the admin password; run later: Lp reset-password" ;;
      install_lp_cli) echo "Installing Lp CLI tool..." ;;
      install_lp_cli_done) echo "Lp command ready: %s (alias: lp)" ;;
      install_lp_cli_failed) echo "Failed to install Lp CLI (skipped): %s" ;;
      configure_systemd) echo "Configuring systemd service..." ;;
      firewall_opened) echo "Opened in %s: port %s/tcp." ;;
      health_wait) echo "Waiting for the panel to start..." ;;
      health_ok) echo "Panel is up and passed the health check." ;;
      health_failed) echo "Panel did not come up within %s seconds. Recent logs:" ;;
      rollback) echo "Upgrade failed, rolling back to the previous version..." ;;
      rollback_done) echo "Rolled back to the previous version." ;;
      no_systemd) echo "systemd not detected, starting with nohup..." ;;
      deploy_complete) echo "Deploy complete" ;;
      upgrade_complete) echo "Upgrade complete" ;;
      panel_url_public) echo "Public URL:   %s" ;;
      panel_url_internal) echo "Internal URL: %s" ;;
      panel_user) echo "Username:     %s" ;;
      panel_password) echo "Password:     %s" ;;
      password_unchanged) echo "Password:     (unchanged)" ;;
      entrance_invalid) echo "Invalid security entrance, skipped: %s" ;;
      ssl_failed) echo "Could not enable panel HTTPS; continuing with HTTP." ;;
      https_notice) echo "Self-signed certificate: when the browser warns, choose Advanced -> Proceed." ;;
      entrance_notice) echo "Open the full URL above including its entrance path; bare IP:port returns 404. Forgot it? Lp entrance" ;;
      entrance_hint) echo "No security entrance is set; consider: Lp entrance random" ;;
      password_notice) echo "Save this password now; it is not shown again. Forgot it? Run: Lp reset-password" ;;
      cloud_notice) echo "Cannot connect? Allow TCP port %s in your cloud provider's security group." ;;
      lp_hint) echo "Manage with: Lp status | Lp restart | Lp logs | Lp update | Lp help" ;;
      *) echo "${key}" ;;
    esac
  fi
}

logf() {
  local key="$1"
  shift
  local template
  template="$(msg "${key}")"
  printf "[LinxPanel] " >&2
  # shellcheck disable=SC2059 # templates come from msg(), not user input
  printf "${template}\n" "$@" >&2
}

die() {
  logf "$@"
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
  [[ "${ip}" =~ ^[0-9a-fA-F:]+$ && "${ip}" == *:* ]]
}

format_host_for_url() {
  local host="$1"
  if is_valid_ipv6 "${host}"; then
    printf '[%s]' "${host}"
  else
    printf '%s' "${host}"
  fi
}

# fetch URL OUTPUT: download with retries via curl, falling back to wget.
fetch() {
  local url="$1"
  local out="$2"
  if require_cmd curl; then
    local progress=(-sS)
    [[ -t 2 ]] && progress=(-#)
    curl -fL "${progress[@]}" --retry 3 --retry-delay 2 --connect-timeout 10 --max-time 900 \
      -o "${out}" "${url}"
  elif require_cmd wget; then
    wget -q --tries=3 --timeout=30 -O "${out}" "${url}"
  else
    die curl_required
  fi
}

# fetch_text URL: print a small response body, empty on failure.
fetch_text() {
  local url="$1"
  if require_cmd curl; then
    curl -fsSL --retry 2 --connect-timeout 5 --max-time 20 "${url}" 2>/dev/null || true
  elif require_cmd wget; then
    wget -qO- --tries=2 --timeout=10 "${url}" 2>/dev/null || true
  fi
}

github_url() {
  printf '%s%s' "${GITHUB_PROXY}" "$1"
}

detect_public_ip() {
  local ip_candidate=""
  local url=""

  if [[ -n "${LINXPANEL_PUBLIC_IP:-}" ]]; then
    printf '%s' "${LINXPANEL_PUBLIC_IP}"
    return
  fi
  if ! require_cmd curl; then
    return
  fi

  for url in https://api.ipify.org https://ipv4.icanhazip.com https://4.ipw.cn; do
    ip_candidate="$(curl -4fsS --connect-timeout 3 --max-time 5 "${url}" 2>/dev/null | tr -d '[:space:]' || true)"
    if is_valid_ipv4 "${ip_candidate}"; then
      printf '%s' "${ip_candidate}"
      return
    fi
  done

  ip_candidate="$(curl -6fsS --connect-timeout 3 --max-time 5 https://api64.ipify.org 2>/dev/null | tr -d '[:space:]' || true)"
  if is_valid_ipv6 "${ip_candidate}"; then
    printf '%s' "${ip_candidate}"
  fi
}

detect_internal_ip() {
  local ip_candidate=""

  if require_cmd ip; then
    ip_candidate="$(ip -4 route get 1.1.1.1 2>/dev/null | awk '{for(i=1;i<=NF;i++){if($i=="src"){print $(i+1); exit}}}' || true)"
    if is_valid_ipv4 "${ip_candidate}"; then
      printf '%s' "${ip_candidate}"
      return
    fi
  fi

  ip_candidate="$(hostname -I 2>/dev/null | tr ' ' '\n' | grep -Ev '^(127\.|$)' | grep -v ':' | head -n1 || true)"
  if [[ -n "${ip_candidate}" ]]; then
    printf '%s' "${ip_candidate}"
    return
  fi

  printf '%s' "127.0.0.1"
}

pkg_install() {
  if require_cmd apt-get; then
    ${SUDO} env DEBIAN_FRONTEND=noninteractive apt-get update -y
    ${SUDO} env DEBIAN_FRONTEND=noninteractive apt-get install -y "$@"
  elif require_cmd dnf; then
    ${SUDO} dnf install -y "$@"
  elif require_cmd yum; then
    ${SUDO} yum install -y "$@"
  elif require_cmd zypper; then
    ${SUDO} zypper --non-interactive install "$@"
  elif require_cmd pacman; then
    ${SUDO} pacman -Sy --noconfirm "$@"
  elif require_cmd apk; then
    ${SUDO} apk add --no-cache "$@"
  else
    return 1
  fi
}

# The binary path only needs a downloader, tar and sha256sum.
ensure_base_deps() {
  local missing=()
  if ! require_cmd curl && ! require_cmd wget; then
    missing+=(curl)
  fi
  require_cmd tar || missing+=(tar)
  require_cmd sha256sum || missing+=(coreutils)
  require_cmd gzip || missing+=(gzip)
  # Active defense bans addresses in the kernel with nftables.
  require_cmd nft || missing+=(nftables)
  # Scheduled jobs and backups run from cron; minimal images ship without it.
  if ! require_cmd crontab; then
    if require_cmd apt-get; then
      missing+=(cron)
    elif require_cmd dnf || require_cmd yum; then
      missing+=(cronie)
    elif require_cmd zypper; then
      missing+=(cron)
    fi
  fi
  if [[ ${#missing[@]} -eq 0 ]]; then
    return
  fi
  logf install_base_deps "${missing[*]}"
  pkg_install ca-certificates "${missing[@]}" || true
  if [[ "${USE_SYSTEMD}" == "1" ]]; then
    ${SUDO} systemctl enable --now cron >/dev/null 2>&1 \
      || ${SUDO} systemctl enable --now crond >/dev/null 2>&1 || true
  fi
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
  logf install_deps
  if require_cmd apt-get; then
    pkg_install curl ca-certificates git build-essential pkg-config xz-utils
  elif require_cmd dnf; then
    pkg_install curl ca-certificates git gcc gcc-c++ make pkgconfig xz
  elif require_cmd yum; then
    pkg_install curl ca-certificates git gcc gcc-c++ make pkgconfig xz
  elif require_cmd zypper; then
    pkg_install curl ca-certificates git gcc gcc-c++ make pkg-config xz
  elif require_cmd pacman; then
    pkg_install curl ca-certificates git base-devel pkgconf xz
  elif require_cmd apk; then
    pkg_install curl ca-certificates git build-base pkgconf xz bash
  else
    die unsupported_pkg_manager
  fi
}

node_major() {
  node --version 2>/dev/null | sed -E 's/^v([0-9]+).*/\1/' || true
}

# Distro Node.js is often too old for Vite, so install an official build when needed.
ensure_modern_node() {
  local major=""
  local node_arch=""
  local archive=""
  major="$(node_major)"
  if [[ -n "${major}" && "${major}" -ge "${MIN_NODE_MAJOR}" ]] && require_cmd npm; then
    logf node_ok "$(node --version)"
    return
  fi

  logf node_install "${MIN_NODE_MAJOR}" "${NODE_DIST_VERSION}"
  case "${ARCH}" in
    x86_64) node_arch="x64" ;;
    aarch64) node_arch="arm64" ;;
  esac
  archive="node-${NODE_DIST_VERSION}-linux-${node_arch}.tar.xz"
  if ! fetch "${NODE_MIRROR}/${NODE_DIST_VERSION}/${archive}" "${TMP_DIR}/${archive}"; then
    die node_failed "${MIN_NODE_MAJOR}"
  fi
  ${SUDO} mkdir -p /usr/local/lib/nodejs
  ${SUDO} tar -xJf "${TMP_DIR}/${archive}" -C /usr/local/lib/nodejs
  export PATH="/usr/local/lib/nodejs/node-${NODE_DIST_VERSION}-linux-${node_arch}/bin:${PATH}"
  major="$(node_major)"
  if [[ -z "${major}" || "${major}" -lt "${MIN_NODE_MAJOR}" ]]; then
    die node_failed "${MIN_NODE_MAJOR}"
  fi
  logf node_ok "$(node --version)"
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
    if ! curl -sSf https://sh.rustup.rs | RUSTUP_INIT_SKIP_PATH_CHECK=yes sh -s -- -y --profile minimal --default-toolchain stable; then
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

detect_arch() {
  local machine
  machine="$(uname -m 2>/dev/null || true)"
  case "${machine}" in
    x86_64 | amd64) echo "x86_64" ;;
    aarch64 | arm64) echo "aarch64" ;;
    # armv8l: a 32-bit userland on a 64-bit ARM kernel runs armv7 binaries.
    armv7* | armv8l | armhf) echo "armv7" ;;
    i386 | i486 | i586 | i686) echo "i686" ;;
    riscv64) echo "riscv64" ;;
    loongarch64) echo "loongarch64" ;;
    ppc64le) echo "ppc64le" ;;
    s390x) echo "s390x" ;;
    *) echo "" ;;
  esac
}

find_binary_in_dir() {
  local search_dir="$1"
  local candidate=""

  candidate="$(find "${search_dir}" -type f -name 'linxpanel' | head -n1 || true)"
  if [[ -z "${candidate}" ]]; then
    candidate="$(find "${search_dir}" -type f \( -name 'linxpanel-*' -o -name 'linxpanel_*' \) \
      ! -name '*.sha256' | head -n1 || true)"
  fi
  if [[ -z "${candidate}" ]]; then
    return 1
  fi

  chmod +x "${candidate}" || true
  printf '%s' "${candidate}"
}

# verify_checksum FILE CHECKSUM_FILE: compare against the first hash in the file.
verify_checksum() {
  local file="$1"
  local sum_file="$2"
  local expected=""
  local actual=""
  expected="$(awk '{print $1; exit}' "${sum_file}" | tr '[:upper:]' '[:lower:]')"
  actual="$(sha256sum "${file}" | awk '{print $1}')"
  if [[ -z "${expected}" || "${expected}" != "${actual}" ]]; then
    logf checksum_failed "$(basename "${file}")"
    return 1
  fi
  logf checksum_ok
}

# Picks an asset URL from the GitHub API, for releases that do not follow the
# linxpanel-linux-<arch>.tar.gz naming the release workflow uses.
find_asset_via_api() {
  local release_json=""
  local asset_list=""
  local selected_url=""
  local asset_lower=""

  logf fetch_release
  release_json="$(fetch_text "${RELEASE_API_URL}")"
  asset_list="$(printf '%s\n' "${release_json}" | grep -Eo '"browser_download_url":[[:space:]]*"[^"]+"' | sed -E 's/.*"([^"]+)"/\1/' || true)"
  while IFS= read -r selected_url; do
    asset_lower="$(printf '%s' "${selected_url}" | tr '[:upper:]' '[:lower:]')"
    if [[ "${asset_lower}" == *linux* && "${asset_lower}" == *"${ARCH}"* && "${asset_lower}" == *linxpanel* \
      && "${asset_lower}" != *sha256* && "${asset_lower}" != *checksums* && "${asset_lower}" != *.sig ]]; then
      printf '%s' "${selected_url}"
      return 0
    fi
  done <<< "${asset_list}"
  return 1
}

download_binary_release() {
  local tmp_dir="$1"
  local asset_name="linxpanel-linux-${ARCH}.tar.gz"
  local base_url=""
  local asset_url=""
  local asset_file=""
  local extract_dir="${tmp_dir}/release-extract"
  local binary_path=""

  if [[ -n "${RELEASE_BASE_URL}" ]]; then
    asset_url="${RELEASE_BASE_URL%/}/${asset_name}"
  else
    if [[ "${VERSION}" == "latest" ]]; then
      base_url="https://github.com/${GITHUB_REPO}/releases/latest/download"
    else
      base_url="https://github.com/${GITHUB_REPO}/releases/download/${VERSION}"
    fi
    asset_url="$(github_url "${base_url}/${asset_name}")"
  fi
  asset_file="${tmp_dir}/${asset_name}"

  logf download_release_asset "${asset_url}"
  if fetch "${asset_url}" "${asset_file}"; then
    if fetch "${asset_url}.sha256" "${asset_file}.sha256" 2>/dev/null; then
      # A mismatch may mean tampering, so it aborts instead of falling back.
      verify_checksum "${asset_file}" "${asset_file}.sha256" || return 4
    else
      logf checksum_missing
    fi
  else
    # Older or hand-made releases: look the asset up by name instead.
    asset_url="$(find_asset_via_api || true)"
    if [[ -z "${asset_url}" ]]; then
      logf release_asset_not_found
      return 2
    fi
    asset_file="${tmp_dir}/$(basename "${asset_url}")"
    logf download_release_asset "${asset_url}"
    if ! fetch "$(github_url "${asset_url}")" "${asset_file}"; then
      logf release_download_failed "${asset_url}"
      return 3
    fi
  fi

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
      cp "${asset_file}" "${extract_dir}/linxpanel"
      ;;
  esac

  if ! binary_path="$(find_binary_in_dir "${extract_dir}")"; then
    logf binary_not_found_after_extract
    return 3
  fi
  printf '%s' "${binary_path}"
}

build_binary_from_source() {
  local tmp_dir="$1"
  local bin_source=""
  local mem_kb=""

  # Only the binary path may reach stdout: callers capture it with $(...).
  {
    install_deps
    ensure_modern_node
    ensure_modern_cargo
  } >&2

  for cmd in git npm cargo bash; do
    if ! require_cmd "${cmd}"; then
      die required_cmd_missing "${cmd}"
    fi
  done

  # rustc needs ~1GB per job; small VPSes get OOM-killed with parallel jobs.
  mem_kb="$(awk '/MemTotal/ {print $2}' /proc/meminfo 2>/dev/null || echo 0)"
  if [[ -z "${CARGO_BUILD_JOBS:-}" && "${mem_kb}" -gt 0 && "${mem_kb}" -lt 2000000 ]]; then
    logf low_memory
    export CARGO_BUILD_JOBS=1
  fi

  logf clone_repo
  git clone --depth 1 "$(github_url "${REPO_URL}")" "${tmp_dir}/repo" >&2 \
    || die clone_failed "${REPO_URL}"

  logf build_linxpanel
  LINXPANEL_LANG="${LANGUAGE}" bash "${tmp_dir}/repo/scripts/build-single.sh" >&2 \
    || die build_failed

  bin_source="${tmp_dir}/repo/target/release/linxpanel"
  if [[ ! -f "${bin_source}" ]]; then
    die build_bin_not_found "${bin_source}"
  fi

  printf '%s' "${bin_source}"
}

# bin_version BIN: prints the version, bounded by a timeout because binaries
# older than the `--version` flag ignore it and start the server instead.
bin_version() {
  if require_cmd timeout; then
    timeout 5 "$1" --version 2>/dev/null | awk '/^linxpanel / {print $2}'
  else
    "$1" --version 2>/dev/null | awk '/^linxpanel / {print $2}'
  fi
}

port_in_use() {
  local port="$1"
  if require_cmd ss; then
    ss -Htln 2>/dev/null | awk '{print $4}' | grep -Eq "[:.]${port}$"
  elif require_cmd netstat; then
    netstat -tln 2>/dev/null | awk '{print $4}' | grep -Eq "[:.]${port}$"
  else
    return 1
  fi
}

# Port of an existing install, read back from its systemd unit or env file.
existing_port() {
  local port=""
  if [[ -f "${SERVICE_FILE}" ]]; then
    port="$(sed -n -E 's/^Environment=PORT=([0-9]+)$/\1/p' "${SERVICE_FILE}" | head -n1)"
  fi
  if [[ -z "${port}" && -f "${INSTALL_ROOT}/linxpanel.env" ]]; then
    port="$(sed -n -E 's/^PORT=([0-9]+)$/\1/p' "${INSTALL_ROOT}/linxpanel.env" | head -n1)"
  fi
  printf '%s' "${port}"
}

# Saved for nohup mode and `Lp`, which have no unit file to read settings from.
write_env_file() {
  ${SUDO} tee "${INSTALL_ROOT}/linxpanel.env" >/dev/null <<EOF
PORT=${PORT}
DATABASE_URL=sqlite:${INSTALL_ROOT}/data/linxpanel.db?mode=rwc
EOF
}

stop_panel() {
  if require_cmd systemctl && [[ -f "${SERVICE_FILE}" ]]; then
    ${SUDO} systemctl stop "${SERVICE_NAME}" >/dev/null 2>&1 || true
  fi
  if require_cmd pkill; then
    ${SUDO} pkill -TERM -f "^${INSTALL_ROOT}/bin/linxpanel( |$)" >/dev/null 2>&1 || true
    sleep 1
  fi
}

backup_existing() {
  local stamp
  stamp="$(date +%Y%m%d-%H%M%S)"
  BACKUP_DIR="${INSTALL_ROOT}/backup/${stamp}"
  ${SUDO} mkdir -p "${BACKUP_DIR}"
  ${SUDO} cp -a "${INSTALL_ROOT}/bin/linxpanel" "${BACKUP_DIR}/linxpanel"
  # The panel is stopped, so the SQLite files (incl. WAL) are consistent.
  local db_file=""
  for db_file in "${INSTALL_ROOT}"/data/linxpanel.db*; do
    [[ -f "${db_file}" ]] && ${SUDO} cp -a "${db_file}" "${BACKUP_DIR}/"
  done
  # Keep the five most recent backups.
  # shellcheck disable=SC2012 # backup names are timestamps we created
  ls -1d "${INSTALL_ROOT}"/backup/*/ 2>/dev/null | sort -r | tail -n +6 | while IFS= read -r old; do
    ${SUDO} rm -rf "${old}"
  done
  logf backup_done "${BACKUP_DIR}"
}

# Runs the panel binary's CLI against the installed database.
panel_cli() {
  ${SUDO} env \
    DATABASE_URL="sqlite:${INSTALL_ROOT}/data/linxpanel.db?mode=rwc" \
    RUST_LOG=error \
    "${INSTALL_ROOT}/bin/linxpanel" "$@"
}

set_admin_password() {
  logf set_password
  if [[ -z "${ADMIN_PASSWORD}" ]]; then
    ADMIN_PASSWORD="$(LC_ALL=C tr -dc 'abcdefghijkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789' </dev/urandom | head -c 16 || true)"
  fi
  if ! printf '%s\n' "${ADMIN_PASSWORD}" | panel_cli reset-password --password-stdin >/dev/null; then
    logf set_password_failed
    ADMIN_PASSWORD=""
  fi
}

# Security entrance and panel HTTPS: on by default for fresh installs; upgrades
# keep their settings unless LINXPANEL_ENTRANCE / LINXPANEL_SSL are given.
configure_access() {
  local entrance="${ENTRANCE_MODE}"
  local ssl="${SSL_MODE}"
  local sans=()
  if [[ "${IS_UPGRADE}" == "0" ]]; then
    entrance="${entrance:-random}"
    ssl="${ssl:-1}"
  fi

  case "${entrance}" in
    "") ;;
    random) panel_cli entrance random >/dev/null || logf entrance_invalid "${entrance}" ;;
    off | 0) panel_cli entrance off >/dev/null || true ;;
    *) panel_cli entrance set "${entrance}" >/dev/null || logf entrance_invalid "${entrance}" ;;
  esac

  case "${ssl}" in
    "") ;;
    0 | off) panel_cli ssl off >/dev/null || true ;;
    *)
      # Cover the addresses the panel will be opened at.
      [[ -n "${PUBLIC_IP}" ]] && sans+=(--san "${PUBLIC_IP}")
      [[ -n "${INTERNAL_IP}" ]] && sans+=(--san "${INTERNAL_IP}")
      panel_cli ssl on ${sans[@]+"${sans[@]}"} >/dev/null || logf ssl_failed
      ;;
  esac
}

# A port changed in the panel is stored in its database and wins over the
# service's PORT; keep the unit, env file, health check and summary in step.
sync_port_with_panel() {
  local saved=""
  saved="$(panel_info listen_port)"
  if [[ "${PORT_EXPLICIT}" == "1" ]]; then
    if [[ -n "${saved}" && "${saved}" != "${PORT}" ]]; then
      panel_cli port "${PORT}" >/dev/null || true
    fi
  elif [[ -n "${saved}" ]]; then
    PORT="${saved}"
  fi
}

# Reads a key from `linxpanel info` (scheme, entrance, version, listen_port, domain).
panel_info() {
  panel_cli info 2>/dev/null | sed -n -E "s/^$1=(.*)$/\1/p" | head -n1 || true
}

install_lp_cli() {
  local source_cli=""
  local cli_tmp="${TMP_DIR}/lp"
  local cli_target="/usr/local/bin/Lp"
  local cli_alias="/usr/local/bin/lp"

  logf install_lp_cli

  # Prefer the copy shipped with the binary we just installed, so the two match.
  for source_cli in "${TMP_DIR}/release-extract/lp" "${TMP_DIR}/repo/scripts/lp" ""; do
    [[ -n "${source_cli}" && -f "${source_cli}" ]] && break
  done
  if [[ -z "${source_cli}" ]]; then
    source_cli="${TMP_DIR}/lp.raw"
    if ! fetch "$(github_url "${LP_CLI_RAW_URL}")" "${source_cli}" 2>/dev/null; then
      logf install_lp_cli_failed "download ${LP_CLI_RAW_URL}"
      return 0
    fi
  fi

  sed "s|__INSTALL_ROOT__|${INSTALL_ROOT//|/\\|}|g" "${source_cli}" > "${cli_tmp}"

  ${SUDO} mkdir -p "/usr/local/bin"
  if ! ${SUDO} install -m 755 "${cli_tmp}" "${cli_target}"; then
    logf install_lp_cli_failed "install ${cli_target}"
    return 0
  fi
  ${SUDO} ln -sf "${cli_target}" "${cli_alias}" || true
  logf install_lp_cli_done "${cli_target}"
}

systemd_version() {
  systemctl --version 2>/dev/null | awk 'NR==1 {print $2}' | grep -Eo '^[0-9]+' || echo 0
}

write_systemd_unit() {
  local output_lines=""
  # `append:` needs systemd >= 240 (not on CentOS 7 / Ubuntu 18.04); older ones log to journald.
  if [[ "$(systemd_version)" -ge 240 ]]; then
    output_lines="StandardOutput=append:${INSTALL_ROOT}/logs/linxpanel.out.log
StandardError=append:${INSTALL_ROOT}/logs/linxpanel.err.log"
  fi

  logf configure_systemd
  ${SUDO} tee "${SERVICE_FILE}" >/dev/null <<EOF
[Unit]
Description=LinxPanel Server Management Panel
After=network-online.target
Wants=network-online.target

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
# The panel stops its managed apps itself on SIGTERM; then systemd reaps the rest.
KillMode=mixed
TimeoutStopSec=20
LimitNOFILE=1048576
# Under attack the panel is how you fight back: keep it schedulable when the
# CPU is saturated, and let the OOM killer pick other processes first.
Nice=-5
OOMScoreAdjust=-500
${output_lines}

[Install]
WantedBy=multi-user.target
EOF
  ${SUDO} systemctl daemon-reload
  ${SUDO} systemctl enable "${SERVICE_NAME}" >/dev/null 2>&1
}

write_logrotate() {
  if [[ ! -d /etc/logrotate.d ]]; then
    return
  fi
  ${SUDO} tee /etc/logrotate.d/linxpanel >/dev/null <<EOF
${INSTALL_ROOT}/logs/*.log {
    weekly
    rotate 4
    maxsize 50M
    compress
    missingok
    notifempty
    copytruncate
}

# Per-site access logs written by nginx (read by the auto-ban engine). Under
# attack they grow fast, so rotate by size as well.
${INSTALL_ROOT}/logs/web/*.log {
    daily
    rotate 7
    maxsize 100M
    compress
    delaycompress
    missingok
    notifempty
    copytruncate
}
EOF
}

harden_kernel() {
  if [[ "${HARDEN_KERNEL}" != "1" ]]; then
    return
  fi
  # On upgrades, respect an earlier `Lp harden off`: only refresh a profile
  # that is still installed.
  if [[ "${IS_UPGRADE}" == "1" && ! -f /etc/sysctl.d/90-linxpanel.conf ]]; then
    return
  fi
  if ${SUDO} "${INSTALL_ROOT}/bin/linxpanel" harden on >/dev/null 2>&1; then
    logf harden_kernel
  else
    logf harden_skipped
  fi
}

start_panel() {
  if [[ "${USE_SYSTEMD}" == "1" ]]; then
    ${SUDO} systemctl restart "${SERVICE_NAME}"
  else
    logf no_systemd
    ${SUDO} env \
      PORT="${PORT}" \
      DATABASE_URL="sqlite:${INSTALL_ROOT}/data/linxpanel.db?mode=rwc" \
      nohup "${INSTALL_ROOT}/bin/linxpanel" \
      >> "${INSTALL_ROOT}/logs/linxpanel.out.log" \
      2>> "${INSTALL_ROOT}/logs/linxpanel.err.log" &
  fi
}

# Whether the panel answers locally. /api/health bypasses the security entrance;
# `/` covers versions that predate it (a rollback target).
probe_panel() {
  local url=""
  for url in "https://127.0.0.1:${PORT}/api/health" "http://127.0.0.1:${PORT}/api/health" \
    "http://127.0.0.1:${PORT}/"; do
    if require_cmd curl; then
      curl -kfsS --connect-timeout 2 --max-time 4 -o /dev/null "${url}" 2>/dev/null && return 0
    elif require_cmd wget; then
      wget -q --no-check-certificate -T 4 -O /dev/null "${url}" 2>/dev/null && return 0
    fi
  done
  return 1
}

health_check() {
  local waited=0
  logf health_wait
  while [[ "${waited}" -lt "${HEALTH_TIMEOUT}" ]]; do
    if probe_panel; then
      logf health_ok
      return 0
    fi
    sleep 1
    waited=$((waited + 1))
  done
  return 1
}

show_recent_logs() {
  if [[ -f "${INSTALL_ROOT}/logs/linxpanel.err.log" ]]; then
    ${SUDO} tail -n 20 "${INSTALL_ROOT}/logs/linxpanel.err.log" >&2 || true
  fi
  if [[ "${USE_SYSTEMD}" == "1" ]] && require_cmd journalctl; then
    ${SUDO} journalctl -u "${SERVICE_NAME}" -n 20 --no-pager >&2 || true
  fi
}

open_firewall_port() {
  if [[ "${OPEN_FIREWALL}" != "1" ]]; then
    return
  fi
  if require_cmd ufw && ${SUDO} ufw status 2>/dev/null | grep -q "Status: active"; then
    ${SUDO} ufw allow "${PORT}/tcp" >/dev/null 2>&1 && logf firewall_opened "ufw" "${PORT}"
  elif require_cmd firewall-cmd && ${SUDO} firewall-cmd --state >/dev/null 2>&1; then
    ${SUDO} firewall-cmd --permanent --add-port="${PORT}/tcp" >/dev/null 2>&1 \
      && ${SUDO} firewall-cmd --reload >/dev/null 2>&1 \
      && logf firewall_opened "firewalld" "${PORT}"
  fi
}

print_summary() {
  local title="deploy_complete"
  local scheme=""
  local entrance=""
  [[ "${IS_UPGRADE}" == "1" ]] && title="upgrade_complete"
  local domain=""
  local user=""
  scheme="$(panel_info scheme)"
  entrance="$(panel_info entrance)"
  domain="$(panel_info domain)"
  user="$(panel_info admin_user)"
  scheme="${scheme:-http}"

  {
    echo
    echo "=================================================================="
    printf ' LinxPanel %s — %s\n' "$(bin_version "${INSTALL_ROOT}/bin/linxpanel")" "$(msg "${title}")"
    echo "=================================================================="
    if [[ -n "${domain}" ]]; then
      # A bound domain is the only host the panel answers to.
      # shellcheck disable=SC2059
      printf " $(msg panel_url_public)\n" "${scheme}://${domain}:${PORT}${entrance}"
    else
      if [[ -n "${PUBLIC_IP}" ]]; then
        # shellcheck disable=SC2059
        printf " $(msg panel_url_public)\n" "${scheme}://$(format_host_for_url "${PUBLIC_IP}"):${PORT}${entrance}"
      fi
      # shellcheck disable=SC2059
      printf " $(msg panel_url_internal)\n" "${scheme}://$(format_host_for_url "${INTERNAL_IP}"):${PORT}${entrance}"
    fi
    # shellcheck disable=SC2059
    printf " $(msg panel_user)\n" "${user:-admin}"
    if [[ -n "${ADMIN_PASSWORD}" ]]; then
      # shellcheck disable=SC2059
      printf " $(msg panel_password)\n" "${ADMIN_PASSWORD}"
    else
      echo " $(msg password_unchanged)"
    fi
    echo "------------------------------------------------------------------"
    [[ -n "${ADMIN_PASSWORD}" ]] && echo " $(msg password_notice)"
    if [[ -n "${entrance}" ]]; then
      echo " $(msg entrance_notice)"
    else
      echo " $(msg entrance_hint)"
    fi
    [[ "${scheme}" == "https" ]] && echo " $(msg https_notice)"
    # shellcheck disable=SC2059
    printf " $(msg cloud_notice)\n" "${PORT}"
    echo " $(msg lp_hint)"
    echo "=================================================================="
  } >&2
}

# ---------------------------------------------------------------------------

if [[ "$(uname -s)" != "Linux" ]]; then
  die linux_only
fi

select_language
if [[ "${LANGUAGE}" == "zh" ]]; then
  logf language_selected "中文"
else
  logf language_selected "English"
fi

ARCH="$(detect_arch)"
if [[ -z "${ARCH}" ]]; then
  die unsupported_arch "$(uname -m)"
fi

if [[ "$(id -u)" -eq 0 ]]; then
  SUDO=""
  INVOKING_USER="${SUDO_USER:-root}"
else
  if ! command -v sudo >/dev/null 2>&1; then
    die sudo_not_found
  fi
  SUDO="sudo"
  INVOKING_USER="${USER:-$(id -un)}"
fi

# The panel installs packages, manages services and writes /etc, so it runs as
# root (as BaoTa and 1Panel do). Running it as the user who invoked sudo made
# every install depend on passwordless sudo and fail on most servers.
RUN_USER="root"
RUN_GROUP="root"
# Only for finding a Rust toolchain in build mode.
RUN_HOME="$(getent passwd "${INVOKING_USER}" 2>/dev/null | cut -d: -f6 || true)"
if [[ -z "${RUN_HOME}" ]]; then
  RUN_HOME="${HOME}"
fi
export PATH="${RUN_HOME}/.cargo/bin:${HOME}/.cargo/bin:${PATH}"

USE_SYSTEMD=0
if require_cmd systemctl && [[ -d /run/systemd/system ]]; then
  USE_SYSTEMD=1
fi

IS_UPGRADE=0
if [[ -x "${INSTALL_ROOT}/bin/linxpanel" ]]; then
  IS_UPGRADE=1
  logf upgrade_detected "$(bin_version "${INSTALL_ROOT}/bin/linxpanel" || true)"
  # An upgrade keeps the port it was installed with unless one is given explicitly.
  PORT="${PORT:-$(existing_port)}"
fi
PORT="${PORT:-${DEFAULT_PORT}}"

if [[ "${IS_UPGRADE}" == "0" ]] && port_in_use "${PORT}"; then
  die port_in_use "${PORT}"
fi

select_install_mode
if [[ "${INSTALL_MODE}" == "build" ]]; then
  logf install_mode_selected "$(msg mode_build)"
else
  logf install_mode_selected "$(msg mode_binary)"
fi

TMP_DIR="$(mktemp -d -t linxpanel-install-XXXXXX)"
trap 'rm -rf "${TMP_DIR}"' EXIT
BACKUP_DIR=""

ensure_base_deps

BIN_SOURCE=""
if [[ "${INSTALL_MODE}" == "binary" ]]; then
  download_rc=0
  BIN_SOURCE="$(download_binary_release "${TMP_DIR}")" || download_rc=$?
  if [[ "${download_rc}" -eq 4 ]]; then
    exit 1
  fi
  if [[ "${download_rc}" -ne 0 || -z "$(bin_version "${BIN_SOURCE}")" ]]; then
    [[ "${download_rc}" -eq 0 ]] && logf binary_broken "${BIN_SOURCE}"
    logf binary_mode_fallback_build
    INSTALL_MODE="build"
  fi
fi

if [[ "${INSTALL_MODE}" == "build" ]]; then
  BIN_SOURCE="$(build_binary_from_source "${TMP_DIR}")"
  if [[ -z "$(bin_version "${BIN_SOURCE}")" ]]; then
    die binary_broken "${BIN_SOURCE}"
  fi
fi

if [[ "${IS_UPGRADE}" == "1" ]]; then
  stop_panel
  backup_existing
fi

logf install_files "${INSTALL_ROOT}"
${SUDO} mkdir -p "${INSTALL_ROOT}/bin" "${INSTALL_ROOT}/data" "${INSTALL_ROOT}/logs"
${SUDO} install -m 755 "${BIN_SOURCE}" "${INSTALL_ROOT}/bin/linxpanel.new"
${SUDO} mv -f "${INSTALL_ROOT}/bin/linxpanel.new" "${INSTALL_ROOT}/bin/linxpanel"

# Never leave a fresh install on the published default password.
if [[ "${IS_UPGRADE}" == "0" || -n "${ADMIN_PASSWORD}" ]]; then
  set_admin_password
fi
PUBLIC_IP="$(detect_public_ip)"
INTERNAL_IP="$(detect_internal_ip)"
configure_access
sync_port_with_panel

${SUDO} chown -R "${RUN_USER}:${RUN_GROUP}" "${INSTALL_ROOT}"
${SUDO} chmod 700 "${INSTALL_ROOT}/data"

write_env_file
write_logrotate
harden_kernel
if [[ "${USE_SYSTEMD}" == "1" ]]; then
  write_systemd_unit
fi

start_panel
if ! health_check; then
  logf health_failed "${HEALTH_TIMEOUT}"
  show_recent_logs
  if [[ "${IS_UPGRADE}" == "1" && -n "${BACKUP_DIR}" ]]; then
    logf rollback
    stop_panel
    ${SUDO} install -m 755 "${BACKUP_DIR}/linxpanel" "${INSTALL_ROOT}/bin/linxpanel"
    for db_file in "${BACKUP_DIR}"/linxpanel.db*; do
      [[ -f "${db_file}" ]] && ${SUDO} cp -a "${db_file}" "${INSTALL_ROOT}/data/"
    done
    start_panel
    health_check && logf rollback_done
  fi
  exit 1
fi

# Only now: after a rollback, Lp must keep matching the binary that is running.
install_lp_cli
open_firewall_port
print_summary
